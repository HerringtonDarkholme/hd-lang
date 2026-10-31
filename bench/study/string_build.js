// string-build: append 1.5M pieces to one string. Prints its length.
const ITEM_COUNT = 7_000_000;
const parts = [];
for (let i = 0; i < ITEM_COUNT; i++) {
  parts.push(`item-${i};`);
}
const text = parts.join("");
console.log(text.length);
