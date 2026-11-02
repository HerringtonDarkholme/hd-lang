import { pathToFileURL } from "node:url";

const source = process.argv[2];
const runs = Number(process.argv[3] ?? "7");
if (!source || !Number.isInteger(runs) || runs < 1) {
  throw new Error("usage: node cost-js-runner.mjs SOURCE RUNS");
}

const loaded = await import(pathToFileURL(source).href);
if (typeof loaded.main !== "function") {
  throw new Error(`${source} must export function main()`);
}

loaded.main();
const times = [];
for (let run = 0; run < runs; run++) {
  const start = performance.now();
  loaded.main();
  times.push(performance.now() - start);
}
console.log(Math.min(...times));
