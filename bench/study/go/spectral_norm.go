// spectral-norm, n = 2500 (benchmarks game). Prints trunc(norm * 1e9).
package main

import (
	"fmt"
	"math"
)

const n = 2500

func a(i, j int) float64 { return 1 / float64((i+j)*(i+j+1)/2+i+1) }

func mulAv(v, out []float64) {
	for i := range out {
		s := 0.0
		for j := range v {
			s += a(i, j) * v[j]
		}
		out[i] = s
	}
}

func mulAtv(v, out []float64) {
	for i := range out {
		s := 0.0
		for j := range v {
			s += a(j, i) * v[j]
		}
		out[i] = s
	}
}

func main() {
	u := make([]float64, n)
	v := make([]float64, n)
	tmp := make([]float64, n)
	for i := range u {
		u[i] = 1
	}
	for i := 0; i < 10; i++ {
		mulAv(u, tmp)
		mulAtv(tmp, v)
		mulAv(v, tmp)
		mulAtv(tmp, u)
	}
	vBv, vv := 0.0, 0.0
	for i := range u {
		vBv += u[i] * v[i]
		vv += v[i] * v[i]
	}
	fmt.Println(int64(math.Sqrt(vBv/vv) * 1e9))
}
