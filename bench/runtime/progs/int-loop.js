function checksum(n) {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const r = i % 10007;
    total = (total + ((r * r) % 10007)) % 1000003;
  }
  return total;
}
console.log(checksum(240000000));
