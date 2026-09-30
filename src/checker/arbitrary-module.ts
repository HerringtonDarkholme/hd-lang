import type { Expression, Program } from "../ast.ts";

// The module `std.testing.arbitrary` (spec/std/testing.md#derived-arbitrary).
// The prototype has no std submodules, so the module is
// `lib/std/arbitrary.hd`, joined as `std.arbitrary`. Derived `Arbitrary`
// itself is the `std.testing` template (lib/std/testing.hd), which the
// typed-derivation pass instantiates like any other.

/** The hidden name of `std.testing.arbitrary.with` (lib/std/arbitrary.hd). */
const ARBITRARY_WITH = "__std_arbitrary_with";

function transform(node: unknown, callback: (value: Record<string, unknown>) => unknown): unknown {
  if (Array.isArray(node)) return node.map((item) => transform(item, callback));
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(node)) result[key] = transform(value, callback);
  return callback(result);
}

/**
 * `use std.testing.arbitrary` imports a module, which the prototype has no
 * values for: each `arbitrary.with(gen)` becomes a call of `with` by its
 * hidden name, which joins `lib/std/arbitrary.hd`.
 */
export function withArbitraryModule(program: Program): Program {
  const aliases = new Set<string>();
  const uses = program.uses.flatMap((use) => {
    if (use.module !== "std.testing") return [use];
    const names = use.names.filter((name) => {
      if (name.name !== "arbitrary") return true;
      aliases.add(name.alias ?? name.name);
      return false;
    });
    if (names.length === use.names.length) return [use];
    return names.length > 0 ? [{ ...use, names }] : [];
  });
  if (aliases.size === 0) return program;
  return transform({ ...program, uses }, (value) => {
    const callee = value.callee as Expression | undefined;
    if (
      value.kind === "call" &&
      callee?.kind === "member" &&
      callee.name === "with" &&
      callee.receiver.kind === "name" &&
      aliases.has(callee.receiver.name)
    )
      return { ...value, callee: { kind: "name", name: ARBITRARY_WITH, span: callee.span } };
    return value;
  }) as Program;
}

/** The local name of a `std.testing` declaration, or its hidden name. */
export function testingName(program: Program, name: string): string | undefined {
  for (const use of program.uses)
    if (use.module === "std.testing")
      for (const imported of use.names)
        if (imported.name === name) return imported.alias ?? imported.name;
  return undefined;
}
