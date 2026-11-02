export function main() {
  let out = "";
  for (let i = 0; i < 66_000; i++) {
    out = out + "abcdefgh";
  }
  if (out.length !== 528_000) {
    throw new Error("bad string length");
  }
}
