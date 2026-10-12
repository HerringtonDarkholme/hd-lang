function fib(n) {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}

function main() {
  const result = fib(30);
  if (result !== 832040) throw new Error(`wrong: ${result}`);
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((a, b) => a - b);
console.log(times[1].toFixed(1));
