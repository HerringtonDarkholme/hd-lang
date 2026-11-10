// Classifies each line of a Rust source file for the core coverage report
// (scripts/coverage.ts). LLVM counts a line by its regions alone: a blank or
// comment line inside a function body, or a line of a `#[cfg(test)]` module,
// is a counted line too. This module lexes the file instead, so the report
// counts only lines with code, and sorts them into three kinds:
//
// - "test": every token is inside an item marked `#[cfg(test)]` or `#[test]`.
//   Test code is not compiler code, and it leaves the report.
// - "invariant": every token is part of a check that fails only when an
//   internal invariant breaks (#1442). These are counted apart:
//   - `unreachable!`, `panic!`, `assert!` and `debug_assert!` (`_eq`, `_ne`) invocations;
//   - verifier findings: a `VerifyError { .. }` literal anywhere, and inside
//     verifier code a call that reports a finding (`err(..)`, `self.err(..)`,
//     `errs.push(..)`, `self.errs.push(..)`). Verifier code is a function
//     named `verify*`, a function whose signature names `VerifyError`, and
//     every `impl` of a struct with a field naming `VerifyError`;
//   - the braces, `else`, match-arm head (`Pat =>`) or function signature
//     around a block that holds nothing else, so `None => {` and the closing
//     `}` of a `panic!`-only block go with the panic.
// - "code": every other line with a token.
//
// `,` and `;` don't decide a line's kind. A line absent from `kinds` has no
// code (blank, comment, or inside nothing LLVM can count).

export type LineKind = "code" | "invariant" | "test";

export type FileLines = {
  kinds: Map<number, LineKind>;
  // Modules declared `#[cfg(test)] mod name;`, whose files are test code.
  testModules: string[];
  // `#![cfg(test)]` at the top: the whole file is test code.
  wholeFileTest: boolean;
};

type Token = { text: string; line: number; end: number };

const INVARIANT_MACROS = new Set([
  "unreachable",
  "panic",
  "assert",
  "assert_eq",
  "assert_ne",
  "debug_assert",
  "debug_assert_eq",
  "debug_assert_ne",
]);
const OPEN = new Set(["(", "[", "{"]);
const CLOSE = new Set([")", "]", "}"]);
const PAIR: Record<string, string> = { ")": "(", "]": "[", "}": "{" };
const NEUTRAL = new Set([",", ";"]);
const ITEM_KEYWORDS = new Set(["struct", "impl", "for", "enum", "trait", "union", "type"]);

const isIdentStart = (ch: string): boolean => /[\p{XID_Start}_]/u.test(ch);
const isIdentContinue = (ch: string): boolean => /[\p{XID_Continue}]/u.test(ch);
const isDigit = (ch: string): boolean => ch >= "0" && ch <= "9";

class Lexer {
  readonly tokens: Token[] = [];
  pos = 0;
  line = 1;
  private readonly src: string;

  constructor(src: string) {
    this.src = src;
  }

  run(): Token[] {
    while (this.pos < this.src.length) {
      this.step();
    }
    return this.tokens;
  }

  private step(): void {
    const ch = this.src[this.pos];
    const next = this.src[this.pos + 1];
    if (ch === "\n") {
      this.line++;
      this.pos++;
    } else if (ch.trim() === "") {
      this.pos++;
    } else if (ch === "/" && next === "/") {
      const end = this.src.indexOf("\n", this.pos);
      this.pos = end < 0 ? this.src.length : end;
    } else if (ch === "/" && next === "*") {
      this.blockComment();
    } else if (ch === '"') {
      this.quoted(this.pos);
    } else if (ch === "'") {
      this.quote(this.pos);
    } else if (isIdentStart(ch)) {
      this.word();
    } else if (isDigit(ch)) {
      this.number();
    } else if (ch === "=" && next === ">") {
      this.push("=>", this.line);
      this.pos += 2;
    } else {
      this.push(ch, this.line);
      this.pos++;
    }
  }

  private push(text: string, line: number): void {
    this.tokens.push({ text, line, end: this.line });
  }

  // Advances over `[from, to)`, counting its newlines.
  private advance(to: number): void {
    for (let i = this.pos; i < to; i++) {
      if (this.src[i] === "\n") {
        this.line++;
      }
    }
    this.pos = to;
  }

  private blockComment(): void {
    let depth = 0;
    let i = this.pos;
    do {
      if (this.src.startsWith("/*", i)) {
        depth++;
        i += 2;
      } else if (this.src.startsWith("*/", i)) {
        depth--;
        i += 2;
      } else {
        i++;
      }
    } while (depth > 0 && i < this.src.length);
    this.advance(i);
  }

  // A string from `start`, whose opening quote is at `this.pos`.
  private quoted(start: number): void {
    const line = this.line;
    let i = this.pos + 1;
    while (i < this.src.length && this.src[i] !== '"') {
      i += this.src[i] === "\\" ? 2 : 1;
    }
    this.advance(i + 1);
    this.tokens.push({ text: this.src.slice(start, this.pos), line, end: this.line });
  }

  // A raw string from `start`: `this.pos` is at its first `#` or quote.
  private raw(start: number): void {
    const line = this.line;
    let hashes = 0;
    while (this.src[this.pos + hashes] === "#") {
      hashes++;
    }
    const close = `"${"#".repeat(hashes)}`;
    const end = this.src.indexOf(close, this.pos + hashes + 1);
    this.advance(end < 0 ? this.src.length : end + close.length);
    this.tokens.push({ text: this.src.slice(start, this.pos), line, end: this.line });
  }

  // A character literal or a lifetime; `this.pos` is at the quote.
  private quote(start: number): void {
    const src = this.src;
    let i = this.pos + 1;
    if (src[i] === "\\") {
      i = src.indexOf("'", i + 2);
    } else if (src[i + 1] === "'" && src[i] !== "\n") {
      i += 1;
    } else if ((src.codePointAt(i) ?? 0) > 0xffff && src[i + 2] === "'") {
      i += 2;
    } else {
      while (i < src.length && isIdentContinue(src[i])) {
        i++;
      }
      this.push(src.slice(start, i), this.line);
      this.pos = i;
      return;
    }
    this.push(src.slice(start, i + 1), this.line);
    this.pos = i + 1;
  }

  private word(): void {
    const start = this.pos;
    let i = this.pos;
    while (i < this.src.length && isIdentContinue(this.src[i])) {
      i++;
    }
    const text = this.src.slice(start, i);
    const next = this.src[i];
    this.pos = i;
    if ((text === "b" || text === "c") && next === '"') {
      this.quoted(start);
    } else if (text === "b" && next === "'") {
      this.quote(start);
    } else if (["r", "br", "cr"].includes(text) && (next === '"' || this.rawHashes(i))) {
      this.raw(start);
    } else if (text === "r" && next === "#") {
      this.pos = i + 1;
      this.word();
    } else {
      this.push(text, this.line);
    }
  }

  // Whether `#`s from `i` lead to a quote: `r#"` starts a raw string, `r#x` a raw identifier.
  private rawHashes(i: number): boolean {
    let j = i;
    while (this.src[j] === "#") {
      j++;
    }
    return j > i && this.src[j] === '"';
  }

  private number(): void {
    let i = this.pos;
    const src = this.src;
    while (i < src.length) {
      const ch = src[i];
      if (isIdentContinue(ch) || (ch === "." && isDigit(src[i + 1] ?? "") && src[i - 1] !== ".")) {
        i++;
      } else {
        break;
      }
    }
    this.push(src.slice(this.pos, i), this.line);
    this.pos = i;
  }
}

export function tokenize(src: string): Token[] {
  return new Lexer(src).run();
}

// For each bracket token, the index of its partner; -1 for other tokens.
function partners(tokens: Token[], file: string): Int32Array {
  const partner = new Int32Array(tokens.length).fill(-1);
  const stack: number[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const text = tokens[i].text;
    if (OPEN.has(text)) {
      stack.push(i);
    } else if (CLOSE.has(text)) {
      const open = stack.pop();
      if (open === undefined || tokens[open].text !== PAIR[text]) {
        throw new Error(`${file}:${tokens[i].line}: unbalanced \`${text}\``);
      }
      partner[open] = i;
      partner[i] = open;
    }
  }
  if (stack.length > 0) {
    throw new Error(`${file}:${tokens[stack[0]].line}: unclosed \`${tokens[stack[0]].text}\``);
  }
  return partner;
}

class Classifier {
  readonly inv: Uint8Array;
  readonly test: Uint8Array;
  readonly testModules: string[] = [];
  wholeFileTest = false;
  private readonly tokens: Token[];
  private readonly partner: Int32Array;

  constructor(tokens: Token[], partner: Int32Array) {
    this.tokens = tokens;
    this.partner = partner;
    this.inv = new Uint8Array(tokens.length);
    this.test = new Uint8Array(tokens.length);
  }

  private text(i: number): string {
    return this.tokens[i]?.text ?? "";
  }

  private mark(flags: Uint8Array, from: number, to: number): void {
    flags.fill(1, from, to + 1);
  }

  // The index after a bracketed group opened at `i`, or `i + 1` for any other token.
  private skip(i: number): number {
    return OPEN.has(this.text(i)) ? this.partner[i] + 1 : i + 1;
  }

  // ---- test items ----

  markTestItems(): void {
    for (let i = 0; i < this.tokens.length; i++) {
      if (this.text(i) !== "#") {
        continue;
      }
      const inner = this.text(i + 1) === "!";
      const open = inner ? i + 2 : i + 1;
      if (this.text(open) !== "[" || !this.isTestAttribute(open)) {
        continue;
      }
      if (inner) {
        this.wholeFileTest = true;
        continue;
      }
      const end = this.itemEnd(this.partner[open] + 1);
      this.mark(this.test, i, end);
    }
  }

  private isTestAttribute(open: number): boolean {
    const body = this.tokens
      .slice(open + 1, this.partner[open])
      .map((token) => token.text)
      .join("");
    return body === "cfg(test)" || body === "test";
  }

  // The last token of the item that starts at `from` (after an attribute).
  private itemEnd(from: number): number {
    let i = from;
    let angle = 0;
    while (i < this.tokens.length) {
      const text = this.text(i);
      if (text === "#" && this.text(i + 1) === "[") {
        i = this.partner[i + 1] + 1;
        continue;
      }
      if (text === "mod" && this.text(i + 2) === ";") {
        this.testModules.push(this.text(i + 1));
        return i + 2;
      }
      if (text === "{") {
        return this.partner[i];
      }
      if (text === ";" || CLOSE.has(text) || (text === "," && angle === 0)) {
        return text === ";" || text === "," ? i : i - 1;
      }
      angle += this.angleStep(i);
      i = this.skip(i);
    }
    return this.tokens.length - 1;
  }

  private angleStep(i: number): number {
    const text = this.text(i);
    if (text === "<") {
      return 1;
    }
    return text === ">" && this.text(i - 1) !== "-" ? -1 : 0;
  }

  // ---- invariant spans ----

  markInvariantMacros(): void {
    for (let i = 0; i + 2 < this.tokens.length; i++) {
      if (
        INVARIANT_MACROS.has(this.text(i)) &&
        this.text(i + 1) === "!" &&
        OPEN.has(this.text(i + 2)) &&
        this.text(i - 1) !== "macro_rules"
      ) {
        this.mark(this.inv, this.pathStart(i), this.partner[i + 2]);
      }
    }
  }

  // The first token of a path ending at `i` (`std::panic` starts at `std`).
  private pathStart(i: number): number {
    let start = i;
    while (this.text(start - 1) === ":" && this.text(start - 2) === ":") {
      start -= 3;
    }
    return start;
  }

  markVerifierFindings(): void {
    const types = this.verifierTypes();
    for (let i = 0; i < this.tokens.length; i++) {
      if (this.text(i) === "VerifyError" && this.text(i + 1) === "{" && this.isLiteral(i)) {
        this.mark(this.inv, i, this.partner[i + 1]);
      }
      const body = this.verifierBody(i, types);
      if (body >= 0) {
        this.markFindingCalls(body, this.partner[body]);
      }
    }
  }

  private isLiteral(i: number): boolean {
    const before = this.text(i - 1);
    return !ITEM_KEYWORDS.has(before) && !(before === ">" && this.text(i - 2) === "-");
  }

  // Structs with a field that names `VerifyError`: their impls are verifier code.
  private verifierTypes(): Set<string> {
    const types = new Set<string>();
    for (let i = 0; i + 1 < this.tokens.length; i++) {
      if (this.text(i) !== "struct") {
        continue;
      }
      const open = this.findBody(i + 2);
      if (open >= 0 && this.mentions(open, this.partner[open], "VerifyError")) {
        types.add(this.text(i + 1));
      }
    }
    return types;
  }

  // The body `{` of a declaration whose head starts at `from`, or -1 when it has none.
  private findBody(from: number): number {
    for (let i = from; i < this.tokens.length; i = this.skip(i)) {
      const text = this.text(i);
      if (text === "{") {
        return i;
      }
      if (text === ";" || text === "=" || CLOSE.has(text)) {
        return -1;
      }
    }
    return -1;
  }

  private mentions(from: number, to: number, name: string): boolean {
    for (let i = from; i <= to; i++) {
      if (this.text(i) === name) {
        return true;
      }
    }
    return false;
  }

  // The body `{` of the verifier `fn` or `impl` at `i`, or -1.
  private verifierBody(i: number, types: Set<string>): number {
    const keyword = this.text(i);
    if (keyword === "fn" && /^[\p{XID_Start}_]/u.test(this.text(i + 1))) {
      const open = this.findBody(i + 2);
      const named = this.text(i + 1).startsWith("verify");
      return open >= 0 && (named || this.mentions(i, open, "VerifyError")) ? open : -1;
    }
    if (keyword === "impl") {
      const open = this.findBody(i + 1);
      const names = this.tokens.slice(i + 1, Math.max(open, i + 1)).map((token) => token.text);
      return open >= 0 && names.some((name) => types.has(name)) ? open : -1;
    }
    return -1;
  }

  private markFindingCalls(from: number, to: number): void {
    for (let i = from; i < to; i++) {
      const start = this.text(i - 1) === "." && this.text(i - 2) === "self" ? i - 2 : i;
      const callee = this.text(i);
      if (callee === "err" && this.text(i + 1) === "(" && this.isSinkReceiver(i)) {
        this.mark(this.inv, start, this.partner[i + 1]);
      } else if (
        callee === "errs" &&
        this.text(i + 1) === "." &&
        this.text(i + 2) === "push" &&
        this.text(i + 3) === "("
      ) {
        this.mark(this.inv, start, this.partner[i + 3]);
      }
    }
  }

  // `err(..)` or `self.err(..)`, and not `fn err(` or `other.err(`.
  private isSinkReceiver(i: number): boolean {
    const before = this.text(i - 1);
    if (before === ".") {
      return this.text(i - 2) === "self";
    }
    return before !== "fn";
  }

  // ---- blocks and arms that hold only invariant code ----

  propagate(): void {
    const events: { end: number; start: number }[] = [];
    for (let i = 0; i < this.tokens.length; i++) {
      const text = this.text(i);
      if (text === "{") {
        events.push({ end: this.partner[i], start: i });
      } else if (text === "=>" && this.text(i + 1) !== "{") {
        events.push({ end: this.armEnd(i + 1), start: i });
      }
    }
    events.sort((left, right) => left.end - right.end);
    for (const { start, end } of events) {
      if (this.text(start) === "{") {
        this.block(start, end);
      } else if (end > start && this.onlyInvariant(start + 1, end)) {
        this.inv[start] = 1;
        this.armHead(start);
      }
    }
  }

  // The last token of a brace-less match arm body starting at `from`.
  private armEnd(from: number): number {
    let i = from;
    while (i < this.tokens.length) {
      const text = this.text(i);
      if (text === "," || CLOSE.has(text)) {
        return i - 1;
      }
      i = this.skip(i);
    }
    return this.tokens.length - 1;
  }

  private onlyInvariant(from: number, to: number): boolean {
    let any = false;
    for (let i = from; i <= to; i++) {
      if (this.inv[i]) {
        any = true;
      } else if (!this.test[i] && !NEUTRAL.has(this.text(i))) {
        return false;
      }
    }
    return any;
  }

  private block(open: number, close: number): void {
    if (!this.onlyInvariant(open + 1, close - 1)) {
      return;
    }
    this.inv[open] = 1;
    this.inv[close] = 1;
    const before = this.text(open - 1);
    if (before === "else") {
      this.inv[open - 1] = 1;
    } else if (before === "=>") {
      this.inv[open - 1] = 1;
      this.armHead(open - 1);
    } else {
      this.functionHead(open);
    }
  }

  // Marks the pattern before the arm's `=>` at `arrow`. The pattern starts
  // after a `,`, the match's `{`, or a block arm's `}`. A guard (`if`) runs
  // code, and reaching another `=>` first means the previous arm had no comma
  // and its end is unclear, so either leaves the pattern unmarked.
  private armHead(arrow: number): void {
    let i = arrow - 1;
    while (i >= 0) {
      const text = this.text(i);
      if (text === "," || OPEN.has(text)) {
        break;
      }
      if (text === "}" && this.text(this.partner[i] - 1) === "=>") {
        break;
      }
      if (text === "if" || text === "=>") {
        return;
      }
      i = CLOSE.has(text) ? this.partner[i] - 1 : i - 1;
    }
    this.mark(this.inv, i + 1, arrow);
  }

  // Marks the signature (with attributes and visibility) of a `fn` whose body opens at `open`.
  private functionHead(open: number): void {
    let i = open - 1;
    let isFunction = false;
    while (i >= 0) {
      const text = this.text(i);
      if (text === ";" || text === "{" || text === "}") {
        break;
      }
      if (text === "fn" && /^[\p{XID_Start}_]/u.test(this.text(i + 1))) {
        isFunction = true;
      }
      i = text === ")" || text === "]" ? this.partner[i] - 1 : i - 1;
    }
    if (isFunction) {
      this.mark(this.inv, i + 1, open);
    }
  }

  // ---- lines ----

  lines(): Map<number, LineKind> {
    const state = new Map<number, { test: boolean; invariant: boolean }>();
    for (let i = 0; i < this.tokens.length; i++) {
      const token = this.tokens[i];
      if (NEUTRAL.has(token.text)) {
        continue;
      }
      for (let line = token.line; line <= token.end; line++) {
        const seen = state.get(line) ?? { test: true, invariant: true };
        seen.test &&= this.test[i] === 1;
        seen.invariant &&= this.test[i] === 1 || this.inv[i] === 1;
        state.set(line, seen);
      }
    }
    const kinds = new Map<number, LineKind>();
    for (const [line, { test, invariant }] of state) {
      kinds.set(line, kindOf(test || this.wholeFileTest, invariant));
    }
    return kinds;
  }
}

function kindOf(test: boolean, invariant: boolean): LineKind {
  if (test) {
    return "test";
  }
  return invariant ? "invariant" : "code";
}

export function classifyRust(src: string, file = "<source>"): FileLines {
  const tokens = tokenize(src);
  const classifier = new Classifier(tokens, partners(tokens, file));
  classifier.markTestItems();
  classifier.markInvariantMacros();
  classifier.markVerifierFindings();
  classifier.propagate();
  return {
    kinds: classifier.lines(),
    testModules: classifier.testModules,
    wholeFileTest: classifier.wholeFileTest,
  };
}
