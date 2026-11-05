// The same tree and two visitor interfaces as plugins.hd.
class Linear {
  constructor(multiplier) {
    this.multiplier = multiplier;
  }
  value(value, depth) {
    return value * this.multiplier + depth;
  }
}
class DepthBias {
  constructor(bias) {
    this.bias = bias;
  }
  value(value, depth) {
    return value + depth * this.bias;
  }
}
class Totals {
  constructor() {
    this.total = 0;
    this.count = 0;
  }
  emit(value, _depth) {
    this.total += value;
    this.count++;
  }
}
class Histogram {
  constructor() {
    this.buckets = [0, 0, 0, 0];
    this.count = 0;
  }
  emit(value, depth) {
    this.buckets[depth % this.buckets.length] += value;
    this.count++;
  }
}
function tree(depth, seed) {
  const children = [];
  if (depth > 0) {
    for (let branch = 0; branch < 3; branch++) {
      children.push(tree(depth - 1, seed * 3 + branch + 1));
    }
  }
  return { value: seed % 97, children };
}
function walk(node, score, sink, depth) {
  sink.emit(score.value(node.value, depth), depth);
  for (const child of node.children) {
    walk(child, score, sink, depth + 1);
  }
}
const root = tree(6, 1),
  linear = new Linear(3),
  biased = new DepthBias(7);
const totals = new Totals(),
  histogram = new Histogram();
for (let round = 0; round < 15000; round++) {
  if (round % 2 === 0) {
    walk(root, linear, totals, 0);
  } else {
    walk(root, biased, histogram, 0);
  }
}
const bucketTotal = histogram.buckets.reduce((sum, value) => sum + value, 0);
console.log(`${totals.total} ${bucketTotal} ${totals.count + histogram.count}`);
