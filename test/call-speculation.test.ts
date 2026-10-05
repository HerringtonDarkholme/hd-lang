import assert from "node:assert/strict";
import test from "node:test";

import { speculate, TRIAL_STATE, type TrialSnapshot } from "../src/checker/call-speculation.ts";
import { FunctionChecker } from "../src/checker/checker.ts";
import { SignatureInference } from "../src/checker/program-inference.ts";
import type { ProgramCheckContext } from "../src/checker/program-context.ts";
import type { Signature } from "../src/checker/context.ts";

test("production trials do not traverse immutable checker input registries", () => {
  const forbidden = new Proxy(
    {},
    {
      ownKeys: () => {
        throw new Error("immutable input traversed");
      },
    },
  );
  const local = { type: "i32", mutable: false };
  const key = Symbol("mutable checker cache");
  const checker = Object.assign(Object.create(FunctionChecker.prototype), {
    declaration: forbidden,
    signature: forbidden,
    signatures: forbidden,
    dataTypes: forbidden,
    enumTypes: forbidden,
    traitTypes: forbidden,
    implementations: forbidden,
    inherentMethods: forbidden,
    imports: forbidden,
    locals: [local],
    closures: [],
    globals: new Map(),
    prechecked: new Map(),
    [key]: { value: 1 },
  });
  Object.defineProperty(checker, "unusedAccessor", {
    get: () => {
      throw new Error("snapshot invoked accessor");
    },
  });
  speculate(checker, () => {
    local.type = "bool";
    checker.locals.push({ type: "bool", mutable: true });
    checker.prechecked.set("trial", true);
    checker[key].value = 2;
  });
  assert.equal(local.type, "i32");
  assert.equal(checker.locals.length, 1);
  assert.equal(checker.prechecked.size, 0);
  assert.equal(checker[key].value, 1);
});

test("partial participant discovery failures unwind journals before checking", () => {
  let active = 0;
  const participant = {
    [TRIAL_STATE](snapshot: TrialSnapshot, rollback: (reset: () => void) => void) {
      active++;
      rollback(() => {
        active--;
      });
      snapshot(new WeakMap());
    },
  };
  let checked = false;
  assert.throws(
    () =>
      speculate(participant, () => {
        checked = true;
      }),
    /opaque weak/,
  );
  assert.equal(active, 0);
  assert.equal(checked, false);
});

test("deep aliases upgrade earlier shallow snapshots", () => {
  const child = { value: 1 };
  const shared = [child];
  const participant = {
    [TRIAL_STATE](snapshot: TrialSnapshot) {
      snapshot(shared, false);
      snapshot(shared);
    },
  };
  speculate(participant, () => {
    child.value = 2;
    shared.push({ value: 3 });
  });
  assert.equal(child.value, 1);
  assert.deepEqual(shared, [child]);
});

test("sparse signature journals restore nested savepoints without traversing unrelated entries", () => {
  const context = { diagnostics: [] } as unknown as ProgramCheckContext;
  const inference = new SignatureInference(context, [], new Map(), [], undefined);
  const signatures = inference.signatures;
  const untouched = new Proxy({ result: "i32" } as Signature, {
    ownKeys: () => {
      throw new Error("unrelated signature traversed");
    },
  });
  for (let index = 0; index < 10_000; index++) signatures.set(`unrelated${index}`, untouched);
  const original = { result: "i32" } as Signature;
  const outer = { result: "bool" } as Signature;
  const inner = { result: "string" } as Signature;
  signatures.set("changed", original);
  signatures.keys = () => {
    throw new Error("unrelated signature keys enumerated");
  };
  signatures.entries = () => {
    throw new Error("unrelated signatures enumerated");
  };
  signatures[Symbol.iterator] = signatures.entries;
  speculate(signatures, () => {
    signatures.set("changed", outer);
    speculate(signatures, () => {
      signatures.set("changed", inner);
      signatures.set("new", inner);
    });
    assert.equal(signatures.peek("changed"), outer);
    assert.equal(signatures.has("new"), false);
    assert.equal(signatures.peek("unrelated0"), untouched);
    signatures.set("new", outer);
  });
  assert.equal(signatures.peek("changed"), original);
  assert.equal(signatures.has("new"), false);
  assert.equal(signatures.size, 10_001);
  assert.deepEqual([...Map.prototype.keys.call(signatures)].slice(0, 3), [
    "unrelated0",
    "unrelated1",
    "unrelated2",
  ]);
});

test("destructive signature operations preserve nested Map iteration order", () => {
  const inference = new SignatureInference(
    { diagnostics: [] } as unknown as ProgramCheckContext,
    [],
    new Map(),
    [],
    undefined,
  );
  const signatures = inference.signatures;
  const signature = { result: "i32" } as Signature;
  for (const name of ["first", "second", "third"]) signatures.set(name, signature);
  speculate(signatures, () => {
    signatures.set("outer", signature);
    signatures.delete("second");
    signatures.set("second", signature);
    const parentOrder = [...signatures.keys()];
    speculate(signatures, () => {
      signatures.set("inner", signature);
      signatures.clear();
      signatures.set("replacement", signature);
    });
    assert.deepEqual([...signatures.keys()], parentOrder);
    speculate(signatures, () => {
      signatures.delete("first");
      signatures.set("first", signature);
    });
    assert.deepEqual([...signatures.keys()], parentOrder);
  });
  assert.deepEqual([...signatures.keys()], ["first", "second", "third"]);
});

test("trials restore the complete reachable graph without replacing local identities", () => {
  const local = { type: "i32", mutable: false };
  const symbol = Symbol("pending");
  const state = {
    locals: [local],
    scopes: [new Map([["value", local]])],
    closures: [] as unknown[],
    captures: new Set([local]),
    inferred: undefined as string | undefined,
    [symbol]: local,
    frozen: Object.freeze({ value: 1 }),
  };
  assert.throws(() =>
    speculate(state, () => {
      local.type = "bool";
      local.mutable = true;
      state.locals.push({ type: "bool", mutable: true });
      state.scopes[0]!.clear();
      state.scopes.push(new Map());
      state.captures.clear();
      state.closures.push({ captures: [local] });
      state.inferred = "bool";
      Object.assign(state, { temporary: true });
      state[symbol] = { type: "string", mutable: true };
      throw new Error("failed candidate");
    }),
  );
  assert.deepEqual(local, { type: "i32", mutable: false });
  assert.equal(state.locals.length, 1);
  assert.equal(state.locals[0], local);
  assert.equal(state.scopes.length, 1);
  assert.equal(state.scopes[0]!.get("value"), local);
  assert.deepEqual([...state.captures], [local]);
  assert.deepEqual(state.closures, []);
  assert.equal(state.inferred, undefined);
  assert.equal(state[symbol], local);
  assert.equal(Object.hasOwn(state, "temporary"), false);
});

test("nested successful trials restore their own entry states", () => {
  const state = { local: { type: "i32" }, cache: new Map<string, string>() };
  const result = speculate(state, () => {
    state.local.type = "bool";
    state.cache.set("outer", "bool");
    speculate(state, () => {
      state.local.type = "string";
      state.cache.clear();
    });
    assert.equal(state.local.type, "bool");
    assert.equal(state.cache.get("outer"), "bool");
    return 42;
  });
  assert.equal(result, 42);
  assert.equal(state.local.type, "i32");
  assert.equal(state.cache.size, 0);
});

test("cyclic graphs restore aliases and opaque weak state fails before checking", () => {
  const state: { value: number; self?: object; aliases: Map<object, object> } = {
    value: 1,
    aliases: new Map(),
  };
  state.self = state;
  state.aliases.set(state, state);
  speculate(state, () => {
    state.value = 2;
    state.self = {};
    state.aliases.clear();
  });
  assert.equal(state.value, 1);
  assert.equal(state.self, state);
  assert.equal(state.aliases.get(state), state);
  for (const opaque of [new WeakMap(), new WeakSet()]) {
    let entered = false;
    assert.throws(
      () =>
        speculate({ opaque }, () => {
          entered = true;
        }),
      /opaque weak collections/,
    );
    assert.equal(entered, false);
  }
});
