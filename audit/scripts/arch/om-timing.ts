// 5.1: map lookup complexity and string concat/interpolation growth.
//   node --experimental-strip-types audit/scripts/arch/om-timing.ts
import { writeFileSync } from "node:fs";
import {
  callExport,
  instantiatePlain,
  readProbe,
  ROOT,
  timeCall,
} from "./alloc-lib.ts";

const lines: string[] = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/om-timing.ts; date: ${new Date().toISOString()}`,
  "# timings are medians of 5-7 runs on a CPU shared with other audit workers; read ratios, not absolutes",
];
const log = (line: string): void => {
  lines.push(line);
  console.log(line);
};

const map = await instantiatePlain(
  readProbe("audit/probes/arch/object-model/om-map.hd"),
);
const reps = 20000;
log(
  "\n## map[i32, i32]: get cost (lookup(n, reps) - lookup(n, 0)), and build cost",
);
log("n\tbuild_ms\tget_total_ms\tns_per_get\tns_per_get / n");
for (const n of [10, 100, 1000, 10000]) {
  const build = timeCall(() => callExport(map, "lookup", n, 0), 5, 1);
  const withGets = timeCall(() => callExport(map, "lookup", n, reps), 5, 1);
  const perGet = ((withGets - build) * 1e6) / reps;
  log(
    `${n}\t${build.toFixed(3)}\t${(withGets - build).toFixed(3)}\t${perGet.toFixed(1)}\t${(perGet / n).toFixed(3)}`,
  );
}

const text = await instantiatePlain(
  readProbe("audit/probes/arch/object-model/om-string.hd"),
);
log(
  '\n## string accumulation: text = text + "x" and text = "${text}x" in a loop',
);
log("n\tconcat_ms\tinterp_ms\tconcat ms/n\tinterp ms/n");
for (const n of [1000, 10000, 100000]) {
  const runs = n >= 100000 ? 3 : 7;
  const concat = timeCall(() => callExport(text, "concat_loop", n), runs, 1);
  const interp = timeCall(() => callExport(text, "interp_loop", n), runs, 1);
  log(
    `${n}\t${concat.toFixed(3)}\t${interp.toFixed(3)}\t${(concat / n).toFixed(5)}\t${(interp / n).toFixed(5)}`,
  );
}
log('\n## fresh interpolation "v=${i};" per iteration (no accumulation)');
log("n\tms\tms/n");
for (const n of [1000, 10000, 100000]) {
  const fresh = timeCall(() => callExport(text, "interp_fresh", n), 7, 1);
  log(`${n}\t${fresh.toFixed(3)}\t${(fresh / n).toFixed(5)}`);
}
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/timing-map-string.txt`,
  lines.join("\n") + "\n",
);
