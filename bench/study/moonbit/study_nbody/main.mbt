// nbody, 1.15M steps (benchmarks game). Prints trunc(energy * 1e9) before and after.

///|
let steps : Int32 = 1_150_000

///|
let pi : Double = 3.141592653589793

///|
let days : Double = 365.24

///|
struct Body {
  mut x : Double
  mut y : Double
  mut z : Double
  mut vx : Double
  mut vy : Double
  mut vz : Double
  mass : Double
}

///|
fn body(
  x : Double,
  y : Double,
  z : Double,
  vx : Double,
  vy : Double,
  vz : Double,
  mass : Double,
) -> Body {
  {
    x,
    y,
    z,
    vx: vx * days,
    vy: vy * days,
    vz: vz * days,
    mass: mass * 4.0 * pi * pi,
  }
}

///|
fn energy(bodies : Array[Body]) -> Double {
  let mut e = 0.0
  let n = bodies.length()
  for i in 0..<n {
    let b = bodies[i]
    e = e + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
    for j = i + 1; j < n; j = j + 1 {
      let c = bodies[j]
      let dx = b.x - c.x
      let dy = b.y - c.y
      let dz = b.z - c.z
      e = e - b.mass * c.mass / (dx * dx + dy * dy + dz * dz).sqrt()
    }
  }
  e
}

///|
fn advance(bodies : Array[Body], dt : Double) -> Unit {
  let n = bodies.length()
  for i in 0..<n {
    for j = i + 1; j < n; j = j + 1 {
      let dx = bodies[i].x - bodies[j].x
      let dy = bodies[i].y - bodies[j].y
      let dz = bodies[i].z - bodies[j].z
      let d2 = dx * dx + dy * dy + dz * dz
      let mag = dt / (d2 * d2.sqrt())
      let mi = bodies[i].mass
      let mj = bodies[j].mass
      bodies[i].vx = bodies[i].vx - dx * mj * mag
      bodies[i].vy = bodies[i].vy - dy * mj * mag
      bodies[i].vz = bodies[i].vz - dz * mj * mag
      bodies[j].vx = bodies[j].vx + dx * mi * mag
      bodies[j].vy = bodies[j].vy + dy * mi * mag
      bodies[j].vz = bodies[j].vz + dz * mi * mag
    }
  }
  for i in 0..<n {
    bodies[i].x = bodies[i].x + dt * bodies[i].vx
    bodies[i].y = bodies[i].y + dt * bodies[i].vy
    bodies[i].z = bodies[i].z + dt * bodies[i].vz
  }
}

///|
fn main {
  let bodies = [
    body(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
    body(
      4.84143144246472090e00, -1.16032004402742839e00, -1.03622044471123109e-01,
      1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05,
      9.54791938424326609e-04,
    ),
    body(
      8.34336671824457987e00, 4.12479856412430479e00, -4.03523417114321381e-01, -2.76742510726862411e-03,
      4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04,
    ),
    body(
      1.28943695621391310e01, -1.51111514016986312e01, -2.23307578892655734e-01,
      2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05,
      4.36624404335156298e-05,
    ),
    body(
      1.53796971148509165e01, -2.59193146099879641e01, 1.79258772950371181e-01, 2.68067772490389322e-03,
      1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05,
    ),
  ]
  let solar = 4.0 * pi * pi
  let mut px = 0.0
  let mut py = 0.0
  let mut pz = 0.0
  for b in bodies {
    px = px + b.vx * b.mass
    py = py + b.vy * b.mass
    pz = pz + b.vz * b.mass
  }
  bodies[0].vx = 0.0 - px / solar
  bodies[0].vy = 0.0 - py / solar
  bodies[0].vz = 0.0 - pz / solar
  let before = (energy(bodies) * 1000000000.0).to_int64()
  for _ in 0..<steps {
    advance(bodies, 0.01)
  }
  println("\{before} \{(energy(bodies) * 1000000000.0).to_int64()}")
}
