import { Worker } from "node:worker_threads";

const CLEANUP_BACKSTOP_MS = 200;

const empty = () => new Uint8Array();
const generic = (status, retryAfter) => ({
  response: {
    status,
    url: "",
    headers: retryAfter ? [["Retry-After", retryAfter]] : [],
    body: empty(),
  },
  cancelled: null,
  abandoned: true,
});
const cancelled = (reason) => ({ response: null, cancelled: reason, abandoned: true });
const reasonOf = (signal) =>
  ["Deadline", "Stopping"].includes(signal?.reason) ? signal.reason : "Disconnected";

// The cleanup backstop is fixed by the SDK contract. Other lifecycle budgets
// and explicit module URLs are supplied by the embedding application.
export function createSupervisor({
  program,
  policy,
  event = () => {},
  log = () => {},
  exhausted = () => {},
}) {
  if (!(program.module instanceof WebAssembly.Module))
    throw new TypeError("compiled module required");
  for (const key of ["runtimeURL", "bindingsURL", "applicationURL"])
    if (typeof program[key] !== "string") throw new TypeError(`explicit ${key} required`);
  for (const key of [
    "workers",
    "startupMs",
    "requestMs",
    "drainMs",
    "stopKillMs",
    "restartLimit",
    "restartWindowMs",
  ])
    if (!Number.isSafeInteger(policy[key]) || policy[key] < (key === "restartLimit" ? 0 : 1))
      throw new TypeError(`explicit positive ${key} required`);
  for (const key of ["startupMs", "requestMs", "drainMs", "stopKillMs"])
    if (policy[key] > 2147483647)
      throw new TypeError(`${key} must not exceed 2147483647 milliseconds`);
  if (policy.stopKillMs < CLEANUP_BACKSTOP_MS)
    throw new TypeError("the outer stop budget cannot precede the cleanup backstop");
  if (typeof policy.retryAfter !== "string")
    throw new TypeError("explicit Retry-After value required");
  // The snapshot owns plain grant/config values; callers cannot change a live generation.
  const input = { ...structuredClone(program), module: program.module };
  const rules = Object.freeze({ ...policy });
  let lifecycle = "Starting",
    nextRequest = 0,
    drainTimer,
    stopTimer,
    stopResolve;
  const stopped = new Promise((resolve) => {
    stopResolve = resolve;
  });
  const slots = Array.from({ length: rules.workers }, (_, index) => ({
    index,
    generation: 0,
    worker: null,
    ready: false,
    active: null,
    restarts: [],
    startupTimer: null,
    terminating: false,
  }));
  const publish = (kind, slot, extra = {}) => {
    try {
      event(
        Object.freeze({
          kind,
          slot: slot?.index,
          generation: slot?.generation,
          lifecycle,
          ...extra,
        }),
      );
    } catch {
      /* Observers cannot bypass owned cleanup. */
    }
  };
  const ready = () => lifecycle === "Ready" && slots.every((slot) => slot.ready);
  const changed = () => {
    if (lifecycle === "Starting" && slots.every((slot) => slot.ready)) lifecycle = "Ready";
    if (
      ["Draining", "Cancelling"].includes(lifecycle) &&
      slots.every((slot) => !slot.worker && !slot.terminating)
    ) {
      lifecycle = "Stopped";
      clearTimeout(drainTimer);
      clearTimeout(stopTimer);
      stopResolve();
    }
  };
  const settle = (slot, result) => {
    const job = slot.active;
    if (!job) return;
    slot.active = null;
    clearTimeout(job.deadline);
    clearTimeout(job.cleanup);
    job.signal?.removeEventListener("abort", job.abort);
    job.resolve(structuredClone(result));
  };
  const cancel = (slot, reason) => {
    const job = slot.active;
    if (!job || job.reason) return;
    job.reason = reason;
    publish("cancel", slot, { id: job.id, reason });
    slot.worker?.postMessage({ type: "cancel", generation: slot.generation, id: job.id, reason });
    job.cleanup = setTimeout(
      () => retire(slot, "cleanup-deadline", cancelled(reason)),
      CLEANUP_BACKSTOP_MS,
    );
  };
  function retire(slot, cause, result = generic(500)) {
    const worker = slot.worker;
    if (!worker) return;
    slot.worker = null;
    slot.ready = false;
    slot.terminating = true;
    clearTimeout(slot.startupTimer);
    settle(slot, result);
    publish("terminated", slot, { cause, cleanupConfirmed: false });
    // Termination is owned even for a worker stuck inside Wasm or JS. There
    // are no child processes in the portable profile; no shell kill command.
    worker.terminate().then(() => {
      slot.terminating = false;
      if (["Starting", "Ready"].includes(lifecycle)) {
        const now = performance.now();
        slot.restarts = slot.restarts.filter((time) => now - time < rules.restartWindowMs);
        if (slot.restarts.length < rules.restartLimit) {
          slot.restarts.push(now);
          spawn(slot);
        } else {
          publish("restart-exhausted", slot, { cause });
          try {
            exhausted(Object.freeze({ slot: slot.index, generation: slot.generation, cause }));
          } catch {}
        }
      }
      changed();
    });
  }
  function spawn(slot) {
    const generation = ++slot.generation;
    const worker = new Worker(new URL("./worker.mjs", import.meta.url), {
      workerData: { program: input, generation },
    });
    slot.worker = worker;
    publish("starting", slot);
    slot.startupTimer = setTimeout(() => retire(slot, "startup-deadline"), rules.startupMs);
    worker.on("message", (message) => {
      if (slot.worker !== worker || message.generation !== generation) return;
      if (message.type === "log") {
        const record = message.record;
        if (
          record?.version !== 1 ||
          !["out", "err"].includes(record.stream) ||
          typeof record.text !== "string" ||
          (record.lease !== null &&
            (typeof record.lease?.generation !== "string" ||
              typeof record.lease?.sequence !== "string" ||
              !/^\d+$/.test(record.lease.generation) ||
              !/^\d+$/.test(record.lease.sequence))) ||
          (record.lease === null
            ? message.request !== null
            : !Number.isSafeInteger(message.request) || message.request !== slot.active?.id)
        )
          return;
        try {
          log(
            Object.freeze({
              version: 1,
              stream: record.stream,
              text: record.text,
              lease: record.lease
                ? Object.freeze({
                    generation: record.lease.generation,
                    sequence: record.lease.sequence,
                  })
                : null,
              worker: Object.freeze({ slot: slot.index, generation }),
              request: message.request ?? null,
            }),
          );
        } catch {
          // Application logs are separate observers, never response owners.
        }
      } else if (message.type === "ready") {
        clearTimeout(slot.startupTimer);
        slot.ready = true;
        publish("ready", slot);
        changed();
      } else if (message.type === "result") {
        if (!slot.active || message.id !== slot.active.id) return;
        if (!message.released) return retire(slot, "response-before-release");
        if (!message.result || typeof message.result.abandoned !== "boolean")
          return retire(slot, "invalid-worker-result");
        const reason = slot.active.reason;
        settle(slot, reason ? { ...message.result, cancelled: reason } : message.result);
        publish("released", slot, { id: message.id, cleanupConfirmed: !message.result.abandoned });
      } else if (message.type === "failed") {
        // The original diagnostic stays inside the worker. Only a generic
        // failure classification reaches affected physical response owners.
        retire(slot, message.category === "host-contract" ? "host-contract" : "poisoned-instance");
      } else if (message.type === "stopped") {
        if (["Starting", "Ready"].includes(lifecycle)) return retire(slot, "unexpected-main-stop");
        clearTimeout(slot.startupTimer);
        slot.ready = false;
        slot.worker = null;
        slot.terminating = true;
        settle(slot, cancelled(slot.active?.reason ?? "Stopping"));
        worker.terminate().then(() => {
          slot.terminating = false;
          changed();
        });
        publish("drained", slot, { cleanupConfirmed: true });
      }
    });
    worker.on("error", () => {
      if (slot.worker === worker) retire(slot, "worker-error");
    });
    worker.on("exit", () => {
      if (slot.worker === worker) retire(slot, "unexpected-exit");
    });
  }
  const escalate = () => {
    if (lifecycle === "Stopped" || lifecycle === "Cancelling") return;
    lifecycle = "Cancelling";
    clearTimeout(drainTimer);
    for (const slot of slots) cancel(slot, "Stopping");
    stopTimer = setTimeout(() => {
      for (const slot of slots)
        retire(slot, "outer-stop-deadline", cancelled(slot.active?.reason ?? "Stopping"));
      changed();
    }, rules.stopKillMs);
    publish("cancelling");
  };
  for (const slot of slots) spawn(slot);
  return Object.freeze({
    dispatch(request, signal, { admittedAt = performance.now() } = {}) {
      if (signal?.aborted) return Promise.resolve(cancelled(reasonOf(signal)));
      if (!Number.isFinite(admittedAt)) throw new TypeError("finite admission timestamp required");
      const remaining = admittedAt + rules.requestMs - performance.now();
      if (remaining <= 0) {
        publish("deadline-before-dispatch");
        return Promise.resolve({ ...cancelled("Deadline"), abandoned: false });
      }
      const slot = lifecycle === "Ready" ? slots.find((slot) => slot.ready && !slot.active) : null;
      if (!slot) {
        publish("overload");
        return Promise.resolve(generic(503, rules.retryAfter));
      }
      const ownedRequest = structuredClone(request);
      return new Promise((resolve) => {
        const job = {
          id: ++nextRequest,
          signal,
          resolve,
          reason: null,
          abort: () => cancel(slot, reasonOf(signal)),
          deadline: null,
          cleanup: null,
        };
        slot.active = job;
        signal?.addEventListener("abort", job.abort, { once: true });
        job.deadline = setTimeout(() => cancel(slot, "Deadline"), remaining);
        slot.worker.postMessage({
          type: "request",
          generation: slot.generation,
          id: job.id,
          request: ownedRequest,
        });
        publish("admitted", slot, { id: job.id });
      });
    },
    stop() {
      if (lifecycle === "Stopped") return stopped;
      if (["Draining", "Cancelling"].includes(lifecycle)) {
        escalate();
        return stopped;
      }
      lifecycle = "Draining";
      for (const slot of slots)
        slot.worker?.postMessage({ type: "stop", generation: slot.generation });
      drainTimer = setTimeout(escalate, rules.drainMs);
      publish("draining");
      changed();
      return stopped;
    },
    fail() {
      lifecycle = "Failed";
      clearTimeout(drainTimer);
      clearTimeout(stopTimer);
      publish("failed");
      for (const slot of slots) retire(slot, "adapter-failure", generic(500));
    },
    close() {
      if (lifecycle === "Stopped") return stopped;
      lifecycle = "Cancelling";
      clearTimeout(drainTimer);
      for (const slot of slots)
        retire(slot, "forced-close", cancelled(slot.active?.reason ?? "Stopping"));
      changed();
      return stopped;
    },
    ready,
    state: () => lifecycle,
    active: () => slots.filter((slot) => slot.active).length,
  });
}
