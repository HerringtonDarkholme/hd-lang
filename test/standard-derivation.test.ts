import assert from "node:assert/strict";
import test from "node:test";

import type { DataDecl, Program } from "../src/ast.ts";
import { parse } from "../src/parser/index.ts";
import { check } from "../src/checker/program.ts";
import { withTypeDeclarations } from "../src/checker/type-declarations.ts";
import { reachableProgram } from "../src/emitter/reachability.ts";
import { withStandardLibrary } from "../src/checker/standard-library.ts";
import { withStandardTraits } from "../src/checker/standard-traits.ts";
import {
  structureRenames,
  withTypedDerivation,
  withTypedDerivationSupport,
} from "../src/checker/typed-derivation.ts";

function parsed(source: string): Program {
  const result = parse(source);
  assert.deepEqual(result.diagnostics, []);
  assert.ok(result.program);
  return result.program;
}

function derivedData(name: string, standard: boolean): DataDecl {
  const declaration = parsed(`@derive(Debug)\ndata ${name}:\n    value: i32\n`).data[0]!;
  return standard
    ? { ...declaration, standard: true, standardName: `std.probe.${name}` }
    : declaration;
}

test("std.structure bindings remain stable after support is declared", () => {
  const source = parsed("data Facts: pass\n\n@derive(Debug)\ndata Value: pass\n");
  const before = structureRenames(source);
  const supported = withTypedDerivationSupport(source);
  const after = structureRenames(supported);

  assert.equal(before.get("Facts"), "hd__structure_Facts");
  assert.equal(after.get("Facts"), "hd__structure_Facts");
  assert.ok(
    supported.data.some(
      (declaration) =>
        declaration.name === "hd__structure_Facts" &&
        declaration.standardName === "std.structure.Facts",
    ),
  );
});

test("derivation support loads only for reachable tuple shapes", () => {
  const empty = withStandardLibrary(parsed(""));
  assert.strictEqual(withTypedDerivationSupport(empty), empty);

  const tuple = withStandardLibrary(parsed("pub fn pair() -> (i32, i32): (1, 2)\n"));
  assert.notStrictEqual(withTypedDerivationSupport(tuple), tuple);
});

test("joined std declarations resolve non-prelude derivation templates by identity", () => {
  const joined = withStandardLibrary(parsed("use std.testing.it\n"));
  const arbitrary = joined.traits.find(
    (declaration) => declaration.standardName === "std.testing.Arbitrary",
  );
  assert.ok(arbitrary);
  const declaration = derivedData("StdArbitrary", true);
  const source: Program = {
    ...joined,
    data: [
      ...joined.data,
      {
        ...declaration,
        decorators: {
          ...declaration.decorators!,
          derives: declaration.decorators!.derives.map((derive) => ({
            ...derive,
            name: arbitrary.name,
          })),
        },
      },
    ],
  };
  const supported = withStandardTraits(withTypedDerivationSupport(source));
  const result = withTypedDerivation(supported);

  assert.deepEqual(result.diagnostics, []);
  assert.ok(
    result.program.implementations.some(
      (implementation) =>
        implementation.targetName === "StdArbitrary" &&
        implementation.traitName === arbitrary.name &&
        implementation.standard,
    ),
  );
});

test("compiler-loaded inspect declarations are present before derivation support", () => {
  const joined = withStandardLibrary(parsed("use std.inspect.TypeId\n"));
  const inspected = withStandardTraits(joined);
  const derive = derivedData("Probe", false).decorators!;
  const typeId = inspected.data.find(
    (declaration) => declaration.standardName === "std.inspect.TypeId",
  );
  assert.ok(typeId);
  const annotated: Program = {
    ...inspected,
    data: inspected.data.map((declaration) =>
      declaration === typeId ? { ...declaration, decorators: derive } : declaration,
    ),
  };
  const supported = withStandardTraits(withTypedDerivationSupport(annotated));
  const result = withTypedDerivation(supported);

  assert.equal(
    supported.traits.filter((declaration) => declaration.standardName === "std.inspect.Inspectable")
      .length,
    1,
  );
  assert.ok(
    supported.data.some((declaration) => declaration.standardName === "std.structure.Facts"),
  );
  assert.deepEqual(result.diagnostics, []);
  assert.ok(
    result.program.implementations.some(
      (implementation) => implementation.targetName === typeId.name && implementation.standard,
    ),
  );
});

test("facts on std declarations still receive semantic check functions", () => {
  const source = parsed("@missing\ndata StdFact: pass\n");
  const result = check({
    ...source,
    data: [{ ...source.data[0]!, standard: true, standardName: "std.probe.StdFact" }],
  });

  assert.ok(result.diagnostics.some((diagnostic) => diagnostic.code === "unknown-name"));
});

test("std newtypes retain declaration provenance when lowered", () => {
  const source = parsed("type StdId(i32)\n");
  const result = withTypeDeclarations({
    ...source,
    types: [{ ...source.types![0]!, standard: true, standardName: "std.probe.StdId" }],
  });

  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.program.data[0]?.standard, true);
  assert.equal(result.program.data[0]?.standardName, "std.probe.StdId");
});

test("unused std derivation helpers are not roots but called helpers remain reachable", () => {
  const analyze = (body: string) => {
    const source = parsed(`@derive(Debug)\ndata StdValue:\n    value: i32\n${body}`);
    const result = check({
      ...source,
      data: [{ ...source.data[0]!, standard: true, standardName: "std.probe.StdValue" }],
    });
    assert.deepEqual(result.diagnostics, []);
    assert.ok(result.program);
    return reachableProgram(result.program);
  };
  const unused = analyze("pub fn main() -> void: pass\n");
  const used = analyze("pub fn main() -> void:\n    _ := debug(StdValue { value: 1 })\n");

  assert.ok(!unused.functions.some((declaration) => declaration.name.includes("StdValue")));
  assert.ok(used.functions.some((declaration) => declaration.name.includes("StdValue")));
});

test("std declarations derive after joining without becoming emission roots", () => {
  const joined = withStandardLibrary(parsed(""));
  const source: Program = {
    ...joined,
    data: [...joined.data, derivedData("StdValue", true), derivedData("UserValue", false)],
  };
  const supported = withStandardTraits(withTypedDerivationSupport(source));
  const result = withTypedDerivation(supported);

  assert.deepEqual(result.diagnostics, []);
  for (const name of [
    "hd__s_StdValue_facts",
    "hd__s_StdValue_holds_0",
    "hd__s_StdValue_variant_0",
    "hd__s_StdValue_rfield_0_0_get",
  ])
    assert.equal(result.program.functions.find((item) => item.name === name)?.standard, true);
  assert.equal(
    result.program.functions.find((item) => item.name === "hd__s_UserValue_rfield_0_0_get")
      ?.standard,
    undefined,
  );

  const implementations = result.program.implementations.filter((item) =>
    /^(StdValue|UserValue)$/.test(item.targetName),
  );
  assert.ok(
    implementations.filter((item) => item.targetName === "StdValue").every((item) => item.standard),
  );
  assert.ok(
    implementations
      .filter((item) => item.targetName === "UserValue")
      .every((item) => !item.standard),
  );

  const bindings = result.program.statements.flatMap((statement) =>
    statement.kind === "binding" ? [statement] : [],
  );
  assert.ok(bindings.some((statement) => statement.name.includes("UserValue")));
  const standardBindings = bindings.filter((statement) => statement.name.includes("StdValue"));
  assert.ok(standardBindings.length > 0);
  assert.ok(standardBindings.every((statement) => statement.standard));
});
