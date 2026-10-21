"""Build two million words, split the text, and count each word."""
N = 650_000


def main():
    parts = [f"w{(i * 7919) % 50_021}" for i in range(N)]
    text = " ".join(parts)
    counts = {}
    for word in text.split(" "):
        counts[word] = counts.get(word, 0) + 1
    squares = 0
    for count in counts.values():
        squares += count * count
    print(len(counts), squares)


if __name__ == "__main__":
    main()
