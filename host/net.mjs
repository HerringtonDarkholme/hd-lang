// The default profile's sockets (spec/std/net.md): a worker thread that
// `node.mjs` starts on a program's first `Net` call, and that owns every
// socket the program opens. The program's thread posts each operation on
// `port` and waits on `flag`; this thread does it, posts the outcome back,
// and wakes it. So each operation finishes inside its `.start` call, as a
// `Process.run!` does: `read!`, `accept!` and `receive!` hold the program
// until a byte, a connection or a datagram arrives.
//
// A handle's timeout (std-net.timeout.set) bounds its `read!`, `accept!`
// and `receive!` with a timer, cleared when the wait completes: a wait it
// ends is `Timeout`, takes nothing, and leaves every arrival for the next
// call (std-net.timeout.error). A zero or negative timeout returns what
// already arrived and otherwise ends at once (std-net.timeout.negative).
// `connect_timeout!` bounds a connect the same way.
//
// An outcome is `{ ok }`, `{ error: [variant, text] }` for a `NetError`,
// or `{ disposed: true }` for an operation on a closed handle
// (std-net.handles.closable). The program's thread checks the host's form
// and the grant before it posts an operation that accesses an address;
// a metadata-only check may first reject a closed or unknown handle.
import dgram from "node:dgram";
import { promises as dns } from "node:dns";
import net from "node:net";
import { workerData } from "node:worker_threads";
import { validHost } from "./node.mjs";

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
const timedOut = () => ({ error: ["Timeout"] });

// What `Inbox.next` returns when its bound ends first.
const TIMED_OUT = Symbol("timed out");

// The `NetError` of a failed socket operation on `address`, or of a lookup
// of `host` (std-net.error.*).
const failure = (e, address, host) => {
  const code = e?.code ?? "";
  if (
    code === "ENOTFOUND" ||
    code === "EAI_AGAIN" ||
    code === "EAI_NODATA" ||
    code === "EAI_FAIL"
  ) {
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
  // The next arrival, or `null` once nothing more will come, or
  // `TIMED_OUT` once `ms` milliseconds pass first (`null`: no bound).
  async next(ms = null) {
    const ready = () => this.items.length || this.end !== null;
    if (ms !== null && ms <= 0 && !ready()) return TIMED_OUT;
    let late = false;
    const timer =
      ms === null
        ? null
        : setTimeout(() => {
            late = true;
            this.wake();
          }, ms);
    try {
      while (!ready()) {
        if (late) return TIMED_OUT;
        await new Promise((r) => {
          this.waiter = r;
        });
      }
    } finally {
      clearTimeout(timer);
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
  return open({ kind: "stream", socket, inbox, closed: false, rest: null, timeout: null });
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
  // std-net.connect-timeout: `timeout` (milliseconds, or `null` for
  // `connect!`) bounds the whole connect; past it, the socket closes.
  connect: ({ host, port, address, timeout = null }) =>
    new Promise((done) => {
      if (timeout !== null && timeout <= 0) return done(timedOut());
      const socket = net.connect({ host, port });
      const timer =
        timeout === null
          ? null
          : setTimeout(() => {
              socket.removeAllListeners();
              socket.on("error", () => {});
              socket.destroy();
              done(timedOut());
            }, timeout);
      socket.once("error", (e) => {
        clearTimeout(timer);
        done(failure(e, address, host));
      });
      socket.once("connect", () => {
        clearTimeout(timer);
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
        done({ ok: open({ kind: "listener", server, inbox, closed: false, timeout: null }) });
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
        done({ ok: open({ kind: "udp", socket, inbox, closed: false, timeout: null }) });
      });
    }),
  // std-net.read: at least one byte, at most `max`; empty at the end.
  read: async ({ h, max: signed }) => {
    // `usize` crosses the ABI as an i32 bit pattern, so read it unsigned.
    const max = signed >>> 0;
    if (max === 0) return { ok: new Uint8Array(0) };
    let chunk = h.rest;
    h.rest = null;
    if (chunk === null) {
      const got = await h.inbox.next(h.timeout);
      if (got === TIMED_OUT) return timedOut();
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
    const socket = await h.inbox.next(h.timeout);
    if (socket === TIMED_OUT) return timedOut();
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
  receive: async ({ h, max: signed }) => {
    const max = signed >>> 0;
    const got = await h.inbox.next(h.timeout);
    if (got === TIMED_OUT) return timedOut();
    if (got === null) return failure(h.inbox.end?.error ?? "the socket stopped", "", "");
    return {
      ok: { bytes: got.msg.subarray(0, max), host: got.rinfo.address, port: got.rinfo.port },
    };
  },
  // std-net.timeout.set: milliseconds, or `null` for no bound.
  set_timeout: ({ h, timeout }) => {
    h.timeout = timeout;
    return { ok: null };
  },
  // std-net.option.nodelay: `TCP_NODELAY` on or off; a refusal carries the
  // system's message (std-net.option.failure).
  set_nodelay: ({ h, enabled }) => {
    try {
      h.socket.setNoDelay(enabled);
      return { ok: null };
    } catch (e) {
      return fail("Other", e.message);
    }
  },
  // std-net.option.keepalive: `idle` milliseconds before the first probe,
  // or `null` for off. The system counts whole seconds, so a shorter idle
  // rounds up to one second, as on wasmtime.
  set_keepalive: ({ h, idle }) => {
    try {
      if (idle === null) h.socket.setKeepAlive(false);
      else h.socket.setKeepAlive(true, Math.max(1, Math.ceil(idle / 1000)) * 1000);
      return { ok: null };
    } catch (e) {
      return fail("Other", e.message);
    }
  },
  // std-net.local-address: the bound IP address as text and the port, which
  // is the one the system picked for port 0.
  local_address: ({ h }) => {
    try {
      const { address, port } = (h.kind === "listener" ? h.server : h.socket).address();
      return { ok: [address, port] };
    } catch (e) {
      return fail("Other", e.message);
    }
  },
  close: ({ h }) => {
    // std-net.close.release: a full close of both directions.
    if (h.kind === "stream") h.socket.destroy();
    else if (h.kind === "listener") h.server.close();
    else h.socket.close();
    h.closed = true;
    return { ok: null };
  },
  check: () => ({ ok: null }),
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
