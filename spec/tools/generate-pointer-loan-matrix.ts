// Checked-in, implementation-neutral runtime cases for issue #1336: the
// per-object loans of docs/compiler/ownership.md#pointer-loans-model-a, as reference kind
// x path x lifetime x alias action, each with the verdict the accepted design
// gives (runs, or panics `borrow-conflict`).
// Run: node spec/tools/generate-pointer-loan-matrix.ts [--check]
import { existsSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { extraColumns, extras as extraSources } from "./pointer-loan-extras.ts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
const stem = "pointer-loan-";
const section = "lang/13-values-and-references.md#conflicts-through-aliases";

/** The primary section: a std rule when the source imports a std item. */
function sectionOf(source: string): string {
  const cited: [string, string][] = [
    ["use std.collections.UniqList", "std/collections.md#r-std-collections.uniq-list.type"],
    ["use std.collections.UniqMap", "std/collections.md#r-std-collections.uniq-map.type"],
    ["use std.time.", "std/time.md#r-std-time.helper.sleep"],
  ];
  return cited.find(([use]) => source.includes(use))?.[1] ?? section;
}
const rows: string[] = [];
const written = new Set<string>();

type Kind = "ref" | "mut";
type Path = "local" | "ptr" | "field" | "ptr2" | "list" | "map" | "ulist" | "umap";
type Life = "expr" | "stmt" | "arg" | "ret" | "struct" | "loop" | "bang" | "closure";
type Action = "read" | "write" | "other" | "elem" | "mut" | "replace" | "grow" | "shrink" | "drop";

const kinds: Kind[] = ["ref", "mut"];
const paths: Path[] = ["local", "ptr", "field", "ptr2", "list", "map", "ulist", "umap"];
const lives: Life[] = ["expr", "stmt", "arg", "ret", "struct", "loop", "bang", "closure"];
const collections: Path[] = ["list", "map", "ulist", "umap"];

/** The alias actions each path has, under one lifetime. */
function actions(path: Path, life: Life): Action[] {
  if (path === "local") {
    return ["write"];
  }
  const structural: Action = path === "ptr" ? "drop" : "replace";
  if (life === "expr") {
    return [collections.includes(path) ? "grow" : structural];
  }
  if (life !== "stmt") {
    return collections.includes(path) ? ["read", "grow", "replace"] : ["read", "write", structural];
  }
  const all: Action[] = ["read", "write", "other", "mut"];
  const tails: Record<string, Action[]> = {
    ptr: ["drop"],
    field: ["replace", "drop"],
    ptr2: ["replace"],
  };
  return [...all, ...(tails[path] ?? ["elem", "replace", "grow", "shrink"])];
}

/** The accepted design's verdict: true when the case panics `borrow-conflict`. */
function panics(kind: Kind, path: Path, life: Life, action: Action): boolean {
  if (path === "local" || life === "expr" || action === "drop") {
    return false;
  }
  return action !== "read" || kind === "mut";
}

const words = {
  path: {
    local: "a field of a local struct",
    ptr: "a field of a boxed struct",
    field: "a field of a struct field behind a `*T`",
    ptr2: "a field behind a `*T` then a `*T`",
    list: "a `List` element behind a `*T`",
    map: "a `Map` value behind a `*T`",
    ulist: "a `UniqList` element behind a `*T`",
    umap: "a `UniqMap` value behind a `*T`",
  },
  life: {
    expr: "used within one expression",
    stmt: "held across a statement",
    arg: "passed as a call argument",
    ret: "returned by a method",
    struct: "stored in a lifetime struct",
    loop: "held across a loop iteration",
    bang: "held across a bang call that suspends and acts",
    closure: "captured by a closure",
  },
  action: {
    read: "reads the place",
    write: "writes the place",
    other: "writes another field of the same object",
    elem: "writes another element of the same collection",
    mut: "takes `&mut` of the place",
    replace: "replaces what holds the place",
    grow: "grows the collection",
    shrink: "removes the referenced entry",
    drop: "drops the last other pointer to the object",
  },
};

/** The type the reference's root pointer refers to. */
function rootType(path: Path): string {
  return path === "ptr" || path === "local" ? "Cell" : "Outer";
}

function decls(path: Path): string {
  const cell = "struct Cell:\n    value: string\n    other: string\n";
  const imports: Record<string, string> = {
    ulist: "use std.collections.UniqList\n\n",
    umap: "use std.collections.UniqMap\n\n",
  };
  const uses = imports[path] ?? "";
  const fields: Record<Path, string> = {
    local: "",
    ptr: "",
    field:
      "struct Inner:\n    value: string\n\nstruct Outer:\n    inner: Inner\n    other: string\n",
    ptr2: `${cell}\nstruct Outer:\n    next: *mut Cell\n    other: string\n`,
    list: "struct Outer:\n    items: List[string]\n    other: string\n",
    map: "struct Outer:\n    map: Map[string, string]\n    other: string\n",
    ulist: "struct Outer:\n    items: UniqList[string]\n    other: string\n",
    umap: "struct Outer:\n    map: UniqMap[string, string]\n    other: string\n",
  };
  const base = path === "local" || path === "ptr" ? cell : fields[path];
  const make: Record<Path, string> = {
    local: "box(Cell { value: orig(), other: second() })",
    ptr: "box(Cell { value: orig(), other: second() })",
    field: "box(Outer { inner: Inner { value: orig() }, other: second() })",
    ptr2: "box(Outer { next: box(Cell { value: orig(), other: second() }), other: second() })",
    list: "box(Outer { items: [second(), orig()], other: second() })",
    map: 'box(Outer { map: { "a": orig(), "b": second() }, other: second() })',
    ulist: "box(Outer { items: uniq(second(), orig()), other: second() })",
    umap: "box(Outer { map: uniq_map(orig(), second()), other: second() })",
  };
  const helpers: Record<Path, string> = {
    local: "",
    ptr: "",
    field: "",
    ptr2: "",
    list: "",
    map: "",
    ulist:
      "fn uniq(first: string, last: string) -> UniqList[string]:\n    let mut items = UniqList::[string].new()\n    items.push(first)\n    items.push(last)\n    items\n\n",
    umap: 'fn uniq_map(a: string, b: string) -> UniqMap[string, string]:\n    let mut map = UniqMap::[string, string].new()\n    _ := map.insert("a", a)\n    _ := map.insert("b", b)\n    map\n\n',
  };
  return `${uses}${base}
fn orig() -> string:
    "orig-" + "z".repeat(40)

fn second() -> string:
    "second-" + "y".repeat(40)

fn fresh() -> string:
    "w-" + "x".repeat(40)

fn set(slot: &mut string, value: string) -> void:
    *slot = value

# Allocates blocks the size of a freed string, so a read of freed memory
# sees other bytes.
fn churn() -> List[string]:
    let mut junk: List[string] = []
    for round in 0..8:
        junk.push("j".repeat(44 + round))
    junk

${helpers[path]}fn make() -> *mut ${rootType(path)}:
    ${make[path]}
`;
}

/** The place the reference names, from root `r`. */
function place(path: Path, root: string): string {
  return {
    local: `${root}.value`,
    ptr: `${root}.value`,
    field: `${root}.inner.value`,
    ptr2: `${root}.next.value`,
    list: `${root}.items[1]`,
    map: `${root}.map["a"]`,
    ulist: "",
    umap: "",
  }[path];
}

/** The reference expression of kind `kind` from root `root`. */
function reference(kind: Kind, path: Path, root: string): string {
  if (path === "ulist") {
    return `${root}.items.${kind === "mut" ? "get_mut" : "get_ref"}(1).expect("slot")`;
  }
  if (path === "umap") {
    return `${root}.map.${kind === "mut" ? "get_mut" : "get_ref"}("a").expect("slot")`;
  }
  return `${kind === "mut" ? "&mut " : "&"}${place(path, root)}`;
}

/** The alias statement, through `alias`. */
function act(path: Path, action: Action): string {
  const target = path === "local" ? "ptr" : path;
  const table: Record<string, string> = {
    "ptr:read": "seen_alias := alias.value",
    "field:read": "seen_alias := alias.inner.value",
    "ptr2:read": "seen_alias := alias.next.value",
    "list:read": "seen_alias := alias.items[1]",
    "map:read": 'seen_alias := alias.map["a"]',
    "ulist:read": 'seen_alias := alias.items.get(1).unwrap_or("")',
    "umap:read": 'seen_alias := alias.map.get("a").unwrap_or("")',
    "ptr:write": "alias.value = fresh()",
    "field:write": "alias.inner.value = fresh()",
    "ptr2:write": "alias.next.value = fresh()",
    "list:write": "alias.items[1] = fresh()",
    "map:write": 'alias.map["a"] = fresh()',
    "ulist:write": "alias.items.swap(0, 1)",
    "umap:write": '_ := alias.map.insert("a", fresh())',
    "ptr:other": "alias.other = fresh()",
    "field:other": "alias.other = fresh()",
    "ptr2:other": "alias.next.other = fresh()",
    "list:other": "alias.other = fresh()",
    "map:other": "alias.other = fresh()",
    "ulist:other": "alias.other = fresh()",
    "umap:other": "alias.other = fresh()",
    "list:elem": "alias.items[0] = fresh()",
    "map:elem": 'alias.map["b"] = fresh()',
    "ulist:elem": 'set(alias.items.get_mut(0).expect("first"), fresh())',
    "umap:elem": '_ := alias.map.insert("b", fresh())',
    "ptr:mut": "set(&mut alias.value, fresh())",
    "field:mut": "set(&mut alias.inner.value, fresh())",
    "ptr2:mut": "set(&mut alias.next.value, fresh())",
    "list:mut": "set(&mut alias.items[1], fresh())",
    "map:mut": 'set(&mut alias.map["a"], fresh())',
    "ulist:mut": 'set(alias.items.get_mut(1).expect("slot"), fresh())',
    "umap:mut": 'set(alias.map.get_mut("a").expect("slot"), fresh())',
    "field:replace": "alias.inner = Inner { value: fresh() }",
    "ptr2:replace": "alias.next = box(Cell { value: fresh(), other: fresh() })",
    "list:replace": "alias.items = [fresh(), fresh()]",
    "map:replace": 'alias.map = { "a": fresh() }',
    "ulist:replace": "alias.items = uniq(fresh(), fresh())",
    "umap:replace": "alias.map = uniq_map(fresh(), fresh())",
    "list:grow": "for round in 0..16:\n    alias.items.push(fresh())",
    "map:grow": 'for round in 0..16:\n    alias.map["z$round"] = fresh()',
    "ulist:grow": "for round in 0..16:\n    alias.items.push(fresh())",
    "umap:grow": 'for round in 0..16:\n    _ := alias.map.insert("z$round", fresh())',
    "list:shrink": "_ := alias.items.remove_at(1)",
    "map:shrink": '_ := alias.map.remove("a")',
    "ulist:shrink": "_ := alias.items.remove_at(1)",
    "umap:shrink": '_ := alias.map.remove("a")',
    "ptr:drop": "holder.child = make()",
    "field:drop": "holder.child = make()",
  };
  const text = table[`${target}:${action}`];
  if (text === undefined) {
    throw new Error(`no action ${target}:${action}`);
  }
  return text;
}

/** The type of the alias parameter. */
function aliasType(path: Path, action: Action): string {
  if (action === "drop") {
    return "*mut Holder";
  }
  return `${action === "read" ? "*" : "*mut "}${rootType(path)}`;
}

/** What one case's generated parts need to know. */
interface Case {
  kind: Kind;
  path: Path;
  life: Life;
  action: Action;
  panic: boolean;
  aliasName: string;
  aliasParam: string;
  refType: string;
  rootDecl: string;
  root: string;
  rootParam: string;
  /** The alias statement, then a churn of freed blocks, at an indent. */
  acting: (indent: string) => string;
}

function makeCase(kind: Kind, path: Path, life: Life, action: Action): Case {
  const panic = panics(kind, path, life, action);
  const marker = panic ? " # panic: borrow-conflict" : "";
  const statement = act(path, action);
  const reads = action === "read";
  const local = path === "local";
  const aliasName = action === "drop" ? "holder" : "alias";
  // A grow is a loop, whose last line carries the marker.
  const acting = (indent: string) => {
    const lines = statement.split("\n").map((line) => `${indent}${line}`);
    const read = reads ? `\n${indent}_ := seen_alias.len()` : "";
    return `${lines.join("\n")}${marker}\n${indent}junk := churn()${read}`;
  };
  return {
    kind,
    path,
    life,
    action,
    panic,
    aliasName,
    aliasParam: `${aliasName}: ${aliasType(path, action)}`,
    refType: `${kind === "mut" ? "&mut " : "&"}string`,
    rootDecl: local ? "    let mut own = Cell { value: orig(), other: second() }\n" : "",
    root: local ? "own" : "p",
    rootParam: local ? "" : `p: *mut ${rootType(path)}, `,
    acting,
  };
}

/** The helper declarations and the `probe` function of each lifetime. */
const shapes: Record<Life, (cs: Case) => { extra: string; probe: string }> = {
  expr: (cs) => {
    const name = cs.kind === "mut" ? "touch" : "length";
    return {
      extra: `fn ${name}(slot: ${cs.refType}) -> string:\n    *slot\n\n`,
      probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    seen := ${name}(${reference(cs.kind, cs.path, cs.root)})\n${cs.acting("    ")}\n    _ := junk.len()\n    seen\n`,
    };
  },
  stmt: (cs) => ({
    extra: "",
    probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    r := ${reference(cs.kind, cs.path, cs.root)}\n${cs.acting("    ")}\n    seen := *r\n    _ := junk.len()\n    seen\n`,
  }),
  loop: (cs) => ({
    extra: "",
    probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    r := ${reference(cs.kind, cs.path, cs.root)}\n    let seen = ""\n    for round in 0..2:\n        if round == 1:\n${cs.acting("            ")}\n            _ := junk.len()\n        seen = *r\n    seen\n`,
  }),
  arg: (cs) => ({
    extra: `fn observe(r: ${cs.refType}, ${cs.aliasParam}) -> string:\n${cs.acting("    ")}\n    seen := *r\n    _ := junk.len()\n    seen\n\n`,
    probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    observe(${reference(cs.kind, cs.path, cs.root)}, ${cs.aliasName})\n`,
  }),
  ret: (cs) => {
    const self = cs.kind === "mut" ? "&mut self" : "&self";
    const getter = cs.kind === "mut" ? "slot_mut" : "slot_ref";
    return {
      extra: `impl ${rootType(cs.path)}:\n    fn ${getter}(${self}) -> ${cs.refType}:\n        ${reference(cs.kind, cs.path, "self")}\n\n`,
      probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    r := ${cs.root}.${getter}()\n${cs.acting("    ")}\n    seen := *r\n    _ := junk.len()\n    seen\n`,
    };
  },
  struct: (cs) => {
    const view = cs.kind === "mut" ? "ViewMut" : "View";
    return {
      extra: `struct ${view}['a]:\n    r: ${cs.kind === "mut" ? "&'a mut" : "&'a"} string\n\n`,
      probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    view := ${view} { r: ${reference(cs.kind, cs.path, cs.root)} }\n${cs.acting("    ")}\n    seen := *view.r\n    _ := junk.len()\n    seen\n`,
    };
  },
  closure: (cs) => ({
    extra: `fn apply(${cs.aliasParam}, read: fn() -> string) -> string:\n${cs.acting("    ")}\n    seen := read()\n    _ := junk.len()\n    seen\n\n`,
    probe: `fn probe(${cs.rootParam}${cs.aliasParam}) -> string:\n${cs.rootDecl}    r := ${reference(cs.kind, cs.path, cs.root)}\n    apply(${cs.aliasName}, fn() -> string: *r)\n`,
  }),
  bang: (cs) => ({
    extra: `fn act!(${cs.aliasParam}) -> string $ Clock:\n    sleep!(1ms)\n${cs.acting("    ")}\n    _ := junk.len()\n    "acted"\n\n`,
    probe: `fn probe!(${cs.rootParam}${cs.aliasParam}) -> string $ Clock:\n${cs.rootDecl}    r := ${reference(cs.kind, cs.path, cs.root)}\n    _ := act!(${cs.aliasName})\n    seen := *r\n    seen\n`,
  }),
};

/** The entry point: it builds the objects, probes, and checks the value read. */
function entry(cs: Case): string {
  const expected = cs.panic
    ? "!(seen == orig() || seen == second() || seen == fresh())"
    : "seen != orig()";
  const message = cs.panic ? "a value no write stored" : "another value than its original";
  const verify = `    if ${expected}:\n        panic("the reference read ${message}")\n`;
  let setup = "    p := make()\n";
  let args = "p, p";
  if (cs.action === "drop") {
    setup = "    holder := box(Holder { child: make() })\n";
    args = "holder.child, holder";
  } else if (cs.path === "local") {
    setup = "    other := make()\n";
    args = "other";
  }
  if (cs.life === "bang") {
    return `pub fn main!() -> void $ Console + Clock:\n${setup}    seen := probe!(${args})\n${verify}    println("ok")\n`;
  }
  return `pub fn main() -> void $ Console:\n${setup}    seen := probe(${args})\n${verify}    println("ok")\n`;
}

/**
 * The other columns (git history at 7ccbb4ea8, POINTER_LOANS.md, "Two Models"): whether
 * per-reference elision removes model A's loan, and model B's outcome.
 */
function elisionOf(kind: Kind, path: Path, life: Life, action: Action): string {
  if (path === "local") {
    return "no *T";
  }
  const free = life === "expr" || action === "drop" || (action === "read" && kind === "ref");
  return free ? "elided" : "loan needed";
}

function goOf(path: Path, life: Life, action: Action): string {
  if (path === "local" || life === "expr" || action === "drop" || action === "read") {
    return "runs";
  }
  const moves = ["grow", "replace", "shrink"].includes(action);
  if ((path === "ulist" || path === "umap") && moves) {
    return "panics collection-pinned";
  }
  // A moved-on collection or a replaced object leaves the reference on the
  // storage it counted; a store in place is seen.
  return moves && path !== "field" ? "runs, stale" : "runs, sees the write";
}

function columns(kind: Kind, path: Path, life: Life, action: Action): string {
  return `# Verdicts: loans (A) with elision: ${elisionOf(kind, path, life, action)}; keep-alive (B): ${goOf(path, life, action)}.\n`;
}

function generate(kind: Kind, path: Path, life: Life, action: Action) {
  const cs = makeCase(kind, path, life, action);
  const name = `${stem}${life}-${kind}-${path}-${action}${cs.panic ? "-panics" : ""}`;
  const file = `${cs.panic ? "runtime/panic" : "runtime/valid"}/${name}.hd`;
  const { extra, probe } = shapes[life](cs);
  let head = decls(path);
  if (life === "bang") {
    const uses = "use std.time.{Clock, sleep}\n";
    head = head.startsWith("use ") ? `${uses}${head}` : `${uses}\n${head}`;
  }
  const holder = action === "drop" ? `\nstruct Holder:\n    child: *mut ${rootType(path)}\n` : "";
  const verdict = cs.panic ? "panics borrow-conflict" : "runs";
  const lend = kind === "mut" ? "&mut" : "&";
  const title = `# test: pointer loans: a \`${lend}\` to ${words.path[path]}, ${words.life[life]}; an alias ${words.action[action]}; ${verdict}`;
  const contents = `${title}
${cs.panic ? "" : "# expect-stdout: ok\n"}# Generated by spec/tools/generate-pointer-loan-matrix.ts; edit the generator.
# Accepted design: docs/compiler/ownership.md#pointer-loans-model-a (2026-11-03; built under #1336).
${columns(kind, path, life, action)}${head}${holder}
${extra}${probe}
${entry(cs)}`;
  output(`conformance/${file}`, contents);
  rows.push(
    `${file}\truntime\t${cs.panic ? "panic:borrow-conflict" : "accept"}\t${sectionOf(contents)}`,
  );
}

function output(path: string, contents: string) {
  const full = resolve(root, path);
  written.add(full);
  if (check) {
    if (!existsSync(full) || readFileSync(full, "utf8") !== contents) {
      throw new Error(`generated file is stale: ${path}`);
    }
  } else {
    writeFileSync(full, contents);
  }
}

for (const life of lives) {
  for (const kind of kinds) {
    for (const path of paths) {
      for (const action of actions(path, life)) {
        generate(kind, path, life, action);
      }
    }
  }
}

// Adversarial cases beside the product: one source each, after the shared
// helpers of a `Cell` path.
const helpers = `fn orig() -> string:
    "orig-" + "z".repeat(40)

fn fresh() -> string:
    "w-" + "x".repeat(40)

fn churn() -> List[string]:
    let mut junk: List[string] = []
    for round in 0..8:
        junk.push("j".repeat(44 + round))
    junk
`;
const tail = `    if seen != orig():
        panic("the reference read another value than its original")
    println("ok")
`;
const extras = extraSources.map((extra) => ({
  ...extra,
  source: extra.source.replaceAll("@@HELPERS@@", helpers).replaceAll("@@TAIL@@", tail),
}));

for (const { name, panic, what, source, rule } of extras) {
  const file = `${panic ? "runtime/panic" : "runtime/valid"}/${stem}extra-${name}.hd`;
  const [elision, go] = extraColumns[name];
  const contents = `# test: pointer loans, adversarial: ${what}; ${panic ? "panics borrow-conflict" : "runs"}
${panic ? "" : "# expect-stdout: ok\n"}# Generated by spec/tools/generate-pointer-loan-matrix.ts; edit the generator.
# Accepted design: docs/compiler/ownership.md#pointer-loans-model-a (2026-11-03; built under #1336).
# Verdicts: loans (A) with elision: ${elision}; keep-alive (B): ${go}.
${source}`;
  output(`conformance/${file}`, contents);
  rows.push(
    `${file}\truntime\t${panic ? "panic:borrow-conflict" : "accept"}\t${rule ? `lang/13-values-and-references.md#r-${rule}` : sectionOf(source)}`,
  );
}

// Same-root shapes: two references, or a reference and an access, from
// one name. The checker sees them, so both models rejects them at
// compile time (`ref.excl.place`); only aliasing it cannot see reaches
// the run-time model.
const sameRootDecls = `struct Cell:
    n: i32

impl Cell:
    fn change(&mut self) -> void:
        self.n += 1
    fn read(&self) -> i32:
        self.n

struct Outer:
    next: *mut Cell
    list: List[i32]
    map: Map[string, i32]

fn bump(c: *mut Cell) -> void:
    c.n += 1
`;
const sameRoot: { name: string; what: string; body: string; code?: string }[] = [
  {
    name: "whole-object",
    what: "a `&mut` and a `&` of one boxed object",
    body: "fn probe(a: *mut Cell) -> i32:\n    b := &mut *a\n    c := &*a # diagnostic: conflicting-access\n    b.change()\n    c.read()\n",
  },
  {
    name: "field",
    what: "a `&mut` and a `&` of one field behind a pointer",
    body: "fn probe(a: *mut Cell) -> i32:\n    b := &mut a.n\n    c := &a.n # diagnostic: conflicting-access\n    *b += 1\n    *c\n",
  },
  {
    name: "method-write",
    what: "a `&mut self` call on the pointer while a field reference lives",
    body: "fn probe(a: *mut Cell) -> i32:\n    c := &a.n\n    a.change() # diagnostic: conflicting-access\n    *c\n",
  },
  {
    name: "pass-the-pointer",
    what: "passing the same pointer to a callee while a field reference lives",
    body: "fn probe() -> i32:\n    a := box(Cell { n: +0 })\n    c := &a.n\n    bump(a) # diagnostic: conflicting-access\n    *c\n",
  },
  {
    name: "nested-pointer",
    what: "replacing the inner pointer a reference crossed, through the same root",
    body: "fn probe(a: *mut Outer) -> i32:\n    c := &a.next.n\n    a.next = box(Cell { n: +1 }) # diagnostic: conflicting-access\n    *c\n",
  },
  {
    name: "list-push",
    what: "growing a list behind the pointer while an element reference lives",
    body: "fn probe(a: *mut Outer) -> i32:\n    c := &a.list[0]\n    a.list.push(2) # diagnostic: conflicting-access\n    *c\n",
  },
  {
    name: "map-store",
    what: "storing a new key in a map behind the pointer while a value reference lives",
    body: 'fn probe(a: *mut Outer) -> i32:\n    c := &a.map["k"]\n    a.map["z"] = 1 # diagnostic: conflicting-access\n    *c\n',
  },
  {
    name: "uniq-replace",
    what: "replacing a `UniqList` behind the pointer while an element reference lives",
    body: 'struct Bag:\n    items: UniqList[i32]\n\nfn probe(a: *mut Bag) -> i32:\n    c := a.items.get_ref(0).expect("slot")\n    a.items = UniqList::[i32].new() # diagnostic: conflicting-access\n    *c\n',
  },
  {
    name: "pass-mut-with-shared",
    what: "passing a `*mut` and a `&` of its field to one call",
    body: "fn mutate(c: *mut Cell, f: &i32) -> i32:\n    c.n + *f\n\nfn probe(boxed: *mut Cell) -> i32:\n    mutate(boxed, &boxed.n) # diagnostic: conflicting-access\n",
  },
  {
    name: "pass-mut-with-mut",
    what: "passing a `*mut` and a `&mut` of its field to one call",
    body: "fn mutate(c: *mut Cell, f: &mut i32) -> void:\n    *f += c.n\n\nfn probe(boxed: *mut Cell) -> void:\n    mutate(boxed, &mut boxed.n) # diagnostic: conflicting-access\n",
  },
  {
    name: "pass-read-with-mut",
    what: "passing a pointer as a `*T` and a `&mut` of its field to one call",
    body: "fn inspect(c: *Cell, f: &mut i32) -> void:\n    *f += c.n\n\nfn probe(view: *mut Cell) -> void:\n    inspect(view, &mut view.n) # diagnostic: conflicting-access\n",
  },
  {
    name: "dependent-call",
    what: "a `&mut self` call on the pointer while a call result that depends on it lives",
    body: "fn borrow(b: &Cell) -> &i32:\n    &b.n\n\nfn probe(boxed: *mut Cell) -> i32:\n    r := borrow(boxed)\n    boxed.change() # diagnostic: conflicting-access\n    *r\n",
  },
  {
    name: "dependent-method",
    what: "a write through the pointer while a method result that depends on it lives",
    body: "impl Cell:\n    fn slot(&self) -> &i32:\n        &self.n\n\nfn probe(boxed: *mut Cell) -> i32:\n    r := boxed.slot()\n    boxed.n += 1 # diagnostic: conflicting-access\n    *r\n",
  },
  {
    name: "observer-notify",
    what: "a listener loop over a list behind a pointer that passes the same pointer to each listener",
    body: "struct Bus:\n    listeners: List[i32]\n    total: i32\n\nfn notify(bus: *mut Bus, weight: i32) -> void:\n    bus.total += weight\n\nfn publish(bus: *mut Bus) -> void:\n    for weight in &bus.listeners:\n        notify(bus, *weight) # diagnostic: conflicting-access\n",
  },
  {
    name: "dependent-outlives-root",
    what: "a call result that depends on a local pointer, used after the local's block",
    code: "local-reference-outlives-local",
    body: "fn borrow(b: &Cell) -> &i32:\n    &b.n\n\nfn probe(fallback: &i32, pick: bool) -> i32:\n    let r: &i32 = fallback\n    if pick:\n        boxed := box(Cell { n: +1 })\n        r = borrow(boxed)\n    *r # diagnostic: local-reference-outlives-local\n",
  },
];
for (const { name, what, body, code = "conflicting-access" } of sameRoot) {
  const file = `typing/invalid/${stem}same-root-${name}.hd`;
  const contents = `# test: pointer loans, same root: ${what} is a compile-time error in both models
# Generated by spec/tools/generate-pointer-loan-matrix.ts; edit the generator.
# Accepted design: docs/compiler/ownership.md#pointer-loans-model-a (2026-11-03; built under #1336).
${body.includes("UniqList") ? "use std.collections.UniqList\n\n" : ""}${sameRootDecls}
${body}`;
  output(`conformance/${file}`, contents);
  let rule = "ref.dep.scope";
  if (name.includes("-with-")) {
    rule = "ref.excl.args.pointer";
  } else if (name.startsWith("dependent-")) {
    rule = "ref.dep.call.pointer";
  } else if (code === "conflicting-access") {
    rule = "ref.excl.place";
  }
  const cited = body.includes("UniqList")
    ? "std/collections.md#r-std-collections.uniq-list.type"
    : `lang/13-values-and-references.md#r-${rule}`;
  rows.push(`${file}\ttype\treject:${code}\t${cited}`);
}

// Stale generated fixtures: a matrix file the generator no longer writes.
for (const dir of ["runtime/valid", "runtime/panic", "typing/invalid"]) {
  const full = resolve(root, "conformance", dir);
  // cost: a generator run by hand; three fixed directories.
  for (const name of readdirSync(full)) {
    const path = resolve(full, name);
    if (name.startsWith(stem) && !written.has(path)) {
      if (check) {
        throw new Error(`stale generated file: conformance/${dir}/${name}`);
      }
      unlinkSync(path);
    }
  }
}

const indexPath = "conformance/cases.tsv";
const old = readFileSync(resolve(root, indexPath), "utf8");
const matrix = (line: string) =>
  ["runtime/valid", "runtime/panic", "typing/invalid"].some((dir) =>
    line.startsWith(`${dir}/${stem}`),
  );
const index = old.split("\n").filter((line) => line && !matrix(line));
index.push(...rows);
output(indexPath, `${index.join("\n")}\n`);
console.log(`${check ? "checked" : "generated"} ${rows.length} pointer-loan cases`);
