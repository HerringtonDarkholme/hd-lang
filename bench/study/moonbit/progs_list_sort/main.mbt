// insertion sort of 30000 pseudo-random i32 values. Prints a checksum.

///|
fn fill(n : Int32) -> Array[Int32] {
  let items : Array[Int32] = []
  let mut state = 12345
  for _ in 0..<n {
    state = (state * 31 + 7) % 100003
    items.push(state)
  }
  items
}

///|
fn sort(items : Array[Int32]) -> Unit {
  for i in 1..<items.length() {
    let key = items[i]
    let mut j = i
    while j > 0 {
      if items[j - 1] <= key {
        break
      }
      items[j] = items[j - 1]
      j = j - 1
    }
    items[j] = key
  }
}

///|
fn checksum(items : Array[Int32]) -> Int32 {
  let mut total = 0
  for value in items {
    total = (total + value) % 1000003
  }
  total
}

///|
fn main {
  let items = fill(30000)
  sort(items)
  println(checksum(items))
}
