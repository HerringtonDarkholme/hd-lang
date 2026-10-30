// fannkuch-redux, n = 10 (benchmarks game). Prints checksum and max flips.
const N = 10;
const perm = new Int32Array(N);
const perm1 = new Int32Array(N);
const count = new Int32Array(N);
for (let i = 0; i < N; i++) {
  perm1[i] = i;
}
let maxFlips = 0;
let checksum = 0;
let permCount = 0;
let r = N;
for (;;) {
  while (r !== 1) {
    count[r - 1] = r;
    r--;
  }
  for (let i = 0; i < N; i++) {
    perm[i] = perm1[i];
  }
  let flips = 0;
  let k = perm[0];
  while (k !== 0) {
    let i = 0;
    let j = k;
    while (i < j) {
      const t = perm[i];
      perm[i] = perm[j];
      perm[j] = t;
      i++;
      j--;
    }
    flips++;
    k = perm[0];
  }
  if (flips > maxFlips) {
    maxFlips = flips;
  }
  checksum += permCount % 2 === 0 ? flips : -flips;
  let done = true;
  while (r !== N) {
    const p0 = perm1[0];
    for (let i = 0; i < r; i++) {
      perm1[i] = perm1[i + 1];
    }
    perm1[r] = p0;
    count[r]--;
    if (count[r] > 0) {
      done = false;
      break;
    }
    r++;
  }
  if (done) {
    break;
  }
  permCount++;
}
console.log(`${checksum} ${maxFlips}`);
