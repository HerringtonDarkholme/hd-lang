export function main() {
  const xs = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff", "ggggggg", "hhhhhhhh"];
  let total = 0;
  for (let i = 0; i < 1_000_000; i++) {
    for (const item of xs) {
      total += item.length;
    }
  }
  if (total !== 36_000_000) {
    throw new Error("bad loop total");
  }
}
