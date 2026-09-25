import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import type {
  ChartItem,
  EbnfNode,
  EbnfToken,
  Grammar,
  GrammarToken,
  ParseResult,
} from "./types.ts";

function isLetter(character: string): boolean {
  return /^\p{L}$/u.test(character);
}

function isLetterOrNumber(character: string): boolean {
  return /^[\p{L}\p{N}]$/u.test(character);
}

class EbnfReader {
  readonly tokens: readonly EbnfToken[];
  private index = 0;

  constructor(text: string) {
    this.tokens = EbnfReader.lex(text);
  }

  private static lex(text: string): EbnfToken[] {
    const result: EbnfToken[] = [];
    let index = 0;
    while (index < text.length) {
      const character = text[index]!;
      if (/\s/u.test(character)) {
        index += 1;
        continue;
      }
      if (isLetter(character) || character === "_") {
        let end = index + 1;
        while (end < text.length && (isLetterOrNumber(text[end]!) || text[end] === "_")) end += 1;
        result.push({ kind: "name", value: text.slice(index, end) });
        index = end;
        continue;
      }
      if (character === '"' || character === "'") {
        const quote = character;
        let end = index + 1;
        while (end < text.length) {
          if (text[end] === "\\") end += 2;
          else if (text[end] === quote) {
            end += 1;
            break;
          } else end += 1;
        }
        if (end > text.length || text[end - 1] !== quote)
          throw new Error("unterminated EBNF literal");
        result.push({ kind: "literal", value: text.slice(index + 1, end - 1) });
        index = end;
        continue;
      }
      if ("=;|,()[]{}".includes(character)) {
        result.push({ kind: character, value: character });
        index += 1;
        continue;
      }
      throw new Error(`unexpected EBNF character ${JSON.stringify(character)}`);
    }
    return result;
  }

  private peek(kind?: string): EbnfToken | undefined {
    const found = this.tokens[this.index];
    return !kind || found?.kind === kind ? found : undefined;
  }

  private take(kind: string): string {
    const found = this.peek(kind);
    if (!found) throw new Error(`expected EBNF ${kind}, got ${JSON.stringify(this.peek())}`);
    this.index += 1;
    return found.value;
  }

  productions(): ReadonlyMap<string, EbnfNode> {
    const result = new Map<string, EbnfNode>();
    while (this.peek()) {
      const name = this.take("name");
      this.take("=");
      result.set(name, this.alternatives(new Set([";"])));
      this.take(";");
    }
    return result;
  }

  private alternatives(end: ReadonlySet<string>): EbnfNode {
    const choices = [this.sequence(new Set([...end, "|"]))];
    while (this.peek("|")) {
      this.take("|");
      choices.push(this.sequence(new Set([...end, "|"])));
    }
    return choices.length === 1 ? choices[0]! : { children: choices, kind: "alternative" };
  }

  private sequence(end: ReadonlySet<string>): EbnfNode {
    const parts = [this.factor()];
    while (this.peek(",")) {
      this.take(",");
      parts.push(this.factor());
    }
    const found = this.peek();
    if (found && !end.has(found.kind))
      throw new Error(`missing EBNF comma before ${JSON.stringify(found)}`);
    return parts.length === 1 ? parts[0]! : { children: parts, kind: "sequence" };
  }

  private factor(): EbnfNode {
    const found = this.peek();
    if (!found) throw new Error("unexpected end of EBNF");
    if (found.kind === "name") {
      this.index += 1;
      return { kind: "symbol", value: found.value };
    }
    if (found.kind === "literal") {
      this.index += 1;
      return { kind: "literal", value: found.value };
    }
    const wrappers = new Map<string, readonly [string, EbnfNode["kind"]]>([
      ["(", [")", "group"]],
      ["[", ["]", "optional"]],
      ["{", ["}", "repeat"]],
    ]);
    const wrapper = wrappers.get(found.kind);
    if (!wrapper) throw new Error(`unexpected EBNF token ${JSON.stringify(found)}`);
    this.index += 1;
    const child = this.alternatives(new Set([wrapper[0]]));
    this.take(wrapper[0]);
    return { children: [child], kind: wrapper[1] };
  }
}

class GrammarCompiler {
  private readonly rules = new Map<string, string[][]>();
  private counter = 0;

  compile(productions: ReadonlyMap<string, EbnfNode>): Grammar {
    for (const [name, node] of productions) this.rules.set(name, this.compileNode(node));
    this.rules.set("string_expression", [["string_literal"]]);
    return new Map(
      [...this.rules].map(([name, alternatives]) => [
        name,
        [...new Map(alternatives.map((rhs) => [rhs.join("\u0000"), rhs])).values()],
      ]),
    );
  }

  private helper(prefix: string): string {
    this.counter += 1;
    return `@${prefix}_${this.counter}`;
  }

  private compileNode(node: EbnfNode): string[][] {
    if (node.kind === "symbol" || node.kind === "literal") return [[node.value!]];
    if (node.kind === "alternative")
      return node.children!.flatMap((child) => this.compileNode(child));
    if (node.kind === "sequence") {
      let result: string[][] = [[]];
      for (const child of node.children!) {
        const additions = this.compileNode(child);
        result = result.flatMap((left) => additions.map((right) => [...left, ...right]));
      }
      return result;
    }
    if (node.kind === "group") return this.compileNode(node.children![0]!);
    const name = this.helper(node.kind);
    const compiled = this.compileNode(node.children![0]!);
    this.rules.set(
      name,
      node.kind === "optional" ? [[], ...compiled] : [[], ...compiled.map((rhs) => [...rhs, name])],
    );
    return [[name]];
  }
}

function chapterGrammar(): Grammar {
  const chapter = readFileSync(resolve(import.meta.dirname, "..", "02-grammar.md"), "utf8");
  const blocks = [...chapter.matchAll(/^```ebnf\s*\n(.*?)^```/gms)].map((match) => match[1]!);
  return new GrammarCompiler().compile(new EbnfReader(blocks.join("\n")).productions());
}

const grammar = chapterGrammar();

function itemKey(item: ChartItem): string {
  return `${item.lhs}\u0001${item.rhs.join("\u0002")}\u0001${item.dot}\u0001${item.origin}`;
}

function addItem(items: Map<string, ChartItem>, item: ChartItem): boolean {
  const key = itemKey(item);
  if (items.has(key)) return false;
  items.set(key, item);
  return true;
}

export function earleyAccepts(tokens: readonly GrammarToken[]): ParseResult {
  const chart = Array.from({ length: tokens.length + 1 }, () => new Map<string, ChartItem>());
  addItem(chart[0]!, { dot: 0, lhs: "@root", origin: 0, rhs: ["source_file"] });
  let farthest = 0;
  for (let position = 0; position < chart.length; position += 1) {
    const agenda = [...chart[position]!.values()];
    for (let cursor = 0; cursor < agenda.length; cursor += 1) {
      const item = agenda[cursor]!;
      if (item.dot < item.rhs.length) {
        const symbol = item.rhs[item.dot]!;
        const alternatives = grammar.get(symbol);
        if (alternatives) {
          for (const rhs of alternatives) {
            const predicted = { dot: 0, lhs: symbol, origin: position, rhs };
            if (addItem(chart[position]!, predicted)) agenda.push(predicted);
          }
        }
      } else {
        for (const waiting of chart[item.origin]!.values()) {
          if (waiting.dot < waiting.rhs.length && waiting.rhs[waiting.dot] === item.lhs) {
            const completed = { ...waiting, dot: waiting.dot + 1 };
            if (addItem(chart[position]!, completed)) agenda.push(completed);
          }
        }
      }
    }
    if (position >= tokens.length) continue;
    for (const item of chart[position]!.values()) {
      if (item.dot < item.rhs.length && tokens[position]!.kinds.has(item.rhs[item.dot]!))
        addItem(chart[position + 1]!, { ...item, dot: item.dot + 1 });
    }
    if (chart[position + 1]!.size > 0) farthest = position + 1;
  }
  const accepted = chart[tokens.length]!.has(
    itemKey({ dot: 1, lhs: "@root", origin: 0, rhs: ["source_file"] }),
  );
  return { accepted, farthest };
}
