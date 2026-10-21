"""3.3 million two-field records, ten rounds, each round a new list."""
N = 3_300_000


class Point:
    __slots__ = ("x", "y")

    def __init__(self, x, y):
        self.x = x
        self.y = y


def main():
    points = [Point(i, i * 7 % 1000) for i in range(N)]
    for round in range(10):
        points = [Point((p.x + p.y) % 1_000_003, (p.y * 3 + round) % 1000)
                  for p in points]
    total = 0
    for p in points:
        total += p.x + p.y
    print(total)


if __name__ == "__main__":
    main()
