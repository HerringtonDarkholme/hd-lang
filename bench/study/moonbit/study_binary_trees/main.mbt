// binary-trees, depth 14 (benchmarks game). Prints one line per depth.

///|
let max_depth : Int32 = 14

///|
let min_depth : Int32 = 4

///|
struct Node {
  left : Node?
  right : Node?
}

///|
fn make(depth : Int32) -> Node {
  if depth == 0 {
    { left: None, right: None, }
  } else {
    { left: Some(make(depth - 1)), right: Some(make(depth - 1)), }
  }
}

///|
fn check(node : Node) -> Int64 {
  match node.left {
    None => 1
    Some(left) =>
      match node.right {
        None => 1 + check(left)
        Some(right) => 1 + check(left) + check(right)
      }
  }
}

///|
fn main {
  println("stretch \{max_depth + 1} \{check(make(max_depth + 1))}")
  let long = make(max_depth)
  for d = min_depth; d <= max_depth; d = d + 2 {
    let iters = 1 << (max_depth - d + min_depth)
    let mut sum : Int64 = 0
    for _ in 0..<iters {
      sum = sum + check(make(d))
    }
    println("\{iters} \{d} \{sum}")
  }
  println("long \{max_depth} \{check(long)}")
}
