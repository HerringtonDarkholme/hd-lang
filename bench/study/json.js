// json: encode 400k records to JSON text, decode it back, check the sums.
const N = 400_000;
const records = [];
for (let i = 0; i < N; i++) {
  records.push({ id: i, name: `user${i}`, active: i % 3 === 0, score: (i * 37) % 1000, tags: [`t${i % 10}`, `g${i % 7}`] });
}
const text = JSON.stringify(records);
const back = JSON.parse(text);
let sum = 0;
for (const r of back) sum += r.score + r.tags[1].length + (r.active ? 1 : 0);
console.log(`${text.length} ${sum}`);
