function main() {
  const parts = [];
  for (let i = 0; i < 100_000; i++) parts.push(`part-${i};`);
  const text = parts.join("");
  if (text.length !== 1_088_890) throw new Error(`wrong length: ${text.length}`);
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((a, b) => a - b);
console.log(times[1].toFixed(1));
