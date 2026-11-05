// Lazy sensor adapters and a captured counter, matching iter-pipeline.hd.
function weighted(value, scale, bias) {
  return value * scale + bias;
}
function* map(source, transform) {
  for (const value of source) {
    yield transform(value);
  }
}
function* filter(source, keep) {
  for (const value of source) {
    if (keep(value)) {
      yield value;
    }
  }
}
function fold(source, initial, step) {
  let total = initial;
  for (const value of source) {
    total = step(total, value);
  }
  return total;
}
const readings = [];
for (let index = 0; index < 20000; index++) {
  readings.push((index * 7919) % 10007);
}
let total = 0,
  seen = 0;
for (let round = 0; round < 1000; round++) {
  const scale = (round % 7) + 1,
    bias = round % 13,
    cutoff = (round * 31) % 997;
  const mapped = map(readings, (value) => {
    seen++;
    return weighted(value, scale, bias);
  });
  total += fold(
    filter(mapped, (value) => value % 17 !== 0 && value > cutoff),
    0,
    (sum, value) => sum + value,
  );
}
console.log(`${total} ${seen}`);
