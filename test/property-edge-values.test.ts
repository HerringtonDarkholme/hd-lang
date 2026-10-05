import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { hd } from "./hd-in-process.ts";

// Edge values (spec/std/testing.md#edge-values): within the first four
// generated cases of a property run, the integer draws reach 0, the bounds
// of the range, and the smallest and largest value of the type, and the
// collection draws reach the empty collection. Each property below panics
// when its draw is the edge, so a run that reaches the edge in four cases
// reports a failing property named after it. Cases are separate program
// instances, so a conformance fixture cannot count the edges across them.

const GENERATORS = `use std.testing.{Choices, it_prop, it_prop_with}

fn bounded(c: mut Choices) -> i64:
    c.int(3, 9)

fn around_zero(c: mut Choices) -> i64:
    c.int(-4, 4)

fn numbers(c: mut Choices) -> List[i32]:
    c.list(5, fn(inner: mut Choices) -> i32: inner.int(0, 9))

fn letters(c: mut Choices) -> string:
    c.string(max_chars=5)

fn table(c: mut Choices) -> Map[i32, i32]:
    c.map(5, key=fn(inner: mut Choices) -> i32: inner.int(0, 9), value=fn(inner: mut Choices) -> i32: inner.int(0, 9))

fn seen(found: bool) -> void:
    if found:
        panic("edge value reached")
`;

interface Edge {
  readonly name: string;
  readonly registration: string;
}

const RANGE_EDGES: readonly Edge[] = [
  ["int reaches its lower bound", "bounded", "n == 3"],
  ["int reaches its upper bound", "bounded", "n == 9"],
  ["int reaches zero inside its range", "around_zero", "n == 0"],
].map(([name, gen, condition]) => ({
  name: name!,
  registration: `it_prop_with("${name}", gen=${gen}, cases=4, prop=fn!(n: i64): seen(${condition}))`,
}));

const TYPE_EDGES: readonly Edge[] = [
  ["i32 reaches its smallest value", "i32", "n == -2147483648"],
  ["i32 reaches its largest value", "i32", "n == 2147483647"],
  ["i32 reaches zero", "i32", "n == 0"],
  ["i64 reaches its smallest value", "i64", "n == -9223372036854775807 - 1"],
  ["i64 reaches its largest value", "i64", "n == 9223372036854775807"],
  ["i64 reaches zero", "i64", "n == 0"],
  ["u32 reaches its largest value", "u32", "n == 4294967295"],
  ["u32 reaches zero", "u32", "n == 0"],
  ["u64 reaches its largest value", "u64", "n == 18446744073709551615"],
  ["u64 reaches zero", "u64", "n == 0"],
  ["u8 reaches its largest value", "u8", "n == 255"],
  ["u8 reaches zero", "u8", "n == 0"],
  ["i8 reaches its smallest value", "i8", "n == -128"],
  ["i8 reaches its largest value", "i8", "n == 127"],
  ["i8 reaches zero", "i8", "n == 0"],
  ["i16 reaches its smallest value", "i16", "n == -32768"],
  ["u16 reaches its largest value", "u16", "n == 65535"],
].map(([name, type, condition]) => ({
  name: name!,
  registration: `it_prop("${name}", cases=4, prop=fn!(n: ${type}): seen(${condition}))`,
}));

const EMPTY_EDGES: readonly Edge[] = [
  {
    name: "a default list is empty once",
    registration:
      'it_prop("a default list is empty once", cases=4, prop=fn!(items: List[i32]): seen(items.len() == 0))',
  },
  {
    name: "a drawn list is empty once",
    registration:
      'it_prop_with("a drawn list is empty once", gen=numbers, cases=4, prop=fn!(items: List[i32]): seen(items.len() == 0))',
  },
  {
    name: "a drawn string is empty once",
    registration:
      'it_prop_with("a drawn string is empty once", gen=letters, cases=4, prop=fn!(text: string): seen(text == ""))',
  },
  {
    name: "a drawn map is empty once",
    registration:
      'it_prop_with("a drawn map is empty once", gen=table, cases=4, prop=fn!(entries: Map[i32, i32]): seen(entries.len() == 0))',
  },
];

const EDGES: readonly Edge[] = [...RANGE_EDGES, ...TYPE_EDGES, ...EMPTY_EDGES];

async function run(
  source: string,
  flags: readonly string[],
): Promise<{ passed: boolean; output: string }> {
  const directory = await mkdtemp(join(tmpdir(), "hd-edge-values-"));
  try {
    const file = join(directory, "edges.hd");
    await writeFile(file, source);
    try {
      const { stdout, stderr } = await hd(["test", ...flags, file]);
      return { passed: true, output: stdout + stderr };
    } catch (error) {
      const failure = error as { stdout?: string; stderr?: string };
      return { passed: false, output: `${failure.stdout ?? ""}${failure.stderr ?? ""}` };
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

// `hd test` stops at the first failing test case, so each edge runs alone
// under `--filter`. The run seed is the first case's seed, and the seeds
// below cover each residue of four, so no seed is a lucky one.
test("the first four generated cases of a property reach every edge", async () => {
  const program = `${GENERATORS}\ntests:\n${EDGES.map((edge) => `    ${edge.registration}\n`).join("\n")}`;
  for (const [index, edge] of EDGES.entries()) {
    const seed = 4 * index + (index % 4) + 1000;
    const { passed, output } = await run(program, ["--filter", edge.name, "--seed", String(seed)]);
    assert.equal(passed, false, `${edge.name} (seed ${seed}) reached its edge`);
    assert.ok(output.includes(`property test "${edge.name}"`), output);
  }
});

test("a property that never meets its condition still passes", async () => {
  const program = `${GENERATORS}
tests:
    it_prop_with("never outside the range", gen=bounded, cases=4, prop=fn!(n: i64): seen(n < 3 || n > 9))
    it_prop_with("a list stays within its maximum", gen=numbers, cases=4, prop=fn!(items: List[i32]): seen(items.len() > 5))
`;
  const { passed, output } = await run(program, []);
  assert.equal(passed, true, output);
});
