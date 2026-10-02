import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
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
  ruleHistory,
} from "../spec/tools/spec-refs.ts";
import {
  glossary,
  glossaryMarkdown,
  glossaryReport,
  rebaseLinks,
  termKeys,
} from "../spec/tools/spec-glossary.ts";
import { failures, rewrite, rewriteSummary } from "../spec/tools/spec-rewrite.ts";
import { run } from "../spec/tools/spec.ts";

/** Whether this checkout's history holds `rev`; a shallow clone may not. */
function hasCommit(rev: string): boolean {
  try {
    execFileSync("git", ["-C", REPO_ROOT, "cat-file", "-e", `${rev}^{commit}`], {
      stdio: "ignore",
    });
    return true;
  } catch {
    return false;
  }
}

// A miniature specification: one language chapter, one stdlib chapter, and
// the README tables the tools read.
const README = `# Spec

### Diagnostics

| Severity | Stable diagnostic codes |
| --- | --- |
| Error | \`bad-thing\`, \`never-named\`, \`syntax-error\` |
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
    await mkdir(join(spec, "lang"), { recursive: true });
    await writeFile(join(spec, "README.md"), README);
    await writeFile(join(spec, "lang", "01-lexical-structure.md"), LEXICAL);
    await writeFile(join(spec, "std", "README.md"), "# Std\n");
    await writeFile(join(spec, "std", "iter.md"), STD_ITER);
    await run(spec);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("counts rules per chapter, prefix, tier, and heuristic kind", async () => {
  await withSpec(async (spec) => {
    assert.deepEqual(chapterNames(spec), ["lang/01-lexical-structure.md", "std/iter.md"]);
    const result = counts(loadCorpus(spec));
    assert.deepEqual(result.tiers, { language: 5, std: 1, cli: 0 });
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
    await readFile(resolve(SPEC_ROOT, "lang", "06-control-flow.md"), "utf8"),
  );
  const numbered = (await readdir(resolve(SPEC_ROOT, "lang")))
    .filter((name) => /^\d\d-.*\.md$/.test(name))
    .map((name) => `lang/${name}`);
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
    "/* spec/lang/08-data-and-enums.md#r-data.embed.width */",
    'const url = "#r-data.embed.width";',
  ].join("\n");
  assert.deepEqual(
    citationsIn("src/x.ts", source, "src", isRuleId).map((c) => [c.line, c.form]),
    [
      [1, "id"],
      [2, "anchor"],
    ],
  );
  const hd = "# (spec/lang/08-data-and-enums.md#r-data.embed.width)\nx := data.embed.width\n";
  assert.equal(citationsIn("lib/std/x.hd", hd, "lib-std", isRuleId).length, 1);
});

test("refs reads retired IDs and the commit that removed each from git history", async () => {
  const repo = await mkdtemp(join(tmpdir(), "spec-refs-"));
  try {
    const config = ["user.name=t", "user.email=t@example.com", "commit.gpgsign=false"];
    const git = (...args: string[]): string =>
      execFileSync("git", ["-C", repo, ...config.flatMap((c) => ["-c", c]), ...args], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      });
    const chapter = join(repo, "spec", "lang", "01-lexical-structure.md");
    await mkdir(join(repo, "spec", "lang"), { recursive: true });
    git("init", "-q");
    await writeFile(chapter, "1. r[lex.widget.old] Old.\n2. r[lex.widget.kept] Kept.\n");
    git("add", ".");
    git("commit", "-q", "-m", "Add widgets");
    await writeFile(chapter, "1. r[lex.widget.kept] Kept, reworded.\n");
    git("commit", "-q", "-am", "Retire the old widget");
    const history = ruleHistory(repo);
    assert.deepEqual([...history.ids].sort(), ["lex.widget.kept", "lex.widget.old"]);
    assert.match(history.removedIn.get("lex.widget.old")!, /^[0-9a-f]+ "Retire the old widget"$/);
    assert.ok(!history.removedIn.has("lex.widget.kept"), "a reworded rule is not removed");
  } finally {
    await rm(repo, { recursive: true, force: true });
  }
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
    live: new Map([["lex.widget.one", "spec/lang/01-lexical-structure.md:5"]]),
    history: {
      ids: new Set(["lex.widget.old", "lex.gone.away"]),
      removedIn: new Map([["lex.widget.old", 'abc1234 "Retire widgets"']]),
    },
    citations: [
      cite("spec/lang/02-grammar.md", "lex.widget.one", false, "01-lexical-structure.md"),
      cite("spec/lang/02-grammar.md", "lex.widget.one", false, "02-grammar.md"),
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
      ["spec/lang/02-grammar.md", true, false],
      ["guide/TOUR.md", true, false],
      ["spec/README.md", false, true],
      ["src/x.ts", false, false],
      ["future-work/X.md", false, false],
    ],
  );
  assert.match(dead[0]!.reason, /rule is in spec\/lang\/01-lexical-structure.md/);
  assert.match(dead[1]!.reason, /retired in commit abc1234 "Retire widgets"/);
  assert.equal(dead[4]!.reason, "retired (in git history)");
});

test("the spec CLI reports usage errors and the real language tier", () => {
  assert.throws(() => run(["nope"]), /unknown command/);
  assert.throws(() => run(["counts", "--by", "nope"]), /unknown counts view/);
  const { status, stdout } = run(["counts", "--json"]);
  assert.equal(status, 0);
  const parsed = JSON.parse(stdout) as { tiers: { language: number }; total: number };
  assert.ok(parsed.tiers.language > 0 && parsed.total >= parsed.tiers.language);
  const refs = run(["refs", "#r-data.embed.width"], { spec: SPEC_ROOT, repo: REPO_ROOT });
  assert.match(
    refs.stdout,
    /^data\.embed\.width: defined at spec\/lang\/08-data-and-enums\.md:\d+/,
  );
});

// A rewrite of LEXICAL: one rule retired, one added, one reworded, a code
// dropped, one example moved to the stdlib chapter, and one lost.
const LEXICAL_AFTER = `# Lexical Structure

## Widgets

1. r[lex.widget.one] A widget must be blue.
2. r[lex.widget.two] A widget may be round and must sit on the right of the gadget line.
3. r[lex.widget.four] A widget has four corners.

\`\`\`text
widget
\`\`\`

## Gadgets

1. r[lex.gadget.one] A widget may be round and must sit on the left of the gadget line.
2. r[lex.gadget.two] A gadget is bad. Error: \`unlisted-code\`.

\`\`\`text
gadget
\`\`\`
`;

const LEXICAL_BASE = `${LEXICAL}
\`\`\`text
gadget
\`\`\`

\`\`\`text
moved to iter
\`\`\`

\`\`\`text
dropped for good
\`\`\`
`;

const STD_ITER_AFTER = `${STD_ITER}
\`\`\`text
moved to iter
\`\`\`
`;

async function withSpecs(
  files: Record<string, string>,
  run: (spec: string) => Promise<void> | void,
): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), "hd-spec-tools-"));
  try {
    const spec = join(root, "spec");
    await mkdir(join(spec, "std"), { recursive: true });
    await mkdir(join(spec, "lang"), { recursive: true });
    for (const [name, text] of Object.entries(files)) await writeFile(join(spec, name), text);
    await run(spec);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("rewrite reports counts, IDs, codes, and examples between two versions", async () => {
  const shared = { "README.md": README, "std/README.md": "# Std\n" };
  await withSpecs(
    {
      ...shared,
      "lang/01-lexical-structure.md": LEXICAL_BASE,
      "std/iter.md": STD_ITER,
    },
    async (baseSpec) =>
      withSpecs(
        {
          ...shared,
          "lang/01-lexical-structure.md": LEXICAL_AFTER,
          "std/iter.md": STD_ITER_AFTER,
        },
        (headSpec) => {
          const result = rewrite({
            base: loadCorpus(baseSpec),
            head: loadCorpus(headSpec),
            baseLabel: "base",
            headLabel: "head",
            earlierIds: new Set(["lex.widget.four"]),
          });
          assert.deepEqual(result.tiers.language, {
            before: 5,
            after: 5,
            delta: 0,
          });
          assert.deepEqual(result.retired, [
            { id: "lex.widget.three", chapter: "lang/01-lexical-structure.md" },
          ]);
          assert.deepEqual(
            result.added.map((entry) => entry.id),
            ["lex.widget.four"],
          );
          assert.deepEqual(result.revived, ["lex.widget.four"]);
          assert.deepEqual(
            result.changed.map((entry) => entry.id),
            ["lex.widget.one", "lex.widget.two"],
          );
          assert.deepEqual(result.lostCodes, ["bad-thing"]);
          assert.equal(result.examples.statuses.unchanged, 3, "widget, gadget, and xs.map(f)");
          assert.equal(result.examples.statuses.moved, 1);
          assert.deepEqual(
            result.examples.lost.map((entry) => [entry.chapter, entry.index, entry.missing]),
            [["lang/01-lexical-structure.md", 4, ["dropped for good"]]],
          );
          assert.deepEqual(failures(result, ["lost-codes", "lost-examples", "reused-ids"]), [
            "lost-codes",
            "lost-examples",
            "reused-ids",
          ]);
          assert.match(
            rewriteSummary(result),
            /^rules: language 5 -> 5 \(0\), stdlib 1 -> 1 \(0\), cli 0 -> 0 \(0\); IDs: 1 retired, 1 added, 0 moved, 2 changed text, 1 reused; codes: 1 lost/,
          );
        },
      ),
  );
});

test("rewrite reads batch 34 from git history", { skip: !hasCommit("42090f42") }, () => {
  const { status, stdout } = run([
    "rewrite",
    "42090f42",
    "5125d42b",
    "--fail-on",
    "lost-codes,reused-ids",
  ]);
  assert.equal(status, 0);
  assert.match(stdout, /Language tier: 3,614 -> 3,624 \(\+10\)/);
  assert.match(
    stdout,
    /Retired IDs \(2\): `fn\.vararg\.collect\.tuple`, `fn\.vararg\.collect\.tuple\.rest`/,
  );
  assert.match(stdout, /Added IDs \(12\): /);
  assert.match(stdout, /lost: lang\/02-grammar\.md example \d+/);
  const failing = run(["rewrite", "42090f42", "5125d42b", "--fail-on", "lost-examples", "--json"]);
  assert.equal(failing.status, 1);
  assert.deepEqual((JSON.parse(failing.stdout) as { failed: string[] }).failed, ["lost-examples"]);
  assert.throws(() => run(["rewrite", "42090f42", "--fail-on", "nope"]), /unknown --fail-on kind/);
});

const GLOSSARY_README = `# Spec

## Glossary

| Term | Definition |
| --- | --- |
| **widget** | A blue thing. See [\`lex.widget.one\`](lang/01-lexical-structure.md#r-lex.widget.one). |
| **map adapter** | A stdlib term, in the [Standard Library glossary](std/README.md#glossary). |
`;

const GLOSSARY_STD_README = `# Std

## Glossary

| Term | Definition |
| --- | --- |
| **map adapter** | A lazy \`map\`. See [Adapters](iter.md#adapters). |
`;

const GLOSSARY_LEXICAL = `# Lexical Structure

## Widgets

1. r[lex.widget.one] A **widget** must be blue.

A **gadget** sits beside a widget. It **must** be green.

> **Note.** A **callout term** is not a definition.

## Sprockets

A sprocket is a small gear.
`;

test("glossary uses the hand-written glossaries and reports chapter terms they lack", async () => {
  await withSpecs(
    {
      "README.md": GLOSSARY_README,
      "std/README.md": GLOSSARY_STD_README,
      "lang/01-lexical-structure.md": GLOSSARY_LEXICAL,
      "std/iter.md": STD_ITER,
    },
    (spec) => {
      const result = glossary(loadCorpus(spec));
      assert.ok(result.handWritten);
      assert.deepEqual(
        result.terms.map((entry) => [entry.term, entry.source, entry.chapter, entry.anchor]),
        [
          ["gadget", "chapter-bold", "lang/01-lexical-structure.md", "widgets"],
          ["map adapter", "glossary", "std/iter.md", "adapters"],
          ["sprockets", "chapter-heading", "lang/01-lexical-structure.md", "sprockets"],
          ["widget", "glossary", "lang/01-lexical-structure.md", "r-lex.widget.one"],
        ],
      );
      assert.deepEqual(
        result.missing.map((entry) => entry.term),
        ["gadget", "sprockets"],
      );
      const widget = result.terms.find((entry) => entry.term === "widget")!;
      assert.equal(widget.definition, "A blue thing.");
      assert.equal(widget.rule, "lex.widget.one");
      const gadget = result.terms.find((entry) => entry.term === "gadget")!;
      assert.equal(gadget.definition, "A **gadget** sits beside a widget.");
      const page = glossaryMarkdown(result);
      assert.match(
        page,
        /\| \[\*\*widget\*\*\]\(lang\/01-lexical-structure\.md#r-lex\.widget\.one\) \| A blue thing\. \|/,
      );
      assert.match(page, /^## M$/m);
      assert.match(glossaryReport(result), /missing from the hand-written glossaries: 2/);
    },
  );
});

test("glossary helpers rebase links and match plurals", () => {
  assert.equal(
    rebaseLinks("[a](iter.md#x) [b](../README.md#y)", "std"),
    "[a](std/iter.md#x) [b](README.md#y)",
  );
  assert.equal(rebaseLinks("[a](https://e.com/x.md)", "std"), "[a](https://e.com/x.md)");
  assert.ok(termKeys("trait candidates").includes("trait candidate"));
  assert.ok(termKeys("mutable edges").includes("mutable edge"));
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
