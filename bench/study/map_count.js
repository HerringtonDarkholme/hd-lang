// map-count: build a text of 650k words, split it, count each word.
const WORD_COUNT = 2_500_000;
const parts = [];
for (let i = 0; i < WORD_COUNT; i++) {
  parts.push(`w${(i * 7919) % 50_021}`);
}
const text = parts.join(" ");
const counts = new Map();
for (const word of text.split(" ")) {
  counts.set(word, (counts.get(word) ?? 0) + 1);
}
let squares = 0;
for (const count of counts.values()) {
  squares += count * count;
}
console.log(`${counts.size} ${squares}`);
