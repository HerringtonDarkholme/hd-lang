export function main() {
  const out = [];
  for (let i = 0; i < 3_000_000; i++) {
    out.push("abcdefgh");
  }
  const text = out.join("");
  if (text.length !== 24_000_000) {
    throw new Error("bad string length");
  }
}
