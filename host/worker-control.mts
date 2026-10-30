// Control replies are correlated messages. The shared flag is only a wake
// hint: an old notification or a delayed queue publication is not a reply.
// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.mjs).
const { receiveMessageOnPort } = process.getBuiltinModule("node:worker_threads");
import type { MessagePort } from "node:worker_threads";

type Dynamic = ReturnType<typeof JSON.parse>;
type Flag = Int32Array<SharedArrayBuffer>;
interface ControlCall {
  id: number;
  request: Dynamic;
}
interface ControlReply {
  id: number;
  result?: Dynamic;
  fault?: string;
}

const post = (port: MessagePort, flag: Flag, id: number, reply: Omit<ControlReply, "id">): void => {
  port.postMessage({ id, ...reply });
  Atomics.store(flag, 0, 1);
  Atomics.notify(flag, 0);
};

export function receiveReply(
  port: MessagePort,
  flag: Flag,
  id: number,
  left: () => number,
): ControlReply | null {
  for (;;) {
    const remaining = left();
    if (remaining <= 0) {
      return null;
    }
    const reply = receiveMessageOnPort(port);
    if (reply) {
      if (reply.message.id === id) {
        return reply.message;
      }
      continue;
    }
    // Poll after a bounded wait even if the matching notification already
    // happened before the port made its message visible to this thread.
    Atomics.wait(flag, 0, Atomics.load(flag, 0), Math.min(1, remaining));
  }
}

export function postReply(port: MessagePort, flag: Flag, id: number, result: Dynamic): void {
  post(port, flag, id, { result });
}

// Serve one capability worker's correlated control calls. A thrown error is
// a fault in the host protocol, not an ordinary provider error for the guest.
export function serve(
  port: MessagePort,
  flag: Flag,
  run: (request: Dynamic) => Dynamic | Promise<Dynamic>,
  closes: (request: Dynamic) => boolean = () => false,
): void {
  port.on("message", async ({ id, request }: ControlCall) => {
    let reply: Omit<ControlReply, "id">;
    try {
      reply = { result: await run(request) };
    } catch (error) {
      reply = { fault: String((error as Dynamic)?.message ?? error) };
    }
    post(port, flag, id, reply);
    if (closes(request)) {
      port.close();
    }
  });
}
