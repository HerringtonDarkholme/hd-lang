// spectral-norm, n = 2000 (benchmarks game). Prints trunc(norm * 1e9).
const N = 2800;
const a = (i, j) => 1 / (((i + j) * (i + j + 1)) / 2 + i + 1);
function mulAv(n, v, out) {
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let j = 0; j < n; j++) s += a(i, j) * v[j];
    out[i] = s;
  }
}
function mulAtv(n, v, out) {
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let j = 0; j < n; j++) s += a(j, i) * v[j];
    out[i] = s;
  }
}
function mulAtAv(n, v, out, tmp) {
  mulAv(n, v, tmp);
  mulAtv(n, tmp, out);
}
const u = new Float64Array(N).fill(1);
const v = new Float64Array(N);
const tmp = new Float64Array(N);
for (let i = 0; i < 10; i++) {
  mulAtAv(N, u, v, tmp);
  mulAtAv(N, v, u, tmp);
}
let vBv = 0;
let vv = 0;
for (let i = 0; i < N; i++) {
  vBv += u[i] * v[i];
  vv += v[i] * v[i];
}
console.log(Math.trunc(Math.sqrt(vBv / vv) * 1e9));
