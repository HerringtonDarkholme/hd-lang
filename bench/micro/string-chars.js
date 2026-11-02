export function main() {
  const text = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789é";
  let total = 0;
  for (let i = 0; i < 500_000; i++) {
    for (const character of text) {
      total++;
      void character;
    }
  }
  if (total !== 31_500_000) {
    throw new Error("bad character total");
  }
}
