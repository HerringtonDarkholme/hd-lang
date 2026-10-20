"""One million items, fifty rounds of the same fused map/filter/fold as JS."""
N = 1_000_000


def main():
    items = [(i * 31 + 7) % 100_003 for i in range(N)]
    total = 0
    for round in range(50):
        acc = 0
        for x in items:
            y = x * 3 + round
            if y % 2 == 0:
                acc = (acc + y) % 1_000_003
        total = (total + acc) % 1_000_003
    print(total)


if __name__ == "__main__":
    main()
