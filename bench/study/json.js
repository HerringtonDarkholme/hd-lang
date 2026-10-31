// json: encode the benchmark records to JSON text, decode them back, check the sums.
const RECORD_COUNT = 170_000;
const records = [];
for (let i = 0; i < RECORD_COUNT; i++) {
  records.push({
    id: i,
    name: `user${i}`,
    active: i % 3 === 0,
    score: (i * 37) % 1000,
    tags: [`t${i % 10}`, `g${i % 7}`],
  });
}
const text = JSON.stringify(records);
const back = JSON.parse(text);
let sum = 0;
for (const record of back) {
  sum += record.score + record.tags[1].length + (record.active ? 1 : 0);
}
console.log(`${text.length} ${sum}`);
