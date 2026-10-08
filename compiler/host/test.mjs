// `hd test` on V8 (Node): the test program is compiled once; each case
// runs in its own fresh instance (module.testing.instance), its module's
// init export first, then its test export polled to completion
// (engines-and-test-runner.md §19.3). The cases are `argv[3]`, as
// `test:init` export indices separated by commas. One JSON line per case
// goes to standard output as the case ends: its index, status, whether it
// trapped, its captured output and its time.
import { readFileSync, writeSync } from "node:fs";
import { createHost } from "./core.mjs";

const module = await WebAssembly.compile(readFileSync(process.argv[2]));
const cases = (process.argv[3] ?? "").split(",").filter((c) => c !== "");
for (const c of cases) {
  const [test, init] = c.split(":");
  let stdout = "";
  let stderr = "";
  const host = createHost({
    out: (text) => {
      stdout += text;
    },
    err: (text) => {
      stderr += text;
    },
  });
  const t0 = performance.now();
  const instance = await WebAssembly.instantiate(module, host.imports);
  const { status, trapped } = await host.run(instance, `hd.init.${init}`, `hd.test.${test}`);
  const us = Math.round((performance.now() - t0) * 1000);
  writeSync(1, JSON.stringify({ test: Number(test), status, trapped, stdout, stderr, us }) + "\n");
}
