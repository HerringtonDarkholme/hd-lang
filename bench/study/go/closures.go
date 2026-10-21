// closures: 1M items, 50 rounds of map/filter/fold through function values.
package main

import "fmt"

const n = 170_000

func fold(items []int32, m func(int32) int32, keep func(int32) bool, step func(int32, int32) int32) int32 {
	acc := int32(0)
	for _, x := range items {
		y := m(x)
		if keep(y) {
			acc = step(acc, y)
		}
	}
	return acc
}

func main() {
	items := make([]int32, n)
	for i := range items {
		items[i] = int32((i*31 + 7) % 100_003)
	}
	total := int32(0)
	for round := int32(0); round < 50; round++ {
		r := round
		acc := fold(items, func(x int32) int32 { return x*3 + r }, func(y int32) bool { return y%2 == 0 },
			func(a, y int32) int32 { return (a + y) % 1_000_003 })
		total = (total + acc) % 1_000_003
	}
	fmt.Println(total)
}
