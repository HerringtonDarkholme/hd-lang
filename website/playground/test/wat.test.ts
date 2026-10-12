import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import {
  GOAL,
  STATUS,
  compileRequest,
  encodeRequest,
  type CompilerExports,
} from "../src/compiler-wire.ts";
import { tokenizeWat, type WatTokenKind } from "../src/wat.ts";

const root = resolve(import.meta.dirname, "../../..");

/** The non-plain tokens of `text`, as `kind:text`. */
function styled(text: string): string[] {
  return tokenizeWat(text)
    .filter(({ kind }) => kind !== "plain")
    .map(({ kind, text: value }) => `${kind}:${value}`);
}

test("module structure, instructions, types, names, and numbers", () => {
  const wat =
    '(func $f0 (export "main") (param $provider0 externref) (result i32)\n  (i32.add (local.get $x) (i32.const -42)))';
  assert.deepEqual(styled(wat), [
    "paren:(",
    "keyword:func",
    "name:$f0",
    "paren:(",
    "keyword:export",
    'string:"main"',
    "paren:)",
    "paren:(",
    "keyword:param",
    "name:$provider0",
    "type:externref",
    "paren:)",
    "paren:(",
    "keyword:result",
    "type:i32",
    "paren:)",
    "paren:(",
    "instruction:i32.add",
    "paren:(",
    "instruction:local.get",
    "name:$x",
    "paren:)",
    "paren:(",
    "instruction:i32.const",
    "number:-42",
    "paren:)))",
  ]);
});

test("GC types and reference instructions", () => {
  assert.deepEqual(styled("(type $hd.bytes (array (mut i8)))"), [
    "paren:(",
    "keyword:type",
    "name:$hd.bytes",
    "paren:(",
    "keyword:array",
    "paren:(",
    "keyword:mut",
    "type:i8",
    "paren:)))",
  ]);
  assert.deepEqual(styled("(ref.cast (ref null $hd.map) (struct.get $s $f (local.get 0)))"), [
    "paren:(",
    "instruction:ref.cast",
    "paren:(",
    "type:ref",
    "type:null",
    "name:$hd.map",
    "paren:)",
    "paren:(",
    "instruction:struct.get",
    "name:$s",
    "name:$f",
    "paren:(",
    "instruction:local.get",
    "number:0",
    "paren:)))",
  ]);
});

test("number forms", () => {
  for (const number of ["0", "1_000", "0xFF", "-0x1p-3", "2.5e10", "inf", "-nan", "nan:0x7f"])
    assert.deepEqual(styled(number), [`number:${number}`], number);
  assert.deepEqual(styled("offset=16 align=4"), [
    "keyword:offset=",
    "number:16",
    "keyword:align=",
    "number:4",
  ]);
});

test("comments and strings", () => {
  assert.deepEqual(styled(';; a (line) "comment"\n(nop)'), [
    'comment:;; a (line) "comment"',
    "paren:(",
    "instruction:nop",
    "paren:)",
  ]);
  assert.deepEqual(styled("(; outer (; nested ;) still ;) (drop)"), [
    "comment:(; outer (; nested ;) still ;)",
    "paren:(",
    "instruction:drop",
    "paren:)",
  ]);
  assert.deepEqual(styled('(data "a \\"quoted\\" \\00 byte")'), [
    "paren:(",
    "keyword:data",
    'string:"a \\"quoted\\" \\00 byte"',
    "paren:)",
  ]);
});

test("unterminated input still covers every character", () => {
  for (const text of ['"open', "(; open", ";", "$", "(i32.const", "offset=", "@ #"]) {
    const tokens = tokenizeWat(text);
    assert.equal(tokens.map(({ text: value }) => value).join(""), text, text);
  }
});

test("quoted names", () => {
  assert.deepEqual(styled('(func $"std/impl Display for usize#0" (;3;) (type 2))'), [
    "paren:(",
    "keyword:func",
    'name:$"std/impl Display for usize#0"',
    "comment:(;3;)",
    "paren:(",
    "keyword:type",
    "number:2",
    "paren:))",
  ]);
  assert.deepEqual(styled('$"open'), ['name:$"open']);
});

test("tokens concatenate back to the text the compiler wasm prints", async () => {
  // The WAT view shows what the wat goal of crates/hd_web prints with
  // wasmprinter; the wasm is a build output, built when a checkout has none.
  const wasm = resolve(import.meta.dirname, "../assets/hd_web.wasm");
  if (!existsSync(wasm))
    execFileSync(process.execPath, [resolve(root, "scripts/build-web-compiler.js")], {
      stdio: "inherit",
    });
  const { instance } = await WebAssembly.instantiate(await readFile(wasm), {});
  const source = 'pub fn main() -> void $ Console:\n    println("hi")\n';
  const compiled = compileRequest(
    instance.exports as unknown as CompilerExports,
    encodeRequest({ "src/main.hd": source }, "src/main.hd", GOAL.wat),
  );
  assert.equal(compiled.status, STATUS.ok);
  const text = compiled.wat;
  assert.ok(text.startsWith("(module"), text.slice(0, 40));
  assert.match(text, /\(func \$playground\/main\/main#0 /);
  const tokens = tokenizeWat(text);
  assert.equal(tokens.map(({ text: value }) => value).join(""), text);
  const kinds = new Set<WatTokenKind>(tokens.map(({ kind }) => kind));
  for (const kind of ["keyword", "instruction", "type", "name", "number", "paren"] as const)
    assert.ok(kinds.has(kind), kind);
  assert.ok(
    tokens.some(({ kind, text: value }) => kind === "name" && value === "$playground/main/main#0"),
  );
  // A quoted name is one token, never a bare `$` and a string.
  assert.ok(!tokens.some(({ kind, text: value }) => kind === "name" && value === "$"));
});
