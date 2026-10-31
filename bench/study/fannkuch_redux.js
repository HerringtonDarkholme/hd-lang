// fannkuch-redux, n = 10 (benchmarks game). Prints checksum and max flips.
const SIZE = 10;
const perm = new Int32Array(SIZE);
const perm1 = new Int32Array(SIZE);
const count = new Int32Array(SIZE);
for (let i = 0; i < SIZE; i++) {
  perm1[i] = i;
}
let maxFlips = 0;
let checksum = 0;
let permCount = 0;
let rotation = SIZE;
for (;;) {
  while (rotation !== 1) {
    count[rotation - 1] = rotation;
    rotation--;
  }
  for (let i = 0; i < SIZE; i++) {
    perm[i] = perm1[i];
  }
  let flips = 0;
  let k = perm[0];
  while (k !== 0) {
    let i = 0;
    let j = k;
    while (i < j) {
      const swapped = perm[i];
      perm[i] = perm[j];
      perm[j] = swapped;
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
  while (rotation !== SIZE) {
    const p0 = perm1[0];
    for (let i = 0; i < rotation; i++) {
      perm1[i] = perm1[i + 1];
    }
    perm1[rotation] = p0;
    count[rotation]--;
    if (count[rotation] > 0) {
      done = false;
      break;
    }
    rotation++;
  }
  if (done) {
    break;
  }
  permCount++;
}
console.log(`${checksum} ${maxFlips}`);
