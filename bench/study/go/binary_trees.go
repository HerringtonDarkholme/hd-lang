// binary-trees, depth 20 (benchmarks game). Prints one line per depth.
package main

import "fmt"

const maxDepth, minDepth = 20, 4

type node struct{ left, right *node }

func make_(d int) *node {
	if d == 0 {
		return &node{}
	}
	return &node{make_(d - 1), make_(d - 1)}
}

func (n *node) check() int {
	if n.left == nil {
		return 1
	}
	return 1 + n.left.check() + n.right.check()
}

func main() {
	fmt.Println("stretch", maxDepth+1, make_(maxDepth+1).check())
	long := make_(maxDepth)
	for d := minDepth; d <= maxDepth; d += 2 {
		iters := 1 << (maxDepth - d + minDepth)
		sum := 0
		for i := 0; i < iters; i++ {
			sum += make_(d).check()
		}
		fmt.Println(iters, d, sum)
	}
	fmt.Println("long", maxDepth, long.check())
}
