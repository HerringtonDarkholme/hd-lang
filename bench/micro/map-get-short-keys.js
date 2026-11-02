export function main() {
  const keys = [];
  const values = new Map();
  for (let i = 0; i < 1000; i++) {
    const key = `k${i}`;
    keys.push(key);
    values.set(key, i % 7);
  }
  let total = 0;
  for (let step = 0; step < 5_000_000; step++) {
    total += values.get(keys[((step % 1000) * 7919) % 1000]) ?? 0;
  }
  if (total !== 14_985_000) {
    throw new Error("bad map total");
  }
}
