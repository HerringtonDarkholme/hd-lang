// The default profile's sockets (spec/std/net.md): a worker thread that
// `node.mjs` starts on a program's first `Net` call, and that owns every
// socket the program opens. The program's thread posts each operation on
// `port` and waits on `flag`; control replies never wait for network input.
// Connect, read, accept and receive have start/poll/finish/abort lifetimes,
// so a pending socket does not hold the program's polling thread.
//
// A handle's timeout (std-net.timeout.set) bounds its `read!`, `accept!`
// and `receive!` with a timer, cleared when the wait completes: a wait it
// ends is `Timeout`, takes nothing, and leaves every arrival for the next
// call (std-net.timeout.error). A zero or negative timeout returns what
// already arrived after a non-waiting I/O poll, and otherwise ends at
// once (std-net.timeout.zero).
// A composed deadline cancels an unfinished wait through its abort hook.
//
// An outcome is `{ ok }`, `{ error: [variant, text] }` for a `NetError`,
// or `{ disposed: true }` for an operation on a closed handle, which the
// program's thread reports as `NetError.Closed`
// (std-net.handles.closed). The program's thread checks the host's form
// and the grant before it posts an operation that accesses an address;
// a metadata-only check may first reject a closed or unknown handle.
import { serve } from "./worker-control.mts";
import dgram from "node:dgram";
import { promises as dns } from "node:dns";
import net from "node:net";
import { setImmediate as nextCheck } from "node:timers/promises";
import { workerData } from "node:worker_threads";

// Open handles by number; a closed one stays, marked, so each later
// operation on it is `Closed`.
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
const CANCELLED = Symbol("cancelled");

async function immediateResult(ms, signal, ready) {
  if (ms === null || ms > 0 || ready()) {
    return null;
  }
  await nextCheck();
  await nextCheck();
  if (signal?.aborted) {
    return CANCELLED;
  }
  return ready() ? null : TIMED_OUT;
}

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
  if (code === "ECONNREFUSED" || code === "EADDRINUSE") {
    return fail("Refused", address);
  }
  return fail("Other", String(e?.message ?? e));
};

// A queue of arrivals that a later operation waits for: data chunks, a
// connection, a datagram; or the end or an error, which stays.
class Inbox {
  constructor() {
    this.items = [];
    this.nextItem = 0;
    this.end = null;
    this.waiters = new Set();
  }
  push(x) {
    this.items.push({ value: x, order: this.nextItem++, offset: 0 });
    this.wake();
  }
  restore(item) {
    // A chunk can be split by several concurrent reads. Restoring their
    // reservations in either abort order must keep the original byte order.
    const at = this.items.findIndex(
      (other) =>
        other.order > item.order || (other.order === item.order && other.offset > item.offset),
    );
    this.items.splice(at === -1 ? this.items.length : at, 0, item);
    this.wake();
  }
  finish(why) {
    this.end ??= why;
    this.wake();
  }
  wake() {
    const waiters = [...this.waiters];
    this.waiters.clear();
    for (const wake of waiters) {
      wake();
    }
  }
  // The next arrival, or `null` once nothing more will come, or
  // `TIMED_OUT` once `ms` milliseconds pass first (`null`: no bound).
  async next(ms = null, signal = null) {
    const ready = () => this.items.length || this.end !== null;
    const immediate = await immediateResult(ms, signal, ready);
    if (immediate !== null) {
      return immediate;
    }
    let late = false;
    const timer =
      ms === null
        ? null
        : setTimeout(() => {
            late = true;
            this.wake();
          }, ms);
    const abort = () => this.wake();
    signal?.addEventListener("abort", abort, { once: true });
    try {
      while (!ready()) {
        if (signal?.aborted) {
          return CANCELLED;
        }
        if (late) {
          return TIMED_OUT;
        }
        await new Promise((r) => {
          this.waiters.add(r);
        });
      }
      if (signal?.aborted) {
        return CANCELLED;
      }
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
    }
    return this.items.length ? this.items.shift() : null;
  }
}

// How many received bytes a stream holds before it stops reading from the
// socket. Reading pauses there, so a fast peer meets backpressure from the
// hd reader instead of filling this process's memory (std-net.read).
const READ_AHEAD = 256 * 1024;
const queued = (inbox) => inbox.items.reduce((n, item) => n + item.value.length, 0);

// A TCP stream handle: its socket and the data it received.
const stream = (socket) => {
  const inbox = new Inbox();
  socket.on("data", (c) => {
    inbox.push(c);
    if (queued(inbox) >= READ_AHEAD) {
      socket.pause();
    }
  });
  socket.on("end", () => inbox.finish({ end: true }));
  socket.on("error", (e) => inbox.finish({ error: e }));
  socket.on("close", () => inbox.finish({ end: true }));
  return open({ kind: "stream", socket, inbox, closed: false, timeout: null });
};

// A connect belongs to its operation until the program takes its outcome.
// Aborting also releases a completed stream that was never delivered.
const connects = new Map();
let nextConnect = 1;
// A wait owns its consumed input until finish, including the gap between
// poll reporting readiness and the guest taking its outcome. Abort restores
// undelivered bytes/datagrams, or releases an undelivered accepted stream.
const waits = new Map();
let nextWait = 1;
const beginConnect = ({ host, port, address }, done) => {
  let result = null;
  let stopped = false;
  let socket = null;
  const finish = (outcome) => {
    if (stopped || result !== null) {
      return;
    }
    result = outcome;
    done(outcome);
  };
  socket = net.connect({ host, port });
  socket.once("error", (e) => finish(failure(e, address, host)));
  socket.once("connect", () => {
    if (stopped || result !== null) {
      return socket.destroy();
    }
    socket.removeAllListeners("error");
    finish({ ok: stream(socket) });
  });
  return () => {
    stopped = true;
    socket?.destroy();
    if (result?.ok !== undefined) {
      handles.delete(result.ok);
    }
  };
};

const ops = {
  wait_start: ({ h, kind, ...request }) => {
    if (
      !["read", "accept", "receive", "lookup", "listen", "bind_udp", "write", "send_to"].includes(
        kind,
      )
    ) {
      throw new Error("unknown socket wait");
    }
    const id = nextWait++;
    const state = { h, result: null, undo: null, controller: new AbortController() };
    waits.set(id, state);
    const retain = (undo) => {
      if (state.controller.signal.aborted) {
        undo();
      } else {
        state.undo = undo;
      }
    };
    Promise.resolve()
      .then(() => ops[kind]({ ...request, h, signal: state.controller.signal, retain }))
      .then(
        (result) => {
          if (!state.controller.signal.aborted) {
            state.result = result;
          }
        },
        (error) => {
          if (!state.controller.signal.aborted) {
            state.result = failure(error, request.address ?? "", request.host ?? "");
          }
        },
      );
    return { ok: id };
  },
  wait_poll: ({ operation }) => {
    const state = waits.get(operation);
    if (state === undefined) {
      throw new Error("unknown socket wait");
    }
    if (state.h?.closed) {
      state.undo?.();
      state.undo = null;
      return { disposed: true };
    }
    return state.result ?? { pending: true };
  },
  wait_finish: ({ operation }) => {
    const state = waits.get(operation);
    if (state === undefined) {
      throw new Error("unknown socket wait");
    }
    if (state.result === null && !state.h?.closed) {
      throw new Error("unfinished socket wait");
    }
    if (state.h?.closed) {
      state.undo?.();
    }
    state.undo = null;
    waits.delete(operation);
    return state.h?.closed ? { disposed: true } : { ok: null };
  },
  wait_abort: ({ operation }) => {
    const state = waits.get(operation);
    if (state !== undefined) {
      state.controller.abort();
      state.undo?.();
      waits.delete(operation);
    }
    return { ok: null };
  },
  lookup: async ({ host }) => {
    try {
      const found = await dns.lookup(host, { all: true, verbatim: true });
      return { ok: found.map((a) => a.address) };
    } catch (e) {
      return failure(e, host, host);
    }
  },
  connect_start: (r) => {
    const id = nextConnect++;
    const operation = { result: null, abort: null };
    connects.set(id, operation);
    try {
      operation.abort = beginConnect(r, (result) => {
        operation.result = result;
      });
      return { ok: id };
    } catch (error) {
      connects.delete(id);
      throw error;
    }
  },
  connect_poll: ({ operation }) => {
    const state = connects.get(operation);
    if (state === undefined) {
      throw new Error("unknown connect operation");
    }
    if (state.result === null) {
      return { pending: true };
    }
    connects.delete(operation);
    return state.result;
  },
  connect_abort: ({ operation }) => {
    connects.get(operation)?.abort();
    connects.delete(operation);
    return { ok: null };
  },
  listen: ({ host, port, address, signal, retain }) =>
    new Promise((done) => {
      const server = net.createServer({ pauseOnConnect: false });
      const inbox = new Inbox();
      let stopped = false;
      let settled = false;
      const close = () => {
        stopped = true;
        server.close(() => {});
        for (const item of inbox.items) {
          item.value.destroy();
        }
        inbox.items = [];
      };
      const settle = (result) => {
        if (settled) {
          return;
        }
        settled = true;
        signal?.removeEventListener("abort", abort);
        done(result);
      };
      const abort = () => {
        close();
        settle({ cancelled: true });
      };
      signal?.addEventListener("abort", abort, { once: true });
      if (signal?.aborted) {
        abort();
        return;
      }
      server.on("connection", (socket) => (stopped ? socket.destroy() : inbox.push(socket)));
      server.on("error", (error) => {
        if (!settled) {
          close();
          settle(failure(error, address, host));
        } else {
          inbox.finish({ error });
        }
      });
      server.listen(port, host, () => {
        if (stopped || signal?.aborted) {
          abort();
          return;
        }
        const handle = open({ kind: "listener", server, inbox, closed: false, timeout: null });
        retain?.(() => {
          close();
          handles.delete(handle);
        });
        settle({ ok: handle });
      });
    }),
  bind_udp: ({ host, port, address, signal, retain }) =>
    new Promise((done) => {
      const socket = dgram.createSocket(net.isIPv6(host) ? "udp6" : "udp4");
      const inbox = new Inbox();
      let stopped = false;
      let settled = false;
      const close = () => {
        stopped = true;
        try {
          socket.close();
        } catch (error) {
          if (error.code !== "ERR_SOCKET_DGRAM_NOT_RUNNING") {
            throw error;
          }
        }
      };
      const settle = (result) => {
        if (settled) {
          return;
        }
        settled = true;
        signal?.removeEventListener("abort", abort);
        done(result);
      };
      const abort = () => {
        close();
        settle({ cancelled: true });
      };
      signal?.addEventListener("abort", abort, { once: true });
      if (signal?.aborted) {
        abort();
        return;
      }
      socket.on("message", (msg, rinfo) => {
        if (!stopped) {
          inbox.push({ msg, rinfo });
        }
      });
      socket.on("error", (error) => {
        if (!settled) {
          close();
          settle(failure(error, address, host));
        } else {
          inbox.finish({ error });
        }
      });
      socket.bind(port, host, () => {
        if (stopped || signal?.aborted) {
          abort();
          return;
        }
        const handle = open({ kind: "udp", socket, inbox, closed: false, timeout: null });
        retain?.(() => {
          close();
          handles.delete(handle);
        });
        settle({ ok: handle });
      });
    }),
  // std-net.read: at least one byte, at most `max`; empty at the end.
  read: async ({ h, max: signed, signal, retain }) => {
    // `usize` crosses the ABI as an i32 bit pattern, so read it unsigned.
    const max = signed >>> 0;
    if (max === 0) {
      return { ok: new Uint8Array(0) };
    }
    const item = await h.inbox.next(h.timeout, signal);
    if (item === CANCELLED) {
      return { cancelled: true };
    }
    if (item === TIMED_OUT) {
      return timedOut();
    }
    if (h.closed) {
      return { disposed: true };
    }
    if (item === null) {
      const why = h.inbox.end;
      return why?.error ? failure(why.error, "", "") : { ok: new Uint8Array(0) };
    }
    let chunk = item.value;
    if (chunk.length > max) {
      h.inbox.restore({ ...item, value: chunk.subarray(max), offset: item.offset + max });
      chunk = chunk.subarray(0, max);
    }
    const unread = { ...item, value: chunk };
    retain?.(() => {
      if (!h.closed) {
        h.inbox.restore(unread);
      }
    });
    if (h.socket.isPaused() && queued(h.inbox) < READ_AHEAD / 2) {
      h.socket.resume();
    }
    return { ok: chunk };
  },
  // std-net.write: once every byte is written.
  write: async ({ h, bytes, signal }) => {
    const buffer = Buffer.from(bytes);
    // Limit queued data so cancellation stops submitting further chunks,
    // leaving the stream open and only already accepted bytes in flight.
    for (let offset = 0; offset < buffer.length; offset += 65536) {
      if (signal?.aborted) {
        return { cancelled: true };
      }
      if (h.closed) {
        return { disposed: true };
      }
      const result = await new Promise((done) => {
        let settled = false;
        const settle = (result) => {
          if (settled) {
            return;
          }
          settled = true;
          signal?.removeEventListener("abort", abort);
          done(result);
        };
        const abort = () => settle({ cancelled: true });
        signal?.addEventListener("abort", abort, { once: true });
        h.socket.write(buffer.subarray(offset, offset + 65536), (error) =>
          settle(error ? failure(error, "", "") : { ok: null }),
        );
      });
      if (result.error || result.cancelled) {
        return result;
      }
    }
    return { ok: null };
  },
  // std-net.accept: the next connection's stream.
  accept: async ({ h, signal, retain }) => {
    const item = await h.inbox.next(h.timeout, signal);
    if (item === CANCELLED) {
      return { cancelled: true };
    }
    if (item === TIMED_OUT) {
      return timedOut();
    }
    const socket = item?.value;
    if (h.closed) {
      socket?.destroy();
      return { disposed: true };
    }
    if (item === null) {
      return failure(h.inbox.end?.error ?? "the listener stopped", "", "");
    }
    const handle = stream(socket);
    retain?.(() => {
      socket.destroy();
      handles.delete(handle);
    });
    return { ok: handle };
  },
  // std-net.send-to: one datagram.
  send_to: async ({ h, host, port, address, bytes, signal }) => {
    try {
      // Resolve before queuing the datagram so an aborted name lookup does
      // not send a packet later. Numeric destinations need no resolver wait.
      const destination = net.isIP(host)
        ? host
        : (
            await dns.lookup(host, {
              family: h.socket.type === "udp6" ? 6 : 4,
            })
          ).address;
      if (signal?.aborted) {
        return { cancelled: true };
      }
      if (h.closed) {
        return { disposed: true };
      }
      return await new Promise((done) => {
        h.socket.send(Buffer.from(bytes), port, destination, (error) =>
          done(error ? failure(error, address, host) : { ok: null }),
        );
      });
    } catch (error) {
      return failure(error, address, host);
    }
  },
  // std-net.receive: one datagram, at most `max` of its bytes.
  receive: async ({ h, max: signed, signal, retain }) => {
    const max = signed >>> 0;
    const item = await h.inbox.next(h.timeout, signal);
    if (item === CANCELLED) {
      return { cancelled: true };
    }
    if (item === TIMED_OUT) {
      return timedOut();
    }
    if (h.closed) {
      return { disposed: true };
    }
    if (item === null) {
      return failure(h.inbox.end?.error ?? "the socket stopped", "", "");
    }
    retain?.(() => {
      if (!h.closed) {
        h.inbox.restore(item);
      }
    });
    const got = item.value;
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
      if (idle === null) {
        h.socket.setKeepAlive(false);
      } else {
        h.socket.setKeepAlive(true, Math.max(1, Math.ceil(idle / 1000)) * 1000);
      }
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
    if (h.kind === "stream") {
      h.socket.destroy();
    } else if (h.kind === "listener") {
      h.server.close();
    } else {
      h.socket.close();
    }
    h.closed = true;
    h.inbox.finish({ closed: true });
    return { ok: null };
  },
  check: () => ({ ok: null }),
  // std-net.dispose: a close that reports nothing.
  dispose: (r) => ops.close(r),
  // The open handles by kind, for the leak report of `hd test`
  // (cli.test.leak).
  open: () => {
    const counts = { stream: 0, listener: 0, udp: 0 };
    for (const h of handles.values()) {
      if (!h.closed) {
        counts[h.kind] += 1;
      }
    }
    return { ok: counts };
  },
};

// Shared with worker callback-order regressions; this is the same
// dispatcher the message transport below calls.
export const run = async (r) => {
  if (r.handle === undefined) {
    return ops[r.op](r);
  }
  const h = handles.get(r.handle);
  if (h === undefined) {
    return { unknown: true };
  }
  // Disposing a closed handle does nothing (std-net.dispose).
  if (h.closed) {
    return r.op === "dispose" ? { ok: null } : { disposed: true };
  }
  return ops[r.op]({ ...r, h });
};

const { port, flag } = workerData;
serve(port, flag, run);
