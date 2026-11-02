export function main() {
  const xs = [1, 2, 3, 4, 5, 6, 7, 8];
  let total = 0;
  for (let i = 0; i < 4_000_000; i++) {
    for (const item of xs) {
      total += item;
    }
  }
  if (total !== 144_000_000) {
    throw new Error("bad loop total");
  }
}
