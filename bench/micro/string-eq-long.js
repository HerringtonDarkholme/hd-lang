function makeLong() {
  return ["abcdefghijklmnopqrstuvwxyz012345", "ABCDEFGHIJKLMNOPQRSTUVWXYZ678901"].join("");
}

export function main() {
  const left = makeLong();
  const right = makeLong();
  let equal = 0;
  for (let i = 0; i < 20_000_000; i++) {
    if (left === right) {
      equal++;
    }
  }
  if (equal !== 20_000_000) {
    throw new Error("long strings differ");
  }
}
