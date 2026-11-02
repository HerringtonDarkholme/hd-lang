export function main() {
  const values = new Map([
    ["needle", 7],
    ["other", 11],
  ]);
  let total = 0;
  for (let i = 0; i < 10_000_000; i++) {
    total += values.get("needle") ?? 0;
  }
  if (total !== 70_000_000) {
    throw new Error("bad map total");
  }
}
