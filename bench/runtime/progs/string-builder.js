function build(count) {
  const parts = [];
  for (let i = 0; i < count; i++) {
    parts.push("abcdefgh");
  }
  return parts.join("");
}
console.log(build(33500000).length);
