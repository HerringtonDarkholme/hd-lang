// `hd run` on V8 (Node): one instance of the program, `hd.init` then
// `hd.poll` to completion (core.mjs); its status is the process's.
// `argv[3]`, when it ends in `.json`, is the run's configuration that `hd`
// writes: the program's `Args.program` (`program`) and its capability
// grant (`grants`). The program's arguments follow (`cli.args.pass`). The
// conformance runner's configuration may name a fixture's runtime
// `profile` instead, whose providers profiles.mjs holds.
import { readFileSync, writeSync } from "node:fs";
import { createHost } from "./core.mjs";

const bytes = readFileSync(process.argv[2]);
const spec = process.argv[3] ?? "";
const config = spec.endsWith(".json") ? JSON.parse(readFileSync(spec, "utf8")) : {};
const args = process.argv.slice(spec.endsWith(".json") ? 4 : 3);
const profile = config.profile ? await import("./profiles.mjs") : null;
const host = createHost(
  {
    out: (text) => writeSync(1, text),
    err: (text) => writeSync(2, text),
  },
  {
    args,
    program: config.program ?? "",
    grants: config.grants ?? {},
    providers: profile ? (io) => profile.providers(config.profile, io) : null,
  },
);
const { instance } = await WebAssembly.instantiate(bytes, host.imports);
const { status } = await host.run(instance, "hd.init", "hd.poll");
host.close();
process.exitCode = status;
