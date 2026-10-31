function fib(count) {
  return count < 2 ? count : fib(count - 1) + fib(count - 2);
}

function main() {
  const result = fib(30);
  if (result !== 832040) {
    throw new Error(`wrong: ${result}`);
  }
}

const times = [];
for (let i = 0; i < 3; i++) {
  const start = performance.now();
  main();
  times.push(performance.now() - start);
}
times.sort((left, right) => left - right);
console.log(times[1].toFixed(1));
