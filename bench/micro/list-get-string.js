export function main() {
  const xs = ["aa", "bb", "cc", "dd", "ee", "ff", "gg", "hh"];
  let total = 0;
  for (let i = 0; i < 10_000_000; i++) {
    total += (xs[i % 8] ?? "").length;
  }
  if (total !== 20_000_000) {
    throw new Error("bad get total");
  }
}
