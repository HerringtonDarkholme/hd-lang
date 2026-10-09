// The default profile's sockets (spec/std/net.md): a worker thread that
// `core.mjs` starts on a program's first `Net` call, and that owns every
// socket the program opens. The program's thread posts each operation on
// `port` and waits on `flag`; this thread does it, posts the outcome back,
// and wakes it. So each operation finishes inside its `.start` call, as a
// `Process.run!` does: `read!`, `accept!` and `receive!` hold the program
// until a byte, a connection or a datagram arrives.
//
// An outcome is `{ ok }`, `{ error: [variant, text] }` for a `NetError`,
// or `{ disposed: true }` for an operation on a closed handle
// (std-net.handles.closable). The program's thread checks the host's form
// and the grant before it posts an operation that names an address.
import dgram from "node:dgram";
import { promises as dns } from "node:dns";
import net from "node:net";
import { workerData } from "node:worker_threads";
import { validHost } from "./core.mjs";

// Open handles by number; a closed one stays, marked, so each later
// operation on it is `Disposed`.
const handles = new Map();
let nextHandle = 1;
const open = (h) => {
  const n = nextHandle++;
  handles.set(n, h);
  return n;
};

const fail = (variant, text) => ({ error: [variant, text] });

// The `NetError` of a failed socket operation on `address`, or of a lookup
// of `host` (std-net.error.*).
const failure = (e, address, host) => {
  const code = e?.code ?? "";
  if (code === "ENOTFOUND" || code === "EAI_AGAIN" || code === "EAI_NODATA" || code === "EAI_FAIL") {
    return fail("Dns", host);
  }
  if (code === "ECONNREFUSED" || code === "EADDRINUSE") return fail("Refused", address);
  return fail("Other", String(e?.message ?? e));
};


// A queue of arrivals that a later operation waits for: data chunks, a
// connection, a datagram; or the end or an error, which stays.
class Inbox {
  constructor() {
    this.items = [];
    this.end = null;
    this.waiter = null;
  }
  push(x) {
    this.items.push(x);
    this.wake();
  }
  finish(why) {
    this.end ??= why;
    this.wake();
  }
  wake() {
    const w = this.waiter;
    this.waiter = null;
    w?.();
  }
  // The next arrival, or `null` once nothing more will come.
  async next() {
    while (!this.items.length && this.end === null) {
      await new Promise((r) => {
        this.waiter = r;
      });
    }
    return this.items.length ? this.items.shift() : null;
  }
}

// A TCP stream handle: its socket and the data it received.
const stream = (socket) => {
  const inbox = new Inbox();
  socket.on("data", (c) => inbox.push(c));
  socket.on("end", () => inbox.finish({ end: true }));
  socket.on("error", (e) => inbox.finish({ error: e }));
  socket.on("close", () => inbox.finish({ end: true }));
  return open({ kind: "stream", socket, inbox, closed: false, rest: null });
};

const ops = {
  lookup: async ({ host }) => {
    try {
      const found = await dns.lookup(host, { all: true, verbatim: true });
      return { ok: found.map((a) => a.address) };
    } catch (e) {
      return failure(e, host, host);
    }
  },
  connect: ({ host, port, address }) =>
    new Promise((done) => {
      const socket = net.connect({ host, port });
      socket.once("error", (e) => done(failure(e, address, host)));
      socket.once("connect", () => {
        socket.removeAllListeners("error");
        done({ ok: stream(socket) });
      });
    }),
  listen: ({ host, port, address }) =>
    new Promise((done) => {
      const server = net.createServer({ pauseOnConnect: false });
      const inbox = new Inbox();
      server.on("connection", (s) => inbox.push(s));
      server.once("error", (e) => done(failure(e, address, host)));
      server.listen(port, host, () => {
        server.removeAllListeners("error");
        server.on("error", (e) => inbox.finish({ error: e }));
        done({ ok: open({ kind: "listener", server, inbox, closed: false }) });
      });
    }),
  bind_udp: ({ host, port, address }) =>
    new Promise((done) => {
      const socket = dgram.createSocket(net.isIPv6(host) ? "udp6" : "udp4");
      const inbox = new Inbox();
      socket.on("message", (msg, rinfo) => inbox.push({ msg, rinfo }));
      socket.once("error", (e) => done(failure(e, address, host)));
      socket.bind(port, host, () => {
        socket.removeAllListeners("error");
        socket.on("error", (e) => inbox.finish({ error: e }));
        done({ ok: open({ kind: "udp", socket, inbox, closed: false }) });
      });
    }),
  // std-net.read: at least one byte, at most `max`; empty at the end.
  read: async ({ h, max }) => {
    if (max === 0) return { ok: new Uint8Array(0) };
    let chunk = h.rest;
    h.rest = null;
    if (chunk === null) {
      const got = await h.inbox.next();
      if (got === null) {
        const why = h.inbox.end;
        return why?.error ? failure(why.error, "", "") : { ok: new Uint8Array(0) };
      }
      chunk = got;
    }
    if (chunk.length > max) {
      h.rest = chunk.subarray(max);
      chunk = chunk.subarray(0, max);
    }
    return { ok: chunk };
  },
  // std-net.write: once every byte is written.
  write: ({ h, bytes }) =>
    new Promise((done) => {
      h.socket.write(Buffer.from(bytes), (e) => done(e ? failure(e, "", "") : { ok: null }));
    }),
  // std-net.accept: the next connection's stream.
  accept: async ({ h }) => {
    const socket = await h.inbox.next();
    if (socket === null) return failure(h.inbox.end?.error ?? "the listener stopped", "", "");
    return { ok: stream(socket) };
  },
  // std-net.send-to: one datagram.
  send_to: ({ h, host, port, address, bytes }) =>
    new Promise((done) => {
      h.socket.send(Buffer.from(bytes), port, host, (e) =>
        done(e ? failure(e, address, host) : { ok: null }),
      );
    }),
  // std-net.receive: one datagram, at most `max` of its bytes.
  receive: async ({ h, max }) => {
    const got = await h.inbox.next();
    if (got === null) return failure(h.inbox.end?.error ?? "the socket stopped", "", "");
    return { ok: { bytes: got.msg.subarray(0, max), host: got.rinfo.address, port: got.rinfo.port } };
  },
  close: ({ h }) => {
    if (h.kind === "stream") h.socket.end();
    else if (h.kind === "listener") h.server.close();
    else h.socket.close();
    h.closed = true;
    return { ok: null };
  },
};

const run = async (r) => {
  if (r.handle === undefined) return ops[r.op](r);
  const h = handles.get(r.handle);
  if (h === undefined) return { unknown: true };
  if (h.closed) return { disposed: true };
  return ops[r.op]({ ...r, h });
};

const { port, flag } = workerData;
port.on("message", async (r) => {
  let out;
  try {
    out = await run(r);
  } catch (e) {
    out = fail("Other", String(e?.message ?? e));
  }
  port.postMessage(out);
  Atomics.store(flag, 0, 1);
  Atomics.notify(flag, 0);
});
