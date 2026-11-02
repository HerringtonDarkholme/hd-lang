export function main() {
  const values = new Map([
    ["a", 10],
    ["bb", 20],
    ["ccc", 30],
    ["dddd", 40],
  ]);
  let total = 0;
  for (let i = 0; i < 2_000_000; i++) {
    for (const [key, value] of values) {
      total += key.length + value;
    }
  }
  if (total !== 220_000_000) {
    throw new Error("bad map loop total");
  }
}
