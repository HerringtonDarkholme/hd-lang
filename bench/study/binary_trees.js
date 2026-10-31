// binary-trees, depth 14 (benchmarks game). Prints one line per depth.
const MAX = 17;
const MIN = 4;
function make(depth) {
  return depth === 0 ? { l: null, r: null } : { l: make(depth - 1), r: make(depth - 1) };
}
function check(tree) {
  return tree.l === null ? 1 : 1 + check(tree.l) + check(tree.r);
}
const out = [];
out.push(`stretch ${MAX + 1} ${check(make(MAX + 1))}`);
const long = make(MAX);
for (let depth = MIN; depth <= MAX; depth += 2) {
  const iters = 1 << (MAX - depth + MIN);
  let sum = 0;
  for (let i = 0; i < iters; i++) {
    sum += check(make(depth));
  }
  out.push(`${iters} ${depth} ${sum}`);
}
out.push(`long ${MAX} ${check(long)}`);
console.log(out.join("\n"));
