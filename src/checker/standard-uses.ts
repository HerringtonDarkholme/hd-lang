import type { Program } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { standardDeclarationNames, standardPublicNames } from "./standard-library.ts";
import { STANDARD_MODULES } from "./standard-sources.ts";

// Checks a program's `std` uses before anything joins the standard library:
// a use path must name a std module (`unknown-module`), and each name it
// selects must be declared there (`unknown-import`,
// spec/lang/10-modules.md#r-module.use.missing-name) and `pub`
// (`private-import`, spec/lang/10-modules.md#r-module.use.private-name).

/** The names that the compiler, not `lib/std`, provides in a std module. */
const COMPILER_NAMES: ReadonlyMap<string, readonly string[]> = new Map([
  ["convert", ["From"]],
  ["error", ["Error"]],
  ["function", ["Fn", "SuspendFn"]],
  ["inspect", ["Inspectable", "TypeId", "downcast_val"]],
  [
    "structure",
    [
      "Structure",
      "Facts",
      "Member",
      "VariantInfo",
      "SelfRef",
      "Field",
      "Variant",
      "Key",
      "Members",
      "Walker",
      "Describer",
      "Source",
    ],
  ],
  ["task", ["all", "block_on", "Waker"]],
  ["testing", ["it", "assert_equal", "snapshot", "it_each", "it_prop", "it_prop_with"]],
]);

/**
 * Std modules by path below `std`, each with the file that declares its
 * names. `lib/std/arbitrary.hd` is `std.testing.arbitrary`
 * (checker/arbitrary-module.ts), so `std.arbitrary` is not a module.
 */
const MODULE_FILES: ReadonlyMap<string, string | undefined> = new Map<string, string | undefined>([
  ...STANDARD_MODULES.filter((module) => module !== "arbitrary").map((module): [string, string] => [
    module,
    module,
  ]),
  ...[...COMPILER_NAMES.keys()]
    .filter((module) => !(STANDARD_MODULES as readonly string[]).includes(module))
    .map((module): [string, undefined] => [module, undefined]),
  ["testing.arbitrary", "arbitrary"],
]);

/** Whether a std module declares `name` publicly, privately, or not at all. */
function declares(module: string, name: string): "public" | "private" | undefined {
  if (MODULE_FILES.has(`${module}.${name}`)) return "public";
  if ((COMPILER_NAMES.get(module) ?? []).includes(name)) return "public";
  const file = MODULE_FILES.get(module);
  if (file === undefined) return undefined;
  if ((standardPublicNames(file) ?? []).includes(name)) return "public";
  return (standardDeclarationNames(file) ?? []).includes(name) ? "private" : undefined;
}

/** Diagnostics for `std` uses that name no std module, or no public declaration of one. */
export function standardUseDiagnostics(program: Program): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (const use of program.uses) {
    const [root, ...path] = use.module.split(".");
    if (root !== "std") continue;
    const module = path.join(".");
    // `use std.text` names the module itself.
    if (module === "") {
      for (const { name } of use.names)
        if (!MODULE_FILES.has(name))
          diagnostics.push({
            code: "unknown-module",
            message: `no std module 'std.${name}'`,
            span: use.span,
          });
      continue;
    }
    if (!MODULE_FILES.has(module)) {
      diagnostics.push({
        code: "unknown-module",
        message: `no std module 'std.${module}'`,
        span: use.span,
      });
      continue;
    }
    for (const { name } of use.names) {
      const declared = declares(module, name);
      if (declared === "private")
        diagnostics.push({
          code: "private-import",
          message: `'${name}' is private to module 'std.${module}'`,
          span: use.span,
        });
      else if (declared === undefined)
        diagnostics.push({
          code: "unknown-import",
          message: `module 'std.${module}' declares no '${name}'`,
          span: use.span,
        });
    }
  }
  return diagnostics;
}
