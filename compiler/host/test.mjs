// `hd test` on V8 (Node): the test program is compiled once; each case
// runs in its own fresh instance (module.testing.instance), its module's
// init export first, then its test export polled to completion
// (engines-and-test-runner.md §19.3). `argv[3]` is the run's
// configuration, a `.json` file that `hd test` writes: the program's
// `Args.program` and `args` (Test Environments), then per case its `test`
// and `init` export indices, its own `tempDir`, the base `seed` of a
// property test, and its `grants`. Standard input is closed
// (cli.test.env.stdin).
// One JSON line per case goes to standard output as the case ends: its
// index, status, whether it trapped, its captured output and its time.
import { readFileSync, writeSync } from "node:fs";
import { createHost } from "./core.mjs";

const module = await WebAssembly.compile(readFileSync(process.argv[2]));
// Without a configuration file, `argv[3]` lists the cases alone, as
// `test:init` export indices separated by commas, which run with no
// arguments, no temporary directory and no grant limit.
const plain = (list) => ({
  program: "",
  args: [],
  cases: list
    .split(",")
    .filter((c) => c !== "")
    .map((c) => {
      const [test, init] = c.split(":").map(Number);
      return { test, init, tempDir: null, seed: null, grants: {} };
    }),
});
const spec = process.argv[3] ?? "";
const config = spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : plain(spec);
for (const c of config.cases) {
  let stdout = "";
  let stderr = "";
  const host = createHost(
    {
      out: (text) => {
        stdout += text;
      },
      err: (text) => {
        stderr += text;
      },
    },
    {
      args: config.args,
      program: config.program,
      stdin: null,
      grants: c.grants,
      tempDir: c.tempDir,
      seed: c.seed,
    },
  );
  const t0 = performance.now();
  const instance = await WebAssembly.instantiate(module, host.imports);
  const { status, trapped } = await host.run(instance, `hd.init.${c.init}`, `hd.test.${c.test}`);
  host.close();
  const us = Math.round((performance.now() - t0) * 1000);
  writeSync(1, JSON.stringify({ test: c.test, status, trapped, stdout, stderr, us }) + "\n");
}
