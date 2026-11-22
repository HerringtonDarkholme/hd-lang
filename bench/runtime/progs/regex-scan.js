// The same generated request log and the same four regex searches as hd.
const lines = [];
for (let row = 0; row < 30000; row++) {
  let level = "INFO";
  if (row % 23 === 0) {
    level = "ERROR";
  } else if (row % 7 === 0) {
    level = "WARN";
  }
  lines.push(
    `2026-11-22T${10 + (row % 12)}:${10 + (row % 50)}:${10 + (row % 49)} ${level} user${row % 977}@mail${row % 13}.example.com GET /api/v${row % 3}/items/${(row * 31) % 100000} took ${(row * 7) % 1500}ms\n`,
  );
}
const text = lines.join("");
const addresses = /[a-z0-9.]+@[a-z0-9]+\.example\.com/g;
const slow = /took [0-9][0-9][0-9][0-9]ms/;
const ids = /\/items\/[0-9]+/g;
const who = /ERROR user([0-9]+)@mail([0-9]+)/;
let sum = 0;
for (const found of text.matchAll(addresses)) {
  sum += found[0].length;
}
let slowLines = 0;
let errorUsers = 0;
for (const line of text.split("\n").slice(0, -1)) {
  if (slow.test(line)) {
    slowLines++;
  }
  const groups = who.exec(line);
  if (groups !== null) {
    errorUsers += Number(groups[1]) * 13 + Number(groups[2]);
  }
}
const masked = text.replace(ids, "/items/:id");
console.log(`${sum} ${slowLines} ${errorUsers} ${masked.length}`);
