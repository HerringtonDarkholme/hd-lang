// Checked-in, implementation-neutral runtime cases for issue #1336: the
// per-object loans of docs/future-work/POINTER_LOANS.md, as reference kind
// x path x lifetime x alias action, each with the verdict the proposal
// gives (runs, or panics `borrow-conflict`).
// Run: node spec/tools/generate-pointer-loan-matrix.ts [--check]
import { existsSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
const stem = "pointer-loan-";
const section = "lang/13-values-and-references.md#conflicts-through-aliases";

/** The primary section: a std rule when the source imports a std item. */
function sectionOf(source: string): string {
  const cited: [string, string][] = [
    ["use std.collections.UniqList", "std/collections.md#r-std-collections.uniq-list.type"],
    ["use std.collections.UniqMap", "std/collections.md#r-std-collections.uniq-map.type"],
    ["use std.weak.", "std/weak.md#r-std-weak.upgrade.readonly"],
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

/** The proposal's verdict: true when the case panics `borrow-conflict`. */
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
 * The other columns (POINTER_LOANS.md, "Two Models"): whether
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
# Proposal: docs/future-work/POINTER_LOANS.md (issue #1336), not yet accepted.
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
const extras: { name: string; panic: boolean; what: string; source: string }[] = [
  {
    name: "own-path-other-field",
    panic: false,
    what: "a write to another field through the same pointer the reference crossed counts the reference's own loan",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn probe(p: *mut Cell) -> string:
    r := &p.value
    p.other = fresh()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    seen := probe(box(Cell { value: orig(), other: fresh() }))
${tail}`,
  },
  {
    name: "own-path-loop-other-field",
    panic: false,
    what: "a loop over a list behind a pointer writes another field through the same pointer",
    source: `struct Bag:
    items: List[string]
    count: usize

${helpers}
fn probe(p: *mut Bag) -> string:
    let seen = ""
    for item in &p.items:
        p.count += 1
        seen = *item
    seen

pub fn main() -> void $ Console:
    seen := probe(box(Bag { items: [orig()], count: 0 }))
${tail}`,
  },
  {
    name: "root-pointer-last-use-before-reference",
    panic: false,
    what: "the local holding a reference's root pointer stays alive until the reference's last use",
    source: `use std.collections.UniqList

struct Outer:
    items: UniqList[string]

${helpers}
fn uniq() -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(orig())
    items

fn probe() -> string:
    p := box(Outer { items: uniq() })
    r := p.items.get_ref(0).expect("slot")
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    seen := probe()
${tail}`,
  },
  {
    name: "root-pointer-moved-into-a-task",
    panic: false,
    what: "a task's pointer parameter stays alive across a suspension while a reference through it lives",
    source: `use std.collections.UniqList
use std.task.join
use std.time.{Clock, sleep}

struct Outer:
    items: UniqList[string]

${helpers}
fn uniq() -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(orig())
    items

fn probe!(p: *mut Outer) -> string $ Clock:
    r := p.items.get_ref(0).expect("slot")
    sleep!(2ms)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

fn idle!() -> string $ Clock:
    sleep!(1ms)
    "idle"

pub fn main!() -> void $ Console + Clock:
    p := box(Outer { items: uniq() })
    let (seen, _) = join!(probe(p), idle())
${tail}`,
  },
  {
    name: "callee-writes-other-field-panics",
    panic: true,
    what: "a callee writes another field of an object the caller's reference borrows",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn bump(alias: *mut Cell) -> void:
    alias.other = fresh() # panic: borrow-conflict

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    bump(alias)
    *r

pub fn main() -> void $ Console:
    p := box(Cell { value: orig(), other: fresh() })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "callee-only-reads",
    panic: false,
    what: "passing the pointer to a callee that only reads is no access",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn peek(alias: *mut Cell) -> usize:
    alias.other.len()

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    _ := peek(alias)
    *r

pub fn main() -> void $ Console:
    p := box(Cell { value: orig(), other: fresh() })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "later-argument-replaces-panics",
    panic: true,
    what: "a later argument replaces the object an earlier argument's reference points into",
    source: `struct Cell:
    value: string

struct Outer:
    next: *mut Cell

${helpers}
fn replace(alias: *mut Outer) -> usize:
    alias.next = box(Cell { value: fresh() }) # panic: borrow-conflict
    _ := churn()
    1

fn observe(r: &string, n: usize) -> string:
    *r

fn probe(p: *mut Outer, alias: *mut Outer) -> string:
    observe(&p.next.value, replace(alias))

pub fn main() -> void $ Console:
    p := box(Outer { next: box(Cell { value: orig() }) })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "receiver-argument-reads-the-receiver",
    panic: false,
    what: "an argument of a `&mut self` call reads the receiver's object through an alias, as `xs.push(xs.len())` does",
    source: `struct Bag:
    items: List[usize]

fn probe(p: *mut Bag, alias: *Bag) -> usize:
    p.items.push(alias.items.len())
    p.items[1]

pub fn main() -> void $ Console:
    p := box(Bag { items: [+0usize] })
    if probe(p, p) != 1:
        panic("the push read the wrong length")
    println("ok")
`,
  },
  {
    name: "captured-variable-grows-panics",
    panic: true,
    what: "a closure that shares a variable grows its list while a loop holds element references",
    source: `pub fn main() -> void $ Console:
    let mut xs = [+1, 2, 3]
    grow := fn() -> void:
        for round in 0..16:
            xs.push(4) # panic: borrow-conflict
    for x in &mut xs:
        grow()
        *x += 1
    println("\${xs.len()}")
`,
  },
  {
    name: "captured-variable-replaced-panics",
    panic: true,
    what: "a stored closure that shares a variable replaces it while a reference into it lives",
    source: `struct Hook:
    run: fn() -> void

${helpers}
pub fn main() -> void $ Console:
    let mut xs = [orig()]
    hook := Hook { run: fn() -> void: xs = [] } # panic: borrow-conflict
    r := &xs[0]
    (hook.run)()
    junk := churn()
    seen := *r
    _ := junk.len()
${tail}`,
  },
  {
    name: "deep-shared-loans",
    panic: false,
    what: "forty nested shared loans on one object, past an inline counter's width",
    source: `struct Cell:
    value: string

${helpers}
fn depth(p: *Cell, n: usize) -> usize:
    r := &p.value
    below := if n == 0: 0 else: depth(p, n - 1)
    r.len() + below

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    if depth(p, 40) != 41 * orig().len():
        panic("the nested reads disagree")
    println("ok")
`,
  },
  {
    name: "dyn-receiver-alias-write-panics",
    panic: true,
    what: "a `&mut self` call through a `*mut dyn` writes the object through an alias it was given",
    source: `trait Touch:
    fn touch(&mut self, alias: *mut Counter) -> void

struct Counter:
    n: i32

impl Touch for Counter:
    fn touch(&mut self, alias: *mut Counter) -> void:
        self.n += 1
        alias.n += 1 # panic: borrow-conflict

pub fn main() -> void $ Console:
    c := box(Counter { n: +0 })
    let t: *mut dyn Touch = c
    t.touch(c)
    println("\${c.n}")
`,
  },
  {
    name: "generic-argument-alias-write-panics",
    panic: true,
    what: "a generic callee writes through an alias of the object its `&mut T` argument points into",
    source: `trait Grow:
    fn grow(&mut self) -> void

struct Counter:
    n: i32

impl Grow for Counter:
    fn grow(&mut self) -> void:
        self.n += 1

fn twice[T < Grow](value: &mut T, alias: *mut Counter) -> void:
    value.grow()
    alias.n += 1 # panic: borrow-conflict
    value.grow()

fn probe(c: *mut Counter, alias: *mut Counter) -> void:
    twice(&mut *c, alias)

pub fn main() -> void $ Console:
    c := box(Counter { n: +0 })
    probe(c, c)
    println("\${c.n}")
`,
  },
  {
    name: "copy-under-element-loan-panics",
    panic: true,
    what: "an alias copies a list while a `&mut` element reference into it lives",
    source: `struct Bag:
    items: List[string]

${helpers}
fn probe(p: *mut Bag, alias: *Bag) -> string:
    r := &mut p.items[0]
    copy := alias.items # panic: borrow-conflict
    *r = fresh()
    copy[0]

pub fn main() -> void $ Console:
    p := box(Bag { items: [orig()] })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "sort-comparator-alias-write-panics",
    panic: true,
    what: "a comparator writes another field of the object whose list it sorts",
    source: `struct Bag:
    items: List[i32]
    calls: i32

fn probe(p: *mut Bag, alias: *mut Bag) -> void:
    compare := fn(left: &i32, right: &i32) -> Ordering:
        alias.calls += 1 # panic: borrow-conflict
        left.cmp(right)
    p.items.sort_by(compare)

pub fn main() -> void $ Console:
    p := box(Bag { items: [+3, 1, 2], calls: +0 })
    probe(p, p)
    println("\${p.calls}")
`,
  },
  {
    name: "observer-notify-writes-owner-panics",
    panic: true,
    what: "a listener loop over a list behind a pointer calls listeners that write the owner",
    source: `struct Bus:
    listeners: List[i32]
    total: i32

fn notify(bus: *mut Bus, weight: i32) -> void:
    bus.total += weight # panic: borrow-conflict

fn publish(bus: *mut Bus) -> void:
    for weight in &bus.listeners:
        notify(bus, *weight)

pub fn main() -> void $ Console:
    bus := box(Bus { listeners: [+1, 2], total: +0 })
    publish(bus)
    println("\${bus.total}")
`,
  },
  {
    name: "weak-upgrade-root",
    panic: false,
    what: "a reference through an upgraded weak pointer outlives the last strong holder's release",
    source: `use std.weak.{Weak, weak}

struct Cell:
    value: string

struct Holder:
    child: *Cell

${helpers}
fn probe(holder: *mut Holder, w: Weak[Cell]) -> string:
    match w.upgrade():
        .Some(strong) =>
            r := &strong.value
            holder.child = box(Cell { value: fresh() })
            junk := churn()
            seen := *r
            _ := junk.len()
            seen
        .None => ""

pub fn main() -> void $ Console:
    holder := box(Holder { child: box(Cell { value: orig() }) })
    w := weak(holder.child).expect("a whole object")
    seen := probe(holder, w)
${tail}`,
  },
  {
    name: "elide-callback-panics",
    panic: true,
    what: "a callback passed in writes the borrowed object; a function value has no summary",
    source: `struct Cell:
    value: string

${helpers}
fn probe(p: *mut Cell, alias: *mut Cell, act: fn(*mut Cell) -> void) -> string:
    r := &p.value
    act(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p, fn(c: *mut Cell) -> void: c.value = fresh()) # panic: borrow-conflict
${tail}`,
  },
  {
    name: "elide-dyn-call-panics",
    panic: true,
    what: "a trait-value call writes the borrowed object through a pointer it holds",
    source: `trait Touch:
    fn touch(&self) -> void

struct Cell:
    value: string

struct Toucher:
    target: *mut Cell

impl Touch for Toucher:
    fn touch(&self) -> void:
        self.target.value = fresh() # panic: borrow-conflict

${helpers}
fn probe(p: *mut Cell, t: &dyn Touch) -> string:
    r := &p.value
    t.touch()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    toucher := Toucher { target: p }
    seen := probe(p, &toucher)
${tail}`,
  },
  {
    name: "elide-writes-holder-type-panics",
    panic: true,
    what: "a callee writes only the type that holds the pointer the reference crossed",
    source: `struct Cell:
    value: string

struct Outer:
    next: *mut Cell

${helpers}
fn rewire(alias: *mut Outer) -> void:
    alias.next = box(Cell { value: fresh() }) # panic: borrow-conflict

fn probe(p: *mut Outer, alias: *mut Outer) -> string:
    r := &p.next.value
    rewire(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Outer { next: box(Cell { value: orig() }) })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "elide-writes-through-field-reference-panics",
    panic: true,
    what: "a callee takes `&mut` of the field of a type that contains the borrowed one",
    source: `struct Cell:
    value: string

struct Wrap:
    cell: Cell

${helpers}
fn fix(c: &mut Cell) -> void:
    c.value = fresh()

fn reset(alias: *mut Wrap) -> void:
    fix(&mut alias.cell) # panic: borrow-conflict

fn probe(w: *mut Wrap, alias: *mut Wrap) -> string:
    r := &w.cell.value
    reset(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    w := box(Wrap { cell: Cell { value: orig() } })
    seen := probe(w, w)
${tail}`,
  },
  {
    name: "elide-closure-captured-pointer-panics",
    panic: true,
    what: "a closure that captured the pointer writes the object",
    source: `struct Cell:
    value: string

${helpers}
fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    bump := fn() -> void: alias.value = fresh() # panic: borrow-conflict
    r := &p.value
    bump()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "elide-other-task-panics",
    panic: true,
    what: "another task replaces a `UniqList` while the reference waits across a bang call",
    source: `use std.collections.UniqList
use std.task.join
use std.time.{Clock, sleep}

struct Bag:
    items: UniqList[string]

${helpers}
fn uniq(value: string) -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(value)
    items

fn hold!(p: *mut Bag) -> string $ Clock:
    r := p.items.get_ref(0).expect("slot")
    sleep!(2ms)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

fn act!(alias: *mut Bag) -> string $ Clock:
    sleep!(1ms)
    alias.items = uniq(fresh()) # panic: borrow-conflict
    "acted"

pub fn main!() -> void $ Console + Clock:
    p := box(Bag { items: uniq(orig()) })
    let (seen, _) = join!(hold(p), act(p))
    println("\${p.items.len()}")
${tail}`,
  },
  {
    name: "elide-recursive-call-panics",
    panic: true,
    what: "a recursive callee writes the object at its deepest call",
    source: `struct Cell:
    value: string

${helpers}
fn walk(alias: *mut Cell, depth: i32) -> void:
    if depth == 0:
        alias.value = fresh() # panic: borrow-conflict
    else:
        walk(alias, depth - 1)

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    walk(alias, 3)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p)
${tail}`,
  },
  {
    name: "elide-erased-generic-panics",
    panic: true,
    what: "an erased generic method writes the object through an open `*mut T`",
    source: `trait Bump:
    fn bump(&mut self) -> void

trait Visitor:
    fn visit[T < Bump](&self, target: *mut T) -> void

struct Cell:
    value: string

impl Bump for Cell:
    fn bump(&mut self) -> void:
        self.value = fresh()

struct Bumper:
    times: i32

impl Visitor for Bumper:
    fn visit[T < Bump](&self, target: *mut T) -> void:
        target.bump() # panic: borrow-conflict

${helpers}
fn probe(p: *mut Cell, alias: *mut Cell, v: &dyn Visitor) -> string:
    r := &p.value
    v.visit(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    bumper := Bumper { times: +1 }
    seen := probe(p, p, &bumper)
${tail}`,
  },
  {
    name: "elide-read-then-print",
    panic: false,
    what: "a reference read and printed with no write in its live range",
    source: `struct Person:
    name: string

${helpers}
fn greet(p: *Person) -> void $ Console:
    n := &p.name
    println(*n)

pub fn main() -> void $ Console:
    p := box(Person { name: "ok" })
    greet(p)
`,
  },
  {
    name: "elide-getter-in-loop",
    panic: false,
    what: "a getter called in a loop that writes only another type",
    source: `struct Person:
    name: string

struct Log:
    lines: i32

impl Person:
    fn name_ref(&self) -> &string:
        &self.name

${helpers}
fn total(p: *Person, log: *mut Log) -> usize:
    let sum: usize = 0
    for round in 0..3:
        n := p.name_ref()
        log.lines += 1
        sum += n.len()
    sum

pub fn main() -> void $ Console:
    p := box(Person { name: orig() })
    log := box(Log { lines: +0 })
    seen := if total(p, log) == 3 * orig().len(): orig() else: fresh()
${tail}`,
  },
  {
    name: "elide-read-only-visitor",
    panic: false,
    what: "a read-only visitor called through a trait value over pointers",
    source: `trait Visitor:
    fn see(&mut self, name: &string) -> void

struct Shape:
    name: string

struct Scene:
    shapes: List[*Shape]

struct Counter:
    chars: usize

impl Visitor for Counter:
    fn see(&mut self, name: &string) -> void:
        self.chars += name.len()

${helpers}
fn walk(scene: *Scene, v: &mut dyn Visitor) -> void:
    for shape in &scene.shapes:
        n := &shape.name
        v.see(n)

pub fn main() -> void $ Console:
    scene := box(Scene { shapes: [box(Shape { name: orig() }), box(Shape { name: orig() })] })
    let mut counter = Counter { chars: 0 }
    walk(scene, &mut counter)
    seen := if counter.chars == 2 * orig().len(): orig() else: fresh()
${tail}`,
  },
  {
    name: "pass-read-with-shared",
    panic: false,
    what: "passing a pointer as a `*T` and a `&` of its field to one call: two reads",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn look(c: *Cell, f: &string) -> string:
    _ := c.other.len()
    *f

pub fn main() -> void $ Console:
    view := box(Cell { value: orig(), other: fresh() })
    seen := look(view, &view.value)
${tail}`,
  },
  {
    name: "pass-alias-mut-with-shared-panics",
    panic: true,
    what: "passing an alias as a `*mut` and a `&` of a field through another name",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn mutate(c: *mut Cell, f: &string) -> string:
    c.other = fresh() # panic: borrow-conflict
    *f

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    mutate(alias, &boxed.value)

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
${tail}`,
  },
  {
    name: "pass-alias-read-with-mut-panics",
    panic: true,
    what: "passing an alias as a `*T` and a `&mut` of a field through another name",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn inspect(c: *Cell, f: &mut string) -> string:
    _ := c.other.len() # panic: borrow-conflict
    *f

fn probe(view: *mut Cell, alias: *Cell) -> string:
    inspect(alias, &mut view.value)

pub fn main() -> void $ Console:
    view := box(Cell { value: orig(), other: fresh() })
    seen := probe(view, view)
${tail}`,
  },
  {
    name: "dependent-result-alias-panics",
    panic: true,
    what: "an alias writes the object while a call result that depends on the pointer lives",
    source: `struct Cell:
    value: string
    other: string

impl Cell:
    fn change(&mut self) -> void:
        self.other = fresh()

${helpers}
fn borrow(b: &Cell) -> &string:
    &b.value

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    r := borrow(boxed)
    alias.change() # panic: borrow-conflict
    *r

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
${tail}`,
  },
  {
    name: "hidden-alias-in-another-object-panics",
    panic: true,
    what: "a callee writes another field through an alias it reaches in another object while a reference argument holds a loan",
    source: `struct Cell:
    value: string
    other: string

struct Writer:
    cell: *mut Cell

${helpers}
fn older(writer: &Writer, seen: &string) -> string:
    writer.cell.other = fresh() # panic: borrow-conflict
    *seen

fn probe(boxed: *mut Cell, writer: &Writer) -> string:
    older(writer, &boxed.value)

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    writer := Writer { cell: boxed }
    seen := probe(boxed, &writer)
${tail}`,
  },
  {
    name: "pointer-lent-as-mut-argument-panics",
    panic: true,
    what: "a pointer passed where \`&mut\` is expected lends its whole object from its evaluation, so a later argument that writes through an alias panics",
    source: `struct Cell:
    value: string
    other: string

${helpers}
fn write_other(alias: *mut Cell) -> usize:
    alias.other = fresh() # panic: borrow-conflict
    1

fn observe(c: &mut Cell, n: usize) -> string:
    c.value

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    observe(boxed, write_other(alias))

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
${tail}`,
  },
];
/** Each adversarial case's columns: elision under loans (A), keep-alive (B). */
const extraColumns: Record<string, [string, string]> = {
  "own-path-other-field": ["loan needed", "runs"],
  "own-path-loop-other-field": ["loan needed", "runs"],
  "root-pointer-last-use-before-reference": ["elided", "runs"],
  "root-pointer-moved-into-a-task": ["elided", "runs"],
  "callee-writes-other-field-panics": ["loan needed", "runs"],
  "callee-only-reads": ["elided", "runs"],
  "later-argument-replaces-panics": ["loan needed", "runs, stale"],
  "receiver-argument-reads-the-receiver": ["loan needed", "runs"],
  "captured-variable-grows-panics": ["loan needed", "runs, stale"],
  "captured-variable-replaced-panics": ["loan needed", "runs, stale"],
  "deep-shared-loans": ["elided", "runs"],
  "dyn-receiver-alias-write-panics": ["loan needed", "runs, sees the write"],
  "generic-argument-alias-write-panics": ["loan needed", "runs, sees the write"],
  "copy-under-element-loan-panics": ["loan needed", "runs; the copy keeps its own elements"],
  "sort-comparator-alias-write-panics": ["loan needed", "runs"],
  "observer-notify-writes-owner-panics": ["loan needed", "runs"],
  "weak-upgrade-root": ["elided", "runs"],
  "elide-callback-panics": ["loan needed", "runs, sees the write"],
  "elide-dyn-call-panics": ["loan needed", "runs, sees the write"],
  "elide-writes-holder-type-panics": ["loan needed", "runs, stale"],
  "elide-writes-through-field-reference-panics": ["loan needed", "runs, sees the write"],
  "elide-closure-captured-pointer-panics": ["loan needed", "runs, sees the write"],
  "elide-other-task-panics": ["loan needed", "panics collection-pinned"],
  "elide-recursive-call-panics": ["loan needed", "runs, sees the write"],
  "elide-erased-generic-panics": ["loan needed", "runs, sees the write"],
  "elide-read-then-print": ["elided", "runs"],
  "elide-getter-in-loop": ["elided", "runs"],
  "elide-read-only-visitor": ["elided", "runs"],
  "pass-read-with-shared": ["elided", "runs"],
  "dependent-result-alias-panics": ["loan needed", "runs"],
  "pass-alias-mut-with-shared-panics": ["loan needed", "runs, sees the write"],
  "pass-alias-read-with-mut-panics": ["loan needed", "runs"],
  "hidden-alias-in-another-object-panics": ["loan needed", "runs"],
  "pointer-lent-as-mut-argument-panics": ["loan needed", "runs"],
};
for (const { name, panic, what, source } of extras) {
  const file = `${panic ? "runtime/panic" : "runtime/valid"}/${stem}extra-${name}.hd`;
  const [elision, go] = extraColumns[name];
  const contents = `# test: pointer loans, adversarial: ${what}; ${panic ? "panics borrow-conflict" : "runs"}
${panic ? "" : "# expect-stdout: ok\n"}# Generated by spec/tools/generate-pointer-loan-matrix.ts; edit the generator.
# Proposal: docs/future-work/POINTER_LOANS.md (issue #1336), not yet accepted.
# Verdicts: loans (A) with elision: ${elision}; keep-alive (B): ${go}.
${source}`;
  output(`conformance/${file}`, contents);
  rows.push(
    `${file}\truntime\t${panic ? "panic:borrow-conflict" : "accept"}\t${sectionOf(source)}`,
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
# Proposal: docs/future-work/POINTER_LOANS.md (issue #1336), not yet accepted.
${body.includes("UniqList") ? "use std.collections.UniqList\n\n" : ""}${sameRootDecls}
${body}`;
  output(`conformance/${file}`, contents);
  const rule = name.includes("-with-")
    ? "ref.excl.args.pointer"
    : code === "conflicting-access"
      ? "ref.excl.place"
      : "ref.dep.scope";
  const cited = body.includes("UniqList")
    ? "std/collections.md#r-std-collections.uniq-list.type"
    : `lang/13-values-and-references.md#r-${rule}`;
  rows.push(`${file}\ttype\treject:${code}\t${cited}`);
}

// Stale generated fixtures: a matrix file the generator no longer writes.
for (const dir of ["runtime/valid", "runtime/panic", "typing/invalid"]) {
  const full = resolve(root, "conformance", dir);
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
// Before the erased-call matrix, whose generator keeps its rows last.
const erased = index.findIndex((line) => line.startsWith("runtime/valid/erased-call-matrix-"));
index.splice(erased === -1 ? index.length : erased, 0, ...rows);
output(indexPath, `${index.join("\n")}\n`);
console.log(`${check ? "checked" : "generated"} ${rows.length} pointer-loan cases`);
