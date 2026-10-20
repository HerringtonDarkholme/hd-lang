// map-count: build a text of 2M words, split it, count each word.
const N = 2_000_000;
const parts = [];
for (let i = 0; i < N; i++) parts.push(`w${(i * 7919) % 50_021}`);
const text = parts.join(" ");
const counts = new Map();
for (const word of text.split(" ")) counts.set(word, (counts.get(word) ?? 0) + 1);
let squares = 0;
for (const c of counts.values()) squares += c * c;
console.log(`${counts.size} ${squares}`);
