// #634 in Go: a filter/map chain behind an interface returned by a
// function, the same chain as concrete types, and a plain loop. Prints
// each sum on stdout and each variant's best of three on stderr.
package main

import (
	"fmt"
	"os"
	"time"
)

const n, calls, short = 1_000_000, 10_000, 100

type Iterator interface{ Next() (int64, bool) }

type counter struct{ next, end int64 }

func (c *counter) Next() (int64, bool) {
	if c.next >= c.end {
		return 0, false
	}
	v := c.next
	c.next++
	return v, true
}

type filter struct {
	src  Iterator
	keep func(int64) bool
}

func (f *filter) Next() (int64, bool) {
	for {
		v, ok := f.src.Next()
		if !ok || f.keep(v) {
			return v, ok
		}
	}
}

type mapper struct {
	src Iterator
	f   func(int64) int64
}

func (m *mapper) Next() (int64, bool) {
	v, ok := m.src.Next()
	if !ok {
		return 0, false
	}
	return m.f(v), true
}

func sum(it Iterator) int64 {
	var t int64
	for v, ok := it.Next(); ok; v, ok = it.Next() {
		t += v
	}
	return t
}

// (a) The chain behind an interface, as a function returning it.
func erased(n int64) Iterator {
	return &mapper{&filter{&counter{0, n}, func(x int64) bool { return x%3 != 0 }}, func(x int64) int64 { return x * 2 }}
}

// (b) Concrete generic adapters, as Rust's or hd's monomorphized chain.
type cFilter[S interface{ Next() (int64, bool) }] struct {
	src  S
	keep func(int64) bool
}

func (f *cFilter[S]) Next() (int64, bool) {
	for {
		v, ok := f.src.Next()
		if !ok || f.keep(v) {
			return v, ok
		}
	}
}

func inPlace(n int64) int64 {
	f := cFilter[*counter]{&counter{0, n}, func(x int64) bool { return x%3 != 0 }}
	var t int64
	for v, ok := f.Next(); ok; v, ok = f.Next() {
		t += v * 2
	}
	return t
}

func plain(n int64) int64 {
	var t int64
	for x := int64(0); x < n; x++ {
		if x%3 != 0 {
			t += x * 2
		}
	}
	return t
}

func best(name string, run func() int64) {
	var fastest time.Duration
	var r int64
	for i := 0; i < 3; i++ {
		s := time.Now()
		r = run()
		d := time.Since(s)
		if i == 0 || d < fastest {
			fastest = d
		}
	}
	fmt.Println(name, r)
	fmt.Fprintf(os.Stderr, "%s %dms\n", name, fastest.Milliseconds())
}

func main() {
	best("dyn", func() int64 { return sum(erased(n)) })
	best("in-place", func() int64 { return inPlace(n) })
	best("loop", func() int64 { return plain(n) })
	best("range", func() int64 { return sum(erased(n)) })
	best("dyn-short", func() int64 {
		var t int64
		for i := 0; i < calls; i++ {
			t += sum(erased(short))
		}
		return t
	})
	best("in-place-short", func() int64 {
		var t int64
		for i := 0; i < calls; i++ {
			t += inPlace(short)
		}
		return t
	})
}
