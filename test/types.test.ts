import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { analyze } from "../src/compiler.ts";
import { parse } from "../src/parser/index.ts";
import { ReplSession } from "../src/repl.ts";
import { inspectKey } from "../src/checker/inspectable.ts";
import { unresolvedTypeMessage } from "../src/checker/cannot-infer.ts";
import { matchGenericTypePattern, resolveGenericType } from "../src/checker/shared.ts";
import {
  displayType,
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

test("display rendering hides compiler-internal names but keeps user spellings", () => {
  for (const [internal, shown] of [
    ["generic:T", "T"],
    ["generic:hd_E1", "element 1"],
    ["hd_E1", "element 1"],
    ["trait:ToJson", "ToJson"],
    ["trait:__std_json_ToJson", "ToJson"],
    ["__std_json_ToJson", "ToJson"],
    ["__std_iter_FromIterator[E]", "FromIterator[E]"],
    ["$impl352.member", "member"],
    ["$inherent110.collect", "collect"],
    ["row:R", "$ R"],
    ["R$row:R", "R$ R"],
    ["fn(*Args)->O$row:R", "fn(*Args) -> O$ R"],
    ["Map[K,V]", "Map[K, V]"],
    ["mut List[generic:T]", "mut List[T]"],
    ["i32", "i32"],
    ["Result[void, HiddenError]", "Result[void, HiddenError]"],
  ])
    assert.equal(displayType(internal!), shown);
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

test("optional type identity includes payload mut and not outer mut", () => {
  const environment = {
    nominal: (name: string) => name === "User",
    inspectableParameter: () => false,
  };
  assert.deepEqual(inspectKey("(mut:User)?", environment), ["(", "mut ", "User", ")?"]);
  assert.deepEqual(inspectKey("mut:User?", environment), ["User", "?"]);
  assert.equal(inspectKey("(mut:User?)?", environment)?.join(""), "(mut (User?))?");
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
  assert.equal(outcome.value, "Some(User { name: 42 })");
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
  assert.equal(outcome.value, "Some(Some(User { name: 42 }))");
});
