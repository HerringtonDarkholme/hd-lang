def main():
    table = {}
    for i in range(100_000):
        table[i] = i * 2
    total = 0
    for i in range(100_000):
        total += table.get(i, 0)
    assert total == 9999900000, total


if __name__ == "__main__":
    import time

    times = []
    for _ in range(3):
        start = time.perf_counter()
        main()
        times.append((time.perf_counter() - start) * 1000)
    times.sort()
    print(f"{times[1]:.1f}")
