export function main() {
  const values = new Map([
    [1, 10],
    [2, 20],
    [3, 30],
    [4, 40],
  ]);
  let total = 0;
  for (let i = 0; i < 2_000_000; i++) {
    for (const [key, value] of values) {
      total += key + value;
    }
  }
  if (total !== 220_000_000) {
    throw new Error("bad map loop total");
  }
}
