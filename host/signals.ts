// One Node source owns process listeners for Signals and entry cancellation.
// A worker receives correlated one-shot notifications from that same source.
import { postReady, receiveReply, serve } from "./worker-control.ts";
import type { Flag } from "./worker-control.ts";
import type { MessagePort } from "node:worker_threads";

export type SignalKind = "Interrupt" | "Terminate" | "Hangup";
type Release = () => void;
interface Source {
  wait(kind: SignalKind, ready: () => void): Release;
}
const names = { Interrupt: "SIGINT", Terminate: "SIGTERM", Hangup: "SIGHUP" } as const;
const kinds = Object.keys(names) as SignalKind[];
const { isMainThread, receiveMessageOnPort } = process.getBuiltinModule("node:worker_threads");
const { openSync, readSync, closeSync } = process.getBuiltinModule("node:fs");
const waits = new Map(kinds.map((kind) => [kind, new Set<() => void>()]));
const fallbacks = new Map(kinds.map((kind) => [kind, new Set<() => void>()]));
const listeners = new Map<SignalKind, () => void>();
let relayListening = false;
let forwarded: { fd: number; offset: number } | null = null;
const ignore = () => {};
const dispatch = (kind: SignalKind): void => {
  const pending = waits.get(kind)!;
  const callbacks = [...(pending.size ? pending : fallbacks.get(kind)!)];
  pending.clear();
  update(kind);
  if (!callbacks.length) {
    // Managed CLI children ignore terminal duplicates; restore the original
    // kind's default before forwarding an unowned event to this process.
    process.off(names[kind], ignore);
    process.kill(process.pid, names[kind]);
  }
  for (const callback of callbacks) {
    callback();
  }
};
const forwardedReady = (): void => {
  const source = forwarded!;
  const bytes = Buffer.allocUnsafe(4096);
  for (;;) {
    const count = readSync(source.fd, bytes, 0, bytes.length, source.offset);
    if (!count) {
      return;
    }
    source.offset += count;
    for (let i = 0; i < count; i++) {
      const kind = kinds[bytes[i]];
      if (kind === undefined) {
        throw new Error("invalid signal relay kind");
      }
      dispatch(kind);
    }
  }
};
function update(kind: SignalKind): void {
  const active = waits.get(kind)!.size + fallbacks.get(kind)!.size > 0;
  if (active && !listeners.has(kind)) {
    const listener = () => dispatch(kind);
    listeners.set(kind, listener);
    if (!forwarded) {
      process.on(names[kind], listener);
    }
  } else if (!active && listeners.has(kind)) {
    if (!forwarded) {
      process.off(names[kind], listeners.get(kind)!);
    }
    listeners.delete(kind);
  }
  if (forwarded) {
    if (listeners.size && !relayListening) {
      process.on("SIGUSR2", forwardedReady);
      relayListening = true;
    } else if (!listeners.size && relayListening) {
      process.off("SIGUSR2", forwardedReady);
      relayListening = false;
    }
  }
}
function subscribe(kind: SignalKind, ready: () => void, fallback = false): Release {
  const set = (fallback ? fallbacks : waits).get(kind)!;
  set.add(ready);
  update(kind);
  return () => {
    set.delete(ready);
    update(kind);
  };
}
let source: Source | null = isMainThread ? { wait: subscribe } : null;
export const signalSource = (): Source | null => source;
export const cancelSignals = (request: () => void): Release => {
  const releases = [subscribe("Interrupt", request, true), subscribe("Terminate", request, true)];
  return () => {
    for (const release of releases) {
      release();
    }
  };
};
// CLI-only: SIGUSR2 wakes an append-only queue, retaining original kinds.
export function configureSignalRelay(path?: string): Release {
  if (!path) {
    return () => {};
  }
  forwarded = { fd: openSync(path, "r"), offset: 0 };
  for (const kind of kinds) {
    process.on(names[kind], ignore);
  }
  return () => {
    process.off("SIGUSR2", forwardedReady);
    relayListening = false;
    for (const kind of kinds) {
      process.off(names[kind], ignore);
    }
    closeSync(forwarded!.fd);
    forwarded = null;
  };
}
interface Request {
  key: string;
  kind?: SignalKind;
}
export function serveSignals(port: MessagePort, flag: Flag): Release {
  const leases = new Map<string, Release>();
  serve<Request, null>(port, flag, ({ key, kind }) => {
    if (kind) {
      const release = subscribe(kind, () => {
        leases.delete(key);
        postReady({ port, flag }, key);
      });
      leases.set(key, release);
    } else {
      leases.get(key)?.();
      leases.delete(key);
    }
    return null;
  });
  const close = () => {
    for (const release of leases.values()) {
      release();
    }
    leases.clear();
    port.close();
  };
  port.on("close", close);
  return close;
}
export function connectSignals(port: MessagePort, flag: Flag): Release {
  let serial = 0;
  const pending = new Map<string, () => void>();
  const ready = (key: string): void => {
    const callback = pending.get(key);
    pending.delete(key);
    callback?.();
  };
  const drain = (): void => {
    for (let message; (message = receiveMessageOnPort(port));) {
      if (typeof message.message.ready === "string") {
        ready(message.message.ready);
      }
    }
  };
  port.on("message", (message) => {
    if (typeof message.ready === "string") {
      ready(message.ready);
    }
  });
  port.unref();
  const call = (request: Request): void => {
    const id = ++serial;
    port.postMessage({ id, request });
    const deadline = performance.now() + 30_000;
    const reply = receiveReply(port, flag, id, { left: () => deadline - performance.now(), ready });
    if (!reply || reply.fault) {
      throw new Error(reply?.fault ?? "signal source did not acknowledge registration");
    }
  };
  source = {
    wait: (kind, callback) => {
      const key = String(++serial);
      pending.set(key, callback);
      try {
        call({ key, kind });
      } catch (error) {
        pending.delete(key);
        throw error;
      }
      drain();
      return () => {
        pending.delete(key);
        call({ key });
      };
    },
  };
  return () => {
    source = null;
    pending.clear();
    port.close();
  };
}
