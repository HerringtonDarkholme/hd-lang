export function main() {
  const source = "let x = fn if else while return foo bar if let while fn x y z return else baz";
  const tokens = source.split(" ");
  let count = 0;
  for (let round = 0; round < 500_000; round++) {
    for (const token of tokens) {
      if (token === "if" || token === "let" || token === "fn" || token === "while") {
        count++;
      }
    }
  }
  if (count !== 4_000_000) {
    throw new Error("bad keyword count");
  }
}
