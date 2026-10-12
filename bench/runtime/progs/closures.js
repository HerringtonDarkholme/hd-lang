function total(n) {
  const items = [];
  for (let i = 0; i < n; i++) items.push((i * 31 + 7) % 100003);
  return items
    .map((x) => x * 3 + 1)
    .filter((x) => x % 2 === 0)
    .reduce((acc, x) => (acc + x) % 1000003, 0);
}
console.log(total(20000000));
