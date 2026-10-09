// `hd run` on V8 (Node): one instance of the program, `hd.init` then
// `hd.poll` to completion (core.mjs); its status is the process's.
import { readFileSync, writeSync } from "node:fs";
import { createHost } from "./core.mjs";

const bytes = readFileSync(process.argv[2]);
// The program's arguments follow the module's path (`cli.args.pass`).
const host = createHost(
  {
    out: (text) => writeSync(1, text),
    err: (text) => writeSync(2, text),
  },
  process.argv.slice(3),
);
const { instance } = await WebAssembly.instantiate(bytes, host.imports);
const { status } = await host.run(instance, "hd.init", "hd.poll");
process.exitCode = status;
