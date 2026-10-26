///|
fn build(n : Int32) -> String {
  let out = StringBuilder()
  for _ in 0..<n {
    out.write_string("abcdefgh")
  }
  out.to_string()
}

///|
fn main {
  println(build(40000).length())
}
