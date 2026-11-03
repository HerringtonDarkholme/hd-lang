function makeText() {
  return ["prefix:", "tiny text", ":suffix"].join("");
}

export function main() {
  const text = makeText();
  let equal = 0;
  for (let i = 0; i < 20_000_000; i++) {
    if (text.slice(7, 16) === "tiny text") {
      equal++;
    }
  }
  if (equal !== 20_000_000) {
    throw new Error("substring differs");
  }
}
