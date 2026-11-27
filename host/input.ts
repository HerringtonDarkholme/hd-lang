// Guest stdin belongs to an I/O worker, including under synchronous rt.block.
// Like Net, reads reserve bytes until guest finish; abort restores reservations.
import { postReply, notifyResult } from "./worker-control.ts";
import { createReadStream, fstatSync } from "node:fs";
import { Socket } from "node:net";
import { isatty, ReadStream } from "node:tty";
import { workerData } from "node:worker_threads";

type Dynamic = ReturnType<typeof JSON.parse>;
interface Available {
  offset: number;
  bytes: Buffer;
}
// "bytes": the next bytes as read, at most `max`, as soon as any arrive
// (std-console.stdin.exact); "line" and "all" decode text.
interface InputState {
  kind: "line" | "all" | "bytes";
  max: number;
  parts: Available[];
  result: Dynamic | null;
}

const operations = new Map<number, InputState>();
const waiting = new Set<InputState>();
let nextOperation = 1;
let source: Dynamic = null;
let initialized = false;
let ended = false;
let failure: number | null = null;
let offset = 0;
let available: Available[] = [];

const take = (state: InputState, length: number): void => {
  const item = available[0];
  state.parts.push({ offset: item.offset, bytes: item.bytes.subarray(0, length) });
  if (length === item.bytes.length) {
    available.shift();
  } else {
    available[0] = { offset: item.offset + length, bytes: item.bytes.subarray(length) };
  }
};
const complete = (state: InputState, newline: boolean): void => {
  let bytes = Buffer.concat(state.parts.map((part: Available) => part.bytes));
  if (state.kind === "bytes") {
    state.result = { ok: new Uint8Array(bytes) };
    return;
  }
  if (state.kind === "line") {
    if (newline) {
      bytes = bytes.subarray(0, bytes.length - 1);
      if (bytes.at(-1) === 13) {
        bytes = bytes.subarray(0, bytes.length - 1);
      }
    }
    state.result = { ok: !newline && !bytes.length ? null : bytes.toString("utf8") };
  } else {
    state.result = { ok: bytes.toString("utf8") };
  }
};
const canServe = (): boolean => available.length > 0 || ended || failure !== null;
// A "bytes" read: what is buffered, at most `max`, at once; it always
// completes. cost: O(the bytes taken).
const serveBytes = (state: InputState): void => {
  let left = state.max;
  while (available.length && left > 0) {
    const length = Math.min(left, available[0].bytes.length);
    take(state, length);
    left -= length;
  }
  if (failure !== null && !state.parts.length) {
    state.result = { error: failure };
  } else {
    complete(state, false);
  }
};
// A "line" or "all" read: completes at a newline (a line), the end, or a
// failure; otherwise it keeps what it took and waits.
const serveText = (state: InputState): void => {
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
  if (failure !== null) {
    state.result = { error: failure };
  } else if (newline || ended) {
    complete(state, newline);
  }
};
const serve = (): void => {
  for (const state of waiting) {
    if (!canServe()) {
      break;
    }
    if (state.kind === "bytes") {
      serveBytes(state);
    } else {
      serveText(state);
    }
    if (state.result !== null) {
      waiting.delete(state);
    }
  }
  // Do not read ahead without a pending call; retain already buffered bytes.
  if (waiting.size) {
    source?.resume();
  } else {
    source?.pause();
  }
};

function open(fd: number | null): void {
  if (initialized) {
    return;
  }
  initialized = true;
  if (fd === null) {
    ended = true;
    return;
  }
  try {
    const stat = fstatSync(fd);
    // This is Node's stdio split, without a worker's redirected process.stdin.
    if (isatty(fd)) {
      source = new ReadStream(fd);
    } else if (stat.isFIFO() || stat.isSocket()) {
      source = new Socket({ fd, readable: true, writable: false });
    } else {
      source = createReadStream(null as Dynamic, { fd, autoClose: false });
    }
    source.on("data", (bytes: Buffer) => {
      available.push({ offset, bytes });
      offset += bytes.length;
      serve();
    });
    source.on("end", () => {
      ended = true;
      serve();
    });
    source.on("error", (error: NodeJS.ErrnoException) => {
      failure = Number.isInteger(error?.errno) ? error.errno! : 0;
      serve();
    });
    source.pause();
  } catch (error) {
    failure = Number.isInteger((error as NodeJS.ErrnoException)?.errno)
      ? (error as NodeJS.ErrnoException).errno!
      : 0;
  }
}

const run = (request: Dynamic): Dynamic => {
  switch (request.op) {
    case "wait_start": {
      open(request.fd);
      const operation = nextOperation++;
      const state = notifyResult<InputState>(
        { kind: request.kind, max: request.max ?? 0, parts: [], result: null },
        workerData as Dynamic,
        `wait:${operation}`,
      );
      operations.set(operation, state);
      waiting.add(state);
      serve();
      return { ok: operation };
    }
    case "wait_poll": {
      const state = operations.get(request.operation);
      if (!state) {
        throw new Error("unknown input wait");
      }
      return state.result ?? { pending: true };
    }
    case "wait_finish": {
      const state = operations.get(request.operation);
      if (!state || state.result === null) {
        throw new Error("unfinished input wait");
      }
      operations.delete(request.operation);
      return { ok: null };
    }
    case "wait_abort": {
      const state = operations.get(request.operation);
      if (state) {
        operations.delete(request.operation);
        waiting.delete(state);
        available = available.concat(state.parts);
        available.sort((left, right) => left.offset - right.offset);
        serve();
      }
      return { ok: null };
    }
    default:
      throw new Error("unknown input control call");
  }
};

const { port, flag } = workerData as Dynamic;
port.on("message", ({ id, request }: { id: number; request: Dynamic }) => {
  const result = run(request);
  postReply(port, flag, id, result);
});
