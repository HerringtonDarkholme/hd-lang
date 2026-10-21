// closures: 170k items, 50 rounds of map/filter/fold, written as idiomatic
// JavaScript: array methods with arrow functions (eager, so each round
// builds the mapped and filtered arrays; hd's iterator chain is lazy).
const N = 170_000;
const items = Array.from({ length: N }, (_, i) => (i * 31 + 7) % 100_003);
let total = 0;
for (let round = 0; round < 50; round++) {
  const acc = items
    .map((x) => x * 3 + round)
    .filter((y) => y % 2 === 0)
    .reduce((a, y) => (a + y) % 1_000_003, 0);
  total = (total + acc) % 1_000_003;
}
console.log(total);
