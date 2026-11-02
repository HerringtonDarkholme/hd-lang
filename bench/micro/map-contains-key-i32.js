export function main() {
  const values = new Map([
    [1, 7],
    [2, 11],
  ]);
  let found = 0;
  for (let i = 0; i < 10_000_000; i++) {
    if (values.has(1)) {
      found++;
    }
  }
  if (found !== 10_000_000) {
    throw new Error("key not found");
  }
}
