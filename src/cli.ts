import { readFile, writeFile } from "node:fs/promises";
import { basename, extname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { analyze, compile, instantiate, type ReplayEvent } from "./compiler.ts";
import { DiagnosticError, formatDiagnostic } from "./diagnostics.ts";
import { parse } from "./parser/index.ts";
import { explainRequirements } from "./requirements.ts";

function usage(): never {
  console.error(
    "usage: hd <parse|check|test|run|trace|record|replay|build|dump-hir|explain-requirements> [--wat] [--entry NAME] FILE",
  );
  process.exit(2);
}

export async function main(args = process.argv.slice(2)): Promise<number> {
  const command = args.shift();
  let wat = false;
  let entryName = "main";
  while (args[0]?.startsWith("--")) {
    const option = args.shift();
    if (option === "--wat") wat = true;
    else if (option === "--entry") entryName = args.shift() ?? usage();
    else usage();
  }
  const file = args.shift();
  if (!command || !file || args.length > 0) usage();
  if (entryName !== "main" && command !== "run") usage();
  const path = resolve(file);
  const source = await readFile(path, "utf8");
  try {
    if (command === "parse") {
      const result = parse(source);
      if (!result.program) throw new DiagnosticError(result.diagnostics);
      console.log(`${file}: ok`);
      return 0;
    }
    if (command === "check") {
      const result = analyze(source);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics)
        console.error(formatDiagnostic(file, diagnostic));
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
        console.log(
          `${explanation.functionName}: ${explanation.declared.length > 0 ? "$ " + explanation.declared.join(" + ") : "$()"}`,
        );
        for (const requirement of explanation.paths)
          console.log(`  ${requirement.key}: ${requirement.path.join(" -> ")}`);
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
    if (
      command === "test" ||
      command === "run" ||
      command === "trace" ||
      command === "record" ||
      command === "replay"
    ) {
      const functionNames = new Map<number, string>();
      const eventNames = [
        "construct",
        "poll",
        "ready",
        "cancel",
        "reentrant",
        "invalid-state",
        "pending",
        "cleanup",
      ] as const;
      const replayPath = `${path}.replay.json`;
      const recorded: ReplayEvent[] = [];
      const replayEvents =
        command === "replay"
          ? (JSON.parse(await readFile(replayPath, "utf8")) as ReplayEvent[])
          : undefined;
      const { instance, compilation, replay } = await instantiate(source, {
        console: (text) => console.log(text),
        trace:
          command === "trace"
            ? (functionIndex, event) =>
                console.log(
                  `${eventNames[event]} ${functionNames.get(functionIndex) ?? `function#${functionIndex}`}`,
                )
            : undefined,
        record: command === "record" ? (event) => recorded.push(event) : undefined,
        replay: replayEvents,
        providerConfigurationId: "cli-default",
      });
      compilation.hir.functions.forEach((declaration) =>
        functionNames.set(declaration.index, declaration.name),
      );
      const selected = compilation.hir.functions.filter((declaration) => {
        if (command === "test")
          return declaration.name === "main" || /^\$test\.\d+$/.test(declaration.name);
        return declaration.name === entryName;
      });
      if (selected.length === 0)
        throw new Error(
          command === "test"
            ? "program has no main function or test blocks"
            : `program has no exported ${entryName} function`,
        );
      let result: unknown;
      for (const declaration of selected) {
        if (declaration.parameters.length > 0)
          throw new Error(`${declaration.name} must not declare ordinary parameters`);
        const exportName = /^\$test\.\d+$/.test(declaration.name)
          ? `__hd_test_${declaration.name.slice(6)}`
          : declaration.name;
        const entry = instance.exports[exportName];
        if (typeof entry !== "function")
          throw new Error(`${declaration.name} has no runnable export`);
        result = entry(...declaration.requirements.map((requirement) => ({ requirement })));
      }
      replay.assertComplete();
      if (command === "record") {
        await writeFile(replayPath, JSON.stringify(recorded, null, 2) + "\n");
        console.log(replayPath);
      }
      if (command === "test") console.log(`${file}: ${selected.length} passed`);
      else if (result !== undefined) console.log(result);
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
