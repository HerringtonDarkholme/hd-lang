export function main() {
  const xs = [];
  for (let i = 0; i < 2_000_000; i++) {
    xs.push(i);
  }
  if (xs.length !== 2_000_000) {
    throw new Error("bad push length");
  }
}
