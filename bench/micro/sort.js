function main() {
  const items = [];
  for (let i = 0; i < 100_000; i++) items.push((i * 48271) % 1_000_003);
  items.sort((a, b) => a - b);
  if (items[0] !== 0) throw new Error(`wrong min: ${items[0]}`);
  for (let i = 1; i < 100_000; i++)
    if (items[i] < items[i - 1]) throw new Error(`unsorted at ${i}`);
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((a, b) => a - b);
console.log(times[1].toFixed(1));
