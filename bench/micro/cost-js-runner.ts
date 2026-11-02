import { pathToFileURL } from "node:url";

const source = process.argv[2];
const runs = Number(process.argv[3] ?? "7");
if (!source || !Number.isInteger(runs) || runs < 1) {
  throw new Error("usage: node --experimental-strip-types cost-js-runner.ts SOURCE RUNS");
}

const loaded = (await import(pathToFileURL(source).href)) as { main?: unknown };
if (typeof loaded.main !== "function") {
  throw new Error(`${source} must export function main()`);
}

loaded.main();
const times: number[] = [];
for (let run = 0; run < runs; run++) {
  const start = performance.now();
  loaded.main();
  times.push(performance.now() - start);
}
console.log(Math.min(...times));
