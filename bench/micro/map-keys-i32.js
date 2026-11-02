export function main() {
  const values = new Map([
    [1, 10],
    [2, 20],
    [3, 30],
    [4, 40],
  ]);
  let total = 0;
  for (let i = 0; i < 1_000_000; i++) {
    total += [...values.keys()].length;
  }
  if (total !== 4_000_000) {
    throw new Error("bad keys total");
  }
}
