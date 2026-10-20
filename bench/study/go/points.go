// points: 1M two-field records, 10 rounds, each round a new slice.
package main

import "fmt"

const n = 1_000_000

type point struct{ x, y int64 }

func main() {
	points := make([]point, 0, n)
	for i := int64(0); i < n; i++ {
		points = append(points, point{i, i * 7 % 1000})
	}
	for round := int64(0); round < 10; round++ {
		next := make([]point, len(points))
		for i, p := range points {
			next[i] = point{(p.x + p.y) % 1_000_003, (p.y*3 + round) % 1000}
		}
		points = next
	}
	var sum int64
	for _, p := range points {
		sum += p.x + p.y
	}
	fmt.Println(sum)
}
