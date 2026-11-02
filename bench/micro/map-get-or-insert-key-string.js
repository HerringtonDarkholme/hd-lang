function bump(values, key) {
  values.set(key, (values.get(key) ?? 0) + 1);
}

export function main() {
  const values = new Map([["needle", 0]]);
  for (let i = 0; i < 10_000_000; i++) {
    bump(values, "needle");
  }
  if (values.get("needle") !== 10_000_000) {
    throw new Error("bad map value");
  }
}
