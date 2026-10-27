///|
fn checksum(n : Int32) -> Int32 {
  let mut total = 0
  for i in 0..<n {
    let r = i % 10007
    total = (total + r * r % 10007) % 1000003
  }
  total
}

///|
fn main {
  println(checksum(80000000))
}
