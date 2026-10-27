"""Fannkuch redux, n=10: permutation checksum and maximum flips."""
N = 10


def main():
    perm1 = list(range(N))
    count = [0] * N
    maximum = checksum = permutations = 0
    r = N
    while True:
        while r != 1:
            count[r - 1] = r
            r -= 1
        perm = perm1.copy()
        flips = 0
        k = perm[0]
        while k != 0:
            i, j = 0, k
            while i < j:
                perm[i], perm[j] = perm[j], perm[i]
                i += 1
                j -= 1
            flips += 1
            k = perm[0]
        maximum = max(maximum, flips)
        checksum += flips if permutations % 2 == 0 else -flips
        done = True
        while r != N:
            first = perm1[0]
            for i in range(r):
                perm1[i] = perm1[i + 1]
            perm1[r] = first
            count[r] -= 1
            if count[r] > 0:
                done = False
                break
            r += 1
        if done:
            break
        permutations += 1
    print(checksum, maximum)


if __name__ == "__main__":
    main()
