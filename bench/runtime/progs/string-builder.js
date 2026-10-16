function build(n) {
  const parts = [];
  for (let i = 0; i < n; i++) parts.push("abcdefgh");
  return parts.join("");
}
console.log(build(40000).length);
