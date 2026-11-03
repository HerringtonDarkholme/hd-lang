const size = 1_000_000;
const items = [];
let state = 0x00c0ffee;
for (let i = 0; i < size; i++) {
  state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
  items.push(state);
}
items.sort((left, right) => left - right);
let total = 0n;
for (let i = 0; i < items.length; i += 1024) total ^= BigInt(items[i]);
total ^= BigInt(items[0]);
total ^= BigInt(items[items.length - 1]);
console.log(total.toString());
