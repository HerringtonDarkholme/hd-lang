def main():
    items = [(i * 48271) % 1_000_003 for i in range(100_000)]
    ordered = sorted(items)
    assert ordered[0] == 0, ordered[0]
    for i in range(1, 100_000):
        assert ordered[i] >= ordered[i - 1], i


if __name__ == "__main__":
    import time

    times = []
    for _ in range(3):
        start = time.perf_counter()
        main()
        times.append((time.perf_counter() - start) * 1000)
    times.sort()
    print(f"{times[1]:.1f}")
