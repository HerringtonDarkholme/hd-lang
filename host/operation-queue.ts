// Indexed binary heaps follow Node's internal/priority_queue.js: cancel
// removes an entry in O(log n), rather than retaining cancelled tombstones.
class Heap<Element> {
  private items: Element[] = [];
  private positions = new Map<Element, number>();
  private compare: (left: Element, right: Element) => number;
  constructor(compare: (left: Element, right: Element) => number) {
    this.compare = compare;
  }
  peek(): Element | undefined {
    return this.items[0];
  }
  has(item: Element): boolean {
    return this.positions.has(item);
  }
  add(item: Element): void {
    if (this.has(item)) {
      return;
    }
    this.items.push(item);
    this.up(this.items.length - 1, item);
  }
  remove(item: Element): void {
    const at = this.positions.get(item);
    if (at === undefined) {
      return;
    }
    const last = this.items.pop()!;
    this.positions.delete(item);
    if (at === this.items.length) {
      return;
    }
    if (at > 0 && this.compare(last, this.items[Math.floor((at - 1) / 2)]) < 0) {
      this.up(at, last);
    } else {
      this.down(at, last);
    }
  }
  private put(at: number, item: Element): void {
    this.items[at] = item;
    this.positions.set(item, at);
  }
  private up(at: number, item: Element): void {
    while (at > 0) {
      const parent = Math.floor((at - 1) / 2);
      if (this.compare(this.items[parent], item) <= 0) {
        break;
      }
      this.put(at, this.items[parent]);
      at = parent;
    }
    this.put(at, item);
  }
  private down(at: number, item: Element): void {
    while (at * 2 + 1 < this.items.length) {
      let child = at * 2 + 1;
      if (
        child + 1 < this.items.length &&
        this.compare(this.items[child + 1], this.items[child]) < 0
      ) {
        child++;
      }
      if (this.compare(item, this.items[child]) <= 0) {
        break;
      }
      this.put(at, this.items[child]);
      at = child;
    }
    this.put(at, item);
  }
}

export interface ScheduledOperation {
  handle: number;
  at: number;
  since: number;
  poll: (() => boolean) | null;
  promise: boolean;
}

// Timers and delivered readiness have separate frontiers. Only legacy
// probe-only operations are swept. No turn walks the pending-operation map.
export class OperationQueue<Element extends ScheduledOperation> {
  private deadlines = new Heap<Element>(
    (left, right) => left.at - right.at || left.handle - right.handle,
  );
  private ready = new Heap<Element>(
    (left, right) => left.since - right.since || left.handle - right.handle,
  );
  private legacy = new Set<Element>();
  add(operation: Element, notifies: boolean): void {
    if (!operation.poll || (!notifies && !operation.promise)) {
      this.deadlines.add(operation);
    }
    if (operation.poll && !notifies) {
      this.legacy.add(operation);
    }
  }
  notify(operation: Element): void {
    this.ready.add(operation);
  }
  remove(operation: Element): void {
    this.deadlines.remove(operation);
    this.ready.remove(operation);
    this.legacy.delete(operation);
  }
  nextDeadline(): number | null {
    return this.deadlines.peek()?.at ?? null;
  }
  take(now: number): Element[] {
    // A legacy probe is checked before timeout delivery, even if its last
    // failed probe scheduled a later idle retry. It may consume readiness.
    for (const operation of this.legacy) {
      this.ready.add(operation);
    }
    for (;;) {
      const operation = this.deadlines.peek();
      if (!operation || operation.at > now) {
        break;
      }
      this.deadlines.remove(operation);
      this.ready.add(operation);
    }
    const due: Element[] = [];
    let readyAt: number | null = null;
    for (;;) {
      const operation = this.ready.peek();
      if (!operation || (readyAt !== null && operation.since !== readyAt)) {
        break;
      }
      this.ready.remove(operation);
      if (operation.poll && !operation.poll()) {
        // Remove before changing either heap key.
        this.deadlines.remove(operation);
        operation.since = performance.now();
        operation.at = operation.since + 1;
        if (this.legacy.has(operation) && !operation.promise) {
          this.deadlines.add(operation);
        }
        continue;
      }
      readyAt = operation.since;
      this.remove(operation);
      due.push(operation);
    }
    return due;
  }
}
