// One worker of the in-process conformance adapter (hd-adapter.ts): it
// imports the compiler once, then runs each `hd` command line it receives
// and replies with the exit status and output a spawned `hd` would give.

import { enableCompileCache } from "node:module";
import { parentPort } from "node:worker_threads";

enableCompileCache?.();
const { runHd } = await import("./hd-in-process.ts");

const port = parentPort!;

port.on("message", async ({ args, cwd }: { args: readonly string[]; cwd?: string }) => {
  port.postMessage(await runHd(args, cwd === undefined ? {} : { cwd }));
});
port.postMessage("ready");
