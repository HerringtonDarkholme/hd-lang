// nbody (benchmarks game). Prints trunc(energy * 1e9) before and after.
const PI = 3.141592653589793;
const SOLAR_MASS = 4 * PI * PI;
const DAYS = 365.24;
const STEPS = 7_000_000;
const positionsX = [
  0, 4.8414314424647209, 8.34336671824457987, 1.2894369562139131e1, 1.53796971148509165e1,
];
const positionsY = [
  0, -1.16032004402742839, 4.12479856412430479, -1.51111514016986312e1, -2.59193146099879641e1,
];
const positionsZ = [
  0, -1.03622044471123109e-1, -4.03523417114321381e-1, -2.23307578892655734e-1,
  1.79258772950371181e-1,
];
const vx = [
  0, 1.66007664274403694e-3, -2.76742510726862411e-3, 2.96460137564761618e-3,
  2.68067772490389322e-3,
].map((item) => item * DAYS);
const vy = [
  0, 7.69901118419740425e-3, 4.99852801234917238e-3, 2.3784717395948095e-3, 1.62824170038242295e-3,
].map((item) => item * DAYS);
const vz = [
  0, -6.90460016972063023e-5, 2.30417297573763929e-5, -2.96589568540237556e-5,
  -9.5159225451971587e-5,
].map((item) => item * DAYS);
const mass = [
  1, 9.54791938424326609e-4, 2.85885980666130812e-4, 4.36624404335156298e-5, 5.15138902046611451e-5,
].map((item) => item * SOLAR_MASS);
const count = 5;
let px = 0,
  py = 0,
  pz = 0;
for (let i = 0; i < count; i++) {
  px += vx[i] * mass[i];
  py += vy[i] * mass[i];
  pz += vz[i] * mass[i];
}
vx[0] = -px / SOLAR_MASS;
vy[0] = -py / SOLAR_MASS;
vz[0] = -pz / SOLAR_MASS;
function energy() {
  let total = 0;
  for (let i = 0; i < count; i++) {
    total += 0.5 * mass[i] * (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]);
    for (let j = i + 1; j < count; j++) {
      const dx = positionsX[i] - positionsX[j],
        dy = positionsY[i] - positionsY[j],
        dz = positionsZ[i] - positionsZ[j];
      total -= (mass[i] * mass[j]) / Math.sqrt(dx * dx + dy * dy + dz * dz);
    }
  }
  return total;
}
function advance(dt) {
  for (let i = 0; i < count; i++) {
    for (let j = i + 1; j < count; j++) {
      const dx = positionsX[i] - positionsX[j],
        dy = positionsY[i] - positionsY[j],
        dz = positionsZ[i] - positionsZ[j];
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
  for (let i = 0; i < count; i++) {
    positionsX[i] += dt * vx[i];
    positionsY[i] += dt * vy[i];
    positionsZ[i] += dt * vz[i];
  }
}
const before = Math.trunc(energy() * 1e9);
for (let step = 0; step < STEPS; step++) {
  advance(0.01);
}
console.log(`${before} ${Math.trunc(energy() * 1e9)}`);
