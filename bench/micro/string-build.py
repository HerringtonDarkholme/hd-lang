def main():
    parts = []
    for i in range(100_000):
        parts.append(f"part-{i};")
    text = "".join(parts)
    assert len(text) == 1_088_890, len(text)


if __name__ == "__main__":
    import time

    times = []
    for _ in range(3):
        start = time.perf_counter()
        main()
        times.append((time.perf_counter() - start) * 1000)
    times.sort()
    print(f"{times[1]:.1f}")
