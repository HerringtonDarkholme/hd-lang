import type { Expression, ModuleScope, Program } from "../ast.ts";
import { headName } from "./template-instances.ts";
import type { SourceSpan } from "../diagnostics.ts";

// Package ownership in a linked program (src/package.ts). The linker joins
// the root package's modules and each dependency package's library modules
// into one source, and gives each module a scope over its lines with the
// package it belongs to. The checker reads it here to tell which package
// declares a type, trait, or fact, and which names a module imports, so that
// the rules that depend on the package hold across packages:
//
// - a member without `pub` is not visible from another package
//   (spec/lang/10-modules.md#r-module.vis.members);
// - a trait of another package is available only where a use imports it
//   (spec/lang/09-traits.md#r-trait.avail.module);
// - an implementation needs a package that owns the trait, the target, or
//   a trait argument (spec/lang/09-traits.md#r-trait.own.rule);
// - a per-trait `Self` line warns when the fact's package does not supply
//   the trait (spec/lang/14-annotations.md#r-annot.fact.unused-self-line.per-trait).
//
// A program without module scopes, as a single file, is one package. A
// `lib/std` declaration carries its own `standard` mark; callers check that
// first, since its span is not a line of the joined source.

/** The root package's key; a dependency package's is its id. */
export const ROOT_PACKAGE = "";

export interface PackageOwnership {
  /** The package whose module holds `span`. */
  packageOf(span: SourceSpan): string;
  /** Whether the module that holds `span` imports the declaration joined as `name`. */
  imports(span: SourceSpan, name: string): boolean;
}

function scopeAt(scopes: readonly ModuleScope[], line: number): ModuleScope | undefined {
  return scopes.find(({ firstLine, lastLine }) => firstLine <= line && line <= lastLine);
}

/** The program's package ownership; undefined when it links no dependency package. */
export function packageOwnership(program: Program): PackageOwnership | undefined {
  const scopes = program.packageScopes?.scopes ?? [];
  if (!scopes.some((scope) => scope.package !== undefined)) return undefined;
  const imported = new Map<ModuleScope, ReadonlySet<string>>();
  return {
    packageOf: (span) => scopeAt(scopes, span.start.line)?.package ?? ROOT_PACKAGE,
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

/** The ownership recorded under `key`, or undefined for a program of one package. */
export function registeredPackageOwnership(key: object): PackageOwnership | undefined {
  return registered.get(key);
}

/**
 * The `Self` lines of per-trait derivation blocks that write a fact whose
 * package does not supply the block's trait: that package declares neither
 * the trait nor a template for it
 * (spec/lang/14-annotations.md#r-annot.fact.unused-self-line.per-trait).
 * `lineFacts` and `nonLiteral` read a line's facts as typed derivation does.
 */
export function foreignSelfLines(
  program: Program,
  lineFacts: (value: Expression) => readonly Expression[],
  nonLiteral: (fact: Expression) => boolean,
): SourceSpan[] {
  const ownership = packageOwnership(program);
  if (!ownership) return [];
  const supplies = (packageName: string, trait: string): boolean =>
    program.traits.some(
      (declaration) =>
        declaration.name === trait && ownership.packageOf(declaration.span) === packageName,
    ) ||
    program.implementations.some(
      (template) =>
        template.byStructure !== undefined &&
        !template.standard &&
        template.traitName !== undefined &&
        headName(template.traitName) === trait &&
        ownership.packageOf(template.span) === packageName,
    );
  const factPackage = (fact: Expression): string | undefined => {
    const name =
      fact.kind === "data"
        ? fact.name
        : fact.kind === "call" && fact.callee.kind === "name"
          ? fact.callee.name
          : undefined;
    const declaration = [...program.data, ...program.enums].find(
      (candidate) => candidate.name === name,
    );
    return declaration && !declaration.standard ? ownership.packageOf(declaration.span) : undefined;
  };
  const spans: SourceSpan[] = [];
  for (const block of program.implementations) {
    if (block.byStructure === undefined || block.standard || block.traitName === undefined)
      continue;
    const trait = headName(block.traitName);
    for (const line of block.memberLines ?? [])
      if (
        line.name === "Self" &&
        line.value &&
        lineFacts(line.value)
          .filter(nonLiteral)
          .map(factPackage)
          .some((packageName) => packageName !== undefined && !supplies(packageName, trait))
      )
        spans.push(line.span);
  }
  return spans;
}
