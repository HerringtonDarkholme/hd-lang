// map-count: build a text of 2M words, split it, count each word.
package main

import (
	"fmt"
	"strconv"
	"strings"
)

const n = 650_000

func main() {
	var b strings.Builder
	for i := 0; i < n; i++ {
		if i > 0 {
			b.WriteByte(' ')
		}
		b.WriteByte('w')
		b.WriteString(strconv.Itoa(i * 7919 % 50_021))
	}
	text := b.String()
	counts := map[string]int64{}
	for _, word := range strings.Split(text, " ") {
		counts[word]++
	}
	var squares int64
	for _, c := range counts {
		squares += c * c
	}
	fmt.Println(len(counts), squares)
}
