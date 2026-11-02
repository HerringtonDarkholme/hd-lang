function bump(values, key) {
  values.set(key, (values.get(key) ?? 0) + 1);
}

export function main() {
  const values = new Map([[1, 0]]);
  for (let i = 0; i < 10_000_000; i++) {
    bump(values, 1);
  }
  if (values.get(1) !== 10_000_000) {
    throw new Error("bad map value");
  }
}
