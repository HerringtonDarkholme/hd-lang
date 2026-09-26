// EBNF-driven program generator. Reads the ```ebnf fences of
// spec/02-grammar.md directly; it shares no tables with any parser.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { type Rng, specRoot } from "./common.ts";

export type Node =
  | { readonly kind: "alt"; readonly items: readonly Node[] }
  | { readonly kind: "lit"; readonly value: string }
  | { readonly kind: "opt"; readonly item: Node }
  | { readonly kind: "rep"; readonly item: Node }
  | { readonly kind: "seq"; readonly items: readonly Node[] }
  | { readonly kind: "sym"; readonly value: string };

export type Grammar = ReadonlyMap<string, Node>;

interface Token {
  readonly kind: string;
  readonly value: string;
}

function lexEbnf(text: string): Token[] {
  const tokens: Token[] = [];
  let index = 0;
  while (index < text.length) {
    const character = text[index]!;
    if (/\s/u.test(character)) {
      index += 1;
    } else if (/[\p{L}_]/u.test(character)) {
      const word = /^[\p{L}\p{N}_]+/u.exec(text.slice(index))![0];
      tokens.push({ kind: "name", value: word });
      index += word.length;
    } else if (character === '"' || character === "'") {
      let end = index + 1;
      while (end < text.length && text[end] !== character) end += text[end] === "\\" ? 2 : 1;
      tokens.push({ kind: "lit", value: text.slice(index + 1, end) });
      index = end + 1;
    } else if ("=;|,()[]{}".includes(character)) {
      tokens.push({ kind: character, value: character });
      index += 1;
    } else throw new Error(`generate.ts: unexpected EBNF character ${JSON.stringify(character)}`);
  }
  return tokens;
}

class Reader {
  private index = 0;
  private readonly tokens: readonly Token[];

  constructor(tokens: readonly Token[]) {
    this.tokens = tokens;
  }

  private peek(): Token | undefined {
    return this.tokens[this.index];
  }

  private expect(kind: string): Token {
    const found = this.tokens[this.index];
    if (found?.kind !== kind) throw new Error(`generate.ts: expected ${kind} in EBNF`);
    this.index += 1;
    return found;
  }

  rules(): Map<string, Node> {
    const result = new Map<string, Node>();
    while (this.peek()) {
      const name = this.expect("name").value;
      this.expect("=");
      result.set(name, this.alternatives());
      this.expect(";");
    }
    return result;
  }

  private alternatives(): Node {
    const items = [this.sequence()];
    while (this.peek()?.kind === "|") {
      this.index += 1;
      items.push(this.sequence());
    }
    return items.length === 1 ? items[0]! : { items, kind: "alt" };
  }

  private sequence(): Node {
    const items = [this.factor()];
    while (this.peek()?.kind === ",") {
      this.index += 1;
      items.push(this.factor());
    }
    return items.length === 1 ? items[0]! : { items, kind: "seq" };
  }

  private factor(): Node {
    const found = this.peek();
    if (!found) throw new Error("generate.ts: unexpected end of EBNF");
    this.index += 1;
    if (found.kind === "name") return { kind: "sym", value: found.value };
    if (found.kind === "lit") return { kind: "lit", value: found.value };
    const close = found.kind === "(" ? ")" : found.kind === "[" ? "]" : "}";
    const inner = this.alternatives();
    this.expect(close);
    if (found.kind === "(") return inner;
    return found.kind === "[" ? { item: inner, kind: "opt" } : { item: inner, kind: "rep" };
  }
}

export function loadGrammar(): Grammar {
  const chapter = readFileSync(resolve(specRoot, "02-grammar.md"), "utf8");
  const blocks = [...chapter.matchAll(/^```ebnf\s*\n(.*?)^```/gms)].map((match) => match[1]!);
  return new Reader(lexEbnf(blocks.join("\n"))).rules();
}

/** Terminals rendered from a fixed pool. Everything else undefined is also a terminal. */
const terminalPools: Readonly<Record<string, readonly string[]>> = {
  boolean_literal: ["true", "false"],
  char_literal: ["'a'", "'\\n'", "'$'"],
  float_literal: ["0.5", "1.0", "1e10", "1e309", "0.0"],
  identifier: [
    "x",
    "y",
    "value",
    "f",
    "main",
    "T",
    "Item",
    "Point",
    "Some",
    "None",
    "Ok",
    "i32",
    "i64",
    "f64",
    "bool",
    "string",
    "list",
    "map",
    "Eq",
    "Display",
    "Clock",
    "print",
    "len",
    "self_value",
  ],
  integer_literal: ["0", "1", "2", "42", "2147483647", "2147483648", "1_000"],
  nil_literal: ["nil"],
  raw_string_literal: ['r"raw\\n"'],
  string_expression: ['"text"', '""', '"a $x b"', '"${1 + 2}"', 'r"raw"', '"\\u{1F600}"'],
  string_literal: ['"text"', '""', '"a $x b"'],
};

const layout = new Set(["DEDENT", "EOF", "INDENT", "NEWLINE", "SUITE_END"]);

export class Generator {
  private readonly cost = new Map<string, number>();
  private emitted: string[] = [];
  private budget = 0;
  private readonly grammar: Grammar;

  constructor(grammar: Grammar) {
    this.grammar = grammar;
    this.computeCosts();
  }

  private nodeCost(node: Node): number {
    switch (node.kind) {
      case "lit":
        return 1;
      case "sym":
        return this.grammar.has(node.value) && !terminalPools[node.value]
          ? (this.cost.get(node.value) ?? Infinity)
          : 1;
      case "opt":
      case "rep":
        return 0;
      case "seq":
        return node.items.reduce((sum, item) => sum + this.nodeCost(item), 0);
      case "alt":
        return Math.min(...node.items.map((item) => this.nodeCost(item)));
    }
  }

  private computeCosts(): void {
    for (let changed = true; changed;) {
      changed = false;
      for (const [name, node] of this.grammar) {
        const value = this.nodeCost(node);
        if (value < (this.cost.get(name) ?? Infinity)) {
          this.cost.set(name, value);
          changed = true;
        }
      }
    }
  }

  /** Returns a token list (layout tokens included) derived from `start`. */
  derive(rng: Rng, start = "source_file", budget = 160): string[] {
    this.emitted = [];
    this.budget = budget;
    if (start === "source_file") {
      // `source_file` allows zero items; force 1-4 so the output is never empty.
      const count = 1 + rng.int(4);
      for (let index = 0; index < count; index += 1)
        this.expand({ kind: "sym", value: "top_level_item" }, rng, 1);
      this.emitted.push("EOF");
    } else this.expand({ kind: "sym", value: start }, rng, 0);
    return this.emitted;
  }

  private expand(node: Node, rng: Rng, depth: number): void {
    const starving = this.emitted.length >= this.budget || depth > 40;
    switch (node.kind) {
      case "lit":
        this.emitted.push(node.value);
        return;
      case "sym": {
        const pool = terminalPools[node.value];
        if (pool) this.emitted.push(rng.pick(pool));
        else if (this.grammar.has(node.value))
          this.expand(this.grammar.get(node.value)!, rng, depth + 1);
        else this.emitted.push(layout.has(node.value) ? node.value : rng.pick(["x", "0"]));
        return;
      }
      case "seq":
        for (const item of node.items) this.expand(item, rng, depth);
        return;
      case "opt":
        if (!starving && rng.chance(0.4)) this.expand(node.item, rng, depth);
        return;
      case "rep": {
        const limit = starving ? 0 : rng.pick([0, 0, 1, 1, 2, 3]);
        for (let count = 0; count < limit; count += 1) this.expand(node.item, rng, depth);
        return;
      }
      case "alt": {
        if (starving) {
          let best = node.items[0]!;
          for (const item of node.items) if (this.nodeCost(item) < this.nodeCost(best)) best = item;
          this.expand(best, rng, depth);
        } else this.expand(rng.pick(node.items), rng, depth);
      }
    }
  }
}

/** Renders a derived token list to source text, turning layout tokens into lines. */
export function render(tokens: readonly string[]): string {
  const lines: string[] = [];
  let level = 0;
  let current: string[] = [];
  const flush = (): void => {
    if (current.length) lines.push(`${"    ".repeat(level)}${current.join(" ")}`);
    current = [];
  };
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index]!;
    if (token === "NEWLINE") flush();
    else if (token === "INDENT") {
      flush();
      level += 1;
    } else if (token === "DEDENT") {
      flush();
      level = Math.max(0, level - 1);
    } else if (token === "SUITE_END") {
      const next = tokens.slice(index + 1).find((item) => !layout.has(item) || item === "EOF");
      if (!next || !["else", ",", ")", "]", "}"].includes(next)) flush();
    } else if (token !== "EOF") current.push(token);
  }
  flush();
  return `${lines.join("\n")}\n`;
}

/** Every quoted literal in the grammar: the insertion vocabulary for mutation. */
export function grammarLiterals(grammar: Grammar): string[] {
  const found = new Set<string>();
  const visit = (node: Node): void => {
    if (node.kind === "lit") found.add(node.value);
    else if (node.kind === "opt" || node.kind === "rep") visit(node.item);
    else if (node.kind === "seq" || node.kind === "alt") node.items.forEach(visit);
  };
  for (const node of grammar.values()) visit(node);
  return [...found].filter((value) => !value.startsWith('"')).sort();
}
