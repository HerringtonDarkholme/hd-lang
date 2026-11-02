export function main() {
  const rows = new Map([
    ["a", [1, 2, 3]],
    ["b", [4, 5]],
  ]);
  let total = 0;
  for (let i = 0; i < 5_000_000; i++) {
    total += (rows.get("a") ?? []).length;
  }
  if (total !== 15_000_000) {
    throw new Error("bad nested-map total");
  }
}
