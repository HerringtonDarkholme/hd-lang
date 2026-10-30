// serde-throughput: JSON encode and decode speed of user data, against
// Node's JSON.
//
// The document is a list of 200 orders, each with an id, a customer name,
// a flag, an optional note, a list of tags, and 3 line records: about
// 45 KB of JSON. In hd (fixtures/serde-throughput/) the types derive
// Serialize and Deserialize, and the program encodes (`std.json.encode`)
// or decodes (`std.json.decode`) the document N times. Two builds, N = SMALL and N = LARGE, give the time
// per operation as the median of (large run i - small run i) / (LARGE -
// SMALL), after a warm-up. Node builds the same objects and times
// JSON.stringify and JSON.parse in its own process.
// Target (Pillar 3): within 2x of Node, so hd MB/s / Node MB/s ≥ 0.5.
// n/a: never; a program that fails to build or run fails its line.

import { howNote, nodeMedian, printed, runNodeProgram, scaledCost } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "serde-throughput";
const ORDERS = 200;
const SIZES = { small: 2, large: 32 };
const RUNS = 5;
const TIMEOUT_MS = 120_000;
const LIMIT = 0.5;
const MB = 1024 * 1024;

const NODE_BODY = `
const orders = [];
for (let i = 0; i < ${ORDERS}; i++) {
  const lines = [];
  for (let j = 0; j < 3; j++) lines.push({ sku: \`SKU-\${i}-\${j}\`, quantity: j + 1, price: 9.5 });
  orders.push({ id: i, customer: \`customer \${i}\`, paid: i % 2 === 0, note: i % 3 === 0 ? "leave at door" : null, tags: ["web", "eu"], lines });
}
const text = JSON.stringify(orders);
const REPS = 50;
let sink = 0;
const encode = measure(() => { for (let r = 0; r < REPS; r++) sink += JSON.stringify(orders).length; }).map((ms) => ms / REPS);
const decode = measure(() => { for (let r = 0; r < REPS; r++) sink += JSON.parse(text).length; }).map((ms) => ms / REPS);
if (sink === 0) throw new Error("no work");
console.log(JSON.stringify({ bytes: Buffer.byteLength(text), encode, decode }));
`;

const mbPerSecond = (bytes: number, ms: number): number => bytes / MB / (ms / 1000);

export const serdeThroughput: Metric = {
  name: NAME,
  pillar: 3,
  summary: "JSON encode and decode MB/s of derived types, against Node's JSON",
  async run(context) {
    const target = `≥ ${LIMIT.toFixed(2)}x`;
    const operations = ["encode", "decode"] as const;
    const labels = operations.map((operation) => `JSON ${operation}, hd / Node MB/s`);
    let node: Record<string, unknown>;
    try {
      node = await runNodeProgram(NODE_BODY, "serde");
    } catch (error) {
      return labels.map((label) => failed(NAME, label, target, errorText(error)));
    }
    context.log(
      `${NAME}: ${ORDERS} orders, ${SIZES.small} and ${SIZES.large} operations, ${RUNS} runs each`,
    );
    const results: TargetResult[] = [];
    for (const [index, operation] of operations.entries()) {
      const label = labels[index]!;
      try {
        const measured = await scaledCost(
          context.hd,
          `serde-throughput/${operation}`,
          "repeat",
          SIZES,
          RUNS,
          TIMEOUT_MS,
          { orders: ORDERS },
        );
        const bytes = printed(measured.large, "bytes");
        if (measured.msPerUnit <= 0) {
          throw new Error("no work measured beyond start-up");
        }
        const hd = mbPerSecond(bytes, measured.msPerUnit);
        const nodeRate = mbPerSecond(Number(node.bytes), nodeMedian(node, operation));
        results.push(
          judge(
            NAME,
            label,
            hd / nodeRate,
            LIMIT,
            "x",
            "at-least",
            `hd ${formatValue(hd, "MB/s")} (${bytes} B document), Node ${formatValue(nodeRate, "MB/s")}; ${howNote(measured.program)}`,
          ),
        );
      } catch (error) {
        results.push(failed(NAME, label, target, errorText(error)));
      }
    }
    return results;
  },
};
