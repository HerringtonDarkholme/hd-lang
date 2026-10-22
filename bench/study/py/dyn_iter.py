"""#634's six variants: returned generator chain, local chain, and loop.

Python has no static-vs-erased iterator distinction; the two chain cases
preserve where the chain is built. Each variant reports its best of three.
"""
from sys import stderr
from time import perf_counter

N = 10_000_000
CALLS = 100_000
SHORT = 100


class Counter:
    __slots__ = ("at", "end")

    def __init__(self, end):
        self.at, self.end = 0, end

    def __iter__(self):
        return self

    def __next__(self):
        if self.at >= self.end:
            raise StopIteration
        value = self.at
        self.at += 1
        return value


def erased(n):
    kept = (x for x in Counter(n) if x % 3 != 0)
    return (x * 2 for x in kept)


def via_dyn(n):
    return sum(erased(n))


def in_place(n):
    kept = (x for x in Counter(n) if x % 3 != 0)
    return sum(x * 2 for x in kept)


def plain(n):
    total = x = 0
    while x < n:
        if x % 3 != 0:
            total += x * 2
        x += 1
    return total


def over_range(n):
    kept = (x for x in range(n) if x % 3 != 0)
    return sum(x * 2 for x in kept)


def many(run):
    total = 0
    for _ in range(CALLS):
        total += run(SHORT)
    return total


def best(name, run):
    fastest = float("inf")
    result = 0
    for _ in range(3):
        start = perf_counter()
        result = run()
        fastest = min(fastest, perf_counter() - start)
    print(name, result)
    print(f"{name} {int(fastest * 1000)}ms", file=stderr)


def main():
    best("dyn", lambda: via_dyn(N))
    best("in-place", lambda: in_place(N))
    best("loop", lambda: plain(N))
    best("range", lambda: over_range(N))
    best("dyn-short", lambda: many(via_dyn))
    best("in-place-short", lambda: many(in_place))


if __name__ == "__main__":
    main()
