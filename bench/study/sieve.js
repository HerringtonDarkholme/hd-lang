// sieve of Eratosthenes below 30M. Prints the prime count.
const N = 30_000_000;
const flags = new Uint8Array(N).fill(1);
flags[0] = 0;
flags[1] = 0;
for (let i = 2; i * i < N; i++) {
  if (flags[i]) for (let j = i * i; j < N; j += i) flags[j] = 0;
}
let count = 0;
for (let i = 0; i < N; i++) count += flags[i];
console.log(count);
