// Control replies are correlated messages. The shared flag is only a wake
// hint: an old notification or a delayed queue publication is not a reply.
// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.mjs).
const { receiveMessageOnPort } = process.getBuiltinModule("node:worker_threads");

export function receiveReply(port, flag, id, left) {
  for (;;) {
    const remaining = left();
    if (remaining <= 0) return null;
    const reply = receiveMessageOnPort(port);
    if (reply) {
      if (reply.message.id === id) return reply.message;
      continue;
    }
    // Poll after a bounded wait even if the matching notification already
    // happened before the port made its message visible to this thread.
    Atomics.wait(flag, 0, Atomics.load(flag, 0), Math.min(1, remaining));
  }
}

export function postReply(port, flag, id, result) {
  port.postMessage({ id, result });
  Atomics.store(flag, 0, 1);
  Atomics.notify(flag, 0);
}
