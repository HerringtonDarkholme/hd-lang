import assert from "node:assert/strict";
import test from "node:test";
import { classifyRust, tokenize, type LineKind } from "./rust-lines.ts";

// The kind of each line with code, as "line:kind" in line order.
function kinds(src: string): string[] {
  return [...classifyRust(src).kinds]
    .sort(([left], [right]) => left - right)
    .map(([line, kind]: [number, LineKind]) => `${line}:${kind}`);
}

test("comments, strings, chars and lifetimes do not open brackets or hide code", () => {
  const src = [
    "fn f<'a>(x: &'a str) -> char {", // 1
    '    let s = "{ ( [ // not a comment";', // 2
    "    /* { nested /* } */ ( */", // 3
    '    let r = r#"a "quoted" } "#;', // 4
    "    let c = '{';", // 5
    "    let e = '\\'';", // 6
    "    'outer: loop { break 'outer; }", // 7
    "    b'}'", // 8
    "}", // 9
  ].join("\n");
  assert.deepEqual(kinds(src), [
    "1:code",
    "2:code",
    "4:code",
    "5:code",
    "6:code",
    "7:code",
    "8:code",
    "9:code",
  ]);
  assert.equal(tokenize('let x = "a\nb";').at(-2)?.end, 2);
});

test("unreachable!, panic! and debug_assert! lines are invariant; the code around them is not", () => {
  const src = [
    "fn f(x: Option<u32>) -> u32 {", // 1
    "    debug_assert!(x.is_some());", // 2
    "    match x {", // 3
    "        Some(v) if v > 9 => v,", // 4
    '        None => unreachable!("checked above"),', // 5
    "        Some(0) => {", // 6
    "            panic!(", // 7
    '                "zero: {}",', // 8
    "                1", // 9
    "            )", // 10
    "        }", // 11
    "        Some(v) => v.checked_add(1).unwrap_or_else(|| panic!()),", // 12
    "    }", // 13
    "}", // 14
  ].join("\n");
  assert.deepEqual(kinds(src), [
    "1:code",
    "2:invariant",
    "3:code",
    "4:code",
    "5:invariant",
    "6:invariant",
    "7:invariant",
    "8:invariant",
    "9:invariant",
    "10:invariant",
    "11:invariant",
    "12:code",
    "13:code",
    "14:code",
  ]);
});

test("an else block and a function body that only panic go with the panic", () => {
  const src = [
    "fn f(ok: bool) {", // 1
    "    if ok {", // 2
    "        run();", // 3
    "    } else {", // 4
    '        std::panic!("no");', // 5
    "    }", // 6
    "}", // 7
    "#[cold]", // 8
    "fn never() -> ! {", // 9
    '    unreachable!("never")', // 10
    "}", // 11
  ].join("\n");
  assert.deepEqual(kinds(src), [
    "1:code",
    "2:code",
    "3:code",
    "4:code",
    "5:invariant",
    "6:invariant",
    "7:code",
    "8:invariant",
    "9:invariant",
    "10:invariant",
    "11:invariant",
  ]);
});

test("verifier findings are invariant inside verifier code only", () => {
  const src = [
    "struct Walk { errs: Vec<VerifyError> }", // 1
    "impl Walk {", // 2
    "    fn step(&mut self, i: u32) {", // 3
    "        if i > 3 {", // 4
    '            self.err(i, "too far".into());', // 5
    "        }", // 6
    "    }", // 7
    "}", // 8
    "fn verify(b: &Body, mut err: impl FnMut(u32, String)) {", // 9
    "    for i in 0..b.len() {", // 10
    '        err(i, "bad".into());', // 11
    "    }", // 12
    "}", // 13
    "fn check(&mut self, n: u32) {", // 14
    '    self.err(Code::Bad, n, "a user diagnostic");', // 15
    "}", // 16
    "fn verify_in(b: &Body) -> Vec<VerifyError> {", // 17
    "    let mut errs = Vec::new();", // 18
    "    errs.push(VerifyError {", // 19
    "        invariant: 9,", // 20
    "    });", // 21
    "    errs", // 22
    "}", // 23
  ].join("\n");
  const got = new Map(classifyRust(src).kinds);
  assert.equal(got.get(5), "invariant");
  assert.equal(got.get(6), "invariant");
  assert.equal(got.get(4), "code");
  assert.equal(got.get(11), "invariant");
  assert.equal(got.get(10), "code");
  assert.equal(got.get(15), "code");
  assert.deepEqual(
    [19, 20, 21].map((line) => got.get(line)),
    ["invariant", "invariant", "invariant"],
  );
  assert.equal(got.get(22), "code");
});

test("cfg(test) items leave the report, and test module files are named", () => {
  const src = [
    "pub fn run() {}", // 1
    "#[cfg(test)]", // 2
    "mod tests {", // 3
    "    #[test]", // 4
    "    fn works() { assert!(true); }", // 5
    "}", // 6
    "#[cfg(test)]", // 7
    "mod corpus;", // 8
    "struct S {", // 9
    "    #[cfg(test)]", // 10
    "    probe: Vec<u8>,", // 11
    "    kept: u8,", // 12
    "}", // 13
  ].join("\n");
  const result = classifyRust(src);
  assert.deepEqual(result.testModules, ["corpus"]);
  assert.equal(result.wholeFileTest, false);
  assert.deepEqual(kinds(src), [
    "1:code",
    "2:test",
    "3:test",
    "4:test",
    "5:test",
    "6:test",
    "7:test",
    "8:test",
    "9:code",
    "10:test",
    "11:test",
    "12:code",
    "13:code",
  ]);
  assert.equal(classifyRust("#![cfg(test)]\nfn f() {}").wholeFileTest, true);
});

test("unbalanced brackets name the file and line", () => {
  assert.throws(() => classifyRust("fn f() {\n  (]\n}", "a.rs"), /a\.rs:2: unbalanced/);
});
