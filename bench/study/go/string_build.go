// string-build: append 3M pieces to one string. Prints its length.
package main

import (
	"fmt"
	"strings"
)

const n = 1_500_000

func main() {
	var b strings.Builder
	for i := 0; i < n; i++ {
		fmt.Fprintf(&b, "item-%d;", i)
	}
	fmt.Println(b.Len())
}
