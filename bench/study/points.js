// points: 3.3M two-field records, 10 rounds, each round a new list.
const POINT_COUNT = 7_500_000;
let points = [];
for (let i = 0; i < POINT_COUNT; i++) {
  points.push({ x: i, y: (i * 7) % 1000 });
}
for (let round = 0; round < 10; round++) {
  points = points.map((point) => ({
    x: (point.x + point.y) % 1_000_003,
    y: (point.y * 3 + round) % 1000,
  }));
}
let sum = 0;
for (const point of points) {
  sum += point.x + point.y;
}
console.log(sum);
