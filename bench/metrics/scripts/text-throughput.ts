// text-throughput: string building, splitting and regex speed, against
// Node.
//
// Three operations on text of words `word-I ` (I = 0, 1, ...):
// - build: a StringBuilder (Node: an array and join) of 100,000 words, N times;
// - split: `split(" ")` of a text of 100,000 words, N times;
// - regex: `find_all` of `[a-z]+-[0-9]+` over that text, N times.
// Each hd program (fixtures/text-throughput/) is built at two sizes, N =
// small and N = large; the time per unit is the median of (large run i -
// small run i) / (large - small), after a warm-up. The bytes per unit are
// the printed text length. Node does the same in its own process. Each
// build produces the same text; only the number of repetitions changes.
// Target (Pillar 3): within 2x of Node, so hd MB/s / Node MB/s ≥ 0.5.
// n/a: never; a program that fails to build or run fails its line.

import { howNote, nodeMedian, printed, runNodeProgram, scaledCost } from "../lib/artifact.ts";
import type { TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, type Metric } from "../lib/metric.ts";

const NAME = "text-throughput";
const WORDS = 100_000;
const RUNS = 5;
const TIMEOUT_MS = 120_000;
const LIMIT = 0.5;
const MB = 1024 * 1024;

interface Operation {
  readonly name: "build" | "split" | "regex";
  readonly label: string;
  readonly sizes: { readonly small: number; readonly large: number };
  /** Bytes of work per unit of size, from the large program's output. */
  readonly bytesPerUnit: (
    largeOutput: string,
    sizes: { small: number; large: number },
    smallOutput: string,
  ) => number;
}

const OPERATIONS: readonly Operation[] = [
  {
    name: "build",
    label: "string build, hd / Node MB/s",
    sizes: { small: 1, large: 11 },
    bytesPerUnit: (large, sizes, small) => {
      const bytes = printed(large, "bytes");
      if (
        printed(small, "bytes") !== bytes ||
        printed(small, "total") !== bytes * sizes.small ||
        printed(large, "total") !== bytes * sizes.large
      )
        throw new Error("string builds did not repeat the same-sized text");
      return bytes;
    },
  },
  {
    name: "split",
    label: "split, hd / Node MB/s",
    sizes: { small: 1, large: 11 },
    bytesPerUnit: (large) => printed(large, "bytes"),
  },
  {
    name: "regex",
    label: "regex find_all, hd / Node MB/s",
    sizes: { small: 1, large: 6 },
    bytesPerUnit: (large) => printed(large, "bytes"),
  },
];

const NODE_BODY = `
const WORDS = ${WORDS};
function build(n) {
  const parts = [];
  for (let i = 0; i < n; i++) parts.push(\`word-\${i} \`);
  return parts.join("");
}
const text = build(WORDS);
const bytes = Buffer.byteLength(text);
let sink = 0;
const buildMs = measure(() => { sink += build(WORDS).length; });
const splitMs = measure(() => { sink += text.split(" ").length; });
const pattern = /[a-z]+-[0-9]+/g;
const regexMs = measure(() => { sink += [...text.matchAll(pattern)].length; });
if (sink === 0) throw new Error("no work");
console.log(JSON.stringify({ bytes, build: buildMs, split: splitMs, regex: regexMs }));
`;

const mbPerSecond = (bytes: number, ms: number): number => bytes / MB / (ms / 1000);

export const textThroughput: Metric = {
  name: NAME,
  pillar: 3,
  summary: "string build, split and regex MB/s, against Node",
  async run(context) {
    const target = `≥ ${LIMIT.toFixed(2)}x`;
    let node: Record<string, unknown>;
    try {
      node = await runNodeProgram(NODE_BODY, "text");
    } catch (error) {
      return OPERATIONS.map((operation) => failed(NAME, operation.label, target, errorText(error)));
    }
    context.log(`${NAME}: build, split and regex, ${RUNS} runs at each of two sizes`);
    const results: TargetResult[] = [];
    for (const operation of OPERATIONS) {
      try {
        const measured = await scaledCost(
          context.hd,
          `text-throughput/${operation.name}`,
          "repeat",
          operation.sizes,
          RUNS,
          TIMEOUT_MS,
          { words: WORDS },
        );
        if (measured.msPerUnit <= 0) throw new Error("no work measured beyond start-up");
        const bytes = operation.bytesPerUnit(measured.large, operation.sizes, measured.small);
        if (bytes !== Number(node.bytes))
          throw new Error(`hd built ${bytes} bytes per text; Node built ${node.bytes}`);
        const hd = mbPerSecond(bytes, measured.msPerUnit);
        // Both sides time one whole, fixed-size text per unit.
        const nodeRate = mbPerSecond(Number(node.bytes), nodeMedian(node, operation.name));
        results.push(
          judge(
            NAME,
            operation.label,
            hd / nodeRate,
            LIMIT,
            "x",
            "at-least",
            `hd ${formatValue(hd, "MB/s")}, Node ${formatValue(nodeRate, "MB/s")}; ${WORDS} words, ${bytes} bytes per text${operation.name === "build" ? "; hd StringBuilder, Node Array.join" : ""}; ${howNote(measured.program)}`,
          ),
        );
      } catch (error) {
        results.push(failed(NAME, operation.label, target, errorText(error)));
      }
    }
    return results;
  },
};
