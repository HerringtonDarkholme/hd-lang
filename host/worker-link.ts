// std.remote's host workers, the part Node (workers.ts) and the browser
// (web.ts) share (std-remote.workers.host): the parent's sends in flight,
// by number, and a worker's frames not yet taken. A send is a pending
// operation (std-host.op.wait), so sends from several tasks are in flight
// together. A cancelled send (std-host.op.cancel) stops waiting, and the
// worker drops its frame unless it already runs it, as tokio's
// `spawn_blocking` abort does; a late reply finds no send. A worker whose
// instance ended makes each send still in flight `Unreachable`, and the
// next send starts another.
// Cost per send: one map entry, one message each way with its frame's
// buffer moved, not copied; a crash settles each send in flight once.
import { CrossError } from "./gen/abi.ts";
import { pendingOperation } from "./core.ts";
import { Err, Ok, variant } from "./wire.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// A running worker: `post` sends it a message, moving `frame`'s buffer.
export interface Link {
  post(message: Dynamic, frame?: Uint8Array): void;
}

// A send in flight: its reply once it came, and the wake of its operation.
type Send = { reply: Dynamic; ready: (() => void) | null };
type Open<Worker> = { link: Worker; sends: Map<number, Send> };

const lost = (): Dynamic => Err(variant(CrossError, "Unreachable"));

// `hd:Workers.send` over the worker `start` opens at the first send, or
// `null` where none can start. `start` gets the function that takes each
// message the worker posts back.
export function workerSends<Port extends Link>(
  start: (deliver: (message: Dynamic) => void) => Port | null,
): { send: (frame: Uint8Array) => Dynamic; current: () => Port | null } {
  let current: Open<Port> | null = null;
  let last = 0;
  const settle = (send: Send, reply: Dynamic): void => {
    send.reply = reply;
    send.ready?.();
  };
  const open = (): Open<Port> | null => {
    const sends = new Map<number, Send>();
    let made: Open<Port> | null = null;
    const deliver = (message: Dynamic): void => {
      if (message.lost) {
        if (current === made) {
          current = null;
        }
        for (const send of sends.values()) {
          settle(send, lost());
        }
        sends.clear();
        return;
      }
      const send = sends.get(message.id);
      if (send) {
        sends.delete(message.id);
        settle(send, Ok(message.frame));
      }
    };
    const link = start(deliver);
    made = link && { link, sends };
    return made;
  };
  const send = (frame: Uint8Array): Dynamic => {
    current ??= open();
    if (!current) {
      return lost();
    }
    const { link, sends } = current;
    const id = ++last;
    const entry: Send = { reply: null, ready: null };
    sends.set(id, entry);
    link.post({ id, frame }, frame);
    return pendingOperation(
      () => entry.reply !== null,
      () => entry.reply,
      () => {
        if (sends.delete(id)) {
          link.post({ cancel: id });
        }
      },
      null,
      (ready) => {
        entry.ready = ready;
        if (entry.reply !== null) {
          ready();
        }
        return () => {
          entry.ready = null;
        };
      },
    );
  };
  return { send, current: () => current?.link ?? null };
}

// A worker's frames not yet taken, by send, in the order they came: a
// cancel drops one. `taken` is the send of the frame `take` gave last,
// which the worker's reply answers.
export class Frames {
  private waiting = new Map<number, Uint8Array>();
  taken = 0;
  add({ id, frame, cancel }: Dynamic): void {
    if (cancel === undefined) {
      this.waiting.set(id, frame);
    } else {
      this.waiting.delete(cancel);
    }
  }
  ready(): boolean {
    return this.waiting.size > 0;
  }
  take(): Uint8Array | null {
    for (const [id, frame] of this.waiting) {
      this.waiting.delete(id);
      this.taken = id;
      return frame;
    }
    return null;
  }
}
