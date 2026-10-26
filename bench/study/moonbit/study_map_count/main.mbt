// map-count: build a text of 650k words, split it, count each word.

///|
let n : Int32 = 650_000

///|
fn main {
  let out = StringBuilder()
  for i in 0..<n {
    if i > 0 {
      out.write_string(" ")
    }
    out.write_string("w\{i.to_uint64() * 7919UL % 50_021UL}")
  }
  let text = out.to_string()
  let counts : Map[String, Int64] = Map([])
  for word in text.split(" ") {
    let w = word.to_owned()
    counts[w] = counts.get(w).unwrap_or(0L) + 1L
  }
  let mut squares : Int64 = 0
  for _, c in counts {
    squares = squares + c * c
  }
  println("\{counts.length()} \{squares}")
}
