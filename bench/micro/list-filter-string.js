function long(value) {
  return value.length > 3;
}

export function main() {
  const xs = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff", "ggggggg", "hhhhhhhh"];
  let total = 0;
  for (let i = 0; i < 500_000; i++) {
    total += xs.filter(long).length;
  }
  if (total !== 2_500_000) {
    throw new Error("bad filter total");
  }
}
