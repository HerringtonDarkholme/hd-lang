// closures: 170k items, 50 rounds of map/filter/fold.
const N = 170_000;
const items = new Int32Array(N);
for (let i = 0; i < N; i++) items[i] = (i * 31 + 7) % 100_003;
let total = 0;
for (let round = 0; round < 50; round++) {
  let acc = 0;
  for (const x of items) {
    const y = x * 3 + round;
    if (y % 2 === 0) acc = (acc + y) % 1_000_003;
  }
  total = (total + acc) % 1_000_003;
}
console.log(total);
