export function main() {
  const values = new Map([
    ["a", 10],
    ["bb", 20],
    ["ccc", 30],
    ["dddd", 40],
  ]);
  let total = 0;
  for (let i = 0; i < 1_000_000; i++) {
    total += [...values.keys()].length;
  }
  if (total !== 4_000_000) {
    throw new Error("bad keys total");
  }
}
