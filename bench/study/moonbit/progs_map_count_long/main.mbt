///|
fn tally(n : Int32, buckets : Int32) -> Int32 {
  let counts : Map[Int32, Int32] = Map([])
  for i in 0..<n {
    let key = (i * 31 + 7) % buckets
    counts[key] = match counts.get(key) {
      Some(seen) => seen + 1
      None => 1
    }
  }
  let mut total = 0
  for b in 0..<buckets {
    total = total + counts.get(b).unwrap_or(0)
  }
  total
}

///|
fn main {
  println(tally(20000000, 5000))
}
