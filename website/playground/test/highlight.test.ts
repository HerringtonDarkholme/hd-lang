import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import { highlightTree } from "@lezer/highlight";

import { classify, type TokenClass } from "../../../src/highlight.ts";
import { cssClass, hdHighlighter, hdStreamLanguage, type StyledClass } from "../src/hd-language.ts";

const root = resolve(import.meta.dirname, "../../..");

type Run = readonly [text: string, className: string];

/** The class runs CodeMirror's highlighter gives each line of `source`. */
function editorRuns(source: string): Run[][] {
  const tree = hdStreamLanguage.parser.parse(source);
  const classes = Array.from<string>({ length: source.length }).fill("");
  highlightTree(tree, hdHighlighter, (from, to, className) => {
    for (let index = from; index < to; index += 1) classes[index] = className;
  });
  let offset = 0;
  return source.split("\n").map((line) => {
    const runs = mergeRuns([...line].map((_, index) => [line[index]!, classes[offset + index]!]));
    offset += line.length + 1;
    return runs;
  });
}

/** The class runs `classify` assigns, rendered as the editor's CSS classes. */
function classifyRuns(source: string): Run[][] {
  const className = (kind: TokenClass): string =>
    kind === "plain" ? "" : cssClass(kind as StyledClass);
  return source
    .split("\n")
    .map((line) => mergeRuns(classify(line).map(({ text, kind }) => [text, className(kind)])));
}

function mergeRuns(runs: readonly Run[]): Run[] {
  const merged: [string, string][] = [];
  for (const [text, className] of runs) {
    const last = merged.at(-1);
    if (last && last[1] === className) last[0] += text;
    else if (text !== "") merged.push([text, className]);
  }
  return merged;
}

const TRICKY = [
  'println("total ${count + 1} for $name and ${items.len()}")',
  'path := r"C:\\raw\\${dir} $name"',
  'say := "escaped \\" quote" # trailing comment',
  "# a whole-line comment with fn and 42",
  "fn read_all!(path: string) -> string $ Files:",
  "    text := load!(path)",
  "    ok := !done && (ready || flag) != false",
  "    `match` := `fn` + 1",
  "use pkg.models.user.{User as Person, super_name}",
  "pub use super.shared.{Email}",
  "fn make[reified T](value: T) -> List[Map[string, T]]: []",
  "use_count := as_text + super_value",
  "let big: i64 = 1_000_000 + 0xFF + 3.5e10 + -250ms + 1.5kb",
  "enum Shape: Circle(radius: f64)",
  "data User:",
  '    pub name: string = "anon"',
  "tests:",
  '    it("works"):',
  '        assert(self.ok, reason="with Self and true")',
  "c := 'x' + '\\n'",
  "let mut console = $.use(Console)",
  "    $.with(Clock=FixedClock {}, context...):",
  "@derive(Eq, Debug)",
  "impl[T] Encode for T by Structure:",
  "by := derive + with + context",
  "",
].join("\n");

test("editor token classes match classify for the tricky cases", () => {
  assert.deepEqual(editorRuns(TRICKY), classifyRuns(TRICKY));
  const runs = editorRuns(TRICKY);
  const at = (line: number, text: string): string | undefined =>
    runs[line]!.find(([run]) => run.includes(text))?.[1];
  assert.equal(at(0, "${"), "hd-interpolation");
  assert.equal(at(0, "$name"), "hd-interpolation");
  assert.equal(at(1, 'r"C:'), "hd-string");
  assert.equal(at(2, "# trailing"), "hd-comment");
  assert.equal(at(4, "read_all!"), "hd-function");
  assert.equal(at(6, "&&"), "hd-operator");
  assert.equal(at(7, "`match`"), "");
  assert.equal(at(8, "as"), "hd-keyword");
  assert.equal(at(9, "super"), "hd-keyword");
  assert.equal(at(10, "reified"), "hd-keyword");
  assert.equal(at(10, "List"), "hd-type");
  assert.equal(at(12, "1_000_000"), "hd-number");
  assert.equal(at(12, "250ms"), "hd-number");
  assert.equal(at(16, "tests"), "hd-keyword");
  assert.equal(at(18, "self"), "hd-literal");
  assert.equal(at(18, "with Self"), "hd-string");
  assert.equal(at(20, "use"), "hd-keyword");
  assert.equal(at(20, "mut"), "hd-keyword");
  assert.equal(at(21, "with"), "hd-keyword");
  assert.equal(at(21, "context"), "");
  assert.equal(at(22, "derive"), "hd-keyword");
  assert.equal(at(23, "by"), "hd-keyword");
  assert.equal(at(23, "Structure"), "hd-type");
  assert.equal(at(24, "by"), "");
  assert.equal(at(24, "derive"), "");
  assert.equal(at(24, "with"), "");
});

test("editor token classes match classify on the examples and runtime fixtures", async () => {
  const paths = [
    "examples/core.hd",
    "examples/suspension.hd",
    "spec/conformance/runtime/valid/context-values-install-providers.hd",
    "spec/conformance/runtime/valid/propagation-from-two-domains.hd",
    "spec/conformance/runtime/valid/embedded-field-satisfies-trait.hd",
    "spec/conformance/runtime/valid/println-console-stdout.hd",
    ...[
      "closures",
      "derive",
      "exit-code",
      "mutable-requirement",
      "numbers",
      "std",
      "suffixes",
      "tests",
      "top-level",
    ].map((name) => `website/playground/examples/${name}.hd`),
  ];
  for (const path of paths) {
    const source = await readFile(resolve(root, path), "utf8");
    assert.deepEqual(editorRuns(source), classifyRuns(source), path);
  }
});
