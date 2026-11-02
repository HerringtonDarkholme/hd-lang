export function main() {
  const xs = [{ value: 0 }];
  for (let i = 0; i < 20_000_000; i++) {
    xs[0].value = i % 2;
  }
  if (xs[0].value !== 1) {
    throw new Error("bad field value");
  }
}
