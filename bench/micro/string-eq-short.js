function makeShort() {
  return ["tiny", " text"].join("");
}

export function main() {
  const left = makeShort();
  const right = makeShort();
  let equal = 0;
  for (let i = 0; i < 20_000_000; i++) {
    if (left === right) {
      equal++;
    }
  }
  if (equal !== 20_000_000) {
    throw new Error("short strings differ");
  }
}
