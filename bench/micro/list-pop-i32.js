export function main() {
  const xs = [];
  for (let i = 0; i < 2_000_000; i++) {
    xs.push(1);
  }
  let total = 0;
  for (let i = 0; i < 2_000_000; i++) {
    total += xs.pop() ?? 0;
  }
  if (total !== 2_000_000) {
    throw new Error("bad pop total");
  }
}
