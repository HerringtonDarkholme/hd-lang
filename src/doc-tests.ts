// Doc tests (spec/lang/10-modules.md#doc-tests): each fenced `hd` block in a
// `##` comment of a module under the source root is a test of its own. This
// file finds the blocks of one module's source and turns each into the text
// of its own program: the block's leading `use` lines, then one `it` call
// whose body is the rest of the block (module.test.doc.uses,
// module.test.doc.body). It keeps where each program line came from, so a
// diagnostic or a failure names the `##` line it points into
// (spec/cli/command-line.md#r-cli.test.doc.location). The commands that
// compile and run the programs are in commands/doc-tests.ts.

import type { SourcePosition } from "./diagnostics.ts";
import { lex, type Token } from "./lexer.ts";

/** One doc test of a module. */
export interface DocTest {
  /** `doc <module>.<item>[i]`, or `doc <module>[i]` in module documentation (cli.test.doc.name). */
  readonly name: string;
  /** The program's source text. */
  readonly program: string;
  /**
   * The codes of the block's `# error: CODE` comments: a compile-fail doc
   * test when there are any (module.test.doc.compile-fail).
   */
  readonly errors: readonly string[];
  /** The 1-based line and column of the opening fence's text in the module's file. */
  readonly line: number;
  readonly column: number;
  /** Maps a 1-based line and column of `program` to the module's file. */
  readonly locate: (line: number, column: number) => { line: number; column: number };
  /** The leading `use` lines that start with `self` or `super` (module.test.doc.relative), as program lines. */
  readonly relativeUses: readonly number[];
}

/** A `##` line's text and where that text starts in the file. */
interface DocLine {
  readonly text: string;
  readonly line: number;
  /** 1-based column of the text's first character. */
  readonly column: number;
}

const DECLARATION_WORDS: ReadonlySet<string> = new Set([
  "fn",
  "data",
  "enum",
  "trait",
  "type",
  "impl",
]);
const LAYOUT: ReadonlySet<string> = new Set(["newline", "indent", "dedent", "eof"]);

/**
 * The doc tests of a module, in source order. `module` is its path, as
 * `text` for `src/text.hd`, or `pkg` for `src/lib.hd` (cli.test.doc.name.root).
 * A source that does not lex has none; compiling the module reports why.
 */
export function docTests(source: string, module: string): DocTest[] {
  if (!source.includes("##")) return [];
  const lexed = lex(source);
  if (lexed.diagnostics.some(({ severity }) => severity !== "warning")) return [];
  const tokens = lexed.tokens.filter(({ kind }) => !LAYOUT.has(kind));
  const lines = source.split(/\r?\n/);
  const tests: DocTest[] = [];
  const counts = new Map<string, number>();
  let index = 0;
  while (index < tokens.length) {
    if (tokens[index]!.kind !== "doc-comment") {
      index += 1;
      continue;
    }
    // One block: `##` lines on consecutive lines at one indentation (lex.doc.attach).
    const block: Token[] = [tokens[index]!];
    index += 1;
    while (
      tokens[index]?.kind === "doc-comment" &&
      tokens[index]!.span.start.line === block.at(-1)!.span.start.line + 1 &&
      tokens[index]!.span.start.column === block[0]!.span.start.column &&
      tokens[index]!.moduleDoc === block[0]!.moduleDoc
    ) {
      block.push(tokens[index]!);
      index += 1;
    }
    const item = block[0]!.moduleDoc ? "" : documentedItem(tokens, index, block);
    // An unattached block is an error of the module itself (lex.doc.unattached).
    if (item === undefined) continue;
    const docLines = block.map((token) => docLine(token, lines));
    for (const fence of hdFences(docLines)) {
      const count = counts.get(item) ?? 0;
      counts.set(item, count + 1);
      const name = `doc ${module}${item === "" ? "" : `.${item}`}[${count}]`;
      tests.push(docTest(name, fence.open, fence.body));
    }
  }
  return tests;
}

function docLine(token: Token, lines: readonly string[]): DocLine {
  const { line, column } = token.span.start;
  // `##` and one following space are not part of the text (lex.doc.text).
  const space = lines[line - 1]?.[column + 1] === " " ? 1 : 0;
  return { text: String(token.value ?? token.text), line, column: column + 2 + space };
}

/**
 * The name of the item a block documents, as `slugify` or `Slug.new`
 * (cli.test.doc.name.member), or undefined when the block attaches to
 * nothing. `next` indexes the first token after the block.
 */
function documentedItem(
  tokens: readonly Token[],
  next: number,
  block: readonly Token[],
): string | undefined {
  const { line, column } = block.at(-1)!.span.start;
  let target: Token | undefined = tokens[next];
  if (!target || target.span.start.line !== line + 1 || target.span.start.column !== column)
    return undefined;
  // A decorator line may stand between the block and its declaration.
  while (target?.text === "@") {
    const decorator = target.span.start.line;
    next = tokens.findIndex(
      (token, at) =>
        at > next && token.span.start.line > decorator && token.span.start.column === column,
    );
    target = next < 0 ? undefined : tokens[next];
  }
  if (!target) return undefined;
  const own = declaredName(tokens, next);
  if (own === undefined) return undefined;
  const parent = enclosingDeclaration(tokens, next);
  return parent === undefined ? own : `${parent}.${own}`;
}

/** The name a line declares, starting at token `at`: a declaration's or a member's. */
function declaredName(tokens: readonly Token[], at: number): string | undefined {
  if (tokens[at]?.text === "pub") at += 1;
  const word = tokens[at];
  if (!word) return undefined;
  if (word.text === "impl") return implementedType(tokens, at + 1);
  if (DECLARATION_WORDS.has(word.text)) return tokens[at + 1]?.text;
  return word.kind === "identifier" ? word.text : undefined;
}

/** The type an `impl` line implements for: the type after `for`, or the inherent impl's type. */
function implementedType(tokens: readonly Token[], at: number): string | undefined {
  const line = tokens[at - 1]!.span.start.line;
  let depth = 0;
  let first: string | undefined;
  for (; tokens[at] && tokens[at]!.span.start.line === line; at += 1) {
    const token = tokens[at]!;
    if (token.text === "[" || token.text === "(") depth += 1;
    else if (token.text === "]" || token.text === ")") depth -= 1;
    else if (depth > 0) continue;
    else if (token.text === "for") return tokens[at + 1]?.text;
    else if (token.text === ":") break;
    else if (first === undefined && token.kind === "identifier") first = token.text;
  }
  return first;
}

/**
 * The name of the declaration that holds the line at token `at`: the
 * nearest line above with less indentation that declares something.
 */
function enclosingDeclaration(tokens: readonly Token[], at: number): string | undefined {
  let column = tokens[at]!.span.start.column;
  if (column === 1) return undefined;
  for (let index = at - 1; index >= 0; index -= 1) {
    const token = tokens[index]!;
    const first = index === 0 || tokens[index - 1]!.span.start.line < token.span.start.line;
    if (!first || token.kind === "doc-comment" || token.span.start.column >= column) continue;
    const word = token.text === "pub" ? tokens[index + 1]?.text : token.text;
    if (word !== undefined && DECLARATION_WORDS.has(word)) return declaredName(tokens, index);
    column = token.span.start.column;
    if (column === 1) return undefined;
  }
  return undefined;
}

/** The fenced code blocks whose info string is `hd` (module.test.doc.fence). */
function hdFences(
  lines: readonly DocLine[],
): { readonly open: DocLine; readonly body: readonly DocLine[] }[] {
  const fences: { open: DocLine; body: DocLine[] }[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const openLine = lines[index]!;
    const open = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(openLine.text);
    if (!open) continue;
    const marker = open[1]!;
    const info = open[2]!.trim().split(/\s+/)[0];
    const body: DocLine[] = [];
    for (index += 1; index < lines.length; index += 1) {
      const close = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[index]!.text);
      if (close && close[1]![0] === marker[0] && close[1]!.length >= marker.length) break;
      body.push(lines[index]!);
    }
    if (info === "hd") fences.push({ open: openLine, body });
  }
  return fences;
}

/** Whether a line is blank or only a comment. */
function isQuiet(text: string): boolean {
  const trimmed = text.trim();
  return trimmed === "" || trimmed.startsWith("#");
}

function docTest(name: string, open: DocLine, body: readonly DocLine[]): DocTest {
  // The block's `use` lines come first (module.test.doc.uses); a grouped use
  // may go on over several lines.
  let header = 0;
  let depth = 0;
  while (header < body.length) {
    const text = body[header]!.text;
    if (depth === 0 && !isQuiet(text) && !/^(pub\s+)?use\s/.test(text.trim())) break;
    depth += (text.match(/\{/g) ?? []).length - (text.match(/\}/g) ?? []).length;
    header += 1;
  }
  const program: string[] = [];
  const origins: { readonly line: number; readonly shift: number }[] = [];
  const add = (text: string, line: number, shift: number): void => {
    program.push(text);
    origins.push({ line, shift });
  };
  const relativeUses: number[] = [];
  for (const line of body.slice(0, header)) {
    if (/^(pub\s+)?use\s+(self|super)\b/.test(line.text.trim()))
      relativeUses.push(program.length + 1);
    add(line.text, line.line, line.column - 1);
  }
  // The rest is the body of the one test case (module.test.doc.body).
  add(`it(${JSON.stringify(name)}):`, open.line, open.column - 1);
  const rest = body.slice(header);
  for (const line of rest)
    add(line.text === "" ? "" : `    ${line.text}`, line.line, line.column - 1 - 4);
  if (rest.every(({ text }) => isQuiet(text))) add("    pass", open.line, open.column - 5);
  const errors = body.flatMap(({ text }) =>
    [...text.matchAll(/#\s*error:\s*([a-z][a-z0-9-]*)/g)].map((match) => match[1]!),
  );
  return {
    name,
    program: `${program.join("\n")}\n`,
    errors,
    line: open.line,
    column: open.column,
    relativeUses,
    locate(line, column) {
      const origin = origins[Math.min(Math.max(line, 1), origins.length) - 1]!;
      return { line: origin.line, column: Math.max(1, column + origin.shift) };
    },
  };
}

/** A position of a doc test's program, moved to the module's file, whose text gives its offset. */
export function filePosition(
  test: DocTest,
  position: SourcePosition,
  lineOffsets: readonly number[],
): SourcePosition {
  const { line, column } = test.locate(position.line, position.column);
  return { line, column, offset: (lineOffsets[line - 1] ?? 0) + column - 1 };
}

/** The offset of each line's start in `text`. */
export function lineOffsets(text: string): number[] {
  const offsets = [0];
  for (let index = text.indexOf("\n"); index !== -1; index = text.indexOf("\n", index + 1))
    offsets.push(index + 1);
  return offsets;
}
