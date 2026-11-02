export function main() {
  const xs = [1, 2, 3, 4, 5, 6, 7, 8];
  let found = 0;
  for (let i = 0; i < 4_000_000; i++) {
    if (xs.includes(8)) {
      found++;
    }
  }
  if (found !== 4_000_000) {
    throw new Error("value not found");
  }
}
