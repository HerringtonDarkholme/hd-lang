import assert from "node:assert/strict";

interface PendingContext {
  signal: AbortSignal;
  pending<Result>(
    ready: () => boolean,
    done: () => Result,
    abort: () => void,
    finish: () => Result,
  ): unknown;
}

let aborted = 0;
export default {
  wait(kind: string, context: PendingContext) {
    let ready = false;
    const timer = setTimeout(
      () => {
        ready = true;
      },
      kind === "cancel" ? 5000 : 1,
    );
    return context.pending(
      () => ready,
      () => "computed",
      () => {
        assert.equal(context.signal.aborted, true);
        clearTimeout(timer);
        aborted++;
      },
      () => {
        assert.equal(context.signal.aborted, false);
        clearTimeout(timer);
        return "delivered";
      },
    );
  },
  aborts() {
    return aborted;
  },
};
