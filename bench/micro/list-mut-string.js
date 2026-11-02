function toggle(xs, index) {
  xs[index] = xs[index].length === 1 ? "bb" : "a";
}

export function main() {
  const xs = ["a", "a", "a", "a", "a", "a", "a", "a"];
  for (let i = 0; i < 10_000_000; i++) {
    toggle(xs, i % 8);
  }
  if (xs.reduce((sum, value) => sum + value.length, 0) !== 8) {
    throw new Error("bad mutable reference total");
  }
}
