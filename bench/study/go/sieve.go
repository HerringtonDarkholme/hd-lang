// sieve of Eratosthenes below 30M. Prints the prime count.
package main

import "fmt"

const n = 30_000_000

func main() {
	flags := make([]bool, n)
	for i := range flags {
		flags[i] = true
	}
	flags[0], flags[1] = false, false
	for i := 2; i*i < n; i++ {
		if flags[i] {
			for j := i * i; j < n; j += i {
				flags[j] = false
			}
		}
	}
	count := 0
	for _, f := range flags {
		if f {
			count++
		}
	}
	fmt.Println(count)
}
