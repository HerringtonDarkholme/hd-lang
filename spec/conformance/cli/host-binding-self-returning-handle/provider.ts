class Node {
  readonly depth: number;
  constructor(depth: number) {
    this.depth = depth;
  }
  depth_of(): number {
    return this.depth;
  }
  next(): Node {
    return new Node(this.depth + 1);
  }
}

export default {
  root() {
    return new Node(0);
  },
};
