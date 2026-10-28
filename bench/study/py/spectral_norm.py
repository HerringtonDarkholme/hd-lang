"""Spectral norm, n=2800; the same ten power iterations as the hd case."""
from math import sqrt

N = 2800


def multiply(v, out, transpose=False):
    for i in range(N):
        total = 0.0
        for j in range(N):
            ij = i + j
            row = j if transpose else i
            total += (1 / (ij * (ij + 1) / 2 + row + 1)) * v[j]
        out[i] = total


def multiply_ata(v, out, tmp):
    multiply(v, tmp)
    multiply(tmp, out, True)


def main():
    u, v, tmp = [1.0] * N, [0.0] * N, [0.0] * N
    for _ in range(10):
        multiply_ata(u, v, tmp)
        multiply_ata(v, u, tmp)
    vbv, vv = 0.0, 0.0
    for i in range(N):
        vbv += u[i] * v[i]
        vv += v[i] * v[i]
    print(int(sqrt(vbv / vv) * 1e9))


if __name__ == "__main__":
    main()
