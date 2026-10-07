// Tiny JS host for the walking skeleton: provides `println` of an i32 and
// runs the module's `main` export on V8 (Node).
import { readFileSync } from "node:fs";

const bytes = readFileSync(process.argv[2]);
const lines = [];
const imports = { hd: { println_i32: (x) => lines.push(String(x)) } };
const t0 = performance.now();
const { instance } = await WebAssembly.instantiate(bytes, imports);
const t1 = performance.now();
instance.exports.main();
const t2 = performance.now();
process.stdout.write(lines.join("\n") + (lines.length ? "\n" : ""));
process.stderr.write(`instantiate_ms=${(t1 - t0).toFixed(3)} run_ms=${(t2 - t1).toFixed(3)}\n`);
