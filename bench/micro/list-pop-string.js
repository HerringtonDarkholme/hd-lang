export function main() {
  const xs = [];
  for (let i = 0; i < 2_000_000; i++) {
    xs.push("value");
  }
  let total = 0;
  for (let i = 0; i < 2_000_000; i++) {
    total += (xs.pop() ?? "").length;
  }
  if (total !== 10_000_000) {
    throw new Error("bad pop total");
  }
}
