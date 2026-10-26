///|
fn build(n : Int32) -> String {
  let mut out = ""
  for _ in 0..<n {
    out = out + "abcdefgh"
  }
  out
}

///|
fn main {
  println(build(40000).length())
}
