"""Count points inside a 1600 by 1600 Mandelbrot grid after 50 iterations."""
SIZE = 1600
ITER = 50


def main():
    inside = 0
    for y in range(SIZE):
        ci = 2 * y / SIZE - 1
        for x in range(SIZE):
            cr = 2 * x / SIZE - 1.5
            zr = zi = 0.0
            i = 0
            while i < ITER and zr * zr + zi * zi <= 4:
                t = zr * zr - zi * zi + cr
                zi = 2 * zr * zi + ci
                zr = t
                i += 1
            if i == ITER:
                inside += 1
    print(inside)


if __name__ == "__main__":
    main()
