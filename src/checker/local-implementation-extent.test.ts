import assert from "node:assert/strict";
import test from "node:test";

import { analyze, instantiate } from "../compiler.ts";

function diagnosticCodes(source: string): readonly string[] {
  return analyze(source).diagnostics.map((diagnostic) => diagnostic.code);
}

const localDeclarations = `
    data Entry:
        label: string
    trait Named:
        fn name(self) -> string
`;

test("a local trait implementation is unavailable before its declaration", () => {
  const source = `fn describe(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    before := entry.name()
    impl Named for Entry:
        fn name(self) -> string: self.label
    before
`;
  assert.deepEqual(diagnosticCodes(source), ["unknown-method"]);
});

test("a child-suite implementation does not leak into its parent suite", () => {
  const source = `fn describe(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    if true:
        impl Named for Entry:
            fn name(self) -> string: self.label
        _ := entry.name()
    entry.name()
`;
  assert.deepEqual(diagnosticCodes(source), ["unknown-method"]);
});

test("a local trait implementation works after its declaration", async () => {
  const source = `pub fn run() -> i32:
${localDeclarations}
    impl Named for Entry:
        fn name(self) -> string: self.label
    if Entry { label: "lexical" }.name() == "lexical": 42 else: 0
`;
  assert.deepEqual(analyze(source).diagnostics, []);
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.run as CallableFunction)(), 42);
});

test("a local inherent implementation has the same lexical extent", () => {
  const before = `fn describe(label: string) -> string:
    data Entry:
        label: string
    entry := Entry { label }
    before := entry.name()
    impl Entry:
        fn name(self) -> string: self.label
    before
`;
  const after = `fn describe(label: string) -> string:
    data Entry:
        label: string
    entry := Entry { label }
    impl Entry:
        fn name(self) -> string: self.label
    entry.name()
`;
  assert.deepEqual(diagnosticCodes(before), ["unknown-method"]);
  assert.deepEqual(diagnosticCodes(after), []);
});

test("trait-value conformance follows local implementation extent", () => {
  const before = `fn erase(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    let erased: Named = entry
    impl Named for Entry:
        fn name(self) -> string: self.label
    erased.name()
`;
  const after = `fn erase(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    impl Named for Entry:
        fn name(self) -> string: self.label
    let erased: Named = entry
    erased.name()
`;
  assert.deepEqual(diagnosticCodes(before), ["type-mismatch"]);
  assert.deepEqual(diagnosticCodes(after), []);
});

test("closures inherit implementations visible where the closure is written", async () => {
  const after = `pub fn run() -> i32:
${localDeclarations}
    entry := Entry { label: "captured" }
    impl Named for Entry:
        fn name(self) -> string: self.label
    read := fn() -> string: entry.name()
    if read() == "captured": 42 else: 0
`;
  assert.deepEqual(analyze(after).diagnostics, []);
  const { instance } = await instantiate(after);
  assert.equal((instance.exports.run as CallableFunction)(), 42);

  const before = `fn describe(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    read := fn() -> string: entry.name()
    impl Named for Entry:
        fn name(self) -> string: self.label
    read()
`;
  assert.deepEqual(diagnosticCodes(before), ["unknown-method"]);
});

test("local implementations remain global for overlap checking", () => {
  const source = `fn describe(label: string) -> string:
${localDeclarations}
    entry := Entry { label }
    if true:
        impl Named for Entry:
            fn name(self) -> string: self.label
        _ := entry.name()
    if false:
        impl Named for Entry:
            fn name(self) -> string: self.label
        _ := entry.name()
    label
`;
  assert.ok(diagnosticCodes(source).includes("overlapping-impl"));
});

test("an implementation method can use its own local implementation", () => {
  const source = `fn describe(label: string) -> string:
${localDeclarations}
    impl Named for Entry:
        fn name(self) -> string:
            if self.label == "": self.name() else: self.label
    Entry { label }.name()
`;
  assert.deepEqual(diagnosticCodes(source), []);
});

test("a local supertrait implementation must already be visible", () => {
  const declarations = `
    data Item: pass
    trait Parent
    trait Child < Parent:
        fn value(self) -> i32
`;
  const future = `fn value() -> i32:
${declarations}
    impl Child for Item:
        fn value(self) -> i32: 42
    impl Parent for Item
    Item {}.value()
`;
  assert.ok(diagnosticCodes(future).includes("missing-supertrait-implementation"));

  const previous = `fn value() -> i32:
${declarations}
    impl Parent for Item
    impl Child for Item:
        fn value(self) -> i32: 42
    Item {}.value()
`;
  assert.deepEqual(diagnosticCodes(previous), []);
});

test("local declaration defaults keep their declaration-point implementation scope", () => {
  const previous = `fn make(label: string) -> string:
${localDeclarations}
    impl Named for Entry:
        fn name(self) -> string: self.label
    data Holder:
        text: string = Entry { label: "default" }.name()
    Holder {}.text
`;
  assert.deepEqual(diagnosticCodes(previous), []);

  const future = `fn make(label: string) -> string:
${localDeclarations}
    data Holder:
        text: string = Entry { label: "default" }.name()
    impl Named for Entry:
        fn name(self) -> string: self.label
    Holder {}.text
`;
  assert.deepEqual(diagnosticCodes(future), ["unknown-method"]);
});

test("local trait defaults keep their declaration-point implementation scope", () => {
  const declarations = `
    data Text:
        value: string
    data Wrapper: pass
    trait Render:
        fn render(self) -> string
`;
  const previous = `fn show() -> string:
${declarations}
    impl Render for Text:
        fn render(self) -> string: self.value
    trait Show:
        fn show(self, text: Text) -> string: text.render()
    impl Show for Wrapper
    Wrapper {}.show(Text { value: "ok" })
`;
  assert.deepEqual(diagnosticCodes(previous), []);

  const future = `fn show() -> string:
${declarations}
    trait Show:
        fn show(self, text: Text) -> string: text.render()
    impl Render for Text:
        fn render(self) -> string: self.value
    impl Show for Wrapper
    Wrapper {}.show(Text { value: "not visible" })
`;
  assert.deepEqual(diagnosticCodes(future), ["unknown-method"]);
});
