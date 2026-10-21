"""Join 1.5 million pieces into one string and print its length."""
N = 1_500_000


def main():
    parts = [f"item-{i};" for i in range(N)]
    text = "".join(parts)
    print(len(text))


if __name__ == "__main__":
    main()
