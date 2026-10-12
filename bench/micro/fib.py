def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)


def main():
    result = fib(30)
    assert result == 832040, result


if __name__ == "__main__":
    import time

    times = []
    for _ in range(3):
        start = time.perf_counter()
        main()
        times.append((time.perf_counter() - start) * 1000)
    times.sort()
    print(f"{times[1]:.1f}")
