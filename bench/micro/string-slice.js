export function main() {
  const text = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
  let total = 0;
  for (let i = 0; i < 3_000_000; i++) {
    const start = i % 32;
    const part = text.slice(start, start + 12);
    total += part.length;
  }
  if (total !== 36_000_000) {
    throw new Error("bad slice total");
  }
}
