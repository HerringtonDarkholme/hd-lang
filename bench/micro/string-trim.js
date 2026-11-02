export function main() {
  const text = "    alpha beta gamma delta    ";
  let total = 0;
  for (let i = 0; i < 3_000_000; i++) {
    total += text.trim().length;
  }
  if (total !== 66_000_000) {
    throw new Error("bad trim total");
  }
}
