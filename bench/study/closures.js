// closures: 170k items, 50 rounds of map/filter/fold through function values,
// as the hd and Go versions do (one fold taking three functions per round).
const N = 170_000;
const items = new Int32Array(N);
for (let i = 0; i < N; i++) items[i] = (i * 31 + 7) % 100_003;

function fold(items, m, keep, step) {
  let acc = 0;
  for (const x of items) {
    const y = m(x);
    if (keep(y)) acc = step(acc, y);
  }
  return acc;
}

let total = 0;
for (let round = 0; round < 50; round++) {
  const r = round;
  const acc = fold(
    items,
    (x) => x * 3 + r,
    (y) => y % 2 === 0,
    (a, y) => (a + y) % 1_000_003,
  );
  total = (total + acc) % 1_000_003;
}
console.log(total);
