// The same generated log, typed integer parsing, per-key records and sorted report as hd.
const names = [];
for (let index = 0; index < 32; index++) {
  names.push(`tenant${index}`);
}
const lines = [];
for (let row = 0; row < 180000; row++) {
  let level = "INFO";
  if (row % 29 === 0) {
    level = "ERROR";
  } else if (row % 7 === 0) {
    level = "WARN";
  }
  lines.push(`${row % 86400} ${names[row % names.length]} ${level} ${((row * 17) % 10000) - 5000}`);
}
const text = lines.join("\n") + "\n",
  aggregates = new Map();
function record(stats, timestamp, level, amount) {
  stats.count++;
  stats.total += amount;
  stats.last = timestamp;
  if (level === "ERROR") {
    stats.errors++;
  }
  for (const character of level) {
    if (/\p{Alphabetic}/u.test(character)) {
      stats.letters++;
    }
  }
}
for (let round = 0; round < 4; round++) {
  for (const line of text.trimEnd().split("\n")) {
    const fields = line.split(" "),
      key = fields[1];
    let stats = aggregates.get(key);
    if (stats === undefined) {
      stats = { count: 0, total: 0, errors: 0, letters: 0, last: 0 };
      aggregates.set(key, stats);
    }
    record(stats, Number.parseInt(fields[0], 10), fields[2], Number.parseInt(fields[3], 10));
  }
}
let report = "";
for (const key of [...aggregates.keys()].sort()) {
  const stats = aggregates.get(key);
  report += `${key}:${stats.count}:${stats.total}:${stats.errors}:${stats.letters}:${stats.last};`;
}
console.log(report);
