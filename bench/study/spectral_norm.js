// spectral-norm, n = 2000 (benchmarks game). Prints trunc(norm * 1e9).
const SIZE = 2800;
const coefficient = (i, j) => 1 / (((i + j) * (i + j + 1)) / 2 + i + 1);
function mulAv(count, input, out) {
  for (let i = 0; i < count; i++) {
    let sum = 0;
    for (let j = 0; j < count; j++) {
      sum += coefficient(i, j) * input[j];
    }
    out[i] = sum;
  }
}
function mulAtv(count, input, out) {
  for (let i = 0; i < count; i++) {
    let sum = 0;
    for (let j = 0; j < count; j++) {
      sum += coefficient(j, i) * input[j];
    }
    out[i] = sum;
  }
}
function mulAtAv(count, input, out, tmp) {
  mulAv(count, input, tmp);
  mulAtv(count, tmp, out);
}
const vectorU = new Float64Array(SIZE).fill(1);
const vectorV = new Float64Array(SIZE);
const tmp = new Float64Array(SIZE);
for (let i = 0; i < 10; i++) {
  mulAtAv(SIZE, vectorU, vectorV, tmp);
  mulAtAv(SIZE, vectorV, vectorU, tmp);
}
let vBv = 0;
let vv = 0;
for (let i = 0; i < SIZE; i++) {
  vBv += vectorU[i] * vectorV[i];
  vv += vectorV[i] * vectorV[i];
}
console.log(Math.trunc(Math.sqrt(vBv / vv) * 1e9));
