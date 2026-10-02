// One worker of the in-process conformance adapter (hd-adapter.ts): it
// imports the compiler once, then runs each `hd` command line it receives
// and replies with the exit status and output a spawned `hd` would give.

import { enableCompileCache } from "node:module";
import { parentPort } from "node:worker_threads";

enableCompileCache?.();
const { runHd } = await import("./hd-in-process.ts");

const port = parentPort!;

port.on("message", async (args: readonly string[]) => {
  port.postMessage(await runHd(args));
});
port.postMessage("ready");
