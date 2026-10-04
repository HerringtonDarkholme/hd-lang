import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { diagnosticFix, type JsonDiagnostic } from "../src/diagnostic-report.ts";
import type { Diagnostic } from "../src/diagnostics.ts";
import { explainCode, indexSpec, loadSpecIndex, namedCodes } from "../src/spec-index.ts";
import { headerAt, type SymbolInfo } from "../src/symbols.ts";
import { runHd } from "./hd-in-process.ts";

interface CommandResult {
  readonly code: number;
  readonly stdout: string;
  readonly stderr: string;
}

const root = resolve(import.meta.dirname, "..");

/**
 * Runs `hd ARGS...` in this process from the repository root, and resolves
 * with its exit code instead of rejecting. `specDir` stands in for `HD_SPEC_DIR`.
 */
async function hd(args: readonly string[], specDir?: string): Promise<CommandResult> {
  const { status, stdout, stderr } = await runHd(args, { cwd: root, specDir });
  return { code: status, stdout, stderr };
}

function jsonLines(text: string): JsonDiagnostic[] {
  return text
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line) as JsonDiagnostic);
}

async function withDirectory(run: (directory: string) => Promise<void>): Promise<void> {
  const directory = await mkdtemp(join(tmpdir(), "hd-lang-agent-"));
  try {
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

// A miniature specification in the restyled form: rule ID markers on list
// items and table cells, "Error: `code`." endings, and the README tables.
const SPEC_README = `# Spec

### Diagnostics

| Severity | Stable diagnostic codes |
| --- | --- |
| Error | \`duplicate-field\`, \`old-struct-declaration\`, \`mutable-embedded-field\` |
| Error (general) | \`unknown-name\`, \`unknown-data-field\` |
| Warning | \`unused-local-binding\` |

| Code | Meaning |
| --- | --- |
| \`unknown-name\` | A value name resolves to no binding in scope. |
| \`unknown-data-field\` | A literal names a field the type does not declare. |
`;

const SPEC_CHAPTER = `# Data

## Data Declarations

1. r[data.field.unique] Field names must be unique within the data type. Error: \`duplicate-field\`.
2. r[data.field.embedded-no-mut] An embedded field is written without \`mut\`. Error: \`mutable-embedded-field\`.
3. r[data.decl.no-struct] \`struct Name:\` must be diagnosed. Error: \`old-struct-declaration\`.

\`\`\`text
data User:  # error: duplicate-field
\`\`\`

## Construction

1. r[data.literal.unknown] An unknown field is a compile-time error. Error: \`unknown-data-field\`.

| Form | Rule |
| --- | --- |
| variant | r[data.enum.construct.unknown] A named argument naming no payload field is a \`unknown-data-field\` error. |

## Construction

Unknown fields report \`unknown-data-field\` here too.
`;

const SPEC_FILES = {
  "README.md": SPEC_README,
  "lang/08-data-and-enums.md": SPEC_CHAPTER,
  "conformance/cases.tsv":
    "path\tphase\texpectation\tspecification\n" +
    "typing/invalid/duplicate-field.hd\ttype\treject:duplicate-field\tlang/08-data-and-enums.md#r-data.field.unique\n" +
    "typing/valid/ok.hd\ttype\taccept\tlang/08-data-and-enums.md#data-declarations\n",
};

async function writeSpec(directory: string): Promise<string> {
  const spec = join(directory, "spec");
  await mkdir(join(spec, "conformance"), { recursive: true });
  await mkdir(join(spec, "lang"), { recursive: true });
  for (const [path, text] of Object.entries(SPEC_FILES)) await writeFile(join(spec, path), text);
  return spec;
}

test("the spec index maps rule ID markers to the codes they name", () => {
  const index = indexSpec(SPEC_FILES);
  assert.deepEqual(
    index.rules.map((rule) => rule.id),
    [
      "data.field.unique",
      "data.field.embedded-no-mut",
      "data.decl.no-struct",
      "data.literal.unknown",
      "data.enum.construct.unknown",
    ],
  );
  assert.deepEqual(
    index.rulesByCode.get("unknown-data-field")?.map((rule) => rule.anchor),
    [
      "spec/lang/08-data-and-enums.md#r-data.literal.unknown",
      "spec/lang/08-data-and-enums.md#r-data.enum.construct.unknown",
    ],
  );
  // `mut` in inline code is not a diagnostic the rule names.
  assert.deepEqual(index.rulesByCode.get("mutable-embedded-field")?.[0]?.codes, [
    "mutable-embedded-field",
  ]);
  assert.equal(index.codes.get("unknown-name")?.category, "error (general)");
  assert.equal(index.codes.get("unused-local-binding")?.category, "warning");
  assert.equal(
    index.codes.get("unknown-name")?.meaning,
    "A value name resolves to no binding in scope.",
  );
  const mentions = index.mentions.get("unknown-data-field") ?? [];
  assert.deepEqual(
    mentions.map((mention) => [mention.anchor, mention.rule ?? null]),
    [
      ["spec/lang/08-data-and-enums.md#construction", "data.literal.unknown"],
      ["spec/lang/08-data-and-enums.md#construction", "data.enum.construct.unknown"],
      ["spec/lang/08-data-and-enums.md#construction-1", null],
    ],
  );
  // Error examples in code fences are not prose mentions.
  assert.equal(
    index.mentions.get("duplicate-field")?.every((mention) => mention.rule),
    true,
  );
  assert.deepEqual(explainCode(index, "duplicate-field")?.fixtures, [
    {
      path: "spec/conformance/typing/invalid/duplicate-field.hd",
      phase: "type",
      expectation: "reject:duplicate-field",
      specification: "spec/lang/08-data-and-enums.md#r-data.field.unique",
    },
  ]);
  assert.equal(explainCode(index, "no-such-code"), undefined);
});

test("rule text names codes with Error:, Warning:, and is-a-code-error forms", () => {
  assert.deepEqual(namedCodes("It is an error. Error: `a-code`."), ["a-code"]);
  assert.deepEqual(namedCodes("Either form is rejected. Error: `first`, or `second`."), [
    "first",
    "second",
  ]);
  assert.deepEqual(namedCodes("Such a binding is a `unused-thing` warning."), ["unused-thing"]);
  assert.deepEqual(namedCodes("Writing `mut` here changes nothing."), []);
});

test("the real specification indexes every stable code", async () => {
  const index = await loadSpecIndex();
  const readme = await readFile(resolve(root, "spec/README.md"), "utf8");
  for (const line of readme.split("\n")) {
    const row = /^\| (Error[^|]*|Warning|Boundary failure) \| (.*) \|$/.exec(line);
    if (!row) continue;
    for (const match of row[2]!.matchAll(/`([a-z0-9-]+)`/g))
      assert.ok(explainCode(index, match[1]!), `hd explain knows ${match[1]}`);
  }
  assert.equal(index.codes.get("integer-overflow")?.category, "runtime panic");
  assert.ok(explainCode(index, "type-mismatch")?.meaning);
  for (const rule of index.rules)
    assert.match(rule.anchor, /^spec\/(?:lang\/\d\d-|std\/)[^#]+\.md#r-[a-z]/);
  // Restyled chapters carry rule IDs; the scanner must keep finding the codes they name.
  if (index.rules.length > 0) assert.ok(index.rulesByCode.size > 0);
});

function diagnosticAt(code: string, source: string, text: string): Diagnostic {
  const offset = source.indexOf(text);
  const start = { offset, line: 1, column: offset + 1 };
  const end = { offset: offset + text.length, line: 1, column: offset + text.length + 1 };
  return { code, message: "", span: { start, end } };
}

test("suggested fixes come only from codes whose message names the replacement", () => {
  const struct = "struct P:";
  assert.deepEqual(
    diagnosticFix(diagnosticAt("old-struct-declaration", struct, "struct"), struct),
    {
      message: "replace 'struct' with 'data'",
      edits: [{ span: diagnosticAt("", struct, "struct").span, replacement: "data" }],
    },
  );
  const exported = "export a.b";
  assert.equal(
    diagnosticFix(diagnosticAt("old-export-declaration", exported, "export"), exported)?.edits[0]
      ?.replacement,
    "pub use",
  );
  const binding = "x: i32 = 1";
  const insertion = diagnosticFix(diagnosticAt("missing-let", binding, "x"), binding);
  assert.equal(insertion?.edits[0]?.replacement, "let ");
  assert.equal(insertion?.edits[0]?.span.end.offset, 0);
  // A span that no longer covers the named token yields no fix.
  assert.equal(
    diagnosticFix(diagnosticAt("old-struct-declaration", struct, "P"), struct),
    undefined,
  );
  assert.equal(diagnosticFix(diagnosticAt("unknown-name", struct, "P"), struct), undefined);
});

test("check --format json writes one JSON diagnostic per stderr line", async () => {
  await withDirectory(async (directory) => {
    const spec = await writeSpec(directory);
    const file = join(directory, "legacy.hd");
    const source = "struct Point:\n    x: i32\n";
    await writeFile(file, source);

    const text = await hd(["check", file]);
    assert.equal(text.code, 1);
    assert.equal(
      text.stderr.trim(),
      `${file}:1:1: old-struct-declaration: 'struct' was replaced by 'data'`,
    );

    const json = await hd(["check", "--format", "json", file], spec);
    assert.equal(json.code, 1);
    assert.equal(json.stdout, "");
    const [diagnostic, ...rest] = jsonLines(json.stderr);
    assert.equal(rest.length, 0);
    assert.deepEqual(diagnostic, {
      kind: "diagnostic",
      code: "old-struct-declaration",
      severity: "error",
      message: "'struct' was replaced by 'data'",
      file,
      span: {
        start: { line: 1, column: 1, offset: 0 },
        end: { line: 1, column: 7, offset: 6 },
      },
      notes: [],
      related: [],
      fix: {
        message: "replace 'struct' with 'data'",
        edits: [
          {
            span: {
              start: { line: 1, column: 1, offset: 0 },
              end: { line: 1, column: 7, offset: 6 },
            },
            replacement: "data",
          },
        ],
      },
      rule: "data.decl.no-struct",
      rules: [
        {
          id: "data.decl.no-struct",
          anchor: "spec/lang/08-data-and-enums.md#r-data.decl.no-struct",
        },
      ],
    });

    // Applying the suggested edit resolves the diagnostic.
    const edit = diagnostic!.fix!.edits[0]!;
    await writeFile(
      file,
      source.slice(0, edit.span.start.offset) +
        edit.replacement +
        source.slice(edit.span.end.offset),
    );
    const fixed = await hd(["check", "--format", "json", file]);
    assert.equal(fixed.code, 0);
    assert.equal(fixed.stderr, "");
    assert.match(fixed.stdout, /legacy\.hd: ok/);
  });
});

test("JSON diagnostics cover warnings, several codes, and runtime panics", async () => {
  await withDirectory(async (directory) => {
    const spec = await writeSpec(directory);
    const warned = join(directory, "warned.hd");
    await writeFile(warned, "pub fn main() -> void:\n    unused := 1\n");
    const check = await hd(["check", "--format", "json", warned], spec);
    assert.equal(check.code, 0);
    const [warning] = jsonLines(check.stderr);
    assert.equal(warning?.code, "unused-local-binding");
    assert.equal(warning?.severity, "warning");
    assert.equal(warning?.span?.start.line, 2);
    assert.equal(warning?.rule, null);
    assert.deepEqual(warning?.rules, []);

    const ambiguous = join(directory, "ambiguous.hd");
    await writeFile(
      ambiguous,
      "data P:\n    x: i32\n\npub fn main() -> void:\n    _ := P { x: 1, y: 2 }\n",
    );
    const unknown = jsonLines((await hd(["check", "--format", "json", ambiguous], spec)).stderr);
    assert.equal(unknown[0]?.code, "unknown-data-field");
    // Two rules name the code, so no single rule is chosen.
    assert.equal(unknown[0]?.rule, null);
    assert.equal(unknown[0]?.rules.length, 2);

    const panics = join(directory, "panics.hd");
    await writeFile(
      panics,
      'pub fn main() -> void $ Console:\n    x := 1 / 0\n    println("$x")\n',
    );
    const run = await hd(["run", "--format", "json", panics]);
    assert.equal(run.code, 1);
    assert.deepEqual(jsonLines(run.stderr), [
      {
        kind: "runtime-panic",
        code: "integer-division-by-zero",
        severity: "error",
        message: "runtime panic",
        file: panics,
        span: null,
        notes: [],
        related: [],
        fix: null,
        rule: null,
        rules: [],
      },
    ]);
    const plain = await hd(["run", panics]);
    assert.equal(plain.stderr.trim(), "integer-division-by-zero: runtime panic");

    const tested = await hd(["test", "--format", "json", warned]);
    assert.equal(tested.code, 0);
    assert.match(tested.stdout, /warned\.hd: 1 passed/);

    const bad = await hd(["check", "--format", "yaml", warned]);
    assert.equal(bad.code, 101);
  });
});

test("hd explain prints a code's meaning, rules, mentions, and fixtures", async () => {
  await withDirectory(async (directory) => {
    const spec = await writeSpec(directory);
    const text = await hd(["explain", "unknown-data-field"], spec);
    assert.equal(text.code, 0);
    assert.equal(
      text.stdout,
      [
        "unknown-data-field: error (general)",
        "  A literal names a field the type does not declare.",
        "  (spec/README.md#diagnostics)",
        "",
        "rules:",
        "  spec/lang/08-data-and-enums.md#r-data.literal.unknown",
        "    An unknown field is a compile-time error. Error: `unknown-data-field`.",
        "  spec/lang/08-data-and-enums.md#r-data.enum.construct.unknown",
        "    A named argument naming no payload field is a `unknown-data-field` error.",
        "",
        "mentioned in:",
        "  spec/lang/08-data-and-enums.md#construction (data.literal.unknown)  line 15",
        "  spec/lang/08-data-and-enums.md#construction (data.enum.construct.unknown)  line 19",
        "  spec/lang/08-data-and-enums.md#construction-1  line 23",
        "",
      ].join("\n"),
    );

    const json = await hd(["explain", "--format", "json", "duplicate-field"], spec);
    assert.equal(json.code, 0);
    assert.deepEqual(JSON.parse(json.stdout), {
      code: "duplicate-field",
      known: true,
      category: "error",
      meaning: null,
      meaningSource: null,
      rules: [
        {
          id: "data.field.unique",
          anchor: "spec/lang/08-data-and-enums.md#r-data.field.unique",
          file: "spec/lang/08-data-and-enums.md",
          line: 5,
          text: "Field names must be unique within the data type. Error: `duplicate-field`.",
        },
      ],
      mentions: [
        {
          anchor: "spec/lang/08-data-and-enums.md#data-declarations",
          heading: "Data Declarations",
          file: "spec/lang/08-data-and-enums.md",
          line: 5,
          rule: "data.field.unique",
        },
      ],
      fixtures: [
        {
          path: "spec/conformance/typing/invalid/duplicate-field.hd",
          phase: "type",
          expectation: "reject:duplicate-field",
          specification: "spec/lang/08-data-and-enums.md#r-data.field.unique",
        },
      ],
    });

    const unknown = await hd(["explain", "no-such-code"], spec);
    assert.equal(unknown.code, 1);
    assert.match(unknown.stderr, /names no diagnostic code 'no-such-code'/);
    const unknownJson = await hd(["explain", "--format", "json", "no-such-code"]);
    assert.equal(unknownJson.code, 1);
    assert.deepEqual(JSON.parse(unknownJson.stdout), { code: "no-such-code", known: false });

    // Against the repository's own specification.
    const real = await hd(["explain", "type-mismatch"]);
    assert.equal(real.code, 0);
    assert.match(real.stdout, /^type-mismatch: error \(general\)\n {2}A value's type/);
    assert.match(real.stdout, /fixtures:\n {2}spec\/conformance\//);
  });
});

test("declaration headers drop bodies, comments, and trailing commas", () => {
  const source = "fn read(\n    ## Point to inspect.\n    point: Point,\n) -> i32: point.x\n";
  assert.equal(headerAt(source, 0), "fn read(point: Point) -> i32");
  assert.equal(headerAt("let limit: i32 = 3\n", 0, "line"), "let limit: i32 = 3");
  assert.equal(
    headerAt('fn greet[T < Display](x: T, sep: string = ": ") -> string $ Console:\n', 0),
    'fn greet[T < Display](x: T, sep: string = ": ") -> string $ Console',
  );
});

const USER_MODULE = `## A registered user.
pub data User:
    ## The login address.
    pub email: string
    name: string = "anon"

impl User:
    ## Greets another user.
    pub fn greet(self, other: string) -> string: "hi $other"
    fn guest() -> User: User { email: "guest" }

## Something that can be shown.
pub trait Show:
    type Output
    ## Renders the value.
    fn show(self) -> string
    fn twice(self) -> string: self.show()

impl Show for User:
    type Output = string
    fn show(self) -> string: self.email

pub enum Status:
    Active
    ## Blocked by a moderator.
    Banned(reason: string)
`;

async function writePackage(directory: string): Promise<string> {
  const project = join(directory, "project");
  await mkdir(join(project, "src/user"), { recursive: true });
  await writeFile(
    join(project, "src/mod.hd"),
    'pub use self.user.{User}\n\n## Entry point.\npub fn main() -> void $ Console:\n    println("hi")\n',
  );
  await writeFile(join(project, "src/user/mod.hd"), USER_MODULE);
  return project;
}

interface LookupOutput {
  readonly query: string;
  readonly symbols: readonly SymbolInfo[];
  readonly suggestions: readonly string[];
}

async function lookup(command: string, name: string, target: string): Promise<LookupOutput> {
  const result = await hd([command, "--format", "json", name, target]);
  return JSON.parse(result.stdout) as LookupOutput;
}

test("hd def and hd doc resolve package symbols by qualified name", async () => {
  await withDirectory(async (directory) => {
    const project = await writePackage(directory);
    const userFile = join(project, "src/user/mod.hd");

    const def = await hd(["def", "pkg.user.User", project]);
    assert.equal(def.code, 0);
    assert.equal(def.stdout, `${userFile}:2:1: data pkg.user.User\n  pub data User\n`);

    const [user] = (await lookup("doc", "pkg.user.User", project)).symbols;
    assert.equal(user?.kind, "data");
    assert.equal(user?.module, "user");
    assert.equal(user?.doc, "A registered user.");
    assert.deepEqual(user?.span, {
      start: { line: 2, column: 1, offset: 22 },
      end: { line: 5, column: 26, offset: 110 },
    });
    assert.deepEqual(
      user?.members?.map((member) => [member.name, member.kind, member.signature, member.trait]),
      [
        ["pkg.user.User.email", "field", "pub email: string", undefined],
        ["pkg.user.User.name", "field", 'name: string = "anon"', undefined],
        ["pkg.user.User.greet", "method", "pub fn greet(self, other: string) -> string", undefined],
        ["pkg.user.User.guest", "associated-function", "fn guest() -> User", undefined],
        ["pkg.user.User.show", "method", "fn show(self) -> string", "Show"],
      ],
    );
    assert.deepEqual(
      user?.implementations?.map((implementation) => [
        implementation.trait,
        implementation.span.start.line,
      ]),
      [["Show", 19]],
    );

    // The same symbol unqualified, through the root re-export, and by member path.
    assert.equal((await lookup("def", "User", project)).symbols[0]?.name, "pkg.user.User");
    const reexported = (await lookup("def", "pkg.User", project)).symbols[0];
    assert.equal(reexported?.name, "pkg.user.User");
    assert.equal(reexported?.via, "pkg.User");
    const email = (await lookup("def", "pkg.user.User.email", project)).symbols;
    assert.deepEqual(
      email.map((symbol) => [symbol.kind, symbol.type, symbol.doc, symbol.owner]),
      [["field", "string", "The login address.", "pkg.user.User"]],
    );
    const guest = (await lookup("def", "User::guest", project)).symbols[0];
    assert.equal(guest?.kind, "associated-function");
    const reason = (await lookup("def", "Status.Banned.reason", project)).symbols[0];
    assert.equal(reason?.signature, "reason: string");
    const banned = (await lookup("doc", "Status.Banned", project)).symbols[0];
    assert.equal(banned?.doc, "Blocked by a moderator.");
    assert.equal(banned?.signature, "Banned(reason: string)");

    const show = (await lookup("doc", "pkg.user.Show", project)).symbols[0];
    assert.deepEqual(
      show?.members?.map((member) => [member.name, member.kind, member.hasDefault ?? null]),
      [
        ["pkg.user.Show.Output", "associated-type", null],
        ["pkg.user.Show.show", "method", false],
        ["pkg.user.Show.twice", "method", true],
      ],
    );
    assert.deepEqual(
      show?.implementations?.map((implementation) => implementation.target),
      ["User"],
    );
    const traitMethod = (await lookup("def", "Show.show", project)).symbols[0];
    assert.equal(traitMethod?.doc, "Renders the value.");
    assert.equal(traitMethod?.trait, "Show");

    const main = (await lookup("doc", "pkg.main", project)).symbols[0];
    assert.equal(main?.file, join(project, "src/mod.hd"));
    assert.deepEqual(main?.requirements, ["Console"]);
    assert.equal(main?.result, "void");

    const doc = await hd(["doc", "Show", project]);
    assert.equal(doc.code, 0);
    assert.match(doc.stdout, /^trait pkg\.user\.Show\n/);
    assert.match(doc.stdout, /\nSomething that can be shown\.\n/);
    assert.match(doc.stdout, /\n {2}fn twice\(self\) -> string {2}\[default\]/);
    assert.match(doc.stdout, /\nimplementations:\n {2}User {2}\(/);

    const missing = await hd(["def", "show", project]);
    assert.equal(missing.code, 1);
    assert.equal(
      missing.stderr.trim(),
      "hd def: no symbol named show; candidates: pkg.user.Show.show, pkg.user.User.show",
    );
    const missingJson = await hd(["def", "--format", "json", "pkg.nothing.User", project]);
    assert.equal(missingJson.code, 1);
    assert.deepEqual(JSON.parse(missingJson.stdout).symbols, []);
  });
});

test("hd doc on a single file reports inferred types", async () => {
  await withDirectory(async (directory) => {
    const file = join(directory, "single.hd");
    await writeFile(file, "## Doubles its input.\nfn twice(x: i32): x * 2\nlimit := twice(2)\n");
    const doc = await hd(["doc", "twice", file]);
    assert.equal(
      doc.stdout,
      [
        "function twice",
        `  ${file}:2:1`,
        "",
        "fn twice(x: i32)",
        "",
        "Doubles its input.",
        "",
        "result: i32 (inferred)",
        "requirements: $() (inferred)",
        "",
      ].join("\n"),
    );
    const limit = (await lookup("doc", "limit", file)).symbols[0];
    assert.deepEqual([limit?.kind, limit?.type, limit?.omitted], ["binding", "i32", ["type"]]);

    // Lookups read parsed modules, so a program that does not type-check still answers.
    await writeFile(file, 'fn broken() -> i32: "text"\n');
    const broken = (await lookup("def", "broken", file)).symbols[0];
    assert.equal(broken?.result, "i32");
    // A parse failure reports its diagnostics and finds nothing.
    await writeFile(file, "fn (\n");
    const failed = await hd(["def", "--format", "json", "broken", file]);
    assert.equal(failed.code, 1);
    assert.equal(jsonLines(failed.stderr)[0]?.kind, "diagnostic");
  });
});
