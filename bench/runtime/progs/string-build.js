function build(n) {
  let out = "";
  for (let i = 0; i < n; i++) out = out + "abcdefgh";
  return out;
}
console.log(build(40000).length);
