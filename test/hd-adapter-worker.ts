// One worker of the in-process conformance adapter (hd-adapter.ts): it
// imports the compiler once, then runs each `hd` command line it receives
// and replies with the exit status and output a spawned `hd` would give.

import { enableCompileCache } from "node:module";
import { parentPort } from "node:worker_threads";

import type { AdapterRunnerOptions } from "./hd-in-process.ts";

enableCompileCache?.();
const { runHd } = await import("./hd-in-process.ts");

const port = parentPort!;

interface Request {
  readonly args: readonly string[];
  readonly cwd?: string;
  readonly options?: AdapterRunnerOptions;
}

port.on("message", async ({ args, cwd, options }: Request) => {
  port.postMessage(await runHd(args, cwd === undefined ? {} : { cwd }, options));
});
port.postMessage("ready");
