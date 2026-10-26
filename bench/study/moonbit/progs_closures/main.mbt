///|
fn total(n : Int32) -> Int32 {
  let items : Array[Int32] = []
  for i in 0..<n {
    items.push((i * 31 + 7) % 100003)
  }
  items
  .iter()
  .map(fn(x) { x * 3 + 1 })
  .filter(fn(x) { x % 2 == 0 })
  .fold(init=0, fn(acc, x) { (acc + x) % 1000003 })
}

///|
fn main {
  println(total(20000000))
}
