// binary-trees, depth 20 (benchmarks game). Prints one line per depth.
const MAX = 20;
const MIN = 4;
function make(d) {
  return d === 0 ? { l: null, r: null } : { l: make(d - 1), r: make(d - 1) };
}
function check(t) {
  return t.l === null ? 1 : 1 + check(t.l) + check(t.r);
}
const out = [];
out.push(`stretch ${MAX + 1} ${check(make(MAX + 1))}`);
const long = make(MAX);
for (let d = MIN; d <= MAX; d += 2) {
  const iters = 1 << (MAX - d + MIN);
  let sum = 0;
  for (let i = 0; i < iters; i++) sum += check(make(d));
  out.push(`${iters} ${d} ${sum}`);
}
out.push(`long ${MAX} ${check(long)}`);
console.log(out.join("\n"));
