function checksum(count) {
  let total = 0;
  for (let i = 0; i < count; i++) {
    const result = i % 10007;
    total = (total + ((result * result) % 10007)) % 1000003;
  }
  return total;
}
console.log(checksum(80000000));
