// string-build: append 1.5M pieces to one string. Prints its length.

///|
let n : Int32 = 7_000_000

///|
fn main {
  let out = StringBuilder()
  for i in 0..<n {
    out.write_string("item-\{i};")
  }
  let text = out.to_string()
  println(text.length())
}
