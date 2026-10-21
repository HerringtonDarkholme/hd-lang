"""Sieve of Eratosthenes below 30 million, using one byte per integer."""
N = 16_000_000


def main():
    flags = bytearray([1]) * N
    flags[0] = flags[1] = 0
    i = 2
    while i * i < N:
        if flags[i]:
            for j in range(i * i, N, i):
                flags[j] = 0
        i += 1
    count = 0
    for flag in flags:
        count += flag
    print(count)


if __name__ == "__main__":
    main()
