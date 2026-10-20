// nbody, 5M steps (benchmarks game). Prints trunc(energy * 1e9) before and after.
package main

import (
	"fmt"
	"math"
)

const (
	steps     = 5_000_000
	solarMass = 4 * math.Pi * math.Pi
	days      = 365.24
)

type body struct{ x, y, z, vx, vy, vz, mass float64 }

func mk(x, y, z, vx, vy, vz, m float64) body {
	return body{x, y, z, vx * days, vy * days, vz * days, m * solarMass}
}

func energy(bs []body) float64 {
	e := 0.0
	for i := range bs {
		b := &bs[i]
		e += 0.5 * b.mass * (b.vx*b.vx + b.vy*b.vy + b.vz*b.vz)
		for j := i + 1; j < len(bs); j++ {
			c := &bs[j]
			dx, dy, dz := b.x-c.x, b.y-c.y, b.z-c.z
			e -= b.mass * c.mass / math.Sqrt(dx*dx+dy*dy+dz*dz)
		}
	}
	return e
}

func advance(bs []body, dt float64) {
	for i := range bs {
		b := &bs[i]
		for j := i + 1; j < len(bs); j++ {
			c := &bs[j]
			dx, dy, dz := b.x-c.x, b.y-c.y, b.z-c.z
			d2 := dx*dx + dy*dy + dz*dz
			mag := dt / (d2 * math.Sqrt(d2))
			b.vx -= dx * c.mass * mag
			b.vy -= dy * c.mass * mag
			b.vz -= dz * c.mass * mag
			c.vx += dx * b.mass * mag
			c.vy += dy * b.mass * mag
			c.vz += dz * b.mass * mag
		}
	}
	for i := range bs {
		b := &bs[i]
		b.x += dt * b.vx
		b.y += dt * b.vy
		b.z += dt * b.vz
	}
}

func main() {
	bs := []body{
		mk(0, 0, 0, 0, 0, 0, 1),
		mk(4.84143144246472090e00, -1.16032004402742839e00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04),
		mk(8.34336671824457987e00, 4.12479856412430479e00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04),
		mk(1.28943695621391310e01, -1.51111514016986312e01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05),
		mk(1.53796971148509165e01, -2.59193146099879641e01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05),
	}
	px, py, pz := 0.0, 0.0, 0.0
	for _, b := range bs {
		px += b.vx * b.mass
		py += b.vy * b.mass
		pz += b.vz * b.mass
	}
	bs[0].vx, bs[0].vy, bs[0].vz = -px/solarMass, -py/solarMass, -pz/solarMass
	before := int64(energy(bs) * 1e9)
	for s := 0; s < steps; s++ {
		advance(bs, 0.01)
	}
	fmt.Println(before, int64(energy(bs)*1e9))
}
