// One worker of the in-process conformance adapter (hd-adapter.ts): it
// imports the compiler once, then runs each `hd` command line it receives
// and replies with the exit status and output a spawned `hd` would give.

import { enableCompileCache } from "node:module";
import { inspect } from "node:util";
import { parentPort } from "node:worker_threads";

enableCompileCache?.();
const { main } = await import("../src/cli.ts");
const { bufferedIo } = await import("../src/commands/index.ts");

const port = parentPort!;

port.on("message", async (args: readonly string[]) => {
  const io = bufferedIo();
  let status: number;
  try {
    status = await main([...args], io);
  } catch (error) {
    // An error that escapes `main` ends the `hd` process with status 1 after
    // Node prints it.
    io.err(inspect(error));
    status = 1;
  }
  // A process exit status is one byte.
  port.postMessage({ status: ((status % 256) + 256) % 256, ...io.output() });
});
port.postMessage("ready");
