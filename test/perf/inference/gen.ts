// Pathological inputs for the inference solver (task #253).
//
// Each case generates an hd program at a scale of 1, 10, or 100. The size
// named in a case's description is the 100x size. `run.ts` times the check
// of each program and flags super-linear growth.
//
// Usage: node --experimental-strip-types test/perf/inference/gen.ts CASE SCALE
// prints the program for one case and scale.

export interface PerfCase {
  readonly name: string;
  readonly describe: string;
  /** `accept`, or the diagnostic code the program must report. */
  readonly expect: "accept" | `reject:${string}`;
  readonly generate: (scale: number) => string;
}

const lines = (count: number, line: (index: number) => string): string =>
  Array.from({ length: count }, (_, index) => line(index)).join("\n");

const indent = (text: string, by = "    "): string =>
  text
    .split("\n")
    .map((line) => (line === "" ? line : by + line))
    .join("\n");

export const CASES: readonly PerfCase[] = [
  {
    name: "list-nest",
    describe: "a literal nested in List 2/20/200 deep, fixed by a typed read at the end",
    expect: "accept",
    generate: (scale) => {
      const depth = 2 * scale;
      return [
        "fn run() -> u16:",
        `    let xs = ${"[".repeat(depth)}1${"]".repeat(depth)}`,
        `    let fixed: u16 = xs${"[0]".repeat(depth)}`,
        "    fixed",
        "",
      ].join("\n");
    },
  },
  {
    name: "open-lets-reverse",
    describe: "1k/10k/100k open lets joined by assignments in reverse order, fixed once at the end",
    expect: "accept",
    generate: (scale) => {
      const count = 1000 * scale;
      return [
        "fn run() -> u64:",
        indent(lines(count, (index) => `let v${index} = ${index}`)),
        indent(lines(count - 1, (index) => `v${count - 1 - index} = v${count - 2 - index}`)),
        "    let fixed: u64 = v0",
        `    fixed + v${count - 1}`,
        "",
      ].join("\n");
    },
  },
  {
    name: "method-calls-one-var",
    describe: "100/1k/10k method calls on one open variable, resolved at the fallback",
    expect: "accept",
    generate: (scale) => {
      const count = 100 * scale;
      return [
        "fn run() -> usize:",
        "    let x = 1",
        "    let total = 0",
        indent(lines(count, () => "total = total + x.to_string().len()")),
        "    total",
        "",
      ].join("\n");
    },
  },
  {
    name: "add-many-types",
    describe: "Add implemented for 1/5/50 data types at three widths, used with open operands",
    expect: "accept",
    generate: (scale) => {
      const typeCount = Math.max(1, scale / 2);
      const decls = lines(typeCount, (index) =>
        [
          `data D${index}:`,
          "    cents: i64",
          "",
          ...["i32", "i64", "usize"].map(
            (width) =>
              `impl Add[${width}] for D${index}:\n    type Out = D${index}\n    fn add(self, rhs: ${width}) -> D${index}:\n        self\n`,
          ),
        ].join("\n"),
      );
      const uses = lines(typeCount, (index) =>
        lines(
          10,
          (use) =>
            `let r${index}_${use} = D${index} { cents: 0 } + n${use % 3}\nlet c${index}_${use} = r${index}_${use}.cents`,
        ),
      );
      return [
        "use std.ops.Add",
        "",
        decls,
        "fn run() -> i64:",
        "    let n0 = 1",
        "    let n1 = -1",
        "    let n2 = 2",
        indent(uses),
        "    let wide: i64 = n2",
        "    wide",
        "",
      ].join("\n");
    },
  },
  {
    name: "overload-candidates",
    describe: "50 calls, each choosing among 1/10/100 instantiations of one generic trait",
    expect: "accept",
    generate: (scale) => {
      const candidates = scale;
      const decls = lines(candidates, (index) =>
        [
          `data K${index}:`,
          "    id: i32",
          "",
          `impl Pick[K${index}] for Money:`,
          `    fn pick(self, key: K${index}) -> i32:`,
          `        key.id + ${index}`,
          "",
        ].join("\n"),
      );
      const calls = lines(50, (index) => {
        const key = index % candidates;
        return `total = total + price.pick(K${key} { id: ${index} })`;
      });
      return [
        "trait Pick[T]:",
        "    fn pick(self, key: T) -> i32",
        "",
        "data Money:",
        "    cents: i32",
        "",
        decls,
        "fn run() -> i32:",
        "    price := Money { cents: 1 }",
        "    let total: i32 = 0",
        indent(calls),
        "    total",
        "",
      ].join("\n");
    },
  },
  {
    name: "big-body",
    describe: "one body of 1k/10k/100k statements mixing open and typed literals",
    expect: "accept",
    generate: (scale) => {
      const count = 250 * scale;
      return [
        "fn run(items: List[i64]) -> i64:",
        "    let total: i64 = 0",
        indent(
          lines(
            count,
            (index) =>
              `let a${index} = ${index}\nlet b${index} = a${index} + 1\nif b${index} < items.len():\n    total = total + items[a${index}]`,
          ),
        ),
        "    total",
        "",
      ].join("\n");
    },
  },
  {
    name: "mutual-bounds",
    describe: "forward and mutual bounds over 5/50/500 parameters of one function",
    expect: "accept",
    generate: (scale) => {
      const pairs = Math.max(1, (5 * scale) / 2);
      const decls = lines(pairs, (index) =>
        [
          `data L${index}:`,
          "    id: i32",
          "",
          `data R${index}:`,
          "    id: i32",
          "",
          `impl Mate[R${index}] for L${index}:`,
          `    fn mate(self) -> R${index}: R${index} { id: self.id }`,
          "",
          `impl Mate[L${index}] for R${index}:`,
          `    fn mate(self) -> L${index}: L${index} { id: self.id }`,
          "",
        ].join("\n"),
      );
      const parameters = lines(
        pairs,
        (index) => `A${index} < Mate[B${index}], B${index} < Mate[A${index}]`,
      )
        .split("\n")
        .join(", ");
      const formals = lines(pairs, (index) => `a${index}: A${index}, b${index}: B${index}`)
        .split("\n")
        .join(", ");
      const actuals = lines(pairs, (index) => `L${index} { id: ${index} }, R${index} { id: 0 }`)
        .split("\n")
        .join(", ");
      return [
        "trait Mate[T]:",
        "    fn mate(self) -> T",
        "",
        decls,
        `fn wire[${parameters}](${formals}) -> i32:`,
        "    0",
        "",
        "fn run() -> i32:",
        `    wire(${actuals})`,
        "",
      ].join("\n");
    },
  },
  {
    name: "closures-shared-var",
    describe: "10/100/1000 closures that share one open variable, fixed after the last",
    expect: "accept",
    generate: (scale) => {
      const count = 10 * scale;
      return [
        "fn run() -> u32:",
        "    let n = 0",
        indent(
          lines(
            count,
            (index) => `let step${index} = fn() -> void:\n    n = n + ${index % 7}\nstep${index}()`,
          ),
        ),
        "    let fixed: u32 = n",
        "    fixed",
        "",
      ].join("\n");
    },
  },
  {
    name: "polymorphic-recursion",
    describe:
      "10/100/1000 generic functions that recurse at a larger type, each called with a literal",
    expect: "accept",
    generate: (scale) => {
      const count = 10 * scale;
      return [
        lines(
          count,
          (index) =>
            `fn grow${index}[T](value: T, depth: u32) -> u32:\n    if depth == 0:\n        return 0\n    grow${index}((value, value), depth - 1) + 1\n`,
        ),
        "fn run() -> u32:",
        "    let total: u32 = 0",
        indent(lines(count, (index) => `total = total + grow${index}(${index}, 3)`)),
        "    total",
        "",
      ].join("\n");
    },
  },
  {
    name: "late-conflict",
    describe: "a conflict at statement 1k/10k/100k with the deciding use at statement 1",
    expect: "reject:type-mismatch",
    generate: (scale) => {
      const count = 1000 * scale;
      return [
        "fn run() -> u32:",
        "    let x = 0",
        "    let first: u8 = x",
        indent(lines(count, (index) => `let filler${index} = ${index % 200}`)),
        "    let late: u32 = x",
        "    late",
        "",
      ].join("\n");
    },
  },
  {
    name: "occurs-check",
    describe: "10/100/1000 functions, each tying a type into itself through a closure",
    expect: "reject:type-mismatch",
    generate: (scale) => {
      const count = 10 * scale;
      return [
        "fn knot[T](seed: T, grow: fn(T) -> List[T]) -> List[T]:",
        "    grow(seed)",
        "",
        lines(
          count,
          (index) =>
            `fn tie${index}() -> void:\n    let start = [${index}]\n    let grown = knot(start, fn(items): items)\n`,
        ),
      ].join("\n");
    },
  },
];

// The gate's yardstick (gate.ts): fully annotated, linear code with no open
// literal and no inference, so its check time tracks the speed of the
// machine and of the checker's ordinary passes. Gate scores are case times
// divided by this program's time, measured in the same process.
export const REFERENCE: PerfCase = {
  name: "reference",
  describe: "100 data types per scale, each with a function of typed lets and one call",
  expect: "accept",
  generate: (scale) => {
    const count = 100 * scale;
    return [
      lines(
        count,
        (index) =>
          `data P${index}:\n    x: i64\n    y: i64\n\nfn area${index}(p: P${index}) -> i64:\n    let w: i64 = p.x * 2\n    let h: i64 = p.y + 3\n    if w > h:\n        return w - h\n    w * h\n`,
      ),
      "fn run() -> i64:",
      "    let total: i64 = 0",
      indent(lines(count, (index) => `total = total + area${index}(P${index} { x: 1, y: 2 })`)),
      "    total",
      "",
    ].join("\n");
  },
};

if (import.meta.main) {
  const [name, scaleText] = process.argv.slice(2);
  const found = [...CASES, REFERENCE].find((entry) => entry.name === name);
  if (!found || !scaleText) {
    console.error(
      `usage: gen.ts CASE SCALE\ncases: ${CASES.map((entry) => entry.name).join(", ")}`,
    );
    process.exit(2);
  }
  process.stdout.write(found.generate(Number(scaleText)));
}
