import { readFile, writeFile } from "node:fs/promises";
import { basename, extname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { analyze, compile, instantiate, type ReplayEvent } from "./compiler.ts";
import { DiagnosticError, formatDiagnostic } from "./diagnostics.ts";
import { explainRequirements } from "./requirements.ts";

function usage(): never {
  console.error("usage: hd <run|trace|record|replay|check|build|dump-hir|explain-requirements> [--wat] FILE");
  process.exit(2);
}

export async function main(args = process.argv.slice(2)): Promise<number> {
  const command = args.shift();
  const wat = args[0] === "--wat" ? Boolean(args.shift()) : false;
  const file = args.shift();
  if (!command || !file || args.length > 0) usage();
  const path = resolve(file);
  const source = await readFile(path, "utf8");
  try {
    if (command === "check") {
      const result = analyze(source);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics) console.error(formatDiagnostic(file, diagnostic));
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "dump-hir") {
      const result = analyze(source);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      console.log(JSON.stringify(result.hir, null, 2));
      return 0;
    }
    if (command === "explain-requirements") {
      const result = analyze(source);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const explanation of explainRequirements(result.hir)) {
        console.log(`${explanation.functionName}: ${explanation.declared.length > 0 ? "$ " + explanation.declared.join(" + ") : "$()"}`);
        for (const requirement of explanation.paths) console.log(`  ${requirement.key}: ${requirement.path.join(" -> ")}`);
      }
      return 0;
    }
    if (command === "build") {
      const result = compile(source);
      if (wat) {
        console.log(result.wat);
      } else {
        const output = resolve(`${basename(path, extname(path))}.wasm`);
        await writeFile(output, result.bytes);
        console.log(output);
      }
      return 0;
    }
    if (command === "run" || command === "trace" || command === "record" || command === "replay") {
      const functionNames = new Map<number, string>();
      const eventNames = ["construct", "poll", "ready", "cancel", "reentrant", "invalid-state", "pending", "cleanup"] as const;
      const replayPath = `${path}.replay.json`;
      const recorded: ReplayEvent[] = [];
      const replayEvents = command === "replay"
        ? JSON.parse(await readFile(replayPath, "utf8")) as ReplayEvent[]
        : undefined;
      const { instance, compilation, replay } = await instantiate(source, {
        console: (text) => console.log(text),
        trace: command === "trace"
          ? (functionIndex, event) => console.log(`${eventNames[event]} ${functionNames.get(functionIndex) ?? `function#${functionIndex}`}`)
          : undefined,
        record: command === "record" ? (event) => recorded.push(event) : undefined,
        replay: replayEvents,
        providerConfigurationId: "cli-default",
      });
      compilation.hir.functions.forEach((declaration) => functionNames.set(declaration.index, declaration.name));
      const entry = instance.exports.main;
      if (typeof entry !== "function") throw new Error("program has no exported main function");
      const mainFunction = compilation.hir.functions.find((declaration) => declaration.name === "main");
      if (!mainFunction || mainFunction.parameters.length > 0) throw new Error("main must not declare ordinary parameters");
      const result = entry(...mainFunction.requirements.map((requirement) => ({ requirement })));
      replay.assertComplete();
      if (command === "record") {
        await writeFile(replayPath, JSON.stringify(recorded, null, 2) + "\n");
        console.log(replayPath);
      }
      if (result !== undefined) console.log(result);
      return 0;
    }
    usage();
  } catch (error) {
    if (error instanceof DiagnosticError) {
      for (const diagnostic of error.diagnostics) console.error(formatDiagnostic(file, diagnostic));
      return 1;
    }
    throw error;
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
