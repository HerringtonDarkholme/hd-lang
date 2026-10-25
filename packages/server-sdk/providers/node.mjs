import http from "node:http";

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

function stateFor(context) {
  let state = instances.get(context.instanceSignal);
  if (state) return state;
  state = {
    generation: ++generation,
    sequence: 0n,
    waiting: [],
    jobs: new Map(),
    stopping: false,
    peak: 0,
    replies: 0,
  };
  instances.set(context.instanceSignal, state);
  const stop = () => {
    if (state.stopping) return;
    state.stopping = true;
    for (const waiter of state.waiting.splice(0)) waiter.resolve(ok(null));
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
    const job = { request, response, cancelled: null, sequence, timers: new Set() };
    state.jobs.set(sequence, job);
    state.peak = Math.max(state.peak, state.jobs.size);
    response.on("close", () => {
      if (!response.writableFinished) {
        job.cancelled = "Disconnected";
      }
    });
    try {
      let length = 0;
      const chunks = [];
      for await (const chunk of request) {
        length += chunk.length;
        if (length > 1048576) {
          response.writeHead(413);
          response.end();
          state.jobs.delete(sequence);
          waiter.resolve(err("Expired"));
          return;
        }
        chunks.push(chunk);
      }
      const address = state.server.address();
      const url = new URL(
        request.url,
        `http://${request.headers.host ?? `127.0.0.1:${address.port}`}`,
      ).href;
      const headers = [];
      for (let n = 0; n < request.rawHeaders.length; n += 2)
        headers.push([request.rawHeaders[n], request.rawHeaders[n + 1]]);
      waiter.resolve(
        ok({
          id: { generation: state.generation, sequence },
          request: {
            method: method(request.method),
            url,
            query: [],
            headers,
            body: Buffer.concat(chunks),
            redirects: 10,
          },
        }),
      );
    } catch (error) {
      state.jobs.delete(sequence);
      if (!response.destroyed) {
        response.writeHead(400);
        response.end();
      }
      waiter.resolve(err("Other", String(error)));
    }
  });
  state.server.on("error", (error) => {
    for (const waiter of state.waiting.splice(0)) waiter.resolve(err("Other", String(error)));
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
      state.server.closeAllConnections();
      for (const job of state.jobs.values()) for (const clear of job.timers) clear();
      state.jobs.clear();
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
    if (state.stopping) return ok(null);
    return new Promise((resolve) => {
      const waiter = {
        resolve: (value) => {
          context.signal.removeEventListener("abort", abort);
          resolve(value);
        },
      };
      const abort = () => {
        const at = state.waiting.indexOf(waiter);
        if (at >= 0) state.waiting.splice(at, 1);
        waiter.resolve(ok(null));
      };
      context.signal.addEventListener("abort", abort, { once: true });
      state.waiting.push(waiter);
    });
  },
  respond(id, value, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job || job.response.destroyed) return err("Expired");
    const response = job.response;
    try {
      if (value.status < 200 || value.status > 599 || value.body.length > 1048576)
        throw new Error("invalid response");
      for (const [name, text] of value.headers) {
        if (["connection", "transfer-encoding", "content-length"].includes(name.toLowerCase()))
          continue;
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
          if (failed || !response.writableFinished) return err("Expired");
          state.replies++;
          return ok(undefined);
        },
        () => response.destroy(),
      );
    } catch (error) {
      if (!response.headersSent) response.writeHead(500);
      response.end();
      return err("Other", String(error));
    }
  },
  cancelled(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job) return { tag: "Stopping", fields: {} };
    return context.pending(
      () => job.cancelled !== null,
      () => ({ tag: job.cancelled, fields: {} }),
    );
  },
  clock(id, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job) throw new Error("expired request lease");
    const check = () => {
      if (state.jobs.get(id.sequence) !== job) throw new Error("expired request lease");
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
        let ready = false,
          timer,
          live = true;
        const due = performance.now() + Number(duration);
        const arm = () => {
          if (!live) return;
          const left = due - performance.now();
          if (left <= 0) ready = true;
          else timer = setTimeout(arm, Math.min(left, 2147483647));
        };
        const clear = () => {
          live = false;
          clearTimeout(timer);
          job.timers.delete(clear);
        };
        job.timers.add(clear);
        arm();
        return call.pending(
          () => ready,
          () => undefined,
          clear,
          () => {
            clear();
          },
        );
      },
    };
  },
  finish(id, abandoned, context) {
    const state = stateFor(context);
    const job = id.generation === state.generation ? state.jobs.get(id.sequence) : null;
    if (!job) return;
    for (const clear of job.timers) clear();
    state.jobs.delete(id.sequence);
    if (!job.response.writableEnded) {
      if (!job.response.destroyed) {
        job.response.writeHead(job.cancelled === "Deadline" ? 504 : 503);
        job.response.end();
      }
    }
    if (job.cancelled || abandoned)
      console.log(
        JSON.stringify({
          event: "request-finished",
          sequence: String(id.sequence),
          cancelled: job.cancelled,
          abandoned,
        }),
      );
  },
};
