import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { inventory } from "../spec/tools/rule-inventory.ts";
import { knownCodes } from "../spec/tools/spec-prose.ts";
import { audit, auditTotals, normalizeSentence } from "../spec/tools/spec-audit.ts";
import { chapterNames, loadCorpus, REPO_ROOT, SPEC_ROOT } from "../spec/tools/spec-corpus.ts";
import { counts } from "../spec/tools/spec-counts.ts";
import {
  areaOf,
  type Citation,
  citationsIn,
  deadCitations,
  type RefIndex,
  retirements,
} from "../spec/tools/spec-refs.ts";
import { run } from "../spec/tools/spec.ts";

// A miniature specification: one language chapter, one stdlib chapter, and
// the README tables the tools read.
const README = `# Spec

### Diagnostics

| Severity | Stable diagnostic codes |
| --- | --- |
| Error | \`bad-thing\`, \`never-named\`, \`syntax-error\` |

## Revision Notes

- Widgets (owner decision W1, batch 7 in Records, 2026-09-30):
  \`lex.widget.old\` and \`.older\` are retired. All \`lex.gone.*\` rules
  are retired. \`std.widget\` is a module, not a rule.
`;

const LEXICAL = `# Lexical Structure

## Widgets

1. r[lex.widget.one] A widget must be blue. Error: \`bad-thing\`.
2. r[lex.widget.two] A widget may be round and must sit on the left of the gadget line.
3. r[lex.widget.three] This sentence has far too many words because it keeps going on and on and on, well past the twenty-five word target that the style guide sets for every sentence.

\`\`\`text
widget
\`\`\`

## Gadgets

1. r[lex.gadget.one] A widget may be round and must sit on the left of the gadget line.
2. r[lex.gadget.two] A gadget is bad. Error: \`unlisted-code\`.
`;

const STD_ITER = `# std.iter

## Adapters

1. r[std-iter.map.lazy] \`map\` is lazy.

\`\`\`text
xs.map(f)
\`\`\`
`;

async function withSpec(run: (spec: string) => Promise<void>): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), "hd-spec-tools-"));
  try {
    const spec = join(root, "spec");
    await mkdir(join(spec, "std"), { recursive: true });
    await writeFile(join(spec, "README.md"), README);
    await writeFile(join(spec, "01-lexical-structure.md"), LEXICAL);
    await writeFile(join(spec, "std", "README.md"), "# Std\n");
    await writeFile(join(spec, "std", "iter.md"), STD_ITER);
    await run(spec);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("counts rules per chapter, prefix, tier, and heuristic kind", async () => {
  await withSpec(async (spec) => {
    assert.deepEqual(chapterNames(spec), ["01-lexical-structure.md", "std/iter.md"]);
    const result = counts(loadCorpus(spec));
    assert.deepEqual(result.tiers, { language: 5, std: 1 });
    assert.equal(result.total, 6);
    assert.deepEqual(result.prefixes, { lex: 5, "std-iter": 1 });
    assert.equal(result.topics["lex.widget"], 3);
    assert.equal(result.kinds.syntax, 5);
    assert.equal(result.kinds["core-library"], 1);
  });
});

test("counts matches rule-inventory over the real chapters", async () => {
  const known = knownCodes(
    await readFile(resolve(SPEC_ROOT, "README.md"), "utf8"),
    await readFile(resolve(SPEC_ROOT, "06-control-flow.md"), "utf8"),
  );
  const numbered = (await readdir(SPEC_ROOT)).filter((name) => /^\d\d-.*\.md$/.test(name));
  let language = 0;
  for (const name of numbered)
    language += inventory(name, await readFile(resolve(SPEC_ROOT, name), "utf8"), known).rules
      .length;
  assert.equal(counts(loadCorpus()).tiers.language, language);
});

test("audit reports each STYLE.md target as a warning kind", async () => {
  await withSpec(async (spec) => {
    const warnings = audit(loadCorpus(spec));
    const totals = auditTotals(warnings);
    assert.equal(totals["long-sentence"], 1);
    assert.equal(totals["no-example"], 1, "Gadgets has rules and no example");
    assert.equal(totals["duplicate-rule"], 1);
    assert.equal(totals["unknown-code"], 1);
    assert.equal(totals["bad-rule-id"], 0);
    assert.deepEqual(
      warnings.filter((w) => w.kind === "unused-code").map((w) => w.message.split(" ")[0]),
      ["`never-named`", "`syntax-error`"],
    );
    const duplicate = warnings.find((w) => w.kind === "duplicate-rule")!;
    assert.match(duplicate.message, /lex\.gadget\.one repeats a sentence of lex\.widget\.two/);
  });
});

test("audit keeps inline code apart when it compares sentences", () => {
  assert.notEqual(normalizeSentence("`a` is an error."), normalizeSentence("`b` is an error."));
  assert.equal(
    normalizeSentence("Fields may *appear* in any order."),
    "fields may appear in any order",
  );
});

test("refs finds anchors and bare IDs in prose and comments, not code", () => {
  const live = new Set(["data.embed.width"]);
  const isRuleId = (id: string): boolean => live.has(id);
  const markdown = [
    "See [`data.embed.width`](08-data-and-enums.md#r-data.embed.width).",
    "```text",
    "data.embed.width",
    "```",
    "A record: `data.embed.width` (since retired).",
  ].join("\n");
  const found = citationsIn("guide/X.md", markdown, "guide", isRuleId);
  assert.deepEqual(
    found.map((c) => [c.line, c.form, c.target, c.history]),
    [
      [1, "anchor", "08-data-and-enums.md", false],
      [5, "id", "", true],
    ],
  );
  const source = [
    "const x = data.embed.width; // see data.embed.width",
    "/* spec/08-data-and-enums.md#r-data.embed.width */",
    'const url = "#r-data.embed.width";',
  ].join("\n");
  assert.deepEqual(
    citationsIn("src/x.ts", source, "src", isRuleId).map((c) => [c.line, c.form]),
    [
      [1, "id"],
      [2, "anchor"],
    ],
  );
  const hd = "# (spec/08-data-and-enums.md#r-data.embed.width)\nx := data.embed.width\n";
  assert.equal(citationsIn("lib/std/x.hd", hd, "lib-std", isRuleId).length, 1);
});

test("refs reads retirements from Revision Notes, with relative IDs and wildcards", () => {
  const { exact, wildcards } = retirements(README, new Set(["lex"]));
  assert.match(exact.get("lex.widget.old")!, /^batch 7 /);
  assert.ok(exact.has("lex.widget.older"));
  assert.ok(wildcards.has("lex.gone"));
  assert.ok(!exact.has("std.widget"), "a module path is not a rule ID");
});

test("refs --dead fails only gated areas and allows history", () => {
  const cite = (file: string, id: string, history = false, target = ""): Citation => ({
    file,
    line: 1,
    id,
    form: target ? "anchor" : "id",
    target,
    area: areaOf(file)!,
    history,
    text: "",
  });
  const index: RefIndex = {
    live: new Map([["lex.widget.one", "spec/01-lexical-structure.md:5"]]),
    historical: new Set(["lex.widget.old"]),
    retired: retirements(README, new Set(["lex"])),
    citations: [
      cite("spec/02-grammar.md", "lex.widget.one", false, "01-lexical-structure.md"),
      cite("spec/02-grammar.md", "lex.widget.one", false, "02-grammar.md"),
      cite("guide/TOUR.md", "lex.widget.old"),
      cite("spec/README.md", "lex.widget.old", true),
      cite("src/x.ts", "lex.widget.old"),
      cite("future-work/X.md", "lex.gone.away"),
    ],
  };
  const dead = deadCitations(index);
  assert.deepEqual(
    dead.map((c) => [c.file, c.failing, c.history]),
    [
      ["spec/02-grammar.md", true, false],
      ["guide/TOUR.md", true, false],
      ["spec/README.md", false, true],
      ["src/x.ts", false, false],
      ["future-work/X.md", false, false],
    ],
  );
  assert.match(dead[0]!.reason, /rule is in spec\/01-lexical-structure.md/);
  assert.match(dead[1]!.reason, /retired in batch 7/);
  assert.match(dead[4]!.reason, /retired in batch 7/);
});

test("the spec CLI reports usage errors and the real language tier", () => {
  assert.throws(() => run(["nope"]), /unknown command/);
  assert.throws(() => run(["counts", "--by", "nope"]), /unknown counts view/);
  const { status, stdout } = run(["counts", "--json"]);
  assert.equal(status, 0);
  const parsed = JSON.parse(stdout) as { tiers: { language: number }; total: number };
  assert.ok(parsed.tiers.language > 0 && parsed.total >= parsed.tiers.language);
  const refs = run(["refs", "#r-data.embed.width"], { spec: SPEC_ROOT, repo: REPO_ROOT });
  assert.match(refs.stdout, /^data\.embed\.width: defined at spec\/08-data-and-enums\.md:\d+/);
});

test("the spec tools import only Node built-ins and spec/", async () => {
  const tools = resolve(SPEC_ROOT, "tools");
  for (const name of (await readdir(tools)).filter((n) => /^spec.*\.ts$/.test(n))) {
    const text = await readFile(resolve(tools, name), "utf8");
    for (const match of text.matchAll(/^import [^"]*"([^"]+)"/gm)) {
      const specifier = match[1]!;
      assert.ok(
        specifier.startsWith("node:") || /^\.\/[\w-]+\.ts$/.test(specifier),
        `${name} imports ${specifier}`,
      );
    }
  }
});
