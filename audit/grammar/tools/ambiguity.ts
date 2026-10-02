// Grammar audit tool: an Earley parser that COUNTS derivations of the chapter
// 02 EBNF instead of only recognizing it, and reports the smallest ambiguous
// node of every input with more than one parse.
//
// It reads the ```ebnf fences of spec/lang/02-grammar.md and lexes source
// text with its own layout lexer (lexer.ts). The `fixture` and `cases` modes
// also run the compiler's `parse` through the command contract
// (spec/conformance/README.md): $HD_TEST_COMMAND, else
// `node --experimental-strip-types bin/hd.js`. Imports: Node built-ins, this
// folder, and spec/ only.
//
//   node --experimental-strip-types audit/grammar/tools/ambiguity.ts probe FILE|-e SRC
//   node --experimental-strip-types audit/grammar/tools/ambiguity.ts fixtures
//   node --experimental-strip-types audit/grammar/tools/ambiguity.ts generate [N] [SEED]
//   node --experimental-strip-types audit/grammar/tools/ambiguity.ts firstfollow
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { repoRoot, Rng, splitCommand } from "../../../spec/tools/fuzz/common.ts";
import { Generator, loadGrammar, type Node } from "../../../spec/tools/fuzz/generate.ts";
import { lexSource } from "./lexer.ts";

interface Tok {
  readonly kinds: ReadonlySet<string>;
  readonly text: string;
  readonly line: number;
}

type Rhs = readonly string[];
type Bnf = Map<string, Rhs[]>;

// ---------------------------------------------------------------- EBNF -> BNF

function toBnf(ebnf: ReadonlyMap<string, Node>): Bnf {
  const rules: Bnf = new Map();
  const counters = new Map<string, number>();
  const helper = (owner: string, kind: string): string => {
    const n = (counters.get(owner) ?? 0) + 1;
    counters.set(owner, n);
    return `${owner}~${kind}${n}`;
  };
  const compile = (owner: string, node: Node): string[][] => {
    switch (node.kind) {
      case "lit":
        // Quoted, so a literal never aliases a production of the same name
        // (a literal "type" and the `type` rule are different symbols).
        return [[`'${node.value}'`]];
      case "sym":
        return [[node.value]];
      case "alt":
        return node.items.flatMap((item) => compile(owner, item));
      case "seq": {
        let out: string[][] = [[]];
        for (const item of node.items) {
          const add = compile(owner, item);
          out = out.flatMap((left) => add.map((right) => [...left, ...right]));
        }
        return out;
      }
      case "opt": {
        const name = helper(owner, "opt");
        rules.set(name, [[], ...compile(owner, node.item)]);
        return [[name]];
      }
      case "rep": {
        const name = helper(owner, "rep");
        rules.set(name, [[], ...compile(owner, node.item).map((rhs) => [...rhs, name])]);
        return [[name]];
      }
    }
  };
  for (const [name, node] of ebnf) rules.set(name, compile(name, node));
  // Simplification: string interpolation is lexical.
  rules.set("string_expression", [["string_literal"], ["prefixed_string_literal"]]);
  for (const [name, alts] of rules) {
    const seen = new Map(alts.map((rhs) => [rhs.join(" "), rhs]));
    rules.set(name, [...seen.values()]);
  }
  return rules;
}

const ebnf = loadGrammar();
const bnf = toBnf(ebnf);

function nullableSet(rules: Bnf): Set<string> {
  const out = new Set<string>();
  for (let changed = true; changed;) {
    changed = false;
    for (const [name, alts] of rules)
      if (!out.has(name) && alts.some((rhs) => rhs.every((s) => out.has(s)))) {
        out.add(name);
        changed = true;
      }
  }
  return out;
}
const nullable = nullableSet(bnf);

// ------------------------------------------------------------- Earley + count

/** A terminal symbol is a quoted literal or a token class such as `identifier`. */
function scans(token: Tok, symbol: string): boolean {
  if (bnf.has(symbol)) return false;
  if (symbol.startsWith("'") && symbol.length > 1) return token.kinds.has(symbol.slice(1, -1));
  return token.kinds.has(symbol);
}

interface Item {
  readonly lhs: string;
  readonly rhs: Rhs;
  readonly dot: number;
  readonly origin: number;
}

interface Parse {
  readonly accepted: boolean;
  readonly farthest: number;
  /** completed[end] : key "lhs\u0001origin" -> rhs list completed there. */
  readonly completed: Map<string, Rhs[]>[];
}

const key = (i: Item): string =>
  `${i.lhs}\u0001${i.rhs.join("\u0002")}\u0001${i.dot}\u0001${i.origin}`;

export function earley(tokens: readonly Tok[], start = "source_file"): Parse {
  const chart = Array.from({ length: tokens.length + 1 }, () => new Map<string, Item>());
  const completed = Array.from({ length: tokens.length + 1 }, () => new Map<string, Rhs[]>());
  const add = (set: Map<string, Item>, item: Item): boolean => {
    const k = key(item);
    if (set.has(k)) return false;
    set.set(k, item);
    return true;
  };
  add(chart[0]!, { dot: 0, lhs: "@root", origin: 0, rhs: [start] });
  let farthest = 0;
  for (let pos = 0; pos < chart.length; pos += 1) {
    const agenda = [...chart[pos]!.values()];
    for (let c = 0; c < agenda.length; c += 1) {
      const item = agenda[c]!;
      if (item.dot < item.rhs.length) {
        const sym = item.rhs[item.dot]!;
        const alts = bnf.get(sym);
        if (alts) {
          for (const rhs of alts) {
            const p = { dot: 0, lhs: sym, origin: pos, rhs };
            if (add(chart[pos]!, p)) agenda.push(p);
          }
          if (nullable.has(sym)) {
            const s = { ...item, dot: item.dot + 1 };
            if (add(chart[pos]!, s)) agenda.push(s);
          }
        }
      } else {
        const ck = `${item.lhs}\u0001${item.origin}`;
        const list = completed[pos]!.get(ck) ?? [];
        if (!list.includes(item.rhs)) list.push(item.rhs);
        completed[pos]!.set(ck, list);
        for (const w of chart[item.origin]!.values())
          if (w.dot < w.rhs.length && w.rhs[w.dot] === item.lhs) {
            const n = { ...w, dot: w.dot + 1 };
            if (add(chart[pos]!, n)) agenda.push(n);
          }
      }
    }
    if (pos >= tokens.length) continue;
    for (const item of chart[pos]!.values())
      if (item.dot < item.rhs.length && scans(tokens[pos]!, item.rhs[item.dot]!))
        add(chart[pos + 1]!, { ...item, dot: item.dot + 1 });
    if (chart[pos + 1]!.size > 0) farthest = pos + 1;
  }
  const accepted = (completed[tokens.length]!.get(`${start}\u00010`) ?? []).length > 0;
  return { accepted, completed, farthest };
}

interface Way {
  readonly rhs: Rhs;
  /** Child spans: for each rhs symbol, [from, to). */
  readonly spans: readonly (readonly [number, number])[];
}

class Forest {
  private readonly memo = new Map<string, number>();
  private readonly waysMemo = new Map<string, Way[]>();
  readonly tokens: readonly Tok[];
  readonly parse: Parse;
  constructor(tokens: readonly Tok[], parse: Parse) {
    this.tokens = tokens;
    this.parse = parse;
  }

  private ends(sym: string, from: number, to: number): number[] {
    if (!bnf.has(sym))
      return this.tokens[from] && scans(this.tokens[from]!, sym) && from + 1 <= to
        ? [from + 1]
        : [];
    const out: number[] = [];
    for (let e = from; e <= to; e += 1)
      if ((this.parse.completed[e]!.get(`${sym}\u0001${from}`) ?? []).length > 0) out.push(e);
    return out;
  }

  /** Every decomposition of (sym, from, to) whose children all derive. */
  ways(sym: string, from: number, to: number): Way[] {
    const k = `${sym}@${from}:${to}`;
    const hit = this.waysMemo.get(k);
    if (hit) return hit;
    const out: Way[] = [];
    this.waysMemo.set(k, out); // cycle guard
    for (const rhs of this.parse.completed[to]!.get(`${sym}\u0001${from}`) ?? []) {
      const walk = (index: number, at: number, spans: [number, number][]): void => {
        if (out.length > 64) return;
        if (index === rhs.length) {
          if (at === to) out.push({ rhs, spans: [...spans] });
          return;
        }
        const child = rhs[index]!;
        for (const e of this.ends(child, at, to)) {
          if (bnf.has(child) && this.count(child, at, e) === 0) continue;
          spans.push([at, e]);
          walk(index + 1, e, spans);
          spans.pop();
        }
      };
      walk(0, from, []);
    }
    return out;
  }

  count(sym: string, from: number, to: number): number {
    if (!bnf.has(sym))
      return this.tokens[from] && scans(this.tokens[from]!, sym) && to === from + 1 ? 1 : 0;
    const k = `${sym}@${from}:${to}`;
    const hit = this.memo.get(k);
    if (hit !== undefined) return hit;
    this.memo.set(k, 0); // cycle guard
    let total = 0;
    for (const way of this.ways(sym, from, to)) {
      let product = 1;
      way.rhs.forEach((child, i) => {
        product *= this.count(child, way.spans[i]![0], way.spans[i]![1]);
      });
      total = Math.min(1e9, total + product);
    }
    this.memo.set(k, total);
    return total;
  }

  /** Smallest ambiguous nodes reachable from the root. */
  ambiguousNodes(start = "source_file"): { sym: string; from: number; to: number; ways: Way[] }[] {
    const found = new Map<string, { sym: string; from: number; to: number; ways: Way[] }>();
    const seen = new Set<string>();
    const visit = (sym: string, from: number, to: number): void => {
      if (!bnf.has(sym)) return;
      const k = `${sym}@${from}:${to}`;
      if (seen.has(k)) return;
      seen.add(k);
      const ways = this.ways(sym, from, to);
      if (ways.length > 1) found.set(k, { from, sym, to, ways });
      for (const way of ways)
        way.rhs.forEach((c, i) => visit(c, way.spans[i]![0], way.spans[i]![1]));
    };
    visit(start, 0, this.tokens.length);
    // Every node here is locally ambiguous: it has more than one
    // decomposition, not merely an ambiguous child.
    return [...found.values()].sort((a, b) => a.from - b.from || b.to - a.to);
  }

  text(from: number, to: number): string {
    return this.tokens
      .slice(from, to)
      .map((t) => t.text)
      .join(" ");
  }

  /** Render one derivation of a node as a bracketed tree, helpers inlined. */
  tree(sym: string, from: number, to: number, depth = 0): string {
    if (!bnf.has(sym)) return this.tokens[from]!.text;
    if (from === to) return "";
    const way = this.ways(sym, from, to)[0];
    if (!way) return "?";
    const parts = way.rhs
      .map((c, i) => this.tree(c, way.spans[i]![0], way.spans[i]![1], depth + 1))
      .filter(Boolean);
    const body = parts.join(" ");
    if (sym.includes("~") || depth > 6 || to - from === 1) return body;
    return `${sym}[${body}]`;
  }

  /** Abstract shape of a decomposition, for grouping: symbols, helpers inlined. */
  shape(way: Way, depth: number): string {
    return way.rhs
      .map((c, i) => {
        const [a, b] = way.spans[i]!;
        if (a === b) return "";
        if (!bnf.has(c)) return c.replace(/^'(.+)'$/, "$1");
        const inner = this.ways(c, a, b)[0];
        if (c.includes("~")) return inner ? this.shape(inner, depth) : c;
        if (depth <= 0 || !inner) return c;
        return `${c}(${this.shape(inner, depth - 1)})`;
      })
      .filter(Boolean)
      .join(" ");
  }

  describeWay(way: Way): string {
    return way.rhs
      .map((c, i) => {
        const [a, b] = way.spans[i]!;
        if (a === b) return null;
        return bnf.has(c) ? this.tree(c, a, b, 1) : this.tokens[a]!.text;
      })
      .filter(Boolean)
      .join("  ");
  }
}

// ------------------------------------------------------------------ reporting

function namedOwner(sym: string): string {
  return sym.split("~")[0]!;
}

export function signature(forest: Forest, n: { sym: string; ways: Way[] }): string {
  const alts = [...new Set(n.ways.map((w) => forest.shape(w, 2)))];
  return `${namedOwner(n.sym)} :: ${alts.sort().join(" | ")}`;
}

export function analyze(tokens: readonly Tok[], start = "source_file") {
  const parse = earley(tokens, start);
  if (!parse.accepted) return { accepted: false as const, farthest: parse.farthest };
  const forest = new Forest(tokens, parse);
  const total = forest.count(start, 0, tokens.length);
  const nodes = total > 1 ? forest.ambiguousNodes(start) : [];
  return { accepted: true as const, forest, nodes, total };
}

function report(label: string, tokens: readonly Tok[]): string[] {
  const result = analyze(tokens);
  if (!result.accepted) {
    const t = tokens[Math.min(result.farthest, tokens.length - 1)]!;
    return [`${label}: REJECT at line ${t.line} near ${JSON.stringify(t.text)}`];
  }
  if (result.total <= 1) return [];
  const out = [`${label}: ${result.total} parses`];
  for (const n of result.nodes) {
    out.push(
      `  ambiguous ${n.sym} [line ${tokens[n.from]?.line}] "${result.forest.text(n.from, n.to)}"`,
    );
    out.push(`    signature: ${signature(result.forest, n)}`);
    for (const w of n.ways.slice(0, 4)) out.push(`    - ${result.forest.describeWay(w)}`);
  }
  return out;
}

function lexTokens(source: string): { tokens: Tok[]; diagnostics: string[] } {
  const { tokens, diagnostics } = lexSource(source);
  return { diagnostics: diagnostics.map((d) => `${d.code}:${d.line}`), tokens };
}

const compiler = splitCommand(
  process.env.HD_TEST_COMMAND ?? "node --experimental-strip-types bin/hd.js",
);

/** The compiler's `parse` verdict on `source`: `CODE:LINE` per located error, or none. */
function compilerParse(source: string): string[] {
  const directory = mkdtempSync(join(tmpdir(), "hd-amb-"));
  const file = join(directory, "case.hd");
  try {
    writeFileSync(file, source);
    const result = spawnSync(compiler[0]!, [...compiler.slice(1), "parse", file], {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    if (result.status === 0) return [];
    const located = /^\S*case\.hd:(\d+):\d+: ([a-z0-9-]+):/;
    const codes = `${result.stdout}\n${result.stderr}`
      .split("\n")
      .map((line) => located.exec(line))
      .filter((match) => match !== null)
      .map((match) => `${match[2]}:${match[1]}`);
    return codes.length ? codes : [`exit-${result.status ?? result.signal}`];
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
}

// ------------------------------------------------------------------ modes

const specRoot = resolve(import.meta.dirname, "../../../spec");

function modeFixtures(): void {
  const manifest = readFileSync(join(specRoot, "conformance/cases.tsv"), "utf8")
    .split("\n")
    .slice(1);
  const bySig = new Map<string, string[]>();
  let checked = 0;
  for (const row of manifest) {
    const [path, phase, expectation] = row.split("\t");
    if (!path) continue;
    if (phase === "parse" && expectation !== "accept") continue;
    const source = readFileSync(join(specRoot, "conformance", path), "utf8");
    const { tokens, diagnostics } = lexTokens(source);
    if (diagnostics.length) continue;
    checked += 1;
    const result = analyze(tokens);
    if (!result.accepted) {
      const t = tokens[Math.min(result.farthest, tokens.length - 1)]!;
      const list = bySig.get("REJECT") ?? [];
      list.push(`${path}:${t.line} near ${t.text}`);
      bySig.set("REJECT", list);
      continue;
    }
    for (const n of result.nodes) {
      const sig = signature(result.forest, n);
      const list = bySig.get(sig) ?? [];
      list.push(`${path}:${tokens[n.from]?.line}  "${result.forest.text(n.from, n.to)}"`);
      bySig.set(sig, list);
    }
  }
  console.log(`fixtures checked: ${checked}; ambiguity signatures: ${bySig.size}`);
  for (const [sig, list] of [...bySig].sort((a, b) => b[1].length - a[1].length)) {
    console.log(`\n[${list.length}] ${sig}`);
    for (const l of list.slice(0, 3)) console.log(`    ${l}`);
  }
}

const reservedWords = new Set(
  "Self annotate break continue data defer else enum false fn for if impl in is let match mut nil pass pub return self trait true type while".split(
    " ",
  ),
);
const pools: Record<string, string> = {
  "'a'": "char_literal",
  "0.5": "float_literal",
  "1.0": "float_literal",
  "1e10": "float_literal",
  "1e309": "float_literal",
  "0.0": "float_literal",
};

function kindsOf(text: string): Set<string> {
  const kinds = new Set([text]);
  if (["NEWLINE", "INDENT", "DEDENT", "SUITE_END", "EOF"].includes(text)) return kinds;
  if (text === "true" || text === "false") kinds.add("boolean_literal");
  if (text === "nil") kinds.add("nil_literal");
  if (/^[0-9][0-9_]*$/.test(text)) kinds.add("integer_literal");
  if (pools[text]) kinds.add(pools[text]!);
  if (text.startsWith("'")) kinds.add("char_literal");
  if (text.startsWith(`"`)) kinds.add("string_literal");
  if (/^[\p{L}_][\p{L}\p{N}_]*"/u.test(text)) kinds.add("prefixed_string_literal");
  if (/^[\p{L}_][\p{L}\p{N}_]*$/u.test(text) && text !== "_" && !reservedWords.has(text))
    kinds.add("identifier");
  return kinds;
}

function modeGenerate(count: number, seed: string): void {
  const generator = new Generator(ebnf);
  const bySig = new Map<string, { n: number; best: string[] }>();
  let rejects = 0;
  const starts = ["source_file"];
  for (let i = 0; i < count; i += 1) {
    const rng = new Rng(`${seed}:${i}`);
    const start = starts[i % starts.length]!;
    const raw = generator.derive(rng, start, 10 + rng.int(40));
    const tokens: Tok[] = raw.map((text) => ({ kinds: kindsOf(text), line: 0, text }));
    const result = analyze(tokens, start);
    if (!result.accepted) {
      rejects += 1;
      continue;
    }
    for (const n of result.nodes) {
      const sig = signature(result.forest, n);
      const text = result.forest.text(n.from, n.to);
      const entry = bySig.get(sig) ?? { best: [], n: 0 };
      entry.n += 1;
      const lines = [text, ...n.ways.slice(0, 3).map((w) => `  - ${result.forest.describeWay(w)}`)];
      if (!entry.best.length || text.length < entry.best[0]!.length) entry.best = lines;
      bySig.set(sig, entry);
    }
  }
  console.log(
    `generated: ${count}; not accepted (generator artefacts): ${rejects}; signatures: ${bySig.size}`,
  );
  for (const [sig, { n, best }] of [...bySig].sort((a, b) => b[1].n - a[1].n)) {
    console.log(`\n[${n}] ${sig}\n    smallest: ${best[0]}`);
    for (const l of best.slice(1)) console.log(`    ${l}`);
  }
}

// FIRST / FOLLOW over the BNF, then LL(1)-style conflicts on selected tokens.
function modeFirstFollow(focus: readonly string[]): void {
  const first = new Map<string, Set<string>>();
  const firstOf = (seq: readonly string[]): { set: Set<string>; nullable: boolean } => {
    const set = new Set<string>();
    for (const s of seq) {
      if (!bnf.has(s)) {
        set.add(s);
        return { nullable: false, set };
      }
      for (const t of first.get(s) ?? []) set.add(t);
      if (!nullable.has(s)) return { nullable: false, set };
    }
    return { nullable: true, set };
  };
  for (const name of bnf.keys()) first.set(name, new Set());
  for (let changed = true; changed;) {
    changed = false;
    for (const [name, alts] of bnf)
      for (const rhs of alts)
        for (const t of firstOf(rhs).set)
          if (!first.get(name)!.has(t)) {
            first.get(name)!.add(t);
            changed = true;
          }
  }
  const follow = new Map<string, Set<string>>([...bnf.keys()].map((n) => [n, new Set<string>()]));
  follow.get("source_file")!.add("EOF");
  for (let changed = true; changed;) {
    changed = false;
    for (const [name, alts] of bnf)
      for (const rhs of alts)
        rhs.forEach((s, i) => {
          if (!bnf.has(s)) return;
          const rest = firstOf(rhs.slice(i + 1));
          const target = follow.get(s)!;
          const addAll = (from: Iterable<string>): void => {
            for (const t of from)
              if (!target.has(t)) {
                target.add(t);
                changed = true;
              }
          };
          addAll(rest.set);
          if (rest.nullable) addAll(follow.get(name)!);
        });
  }
  const lines: string[] = [];
  for (const [name, alts] of bnf) {
    // alternative/alternative conflicts
    const sets = alts.map((rhs) => {
      const f = firstOf(rhs);
      if (f.nullable) for (const t of follow.get(name)!) f.set.add(t);
      return f.set;
    });
    for (const raw of focus) {
      const tok = /^[A-Z_]+$/.test(raw) ? raw : `'${raw}'`;
      const hits = alts.filter((_, i) => sets[i]!.has(tok));
      if (hits.length > 1)
        lines.push(`${tok}\t${name}\t${hits.map((r) => r.join(" ") || "ε").join("  |  ")}`);
    }
  }
  console.log(lines.sort().join("\n"));
}

const [mode, ...rest] = process.argv.slice(2);
if (mode === "probe") {
  const source =
    rest[0] === "-e" ? rest[1]!.replaceAll("\\n", "\n") : readFileSync(rest[0]!, "utf8");
  const { tokens, diagnostics } = lexTokens(source);
  if (diagnostics.length) console.log(`lexer diagnostics: ${diagnostics.join(", ")}`);
  if (rest.includes("--tokens"))
    console.log(
      tokens
        .map((t) => (t.kinds.size > 1 ? `${t.text}{${[...t.kinds].join("|")}}` : t.text))
        .join(" "),
    );
  const lines = report("input", tokens);
  console.log(lines.length ? lines.join("\n") : "input: exactly one parse");
} else if (mode === "fixture") {
  // The compiler's parse diagnostics of fixture files next to their marker line.
  for (const file of rest) {
    const source = readFileSync(file, "utf8");
    const marker =
      source.split("\n").findIndex((l) => /# (diagnostic|warning|panic): /.test(l)) + 1;
    const diags = compilerParse(source);
    console.log(`${file}: marker ${marker || "-"}; compiler ${diags.join(", ") || "accept"}`);
  }
} else if (mode === "cases") {
  // A case file holds many sources, each introduced by a line `#### NAME`.
  // For each: the compiler's parse diagnostics, then the derivation count.
  for (const file of rest) {
    const chunks = readFileSync(file, "utf8")
      .split(/^#### /m)
      .slice(1);
    for (const chunk of chunks) {
      const name = chunk.slice(0, chunk.indexOf("\n")).trim();
      const source = chunk.slice(chunk.indexOf("\n") + 1);
      const diags = compilerParse(source);
      const { tokens } = lexTokens(source);
      const lines = report(name, tokens);
      const verdict = diags.length ? `compiler: ${diags.join(", ")}` : "compiler: accept";
      console.log(`${name}: ${verdict}`);
      for (const line of lines) if (!line.includes("REJECT")) console.log(`  ${line}`);
    }
  }
} else if (mode === "tokens") {
  // Token-level probe: `tokens START "t1 t2 ..."`; layout tokens spelled NEWLINE etc.
  const start = rest[0]!;
  const tokens: Tok[] = rest[1]!
    .split(/\s+/)
    .filter(Boolean)
    .map((text) => ({ kinds: kindsOf(text), line: 0, text }));
  const result = analyze(tokens, start);
  if (!result.accepted)
    console.log(`REJECT near token ${result.farthest}: ${tokens[result.farthest]?.text}`);
  else if (result.total <= 1) console.log("exactly one parse");
  else {
    console.log(`${result.total} parses`);
    for (const n of result.nodes) {
      console.log(`  ambiguous ${n.sym} "${result.forest.text(n.from, n.to)}"`);
      for (const w of n.ways.slice(0, 4)) console.log(`    - ${result.forest.describeWay(w)}`);
    }
  }
} else if (mode === "fixtures") modeFixtures();
else if (mode === "generate") modeGenerate(Number(rest[0] ?? 2000), rest[1] ?? "a1");
else if (mode === "firstfollow")
  modeFirstFollow(
    rest.length
      ? rest
      : [
          ":",
          "[",
          "$",
          "!",
          "?",
          "<",
          "NEWLINE",
          "INDENT",
          "SUITE_END",
          "else",
          "for",
          "if",
          "...",
        ],
  );
else {
  console.error(
    "usage: ambiguity.ts probe FILE|-e SRC [--tokens] | fixtures | generate [N] [SEED] | firstfollow [TOK...]",
  );
  process.exitCode = 2;
}
