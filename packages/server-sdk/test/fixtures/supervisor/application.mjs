import { parentPort, workerData } from "node:worker_threads";

// An actual infinite Wasm loop stands in for arbitrary guest main/handler/cleanup.
// Tests isolate supervision behavior; this is not a compiled HD integration claim.
export function createApplication({ module, grants, onReady, onStopped }) {
  const spin = new WebAssembly.Instance(module).exports.spin;
  let active = 0,
    stopping = false,
    hits = 0;
  return {
    start() {
      if (grants.fixture === "main-cpu") spin();
      onReady();
    },
    async dispatch(request, signal) {
      active++;
      try {
        if (request.url === "/cpu") spin();
        if (request.url === "/stale-messages") {
          parentPort.postMessage({
            type: "result",
            generation: workerData.generation - 1,
            id: 1,
            released: true,
            result: null,
          });
          parentPort.postMessage({
            type: "result",
            generation: workerData.generation,
            id: 0,
            released: false,
          });
        }
        if (request.url === "/trap") throw new Error("private-error-must-never-leak");
        if (request.url === "/wait" || request.url === "/cleanup-cpu") {
          if (!signal.aborted)
            await new Promise((resolve) =>
              signal.addEventListener("abort", resolve, { once: true }),
            );
          if (request.url === "/cleanup-cpu") spin();
          return { response: null, cancelled: signal.reason, abandoned: false };
        }
        if (request.delay) await new Promise((resolve) => setTimeout(resolve, request.delay));
        return {
          response: {
            status: 200,
            url: "",
            headers: [],
            body: new TextEncoder().encode(String(++hits)),
          },
          cancelled: null,
          abandoned: false,
        };
      } finally {
        active--;
        if (stopping && !active) onStopped();
      }
    },
    stop() {
      stopping = true;
      if (!active) onStopped();
    },
  };
}
