// closures: 170k items, 50 rounds of map/filter/fold.

///|
let n : Int32 = 650_000

///|
fn main {
  let items : Array[Int32] = []
  for i in 0..<n {
    items.push((i * 31 + 7) % 100_003)
  }
  let mut total = 0
  for round in 0..<50 {
    let r = round
    let acc = items
      .iter()
      .map(fn(x) { x * 3 + r })
      .filter(fn(y) { y % 2 == 0 })
      .fold(init=0, fn(a, y) { (a + y) % 1_000_003 })
    total = (total + acc) % 1_000_003
  }
  println(total)
}
