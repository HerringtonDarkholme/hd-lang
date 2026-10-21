// mandelbrot: a 1400 x 1400 grid, 50 iterations. Prints the points inside.
const SIZE = 1400;
const ITER = 50;
let inside = 0;
for (let y = 0; y < SIZE; y++) {
  const ci = (2 * y) / SIZE - 1;
  for (let x = 0; x < SIZE; x++) {
    const cr = (2 * x) / SIZE - 1.5;
    let zr = 0;
    let zi = 0;
    let i = 0;
    while (i < ITER && zr * zr + zi * zi <= 4) {
      const t = zr * zr - zi * zi + cr;
      zi = 2 * zr * zi + ci;
      zr = t;
      i++;
    }
    if (i === ITER) inside++;
  }
}
console.log(inside);
