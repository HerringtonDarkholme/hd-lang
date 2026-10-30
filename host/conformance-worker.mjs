// Long-lived Node worker for driver conformance entry runs (M39).
//
// One process per conformance worker thread, started once. It loads the
// host (`node.mjs`, `profiles.mjs`) once and takes cases over stdin, one
// framed message per case: a 4-byte big-endian header length, the header
// JSON, then the wasm bytes and the stdin text. The header is
// `{"id","wasmLen","profile","scenario","args","stdinLen"}`: the case's
// serial as `id`, the wasm byte count, the fixture's runtime profile name
// or null, the runtime scenario name or null (driven by `runScenario`
// instead of `run`), the program arguments (always empty in conformance)
// and the stdin byte count (always empty in conformance, accepted for
// protocol completeness). A header with a `tests` field, the `hd test`
// case list `test:init:kind,...`, runs those cases of the test program as
// `test.mjs` does (`test-run.mjs`); its stdout is their result lines.
// It returns one framed message per case over stdout: a 4-byte big-endian
// header length, the header JSON `{"id","status","stdoutLen","stderrLen"}`,
// then the stdout and stderr bytes.
//
// Each case gets a fresh host (`createHost`, `WebAssembly.instantiate`,
// `host.run`, `host.close`), exactly as `run.mjs` does for a fresh
// process: grants, programs, temp dirs and socket owners never leak
// across cases. Standard input is closed (`null`) when the case sends
// none, which matches the suite (no fixture reads real stdin; scripted
// input runs in-process). `process.exitCode` is never touched; the status
// returns in the frame. A case that kills the worker (an `exit` call, a
// crash, an uncaught throw) yields no frame: the driver restarts the
// worker and reports the case as a plain runtime failure, the same
// verdict a fresh process's non-zero exit gives. A hanging case blocks
// this worker as it would block a fresh process today.
import {
  closeSync,
  mkdtempSync,
  openSync,
  readSync,
  rmSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { entryCancel } from "./core.mts";
import { cycleReport } from "./checked-report.mts";
import { createHost } from "./node.mjs";
import { providers } from "./profiles.mjs";
import { configOf, supervise } from "./test-run.mjs";

const IN = 0;
const OUT = 1;

// Reads exactly `n` bytes from `fd`, or null on clean EOF before any
// byte. Throws on a truncated message.
function readExact(fd, n) {
  if (n === 0) {
    return Buffer.alloc(0);
  }
  const out = Buffer.alloc(n);
  let at = 0;
  while (at < n) {
    const got = readSync(fd, out, at, n - at, null);
    if (got === 0) {
      if (at === 0) {
        return null;
      }
      throw new Error("conformance worker: truncated message");
    }
    at += got;
  }
  return out;
}

function writeAll(fd, buf) {
  let at = 0;
  while (at < buf.length) {
    at += writeSync(fd, buf, at, buf.length - at, null);
  }
}

function readFrame() {
  const lenBuf = readExact(IN, 4);
  if (lenBuf === null) {
    return null;
  }
  const len = lenBuf.readUInt32BE(0);
  const headerBuf = readExact(IN, len);
  if (headerBuf === null) {
    throw new Error("conformance worker: truncated header");
  }
  const header = JSON.parse(headerBuf.toString("utf8"));
  const wasm = readExact(IN, header.wasmLen);
  if (wasm === null) {
    throw new Error("conformance worker: truncated wasm");
  }
  const stdinText = readExact(IN, header.stdinLen);
  if (stdinText === null) {
    throw new Error("conformance worker: truncated stdin");
  }
  return { header, wasm, stdinText };
}

function writeFrame(id, status, stdout, stderr) {
  const header = JSON.stringify({
    id,
    status,
    stdoutLen: stdout.length,
    stderrLen: stderr.length,
  });
  const headerBuf = Buffer.from(header, "utf8");
  const lenBuf = Buffer.alloc(4);
  lenBuf.writeUInt32BE(headerBuf.length, 0);
  writeAll(OUT, lenBuf);
  writeAll(OUT, headerBuf);
  writeAll(OUT, stdout);
  writeAll(OUT, stderr);
}

// The fixture runtime's cleanup bound, in milliseconds, for a host's
// cancellation of the entry (spec/conformance/README.md, Runtime
// Scenarios); `hd_run_wasmtime::scenario` uses the same.
const SCENARIO_CLEANUP_MS = 500;

// Drives one scenario case with a fresh host: the scenario's own steps
// over the scenario exports, not `run` to completion. Returns the status
// the verdict reads: 0 for a cleanup that ran, 3 for a competing or
// reentrant poll's trap (as a trapped run reports), 1 for any other
// failure.
async function runScenario(header, host, instance) {
  const ex = instance.exports;
  if (header.scenario === "cancellation-cleanup") {
    // The host cancels the entry as a Ctrl-C does once its first poll is
    // pending, and drives its cleanup under the bound; the run must end
    // cancelled.
    const cancel = entryCancel(SCENARIO_CLEANUP_MS);
    cancel.request();
    const { cancelled } = await host.run(instance, "hd.init", "hd.poll", { cancel });
    if (!cancelled) {
      return 1;
    }
    return ex["hd.scenario.cleanup"]() === 1 ? 0 : 1;
  }
  host.prime(instance);
  ex["hd.init"]();
  if (header.scenario === "competing-drivers") {
    if (ex["hd.poll"](0) !== -1) {
      return 1;
    }
    try {
      ex["hd.poll"](1);
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) {
        return 3;
      }
      throw e;
    }
    return 1;
  }
  if (header.scenario === "reentrant-poll") {
    if (ex["hd.poll"](0) !== -1) {
      return 1;
    }
    try {
      ex["hd.poll"](0);
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) {
        return 3;
      }
      throw e;
    }
    return 1;
  }
  throw new Error(`unknown runtime scenario '${header.scenario}'`);
}

// Runs one entry case with a fresh host and returns its status and its
// captured standard output and error, as `run.mjs` would write them.
async function runEntry(header, wasm, stdinText) {
  let stdoutStr = "";
  let stderrStr = "";
  let stdinFd = null;
  let stdinTmp = null;
  let host = null;
  try {
    if (header.stdinLen > 0) {
      stdinTmp = mkdtempSync(join(tmpdir(), "hd-conformance-stdin-"));
      const path = join(stdinTmp, "stdin");
      writeFileSync(path, stdinText);
      stdinFd = openSync(path, "r");
    }
    const module = await WebAssembly.compile(wasm);
    host = createHost(
      {
        out: (text) => {
          stdoutStr += text;
        },
        err: (text) => {
          stderrStr += text;
        },
      },
      {
        module,
        args: header.args ?? [],
        program: "",
        grants: {},
        providers: header.profile ? (io) => providers(header.profile, io) : null,
        stdin: stdinFd,
      },
    );
    const instance = await WebAssembly.instantiate(module, host.imports);
    if (header.scenario) {
      const status = await runScenario(header, host, instance);
      // Cancellation uses host.run, which already reports. The other
      // scenarios drive exports directly and report once here.
      if (header.scenario !== "cancellation-cleanup") {
        const report = cycleReport(module, instance);
        if (report !== null) {
          stderrStr += report;
        }
      }
      return { status, stdoutStr, stderrStr };
    }
    const { status } = await host.run(instance, "hd.init", "hd.poll");
    return { status, stdoutStr, stderrStr };
  } catch (e) {
    stderrStr += (e?.stack ?? String(e)) + "\n";
    return { status: 1, stdoutStr, stderrStr };
  } finally {
    try {
      host?.close();
    } catch {
      // Closing never fails a case.
    }
    if (stdinFd !== null) {
      try {
        closeSync(stdinFd);
      } catch {
        // Already gone.
      }
    }
    if (stdinTmp !== null) {
      try {
        rmSync(stdinTmp, { recursive: true, force: true });
      } catch {
        // Already gone.
      }
    }
  }
}

// Runs the listed test cases of a test program, each in a fresh instance
// in a supervised worker thread, and returns their result lines.
async function runTests(header, wasm) {
  let stdoutStr = "";
  try {
    const module = await WebAssembly.compile(wasm);
    await supervise(module, configOf(header.tests), (text) => {
      stdoutStr += text;
    });
    return { status: 0, stdoutStr, stderrStr: "" };
  } catch (e) {
    return { status: 1, stdoutStr, stderrStr: (e?.stack ?? String(e)) + "\n" };
  }
}

for (;;) {
  const msg = readFrame();
  if (msg === null) {
    break;
  }
  const { header, wasm, stdinText } = msg;
  const { status, stdoutStr, stderrStr } =
    typeof header.tests === "string"
      ? await runTests(header, wasm)
      : await runEntry(header, wasm, stdinText);
  writeFrame(header.id, status, Buffer.from(stdoutStr, "utf8"), Buffer.from(stderrStr, "utf8"));
}
