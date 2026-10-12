// Long-lived Node worker for driver conformance entry runs (M39).
//
// One process per conformance worker thread, started once. It loads the
// host (`node.mjs`, `profiles.mjs`) once and takes cases over stdin, one
// framed message per case: a 4-byte big-endian header length, the header
// JSON, then the wasm bytes and the stdin text. The header is
// `{"id","wasmLen","profile","args","stdinLen"}`: the case's serial as
// `id`, the wasm byte count, the fixture's runtime profile name or null,
// the program arguments (always empty in conformance) and the stdin byte
// count (always empty in conformance, accepted for protocol completeness).
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
import { createHost } from "./node.mjs";
import { providers } from "./profiles.mjs";

const IN = 0;
const OUT = 1;

// Reads exactly `n` bytes from `fd`, or null on clean EOF before any
// byte. Throws on a truncated message.
function readExact(fd, n) {
  if (n === 0) return Buffer.alloc(0);
  const out = Buffer.alloc(n);
  let at = 0;
  while (at < n) {
    const got = readSync(fd, out, at, n - at, null);
    if (got === 0) {
      if (at === 0) return null;
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
  if (lenBuf === null) return null;
  const len = lenBuf.readUInt32BE(0);
  const headerBuf = readExact(IN, len);
  if (headerBuf === null) throw new Error("conformance worker: truncated header");
  const header = JSON.parse(headerBuf.toString("utf8"));
  const wasm = readExact(IN, header.wasmLen);
  if (wasm === null) throw new Error("conformance worker: truncated wasm");
  const stdinText = readExact(IN, header.stdinLen);
  if (stdinText === null) throw new Error("conformance worker: truncated stdin");
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
        args: header.args ?? [],
        program: "",
        grants: {},
        providers: header.profile ? (io) => providers(header.profile, io) : null,
        stdin: stdinFd,
      },
    );
    const { instance } = await WebAssembly.instantiate(wasm, host.imports);
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

for (;;) {
  const msg = readFrame();
  if (msg === null) break;
  const { header, wasm, stdinText } = msg;
  const { status, stdoutStr, stderrStr } = await runEntry(header, wasm, stdinText);
  writeFrame(
    header.id,
    status,
    Buffer.from(stdoutStr, "utf8"),
    Buffer.from(stderrStr, "utf8"),
  );
}
