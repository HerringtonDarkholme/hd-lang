// Initialized REPL sessions, checkpoint images and historical replay.
// Kept outside core.mts so ordinary programs do not load the journal.
import { clearOperations, createCoreHost } from "./core.mts";
import { METHODS } from "./gen/abi.mts";
import { HostContract } from "./wire.mts";
import {
  Recorder,
  Replay,
  SESSION_CAP,
  callArgs,
  importTypes,
  joinJournals,
  ran as ranText,
  recorded,
} from "./journal.mts";
// --- REPL sessions ------------------------------------------------------

// Which journal row a call of an import writes (`Kind::record` in
// hd_run_wasmtime): none for `hd:prim`, which answers from its arguments
// alone, or `hd:rt` `fact`, which only marks where a facts body runs, so a
// replay makes them again.
function recordOf(module, name) {
  if (module === "hd:prim" || (module === "hd:rt" && name === "fact")) {
    return "skip";
  }
  if (module === "hd:rt" && (name === "block" || name === "abort")) {
    return name;
  }
  return name.endsWith(".finish") ? "finish" : "call";
}

// The parameter that holds the length of the exchange buffer bytes a call
// reads, with its scalar parameters the call's request; null for none
// (`Kind::input`). A host method's is after its scalar parameters, when
// it takes a structured one (gen/abi.mts `METHODS`).
function readsArgs(module, name) {
  if (module === "hd:rt") {
    return name === "stderr" || name === "stdout" ? 0 : null;
  }
  const starts = name.endsWith(".start");
  const m = METHODS[module]?.[starts ? name.slice(0, -".start".length) : name];
  if (!m || (m.wait === "may") !== starts) {
    return null;
  }
  const scalars = m.params.filter((p) => typeof p === "string").length;
  return scalars < m.params.length ? scalars : null;
}

// A reply value as a Wasm result of type `type`: a narrower integer keeps
// its low bits.
function resultOf(v, type) {
  if (type === "i64") {
    return BigInt.asIntN(64, BigInt(v));
  }
  if (type === "f64" || type === "f32") {
    return Number(v);
  }
  return Number(BigInt.asIntN(32, BigInt(v)));
}

// A host import's reply as the journal holds it, from the JS value it
// returned and its result types.
function replyOf(ret, results) {
  if (ret === undefined) {
    return { tag: 0 };
  }
  if (Array.isArray(ret)) {
    return { tag: 3, v: Number(ret[0]) | 0, w: Number(ret[1]) | 0 };
  }
  if (results[0] === "f64" || results[0] === "f32") {
    const dv = new DataView(new ArrayBuffer(8));
    dv.setFloat64(0, Number(ret), true);
    return { tag: 2, v: dv.getBigUint64(0, true) };
  }
  return { tag: 1, v: typeof ret === "bigint" ? ret : BigInt(Math.trunc(Number(ret))) };
}

// The JS value of a served reply, for an import with result types
// `results`.
function servedValue(value, results) {
  switch (value.tag) {
    case 0:
      return undefined;
    case 1:
      return resultOf(value.v, results[0]);
    case 2: {
      const dv = new DataView(new ArrayBuffer(8));
      dv.setBigUint64(0, value.v, true);
      return dv.getFloat64(0, true);
    }
    default:
      return [resultOf(value.v, results[0]), resultOf(value.w, results[1])];
  }
}

// The handle a `.finish` or `abort` names; no live one is negative.
const handleParam = (args) => {
  const h = Number(args[0]);
  return Number.isInteger(h) && h >= 0 && h <= 0xffffffff ? h : 0xffffffff;
};

// One provider serves every input. Completed code retains initialized
// storage. Checkpoints save memory/globals while archived host results
// remain available for a historical rebuild.
class ReplSession {
  constructor(sink, env, layer, options, createCoreHost, clearOperations) {
    this.clearOperations = clearOperations;
    this.muted = false;
    this.host = createCoreHost(
      {
        out: (text) => this.muted || sink.out(text),
        err: (text) => this.muted || sink.err(text),
      },
      env,
      layer,
    );

    this.st = this.host.core.st;
    this.put = this.host.core.put;
    this.deadlock = this.host.core.deadlock;
    this.flush = this.host.core.flush;
    this.execution = crypto.getRandomValues(new Uint8Array(16));
    this.recorder = new Recorder(options.journalCap ?? SESSION_CAP, this.execution);
    this.replay = null;
    // The host calls made, not served from a record.

    this.calls = 0;
    // A stop request, the wait it ends, and how it ended the input.

    this.stopped = false;
    this.wakeWait = null;
    this.halt = null;
    this.sites = new Map();
    this.driver = {
      poll: () => {
        this.recorder.poll();
        this.replay?.poll();
      },
      replaying: () => this.replay !== null,
      served: () => {
        // A replayed input waits when its record wakes it next.
        if (!this.replay.wakeNext()) {
          this.deadlock();
        }
        const handles = this.replay.wake();
        if (handles.length) {
          this.put(this.handleBytes(handles));
        }
        return handles.length;
      },
      woke: (n) => this.recorder.wake(this.handlesAt(n)),
      wait: (ms) =>
        new Promise((resolve) => {
          if (this.stopped) {
            resolve();
            return;
          }
          const timer = setTimeout(resolve, ms);
          this.wakeWait = () => {
            clearTimeout(timer);
            resolve();
          };
        }).finally(() => {
          this.wakeWait = null;
        }),
      stopping: (cancels) => {
        if (!this.stopped) {
          return null;
        }
        this.stopped = false;
        if (!cancels) {
          this.halt = { kind: "stopped" };
          return "stopped";
        }
        this.recorder.cancel();
        this.halt = { kind: "cancelled" };
        return "cancelled";
      },
    };

    this.linked = null;
  }

  handlesAt = (n) => {
    const words = new DataView(this.st.memory.buffer);
    return Array.from({ length: n }, (_, i) => words.getUint32(4 * i, true));
  };

  handleBytes = (handles) => {
    const out = new Uint8Array(4 * handles.length);
    handles.forEach((h, i) => new DataView(out.buffer).setUint32(4 * i, h, true));
    return out;
  };
  // A call's arguments as the journal holds them: its scalar parameters,
  // then the exchange buffer bytes it reads.
  requestOf = (args, params, reads) => {
    if (reads === null) {
      return callArgs(args, params, null);
    }
    const len = Number(args[reads]);
    if (!(len >= 0 && len <= this.st.memory.buffer.byteLength)) {
      throw new HostContract("an argument length is past the exchange buffer");
    }
    return callArgs(args, params, new Uint8Array(this.st.memory.buffer, 0, len));
  };
  // Answers a call of a replayed input from its record; a call the record
  // does not hold next throws `Diverged`, which ends the input.
  serve = (record, module, name, args, request) => {
    switch (record) {
      case "call":
        return this.replay.call(module, name, request);
      case "finish":
        return this.replay.finish(handleParam(args));
      case "abort":
        this.replay.abort(handleParam(args));
        return { value: { tag: 0 }, result: null };
      default: {
        const handles = this.replay.block();
        // The first run's block found nothing pending either.
        if (handles === null) {
          this.deadlock();
        }
        const result = handles.length ? this.handleBytes(handles) : null;
        return { value: { tag: 1, v: BigInt(handles.length) }, result };
      }
    }
  };
  // The import `module` `name` of type `type`, as a session calls it: the
  // recorder and the replayer sit here, where every host call is made.
  site = (module, name, f, type) => {
    const record = recordOf(module, name);
    if (record === "skip") {
      return f;
    }
    const reads = readsArgs(module, name);
    const params = type?.params ?? [];
    const results = type?.results ?? [];
    // A replayed call is recorded again, so the new journal holds it.
    const write = (args, request, value, result) => {
      if (record === "call") {
        this.recorder.call(module, name, request, value, result);
      } else if (record === "finish") {
        this.recorder.finish(handleParam(args), value, result);
      } else if (record === "abort") {
        this.recorder.abort(handleParam(args));
      } else {
        this.recorder.block(this.handlesAt(Number(value.v)));
      }
    };
    return (...args) => {
      const request = record === "call" ? this.requestOf(args, params, reads) : null;
      if (this.replay) {
        const { value, result } = this.serve(record, module, name, args, request);
        if (result) {
          this.put(result);
        }
        write(args, request, value, result);
        return servedValue(value, results);
      }
      this.calls += 1;
      this.st.payload = null;
      const ret = f(...args);
      const value = replyOf(ret, results);
      write(args, request, value, this.st.payload);
      return ret;
    };
  };

  // Drops pending host operations and clears the runtime state left by
  // an input that did not end with status 0.
  reset = (instance) => {
    this.clearOperations(this.st);
    instance.exports["hd.reset"]?.();
  };

  // Links the session program: its host imports through their call
  // sites. Throws for a host import this host does not have.
  link = (module, wasm, sessionImports = null) => {
    const types = importTypes(wasm);
    const object = {};
    for (const { module: m, name, kind } of WebAssembly.Module.imports(module)) {
      if (sessionImports && Object.hasOwn(sessionImports, m)) {
        const value = sessionImports[m][name];
        if (value === undefined) {
          throw new Error(`the session import ${m} ${name} is missing`);
        }
        (object[m] ??= {})[name] = value;
        continue;
      }
      const f = this.host.imports[m]?.[name];
      if (kind !== "function" || typeof f !== "function") {
        throw new Error(`the import \`${m}\` \`${name}\` is not in this host`);
      }
      const key = `${m}\0${name}`;
      if (!this.sites.has(key)) {
        this.sites.set(key, this.site(m, name, f, types.get(key)));
      }
      object[m] ??= {};
      object[m][name] = this.sites.get(key);
    }
    return object;
  };

  // Runs input `number` of the text `source` from `instance`'s exports
  // `hd.init.N` and `hd.poll.N`, recording it.
  runInput = async (instance, number, source, wasm) => {
    this.halt = null;
    // A panic or standard error of an earlier input does not hide this
    // one's internal error.
    this.st.reported = false;
    this.st.panic = null;
    this.recorder.input(number, source, wasm);
    let outcome;
    try {
      const { status, trapped } = await this.host.run(
        instance,
        `hd.init.${number}`,
        `hd.poll.${number}`,
        this.driver,
      );
      const end = this.halt ?? (trapped ? { kind: "trapped" } : { kind: "exit", status });
      outcome = { status, end, joined: status === 0 && !trapped, error: null };
    } catch (e) {
      this.flush();
      outcome = { status: null, end: { kind: "failed" }, joined: false };
      outcome.error = String(e?.message ?? e);
    }
    if (!outcome.joined) {
      this.reset(instance);
    }
    this.recorder.end(outcome.end);
    return outcome;
  };

  // Starts the instance again with a journal that starts as `journal`,
  // over the same host.
  restart = (journal) => {
    this.flush();
    this.clearOperations(this.st);
    this.st.reported = false;
    this.st.panic = null;
    this.st.memory = null;
    this.st.facts = [];
    this.recorder = journal;
    this.replay = null;
    this.muted = false;
  };

  // Runs `input` again from `instance`, of the text `source`, with its host
  // calls and wakes served from its record and its output dropped. Throws
  // the divergence: it asked for what the record does not hold, or ended
  // otherwise.
  replayInput = async (instance, input, source, wasm) => {
    this.replay = new Replay(input);
    this.muted = true;
    let outcome;
    try {
      outcome = await this.runInput(instance, input.number, source, wasm);
    } finally {
      this.muted = false;
    }
    const served = this.replay;
    this.replay = null;
    served.end(outcome.end, input.end);
  };

  // A linked compiler module carries a private, versioned storage manifest.
  // Ordinary whole-program/replay modules never enter this path.
  sessionInfo = (module) => {
    const sections = WebAssembly.Module.customSections(module, "hd.session");
    if (sections.length === 0) {
      return null;
    }
    if (sections.length !== 1) {
      throw new Error("duplicate session manifests");
    }
    const bytes = sections[0];
    if (bytes.byteLength < 20) {
      throw new Error("truncated session manifest");
    }
    const view = new DataView(bytes);
    const relocations = view.getUint32(16, true);
    if (
      view.getUint32(0, true) !== 1 ||
      relocations > (bytes.byteLength - 20) / 4 ||
      bytes.byteLength !== 20 + relocations * 4
    ) {
      throw new Error("unsupported session manifest");
    }
    const dataBytes = view.getUint32(8, true);
    const offsets = Array.from({ length: relocations }, (_, i) => view.getUint32(20 + i * 4, true));
    if (offsets.some((offset) => offset > dataBytes || dataBytes - offset < 4)) {
      throw new Error("session relocation is outside its static segment");
    }
    return {
      epoch: view.getUint32(4, true),
      dataBytes,
      tableLength: view.getUint32(12, true),
      offsets,
    };
  };

  captureLinked = () => {
    const base = this.linked.instances[0].exports;
    const memories = ["hd.x", "hd.session.heap", "hd.session.stack"].map((name) => [
      name,
      new Uint8Array(base[name].buffer).slice(),
    ]);
    const globals = this.linked.instances.flatMap((instance, epoch) =>
      Object.entries(instance.exports)
        .filter(
          ([name, value]) =>
            name.startsWith("hd.session.g.") && value instanceof WebAssembly.Global,
        )
        .map(([name, value]) => [epoch, name, value.value]),
    );
    return {
      memories,
      globals,
      epochs: this.linked.instances.length,
      tableLength: base["hd.session.table"].length,
    };
  };

  // Reconstruct code and active table segments without invoking an input.
  // A checkpoint restores initialized storage only after all its code epochs
  // exist, since their active data segments otherwise overwrite live values.
  instantiateLinked = async (part, instances, checkpoint = null) => {
    const { module, wasm, info } = part;
    const imports = {};
    let data = 0;
    if (info.epoch > 0) {
      const base = instances[0].exports;
      if (checkpoint) {
        data = part.data;
      } else {
        const block = base["hd.session.alloc"](info.dataBytes + 8);
        new Uint8Array(base["hd.session.heap"].buffer, block, 8).fill(0);
        data = block + 8;
      }
      const table = base["hd.session.table"];
      const length = checkpoint?.tableLength ?? info.tableLength;
      if (table.length < length) {
        table.grow(length - table.length);
      }
      imports["hd:session"] = {
        data: new WebAssembly.Global({ value: "i32", mutable: false }, data),
      };
      instances.forEach((instance, epoch) => {
        imports[`hd:session:${epoch}`] = instance.exports;
      });
    }
    const instance = await WebAssembly.instantiate(module, this.link(module, wasm, imports));
    if (info.epoch === 0 && checkpoint) {
      for (const [name, bytes] of checkpoint.memories) {
        const memory = instance.exports[name];
        const pages = bytes.byteLength / 65536 - memory.buffer.byteLength / 65536;
        if (pages > 0) {
          memory.grow(pages);
        }
      }
    }
    if (info.epoch > 0 && !checkpoint) {
      const heap = new DataView(instance.exports["hd.session.heap"].buffer);
      for (const offset of info.offsets) {
        const at = data + offset;
        heap.setUint32(at, heap.getUint32(at, true) + data - 16, true);
      }
    }
    instances.push(instance);
    return data;
  };

  restoreLinked = async () => {
    const instances = [];
    const dataOffsets = [];
    const checkpoint = this.linked.checkpoint;
    const tail = recorded(this.recorder.journal()).inputs.filter(
      (input) => input.number !== this.linked.cancelled,
    );
    const before = this.recorder;
    try {
      this.restart(new Recorder(Number.MAX_SAFE_INTEGER, this.linked.execution));
      const count = checkpoint?.epochs ?? 0;
      for (const part of this.linked.modules.slice(0, count)) {
        await this.instantiateLinked(part, instances, checkpoint);
      }
      if (checkpoint) {
        const base = instances[0].exports;
        for (const [name, bytes] of checkpoint.memories) {
          new Uint8Array(base[name].buffer, 0, bytes.byteLength).set(bytes);
        }
        for (const [epoch, name, value] of checkpoint.globals) {
          instances[epoch].exports[name].value = value;
        }
      }
      for (const [offset, part] of this.linked.modules.slice(count).entries()) {
        const input = tail[offset];
        if (input?.number !== part.number || !ranText(input, part.source)) {
          throw new Error("the active journal does not match its session epochs");
        }
        dataOffsets.push(await this.instantiateLinked(part, instances));
        await this.replayInput(instances.at(-1), input, part.source, part.wasm);
      }
      if (tail.length !== this.linked.modules.length - count) {
        throw new Error("the active journal has unexpected session inputs");
      }
      this.linked.instances = instances;
      this.linked.modules.slice(count).forEach((part, offset) => {
        part.data = dataOffsets[offset];
      });
      this.linked.cancelled = null;
    } catch (error) {
      this.restart(before);
      throw error;
    }
  };

  runLinked = async (module, info, wasm, sources, live, restoredJournal) => {
    const texts = new Map(sources);
    if (this.linked === null) {
      if (live === null) {
        throw new Error("restoring linked storage needs its original provider session");
      }
      if (info.epoch !== 0) {
        throw new Error("a linked session must start with its base module");
      }
      this.linked = {
        instances: [],
        modules: [],
        texts: new Map(),
        archives: [],
        checkpoint: null,
        execution:
          this.recorder.journal() === null
            ? this.execution
            : recorded(this.recorder.journal()).execution,
        cap: this.recorder.cap,
        cancelled: null,
      };
      // The active limit is checked at completed input boundaries. A single
      // large host reply must remain available to historical reload too.
      if (this.recorder.journal() === null) {
        this.recorder = new Recorder(Number.MAX_SAFE_INTEGER, this.execution);
      } else {
        this.recorder.setCap(Number.MAX_SAFE_INTEGER);
      }
    }
    for (const [number, previous] of this.linked.texts) {
      if (texts.get(number) !== previous) {
        throw new Error(`input ${number} needs a session rebuild`);
      }
    }
    const ran = {
      kept: [...this.linked.texts.keys()],
      skipped: [],
      dropped: [],
      reason: null,
      live: null,
    };
    if (this.linked.cancelled !== null) {
      ran.skipped.push(this.linked.cancelled);
    }
    if (live === null) {
      const previous = this.linked.modules.at(-1)?.wasm;
      if (
        !previous ||
        previous.length !== wasm.length ||
        !previous.every((b, i) => b === wasm[i])
      ) {
        throw new Error("changed session code needs a historical rebuild");
      }
      if (restoredJournal !== undefined) {
        const current = this.historyJournal();
        if (
          !restoredJournal ||
          restoredJournal.length !== current.length ||
          !restoredJournal.every((b, i) => b === current[i])
        ) {
          throw new Error("linked restore needs this provider session's current journal");
        }
      }
      await this.restoreLinked();
      return ran;
    }
    if (this.linked.cancelled !== null) {
      await this.restoreLinked();
    }
    const source = texts.get(live);
    if (source === undefined) {
      throw new Error(`input ${live} has no text`);
    }
    if (info.epoch !== this.linked.instances.length) {
      throw new Error("the module is not an extension of this initialized session");
    }
    const part = { module, info, wasm: wasm.slice(), number: live, source };
    const data = await this.instantiateLinked(part, this.linked.instances);
    const instance = this.linked.instances.at(-1);
    ran.live = await this.runInput(instance, live, source, wasm);
    if (ran.live.end.kind === "stopped" || ran.live.end.kind === "cancelled") {
      // Keep its visible cleanup/mutations until the next rebuild, as the
      // existing replay path does, but never commit its code or bindings.
      this.linked.cancelled = live;
      return ran;
    }
    this.linked.modules.push({ ...part, data });
    this.linked.texts.set(live, source);
    const journal = this.recorder.journal();
    if (journal.byteLength > this.linked.cap) {
      // Historical results stay outside the active journal; only the latest
      // initialized memory/global snapshot is retained.
      this.linked.checkpoint = this.captureLinked();
      this.linked.archives.push(journal.slice());
      this.recorder = new Recorder(Number.MAX_SAFE_INTEGER, this.linked.execution);
    }
    return ran;
  };

  // Public persistence and historical reload still receive the ordinary
  // journal format, including results outside the bounded active tail.
  historyJournal = () => {
    const tail = this.recorder.journal();
    if (!this.linked?.archives.length || tail === null) {
      return tail?.slice() ?? null;
    }
    return joinJournals([...this.linked.archives, tail]);
  };

  run = async (wasm, sources, live = null, journal = undefined) => {
    // A stop from an idle session is not a request to stop its next input.
    this.stopped = false;
    sources = [...sources];
    const module = await WebAssembly.compile(wasm);
    const info = this.sessionInfo(module);
    const previous = this.linked?.modules.at(-1)?.wasm;
    const sameCode = previous?.length === wasm.length && previous.every((b, i) => b === wasm[i]);
    const exports = new Set(WebAssembly.Module.exports(module).map(({ name }) => name));
    const rebuilding =
      info?.epoch === 0 &&
      live === null &&
      !sameCode &&
      sources.every(([number]) => exports.has(`hd.init.${number}`));
    if (info && !rebuilding) {
      return this.runLinked(module, info, wasm, sources, live, journal);
    }
    if (journal === undefined) {
      journal = this.historyJournal();
    }
    if (journal === null) {
      throw new Error("the session's record of host calls is full, so it cannot run another input");
    }
    const record = recorded(journal);
    const texts = new Map(sources);
    // A stop that came while no input ran is dropped; one that comes
    // while the inputs replay is for the new input.
    const cap = this.linked?.cap ?? this.recorder.cap;
    const start = async () => {
      // Refusing an import or failing instantiation must not replace the
      // last good journal. HD modules have no Wasm start function; their
      // effects begin only when runInput invokes the numbered entry.
      const instance = await WebAssembly.instantiate(module, this.link(module, wasm));
      this.restart(new Recorder(rebuilding ? Number.MAX_SAFE_INTEGER : cap, record.execution));
      this.linked = null;
      return instance;
    };
    let instance = await start();
    const ran = { kept: [], skipped: [], dropped: [], reason: null, live: null };
    const kept = [];
    for (const [at, input] of record.inputs.entries()) {
      if (input.end?.kind === "stopped" || input.end?.kind === "cancelled") {
        ran.skipped.push(input.number);
        continue;
      }
      const source = texts.get(input.number);
      try {
        if (source === undefined) {
          throw new Error(`input ${input.number} is not in the session`);
        }
        if (!ranText(input, source)) {
          throw new Error(`input ${input.number} is not the text the session ran`);
        }
        await this.replayInput(instance, input, source, wasm);
      } catch (e) {
        ran.dropped = record.inputs.slice(at).map((i) => i.number);
        ran.reason = String(e?.message ?? e);
        break;
      }
      ran.kept.push(input.number);
      kept.push([input, source]);
    }
    if (ran.dropped.length > 0) {
      instance = await start();
      for (const [input, source] of kept) {
        try {
          await this.replayInput(instance, input, source, wasm);
        } catch (e) {
          throw new Error(`the rebuild did not repeat itself: ${e.message}`);
        }
      }
      // An input left out of `sources` is one the caller forgot: the
      // program does not hold it, so the new input still runs. One that
      // diverged may have declared what the new input uses.
      if (ran.dropped.some((n) => texts.has(n))) {
        return ran;
      }
    }
    if (live !== null) {
      const source = texts.get(live);
      if (source === undefined) {
        throw new Error(`input ${live} has no text`);
      }
      ran.live = await this.runInput(instance, live, source, wasm);
    }
    if (rebuilding && ran.dropped.length === 0 && ran.kept.length > 0) {
      const number = ran.kept.at(-1);
      this.linked = {
        instances: [instance],
        modules: [
          { module, info, wasm: wasm.slice(), number, source: texts.get(number), data: 16 },
        ],
        texts: new Map(ran.kept.map((n) => [n, texts.get(n)])),
        archives: [this.recorder.journal().slice()],
        checkpoint: null,
        execution: record.execution,
        cap,
        cancelled: null,
      };
      this.linked.checkpoint = this.captureLinked();
      this.recorder = new Recorder(Number.MAX_SAFE_INTEGER, this.linked.execution);
    }
    return ran;
  };

  interface() {
    return {
      run: this.run,
      // A rebuild may drop every input. Keep the provider, but forget its
      // old code and history before the next input starts a new base.
      clear: () => {
        const cap = this.linked?.cap ?? this.recorder.cap;
        const history = this.historyJournal();
        const id = history === null ? this.execution : recorded(history).execution;
        const next = new Recorder(Number.MAX_SAFE_INTEGER, id);
        next.setCap(cap);
        this.restart(next);
        this.linked = null;
      },
      // A copy of the journal so far, or null once it passed its cap.
      journal: this.historyJournal,
      setJournalCap: (cap) => {
        if (this.linked) {
          this.linked.cap = cap;
        } else {
          this.recorder.setCap(cap);
        }
      },
      // True once, after the input in which the journal passed its cap.
      takeJournalNotice: () => this.recorder.takeNotice(),
      stop: () => {
        this.stopped = true;
        this.wakeWait?.();
      },
      hostCalls: () => this.calls,
      checkpointStats: () => ({
        epochs: this.linked?.instances.length ?? 0,
        archivedJournals: this.linked?.archives.length ?? 0,
        archivedBytes: this.linked?.archives.reduce((sum, bytes) => sum + bytes.byteLength, 0) ?? 0,
        snapshotBytes:
          this.linked?.checkpoint?.memories.reduce((sum, [, bytes]) => sum + bytes.byteLength, 0) ??
          0,
        activeJournalBytes: this.recorder.journal()?.byteLength ?? 0,
      }),
      close: () => {
        this.linked = null;
        this.host.close();
      },
    };
  }
}

export function createSession(sink, env = {}, layer = null, options = {}) {
  return new ReplSession(
    sink,
    { ...env, isReplayDivergence: (error) => error instanceof Diverged },
    layer,
    options,
    createCoreHost,
    clearOperations,
  ).interface();
}
