// A deterministic generator of hd packages for the metrics.
//
// `generatePackage({ seed, lines })` returns the files of one package of
// about `lines` lines: modules `src/m000.hd`, `src/m001.hd`, ... with data,
// enums, traits, impls, matches, `?`, loops, and a `tests:` block each, plus
// `src/lib.hd`, one integration test and `hd.toml`. The same seed and size
// always give byte-identical files.
//
// Each module carries two edit sites the metrics use:
// - a private body edit: the line `    let bump = +N` in `fn tweakK`;
// - a public signature edit: `pub fn shapeK(x: i32) -> i32:`, which gains a
//   defaulted parameter, so its callers still type-check.
// Module K calls `shape(K-1)` and `total(K-1)`, so module K+1 is the direct
// dependent of module K.

export interface GeneratedModule {
  /** The module path segment, as `m007`. */
  readonly name: string;
  /** The file, relative to the package root, as `src/m007.hd`. */
  readonly file: string;
  /** Modules this module imports. */
  readonly imports: readonly string[];
  /** The name of one test case in this module, unique in the package. */
  readonly testName: string;
}

export interface GeneratedPackage {
  readonly files: ReadonlyMap<string, string>;
  readonly modules: readonly GeneratedModule[];
  readonly lines: number;
}

export const SIZES = { small: 1_000, "10k": 10_000, "50k": 50_000 } as const;
export type SizeName = keyof typeof SIZES;

/** A 32-bit FNV-1a hash of a string, to seed the generator. */
export function hashSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < seed.length; index++) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** mulberry32: a small, fast, deterministic PRNG over 0..1. */
export function makeRandom(seed: string): () => number {
  let state = hashSeed(seed);
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS = [
  "pen",
  "cup",
  "lamp",
  "desk",
  "book",
  "coat",
  "shoe",
  "ring",
  "card",
  "seed",
  "tile",
  "rope",
];

const pad = (index: number): string => String(index).padStart(3, "0");
export const moduleName = (index: number): string => `m${pad(index)}`;

interface Random {
  int(low: number, high: number): number;
  pick<T>(items: readonly T[]): T;
}

function wrap(next: () => number): Random {
  return {
    int: (low, high) => low + Math.floor(next() * (high - low + 1)),
    pick: (items) => items[Math.floor(next() * items.length)]!,
  };
}

/** Helper function bodies; each takes the module index and a helper index. */
const HELPERS: readonly ((k: number, h: number, r: Random) => string[])[] = [
  (k, h, r) => [
    `fn step${k}_${h}(x: i32, y: i32) -> i32:`,
    "    let acc = x",
    "    for i in +0..y:",
    `        if i % ${r.int(2, 7)} == 0:`,
    "            acc = acc + i",
    "        else:",
    `            acc = acc - ${r.int(1, 9)}`,
    "    acc",
  ],
  (k, h, r) => [
    `fn classify${k}_${h}(n: i32) -> string:`,
    "    if n < 0:",
    '        "negative"',
    "    else if n == 0:",
    '        "zero"',
    `    else if n > ${r.int(10, 900)}:`,
    '        "large ${n}"',
    "    else:",
    '        "small ${n}"',
  ],
  (k, h, r) => [
    `fn collect${k}_${h}(limit: i32) -> List[i32]:`,
    "    let out: mut List[i32] = []",
    "    for i in +0..limit:",
    `        out.push(i * ${r.int(2, 9)})`,
    "    out",
  ],
  (k, h, r) => [
    `fn describe${k}_${h}(item: Item${k}) -> string:`,
    `    tag := if item.count > ${r.int(1, 20)}: "bulk" else: "single"`,
    '    "${item.name} x${item.count} (${tag})"',
  ],
  (k, h, r) => [
    `fn priced${k}_${h}(items: List[Item${k}], name: string) -> i64?:`,
    `    item := find${k}(items, name)?`,
    `    item.cost() + ${r.int(1, 99)}`,
  ],
  (k, h, r) => [
    `fn weight${k}_${h}(status: Status${k}) -> i32:`,
    "    match status:",
    `        .Pending => ${r.int(0, 9)}`,
    `        .Paid(amount) => i32(amount % ${r.int(3, 50)})`,
    "        .Shipped(code, days) => days + i32(code.len())",
    "        .Cancelled(_) => -1",
  ],
  (k, h, r) => [
    `fn sum_words${k}_${h}(words: List[string]) -> usize:`,
    "    let total: usize = 0",
    "    for word in words:",
    `        if word.len() > ${r.int(1, 6)}:`,
    "            total = total + word.len()",
    "    total",
  ],
];

function generateModule(k: number, r: Random, deps: readonly number[]): string[] {
  const word = r.pick(WORDS);
  const lines: string[] = [
    `# Module ${k}: line items, order states, and their labels.`,
    "use std.testing.assert_equal",
  ];
  for (const dep of deps)
    lines.push(`use pkg.${moduleName(dep)}.{Item${dep}, shape${dep}, total${dep}}`);
  lines.push(
    "",
    `## A line item of module ${k}.`,
    `pub data Item${k}:`,
    "    pub name: string",
    "    pub price: i64",
    "    pub count: i32",
    "",
    "## The state of an order.",
    "@derive(Eq)",
    `pub enum Status${k}:`,
    "    Pending",
    "    Paid(amount: i64)",
    "    Shipped(code: string, days: i32)",
    "    Cancelled(reason: string)",
    "",
    "## Something with a cost.",
    `pub trait Costed${k}:`,
    "    fn cost(self) -> i64",
    "",
    `impl Costed${k} for Item${k}:`,
    "    fn cost(self) -> i64:",
    "        self.price * i64(self.count)",
    "",
    "## The label of a status.",
    `pub fn label${k}(status: Status${k}) -> string:`,
    "    match status:",
    '        .Pending => "pending"',
    '        .Paid(amount) => "paid ${amount}"',
    '        .Shipped(code, days) => "shipped ${code} in ${days} days"',
    '        .Cancelled(reason) => "cancelled: ${reason}"',
    "",
    "## The total cost of the items.",
    `pub fn total${k}(items: List[Item${k}]) -> i64:`,
    "    let sum: i64 = 0",
    "    for item in items:",
    "        sum = sum + item.cost()",
    "    sum",
    "",
    "## A public function whose signature the recheck metric edits.",
    `pub fn shape${k}(x: i32) -> i32:`,
    `    x * ${r.int(2, 9)} + tweak${k}(x)`,
    "",
    `fn tweak${k}(x: i32) -> i32:`,
    "    let bump = +1",
    "    x + bump",
    "",
    `fn find${k}(items: List[Item${k}], name: string) -> Item${k}?:`,
    "    for item in items:",
    "        if item.name == name:",
    "            return item",
    "    .None",
    "",
  );
  for (const dep of deps)
    lines.push(
      `## Uses module ${dep}.`,
      `pub fn bridge${k}_${dep}(items: List[Item${dep}]) -> i64:`,
      `    total${dep}(items) + i64(shape${dep}(${r.int(1, 9)}))`,
      "",
    );
  const helperCount = r.int(6, 12);
  for (let h = 0; h < helperCount; h++) lines.push(...r.pick(HELPERS)(k, h, r), "");
  lines.push(
    "tests:",
    `    it("t${pad(k)}a totals items"):`,
    `        items := [Item${k} { name: "${word}", price: 3, count: 2 }]`,
    `        assert_equal(total${k}(items), 6, reason="price times count")`,
    "",
    `    it("t${pad(k)}b labels a status"):`,
    `        assert_equal(label${k}(Status${k}.Paid(5)), "paid 5", reason="paid label")`,
    "",
  );
  return lines;
}

export interface GenerateOptions {
  readonly seed: string;
  /** The approximate line count of the package. */
  readonly lines: number;
  /** The package name in hd.toml. Default `gen`. */
  readonly name?: string;
}

export function generatePackage(options: GenerateOptions): GeneratedPackage {
  const r = wrap(makeRandom(options.seed));
  const files = new Map<string, string>();
  const modules: GeneratedModule[] = [];
  let total = 0;
  for (let k = 0; total < options.lines - 20; k++) {
    const deps = k === 0 ? [] : k >= 3 && r.int(0, 1) === 1 ? [k - 1, k - 3] : [k - 1];
    const text = generateModule(k, r, deps);
    total += text.length;
    const file = `src/${moduleName(k)}.hd`;
    files.set(file, text.join("\n"));
    modules.push({
      name: moduleName(k),
      file,
      imports: deps.map(moduleName),
      testName: `t${pad(k)}a`,
    });
  }
  const lib = [
    "# The package's front door.",
    "use pkg.m000.{Item0, total0}",
    "",
    "## The total of one item.",
    "pub fn one_total(item: Item0) -> i64:",
    "    total0([item])",
    "",
  ];
  const integration = [
    "use pkg.{one_total}",
    "use pkg.m000.Item0",
    "use std.testing.assert_equal",
    "",
    'it("integration totals one item"):',
    '    assert_equal(one_total(Item0 { name: "pen", price: 2, count: 3 }), 6, reason="one item")',
    "",
  ];
  files.set("src/lib.hd", lib.join("\n"));
  files.set("tests/smoke.hd", integration.join("\n"));
  files.set("hd.toml", `[package]\nname = "${options.name ?? "gen"}"\n`);
  total += lib.length + integration.length + 2;
  return { files, modules, lines: total };
}

/** The dependents of module `name`: the modules that import it directly. */
export const dependentsOf = (pkg: GeneratedPackage, name: string): GeneratedModule[] =>
  pkg.modules.filter((module) => module.imports.includes(name));

/** Edits the private body of `tweakK`: `let bump = +N` becomes +(N+1). */
export function editBody(text: string): string {
  const next = text.replace(
    /^( {4}let bump = \+)(\d+)$/m,
    (_, head: string, n: string) => `${head}${Number(n) + 1}`,
  );
  if (next === text) throw new Error("no body edit site in the module");
  return next;
}

/** Edits the public signature of `shapeK`: it gains a defaulted parameter. */
export function editSignature(text: string): string {
  const next = text.replace(
    /^(pub fn shape\d+\(x: i32)\) -> i32:$/m,
    "$1, extra: i32 = 0) -> i32:",
  );
  if (next === text) throw new Error("no signature edit site in the module");
  return next;
}
