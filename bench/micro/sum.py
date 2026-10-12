def main():
    total = 0
    for i in range(1, 10_000_001):
        total += i
    assert total == 50000005000000, total


if __name__ == "__main__":
    import time

    times = []
    for _ in range(3):
        start = time.perf_counter()
        main()
        times.append((time.perf_counter() - start) * 1000)
    times.sort()
    print(f"{times[1]:.1f}")
