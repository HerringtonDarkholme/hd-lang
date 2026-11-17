// Helpers used by signature-generated NAME.host.ts modules. Providers are
// trusted JS selected from package manifests after the root grants access.
// Guest contract data is never interpreted as module code or import paths.
import { Dec, Enc, HostContract, decodeUtf8 } from "./wire.ts";
import { identityName } from "./core-package.ts";
export { readPackageContract } from "./package-contract.ts";
import { readPackageContract } from "./package-contract.ts";
import type {
  ContractField,
  ContractIdentity,
  ContractInterface,
  ContractValue,
} from "./package-contract.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;
type RuntimeField = [string, RuntimeCodec];
type RuntimeVariant = [string, RuntimeField[]];

interface RuntimeCodec {
  shape: ContractValue;
  enc(encoder: Enc, value: Dynamic, transaction: number[]): Dynamic;
  dec(decoder: Dec): Dynamic;
  /** A container's own step: its tag or count, and its children. */
  open?: Container;
}

/** One step of the encode loop: a value to write, or a recursive type's exit. */
type EncodeTask = [RuntimeCodec, Dynamic] | (() => void);

/**
 * A container's step in the explicit-stack codec loops: `encode` writes
 * its tag or count and pushes its children's tasks, last first; `decode`
 * reads its tag or count and returns the frame that takes its children's
 * values in order.
 */
interface Container {
  encode(
    encoder: Enc,
    value: Dynamic,
    transaction: number[],
    push: (task: EncodeTask) => void,
  ): void;
  decode(decoder: Dec): Frame;
}

/** A container being decoded: its next child's codec, then its value. */
interface Frame {
  next(): RuntimeCodec | undefined;
  put(value: Dynamic): void;
  done(): Dynamic;
}

/**
 * Writes `value` through `root` with one explicit stack, so a value as deep
 * as the provider makes it never recurses natively (AGENTS.md
 * "Performant"). A failure part way runs the exits still pending, so a
 * recursive type's path set is left empty. Cost: O(1) per container plus
 * each leaf's codec; the stack holds the open containers' pending children.
 */
const encodeAll = (root: RuntimeCodec, encoder: Enc, value: Dynamic, tx: number[]): void => {
  const stack: EncodeTask[] = [[root, value]];
  const push = (task: EncodeTask): void => {
    stack.push(task);
  };
  try {
    for (let task = stack.pop(); task !== undefined; task = stack.pop()) {
      if (typeof task === "function") {
        task();
        continue;
      }
      const [codec, item] = task;
      if (codec.open) {
        codec.open.encode(encoder, item, tx, push);
      } else {
        codec.enc(encoder, item, tx);
      }
    }
  } catch (error) {
    for (const task of stack) {
      if (typeof task === "function") {
        task();
      }
    }
    throw error;
  }
};

/**
 * Reads a value through `root` with one explicit stack of open frames; each
 * finished value goes to the frame below it. Cost: O(1) per container plus
 * each leaf's codec; the stack holds the open containers.
 */
const decodeAll = (root: RuntimeCodec, decoder: Dec): Dynamic => {
  const frames: Frame[] = [];
  let next: RuntimeCodec = root;
  for (;;) {
    let value: Dynamic;
    if (next.open) {
      const frame = next.open.decode(decoder);
      const child = frame.next();
      if (child !== undefined) {
        frames.push(frame);
        next = child;
        continue;
      }
      value = frame.done();
    } else {
      value = next.dec(decoder);
    }
    for (;;) {
      const top = frames.at(-1);
      if (top === undefined) {
        return value;
      }
      top.put(value);
      const child = top.next();
      if (child !== undefined) {
        next = child;
        break;
      }
      frames.pop();
      value = top.done();
    }
  }
};

/** A container codec whose `enc` and `dec` run the explicit-stack loops. */
const container = (shape: ContractValue, open: Container): RuntimeCodec => {
  const self: RuntimeCodec = {
    shape,
    enc: (encoder, value, tx) => encodeAll(self, encoder, value, tx),
    dec: (decoder) => decodeAll(self, decoder),
    open,
  };
  return self;
};

/** The frame of children read in order by `codecs`, built by `build`. */
const sequence = (codecs: RuntimeCodec[], build: (values: Dynamic[]) => Dynamic): Frame => {
  const values: Dynamic[] = [];
  return {
    next: () => codecs[values.length],
    put: (value) => {
      values.push(value);
    },
    done: () => build(values),
  };
};

/** Pushes each child task last first, so the first is written first. */
const pushAll = (push: (task: EncodeTask) => void, tasks: [RuntimeCodec, Dynamic][]): void => {
  for (let index = tasks.length - 1; index >= 0; index--) {
    push(tasks[index]);
  }
};

interface RuntimeMethod {
  name: string;
  member: string;
  wait: "never" | "may";
  params: RuntimeCodec[];
  result: RuntimeCodec;
}

interface RuntimeInterface {
  module: string;
  handle: boolean;
  methods: RuntimeMethod[];
}

interface ProviderRegistration {
  contract: Uint8Array;
  module: string;
  identity: ContractIdentity;
  implementation: DynamicRecord;
}

export interface PackageHostIO extends DynamicRecord {}

export interface PackageRuntimeOptions {
  module?: WebAssembly.Module;
  grants?: Record<string, boolean>;
}

export interface PackageRuntime extends DynamicRecord {
  bind(
    contract: Uint8Array,
    generated: RuntimeInterface[],
    providers: ProviderRegistration[],
  ): DynamicRecord;
  close(): void;
  handleCount(): number;
}

const fail = (message: string): never => {
  throw new HostContract(`package host: ${message}`);
};
const same = (left: Dynamic, right: Dynamic): boolean =>
  JSON.stringify(left) === JSON.stringify(right);
const equalBytes = (left: Uint8Array, right: Uint8Array): boolean =>
  left.length === right.length && left.every((item, i) => item === right[i]);
const own = (value: Dynamic, k: string): Dynamic => {
  if (value === null || typeof value !== "object" || !Object.hasOwn(value, k)) {
    fail(`missing field ${k}`);
  }
  return value[k];
};
const object = (fields: [string, Dynamic][]): DynamicRecord => Object.fromEntries(fields);
const named = (i: ContractInterface): string => identityName(i.identity);

function encodePrimitive(value: Dynamic, tokens: Dynamic[]): boolean {
  if (value === null) {
    tokens.push(["n"]);
  } else if (typeof value === "boolean") {
    tokens.push(["b", value]);
  } else if (typeof value === "string") {
    tokens.push(["s", value]);
  } else if (typeof value === "bigint") {
    if (value < -(1n << 63n) || value >= 1n << 64n) {
      fail("private integer outside 64-bit range");
    }
    tokens.push([value < 0n ? "i" : "u", String(value)]);
  } else if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      tokens.push(["n"]);
    } else {
      const bits = new DataView(new ArrayBuffer(8));
      bits.setFloat64(0, value, true);
      tokens.push(["f", bits.getBigUint64(0, true).toString(16).padStart(16, "0")]);
    }
  } else {
    return false;
  }
  return true;
}

// Typed Serde transport is internal. Provider methods receive JS values:
// bigint integers, number floats, and ordinary arrays/objects. Using a flat
// stream preserves numeric kind and avoids multiplying container depth.
function encodePrivate(value: Dynamic): string {
  const tokens: Dynamic[] = [];
  const pending: ({ value: Dynamic } | { leave: object })[] = [{ value }];
  const active = new Set<object>();
  while (pending.length) {
    const job = pending.pop()!;
    if ("leave" in job) {
      active.delete(job.leave);
      continue;
    }
    const value = job.value;
    if (encodePrimitive(value, tokens)) {
      continue;
    }
    if (typeof value === "object") {
      if (active.has(value)) {
        fail("cyclic private value");
      }
      active.add(value);
      pending.push({ leave: value });
      if (Array.isArray(value)) {
        tokens.push(["a", value.length]);
        for (let i = value.length - 1; i >= 0; --i) {
          pending.push({ value: value[i] });
        }
      } else {
        if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
          fail("private value must be a plain object");
        }
        const entries = Object.entries(value);
        tokens.push(["o", entries.length]);
        for (let i = entries.length - 1; i >= 0; --i) {
          pending.push({ value: entries[i][1] }, { value: entries[i][0] });
        }
      }
    } else {
      fail("unsupported private value");
    }
  }
  return JSON.stringify(tokens);
}

function decodePrivateNumber(tag: Dynamic, value: Dynamic): Dynamic {
  if (
    typeof value !== "string" ||
    !(tag === "f" ? /^[0-9a-f]{16}$/ : /^(?:0|-?[1-9][0-9]*)$/).test(value)
  ) {
    fail("invalid private numeric token");
  }
  const number = BigInt(tag === "f" ? "0x" + value : value);
  if (tag === "i") {
    if (number < -(1n << 63n) || number >= 1n << 63n) {
      fail("private signed integer out of range");
    }
    return number;
  }
  if (number < 0n || number >= 1n << 64n) {
    fail("private unsigned integer out of range");
  }
  if (tag === "u") {
    return number;
  }
  const bits = new DataView(new ArrayBuffer(8));
  bits.setBigUint64(0, number, true);
  const float = bits.getFloat64(0, true);
  if (!Number.isFinite(float)) {
    fail("invalid private float");
  }
  return float;
}

function decodePrivateContainer(tag: Dynamic, length: Dynamic, values: Dynamic[]): Dynamic {
  if (
    !Number.isSafeInteger(length) ||
    length < 0 ||
    length > values.length / (tag === "o" ? 2 : 1)
  ) {
    fail("invalid private container length");
  }
  if (tag === "a") {
    return Array.from({ length }, () => values.pop());
  }
  const entries = [];
  const keys = new Set();
  for (let count = 0; count < length; ++count) {
    const key = values.pop();
    if (typeof key !== "string" || keys.has(key)) {
      fail("invalid private object key");
    }
    keys.add(key);
    entries.push([key, values.pop()]);
  }
  return Object.fromEntries(entries);
}

function decodePrivateToken(token: Dynamic, values: Dynamic[]): void {
  if (!Array.isArray(token)) {
    fail("invalid private value token");
  }
  const [tag, value] = token;
  if (token.length !== (tag === "n" ? 1 : 2)) {
    fail("invalid private value token");
  }
  if (tag === "n") {
    values.push(null);
  } else if (tag === "b" && typeof value === "boolean") {
    values.push(value);
  } else if (tag === "s" && typeof value === "string") {
    values.push(value);
  } else if (["i", "u", "f"].includes(tag)) {
    values.push(decodePrivateNumber(tag, value));
  } else if (tag === "a" || tag === "o") {
    values.push(decodePrivateContainer(tag, value, values));
  } else {
    fail("invalid private value token");
  }
}

function decodePrivate(text: string): Dynamic {
  let tokens: Dynamic;
  try {
    tokens = JSON.parse(text);
  } catch {
    fail("invalid private value transport");
  }
  if (!Array.isArray(tokens)) {
    fail("invalid private value transport");
  }
  const values: Dynamic[] = [];
  for (let i = tokens.length - 1; i >= 0; --i) {
    decodePrivateToken(tokens[i], values);
  }
  if (values.length !== 1) {
    fail("invalid private value framing");
  }
  return values[0];
}

// Each codec carries its canonical shape, checked against the data section
// before an instance starts. Encoding validates before the wire writer can
// coerce values (notably a negative unsigned LEB value).
/* oxlint-disable max-lines-per-function -- codecs and bind share one instance lifetime. */
export function createPackageRuntime(
  io: PackageHostIO,
  { module, grants = {} }: PackageRuntimeOptions = {},
): PackageRuntime {
  const handles = new Map<number, { namespace: string; value: Dynamic }>();
  const handleMethods = new Map<string, string[]>();
  let nextHandle = 1;
  const lifetime = new AbortController();
  const codec = (
    shape: ContractValue,
    enc: RuntimeCodec["enc"],
    dec: RuntimeCodec["dec"],
  ): RuntimeCodec => ({ shape, enc, dec });
  const unit = codec(
    [0],
    (_e, value) => {
      if (value !== undefined) {
        fail("void result has a value");
      }
    },
    () => undefined,
  );
  // A method that never returns (module.boundary.never-result): a provider
  // that returns at all broke the contract, which `write` reports with
  // the method's name.
  const never = codec(
    [19],
    () => fail("the method never returns, but its provider returned"),
    () => fail("a never result has no value"),
  );
  const bool = codec(
    [1],
    (error, value) => {
      if (typeof value !== "boolean") {
        fail("expected bool");
      }
      error.leb(value ? 1 : 0);
    },
    (data) => data.tag(2) === 1,
  );
  const int = (bits: number, signed: boolean): RuntimeCodec => {
    const low = signed ? -(1n << BigInt(bits - 1)) : 0n;
    const high = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
    const check = (value: Dynamic): bigint => {
      if (
        typeof value !== "bigint" &&
        !(typeof value === "number" && Number.isSafeInteger(value))
      ) {
        fail("expected integer");
      }
      const bigInt = BigInt(value);
      if (bigInt < low || bigInt > high) {
        fail("integer is out of range");
      }
      return bigInt;
    };
    return codec(
      [3, bits, signed],
      (error, value) => {
        const checked = check(value);
        if (signed) {
          error.zz(checked);
        } else {
          error.leb(checked);
        }
      },
      (data) => {
        const checked = check(signed ? data.zz() : data.leb());
        return bits === 64 ? checked : Number(checked);
      },
    );
  };
  const char = codec(
    [2],
    (error, value) => {
      if (typeof value !== "string" || [...value].length !== 1) {
        fail("expected char");
      }
      const codePoint = value.codePointAt(0);
      if (codePoint >= 0xd800 && codePoint <= 0xdfff) {
        fail("invalid char");
      }
      error.leb(codePoint);
    },
    (data) => {
      const codePoint = data.num();
      if (codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
        fail("invalid char");
      }
      return String.fromCodePoint(codePoint);
    },
  );
  const float = (tag: 4 | 5, narrow: boolean): RuntimeCodec =>
    codec(
      [tag],
      (error, value) => {
        if (typeof value !== "number") {
          fail("expected float");
        }
        const count = narrow ? 4 : 8;
        error.room(count);
        const view = new DataView(error.buf.buffer, error.buf.byteOffset + error.len, count);
        if (narrow) {
          view.setFloat32(0, value, true);
        } else {
          view.setFloat64(0, value, true);
        }
        error.len += count;
      },
      (data) => {
        const count = narrow ? 4 : 8;
        if (data.at + count > data.input.length) {
          fail("truncated float");
        }
        const view = new DataView(data.input.buffer, data.input.byteOffset + data.at, count);
        const value = narrow ? view.getFloat32(0, true) : view.getFloat64(0, true);
        data.at += count;
        return value;
      },
    );
  const str = codec(
    [6],
    (error, value) => {
      if (typeof value !== "string") {
        fail("expected string");
      }
      error.str(value);
    },
    (data) => data.str(),
  );
  const bytes = codec(
    [7],
    (error, value) => {
      if (!(value instanceof Uint8Array)) {
        fail("expected Uint8Array");
      }
      error.bytes(value);
    },
    (data) => data.bytes().slice(),
  );
  const json = codec(
    [14],
    (error, value) => error.str(encodePrivate(value)),
    (data) => decodePrivate(data.str()),
  );
  const array = (xs: Dynamic, value: number | null = null): void => {
    if (!Array.isArray(xs) || (value !== null && xs.length !== value)) {
      fail("invalid array");
    }
  };
  const list = (text: RuntimeCodec): RuntimeCodec =>
    container([8, text.shape], {
      encode: (error, xs, _tx, push) => {
        array(xs);
        error.leb(xs.length);
        for (let index = xs.length - 1; index >= 0; index--) {
          push([text, xs[index]]);
        }
      },
      decode: (data) => {
        const length = data.num();
        if (!Number.isSafeInteger(length) || length > 0xffffffff) {
          fail("invalid list length");
        }
        const out: Dynamic[] = [];
        return {
          next: () => (out.length < length ? text : undefined),
          put: (value) => {
            out.push(value);
          },
          done: () => out,
        };
      },
    });
  // `Map[K, V]` as a JS `Map`: its size, then each key and value in the
  // map's order. Decoding keeps a repeated key's last value, as hd's
  // decoder does (module.boundary.map-decode).
  const map = (key: RuntimeCodec, value: RuntimeCodec): RuntimeCodec =>
    container([18, key.shape, value.shape], {
      encode: (error, entries, _tx, push) => {
        if (!(entries instanceof Map)) {
          fail("expected Map");
        }
        error.leb(entries.size);
        const tasks: [RuntimeCodec, Dynamic][] = [];
        for (const [entryKey, entryValue] of entries) {
          tasks.push([key, entryKey], [value, entryValue]);
        }
        pushAll(push, tasks);
      },
      decode: (data) => {
        const size = data.num();
        if (!Number.isSafeInteger(size) || size > 0xffffffff) {
          fail("invalid map size");
        }
        const out = new Map<unknown, unknown>();
        let read = 0;
        let entryKey: Dynamic;
        return {
          next: () => {
            if (read >= 2 * size) {
              return undefined;
            }
            return read % 2 === 0 ? key : value;
          },
          put: (item) => {
            if (read % 2 === 0) {
              entryKey = item;
            } else {
              out.set(entryKey, item);
            }
            read++;
          },
          done: () => out,
        };
      },
    });
  const option = (text: RuntimeCodec): RuntimeCodec =>
    container([9, text.shape], {
      encode: (error, value, _tx, push) => {
        if (value === null) {
          error.leb(0);
        } else {
          error.leb(1);
          push([text, value]);
        }
      },
      decode: (data) =>
        sequence(data.tag(2) ? [text] : [], (values) => (values.length > 0 ? values[0] : null)),
    });
  const tuple = (ts: RuntimeCodec[]): RuntimeCodec =>
    container([10, ts.map((item) => item.shape)], {
      encode: (_error, xs, _tx, push) => {
        array(xs, ts.length);
        pushAll(
          push,
          ts.map((item, index): [RuntimeCodec, Dynamic] => [item, xs[index]]),
        );
      },
      decode: () => sequence(ts, (values) => values),
    });
  const fieldShape = (fs: RuntimeField[]): ContractField[] =>
    fs.map(([count, text]) => [count, text.shape]);
  // Each field's codec and value in order, read before any is written, so
  // a missing field fails on the first one as the field order reads.
  const fieldTasks = (value: Dynamic, fs: RuntimeField[]): [RuntimeCodec, Dynamic][] =>
    fs.map(([count, text]) => [text, own(value, count)]);
  const fieldFrame = (fs: RuntimeField[], build: (record: DynamicRecord) => Dynamic): Frame =>
    sequence(
      fs.map(([, text]) => text),
      (values) => build(object(fs.map(([count], index) => [count, values[index]]))),
    );
  const record = (fs: RuntimeField[]): RuntimeCodec =>
    container([11, fieldShape(fs)], {
      encode: (_error, value, _tx, push) => pushAll(push, fieldTasks(value, fs)),
      decode: () => fieldFrame(fs, (fields) => fields),
    });
  const enumeration = (vs: RuntimeVariant[]): RuntimeCodec =>
    container([12, vs.map(([count, fs]) => [count, fieldShape(fs)])], {
      encode: (error, value, _tx, push) => {
        const tag = own(value, "tag"),
          index = vs.findIndex(([count]) => count === tag);
        if (index < 0) {
          fail("unknown enum variant");
        }
        error.leb(index);
        pushAll(push, fieldTasks(own(value, "fields"), vs[index][1]));
      },
      decode: (data) => {
        const [tag, fs] = vs[data.tag(vs.length)];
        return fieldFrame(fs, (fields) => ({ tag, fields }));
      },
    });
  const result = (ok: RuntimeCodec, err: RuntimeCodec): RuntimeCodec =>
    container([13, ok.shape, err.shape], {
      encode: (error, value, _tx, push) => {
        const tag = own(value, "ok");
        if (typeof tag !== "boolean") {
          fail("invalid Result tag");
        }
        error.leb(tag ? 0 : 1);
        push([tag ? ok : err, own(value, tag ? "value" : "error")]);
      },
      decode: (data) =>
        data.tag(2)
          ? sequence([err], ([error]) => ({ ok: false, error }))
          : sequence([ok], ([value]) => ({ ok: true, value })),
    });
  const handle = (namespace: string): RuntimeCodec =>
    codec(
      [15, namespace],
      (error, value, tx) => {
        if ((typeof value !== "object" && typeof value !== "function") || value === null) {
          fail("expected provider handle object");
        }
        const methods = handleMethods.get(namespace);
        if (!methods || methods.some((name) => typeof value[name] !== "function")) {
          fail("returned handle is missing a declared method");
        }
        if (nextHandle > 0x7fffffff) {
          fail("handle space exhausted");
        }
        const id = nextHandle++;
        handles.set(id, { namespace, value: value });
        tx.push(id);
        error.leb(id);
      },
      () => fail("a handle cannot be passed as a data argument"),
    );
  // A recursive type's one codec (`[16, id, body]`). `build` makes the
  // body from the codec of `[17, id]`, which steps into the body again
  // through the caller's explicit stack, never a native call per level.
  // Encoding tracks the values on its path: a provider value that reaches
  // itself again is the `boundary-cycle` failure
  // (module.boundary.cycle.tracked), never an unbounded walk. A value
  // joins the path when its step runs and leaves it when the exit task,
  // pushed under its body's children, runs after the last of them.
  const rec = (id: number, build: (self: RuntimeCodec) => RuntimeCodec): RuntimeCodec => {
    const active = new Set<object>();
    let body: RuntimeCodec | undefined;
    const open: Container = {
      encode: (_error, value, _tx, push) => {
        if (typeof value === "object" && value !== null) {
          if (active.has(value)) {
            fail("boundary-cycle: a provider value reaches itself");
          }
          active.add(value);
          push(() => active.delete(value));
        }
        push([body!, value]);
      },
      decode: () => sequence([body!], ([value]) => value),
    };
    const self = container([17, id], open);
    body = build(self);
    return container([16, id, body.shape], open);
  };
  const runtime = {
    rec,
    unit,
    never,
    bool,
    char,
    int,
    f32: float(4, true),
    f64: float(5, false),
    str,
    bytes,
    json,
    list,
    map,
    option,
    tuple,
    record,
    enumeration,
    result,
    handle,
  };
  const registrationsFor = (providers: ProviderRegistration[]) => {
    if (!Array.isArray(providers)) {
      fail("providers must be explicit registrations");
    }
    const registrations = new Map<
      string,
      { expected: ContractInterface[]; root: ContractInterface; implementation: DynamicRecord }
    >();
    for (const provider of providers) {
      const expected = readPackageContract(provider.contract);
      const root =
        expected.find(
          (item) =>
            !item.handle &&
            item.module === provider.module &&
            same(item.identity, provider.identity),
        ) ?? fail("invalid or duplicate provider registration");
      if (registrations.has(provider.module)) {
        fail("invalid or duplicate provider registration");
      }
      const implementation = provider.implementation;
      if (
        implementation === null ||
        typeof implementation !== "object" ||
        ![Object.prototype, null].includes(Object.getPrototypeOf(implementation))
      ) {
        fail(`provider for ${named(root)} must default-export a plain object of methods`);
      }
      registrations.set(provider.module, { expected, root, implementation });
    }
    return registrations;
  };
  const validateRegistrations = (
    required: ContractInterface[],
    registrations: ReturnType<typeof registrationsFor>,
  ): void => {
    const checkExpected = (
      needed: ContractInterface,
      expected: ContractInterface[],
      seen = new Set<string>(),
    ): void => {
      if (seen.has(needed.module)) {
        return;
      }
      seen.add(needed.module);
      const item =
        expected.find(
          (candidate) =>
            candidate.module === needed.module &&
            candidate.handle === needed.handle &&
            same(candidate.identity, needed.identity),
        ) ?? fail(`provider identity/schema mismatch for ${named(needed)}`);
      for (const method of needed.methods) {
        if (!item.methods.some((other) => same(method, other))) {
          fail(`provider method/schema mismatch for ${named(needed)}.${method.member}`);
        }
      }
      const visit = (value: ContractValue): void => {
        if (value[0] === 15) {
          const nested =
            required.find((entry) => entry.handle && entry.module === value[1]) ??
            fail("missing handle interface");
          checkExpected(nested, expected, seen);
        }
        if (value[0] === 8 || value[0] === 9) {
          visit(value[1]);
        }
        if (value[0] === 10) {
          value[1].forEach(visit);
        }
        if (value[0] === 11) {
          value[1].forEach(([, type]) => visit(type));
        }
        if (value[0] === 12) {
          value[1].forEach(([, fields]) => fields.forEach(([, type]) => visit(type)));
        }
        if (value[0] === 13) {
          visit(value[1]);
          visit(value[2]);
        }
        if (value[0] === 16) {
          visit(value[2]);
        }
      };
      for (const method of needed.methods) {
        method.params.forEach(visit);
        visit(method.result);
      }
    };
    for (const item of required.filter((entry) => !entry.handle)) {
      const registration =
        registrations.get(item.module) ??
        fail(`missing provider for ${named(item)}; register it explicitly`);
      checkExpected(item, registration.expected);
      if (
        item.selectors.some(
          (selector) => Object.hasOwn(grants, selector) && grants[selector] === false,
        )
      ) {
        fail(`capability denied: ${named(item)}`);
      }
      for (const method of item.methods) {
        if (
          !Object.hasOwn(registration.implementation, method.member) ||
          typeof registration.implementation[method.member] !== "function"
        ) {
          fail(`missing provider method ${named(item)}.${method.member}`);
        }
      }
    }
  };
  const argumentsFor = (method: RuntimeMethod, isHandle: boolean, raw: Dynamic[]): Dynamic[] => {
    if (!method.params.length) {
      return [];
    }
    const length = raw.shift();
    if (!Number.isInteger(length) || (length as number) < 0) {
      fail("invalid argument buffer length");
    }
    const bytes = Uint8Array.from(io.view(length as number));
    if (!isHandle && method.params.length === 1 && method.params[0] === str) {
      return [decodeUtf8(bytes)];
    }
    const decoder = new Dec(bytes);
    const args = method.params.map((type) => type.dec(decoder));
    if (decoder.at !== bytes.length) {
      fail("trailing argument bytes");
    }
    return args;
  };
  const bind = (
    contract: Uint8Array,
    generated: RuntimeInterface[],
    providers: ProviderRegistration[],
  ): DynamicRecord => {
    const descriptors = generated.map((i) => ({
      ...i,
      methods: i.methods.map((item) => ({ ...item, params: [...item.params] })),
    }));
    const required = readPackageContract(contract);
    const sections = module && WebAssembly.Module.customSections(module, "hd.capabilities");
    if (!sections || sections.length !== 1 || !equalBytes(contract, new Uint8Array(sections[0]))) {
      fail("generated glue does not match the guest contract");
    }
    const shapes = descriptors.map((i) => ({
      ...i,
      methods: i.methods.map((item) => ({
        ...item,
        params: item.params.map((item) => item.shape),
        result: item.result.shape,
      })),
    }));
    if (
      !same(
        shapes,
        required.map(({ module, handle, methods }) => ({ module, handle, methods })),
      )
    ) {
      fail("generated method shapes do not match the contract");
    }
    for (const i of descriptors.filter((i) => i.handle)) {
      handleMethods.set(
        i.module,
        i.methods.map((item) => item.member),
      );
    }
    const registrations = registrationsFor(providers);
    validateRegistrations(required, registrations);
    const imports: DynamicRecord = Object.create(null);
    for (const i of descriptors) {
      const declared =
        required.find((item) => item.module === i.module) ??
        fail("generated interface is absent from the contract");
      const interfaceIdentity = Object.freeze({ ...declared.identity });
      const out: DynamicRecord = Object.create(null);
      if (i.handle) {
        out.$release = (id: number) => {
          const host = handles.get(id);
          if (!host || host.namespace !== i.module) {
            fail("release of an invalid handle");
          }
          handles.delete(id);
        };
      }
      for (const method of i.methods) {
        const key = `${i.module} ${method.name}`;
        const label = `${named(declared)}.${method.member}`;
        const write = (value: Dynamic): Dynamic => {
          const tx: number[] = [];
          try {
            if (method.result === unit) {
              unit.enc(null as never, value, tx);
              return method.wait === "may" ? 0 : undefined;
            }
            const encoder = new Enc();
            method.result.enc(encoder, value, tx);
            return io.put(encoder);
          } catch (error) {
            for (const id of tx) {
              handles.delete(id);
            }
            throw new HostContract(
              `${label}: ${error instanceof Error ? error.message : String(error)}`,
            );
          }
        };
        const invoke = (raw: Dynamic[], signal: AbortSignal): Dynamic => {
          if (lifetime.signal.aborted) {
            fail("instance is closed");
          }
          let receiver = registrations.get(i.module)?.implementation;
          if (i.handle) {
            const host = handles.get(raw.shift()) ?? fail("call of an invalid handle");
            if (host.namespace !== i.module) {
              fail("call of an invalid handle");
            }
            receiver = host.value;
          }
          const args = argumentsFor(method, i.handle, raw);
          if (raw.length) {
            fail("wrong import arity");
          }
          const file = receiver?.[method.member];
          if (typeof file !== "function") {
            fail(`missing provider method ${method.member}`);
          }
          try {
            const context = {
              signal,
              instanceSignal: lifetime.signal,
              pending(
                poll: () => boolean,
                done: () => Dynamic,
                abort: (() => void) | null = null,
                finish: (() => void) | null = null,
              ): Dynamic {
                if (method.wait !== "may") {
                  fail(`non-suspending method ${label} cannot create a pending operation`);
                }
                if (typeof poll !== "function" || typeof done !== "function") {
                  fail("a pending operation needs readiness and result functions");
                }
                if (
                  (abort !== null && typeof abort !== "function") ||
                  (finish !== null && typeof finish !== "function")
                ) {
                  fail("pending cancellation and delivery hooks must be functions");
                }
                if (!io.prepare) {
                  fail("the host cannot create pending operations");
                }
                return io.prepare(poll, done, abort, finish);
              },
            };
            Object.defineProperty(context, "interfaceIdentity", {
              enumerable: true,
              value: interfaceIdentity,
            });
            Object.defineProperty(context, "http", {
              enumerable: true,
              get() {
                if (!io.http) {
                  fail("the host does not provide HTTP policy services");
                }
                return io.http;
              },
            });
            Object.defineProperty(context, "console", {
              enumerable: true,
              get() {
                if (!io.console) {
                  fail("the host does not provide Console policy services");
                }
                return io.console;
              },
            });
            return file.call(receiver, ...args, context);
          } catch (error) {
            throw new HostContract(`${label}: provider threw ${String(error)}`);
          }
        };
        if (method.wait === "never") {
          out[method.name] = (...raw: Dynamic[]) => {
            const value = invoke(raw, lifetime.signal);
            if (value && typeof value.then === "function") {
              Promise.resolve(value).catch(() => {});
              fail(`non-suspending method ${label} returned a Promise`);
            }
            const prepared = io.prepared?.(value);
            if (prepared) {
              try {
                prepared.abort?.();
              } finally {
                fail(`non-suspending method ${label} returned a pending operation`);
              }
            }
            return write(value);
          };
        } else {
          out[`${method.name}.start`] = (...raw: Dynamic[]) => {
            const operation = new AbortController();
            const value = invoke(raw, operation.signal);
            const prepared = io.prepared?.(value);
            if (prepared) {
              const call = (hook: (() => Dynamic) | null | undefined): Dynamic => {
                try {
                  return hook?.();
                } catch (error) {
                  throw new HostContract(`${label}: provider callback threw ${String(error)}`);
                }
              };
              return io.pending({
                at: prepared.at,
                done: () => call(prepared.done),
                method: key,
                poll: () => {
                  const ready = call(prepared.poll);
                  if (typeof ready !== "boolean") {
                    fail(`readiness probe for ${label} did not return a boolean`);
                  }
                  return ready;
                },
                abort: () => {
                  operation.abort();
                  call(prepared.abort);
                },
                finish: prepared.finish ? () => call(prepared.finish) : null,
              });
            }
            if (!value || typeof value.then !== "function") {
              return [0, write(value)];
            }
            let live = true,
              ready = false,
              rejected = false,
              kept: Dynamic = undefined,
              failure: Dynamic = undefined;
            const pending = io.pending({
              at: performance.now(),
              promise: true,
              done: () => {
                if (rejected) {
                  fail(`provider ${label} rejected: ${String(failure)}`);
                }
                return kept;
              },
              method: key,
              poll: () => ready,
              abort: () => {
                live = false;
                kept = undefined;
                operation.abort();
              },
            });
            Promise.resolve(value).then(
              (value) => {
                if (live) {
                  kept = value;
                  ready = true;
                  io.wake?.();
                }
              },
              (value) => {
                if (live) {
                  failure = value;
                  rejected = true;
                  ready = true;
                  io.wake?.();
                }
              },
            );
            return pending;
          };
          out[`${method.name}.finish`] = (id: number) => write(io.finished(id, key));
        }
      }
      imports[i.module] = out;
    }
    return imports;
  };
  return Object.assign(runtime, {
    bind,
    close: () => {
      lifetime.abort();
      handles.clear();
    },
    handleCount: () => handles.size,
  });
}
/* oxlint-enable max-lines-per-function */
