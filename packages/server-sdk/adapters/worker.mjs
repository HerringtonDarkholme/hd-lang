import { parentPort, workerData } from "node:worker_threads";

const { program, generation } = workerData;
const post = (type, fields = {}) => parentPort.postMessage({ type, generation, ...fields });
const freeze = (value) => {
  if (!value || typeof value !== "object" || value instanceof WebAssembly.Module) return value;
  for (const item of Object.values(value)) freeze(item);
  return Object.freeze(value);
};
freeze(program.grants);
let app,
  active = null,
  startupReturned = false,
  hasAccept = false,
  stopped = false;
const fail = (error) => {
  post("failed", {
    category: error?.constructor?.name === "HostContract" ? "host-contract" : "instance-failure",
  });
  parentPort.close();
};
try {
  const [runtime, bindings, application] = await Promise.all([
    import(program.runtimeURL),
    import(program.bindingsURL),
    import(program.applicationURL),
  ]);
  const ready = () => {
    hasAccept = true;
    if (startupReturned) post("ready");
  };
  app = application.createApplication({
    module: program.module,
    bindings,
    createCoreHost: runtime.createCoreHost,
    grants: program.grants,
    sink: { out() {}, err() {} },
    onReady: ready,
    onStopped: () => {
      stopped = true;
      if (!active) post("stopped");
    },
  });
  // Required eager-start seam: readiness follows a completed turn parked on
  // HttpServer.accept. Merely constructing the lazy adapter is insufficient.
  if (typeof app.start !== "function") throw new Error("Application.start readiness seam required");
  app.start();
  startupReturned = true;
  if (hasAccept) post("ready");
} catch (error) {
  fail(error);
}
parentPort.on("message", (message) => {
  if (!app || message.generation !== generation) return;
  if (message.type === "request") {
    if (active) return fail();
    const controller = new AbortController();
    const job = (active = { id: message.id, controller });
    // The plain copied result is posted only after dispatch has joined guest
    // cleanup or explicitly abandoned it. Physical response ownership stays
    // in the supervisor's thread, outside this interruptible isolate.
    try {
      Promise.resolve(app.dispatch(message.request, controller.signal)).then((result) => {
        if (active !== job) return;
        active = null;
        post("result", { id: job.id, released: true, result });
        if (stopped) post("stopped");
      }, fail);
    } catch (error) {
      fail(error);
    }
  } else if (message.type === "cancel") {
    if (active?.id === message.id && !active.controller.signal.aborted)
      active.controller.abort(message.reason);
  } else if (message.type === "stop") {
    try {
      app.stop();
    } catch (error) {
      fail(error);
    }
  }
});
