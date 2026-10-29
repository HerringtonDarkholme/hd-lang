// Guest stdin belongs to an I/O worker, including under synchronous rt.block.
// Like Net, reads reserve bytes until guest finish; abort restores reservations.
import { postReply } from "./worker-control.mjs";
import { createReadStream, fstatSync } from "node:fs";
import { Socket } from "node:net";
import { isatty, ReadStream } from "node:tty";
import { workerData } from "node:worker_threads";

const operations = new Map();
let nextOperation = 1;
let source = null;
let initialized = false;
let ended = false;
let failure = null;
let offset = 0;
let available = [];

const take = (state, length) => {
  const item = available[0];
  state.parts.push({ offset: item.offset, bytes: item.bytes.subarray(0, length) });
  if (length === item.bytes.length) available.shift();
  else available[0] = { offset: item.offset + length, bytes: item.bytes.subarray(length) };
};
const serve = () => {
  for (const state of operations.values()) {
    if (state.result !== null) continue;
    let newline = false;
    while (available.length) {
      const bytes = available[0].bytes;
      const nl = state.kind === "line" ? bytes.indexOf(10) : -1;
      take(state, nl === -1 ? bytes.length : nl + 1);
      if (nl !== -1) {
        newline = true;
        break;
      }
    }
    if (failure !== null) state.result = { error: failure };
    else if (newline || ended) {
      let bytes = Buffer.concat(state.parts.map((part) => part.bytes));
      if (state.kind === "line") {
        if (newline) {
          bytes = bytes.subarray(0, bytes.length - 1);
          if (bytes.at(-1) === 13) bytes = bytes.subarray(0, bytes.length - 1);
        }
        state.result = { ok: !newline && !bytes.length ? null : bytes.toString("utf8") };
      } else state.result = { ok: bytes.toString("utf8").replaceAll("\r\n", "\n") };
    }
  }
  // Do not read ahead without a pending call; retain already buffered bytes.
  if ([...operations.values()].some((state) => state.result === null)) source?.resume();
  else source?.pause();
};

function open(fd) {
  if (initialized) return;
  initialized = true;
  if (fd === null) {
    ended = true;
    return;
  }
  try {
    const stat = fstatSync(fd);
    // This is Node's stdio split, without a worker's redirected process.stdin.
    source = isatty(fd)
      ? new ReadStream(fd)
      : stat.isFIFO() || stat.isSocket()
        ? new Socket({ fd, readable: true, writable: false })
        : createReadStream(null, { fd, autoClose: false });
    source.on("data", (bytes) => {
      available.push({ offset, bytes });
      offset += bytes.length;
      serve();
    });
    source.on("end", () => {
      ended = true;
      serve();
    });
    source.on("error", (error) => {
      failure = Number.isInteger(error?.errno) ? error.errno : 0;
      serve();
    });
    source.pause();
  } catch (error) {
    failure = Number.isInteger(error?.errno) ? error.errno : 0;
  }
}

const run = (request) => {
  switch (request.op) {
    case "wait_start": {
      open(request.fd);
      const operation = nextOperation++;
      operations.set(operation, { kind: request.kind, parts: [], result: null });
      serve();
      return { ok: operation };
    }
    case "wait_poll": {
      const state = operations.get(request.operation);
      if (!state) throw new Error("unknown input wait");
      return state.result ?? { pending: true };
    }
    case "wait_finish": {
      const state = operations.get(request.operation);
      if (!state || state.result === null) throw new Error("unfinished input wait");
      operations.delete(request.operation);
      return { ok: null };
    }
    case "wait_abort": {
      const state = operations.get(request.operation);
      if (state) {
        operations.delete(request.operation);
        available = available.concat(state.parts);
        available.sort((a, b) => a.offset - b.offset);
        serve();
      }
      return { ok: null };
    }
    default:
      throw new Error("unknown input control call");
  }
};

const { port, flag } = workerData;
port.on("message", ({ id, request }) => {
  const result = run(request);
  postReply(port, flag, id, result);
});
