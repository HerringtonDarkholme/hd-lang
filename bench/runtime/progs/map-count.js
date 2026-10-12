function tally(n, buckets) {
  const counts = new Map();
  for (let i = 0; i < n; i++) {
    const key = (i * 31 + 7) % buckets;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  let total = 0;
  for (const count of counts.values()) total += count;
  return total;
}
console.log(tally(200000, 5000));
