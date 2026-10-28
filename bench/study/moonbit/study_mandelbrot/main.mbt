// mandelbrot: a 1400 x 1400 grid, 50 iterations. Prints the points inside.

///|
let size : Int32 = 1400

///|
let iter : Int32 = 50

///|
fn main {
  let mut inside = 0
  for y = 0; y < size; y = y + 1 {
    let ci = 2.0 * y.to_double() / size.to_double() - 1.0
    for x = 0; x < size; x = x + 1 {
      let cr = 2.0 * x.to_double() / size.to_double() - 1.5
      let mut zr = 0.0
      let mut zi = 0.0
      let mut i = 0
      while i < iter && zr * zr + zi * zi <= 4.0 {
        let t = zr * zr - zi * zi + cr
        zi = 2.0 * zr * zi + ci
        zr = t
        i = i + 1
      }
      if i == iter {
        inside = inside + 1
      }
    }
  }
  println(inside)
}
