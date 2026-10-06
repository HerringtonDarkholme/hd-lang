// Corpus maintenance: shows what an `hd` reports for each mistake program,
// and whether its `# fixed:` variant checks clean, so a reviewer can spot a
// program that holds a second mistake. It judges nothing; the `mistakes`
// metric does that.
//
// Usage: node --experimental-strip-types test/metrics/mistakes/check-corpus.ts
//          [--hd "COMMAND"] [NAME...]

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { loadMistakes } from "../lib/corpus.ts";
import { diagnosticsOf, parseHdCommand, runHd } from "../lib/hd.ts";
import { makeTempDir, removeAllTempDirs } from "../lib/tmp.ts";

const args = process.argv.slice(2);
const hdAt = args.indexOf("--hd");
const hd = parseHdCommand(
  hdAt >= 0 ? args[hdAt + 1]! : "node --experimental-strip-types bin/hd.js",
  join(import.meta.dirname, "..", "..", ".."),
);
const names = args.filter((arg, index) => index !== hdAt && index !== hdAt + 1);
const corpus = loadMistakes(import.meta.dirname).filter(
  (mistake) => names.length === 0 || names.includes(mistake.name),
);

const checkText = async (file: string, text: string) => {
  const dir = makeTempDir("corpus");
  writeFileSync(join(dir, file), text);
  const result = await runHd(hd, ["check", "--tests", "--format", "json", file], { cwd: dir });
  return diagnosticsOf(result.stdout + result.stderr);
};

try {
  for (const mistake of corpus) {
    const found = await checkText(`${mistake.name}.hd`, mistake.text);
    const shown = found.map((d) => `${String(d.code)}@${String(d.line)}`).join(" ") || "none";
    const fixed =
      mistake.fixedText === undefined
        ? "-"
        : (await checkText(`${mistake.name}.hd`, mistake.fixedText))
            .map((d) => `${String(d.code)}@${String(d.line)}`)
            .join(" ") || "clean";
    const ok =
      found.length === 1 && found[0]!.code === mistake.code && found[0]!.line === mistake.line;
    console.log(
      `${ok ? "ok  " : "DIFF"} ${mistake.name}: expect ${mistake.code}@${mistake.line}; got ${shown}; fixed: ${fixed}`,
    );
  }
} finally {
  removeAllTempDirs();
}
