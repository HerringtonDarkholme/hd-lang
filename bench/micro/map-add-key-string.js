export function main() {
  const values = new Map([["needle", 0]]);
  for (let i = 0; i < 10_000_000; i++) {
    values.set("needle", (values.get("needle") ?? 0) + 1);
  }
  if (values.get("needle") !== 10_000_000) {
    throw new Error("bad map value");
  }
}
