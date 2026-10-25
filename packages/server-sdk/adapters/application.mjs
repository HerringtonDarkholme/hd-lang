import { createLeaseDispatcher } from "./dispatcher.mjs";
import { createHttpClient } from "./http.mjs";

const ok = (value) => ({ ok: true, value });
const expired = () => ({ ok: false, error: { tag: "Expired", fields: {} } });

// Runtime and signature-generated bindings are explicit inputs. Packaging
// supplies their public modules; the SDK never reads compiler cache internals.
// This boundary consumes/returns plain HD values, not platform Request/Response.
export function createApplication({ module, bindings, createCoreHost, grants, sink }) {
  const sources = bindings.interfaces.filter((item) => !item.handle);
  if (sources.length !== 1 || sources[0].identity.trait_path !== "server.HttpServer")
    throw new Error("the application must bind the SDK HttpServer capability");
  if (!grants || !sources[0].selectors.some((name) => grants[name] === true))
    throw new Error("the application must grant its HttpServer capability");
  const source = sources[0];
  const leases = new Map();
  const incoming = [];
  let host,
    driver,
    driving = false,
    stopping = false,
    ended = false;
  const dispatcher = createLeaseDispatcher({
    drive,
    failed: () => {
      try {
        host?.close();
      } finally {
        incoming.length = 0;
        leases.clear();
      }
    },
  });
  const lookup = (id) => {
    const lease = leases.get(id.sequence);
    if (!lease || lease.id.generation !== id.generation) throw new Error("expired request lease");
    lease.assertLive();
    return lease;
  };
  const provider = {
    accept(context) {
      let held;
      return context.pending(
        () => {
          while (!held && incoming.length) {
            const candidate = incoming.shift();
            if (leases.has(candidate.id.sequence)) held = candidate;
          }
          return Boolean(held) || stopping;
        },
        () => ok(held ? { id: held.id, request: held.request } : null),
        () => {
          if (held && leases.has(held.id.sequence)) incoming.unshift(held);
        },
      );
    },
    respond(id, response) {
      return lookup(id).respond(response) ? ok(undefined) : expired();
    },
    cancelled(id, context) {
      return lookup(id).cancelled(context);
    },
    client(id, context) {
      const lease = lookup(id);
      const policy = context.http;
      if (
        !policy ||
        typeof policy.coversHost !== "function" ||
        typeof policy.isAsciiUrl !== "function" ||
        typeof policy.writtenHost !== "function"
      )
        throw new Error("server Http requires the compiler HTTP policy service");
      return createHttpClient(lease, {
        grant: {
          coversHost: (key, host, port) => key === "Http" && policy.coversHost(host, port),
        },
        isAsciiUrl: policy.isAsciiUrl,
        writtenHost: policy.writtenHost,
      });
    },
    clock(id) {
      const lease = lookup(id);
      return {
        now() {
          lease.assertLive();
          return new Date().toISOString();
        },
        monotonic() {
          lease.assertLive();
          return BigInt(Math.floor(performance.now()));
        },
        sleep(duration, context) {
          return lease.operation(context, (resolve) => {
            const due = performance.now() + Number(duration);
            let timer;
            const arm = () => {
              const left = due - performance.now();
              if (left <= 0) resolve(undefined);
              else timer = setTimeout(arm, Math.min(left, 2147483647));
            };
            arm();
            return () => clearTimeout(timer);
          });
        },
      };
    },
    finish(id, abandoned) {
      const lease = lookup(id);
      lease.finish(abandoned);
      leases.delete(id.sequence);
    },
  };
  function drive() {
    if (ended) return;
    if (driving) throw new Error("reentrant application turn");
    driving = true;
    try {
      if (!driver) {
        host = createCoreHost(
          sink,
          {
            module,
            grants,
            packageBindings: (runtime) =>
              bindings.bind(runtime, new Map([[source.module, provider]])),
          },
          {
            providers: () => ({
              traits: {
                "hd:Http": {
                  send() {
                    throw new Error("server HTTP sends require a request lease");
                  },
                },
                "hd:Clock": {
                  sleep() {
                    throw new Error("server sleeps require a request lease");
                  },
                },
              },
            }),
          },
        );
        driver = host.createDriver(new WebAssembly.Instance(module, host.imports));
      }
      // A bounded number of ready turns catches a broken host wake loop.
      // CPU-bound Wasm still needs the platform/supervisor's hard limit.
      for (let budget = 0; budget < 1000; budget++) {
        const turn = driver.step();
        if (turn.status !== -1) {
          if (turn.status !== 0) throw new Error("server main failed with status " + turn.status);
          if (!stopping || leases.size) throw new Error("server main ended before its leases");
          ended = true;
          host.close();
          return;
        }
        if (!turn.wakes) return;
      }
      throw new Error("server exceeded its ready-turn budget");
    } finally {
      driving = false;
    }
  }
  return Object.freeze({
    async dispatch(request, signal) {
      if (stopping) throw new Error("server admission is closed");
      const lease = dispatcher.open(request);
      leases.set(lease.id.sequence, lease);
      incoming.push(lease);
      const abort = () =>
        lease.cancel(
          signal?.reason === "Deadline" || signal?.reason === "Stopping"
            ? signal.reason
            : "Disconnected",
        );
      signal?.addEventListener("abort", abort, { once: true });
      if (signal?.aborted) abort();
      try {
        return await lease.run();
      } finally {
        signal?.removeEventListener("abort", abort);
        leases.delete(lease.id.sequence);
      }
    },
    stop() {
      stopping = true;
      try {
        drive();
      } catch (error) {
        dispatcher.fail(error);
      }
    },
    fail: dispatcher.fail,
    active: dispatcher.active,
  });
}
