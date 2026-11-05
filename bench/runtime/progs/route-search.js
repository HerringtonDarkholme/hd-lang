// The same grid, neighbor order, max-heap and ring-deque algorithms as hd.
function neighbors(node, width) {
  const result = [];
  if (node % width > 0) {
    result.push(node - 1);
  }
  if ((node % width) + 1 < width) {
    result.push(node + 1);
  }
  if (node >= width) {
    result.push(node - width);
  }
  if (node + width < width * width) {
    result.push(node + width);
  }
  return result;
}
const greater = (left, right) => left[0] > right[0] || (left[0] === right[0] && left[1] > right[1]);
class Heap {
  constructor() {
    this.items = [];
  }
  push(value) {
    const last = this.items.length;
    this.items.push(value);
    let child = last;
    while (child > 0) {
      const parent = Math.floor((child - 1) / 2);
      if (!greater(value, this.items[parent])) {
        break;
      }
      this.items[child] = this.items[parent];
      child = parent;
    }
    if (child !== last) {
      this.items[child] = value;
    }
  }
  pop() {
    const last = this.items.pop();
    if (this.items.length === 0) {
      return last;
    }
    const top = this.items[0],
      size = this.items.length;
    let parent = 0;
    while (parent < Math.floor(size / 2)) {
      const left = 2 * parent + 1;
      let larger = left;
      if (left + 1 < size && greater(this.items[left + 1], this.items[left])) {
        larger = left + 1;
      }
      if (!greater(this.items[larger], last)) {
        break;
      }
      this.items[parent] = this.items[larger];
      parent = larger;
    }
    this.items[parent] = last;
    return top;
  }
}
class Deque {
  constructor() {
    this.slots = [];
    this.head = 0;
    this.count = 0;
  }
  push(value) {
    if (this.count === this.slots.length) {
      const old = this.slots,
        grown = Array.from({ length: old.length < 4 ? 4 : old.length * 2 });
      for (let index = 0; index < this.count; index++) {
        grown[index] = old[(this.head + index) % old.length];
      }
      this.slots = grown;
      this.head = 0;
    }
    this.slots[(this.head + this.count) % this.slots.length] = value;
    this.count++;
  }
  pop() {
    const value = this.slots[this.head];
    this.slots[this.head] = undefined;
    this.head = (this.head + 1) % this.slots.length;
    this.count--;
    return value;
  }
}
function dijkstra(width) {
  const distance = Array.from({ length: width * width }, () => 1000000000),
    pending = new Heap(),
    visited = new Set();
  distance[0] = 0;
  pending.push([0, 0]);
  let total = 0;
  while (pending.items.length !== 0) {
    const entry = pending.pop(),
      node = entry[1];
    if (visited.has(node)) {
      continue;
    }
    visited.add(node);
    total -= entry[0];
    for (const next of neighbors(node, width)) {
      const cost = distance[node] + 1 + ((node * 17 + next * 31) % 9);
      if (cost < distance[next]) {
        distance[next] = cost;
        pending.push([-cost, next]);
      }
    }
  }
  return [total, visited.size];
}
function breadthFirst(width) {
  const distance = Array.from({ length: width * width }, () => 0),
    pending = new Deque(),
    visited = new Set([0]);
  pending.push(0);
  let total = 0;
  while (pending.count !== 0) {
    const node = pending.pop();
    total += distance[node];
    for (const next of neighbors(node, width)) {
      if (!visited.has(next)) {
        visited.add(next);
        distance[next] = distance[node] + 1;
        pending.push(next);
      }
    }
  }
  return [total, visited.size];
}
let weighted = 0,
  unweighted = 0,
  visited = 0;
for (let round = 0; round < 30; round++) {
  const width = 100 + (round % 3),
    route = dijkstra(width),
    breadth = breadthFirst(width);
  weighted += route[0];
  unweighted += breadth[0];
  visited += route[1] + breadth[1];
}
console.log(`${weighted} ${unweighted} ${visited}`);
