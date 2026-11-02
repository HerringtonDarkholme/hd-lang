function bump(xs, index) {
  xs[index]++;
}

export function main() {
  const xs = [0, 0, 0, 0, 0, 0, 0, 0];
  for (let i = 0; i < 10_000_000; i++) {
    bump(xs, i % 8);
  }
  if (xs.reduce((sum, value) => sum + value, 0) !== 10_000_000) {
    throw new Error("bad mutable reference total");
  }
}
