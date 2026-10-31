function build(count) {
  let out = "";
  for (let i = 0; i < count; i++) {
    out = out + "abcdefgh";
  }
  return out;
}
console.log(build(66000).length);
