// points: 3.3M two-field records, 10 rounds, each round a new list.

///|
let n : Int32 = 7_500_000

///|
struct Point {
  x : Int64
  y : Int64
}

///|
fn main {
  let mut points : Array[Point] = []
  for i in 0..<n {
    points.push({ x: i.to_int64(), y: (i * 7 % 1000).to_int64(), })
  }
  for round in 0..<10 {
    let r = round.to_int64()
    points = points.map(fn(p) {
      { x: (p.x + p.y) % 1_000_003L, y: (p.y * 3L + r) % 1000L, }
    })
  }
  let mut sum : Int64 = 0
  for p in points {
    sum = sum + p.x + p.y
  }
  println(sum)
}
