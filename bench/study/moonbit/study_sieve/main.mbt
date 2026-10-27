// sieve of Eratosthenes below 16M. Prints the prime count.

///|
let n : Int32 = 36_000_000

///|
fn main {
  let flags : Array[Bool] = []
  for _ in 0..<n {
    flags.push(true)
  }
  flags[0] = false
  flags[1] = false
  let mut i = 2
  while i * i < n {
    if flags[i] {
      for j = i * i; j < n; j = j + i {
        flags[j] = false
      }
    }
    i = i + 1
  }
  let mut count = 0
  for flag in flags {
    if flag {
      count = count + 1
    }
  }
  println(count)
}
