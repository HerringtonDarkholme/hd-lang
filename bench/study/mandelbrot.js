// mandelbrot: a 1400 x 1400 grid, 50 iterations. Prints the points inside.
const SIZE = 1400;
const ITER = 50;
let inside = 0;
for (let row = 0; row < SIZE; row++) {
  const ci = (2 * row) / SIZE - 1;
  for (let column = 0; column < SIZE; column++) {
    const cr = (2 * column) / SIZE - 1.5;
    let zr = 0;
    let zi = 0;
    let i = 0;
    while (i < ITER && zr * zr + zi * zi <= 4) {
      const nextReal = zr * zr - zi * zi + cr;
      zi = 2 * zr * zi + ci;
      zr = nextReal;
      i++;
    }
    if (i === ITER) {
      inside++;
    }
  }
}
console.log(inside);
