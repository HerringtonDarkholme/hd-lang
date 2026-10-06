import type { ModuleScope, PackageScopes, Program } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import {
  FOREIGN,
  resolveModuleBindings,
  type ModuleBindings,
  type PathScope,
} from "./standard-bindings.ts";
import { standardTypeSpelling } from "./standard-library.ts";
import { declares, isStandardModulePath } from "./standard-uses.ts";

// Module paths (spec/lang/05-expressions.md#names): a qualified name selects
// a declaration through a module namespace, as `words.squash(word)`,
// `cmp.Ordering`, `cmp.Ordering.Less`, or `text.StringBuilder::new()`.
//
// This pass runs first in the checker. It resolves each module path to the
// spelling under which the program joins the declaration it names, so the
// rest of the checker sees ordinary names. A path to a declaration without
// `pub` is `private-import`, and to one the module lacks is `unknown-import`,
// as a use of it would be (expr.name.qualified.private,
// expr.name.qualified.missing).
//
// In a linked package it also applies each module's scope from the linker
// (`Program.packageScopes`, src/package.ts): the module's own spellings and
// renamed uses map to their joined spellings, and its namespace uses of
// package modules resolve through the linker's member tables. A bare name
// that only another module declares is an error, `unknown-name` for a value
// and `unknown-type` or `unknown-trait` in a type, whose message says how to
// import it (03-names-and-scopes.md#r-names.module.declarations,
// 10-modules.md#r-module.vis.private-default).
//
// A std namespace use, as `use std.cmp`, stays in the program, and a path
// through it to a type becomes the type's joined spelling. A path to a std
// function stays as written: the checker resolves it by declaration
// identity (checker/calls.ts).

/** A namespace's key: a package module identity or a std module path. */
const packageKey = (identity: string): string => `pkg:${identity}`;
const standardKey = (module: string): string => `std:${module}`;

export function withModulePaths(program: Program): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  const standard = standardNamespaces(program);
  const scopes = program.packageScopes;
  const needed =
    standard.size > 0 ||
    (scopes?.scopes.some(
      ({ names, namespaces, foreign }) =>
        Object.keys(names).length > 0 ||
        Object.keys(namespaces).length > 0 ||
        foreign !== undefined,
    ) ??
      false);
  if (!needed) return { program, diagnostics: [] };

  const diagnostics: Diagnostic[] = [];
  const reported = new Set<string>();
  const report = (code: string, message: string, span: SourceSpan | undefined): undefined => {
    const at = span ?? program.span;
    const key = `${at.start.offset}:${at.end.offset}:${code}:${message}`;
    if (!reported.has(key)) diagnostics.push({ code, message, span: at });
    reported.add(key);
    return undefined;
  };
  const typeSpelling = standardTypeSpelling(program);
  const paths: PathScope = {
    resolve(key, member, span, position) {
      if (key.startsWith("std:")) {
        const module = key.slice("std:".length);
        const visibility = declares(module, member);
        if (visibility === "private")
          return report("private-import", `'${member}' is private to module 'std.${module}'`, span);
        if (visibility === undefined)
          return report("unknown-import", `module 'std.${module}' declares no '${member}'`, span);
        const spelled = typeSpelling(module, member);
        if (spelled !== undefined) return spelled;
        // A public name with no type spelling is a child module (or a
        // compiler name): a path cannot reach it through its parent
        // (10-modules.md#r-module.path.no-std-child-import).
        if (isStandardModulePath(`${module}.${member}`))
          return report(
            position === "value" ? "unknown-name" : "unknown-type",
            `'${member}' is a child module of 'std.${module}', which a path can't reach through its parent; import it with \`use std.${module}.${member}\``,
            span,
          );
        return undefined;
      }
      const target = scopes?.modules[key.slice("pkg:".length)];
      if (!target) return undefined;
      const joined = target.members[member];
      if (joined === null)
        return report(
          "private-import",
          `'${member}' is private to module '${target.shown}'; mark it 'pub'`,
          span,
        );
      if (joined === undefined) {
        const child = target.children[member];
        if (child !== undefined) {
          const parent = child.slice(0, child.lastIndexOf("."));
          return report(
            "unknown-import",
            `'${member}' is a child module of '${parent}', which a path can't reach through its parent; import it with \`use ${child}\`, or have the parent re-export it with \`pub use\``,
            span,
          );
        }
        return report("unknown-import", `module '${target.shown}' declares no '${member}'`, span);
      }
      return joined;
    },
  };
  const outside: ModuleBindings = { names: new Map(), namespaces: standard, paths };
  const bindings = new Map<ModuleScope, ModuleBindings>();
  const scopeOf = (span: SourceSpan): ModuleBindings => {
    const scope = moduleScopeAt(scopes, span.start.line);
    if (!scope) return outside;
    let found = bindings.get(scope);
    if (!found) {
      const namespaces = new Map(standard);
      for (const [local, identity] of Object.entries(scope.namespaces))
        namespaces.set(local, packageKey(identity));
      const names = new Map(Object.entries(scope.names));
      const foreign = scope.foreign ?? {};
      for (const name of Object.keys(foreign)) names.set(name, `${FOREIGN}${name}`);
      const reporting: PathScope = {
        resolve: paths.resolve,
        foreign(name, span, position) {
          const { trait, hint } = foreign[name]!;
          const kind = position === "value" ? "name" : trait ? "trait" : "type";
          report(`unknown-${kind}`, `unknown ${kind} '${name}': ${hint}`, span);
        },
      };
      found = { names, namespaces, paths: reporting };
      bindings.set(scope, found);
    }
    return found;
  };
  return { program: resolveModuleBindings(program, scopeOf), diagnostics };
}

function moduleScopeAt(scopes: PackageScopes | undefined, line: number): ModuleScope | undefined {
  return scopes?.scopes.find(({ firstLine, lastLine }) => firstLine <= line && line <= lastLine);
}

/**
 * The program's std namespace uses, as `use std.cmp` or
 * `use std.testing.arbitrary`: each local name with its namespace key.
 */
function standardNamespaces(program: Program): Map<string, string> {
  const namespaces = new Map<string, string>();
  for (const use of program.uses) {
    if (use.module !== "std" && !use.module.startsWith("std.")) continue;
    const parent = use.module.slice("std.".length);
    for (const { name, alias } of use.names) {
      const module = use.module === "std" ? name : `${parent}.${name}`;
      if (isStandardModulePath(module)) namespaces.set(alias ?? name, standardKey(module));
    }
  }
  return namespaces;
}
