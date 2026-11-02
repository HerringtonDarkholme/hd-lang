export function main() {
  let total = 0;
  for (let i = 0; i < 400_000; i++) {
    total += (1.0001 + (i % 1000) / 1000) ** (0.5 + (i % 7));
  }
  if (!(total > 0)) {
    throw new Error("bad power total");
  }
}
