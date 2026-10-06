// Compile-time stress inputs for the `pathological` metric.
//
// Each case generates one single-file hd program at a scale of 1, 2 or 4.
// The program must type-check. Its budget applies at scale 4, the size named
// in `describe`; scales 1 and 2 feed the scaling check, where check time
// must grow near-linearly with size.
//
// Usage: node --experimental-strip-types test/metrics/pathological/cases.ts CASE SCALE
// prints one program, for reading or for trying a case by hand.

export interface PathologicalCase {
  readonly name: string;
  /** What the case stresses, at scale 4. */
  readonly describe: string;
  /** Wall-time budget of `hd check` at scale 4, in milliseconds. */
  readonly budgetMs: number;
  /** Peak-RSS budget of `hd check` at scale 4, in bytes. */
  readonly budgetBytes: number;
  readonly generate: (scale: number) => string;
}

const MB = 1024 * 1024;
const DEFAULT_TIME = 2_000;
const DEFAULT_MEMORY = 200 * MB;

const range = (count: number): number[] => Array.from({ length: count }, (_, index) => index);
const indent = (depth: number): string => "    ".repeat(depth);

export const CASES: readonly PathologicalCase[] = [
  {
    name: "overlapping-from",
    describe:
      "1,600 locals bound by `Cents::from(N)`, each choosing between two `From` impls (F-626)",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) =>
      [
        "use std.convert.From",
        "",
        "data Cents:",
        "    value: i64",
        "",
        "impl From[i32] for Cents:",
        "    fn from(value: i32) -> Cents:",
        "        Cents { value: i64(value) }",
        "",
        "impl From[string] for Cents:",
        "    fn from(value: string) -> Cents:",
        "        Cents { value: i64(value.len()) }",
        "",
        "fn run() -> i64:",
        "    let total: i64 = 0",
        ...range(400 * scale).flatMap((index) => [
          `    let c${index} = Cents::from(${index})`,
          `    total = total + c${index}.value`,
        ]),
        "    total",
        "",
      ].join("\n"),
  },
  {
    name: "deep-blocks",
    describe: "`if` blocks nested 96 deep",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const depth = 24 * scale;
      const lines = ["fn run(n: i32) -> i32:", "    let total = n"];
      for (const level of range(depth))
        lines.push(
          `${indent(level + 1)}if total > ${level}:`,
          `${indent(level + 2)}total = total - 1`,
        );
      lines.push("    total", "");
      return lines.join("\n");
    },
  },
  {
    name: "deep-parens",
    describe: "an arithmetic expression nested 1,000 parentheses deep",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const depth = 250 * scale;
      return [
        "fn run(n: i32) -> i32:",
        `    ${"(".repeat(depth)}n${" + 1)".repeat(depth)}`,
        "",
      ].join("\n");
    },
  },
  {
    name: "method-chain",
    describe: "one expression of 2,000 chained string method calls",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const methods = ["trim()", "lower()", "upper()", "trim_start()", "trim_end()"];
      const chain = range(500 * scale)
        .map((index) => `.${methods[index % methods.length]}`)
        .join("");
      return ["fn run(text: string) -> string:", `    text${chain}`, ""].join("\n");
    },
  },
  {
    name: "iterator-chain",
    describe: "one iterator chain of 200 `map` and `filter` stages with typed closures",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const stages = range(50 * scale).map((index) =>
        index % 2 === 0
          ? `        .map(fn(x: i32) -> i32: x + ${index})`
          : `        .filter(fn(x: i32) -> bool: x % ${(index % 7) + 2} != 0)`,
      );
      return [
        "fn run(values: List[i32]) -> usize:",
        "    values",
        "        .iter()",
        ...stages,
        "        .count()",
        "",
      ].join("\n");
    },
  },
  {
    name: "wide-list-literal",
    describe: "one list literal of 20,000 integers",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const values = range(5_000 * scale).map((index) => String((index * 7919) % 100_003));
      const rows: string[] = [];
      for (let start = 0; start < values.length; start += 20)
        rows.push(`        ${values.slice(start, start + 20).join(", ")},`);
      return [
        "fn run() -> usize:",
        "    let values: List[i64] = [",
        ...rows,
        "    ]",
        "    values.len()",
        "",
      ].join("\n");
    },
  },
  {
    name: "large-enum-match",
    describe: "an enum of 1,000 variants and an exhaustive match over it",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const count = 250 * scale;
      return [
        "enum Code:",
        ...range(count).map((index) =>
          index % 3 === 0 ? `    V${index}(amount: i32)` : `    V${index}`,
        ),
        "",
        "fn weight(code: Code) -> i32:",
        "    match code:",
        ...range(count).map((index) =>
          index % 3 === 0
            ? `        .V${index}(amount) => amount + ${index}`
            : `        .V${index} => ${index}`,
        ),
        "",
      ].join("\n");
    },
  },
  {
    name: "deep-generic",
    describe: "a generic wrapper instantiated 64 levels deep: Wrap[Wrap[...[i32]]]",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const depth = 16 * scale;
      return [
        "data Wrap[T]:",
        "    inner: T",
        "",
        "fn wrap[T](value: T) -> Wrap[T]:",
        "    Wrap { inner: value }",
        "",
        "fn run(n: i32) -> i32:",
        `    nested := ${"wrap(".repeat(depth)}n${")".repeat(depth)}`,
        `    nested${".inner".repeat(depth)}`,
        "",
      ].join("\n");
    },
  },
  {
    name: "question-chain",
    describe: "one function of 1,000 `?` propagations",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) =>
      [
        "enum StepError:",
        "    TooLarge(value: i32)",
        "",
        "fn step(value: i32) -> Result[i32, StepError]:",
        "    if value > 1_000_000: .Err(StepError.TooLarge(value)) else: .Ok(value + 1)",
        "",
        "fn run(start: i32) -> Result[i32, StepError]:",
        "    let value = start",
        ...range(250 * scale).map(() => "    value = step(value)?"),
        "    .Ok(value)",
        "",
      ].join("\n"),
  },
  {
    name: "big-file",
    describe: "one file of about 20,000 lines of ordinary functions",
    budgetMs: DEFAULT_TIME,
    budgetBytes: DEFAULT_MEMORY,
    generate: (scale) => {
      const lines: string[] = [];
      for (const index of range(500 * scale))
        lines.push(
          `fn step_${index}(x: i32, y: i32) -> i32:`,
          "    let acc = x",
          "    for i in +0..y:",
          `        if i % ${(index % 5) + 2} == 0:`,
          "            acc = acc + i",
          "        else:",
          `            acc = acc - ${index % 9}`,
          "    acc",
          "",
          "",
        );
      return lines.join("\n");
    },
  },
];

if (import.meta.main) {
  const [name, scale] = process.argv.slice(2);
  const found = CASES.find((entry) => entry.name === name);
  if (!found || !scale) {
    console.error(
      `usage: cases.ts CASE SCALE\ncases: ${CASES.map((entry) => entry.name).join(", ")}`,
    );
    process.exit(2);
  }
  process.stdout.write(found.generate(Number(scale)));
}
