function main() {
  let total = 0;
  for (let i = 1; i <= 10_000_000; i++) total += i;
  if (total !== 50000005000000) throw new Error(`wrong: ${total}`);
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((a, b) => a - b);
console.log(times[1].toFixed(1));
