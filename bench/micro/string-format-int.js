export function main() {
  let total = 0;
  for (let i = 0; i < 3_000_000; i++) {
    total += String(i % 100_000).length;
  }
  if (total !== 14_666_700) {
    throw new Error("bad format total");
  }
}
