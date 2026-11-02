function even(value) {
  return value % 2 === 0;
}

export function main() {
  const xs = [1, 2, 3, 4, 5, 6, 7, 8];
  let total = 0;
  for (let i = 0; i < 500_000; i++) {
    total += xs.filter(even).length;
  }
  if (total !== 2_000_000) {
    throw new Error("bad filter total");
  }
}
