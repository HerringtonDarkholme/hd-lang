// spectral-norm, n = 2000 (benchmarks game). Prints trunc(norm * 1e9).

///|
let n : Int32 = 2000

///|
fn a(i : Int32, j : Int32) -> Double {
  1.0 / ((i + j) * (i + j + 1) / 2 + i + 1).to_double()
}

///|
fn mul_av(n : Int32, v : Array[Double], out : Array[Double]) -> Unit {
  for i in 0..<n {
    let mut s = 0.0
    for j in 0..<n {
      s = s + a(i, j) * v[j]
    }
    out[i] = s
  }
}

///|
fn mul_atv(n : Int32, v : Array[Double], out : Array[Double]) -> Unit {
  for i in 0..<n {
    let mut s = 0.0
    for j in 0..<n {
      s = s + a(j, i) * v[j]
    }
    out[i] = s
  }
}

///|
fn mul_atav(
  n : Int32,
  v : Array[Double],
  out : Array[Double],
  tmp : Array[Double],
) -> Unit {
  mul_av(n, v, tmp)
  mul_atv(n, tmp, out)
}

///|
fn main {
  let u : Array[Double] = []
  let v : Array[Double] = []
  let tmp : Array[Double] = []
  for _ in 0..<n {
    u.push(1.0)
    v.push(0.0)
    tmp.push(0.0)
  }
  for _ in 0..<10 {
    mul_atav(n, u, v, tmp)
    mul_atav(n, v, u, tmp)
  }
  let mut vbv = 0.0
  let mut vv = 0.0
  for i in 0..<n {
    vbv = vbv + u[i] * v[i]
    vv = vv + v[i] * v[i]
  }
  println(((vbv / vv).sqrt() * 1000000000.0).to_int64())
}
