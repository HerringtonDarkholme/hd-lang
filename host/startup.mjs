// The Node startup selector. Programs whose only host imports are the two
// panic-reporting leaves run without loading the provider host; every other
// program gets node.mjs unchanged.
import { CANCEL_STATUS, HostContract, PROGRAM_TIME_LIMIT } from "./run-support.mts";

const decoder = new TextDecoder("utf-8", { ignoreBOM: true });
const wasmFrames = (error) =>
  Array.from(
    String(error.stack ?? "").matchAll(/wasm-function\[(\d+)\]:0x([0-9a-f]+)/g),
    (match) => [Number(match[1]), parseInt(match[2], 16)],
  );
const panicOfReport = (report) => {
  const text = report.endsWith("\n") ? report.slice(0, -1) : report;
  const colon = text.indexOf(": ");
  return colon < 0
    ? { category: text, message: "" }
    : { category: text.slice(0, colon), message: text.slice(colon + 2) };
};

const startupImports = (module) =>
  WebAssembly.Module.imports(module).every(
    ({ module, name, kind }) =>
      kind === "function" && module === "hd:rt" && (name === "panic" || name === "stderr"),
  );

function createStartupOnlyHost(sink, deadline) {
  let memory = null;
  let reported = false;
  let panic = null;
  const imports = {
    "hd:rt": {
      stderr(len) {
        reported = true;
        const text = decoder.decode(new Uint8Array(memory.buffer, 0, len));
        sink.err(text);
        if (panic !== null) {
          panic += text;
        }
      },
      panic() {
        reported = true;
        sink.err("panic: ");
        panic = "";
      },
    },
  };
  const prime = (instance) => {
    memory = instance.exports["hd.x"] ?? null;
  };
  const run = async (instance, init, poll) => {
    prime(instance);
    let status = 0;
    let trapped = false;
    let cancelled = false;
    let frames = [];
    try {
      instance.exports[init]();
      status = instance.exports[poll](0);
      if (typeof status !== "number" || (status | 0) !== status) {
        throw new HostContract("root poll returned an invalid status");
      }
      if (status === -1) {
        sink.err("panic: deadlock: the program waits, but no host operation is pending\n");
        panic = "deadlock: the program waits, but no host operation is pending\n";
        status = 3;
        trapped = true;
      } else if (status === -2 || status === -3) {
        sink.err(status === -2 ? "cancelled\n" : "cancelled, cleanup abandoned\n");
        status = CANCEL_STATUS;
        cancelled = true;
      }
    } catch (error) {
      if (error instanceof HostContract) {
        sink.err(`panic: host-contract: ${error.message}\n`);
        panic = `host-contract: ${error.message}\n`;
      } else if (error instanceof RangeError && /call stack size/i.test(error.message)) {
        sink.err(
          "panic: stack-exhausted: the call stack ran out; check for recursion that never ends\n",
        );
        panic = "stack-exhausted: the call stack ran out; check for recursion that never ends\n";
        frames = wasmFrames(error);
      } else if (error instanceof WebAssembly.RuntimeError) {
        frames = wasmFrames(error);
        if (!reported) {
          sink.err(`internal error: ${error.message}\n`);
        }
      } else {
        throw error;
      }
      status = 3;
      trapped = true;
    }
    if (deadline !== null && performance.timeOrigin + performance.now() > deadline) {
      sink.err(PROGRAM_TIME_LIMIT);
      panic = PROGRAM_TIME_LIMIT.slice("panic: ".length);
      status = 3;
      trapped = true;
      frames = [];
    }
    return {
      status,
      trapped,
      ended: false,
      frames,
      panic: trapped && panic !== null ? panicOfReport(panic) : null,
      cancelled: cancelled && !trapped,
    };
  };
  return { imports, run, prime, close() {}, leaks: () => [] };
}

export async function createStartupHost(module, sink, env = {}) {
  const cycles = WebAssembly.Module.customSections(module, "hd.cycles").length;
  if (
    startupImports(module) &&
    cycles === 0 &&
    !env.providers &&
    !env.packageBindings &&
    !env.processLauncher
  ) {
    return createStartupOnlyHost(sink, env.deadline ?? null);
  }
  const { createHost } = await import("./node.mjs");
  return createHost(sink, { ...env, module });
}
