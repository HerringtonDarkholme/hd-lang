// Deterministic resource cleanup and owned buffer batches, matching resources.hd.
class Session {
  constructor(counters) {
    this.counters = counters;
    this.active = true;
    counters.opened++;
    counters.live++;
  }
  dispose() {
    if (this.active) {
      this.counters.closed++;
      this.counters.live--;
      this.active = false;
    }
  }
}
class BufferLease {
  constructor() {
    this.data = new Uint8Array(256);
  }
  dispose() {
    this.data = null;
  }
}
function batch(counters, seed) {
  const session = new Session(counters),
    buffers = [];
  try {
    for (let part = 0; part < 8; part++) {
      const buffer = new BufferLease();
      for (let index = 0; index < 256; index++) {
        buffer.data[index] = (seed + part * 13 + index) % 251;
      }
      buffers.push(buffer);
    }
    let total = 0;
    for (const buffer of buffers) {
      for (const value of buffer.data) {
        total += value;
      }
    }
    return total;
  } finally {
    for (let index = buffers.length - 1; index >= 0; index--) {
      buffers[index].dispose();
    }
    session.dispose();
  }
}
const counters = { opened: 0, closed: 0, live: 0 };
let total = 0;
for (let index = 0; index < 20000; index++) {
  total += batch(counters, index);
}
console.log(`${total} ${counters.opened} ${counters.closed} ${counters.live}`);
