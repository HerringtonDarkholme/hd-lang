// Control replies are correlated messages. The shared flag is only a wake
// hint: an old notification or a delayed queue publication is not a reply.
// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.ts).
const { receiveMessageOnPort } = process.getBuiltinModule("node:worker_threads");
import type { MessagePort } from "node:worker_threads";

type Dynamic = ReturnType<typeof JSON.parse>;
export type Flag = Int32Array<SharedArrayBuffer>;
export interface ReadinessTarget {
  port: Pick<MessagePort, "postMessage">;
  flag: Flag;
  reactor?: Flag;
}

// Publish before waking either a synchronous control caller or the reactor.
// Worker notifications name just the operation whose result changed.
export function postReady(target: ReadinessTarget, key: string): void {
  target.port.postMessage({ ready: key });
  Atomics.store(target.flag, 0, 1);
  Atomics.notify(target.flag, 0);
  if (target.reactor) {
    Atomics.add(target.reactor, 0, 1);
    Atomics.notify(target.reactor, 0);
  }
}

// Existing worker state machines retain their result until finish/abort.
// Attach notification to that one transition, including synchronous results.
export function notifyResult<State extends { result: unknown }>(
  state: State,
  target: ReadinessTarget,
  key: string,
): State {
  let value = state.result;
  let notified = false;
  const notify = (): void => {
    if (value !== null && !notified) {
      notified = true;
      postReady(target, key);
    }
  };
  Object.defineProperty(state, "result", {
    enumerable: true,
    get: () => value,
    set: (result: unknown) => {
      value = result;
      notify();
    },
  });
  notify();
  return state;
}
interface ControlCall<Request> {
  id: number;
  request: Request;
}
interface ControlReply {
  id: number;
  result?: Dynamic;
  fault?: string;
}

const post = (
  port: Pick<MessagePort, "postMessage">,
  flag: Flag,
  id: number,
  reply: Omit<ControlReply, "id">,
): void => {
  port.postMessage({ id, ...reply });
  Atomics.store(flag, 0, 1);
  Atomics.notify(flag, 0);
};

export function receiveReply(
  port: MessagePort,
  flag: Flag,
  id: number,
  wait: (() => number) | { left: () => number; ready: (key: string) => void },
): ControlReply | null {
  const { left, ready } =
    typeof wait === "function" ? { left: wait, ready: (_key: string) => {} } : wait;
  for (;;) {
    const remaining = left();
    if (remaining <= 0) {
      return null;
    }
    const reply = receiveMessageOnPort(port);
    if (reply) {
      if (typeof reply.message.ready === "string") {
        ready(reply.message.ready);
        continue;
      }
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

export function postReply(
  port: Pick<MessagePort, "postMessage">,
  flag: Flag,
  id: number,
  result: Dynamic,
): void {
  post(port, flag, id, { result });
}

// Serve one capability worker's correlated control calls. A thrown error is
// a fault in the host protocol, not an ordinary provider error for the guest.
export function serve<Request, Reply>(
  port: MessagePort,
  flag: Flag,
  run: (request: Request) => Reply | Promise<Reply>,
  closes: (request: Request) => boolean = () => false,
): void {
  port.on("message", async ({ id, request }: ControlCall<Request>) => {
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
