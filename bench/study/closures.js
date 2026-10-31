// closures: 170k items, 50 rounds of map/filter/fold, written as idiomatic
// JavaScript: the typed array's own map/filter/reduce with arrow functions
// (eager, so each round builds the mapped and filtered arrays; hd's
// iterator chain is lazy).
const ITEM_COUNT = 650_000;
const items = new Int32Array(ITEM_COUNT);
for (let i = 0; i < ITEM_COUNT; i++) {
  items[i] = (i * 31 + 7) % 100_003;
}
let total = 0;
for (let round = 0; round < 50; round++) {
  const acc = items
    .map((item) => item * 3 + round)
    .filter((item) => item % 2 === 0)
    .reduce((accumulator, value) => (accumulator + value) % 1_000_003, 0);
  total = (total + acc) % 1_000_003;
}
console.log(total);
