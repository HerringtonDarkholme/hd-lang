// The long-lived Node host of a stage test process (`hd_testkit::stages`'
// `stdout` view, #1975). One process serves every program run of the test
// target, one request at a time, so the target pays Node's start and the
// host's module loading once instead of once per case.
//
// A request is a 4-byte big-endian header length, the header JSON
// `{"wasmLen","args"}`, then the Wasm bytes. The answer is a 4-byte
// big-endian header length, the header JSON
// `{"exit","stdoutLen","stderrLen"}`, then the standard output and error
// bytes. A header with `tests`, the `hd test` case list
// `test:init:kind,...`, runs those cases of a test program as `test.ts`
// does (`test-run.ts`, a fresh worker thread per program); its standard
// output is their result records, one JSON object per line, without the
// fields that change from run to run (`us`, the time, and `frames`, the
// Wasm offsets of a trap).
//
// Each run is `run.ts`'s lone run without a configuration (`runHere`): a
// new compile, host, instance and memory, closed standard input, no grants,
// no time limit. So its status and output are the ones a lone
// `node host/run.ts PROGRAM` gives, and nothing of one run is visible to
// the next. `exit` is the status as a process reports it, its low 8 bits.
// A host failure that a lone run would leave uncaught answers status 1 with
// the error in its standard error. A run that never returns, or a crash of
// this process, leaves the request unanswered; the driver restarts the
// process and fails that run alone.
import { runHere } from "./run-here.ts";
import { configOf, prestart, supervise } from "./test-run.ts";
import { Ok } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

const { readSync, writeSync } = process.getBuiltinModule("node:fs");

const IN = 0;
const OUT = 1;

// Reads exactly `count` bytes of standard input, or null at its end before any
// byte. A request cut short is the driver's bug.
function readExact(count: number): Buffer<ArrayBuffer> | null {
  const out = Buffer.alloc(count);
  let at = 0;
  while (at < count) {
    const got = readSync(IN, out, at, count - at, null);
    if (got === 0) {
      if (at === 0) {
        return null;
      }
      throw new Error("stage host: truncated request");
    }
    at += got;
  }
  return out;
}

function writeAll(bytes: Uint8Array): void {
  for (let at = 0; at < bytes.length;) {
    at += writeSync(OUT, bytes, at, bytes.length - at, null);
  }
}

async function runOne(wasm: Buffer<ArrayBuffer>, args: string[]): Promise<[number, string, string]> {
  let stdout = "";
  let stderr = "";
  const sink = {
    out: (text: string) => {
      stdout += text;
    },
    err: (text: string) => {
      stderr += text;
    },
    // A console write takes every byte at once, as a pipe with room does.
    write: (fd: number, text: string) => {
      if (fd === 1) {
        stdout += text;
      } else {
        stderr += text;
      }
      return Ok(undefined);
    },
  };
  let status;
  try {
    status = await runHere({ module: wasm, config: {}, args, sink, stdin: null });
  } catch (error) {
    stderr += `${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`;
    status = 1;
  }
  return [status & 0xff, stdout, stderr];
}

async function runTests(wasm: Buffer<ArrayBuffer>, tests: string): Promise<[number, string, string]> {
  let records = "";
  try {
    const module = await WebAssembly.compile(wasm);
    await supervise(module, configOf(tests), (text) => {
      records += text;
    });
  } catch (error) {
    return [1, "", `${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`];
  }
  const stable = records
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => {
      const { us: _us, frames: _frames, ...record } = JSON.parse(line);
      return `${JSON.stringify(record)}\n`;
    });
  return [0, stable.join(""), ""];
}

for (;;) {
  const length = readExact(4);
  if (length === null) {
    break;
  }
  const header: Dynamic = JSON.parse(readExact(length.readUInt32BE(0))!.toString("utf8"));
  const wasm = readExact(header.wasmLen) ?? Buffer.alloc(0);
  const [exit, stdout, stderr] =
    typeof header.tests === "string"
      ? await runTests(wasm, header.tests)
      : await runOne(wasm, header.args ?? []);
  // After a test program, the next one's worker thread loads while the
  // driver builds it.
  if (typeof header.tests === "string") {
    prestart();
  }
  const out = Buffer.from(stdout, "utf8");
  const err = Buffer.from(stderr, "utf8");
  const answer = Buffer.from(
    JSON.stringify({ exit, stdoutLen: out.length, stderrLen: err.length }),
    "utf8",
  );
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32BE(answer.length, 0);
  writeAll(Buffer.concat([prefix, answer, out, err]));
}
// A program's leftover timer or handle must not keep this process alive
// once the driver has closed its end.
process.exit(0);
