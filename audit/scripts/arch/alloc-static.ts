// 5.2: static allocation-instruction counter over emitted WAT.
// Counts struct.new*, array.new* per op and type, split into user code,
// runtime ($hd.*) functions, and global initializers. Boxing is inline
// `struct.new $hd.box-*` in this emitter, so box counts appear by type.
//   node --experimental-strip-types audit/scripts/arch/alloc-static.ts [files...]
import { readdirSync, writeFileSync } from "node:fs";
import {
  allocationSites,
  isRuntimeFunction,
  readProbe,
  ROOT,
  watFor,
} from "./alloc-lib.ts";

const defaults = ["object-model", "alloc", "erasure"].flatMap((dir) =>
  readdirSync(`${ROOT}audit/probes/arch/${dir}`)
    .filter((name) => name.endsWith(".hd"))
    .map((name) => `audit/probes/arch/${dir}/${name}`),
);
const files = process.argv.length > 2 ? process.argv.slice(2) : defaults;
const lines = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/alloc-static.ts; date: ${new Date().toISOString()}`,
  "probe\tuser sites\tuser boxes\truntime sites\tglobal-init sites\tuser sites by type",
];
for (const file of files.sort()) {
  let wat: string;
  try {
    wat = watFor(readProbe(file));
  } catch (error) {
    lines.push(`${file}\tCHECK FAILED ${String(error).split("\n")[1] ?? ""}`);
    continue;
  }
  let sites;
  try {
    sites = allocationSites(wat);
  } catch (error) {
    lines.push(`${file}\tUNBALANCED WAT ${String(error)}`);
    continue;
  }
  const user = sites.filter(
    (site) => !site.func.startsWith("<") && !isRuntimeFunction(site.func),
  );
  const runtime = sites.filter((site) => isRuntimeFunction(site.func));
  const globals = sites.filter((site) => site.func.startsWith("<"));
  const byType = new Map<string, number>();
  for (const site of user)
    byType.set(site.type, (byType.get(site.type) ?? 0) + 1);
  const boxes = user.filter((site) => site.type.startsWith("$hd.box-")).length;
  lines.push(
    `${file.replace("audit/probes/arch/", "")}\t${user.length}\t${boxes}\t${runtime.length}\t${globals.length}\t${[...byType].map(([type, count]) => `${type}=${count}`).join(" ")}`,
  );
}
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/alloc-static.tsv`,
  lines.join("\n") + "\n",
);
console.log(lines.join("\n"));
