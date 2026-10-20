// nbody, 5M steps (benchmarks game). Prints trunc(energy * 1e9) before and after.
const PI = 3.141592653589793;
const SOLAR_MASS = 4 * PI * PI;
const DAYS = 365.24;
const STEPS = 5_000_000;
const x = [0, 4.84143144246472090e00, 8.34336671824457987e00, 1.28943695621391310e01, 1.53796971148509165e01];
const y = [0, -1.16032004402742839e00, 4.12479856412430479e00, -1.51111514016986312e01, -2.59193146099879641e01];
const z = [0, -1.03622044471123109e-01, -4.03523417114321381e-01, -2.23307578892655734e-01, 1.79258772950371181e-01];
const vx = [0, 1.66007664274403694e-03, -2.76742510726862411e-03, 2.96460137564761618e-03, 2.68067772490389322e-03].map((v) => v * DAYS);
const vy = [0, 7.69901118419740425e-03, 4.99852801234917238e-03, 2.37847173959480950e-03, 1.62824170038242295e-03].map((v) => v * DAYS);
const vz = [0, -6.90460016972063023e-05, 2.30417297573763929e-05, -2.96589568540237556e-05, -9.51592254519715870e-05].map((v) => v * DAYS);
const mass = [1, 9.54791938424326609e-04, 2.85885980666130812e-04, 4.36624404335156298e-05, 5.15138902046611451e-05].map((m) => m * SOLAR_MASS);
const n = 5;
let px = 0, py = 0, pz = 0;
for (let i = 0; i < n; i++) {
  px += vx[i] * mass[i];
  py += vy[i] * mass[i];
  pz += vz[i] * mass[i];
}
vx[0] = -px / SOLAR_MASS;
vy[0] = -py / SOLAR_MASS;
vz[0] = -pz / SOLAR_MASS;
function energy() {
  let e = 0;
  for (let i = 0; i < n; i++) {
    e += 0.5 * mass[i] * (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]);
    for (let j = i + 1; j < n; j++) {
      const dx = x[i] - x[j], dy = y[i] - y[j], dz = z[i] - z[j];
      e -= (mass[i] * mass[j]) / Math.sqrt(dx * dx + dy * dy + dz * dz);
    }
  }
  return e;
}
function advance(dt) {
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      const dx = x[i] - x[j], dy = y[i] - y[j], dz = z[i] - z[j];
      const d2 = dx * dx + dy * dy + dz * dz;
      const mag = dt / (d2 * Math.sqrt(d2));
      vx[i] -= dx * mass[j] * mag;
      vy[i] -= dy * mass[j] * mag;
      vz[i] -= dz * mass[j] * mag;
      vx[j] += dx * mass[i] * mag;
      vy[j] += dy * mass[i] * mag;
      vz[j] += dz * mass[i] * mag;
    }
  }
  for (let i = 0; i < n; i++) {
    x[i] += dt * vx[i];
    y[i] += dt * vy[i];
    z[i] += dt * vz[i];
  }
}
const before = Math.trunc(energy() * 1e9);
for (let s = 0; s < STEPS; s++) advance(0.01);
console.log(`${before} ${Math.trunc(energy() * 1e9)}`);
