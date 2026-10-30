import http from "node:http";
import { createLeaseDispatcher } from "../adapters/dispatcher.mjs";
import { createHttpClient } from "../adapters/http.mjs";
import { createRequestConsole } from "../adapters/console.mjs";

let generation = 0n;
const instances = new WeakMap();
const ok = (value) => ({ ok: true, value });
const err = (tag, reason) => ({
  ok: false,
  error: { tag, fields: reason === undefined ? {} : { reason } },
});
const method = (name) => {
  const standard = {
    GET: "Get",
    HEAD: "Head",
    POST: "Post",
    PUT: "Put",
    PATCH: "Patch",
    DELETE: "Delete",
    OPTIONS: "Options",
  };
  return standard[name] ? { tag: standard[name], fields: {} } : { tag: "Other", fields: { name } };
};

async function requestBody(request, response, waiter) {
  let length = 0;
  const chunks = [];
  for await (const chunk of request) {
    length += chunk.length;
    if (length > 1048576) {
      response.writeHead(413);
      response.end();
      waiter.resolve(err("Expired"));
      return null;
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

function requestHeaders(request) {
  const headers = [];
  for (let n = 0; n < request.rawHeaders.length; n += 2) {
    headers.push([request.rawHeaders[n], request.rawHeaders[n + 1]]);
  }
  return headers;
}

function requestUrl(state, request) {
  const address = state.server.address();
  const authority = request.headers.host ?? `127.0.0.1:${address.port}`;
  return new URL(request.url, `http://${authority}`).href;
}

function stateFor(context) {
  let state = instances.get(context.instanceSignal);
  if (state) {
    return state;
  }
  state = {
    generation: ++generation,
    sequence: 0n,
    waiting: [],
    jobs: new Map(),
    stopping: false,
    peak: 0,
    replies: 0,
  };
  // The ordinary Node host.run driver polls prepared operations itself.
  // Native starts and disposal are still drained by each HTTP callback's
  // lease.run coroutine; this callback must never poll Wasm reentrantly.
  state.dispatcher = createLeaseDispatcher({ drive() {} });
  instances.set(context.instanceSignal, state);
  const stop = () => {
    if (state.stopping) {
      return;
    }
    state.stopping = true;
    for (const waiter of state.waiting.splice(0)) {
      waiter.resolve(ok(null));
    }
    state.server.close();
    console.log(JSON.stringify({ event: "stopped", replies: state.replies, peak: state.peak }));
  };
  state.server = http.createServer({ maxHeaderSize: 65536 }, async (request, response) => {
    const waiter = state.waiting.shift();
    if (!waiter || state.stopping) {
      response.writeHead(503, { "retry-after": "1" });
      response.end();
      request.resume();
      return;
    }
    const sequence = ++state.sequence;
    const job = {
      request,
      response,
      cancelled: null,
      sequence,
      lease: state.dispatcher.open(request),
      closed: false,
      responded: false,
    };
    state.jobs.set(sequence, job);
    state.peak = Math.max(state.peak, state.jobs.size);
    response.on("close", () => {
      if (!response.writableFinished && !job.closed && job.cancelled === null) {
        job.cancelled = "Disconnected";
        job.lease.cancel("Disconnected");
      }
    });
    let handedToGuest = false;
    try {
      const body = await requestBody(request, response, waiter);
      if (body === null) {
        return;
      }
      handedToGuest = waiter.resolve(
        ok({
          id: { generation: state.generation, sequence },
          request: {
            method: method(request.method),
            url: requestUrl(state, request),
            headers: requestHeaders(request),
            body,
            redirects: 10,
            dns_servers: null,
            tls_roots: [],
            tls_identity: null,
            connect_timeout: null,
            tls_handshake_timeout: null,
            response_header_timeout: null,
            body_read_timeout: null,
          },
        }),
      );
      if (!handedToGuest && !response.destroyed) {
        response.writeHead(503);
        response.end();
      }
    } catch (error) {
      if (!response.destroyed) {
        response.writeHead(400);
        response.end();
      }
      waiter.resolve(err("Other", String(error)));
    } finally {
      // An admission/read failure has no HD worker to acknowledge it.
      if (!handedToGuest && !job.closed) {
        job.closed = true;
        job.lease.finish(true);
      }
      try {
        await job.lease.run();
      } catch {
        // A closed instance cannot retain a useful response or be reused.
        // The private failure is never reflected into a client response.
        if (!response.destroyed && !response.writableEnded) {
          if (!response.headersSent) {
            response.writeHead(500);
          }
          response.end();
        }
      } finally {
        state.jobs.delete(sequence);
      }
    }
  });
  state.server.on("error", (error) => {
    for (const waiter of state.waiting.splice(0)) {
      waiter.resolve(err("Other", String(error)));
    }
  });
  state.server.listen(Number(process.env.HD_SERVER_PORT ?? 8080), "127.0.0.1", () => {
    console.log(
      JSON.stringify({
        event: "listening",
        host: "127.0.0.1",
        port: state.server.address().port,
        pid: process.pid,
      }),
    );
  });
  process.once("SIGTERM", stop);
  context.instanceSignal.addEventListener(
    "abort",
    () => {
      stop();
      state.dispatcher.fail(new Error("server instance closed"));
      state.server.closeAllConnections();
      // Owners remove jobs only after their queued native disposal drains.
      process.off("SIGTERM", stop);
    },
    { once: true },
  );
  return state;
}

// Slice-1 prototype: typed source and whole response, not the completed
// deadline/disconnect/body-admission/lifecycle policy of slices 2 and 4.
export default {
  accept(context) {
    const state = stateFor(context);
    if (state.stopping) {
      return ok(null);
    }
    return new Promise((resolve) => {
      let live = true;
      const waiter = {
        resolve: (value) => {
          if (!live) {
            return false;
          }
          live = false;
          context.signal.removeEventListener("abort", abort);
          resolve(value);
          return true;
        },
      };
      const abort = () => {
        const at = state.waiting.indexOf(waiter);
        if (at >= 0) {
          state.waiting.splice(at, 1);
        }
        waiter.resolve(ok(null));
      };
      context.signal.addEventListener("abort", abort, { once: true });
      state.waiting.push(waiter);
    });
  },
  respond(id, value, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed || job.response.destroyed) {
      return err("Expired");
    }
    if (job.responded) {
      return err("Other", "duplicate response");
    }
    const response = job.response;
    try {
      if (value.status < 200 || value.status > 599 || value.body.length > 1048576) {
        throw new Error("invalid response");
      }
      job.responded = true;
      for (const [name, text] of value.headers) {
        if (["connection", "transfer-encoding", "content-length"].includes(name.toLowerCase())) {
          continue;
        }
        response.appendHeader(name, text);
      }
      response.writeHead(value.status);
      const body =
        job.request.method === "HEAD" || [204, 205, 304].includes(value.status)
          ? undefined
          : Buffer.from(value.body);
      let ready = false,
        failed = false;
      response.end(body, (error) => {
        failed = Boolean(error);
        ready = true;
      });
      return context.pending(
        () => ready || response.destroyed,
        () => {
          if (failed || !response.writableFinished) {
            return err("Expired");
          }
          state.replies++;
          return ok(undefined);
        },
        () => response.destroy(),
      );
    } catch (error) {
      if (!response.headersSent) {
        response.writeHead(500);
      }
      response.end();
      return err("Other", String(error));
    }
  },
  cancelled(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed) {
      return { tag: "Stopping", fields: {} };
    }
    return job.lease.cancelled(context);
  },
  client(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed) {
      throw new Error("expired request lease");
    }
    job.lease.assertLive();
    const policy = context.http;
    if (
      !policy ||
      typeof policy.coversHost !== "function" ||
      typeof policy.isAsciiUrl !== "function" ||
      typeof policy.writtenHost !== "function"
    ) {
      throw new Error("server Http requires the compiler HTTP policy service");
    }
    return createHttpClient(job.lease, {
      grant: { coversHost: (key, host, port) => key === "Http" && policy.coversHost(host, port) },
      isAsciiUrl: policy.isAsciiUrl,
      writtenHost: policy.writtenHost,
    });
  },
  clock(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed) {
      throw new Error("expired request lease");
    }
    job.lease.assertLive();
    const check = () => {
      if (job.closed || state.jobs.get(id.sequence) !== job) {
        throw new Error("expired request lease");
      }
      job.lease.assertLive();
    };
    return {
      now() {
        check();
        return new Date().toISOString();
      },
      monotonic() {
        check();
        return BigInt(Math.floor(performance.now()));
      },
      sleep(duration, call) {
        check();
        return job.lease.operation(call, (resolve) => {
          const due = performance.now() + Number(duration);
          let timer;
          const arm = () => {
            const left = due - performance.now();
            if (left <= 0) {
              resolve(undefined);
            } else {
              timer = setTimeout(arm, Math.min(left, 2147483647));
            }
          };
          arm();
          return () => clearTimeout(timer);
        });
      },
    };
  },
  console(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed) {
      throw new Error("expired request lease");
    }
    return createRequestConsole(job.lease, {
      id,
      policy: context.console,
      write(record) {
        const output = record.stream === "out" ? process.stdout : process.stderr;
        output.write(JSON.stringify(record) + "\n");
      },
    });
  },
  finish(id, abandoned, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.closed) {
      return;
    }
    job.closed = true;
    if (!job.response.writableEnded) {
      if (!job.response.destroyed) {
        job.response.writeHead(job.cancelled === "Deadline" ? 504 : 503);
        job.response.end();
      }
    }
    job.lease.finish(abandoned);
    if (job.cancelled || abandoned) {
      console.log(
        JSON.stringify({
          event: "request-finished",
          sequence: String(id.sequence),
          cancelled: job.cancelled,
          abandoned,
        }),
      );
    }
  },
};
