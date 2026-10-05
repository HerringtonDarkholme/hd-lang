import assert from "node:assert/strict";
import test from "node:test";

import { compileToWat, instantiate } from "../src/compiler.ts";
import { RuntimePanicError } from "../src/runtime-panic.ts";

// Build profiles (spec/cli/command-line.md#build-profiles): a release build
// wraps integer overflow, and a debug or test build panics
// (spec/lang/04-type-system.md#r-types.arith.checked).

const add = "pub fn add(a: i32, b: i32) -> i32:\n    a + b\n";

function lines(source: string, release: boolean): string {
  return compileToWat(source, { release }).wat;
}

test("a debug build calls the checked sequence for a + b, a release build emits i32.add", () => {
  const debug = lines(add, false);
  const release = lines(add, true);
  assert.match(debug, /\(call \$hd\.add_i32 /);
  assert.match(debug, /\(func \$hd\.add_i32 /);
  assert.doesNotMatch(release, /\$hd\.add_i32/);
  assert.match(release, /\(i32\.add \(local\.get/);
  assert.ok(release.length < debug.length);
});

test("the default build is a debug build", () => {
  assert.equal(compileToWat(add).wat, lines(add, false));
});

test("a release build drops the checks of every wrapping operator", () => {
  const source = [
    "pub fn all(a: i32, b: i64, c: u8, d: u64, n: u32) -> i64:",
    "    let x = -a * a - a",
    "    let y = c * c + c - c",
    "    let z = (b * b - b + -b) << n",
    "    let w = (d * d + d - d) ** 2",
    "    i64(x) + i64(y) + z + i64(w) + (b ** 3)",
    "",
  ].join("\n");
  const release = lines(source, true);
  for (const name of [
    "add_i32",
    "sub_i32",
    "mul_i32",
    "neg_i32",
    "add_i64",
    "sub_i64",
    "mul_i64",
    "neg_i64",
    "pow_i64",
    "check_u8",
    "check_range_i32",
    "check_shift_i64",
    "add_u64",
    "sub_u64",
    "mul_u64",
    "pow_u64",
  ])
    assert.doesNotMatch(release, new RegExp(`\\$hd\\.${name}\\b`), name);
});

test("division and MIN / -1 stay checked in a release build", () => {
  const source = "pub fn divide(a: i32, b: i32) -> i32:\n    a / b\n";
  const release = lines(source, true);
  assert.match(release, /\(i32\.div_s /);
  assert.match(release, /\(i32\.const -2147483648\)/);
});

/** Runs `main` and returns the lines it printed; a panic rejects. */
async function printed(source: string, release: boolean): Promise<string[]> {
  const output: string[] = [];
  const { instance } = await instantiate(source, {
    release,
    console: (text) => output.push(text.replace(/\n$/, "")),
  });
  (instance.exports.main as CallableFunction)();
  return output;
}

function program(...body: string[]): string {
  return ["pub fn main() -> void $ Console:", ...body.map((line) => `    ${line}`), ""].join("\n");
}

test("overflow wraps in a release build and panics in a debug build", async () => {
  const cases: readonly (readonly [string, string])[] = [
    ["let a: i32 = 2147483647\n    println(a + 1)", "-2147483648"],
    ["let a: i32 = -2147483647 - 1\n    println(a - 1)", "2147483647"],
    ["let a: i32 = 65536\n    println(a * a)", "0"],
    ["let a: i32 = -2147483647 - 1\n    println(-a)", "-2147483648"],
    ["let a: i32 = 3\n    let n: u32 = 25\n    println(a ** n)", "1180052131"],
    ["let a: i64 = 9223372036854775807\n    println(a + 1)", "-9223372036854775808"],
    ["let a: i64 = -9223372036854775807 - 1\n    println(-a)", "-9223372036854775808"],
    ["let a: i64 = 4294967296\n    println(a * a)", "0"],
  ];
  for (const [body, expected] of cases) {
    const source = program(body);
    assert.deepEqual(await printed(source, true), [expected], body);
    await assert.rejects(printed(source, false), RuntimePanicError, body);
  }
});

test("sized integers wrap to their width in a release build", async () => {
  const cases: readonly (readonly [string, string])[] = [
    ["let a: u8 = 250\n    println(a + 10)", "4"],
    ["let a: i8 = 127\n    println(a * 2)", "-2"],
    ["let a: u16 = 0\n    println(a - 1)", "65535"],
    ["let a: u32 = 0\n    println(a - 1)", "4294967295"],
    ["let a: u64 = 0\n    println(a - 1)", "18446744073709551615"],
    ["let a: i8 = -128\n    println(-a)", "-128"],
    ["let a: u8 = 3\n    let n: u32 = 7\n    println(a ** n)", "139"],
  ];
  for (const [body, expected] of cases) {
    const source = program(body);
    assert.deepEqual(await printed(source, true), [expected], body);
    await assert.rejects(printed(source, false), RuntimePanicError, body);
  }
});

test("a release build masks a shift count and a debug build panics", async () => {
  const cases: readonly (readonly [string, string])[] = [
    ["let n: u32 = 33\n    let a: i32 = 3\n    println(a << n)", "6"],
    ["let n: u32 = 65\n    let a: i64 = 3\n    println(a << n)", "6"],
    ["let n: u32 = 9\n    let a: u8 = 1\n    println(a << n)", "2"],
    ["let n: u32 = 33\n    let a: i32 = 4\n    println(a >> n)", "2"],
  ];
  for (const [body, expected] of cases) {
    const source = program(body);
    assert.deepEqual(await printed(source, true), [expected], body);
    await assert.rejects(printed(source, false), RuntimePanicError, body);
  }
});

test("MIN / -1 and a zero divisor panic in a release build", async () => {
  for (const body of [
    "let a: i32 = -2147483647 - 1\n    let b: i32 = -1\n    println(a / b)",
    "let a: i32 = 1\n    let b: i32 = 0\n    println(a / b)",
    "let a: i8 = -128\n    let b: i8 = -1\n    println(a / b)",
  ])
    await assert.rejects(printed(program(body), true), RuntimePanicError, body);
});

test("a program that does not overflow prints the same in both builds", async () => {
  const source = program("let a: i32 = 7", "println(a * 3 + 2 - (a << 2))");
  assert.deepEqual(await printed(source, true), await printed(source, false));
});
