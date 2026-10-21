// fannkuch-redux, n = 9 (benchmarks game). Prints checksum and max flips.
package main

import "fmt"

const n = 9

func main() {
	perm := make([]int, n)
	perm1 := make([]int, n)
	count := make([]int, n)
	for i := range perm1 {
		perm1[i] = i
	}
	maxFlips, checksum, permCount, r := 0, 0, 0, n
	for {
		for r != 1 {
			count[r-1] = r
			r--
		}
		copy(perm, perm1)
		flips := 0
		for k := perm[0]; k != 0; k = perm[0] {
			for i, j := 0, k; i < j; i, j = i+1, j-1 {
				perm[i], perm[j] = perm[j], perm[i]
			}
			flips++
		}
		if flips > maxFlips {
			maxFlips = flips
		}
		if permCount%2 == 0 {
			checksum += flips
		} else {
			checksum -= flips
		}
		done := true
		for r != n {
			p0 := perm1[0]
			copy(perm1[:r], perm1[1:r+1])
			perm1[r] = p0
			count[r]--
			if count[r] > 0 {
				done = false
				break
			}
			r++
		}
		if done {
			break
		}
		permCount++
	}
	fmt.Println(checksum, maxFlips)
}
