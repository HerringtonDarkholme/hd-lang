function total(count) {
  const items = [];
  for (let i = 0; i < count; i++) {
    items.push((i * 31 + 7) % 100003);
  }
  return items
    .map((item) => item * 3 + 1)
    .filter((item) => item % 2 === 0)
    .reduce((acc, value) => (acc + value) % 1000003, 0);
}
console.log(total(24000000));
