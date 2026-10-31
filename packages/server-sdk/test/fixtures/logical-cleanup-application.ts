import { workerData } from "node:worker_threads";
import { createApplication as actualApplication } from "../../adapters/application.ts";

const observation = new Int32Array(workerData.program.testCleanupClock);
const hold = new URL(import.meta.url).searchParams.has("hold");

// A test Clock preserves the 100ms logical cleanup request without reserving
// half of the separate 200ms wall-clock watchdog for an OS scheduling race.
export function createApplication(options) {
  return actualApplication({
    ...options,
    bindings: {
      ...options.bindings,
      bind(runtime, providers) {
        const wrapped = new Map();
        for (const [module, provider] of providers) {
          wrapped.set(module, {
            ...provider,
            clock(id) {
              const actual = provider.clock(id);
              let tick = 0n;
              return {
                ...actual,
                monotonic() {
                  actual.monotonic();
                  return tick;
                },
                sleep(duration, context) {
                  const amount = BigInt(duration);
                  // The handler's 60s sleeps and SDK 20s request deadline stay
                  // pending real operations; only the cleanup timer advances.
                  if (amount === 60000n || amount === 20000n) {
                    return actual.sleep(duration, context);
                  }
                  Atomics.store(observation, 1, Number(amount));
                  Atomics.add(observation, 0, 1);
                  Atomics.notify(observation, 0);
                  if (hold) {
                    return context.pending(
                      () => false,
                      () => undefined,
                    );
                  }
                  return actual.sleep(0n, {
                    ...context,
                    pending(ready, done, ...callbacks) {
                      return context.pending(
                        ready,
                        () => {
                          const value = done();
                          tick += amount;
                          Atomics.add(observation, 2, 1);
                          return value;
                        },
                        ...callbacks,
                      );
                    },
                  });
                },
              };
            },
          });
        }
        return options.bindings.bind(runtime, wrapped);
      },
    },
  });
}
