// points: 3.3M two-field records, 10 rounds, each round a new list.
const N = 7_500_000;
let points = [];
for (let i = 0; i < N; i++) {
  points.push({ x: i, y: (i * 7) % 1000 });
}
for (let round = 0; round < 10; round++) {
  points = points.map((p) => ({ x: (p.x + p.y) % 1_000_003, y: (p.y * 3 + round) % 1000 }));
}
let sum = 0;
for (const p of points) {
  sum += p.x + p.y;
}
console.log(sum);
