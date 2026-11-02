export function main() {
  const xs = ["aa", "bb", "cc", "dd", "ee", "ff", "gg", "hh"];
  let found = 0;
  for (let i = 0; i < 4_000_000; i++) {
    if (xs.includes("hh")) {
      found++;
    }
  }
  if (found !== 4_000_000) {
    throw new Error("value not found");
  }
}
