export function main() {
  const xs = [1, 2, 3, 4, 5, 6, 7, 8];
  let total = 0;
  for (let i = 0; i < 10_000_000; i++) {
    total += xs[i % 8] ?? 0;
  }
  if (total !== 45_000_000) {
    throw new Error("bad get total");
  }
}
