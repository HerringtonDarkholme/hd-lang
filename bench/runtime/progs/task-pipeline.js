// A bounded producer/transformer/consumer pipeline, with the same meter reads as hd.
class Channel {
  constructor(capacity) {
    this.capacity = capacity;
    this.items = [];
    this.closed = false;
    this.readers = [];
    this.writers = [];
  }
  async send(value) {
    while (!this.closed && this.items.length >= this.capacity) {
      await new Promise((resolve) => {
        this.writers.push(resolve);
      });
    }
    if (this.closed) {
      throw new Error("send on a closed channel");
    }
    this.items.push(value);
    for (const resolve of this.readers.splice(0)) {
      resolve();
    }
  }
  async recv() {
    for (;;) {
      if (this.items.length !== 0) {
        const value = this.items.shift();
        for (const resolve of this.writers.splice(0)) {
          resolve();
        }
        return value;
      }
      if (this.closed) {
        return null;
      }
      await new Promise((resolve) => {
        this.readers.push(resolve);
      });
    }
  }
  close() {
    this.closed = true;
    for (const resolve of this.readers.splice(0)) {
      resolve();
    }
    for (const resolve of this.writers.splice(0)) {
      resolve();
    }
  }
}
class Counters {
  constructor() {
    this.calls = 0;
    this.total = 0;
  }
  record(value) {
    this.calls++;
    this.total += value;
  }
}
async function produce(output, count, meter) {
  for (let index = 0; index < count; index++) {
    const value = index % 1009;
    meter.record(value);
    await output.send(value);
  }
  output.close();
}
async function transform(input, output, meter) {
  for (;;) {
    const value = await input.recv();
    if (value === null) {
      break;
    }
    const mapped = value * 3 + 7;
    meter.record(mapped);
    await output.send(mapped);
  }
  output.close();
}
async function consume(input, meter) {
  let total = 0;
  for (;;) {
    const value = await input.recv();
    if (value === null) {
      return total;
    }
    meter.record(value);
    total += value;
  }
}
const counters = new Counters(),
  first = new Channel(16),
  second = new Channel(16);
const result = await Promise.all([
  produce(first, 900000, counters),
  transform(first, second, counters),
  consume(second, counters),
]);
console.log(`${result[2]} ${counters.calls} ${counters.total}`);
