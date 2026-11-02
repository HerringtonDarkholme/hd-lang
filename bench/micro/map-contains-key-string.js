export function main() {
  const values = new Map([
    ["needle", 7],
    ["other", 11],
  ]);
  let found = 0;
  for (let i = 0; i < 10_000_000; i++) {
    if (values.has("needle")) {
      found++;
    }
  }
  if (found !== 10_000_000) {
    throw new Error("key not found");
  }
}
