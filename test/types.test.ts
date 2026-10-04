import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import { parse } from "../src/parser/index.ts";
import { ReplSession } from "../src/repl.ts";
import { inspectKey } from "../src/checker/inspectable.ts";
import { unresolvedTypeMessage } from "../src/checker/cannot-infer.ts";
import { matchGenericTypePattern, resolveGenericType } from "../src/checker/shared.ts";
import {
  eraseTypePermissions,
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  optionalInner,
  optionalType,
  substituteTypeParameters,
  tupleType,
  typeSourceText,
} from "../src/types.ts";

const user = "data User:\n    name: i32\n";

test("inference advice preserves optional permission boundaries in source annotations", () => {
  const span = { start: { offset: 0, line: 1, column: 1 }, end: { offset: 1, line: 1, column: 2 } };
  for (const [type, written] of [
    ["mut:generic:T?", "mut (T?)"],
    ["(mut:generic:T)?", "(mut T)?"],
  ]) {
    const message = unresolvedTypeMessage(["T"], type!, span, undefined);
    assert.ok(message.includes(`let value: ${written} = ...`));
    assert.deepEqual(parameterTypes([written!]), [type!.replaceAll("generic:", "")]);
  }
});

function parameterTypes(written: readonly string[]): readonly string[] {
  const result = parse(
    `fn shape(${written.map((type, index) => `p${index}: ${type}`).join(", ")}) -> void:\n    ()\n`,
  );
  assert.deepEqual(result.diagnostics, []);
  return result.program!.functions[0]!.parameters.map((parameter) => parameter.type.name);
}

test("optional payload permission and optional outer permission have distinct canonical types", () => {
  assert.deepEqual(
    parameterTypes([
      "(mut User)?",
      "Option[mut User]",
      "mut User?",
      "mut Option[User]",
      "mut (User?)",
    ]),
    ["(mut:User)?", "(mut:User)?", "(mut:User)?", "mut:User?", "mut:User?"],
  );
  assert.equal(mutableInner(optionalType(mutableType("User"))), undefined);
  assert.equal(optionalInner(optionalType(mutableType("User"))), "mut:User");
  assert.equal(mutableInner(mutableType(optionalType("User"))), "User?");
  assert.equal(optionalInner(mutableType(optionalType("User"))), "User");
});

test("nesting preserves the scope of each mut prefix and optional constructor", () => {
  assert.deepEqual(
    parameterTypes([
      "Option[Option[mut User]]",
      "((mut User)?)?",
      "Option[mut Option[User]]",
      "(mut User?)?",
      "mut Option[mut User]",
      "(Option[mut User], Option[User])",
    ]),
    [
      "(mut:User)??",
      "(mut:User)??",
      "(mut:User?)?",
      "(mut:User)??",
      "mut:(mut:User)?",
      "((mut:User)?,User?)",
    ],
  );
});

test("function optionality remains distinct from result optionality", () => {
  const optionalFunction = optionalType(functionType(["(mut:User)?"], "mut:User?"));
  assert.equal(optionalFunction, "(fn((mut:User)?)->mut:User?)?");
  assert.equal(functionParts(optionalFunction), undefined);
  assert.equal(functionParts(optionalInner(optionalFunction)!)!.result, "mut:User?");
  assert.equal(optionalInner(functionType([], "(mut:User)?")), undefined);
  assert.deepEqual(parameterTypes(["Option[fn(Option[mut User]) -> mut Option[User]]"]), [
    optionalFunction,
  ]);
});

test("type substitution and generic matching preserve permission constructor boundaries", () => {
  const source = tupleType(["(mut:T)?", "mut:T?", "fn((mut:T)?)->(mut:T)??"]);
  const resolved = resolveGenericType(source, new Set(["T"]));
  assert.equal(
    substituteTypeParameters(resolved, new Map([["T", "User"]])),
    "((mut:User)?,mut:User?,fn((mut:User)?)->(mut:User)??)",
  );
  assert.equal(matchGenericTypePattern("(mut:generic:T)?", "mut:User?", new Map()), false);
  const substitutions = new Map<string, string>();
  assert.equal(matchGenericTypePattern("(mut:generic:T)?", "(mut:User)?", substitutions), true);
  assert.equal(substitutions.get("T"), "User");
});

test("deep permission erasure rebuilds canonical option and function types", () => {
  assert.equal(
    eraseTypePermissions("fn((mut:User)?,mut:User?)->Result[(mut:User)??,List[mut:User]]"),
    "fn(User?,User?)->Result[User??,List[User]]",
  );
});

test("source type rendering round-trips mutable constructors rather than changing prefix scope", () => {
  for (const type of [
    "(mut:User)?",
    "mut:User?",
    "(mut:User?)?",
    "mut:(mut:User)?",
    "fn((mut:User)?)->mut:User?",
    "List[(mut:User?)?]",
  ])
    assert.deepEqual(parameterTypes([typeSourceText(type)]), [type]);
});

test("optional invariance rejects weakening the payload but accepts weakening the outer view", () => {
  for (const type of ["(mut User)?", "Option[mut User]"]) {
    assert.deepEqual(
      analyze(`${user}\nfn view(value: ${type}) -> User?:\n    value\n`).diagnostics.map(
        (diagnostic) => diagnostic.code,
      ),
      ["type-mismatch"],
    );
  }
  assert.deepEqual(
    analyze(`${user}\nfn view(value: mut Option[User]) -> User?:\n    value\n`).diagnostics,
    [],
  );
  assert.deepEqual(
    analyze(
      `${user}\nfn view(value: Option[mut User]) -> mut Option[User]:\n    value\n`,
    ).diagnostics.map((diagnostic) => diagnostic.code),
    ["type-mismatch"],
  );
});

test("optional type identity includes payload mut and not outer mut", () => {
  const environment = {
    nominal: (name: string) => name === "User",
    inspectableParameter: () => false,
  };
  assert.deepEqual(inspectKey("(mut:User)?", environment), ["(", "mut ", "User", ")?"]);
  assert.deepEqual(inspectKey("mut:User?", environment), ["User", "?"]);
  assert.equal(inspectKey("(mut:User?)?", environment)?.join(""), "(mut (User?))?");
});

test("mutation through generic aliases, nested optionals, propagation and matches survives Wasm erasure", async () => {
  const source = `${user}
type Maybe[T] = T?

fn change(value: Maybe[mut User]) -> Maybe[mut User]:
    let item: mut User = value?
    item.name = item.name + 1
    item

fn keep[T](value: T) -> T: value

fn main() -> i32:
    let mut item = User { name: 40 }
    let present: Option[mut User] = keep(item)
    let nested: Option[Option[mut User]] = .Some(change(present))
    match nested:
        .Some(inner) =>
            match inner:
                .Some(value) => value.name = value.name + 1
                .None => pass
        .None => pass
    item.name
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("the REPL renders invariant optional mutable payloads without erasing their generic permissions", async () => {
  const session = new ReplSession();
  for (const input of [
    user.trim(),
    "let mut item = User { name: 42 }",
    "let present: Option[mut User] = item",
  ]) {
    const outcome = await session.evaluate(input);
    assert.deepEqual(outcome.errors, []);
    assert.equal(outcome.accepted, true);
  }
  const outcome = await session.evaluate("present");
  assert.deepEqual(outcome.errors, []);
  assert.equal(outcome.value, ".Some(User { name: 42 })");
});

test("optional functions and tuples retain mutable optional arguments in emitted closure signatures", async () => {
  const source = `${user}
fn change(value: (mut User)?) -> (mut User)?:
    let item: mut User = value?
    item.name = item.name + 2
    item

fn apply(callback: (fn((mut User)?) -> (mut User)?)?, value: (mut User)?) -> (mut User)?:
    function := callback?
    function(value)

fn main() -> i32:
    let mut item = User { name: 40 }
    let pair: ((mut User)?, (fn((mut User)?) -> (mut User)?)?) = (item, .Some(change))
    match apply(pair._1, pair._0):
        .Some(value) => value.name
        .None => 0
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mut User? supplies mutable payload access through inferred let mut and matching", async () => {
  const source = `${user}
fn find() -> mut User?: .Some(User { name: 40 })

fn main() -> i32:
    let mut found = find()
    match found:
        .Some(value) =>
            value.name = value.name + 2
            value.name
        .None => 0
`;
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable optional payload annotations grant only their declared payload access", () => {
  assert.deepEqual(
    analyze(`${user}
fn find() -> mut User?: .Some(User { name: 40 })
fn mutate() -> void:
    let mut found: mut User? = find()
    match found:
        .Some(value) => value.name = 42
        .None => pass
`).diagnostics.map((diagnostic) => diagnostic.code),
    ["redundant-let-mut"],
  );
  const readonlyPayload = analyze(`${user}
fn mutate(found: mut (User?)) -> void:
    match found:
        .Some(value) => value.name = 42
        .None => pass
`).diagnostics;
  assert.equal(readonlyPayload.length, 1);
  assert.equal(readonlyPayload[0]!.code, "readonly-root");
});

test("mutable primitive and tuple payloads reject while mutable optional constructors remain valid", () => {
  for (const type of ["mut i32?", "Option[mut i32]"])
    assert.equal(
      analyze(`fn invalid(value: ${type}) -> void: pass\n`).diagnostics[0]?.code,
      "mut-on-primitive",
    );
  for (const type of ["mut (i32,)?", "Option[mut (i32,)]"])
    assert.equal(
      analyze(`fn invalid(value: ${type}) -> void: pass\n`).diagnostics[0]?.code,
      "mut-on-tuple",
    );
  for (const type of ["mut Option[i32]", "mut (i32?)", "mut Option[(i32,)]"])
    assert.deepEqual(analyze(`fn valid(value: ${type}) -> void: pass\n`).diagnostics, []);
});

test("typed build uses the shared optional constructor for declared mutable members", () => {
  const source = readFileSync(
    new URL("../spec/conformance/typing/valid/typed-derivation-build.hd", import.meta.url),
    "utf8",
  );
  assert.deepEqual(analyze(source).diagnostics, []);
});

test("the REPL preserves mutable outer optionals nested inside another optional", async () => {
  const session = new ReplSession();
  for (const input of [
    user.trim(),
    "let inside: mut Option[User] = .Some(User { name: 42 })",
    "let present: Option[mut Option[User]] = inside",
  ]) {
    const outcome = await session.evaluate(input);
    assert.deepEqual(outcome.errors, []);
    assert.equal(outcome.accepted, true);
  }
  const outcome = await session.evaluate("present");
  assert.deepEqual(outcome.errors, []);
  assert.equal(outcome.value, ".Some(.Some(User { name: 42 }))");
});
