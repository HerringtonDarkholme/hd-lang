// mandelbrot: a 1400 x 1400 grid, 50 iterations. Prints the points inside.
package main

import "fmt"

const size, iter = 1400, 50

func main() {
	inside := 0
	for y := 0; y < size; y++ {
		ci := 2*float64(y)/size - 1
		for x := 0; x < size; x++ {
			cr := 2*float64(x)/size - 1.5
			zr, zi := 0.0, 0.0
			i := 0
			for i < iter && zr*zr+zi*zi <= 4 {
				zr, zi = zr*zr-zi*zi+cr, 2*zr*zi+ci
				i++
			}
			if i == iter {
				inside++
			}
		}
	}
	fmt.Println(inside)
}
