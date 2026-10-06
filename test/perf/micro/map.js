function main() {
  const table = new Map();
  for (let i = 0; i < 100_000; i++) table.set(i, i * 2);
  let total = 0;
  for (let i = 0; i < 100_000; i++) total += table.get(i) ?? 0;
  if (total !== 9999900000) throw new Error(`wrong: ${total}`);
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((a, b) => a - b);
console.log(times[1].toFixed(1));
