// The JS host of `hd run` on V8 (Node): the import object of the ABI in
// `hd_host_abi` (runtime-and-host.md §17.2, §17.10) for what the compiler
// emits today, then the module's `main` export.
//
// - `hd:rt` `stderr(len)` writes the exchange buffer's first `len` bytes
//   to standard error; `block()` waits for a host completion (none is ever
//   pending here, so it returns 0).
// - `hd:Console` `write_line.start(len)` writes one line of output and
//   completes at once: status 0, with `.Ok(())` (variant 0) encoded in
//   the buffer, length 1. `write_line.finish(h)` is never reached.
// A trap is a panic: the run exits with status 3 (never 101,
// module.profile.panic-status).
import { readFileSync, writeSync } from "node:fs";

const bytes = readFileSync(process.argv[2]);
const out = [];
let memory = null;
const view = (len) => new Uint8Array(memory.buffer, 0, len);
const flush = () => {
  if (out.length) {
    writeSync(1, out.join(""));
    out.length = 0;
  }
};
const imports = {
  "hd:rt": {
    stderr: (len) => {
      reported = true;
      flush();
      writeSync(2, Buffer.from(view(len)));
    },
    block: () => 0,
  },
  "hd:Console": {
    "write_line.start": (len) => {
      out.push(Buffer.from(view(len)).toString("utf8") + "\n");
      view(1)[0] = 0;
      return [0, 1];
    },
    "write_line.finish": (_h) => {
      view(1)[0] = 0;
      return 1;
    },
  },
};
const t0 = performance.now();
const { instance } = await WebAssembly.instantiate(bytes, imports);
memory = instance.exports["hd.x"] ?? null;
const t1 = performance.now();
let status = 0;
let reported = false;
try {
  instance.exports.main();
} catch (e) {
  flush();
  if (!(e instanceof WebAssembly.RuntimeError)) throw e;
  // A panic stub wrote its report; any other trap is an internal error.
  if (!reported) process.stderr.write(`internal error: ${e.message}\n`);
  status = 3;
}
const t2 = performance.now();
flush();
process.stderr.write(`instantiate_ms=${(t1 - t0).toFixed(3)} run_ms=${(t2 - t1).toFixed(3)}\n`);
process.exitCode = status;
