export function main() {
  const rows = [
    [1, 2],
    [3, 4],
  ];
  let total = 0;
  for (let i = 0; i < 5_000_000; i++) {
    total += (rows[i % 2] ?? []).length;
  }
  if (total !== 10_000_000) {
    throw new Error("bad nested-list total");
  }
}
