export function main() {
  const template = ["hh", "cc", "ff", "aa", "gg", "bb", "ee", "dd"];
  let total = 0;
  for (let i = 0; i < 200_000; i++) {
    const xs = template.slice();
    xs.sort();
    total += xs[0].length;
  }
  if (total !== 400_000) {
    throw new Error("bad sort total");
  }
}
