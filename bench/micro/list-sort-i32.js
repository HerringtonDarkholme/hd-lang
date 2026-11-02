export function main() {
  const template = [8, 3, 6, 1, 7, 2, 5, 4];
  let total = 0;
  for (let i = 0; i < 200_000; i++) {
    const xs = template.slice();
    xs.sort((left, right) => left - right);
    total += xs[0];
  }
  if (total !== 200_000) {
    throw new Error("bad sort total");
  }
}
