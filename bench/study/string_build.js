// string-build: append 3M pieces to one string. Prints its length.
const N = 3_000_000;
const parts = [];
for (let i = 0; i < N; i++) parts.push(`item-${i};`);
const text = parts.join("");
console.log(text.length);
