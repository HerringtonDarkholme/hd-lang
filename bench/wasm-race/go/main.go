package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"slices"
	"strconv"
	"strings"
	"time"
)

type Record struct {
	ID    int64    `json:"id"`
	Name  string   `json:"name"`
	Email string   `json:"email"`
	Score float64  `json:"score"`
	Tags  []string `json:"tags"`
}

func genRecords(n int) []Record {
	out := make([]Record, n)
	for i := 0; i < n; i++ {
		out[i] = Record{
			ID: int64(i), Name: fmt.Sprintf("user_%d", i),
			Email: fmt.Sprintf("user_%d@example.com", i),
			Score: float64(i%1000) + 0.5, Tags: []string{"a", "b", "c"},
		}
	}
	return out
}

func benchJSON(n int) (time.Duration, uint64) {
	recs := genRecords(n)
	start := time.Now()
	raw, err := json.Marshal(recs)
	if err != nil {
		panic(err)
	}
	var decoded []Record
	if err := json.Unmarshal(raw, &decoded); err != nil {
		panic(err)
	}
	elapsed := time.Since(start)
	var sum uint64
	for _, r := range decoded {
		sum += uint64(r.ID) + uint64(len(r.Name)) + uint64(len(r.Email)) + uint64(len(r.Tags))
	}
	sum += uint64(len(raw))
	return elapsed, sum
}

func benchSieve(limit int) (time.Duration, uint64) {
	start := time.Now()
	prime := make([]bool, limit+1)
	for i := range prime {
		prime[i] = true
	}
	if limit >= 0 {
		prime[0] = false
	}
	if limit >= 1 {
		prime[1] = false
	}
	for p := 2; p*p <= limit; p++ {
		if prime[p] {
			for m := p * p; m <= limit; m += p {
				prime[m] = false
			}
		}
	}
	var count uint64
	for i := 2; i <= limit; i++ {
		if prime[i] {
			count++
		}
	}
	return time.Since(start), count
}

func fillU32LCG(dst []uint32, seed uint32) {
	state := seed
	for i := range dst {
		state = state*1664525 + 1013904223
		dst[i] = state
	}
}

func benchSort(n int) (time.Duration, uint64) {
	a := make([]uint32, n)
	fillU32LCG(a, 0xC0FFEE)
	start := time.Now()
	slices.Sort(a)
	elapsed := time.Since(start)
	var sum uint64 = uint64(a[0]) + uint64(a[n-1])
	for i := 0; i < n; i += 1024 {
		sum ^= uint64(a[i])
	}
	return elapsed, sum
}

func benchHashmap(n int) (time.Duration, uint64) {
	keys := make([]uint32, n)
	fillU32LCG(keys, 0xBADC0DE)
	start := time.Now()
	m := make(map[uint32]uint32, n)
	for i := 0; i < n; i++ {
		m[keys[i]] = uint32(i)
	}
	var sum uint64
	for i := 0; i < n; i++ {
		sum += uint64(m[keys[i]])
	}
	var misses uint64
	for i := 0; i < n; i++ {
		if _, ok := m[keys[i]^0xFFFFFFFF]; !ok {
			misses++
		}
	}
	return time.Since(start), sum + misses
}

func benchFindBytes(buf, needle []byte) (time.Duration, uint64) {
	start := time.Now()
	var count uint64
	for pos := 0; pos+len(needle) <= len(buf); {
		i := bytes.Index(buf[pos:], needle)
		if i < 0 {
			break
		}
		count++
		pos += i + 1
	}
	return time.Since(start), count
}

type tnode struct {
	left, right *tnode
	val         int32
}

func buildTree(d int) *tnode {
	if d == 0 {
		return &tnode{val: 1}
	}
	return &tnode{left: buildTree(d - 1), right: buildTree(d - 1), val: int32(d)}
}

func walkTree(n *tnode) int64 {
	if n == nil {
		return 0
	}
	return int64(n.val) + walkTree(n.left) + walkTree(n.right)
}

func benchAlloc(depth, rounds int) (time.Duration, uint64) {
	start := time.Now()
	var sum uint64
	for r := 0; r < rounds; r++ {
		sum += uint64(walkTree(buildTree(depth)))
	}
	return time.Since(start), sum
}


type btNode struct{ left, right *btNode }

func bottomUpTree(d int) *btNode {
	if d == 0 {
		return &btNode{}
	}
	return &btNode{left: bottomUpTree(d - 1), right: bottomUpTree(d - 1)}
}

func itemCheck(t *btNode) int {
	if t.left == nil {
		return 1
	}
	return 1 + itemCheck(t.left) + itemCheck(t.right)
}

// GCBench-lite: stretch at maxDepth+1, long-lived maxDepth, then depth 4..maxDepth step 2.
func benchBTree(maxDepth int) (time.Duration, uint64) {
	start := time.Now()
	var sum uint64
	sum += uint64(itemCheck(bottomUpTree(maxDepth + 1)))
	longLived := bottomUpTree(maxDepth)
	for depth := 4; depth <= maxDepth; depth += 2 {
		iterations := 1 << (maxDepth - depth + 4)
		for i := 0; i < iterations; i++ {
			sum += uint64(itemCheck(bottomUpTree(depth)))
		}
	}
	sum += uint64(itemCheck(longLived))
	return time.Since(start), sum
}

func benchBTreeStretch(depth int) (time.Duration, uint64) {
	start := time.Now()
	sum := uint64(itemCheck(bottomUpTree(depth)))
	return time.Since(start), sum
}

func buildGraph(n int) []int {
	// CSR: deg=3, edges[u*3+k]
	edges := make([]int, n*3)
	for i := 0; i < n; i++ {
		edges[i*3+0] = (i*7 + 1) % n
		edges[i*3+1] = (i*7 + 3) % n
		edges[i*3+2] = (i*11 + 5) % n
	}
	return edges
}

func benchGraphDFS(n int) (time.Duration, uint64) {
	start := time.Now()
	edges := buildGraph(n)
	seen := make([]bool, n)
	stack := make([]int, 0, 64)
	stack = append(stack, 0)
	seen[0] = true
	var count, idSum uint64
	for len(stack) > 0 {
		u := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		count++
		idSum += uint64(u)
		base := u * 3
		for k := 0; k < 3; k++ {
			v := edges[base+k]
			if !seen[v] {
				seen[v] = true
				stack = append(stack, v)
			}
		}
	}
	return time.Since(start), idSum + count
}

func benchGraphBFS(n int) (time.Duration, uint64) {
	start := time.Now()
	edges := buildGraph(n)
	seen := make([]bool, n)
	dist := make([]int32, n)
	for i := range dist {
		dist[i] = -1
	}
	q := make([]int, 0, n)
	q = append(q, 0)
	seen[0] = true
	dist[0] = 0
	var count, idSum, distSum uint64
	for head := 0; head < len(q); head++ {
		u := q[head]
		count++
		idSum += uint64(u)
		distSum += uint64(dist[u])
		base := u * 3
		for k := 0; k < 3; k++ {
			v := edges[base+k]
			if !seen[v] {
				seen[v] = true
				dist[v] = dist[u] + 1
				q = append(q, v)
			}
		}
	}
	return time.Since(start), distSum + idSum + count
}

func fib(n int) int64 {
	if n < 2 {
		return int64(n)
	}
	return fib(n-1) + fib(n-2)
}

func benchFib(n int) (time.Duration, uint64) {
	start := time.Now()
	v := fib(n)
	return time.Since(start), uint64(v)
}

func benchMatmul(n int) (time.Duration, uint64) {
	a := make([]float64, n*n)
	b := make([]float64, n*n)
	c := make([]float64, n*n)
	for i := 0; i < n*n; i++ {
		a[i] = float64((i*3+1)%97) / 97.0
		b[i] = float64((i*5+2)%89) / 89.0
	}
	start := time.Now()
	for i := 0; i < n; i++ {
		for k := 0; k < n; k++ {
			aik := a[i*n+k]
			for j := 0; j < n; j++ {
				c[i*n+j] += aik * b[k*n+j]
			}
		}
	}
	elapsed := time.Since(start)
	var sum float64
	for i := 0; i < n; i++ {
		sum += c[i*n+i]
	}
	return elapsed, math.Float64bits(sum)
}

func benchUTF8(repeats int) (time.Duration, uint64) {
	chunk := "Hello世界🙂MoonBit综合性能ABC"
	var b strings.Builder
	b.Grow(len(chunk) * repeats)
	for i := 0; i < repeats; i++ {
		b.WriteString(chunk)
	}
	s := b.String()
	start := time.Now()
	var sum uint64
	for _, r := range s {
		sum += uint64(r)
	}
	var runes uint64
	for _, r := range s {
		sum += uint64(r)
		runes++
	}
	sum += uint64(len(s)) + runes
	return time.Since(start), sum
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: go-bench <workload> [args...]")
		os.Exit(2)
	}
	var d time.Duration
	var sum uint64
	switch os.Args[1] {
	case "json":
		n := 10000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchJSON(n)
	case "sieve":
		n := 10000000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchSieve(n)
	case "sort":
		n := 1000000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchSort(n)
	case "hashmap":
		n := 500000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchHashmap(n)
	case "find":
		size, stride := 8<<20, 1024
		if len(os.Args) > 2 {
			size, _ = strconv.Atoi(os.Args[2])
		}
		if len(os.Args) > 3 {
			stride, _ = strconv.Atoi(os.Args[3])
		}
		needle := []byte("needle!!")
		buf := make([]byte, size)
		for i := range buf {
			buf[i] = byte(i)
		}
		for i := 0; i+len(needle) <= size; i += stride {
			copy(buf[i:], needle)
		}
		d, sum = benchFindBytes(buf, needle)
	case "alloc":
		depth, rounds := 16, 20
		if len(os.Args) > 2 {
			depth, _ = strconv.Atoi(os.Args[2])
		}
		if len(os.Args) > 3 {
			rounds, _ = strconv.Atoi(os.Args[3])
		}
		d, sum = benchAlloc(depth, rounds)
	case "fib":
		n := 37
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchFib(n)
	case "matmul":
		n := 256
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchMatmul(n)
	case "utf8":
		n := 100000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchUTF8(n)
	case "btree":
		n := 15
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchBTree(n)
	case "btree-stretch":
		n := 18
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchBTreeStretch(n)
	case "graph-dfs":
		n := 200000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchGraphDFS(n)
	case "graph-bfs":
		n := 200000
		if len(os.Args) > 2 {
			n, _ = strconv.Atoi(os.Args[2])
		}
		d, sum = benchGraphBFS(n)
	default:
		fmt.Fprintln(os.Stderr, "unknown workload")
		os.Exit(2)
	}
	fmt.Printf("RESULT lang=go workload=%s ns=%d checksum=%d\n", os.Args[1], d.Nanoseconds(), sum)
}

