///|
trait Step {
  fn step(Self, Int32) -> Int32
}

///|
struct Twice {
  k : Int32
}

///|
impl Step for Twice with fn step(self, x) {
  x * 2 + self.k
}

///|
struct Add {
  k : Int32
}

///|
impl Step for Add with fn step(self, x) {
  x + self.k
}

///|
fn[S : Step] apply(s : S, x : Int32) -> Int32 {
  s.step(x)
}

///|
fn drive(n : Int32) -> Int32 {
  let double = Twice::{ k: 3, }
  let add = Add::{ k: 5, }
  let mut x = 1
  for _ in 0..<n {
    x = apply(double, x) % 1000003
    x = apply(add, x) % 1000003
  }
  x
}

///|
fn main {
  println(drive(35000000))
}
