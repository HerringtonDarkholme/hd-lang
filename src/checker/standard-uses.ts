import type { Program } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { standardDeclarationNames, standardPublicNames } from "./standard-library.ts";
import { STANDARD_CORE_NAMES } from "./standard-core.ts";
import { STANDARD_MODULES } from "./standard-sources.ts";

// Checks a program's `std` uses before anything joins the standard library:
// a use path must name a std module (`unknown-module`), and each name it
// selects must be declared there (`unknown-import`,
// spec/lang/10-modules.md#r-module.use.missing-name) and `pub`
// (`private-import`, spec/lang/10-modules.md#r-module.use.private-name).

/** The names that the compiler, not `lib/std`, provides in a std module. */
const COMPILER_NAMES: ReadonlyMap<string, readonly string[]> = new Map([
  ["core", STANDARD_CORE_NAMES],
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
 * names. `std.prelude` and `std.prelude.testing` are the prototype's files
 * of the prelude's `use` lines, which the specification names no module.
 */
const MODULE_FILES: ReadonlyMap<string, string | undefined> = new Map<string, string | undefined>([
  ...STANDARD_MODULES.filter((module) => !module.startsWith("prelude")).map(
    (module): [string, string] => [module, module],
  ),
  ...[...COMPILER_NAMES.keys()]
    .filter((module) => !(STANDARD_MODULES as readonly string[]).includes(module))
    .map((module): [string, undefined] => [module, undefined]),
]);

/** Whether `module`, a path below `std` such as `testing.arbitrary`, is a std module. */
export function isStandardModulePath(module: string): boolean {
  return MODULE_FILES.has(module);
}

/**
 * Whether a std module declares `name` publicly, privately, or not at all:
 * what a use of it, or a module path to it, may select. A child module and a
 * compiler-provided name count as public.
 */
export function declares(module: string, name: string): "public" | "private" | undefined {
  if (MODULE_FILES.has(`${module}.${name}`)) return "public";
  if ((COMPILER_NAMES.get(module) ?? []).includes(name)) return "public";
  const file = MODULE_FILES.get(module);
  if (file === undefined) return undefined;
  if ((standardPublicNames(file) ?? []).includes(name)) return "public";
  return (standardDeclarationNames(file) ?? []).includes(name) ? "private" : undefined;
}

/**
 * The roots a single-file program may not use
 * (spec/lang/10-modules.md#r-module.single-file.roots). The package linker
 * removes every package use before the checker sees a package's program, so
 * a use with one of these roots reaches the checker only in a single file.
 */
const PACKAGE_ROOTS: ReadonlySet<string> = new Set(["pkg", "dep", "self", "super"]);

/** The message of a package use in a single-file program. */
export const SINGLE_FILE_USE = "the file is a single-file program, in no package";

/**
 * Diagnostics for `std` uses that name no std module, or no public declaration
 * of one, and for package uses in a single-file program.
 */
export function standardUseDiagnostics(program: Program): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (const use of program.uses) {
    const [root, ...path] = use.module.split(".");
    if (PACKAGE_ROOTS.has(root!)) {
      diagnostics.push({
        code: "unknown-module",
        message: `'${root}' names no module: ${SINGLE_FILE_USE}, so it may use only std`,
        span: use.span,
      });
      continue;
    }
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
