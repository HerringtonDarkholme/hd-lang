///|
struct Record {
  id : Int64
  name : String
  email : String
  score : Double
  tags : Array[String]
} derive(ToJson, @json.FromJson)

///|
fn gen_records(n : Int) -> Array[Record] {
  let out : Array[Record] = []
  for i = 0; i < n; i = i + 1 {
    out.push({
      id: i.to_int64(),
      name: "user_" + i.to_string(),
      email: "user_" + i.to_string() + "@example.com",
      score: (i % 1000).to_double() + 0.5,
      tags: ["a", "b", "c"],
    })
  }
  out
}

///|
fn bench_json(n : Int) -> (Int64, UInt64) raise {
  let recs = gen_records(n)
  let t0 = @bench.monotonic_clock_start()
  let raw = @json.to_json(recs).stringify()
  let parsed = @json.parse(raw)
  let decoded : Array[Record] = @json.from_json(parsed)
  let us = @bench.monotonic_clock_end(t0)
  let mut sum : UInt64 = 0
  for r in decoded {
    sum = sum + r.id.reinterpret_as_uint64() + r.name.length().to_uint64() +
      r.email.length().to_uint64() + r.tags.length().to_uint64()
  }
  sum = sum + raw.length().to_uint64()
  ((us * 1000.0).to_int64(), sum)
}

///|
fn bench_csv(n : Int) -> (Int64, UInt64) raise {
  let t0 = @bench.monotonic_clock_start()
  let sb = StringBuilder::StringBuilder(size_hint=n * 24)
  for i = 0; i < n; i = i + 1 {
    sb.write_string(i.to_string())
    sb.write_char(',')
    sb.write_string("user_")
    sb.write_string(i.to_string())
    sb.write_char(',')
    sb.write_string((i % 1000).to_string())
    sb.write_char('\n')
  }
  let s = sb.to_string()
  let mut sum : UInt64 = 0
  for line in s.split("\n") {
    if line.length() == 0 { continue }
    match line.rev_split_once(",") {
      Some((_, score_s)) => { sum = sum + @string.parse_int(score_s).to_uint64() }
      None => ()
    }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum + s.length().to_uint64())
}

///|
fn bench_sieve(limit : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let prime = FixedArray::make(limit + 1, true)
  if limit >= 0 { prime[0] = false }
  if limit >= 1 { prime[1] = false }
  let mut p = 2
  while p * p <= limit {
    if prime[p] {
      let mut m = p * p
      while m <= limit {
        prime[m] = false
        m = m + p
      }
    }
    p = p + 1
  }
  let mut count : UInt64 = 0
  for i = 2; i <= limit; i = i + 1 {
    if prime[i] { count = count + 1 }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), count)
}


///|
fn fill_u32_lcg(dst : FixedArray[UInt], seed : UInt) -> Unit {
  let mut state = seed
  for i = 0; i < dst.length(); i = i + 1 {
    state = state * 1664525U + 1013904223U
    dst[i] = state
  }
}

///|
fn bench_sort(n : Int) -> (Int64, UInt64) {
  let a : FixedArray[UInt] = FixedArray::make(n, 0U)
  fill_u32_lcg(a, 0xC0FFEEU)
  let t0 = @bench.monotonic_clock_start()
  a.sort()
  let us = @bench.monotonic_clock_end(t0)
  let mut sum : UInt64 = a[0].to_uint64() + a[n - 1].to_uint64()
  let mut i = 0
  while i < n {
    sum = sum ^ a[i].to_uint64()
    i = i + 1024
  }
  ((us * 1000.0).to_int64(), sum)
}

///|
fn bench_hashmap(n : Int) -> (Int64, UInt64) {
  let keys : FixedArray[UInt] = FixedArray::make(n, 0U)
  fill_u32_lcg(keys, 0xBADC0DEU)
  let t0 = @bench.monotonic_clock_start()
  let m : Map[UInt, UInt] = Map([], capacity=n)
  for i = 0; i < n; i = i + 1 { m.set(keys[i], i.reinterpret_as_uint()) }
  let mut sum : UInt64 = 0
  for i = 0; i < n; i = i + 1 { sum = sum + m.get(keys[i]).unwrap().to_uint64() }
  let mut misses : UInt64 = 0
  for i = 0; i < n; i = i + 1 {
    if m.get(keys[i] ^ 0xFFFFFFFFU) is None { misses = misses + 1 }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum + misses)
}

///|
fn bench_find(size : Int, stride : Int) -> (Int64, UInt64) {
  let needle = b"needle!!"
  let arr = FixedArray::make(size, b'\x00')
  for i = 0; i < size; i = i + 1 { arr[i] = (i % 256).to_byte() }
  let mut i = 0
  while i + needle.length() <= size {
    for j = 0; j < needle.length(); j = j + 1 { arr[i + j] = needle[j] }
    i = i + stride
  }
  let hay = Bytes::from_fixedarray(arr)
  let t0 = @bench.monotonic_clock_start()
  let mut count : UInt64 = 0
  let mut pos = 0
  while pos + needle.length() <= hay.length() {
    match hay.clamped_view(start=pos).find(needle) {
      Some(idx) => { count = count + 1; pos = pos + idx + 1 }
      None => break
    }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), count)
}


///|
enum Tree {
  Leaf(Int)
  Node(Tree, Tree, Int)
}

///|
fn build_tree(d : Int) -> Tree {
  if d == 0 {
    Leaf(1)
  } else {
    Node(build_tree(d - 1), build_tree(d - 1), d)
  }
}

///|
fn walk_tree(t : Tree) -> Int64 {
  match t {
    Leaf(v) => v.to_int64()
    Node(l, r, v) => v.to_int64() + walk_tree(l) + walk_tree(r)
  }
}

///|
fn bench_alloc(depth : Int, rounds : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let mut sum : UInt64 = 0
  for _ in 0..<rounds {
    sum = sum + walk_tree(build_tree(depth)).reinterpret_as_uint64()
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum)
}


///|
enum BT {
  Nil
  Node(BT, BT)
}

///|
fn bottom_up(d : Int) -> BT {
  if d == 0 {
    Node(Nil, Nil)
  } else {
    Node(bottom_up(d - 1), bottom_up(d - 1))
  }
}

///|
fn item_check(t : BT) -> Int {
  match t {
    Nil => 0
    Node(l, r) => 1 + item_check(l) + item_check(r)
  }
}

///|
fn bench_btree(max_depth : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let mut sum : UInt64 = 0
  sum = sum + item_check(bottom_up(max_depth + 1)).to_int64().reinterpret_as_uint64()
  let long_lived = bottom_up(max_depth)
  let mut depth = 4
  while depth <= max_depth {
    let iterations = 1 << (max_depth - depth + 4)
    for _ in 0..<iterations {
      sum = sum + item_check(bottom_up(depth)).to_int64().reinterpret_as_uint64()
    }
    depth = depth + 2
  }
  sum = sum + item_check(long_lived).to_int64().reinterpret_as_uint64()
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum)
}

///|
fn bench_btree_stretch(depth : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let sum = item_check(bottom_up(depth)).to_int64().reinterpret_as_uint64()
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum)
}

///|
fn build_graph(n : Int) -> FixedArray[Int] {
  let edges = FixedArray::make(n * 3, 0)
  for i = 0; i < n; i = i + 1 {
    edges[i * 3 + 0] = (i * 7 + 1) % n
    edges[i * 3 + 1] = (i * 7 + 3) % n
    edges[i * 3 + 2] = (i * 11 + 5) % n
  }
  edges
}

///|
fn bench_graph_dfs(n : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let edges = build_graph(n)
  let seen = FixedArray::make(n, false)
  let stack : Array[Int] = []
  stack.push(0)
  seen[0] = true
  let mut count : UInt64 = 0
  let mut id_sum : UInt64 = 0
  while true {
    match stack.pop() {
      None => break
      Some(u) => {
        count = count + 1
        id_sum = id_sum + u.to_int64().reinterpret_as_uint64()
        let base = u * 3
        for k = 0; k < 3; k = k + 1 {
          let v = edges[base + k]
          if not(seen[v]) {
            seen[v] = true
            stack.push(v)
          }
        }
      }
    }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), id_sum + count)
}

///|
fn bench_graph_bfs(n : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let edges = build_graph(n)
  let seen = FixedArray::make(n, false)
  let dist = FixedArray::make(n, -1)
  let q : Array[Int] = []
  q.push(0)
  seen[0] = true
  dist[0] = 0
  let mut count : UInt64 = 0
  let mut id_sum : UInt64 = 0
  let mut dist_sum : UInt64 = 0
  let mut head = 0
  while head < q.length() {
    let u = q[head]
    head = head + 1
    count = count + 1
    id_sum = id_sum + u.to_int64().reinterpret_as_uint64()
    dist_sum = dist_sum + dist[u].to_int64().reinterpret_as_uint64()
    let base = u * 3
    for k = 0; k < 3; k = k + 1 {
      let v = edges[base + k]
      if not(seen[v]) {
        seen[v] = true
        dist[v] = dist[u] + 1
        q.push(v)
      }
    }
  }
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), dist_sum + id_sum + count)
}

///|
fn fib(n : Int) -> Int64 {
  if n < 2 {
    n.to_int64()
  } else {
    fib(n - 1) + fib(n - 2)
  }
}

///|
fn bench_fib(n : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let v = fib(n)
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), v.reinterpret_as_uint64())
}

///|
fn bench_matmul(n : Int) -> (Int64, UInt64) {
  let a = FixedArray::make(n * n, 0.0)
  let b = FixedArray::make(n * n, 0.0)
  let c = FixedArray::make(n * n, 0.0)
  for i = 0; i < n * n; i = i + 1 {
    a[i] = ((i * 3 + 1) % 97).to_double() / 97.0
    b[i] = ((i * 5 + 2) % 89).to_double() / 89.0
  }
  let t0 = @bench.monotonic_clock_start()
  for i = 0; i < n; i = i + 1 {
    for k = 0; k < n; k = k + 1 {
      let aik = a[i * n + k]
      for j = 0; j < n; j = j + 1 {
        c[i * n + j] = c[i * n + j] + aik * b[k * n + j]
      }
    }
  }
  let us = @bench.monotonic_clock_end(t0)
  let mut sum = 0.0
  for i = 0; i < n; i = i + 1 { sum = sum + c[i * n + i] }
  ((us * 1000.0).to_int64(), sum.reinterpret_as_uint64())
}

///|
fn append_u64_le(buf : Array[Byte], v : UInt64) -> Unit {
  let mut x = v
  for _ in 0..<8 {
    buf.push((x & 0xFFUL).to_byte())
    x = x >> 8
  }
}

///|
fn append_u16_le(buf : Array[Byte], v : Int) -> Unit {
  buf.push((v & 0xFF).to_byte())
  buf.push(((v >> 8) & 0xFF).to_byte())
}

///|
fn read_u64_le(buf : Array[Byte], pos : Int) -> UInt64 {
  let mut v : UInt64 = 0
  for i = 0; i < 8; i = i + 1 {
    v = v | (buf[pos + i].to_int64().reinterpret_as_uint64() << (i * 8))
  }
  v
}

///|
fn read_u16_le(buf : Array[Byte], pos : Int) -> Int {
  buf[pos].to_int() | (buf[pos + 1].to_int() << 8)
}

///|
fn bench_binenc(n : Int) -> (Int64, UInt64) {
  let t0 = @bench.monotonic_clock_start()
  let buf2 : Array[Byte] = []
  for i = 0; i < n; i = i + 1 {
    append_u64_le(buf2, i.to_uint64())
    let name = "user_" + i.to_string()
    append_u16_le(buf2, name.length())
    for j = 0; j < name.length(); j = j + 1 {
      buf2.push(name[j].to_int().to_byte())
    }
    let bits = ((i % 1000).to_double() + 0.5).reinterpret_as_uint64()
    append_u64_le(buf2, bits)
    buf2.push(b'\x03')
    for t in ["a", "b", "c"] {
      buf2.push(t.length().to_byte())
      for j = 0; j < t.length(); j = j + 1 {
        buf2.push(t[j].to_int().to_byte())
      }
    }
  }
  let mut sum : UInt64 = 0
  let mut pos = 0
  while pos < buf2.length() {
    let id = read_u64_le(buf2, pos)
    pos = pos + 8
    let nl = read_u16_le(buf2, pos)
    pos = pos + 2 + nl
    pos = pos + 8
    let tc = buf2[pos].to_int()
    pos = pos + 1
    for _ in 0..<tc {
      let l = buf2[pos].to_int()
      pos = pos + 1 + l
    }
    sum = sum + id
  }
  let us = @bench.monotonic_clock_end(t0)
  sum = sum + buf2.length().to_uint64()
  if buf2.length() >= 8 {
    sum = sum ^ read_u64_le(buf2, 0)
  }
  ((us * 1000.0).to_int64(), sum)
}


///|
fn bench_utf8(repeats : Int) -> (Int64, UInt64) {
  let chunk = "Hello世界🙂MoonBit综合性能ABC"
  let sb = StringBuilder::StringBuilder()
  for _ in 0..<repeats { sb.write_string(chunk) }
  let s = sb.to_string()
  let t0 = @bench.monotonic_clock_start()
  let mut sum : UInt64 = 0
  for ch in s { sum = sum + ch.to_uint().to_uint64() }
  let mut rune_count : UInt64 = 0
  let mut nbytes : UInt64 = 0
  for ch in s {
    sum = sum + ch.to_uint().to_uint64()
    rune_count = rune_count + 1
    let cp = ch.to_int()
    if cp < 0x80 { nbytes = nbytes + 1 }
    else if cp < 0x800 { nbytes = nbytes + 2 }
    else if cp < 0x10000 { nbytes = nbytes + 3 }
    else { nbytes = nbytes + 4 }
  }
  sum = sum + nbytes + rune_count
  let us = @bench.monotonic_clock_end(t0)
  ((us * 1000.0).to_int64(), sum)
}

///|
fn parse_arg_int(args : Array[String], idx : Int, default : Int) -> Int raise {
  if args.length() > idx { @string.parse_int(args[idx]) } else { default }
}

///|
///|
///|
///|
fn main {
  let args = @env.args()
  if args.length() < 2 {
    println("usage: moon-bench <workload> [args...]")
    panic()
  }
  let workload = args[1]
  let mut ns : Int64 = 0
  let mut sum : UInt64 = 0
  try {
    match workload {
      "json" => { let (a, b) = bench_json(parse_arg_int(args, 2, 10000)); ns = a; sum = b }
      "csv" => { let (a, b) = bench_csv(parse_arg_int(args, 2, 500000)); ns = a; sum = b }
      "sieve" => { let (a, b) = bench_sieve(parse_arg_int(args, 2, 10000000)); ns = a; sum = b }
      "sort" => { let (a, b) = bench_sort(parse_arg_int(args, 2, 1000000)); ns = a; sum = b }
      "hashmap" => { let (a, b) = bench_hashmap(parse_arg_int(args, 2, 500000)); ns = a; sum = b }
      "find" => {
        let (a, b) = bench_find(parse_arg_int(args, 2, 8388608), parse_arg_int(args, 3, 1024))
        ns = a; sum = b
      }
      "alloc" => {
        let (a, b) = bench_alloc(parse_arg_int(args, 2, 16), parse_arg_int(args, 3, 20))
        ns = a; sum = b
      }
      "fib" => { let (a, b) = bench_fib(parse_arg_int(args, 2, 37)); ns = a; sum = b }
      "matmul" => { let (a, b) = bench_matmul(parse_arg_int(args, 2, 256)); ns = a; sum = b }
      "binenc" => { let (a, b) = bench_binenc(parse_arg_int(args, 2, 100000)); ns = a; sum = b }
      "utf8" => { let (a, b) = bench_utf8(parse_arg_int(args, 2, 100000)); ns = a; sum = b }
      "btree" => { let (a, b) = bench_btree(parse_arg_int(args, 2, 15)); ns = a; sum = b }
      "btree-stretch" => { let (a, b) = bench_btree_stretch(parse_arg_int(args, 2, 18)); ns = a; sum = b }
      "graph-dfs" => { let (a, b) = bench_graph_dfs(parse_arg_int(args, 2, 200000)); ns = a; sum = b }
      "graph-bfs" => { let (a, b) = bench_graph_bfs(parse_arg_int(args, 2, 200000)); ns = a; sum = b }
      _ => { println("unknown workload"); panic() }
    }
  } catch {
    e => { println("error: " + e.to_string()); panic() }
  }
  println(
    "RESULT lang=moonbit workload=" + workload + " ns=" + ns.to_string() + " checksum=" +
    sum.to_string(),
  )
}

