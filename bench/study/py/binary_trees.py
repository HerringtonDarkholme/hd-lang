"""Allocate and traverse binary trees, maximum depth 14."""
MAX = 14
MIN = 4


class Node:
    __slots__ = ("left", "right")

    def __init__(self, left=None, right=None):
        self.left = left
        self.right = right


def make(depth):
    if depth == 0:
        return Node()
    return Node(make(depth - 1), make(depth - 1))


def check(tree):
    if tree.left is None:
        return 1
    return 1 + check(tree.left) + check(tree.right)


def main():
    print("stretch", MAX + 1, check(make(MAX + 1)))
    long_lived = make(MAX)
    for depth in range(MIN, MAX + 1, 2):
        iterations = 1 << (MAX - depth + MIN)
        total = 0
        for _ in range(iterations):
            total += check(make(depth))
        print(iterations, depth, total)
    print("long", MAX, check(long_lived))


if __name__ == "__main__":
    main()
