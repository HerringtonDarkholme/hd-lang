import type { ModuleScope, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";

// Module and package ownership in a linked program (src/package.ts). The
// linker joins the root package's modules and each dependency package's
// library modules into one source, and gives each module a scope over its
// lines with the package it belongs to. The checker reads it here to tell
// which module and package declare a type, trait, or fact, and which names a
// module imports, so that the rules that depend on them hold:
//
// - a member without `pub` is not visible from another module, of the same
//   package or not (spec/lang/10-modules.md#r-module.vis.members);
// - a trait is available only in the module that declares it or where a use
//   imports it (spec/lang/09-traits.md#r-trait.avail.module);
// - an implementation needs a package that owns the trait, the target, or
//   a trait argument (spec/lang/09-traits.md#r-trait.own.rule).
//
// A program without module scopes, as a single file, is one module. A
// `lib/std` declaration carries its own `standard` mark; callers check that
// first, since its span is not a line of the joined source.

/** The root package's key; a dependency package's is its id. */
export const ROOT_PACKAGE = "";

export interface PackageOwnership {
  /** The package whose module holds `span`. */
  packageOf(span: SourceSpan): string;
  /**
   * Whether `left` and `right` lie in one module. A span outside every
   * module, as of code the checker generates, counts as in each module.
   */
  sameModule(left: SourceSpan, right: SourceSpan): boolean;
  /** Whether the module that holds `span` imports the declaration joined as `name`. */
  imports(span: SourceSpan, name: string): boolean;
}

function scopeAt(scopes: readonly ModuleScope[], line: number): ModuleScope | undefined {
  return scopes.find(({ firstLine, lastLine }) => firstLine <= line && line <= lastLine);
}

/** The program's ownership; undefined when it is not a linked package. */
export function packageOwnership(program: Program): PackageOwnership | undefined {
  const scopes = program.packageScopes?.scopes ?? [];
  if (scopes.length === 0) return undefined;
  const imported = new Map<ModuleScope, ReadonlySet<string>>();
  return {
    packageOf: (span) => scopeAt(scopes, span.start.line)?.package ?? ROOT_PACKAGE,
    sameModule(left, right) {
      const scope = scopeAt(scopes, left.start.line);
      const other = scopeAt(scopes, right.start.line);
      return scope === undefined || other === undefined || scope === other;
    },
    imports(span, name) {
      const scope = scopeAt(scopes, span.start.line);
      if (!scope) return false;
      let names = imported.get(scope);
      if (!names) {
        names = new Set(scope.imports ?? []);
        imported.set(scope, names);
      }
      return names.has(name);
    },
  };
}

const registered = new WeakMap<object, PackageOwnership | undefined>();

/**
 * Records `program`'s ownership under `key`, an object every function check
 * of the program shares, such as its trait map.
 */
export function registerPackageOwnership(key: object, program: Program): void {
  registered.set(key, packageOwnership(program));
}

/** The ownership recorded under `key`, or undefined for a program of one module. */
export function registeredPackageOwnership(key: object): PackageOwnership | undefined {
  return registered.get(key);
}
