// fannkuch-redux, n = 9. Prints checksum and max flips.

///|
let n : Int32 = 9

///|
fn main {
  let perm : Array[Int32] = []
  let perm1 : Array[Int32] = []
  let count : Array[Int32] = []
  for i in 0..<n {
    perm.push(0)
    perm1.push(i)
    count.push(0)
  }
  let mut max_flips : Int64 = 0
  let mut checksum : Int64 = 0
  let mut perm_count : Int64 = 0
  let mut r = n
  while true {
    while r != 1 {
      count[r - 1] = r
      r = r - 1
    }
    for i in 0..<n {
      perm[i] = perm1[i]
    }
    let mut flips : Int64 = 0
    let mut k = perm[0]
    while k != 0 {
      let mut i = 0
      let mut j = k
      while i < j {
        let t = perm[i]
        perm[i] = perm[j]
        perm[j] = t
        i = i + 1
        j = j - 1
      }
      flips = flips + 1
      k = perm[0]
    }
    if flips > max_flips {
      max_flips = flips
    }
    checksum = if perm_count % 2 == 0 {
      checksum + flips
    } else {
      checksum - flips
    }
    let mut done = true
    while r != n {
      let p0 = perm1[0]
      for i in 0..<r {
        perm1[i] = perm1[i + 1]
      }
      perm1[r] = p0
      count[r] = count[r] - 1
      if count[r] > 0 {
        done = false
        break
      }
      r = r + 1
    }
    if done {
      break
    }
    perm_count = perm_count + 1
  }
  println("\{checksum} \{max_flips}")
}
