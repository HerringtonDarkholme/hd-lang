// 5.2: suspension frame size versus locals live across each suspension point.
// Frame fields come from the emitted WAT. Live sets are derived by hand from
// the probe source (labeled as such); the emitter has no liveness analysis.
//   node --experimental-strip-types audit/scripts/arch/alloc-frames.ts
import { writeFileSync } from "node:fs";
import { analyze } from "../../../src/compiler.ts";
import { readProbe, ROOT, watFor } from "./alloc-lib.ts";

const probe = "audit/probes/arch/alloc/alloc-frames.hd";
const source = readProbe(probe);
const wat = watFor(source);
const hir = analyze(source).hir!;
// Hand-derived: parent locals whose value is read after resuming at each site.
const live: Record<string, string> = {
  wide: "site1: {} (e is consumed as the child's argument)",
  accumulate: "site1: {n, i, total-before-call} = 3",
  heap: "site1: {} ; site2: {} (text, values, point dead after first argument)",
  phases: "site1: {x} = 1 ; site2: {} ",
};
const lines = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/alloc-frames.ts; date: ${new Date().toISOString()}`,
  "function\tframe type\tvalue slots (args+locals)\tref slots among them\tchild slots\tcontrol slots\tmax live across a suspension (hand-derived)\tlive sets",
];
for (const declaration of hir.functions) {
  if (!declaration.suspending || !(declaration.name in live)) continue;
  const index = declaration.suspensionIndex ?? declaration.index;
  const start = wat.indexOf(`(type $s${index} (struct`);
  const end = wat.indexOf("result", start);
  const names = [
    ...wat.slice(start, end).matchAll(/\(field \$s\d+([a-z]+)(\d*) (.*)\)/g),
  ];
  const valueSlots = names.filter((m) => m[1] === "a" || m[1] === "l");
  const refs = valueSlots.filter((m) => m[3]!.includes("ref")).length;
  const children = names.filter((m) => m[1] === "child").length;
  const control =
    names.filter((m) => m[1] === "state" || m[1] === "polls").length + 1;
  const maxLive = Math.max(
    ...[...live[declaration.name]!.matchAll(/\{([^}]*)\}/g)].map((m) =>
      m[1]!.trim() ? m[1]!.split(",").length : 0,
    ),
  );
  lines.push(
    `${declaration.name}\t$s${index}\t${valueSlots.length}\t${refs}\t${children}\t${control}\t${maxLive}\t${live[declaration.name]}`,
  );
}
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/frames.tsv`,
  lines.join("\n") + "\n",
);
console.log(lines.join("\n"));
