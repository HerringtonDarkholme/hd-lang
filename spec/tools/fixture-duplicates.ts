// Duplicate conformance inputs: two cases that test one program, and two
// package trees with the same files. spec/check-fixture-index.ts runs both
// checks over text it has already read.
//
// Cost: each fixture is normalized once, in a constant number of linear
// passes over its text, and keyed into one Map; each tree file is hashed once.
// No step compares a fixture or tree with another, so the work is linear in
// the suite's bytes.

import { createHash } from "node:crypto";

/** One `cases.tsv` row with its fixture text. */
export interface FixtureCase {
  readonly path: string;
  readonly phase: string;
  readonly expectation: string;
  readonly text: string;
}

/** One package tree: its name and its files as (path inside the tree, bytes). */
export interface PackageTree {
  readonly name: string;
  readonly files: ReadonlyArray<readonly [string, Uint8Array | string]>;
}

const directive = /^# (?:fixture-[a-z-]+|expect|expect-stdout|expect-empty-stdout):/;
const marker = /^#\s*(diagnostic|warning|panic):\s*([a-z0-9-]+)/;

/** Splits code from comments across the lines of one fixture. */
class CommentScanner {
  // Inside a `"..."` or `"""..."""` literal: its closing quote; else "".
  private quote = "";
  // Open `${` interpolations: the literal each returns to, and its open braces.
  private readonly resume: string[] = [];
  private readonly braces: number[] = [];

  /** Whether the scanner is outside every literal and interpolation. */
  get atCode(): boolean {
    return this.quote === "" && this.resume.length === 0;
  }

  /** The line's code; a comment keeps only a doc comment or an expectation marker. */
  scan(line: string): string {
    let code = "";
    let at = 0;
    while (at < line.length) {
      if (this.quote === "" && line[at] === "#") {
        return comment(code, line, at);
      }
      const [text, next] = this.quote === "" ? this.code(line, at) : this.literal(line, at);
      code += text;
      at = next;
    }
    return code;
  }

  /** One step inside a string literal. */
  private literal(line: string, at: number): [string, number] {
    if (line[at] === "\\") {
      return [line.slice(at, at + 2), at + 2];
    }
    if (line.startsWith(this.quote, at)) {
      const quote = this.quote;
      this.quote = "";
      return [quote, at + quote.length];
    }
    if (line.startsWith("${", at)) {
      this.resume.push(this.quote);
      this.braces.push(0);
      this.quote = "";
      return ["${", at + 2];
    }
    return [line[at]!, at + 1];
  }

  /** One step of code outside a literal. */
  private code(line: string, at: number): [string, number] {
    const ch = line[at]!;
    if (ch === '"') {
      this.quote = line.startsWith('"""', at) ? '"""' : '"';
      return [this.quote, at + this.quote.length];
    }
    if (ch === "'") {
      // A character literal; a lone apostrophe begins a lifetime (lex.lifetime.char).
      const literal = /^'(?:\\.[^']*|[^'\\])'/.exec(line.slice(at));
      const length = literal ? literal[0].length : 1;
      return [line.slice(at, at + length), at + length];
    }
    if (this.resume.length > 0) {
      this.interpolation(ch);
    }
    return [ch, at + 1];
  }

  /** Tracks braces inside `${...}`; the brace that closes it returns to its literal. */
  private interpolation(ch: string): void {
    const depth = this.braces.length - 1;
    if (ch === "{") {
      this.braces[depth]! += 1;
    } else if (ch === "}" && this.braces[depth] === 0) {
      this.braces.pop();
      this.quote = this.resume.pop()!;
    } else if (ch === "}") {
      this.braces[depth]! -= 1;
    }
  }
}

/** The code before a comment at `at`, plus the comment if it is a doc comment or a marker. */
function comment(code: string, line: string, at: number): string {
  const text = line.slice(at);
  if (text.startsWith("##") && code.trim() === "") {
    return line;
  }
  const expected = marker.exec(text);
  return expected ? `${code.trimEnd()} #${expected[1]}:${expected[2]}` : code;
}

/**
 * The fixture's program: header directives kept verbatim, then the code with
 * plain comments and blank lines removed. A `#` starts a comment only outside
 * a string or character literal (lex.comment.line); `##` documentation
 * comments and expectation markers stay, since they change what the case
 * tests.
 */
export function fixtureProgram(text: string): string {
  const out: string[] = [];
  const scanner = new CommentScanner();
  for (const physical of text.split("\n")) {
    // A CRLF line keeps its CR, so a CRLF fixture never matches its LF twin.
    const ending = physical.endsWith("\r") ? "\r" : "";
    const line = ending ? physical.slice(0, -1) : physical;
    const code = scanner.atCode && directive.test(line) ? line : scanner.scan(line);
    if (code.trim() !== "") {
      out.push(code.trimEnd() + ending);
    }
  }
  return out.join("\n");
}

// Names that renaming keeps: keywords and the prelude and std names whose
// identity is part of what a case tests.
const KEEP = new Set(
  `fn pub let mut match if else for in while loop break continue return struct enum trait impl
type use tests it assert assert_equal true false self Self and or not as is pass defer by where
println print main string bool i8 i16 i32 i64 u8 u16 u32 u64 usize f32 f64 void never List Map
Result Option Some None Ok Err dyn const`.split(/\s+/),
);

// A final `main` that prints only a literal runs nothing the case checks.
const trivialMain =
  /\npub fn main\(\) -> void(?: \$ Console)?:\n {4}println\((?:-?\d+|"[^"$\\]*")\)$/;

/**
 * The program with its own names replaced by their order of first use: each
 * lowercase name it declares becomes `v1`, `v2` ..., and each type it
 * declares `T1`, `T2` .... Two cases that differ only in their names map to
 * one string.
 */
export function renamedProgram(program: string): string {
  const text = program.replace(trivialMain, "");
  const declared = new Set<string>();
  for (const pattern of [
    /\bfn\s+(\w+)/g,
    /\b(\w+)\s*:=/g,
    /\blet\s+(?:mut\s+)?(\w+)/g,
    /\bfor\s+(\w+)\s+in/g,
    /[(,]\s*(?:mut\s+)?(\w+)\s*:/g,
    /^\s+(\w+):\s/gm,
  ]) {
    for (const match of text.matchAll(pattern)) {
      declared.add(match[1]!);
    }
  }
  const names = new Map<string, string>();
  const values = text.replace(/\b[A-Za-z_][A-Za-z0-9_]*\b/g, (word) => {
    if (KEEP.has(word) || /^[A-Z]/.test(word) || !declared.has(word)) {
      return word;
    }
    let name = names.get(word);
    if (name === undefined) {
      name = `v${names.size + 1}`;
      names.set(word, name);
    }
    return name;
  });
  const types = new Map<string, string>();
  for (const match of values.matchAll(/\b(?:struct|enum|trait|type)\s+([A-Z]\w*)/g)) {
    if (!types.has(match[1]!)) {
      types.set(match[1]!, `T${types.size + 1}`);
    }
  }
  return values.replace(/\b[A-Z]\w*\b/g, (word) => types.get(word) ?? word);
}

/**
 * One message per pair of cases with the same phase, expectation and
 * program, before or after renaming (checks A2 and A14 of the 2026-11-14
 * fixture review). Each later case is reported against the first one.
 */
export function duplicateFixtures(cases: Iterable<FixtureCase>): string[] {
  const first = new Map<string, FixtureCase & { readonly program: string }>();
  const problems: string[] = [];
  for (const fixture of cases) {
    const program = fixtureProgram(fixture.text);
    const key = `${fixture.phase}\t${fixture.expectation}\t${renamedProgram(program)}`;
    const earlier = first.get(key);
    if (earlier === undefined) {
      first.set(key, { ...fixture, program });
      continue;
    }
    const how = earlier.program === program ? "the same program" : "the same program up to names";
    problems.push(
      `fixtures ${earlier.path} and ${fixture.path} test ${how} with the same phase and expectation; delete one, or make them differ in what they test`,
    );
  }
  return problems;
}

/** One message per pair of package trees whose files are identical (check A1). */
export function duplicateTrees(trees: Iterable<PackageTree>): string[] {
  const first = new Map<string, string>();
  const problems: string[] = [];
  for (const tree of trees) {
    const hash = createHash("sha256");
    for (const [path, bytes] of [...tree.files].sort(([left], [right]) =>
      left < right ? -1 : 1,
    )) {
      hash.update(`${path}\0`);
      hash.update(bytes);
      hash.update("\0");
    }
    const digest = hash.digest("hex");
    const earlier = first.get(digest);
    if (earlier === undefined) {
      first.set(digest, tree.name);
    } else {
      problems.push(
        `package trees ${earlier} and ${tree.name} hold identical files; point their fixtures at one tree`,
      );
    }
  }
  return problems;
}
