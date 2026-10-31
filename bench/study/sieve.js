// sieve of Eratosthenes below 16M. Prints the prime count.
const LIMIT = 36_000_000;
const flags = new Uint8Array(LIMIT).fill(1);
flags[0] = 0;
flags[1] = 0;
for (let i = 2; i * i < LIMIT; i++) {
  if (flags[i]) {
    for (let j = i * i; j < LIMIT; j += i) {
      flags[j] = 0;
    }
  }
}
let count = 0;
for (let i = 0; i < LIMIT; i++) {
  count += flags[i];
}
console.log(count);
