import { writtenVariantGadt } from "./gadt.ts";
import type {
  DataDecl,
  Decorators,
  EnumDecl,
  Expression,
  FunctionDecl,
  ImplDecl,
  MemberLine,
  MethodDecl,
  Program,
  Statement,
  TraitDecl,
  TypeDecl,
  TypeRef,
  UseDecl,
} from "../ast.ts";
import { DiagnosticError, type Diagnostic, type SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";
import { checkLawPartners, DERIVE_CHECKED_TRAITS, derivedFieldSpan } from "./derive-intrinsics.ts";
import { carriedLibraryUses, renameStandardBindings } from "./standard-bindings.ts";
import { deriveMissing, expandTypeAlias, nullaryTypeAliases } from "./derive-aliases.ts";
import { standardTemplate, standardTupleTraits } from "./standard-library.ts";
import { withStandardSource } from "./standard-provenance.ts";
import { standardDocument } from "./standard-sources.ts";
import {
  loadTupleTemplates,
  localTupleName,
  tupleShapesInJoinedProgram,
  tupleInstances,
  type TupleInstance,
} from "./tuple-templates.ts";
import { displayType, optionalType, readonlyType } from "../types.ts";
import { selfRefScope, type SelfRefScope } from "./self-ref.ts";
import {
  effectiveFacts,
  variantModels,
  type MemberModel,
  type VariantModel,
} from "./derivation-models.ts";
import {
  compileTemplate,
  forwardNewtype,
  headName,
  instanceImplementations,
  renameWords,
  sourceMemberBound,
  STRUCTURE,
  TUPLE_REST,
  transform,
  traversalGenerics,
  visit,
  withBound,
  type CompiledTemplate,
  type TraversalGenerics,
} from "./template-instances.ts";
import { withTypedFacts } from "./typed-facts.ts";
import { factsOfBuilders, importsFactsOf, STRUCTURE_FACT } from "./function-facts.ts";
import { checkDuplicateDeclarationFacts } from "./declaration-facts.ts";
import {
  checkMemberLines,
  isSpreadFact,
  withInlinedListLines,
  type Target,
  withTraitLessBlocks,
} from "./member-lines.ts";

// Typed derivation (spec/lang/14-annotations.md#typed-derivation), lowered before
// checking. Each template is checked once (`compileTemplate`): its methods
// become generic functions over the template's `T`, bounded by a hidden
// trait that stands for `T`'s `Structure` in that template, with one method
// per `walk`, `describe`, or `build` call site. Each `@derive(X)` of a trait
// with a template, and each derivation block `impl X for T by Structure:`,
// becomes two ordinary implementations (`generateDerivation`): the hidden
// trait for the target, whose methods call traversal functions specialized
// to the target and to the walker, describer, or source type, and `X` for
// the target, whose methods call the template's functions. The template
// must hold the walker, describer, or source in a local declared with its
// type. The handles, facts, and member information are values of the
// `std.structure` declarations in `lib/std/structure.hd`, which the pass
// declares itself, with hidden fields for the compiler-supplied bodies.
//
// Prototype gaps: `Facts` holds `Inspectable` values rather than `Any`, so
// each fact list erases its values through `hd__structure_fact`, which
// names any type; `VariantInfo.shared` is always empty; a build
// handle's `get` returns the member's declared type whatever the argument's
// permission; the traversals are generated hd source, parsed and checked
// per derivation; `Walker.rest`'s default body panics, since generated
// `walk` calls `member` for a walker without `rest`, which is what the
// default does (annot.walk.rest.default);
// drift and unused-fact warnings treat every local trait as one package.

const STRUCTURE_MODULE = "std.structure";
const PROTOCOL_TRAITS = new Set(["Walker", "Describer", "Source"]);

/** The checker intrinsic that panics with `structure-variant-mismatch`. */
export const STRUCTURE_MISMATCH = "hd__structure_variant_mismatch";
/** The checker intrinsic that gives a build handle its member's declared type. */
export const STRUCTURE_AS_DECLARED = "hd__structure_as_declared";
/**
 * The checker intrinsic `hd__structure_witness::[X]()`: an `Inspectable`
 * value whose dictionary names `X`, held by a handle so that `h.fact`
 * needs no `Inspectable` bound on the handle's `F` (annot.handle.fact.key).
 */
export const STRUCTURE_WITNESS = "hd__structure_witness";
/** The handle field that holds the witness, which `h.fact` reads (lib/std/structure.hd). */
export const STRUCTURE_WITNESS_FIELD = "hd_witness";
const DOWNCAST = "hd__downcast_val";
/**
 * The std template whose `build` fills a member that declares a default from
 * that default, without its source's bounded `member`, so the member's type
 * needs no `Default` (spec/std/ops.md#r-std-ops.default.derive.member-bound.declared).
 */
const DECLARED_DEFAULT_TEMPLATE = "Default";

/**
 * `lib/std/structure.hd` in the program's names. Its `use` lines become the
 * pass's own use of `std.inspect`, which imports `downcast_val` under a
 * hidden name. Other uses resolve against the joined declarations by
 * standard name (checker/standard-bindings.ts).
 */
interface DerivationResult {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
  /** Opt-in spans whose generated member calls report `member-not-derivable`. */
  readonly optInSpans: readonly SourceSpan[];
  /**
   * Derived `Arbitrary` opt-ins, whose unmet member bound stays
   * `unsatisfied-trait-bound` and names the member
   * (std-testing.arbitrary.derive.not-derivable).
   */
  readonly arbitraryOptIns: readonly ArbitraryOptIn[];
}

interface ArbitraryOptIn {
  readonly span: SourceSpan;
  readonly members: readonly { readonly name: string; readonly type: string }[];
}

interface Derivation {
  readonly trait: string;
  readonly target: Target;
  readonly lines: readonly MemberLine[];
  readonly block?: ImplDecl;
  readonly span: SourceSpan;
  /** A tuple template's instance for one tuple shape (annot.template.tuple.instance). */
  readonly tuple?: TupleInstance;
}

// ---------------------------------------------------------------------------

function writtenStructureImports(program: Program): Set<string> {
  const names = new Set<string>();
  for (const use of program.uses)
    if (use.module === STRUCTURE_MODULE && !use.standard)
      for (const name of use.names) names.add(name.name);
  return names;
}

/** The declarations of `std.structure` that the pass declares. */
const STRUCTURE_DECLARATIONS = [
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
];

/**
 * The names under which the pass declares `std.structure`'s items, when not
 * their own: an imported item takes its local name, and one whose name the
 * program declares itself, such as a `data Key`, a hidden name.
 */
export function structureRenames(program: Program): Map<string, string> {
  const existing = new Map(
    [...program.data, ...program.enums, ...program.traits].flatMap((declaration) => {
      const prefix = `${STRUCTURE_MODULE}.`;
      return declaration.standardName?.startsWith(prefix)
        ? [[declaration.standardName.slice(prefix.length), declaration.name] as const]
        : [];
    }),
  );
  const declared = new Set(
    [...program.data, ...program.enums, ...program.traits, ...(program.types ?? [])].map(
      (declaration) => declaration.name,
    ),
  );
  const imported = new Map<string, string>();
  for (const use of program.uses)
    if (use.module === STRUCTURE_MODULE && !use.standard)
      for (const name of use.names) imported.set(name.name, name.alias ?? name.name);
  const renames = new Map<string, string>();
  for (const name of STRUCTURE_DECLARATIONS) {
    const local =
      existing.get(name) ??
      imported.get(name) ??
      (declared.has(name) ? `hd__structure_${name}` : name);
    if (local !== name) renames.set(name, local);
  }
  return renames;
}

function needsStructureDeclarations(program: Program): boolean {
  const derives = [...(program.types ?? []), ...program.data, ...program.enums].some(
    (declaration) => (declaration.decorators?.derives.length ?? 0) > 0,
  );
  const blocks = program.implementations.some(
    (implementation) => implementation.byStructure !== undefined,
  );
  const standardTuples =
    tupleShapesInJoinedProgram(program).length > 0 && standardTupleTraits(program).length > 0;
  // `std.serde` names `Facts`, `VariantInfo`, and `Member` (14-annotations.md#serialization).
  const serde = program.traits.some((item) => item.standardName?.startsWith("std.serde."));
  return (
    serde ||
    writtenStructureImports(program).size > 0 ||
    derives ||
    blocks ||
    standardTuples ||
    importsFactsOf(program)
  );
}

/**
 * Declares the compiler-provided std.structure module before derivation.
 * Its own std declarations can therefore use @derive, and every derivation
 * sees the same declaration identities rather than a late appended copy.
 */
export function withTypedDerivationSupport(source: Program): Program {
  if (
    !needsStructureDeclarations(source) ||
    source.data.some((declaration) => declaration.standardName === "std.structure.Facts")
  )
    return source;
  const renames = structureRenames(source);
  const document = standardDocument("structure");
  const parsed = parse(document.text, { standardLibrary: true });
  if (!parsed.program)
    throw new DiagnosticError(
      parsed.diagnostics.map((diagnostic) =>
        withStandardSource(diagnostic, document, diagnostic.span),
      ),
    );
  const original = parsed.program;
  const carried = carriedLibraryUses(source, original, ["std.inspect"]);
  const structure = renameStandardBindings(
    withStandardSource(original, document, source.span),
    new Map([...carried, ...renames, ["downcast_val", DOWNCAST]]),
  );
  const alreadyImports = (name: string, local = name): boolean =>
    source.uses.some(
      (use) =>
        use.module === "std.inspect" &&
        use.names.some((imported) => imported.name === name && (imported.alias ?? name) === local),
    );
  const inspectUse: UseDecl = {
    kind: "use",
    module: "std.inspect",
    names: [
      ...(!alreadyImports("Inspectable") ? [{ name: "Inspectable" }] : []),
      ...(!alreadyImports("downcast_val", DOWNCAST)
        ? [{ name: "downcast_val", alias: DOWNCAST }]
        : []),
    ],
    standard: true,
    span: structure.uses.find((use) => use.module === "std.inspect")?.span ?? source.span,
  };
  const standard = <T>(items: readonly T[]): Array<T & { readonly standard: true }> =>
    items.map((item) => ({ ...item, standard: true as const }));
  const data = structure.data.map((declaration, index) => ({
    ...declaration,
    standard: true as const,
    standardName: `std.structure.${original.data[index]!.name}`,
  }));
  const enums = structure.enums.map((declaration, index) => ({
    ...declaration,
    standard: true as const,
    standardName: `std.structure.${original.enums[index]!.name}`,
  }));
  const traits = structure.traits.map((declaration, index) => ({
    ...declaration,
    standard: true as const,
    standardName: `std.structure.${original.traits[index]!.name}`,
    strengthenableMembers: ["member", "rest"],
  }));
  const carriedUses = inspectUse.names.length > 0 ? [...source.uses, inspectUse] : source.uses;
  return {
    ...source,
    uses: carriedUses,
    ...(structure.types ? { types: [...(source.types ?? []), ...standard(structure.types)] } : {}),
    data: [...source.data, ...data],
    enums: [...source.enums, ...enums],
    traits: [...source.traits, ...traits],
    implementations: [...source.implementations, ...standard(structure.implementations)],
    functions: [...source.functions, ...standard(structure.functions)],
  };
}

/** An enum with a variant that refines its result or declares its own parameters (13-gadts.md). */
function isGadt(declaration: EnumDecl): boolean {
  return declaration.variants.some((variant) => writtenVariantGadt(declaration, variant));
}

/** The concrete type a fact expression evaluates to, when it is evident from syntax. */
/**
 * A variant's `@default` marker, if its metadata holds one: a bare `default`
 * name that resolves to the `DefaultVariant` marker function, or a fact
 * call whose type is `DefaultVariant`
 * (std-ops.default.derive.marker).
 */
function defaultVariantMark(
  metadata: readonly Expression[],
  functions: ReadonlyMap<string, FunctionDecl>,
): Expression | undefined {
  // A fact type names the hidden `__std_ops_DefaultVariant` rename, so the
  // comparison goes through the display form, as diagnostics do.
  const isDefaultVariant = (type: string): boolean => displayType(type) === "DefaultVariant";
  return metadata.find((fact) => {
    if (fact.kind === "name") {
      const declaration = functions.get(fact.name);
      return (
        declaration !== undefined &&
        !declaration.resultOmitted &&
        isDefaultVariant(readonlyType(declaration.result.name))
      );
    }
    return isDefaultVariant(factType(fact, functions));
  });
}

export function factType(
  expression: Expression,
  functions: ReadonlyMap<string, FunctionDecl>,
): string {
  if (expression.kind === "data") return expression.name;
  if (expression.kind === "call" && expression.callee.kind === "name") {
    const declaration = functions.get(expression.callee.name);
    if (declaration && !declaration.resultOmitted) return readonlyType(declaration.result.name);
    return `call:${expression.callee.name}`;
  }
  if (expression.kind === "string" || expression.kind === "interpolated-string") return "string";
  if (expression.kind === "integer") return "i32";
  if (expression.kind === "boolean") return "bool";
  return `expression:${JSON.stringify(stripSpans(expression))}`;
}

function stripSpans(node: unknown): unknown {
  return transform(node, (value) => {
    const { span: _span, ...rest } = value;
    return rest;
  });
}

/** Whether a statement list mentions `Structure::walk` or `T::describe`/`T::build`. */
function callsTraversal(statements: readonly unknown[], parameter: string): boolean {
  let found = false;
  visit(statements, (value) => {
    if (value.kind !== "call") return;
    const callee = value.callee as Expression;
    if (callee.kind !== "qualified-name") return;
    if (callee.owner === STRUCTURE && callee.name === "walk") found = true;
    if (callee.owner === parameter && (callee.name === "describe" || callee.name === "build"))
      found = true;
  });
  return found;
}

// ---------------------------------------------------------------------------

export function withTypedDerivation(source: Program): DerivationResult {
  const diagnostics: Diagnostic[] = [];
  const optInSpans: SourceSpan[] = [];
  const arbitraryOptIns: ArbitraryOptIn[] = [];
  const imported = writtenStructureImports(source);
  // The names of std.structure's items in this program.
  const renames = structureRenames(source);
  const structureName = (name: string): string => renames.get(name) ?? name;
  const structureVisible = imported.has(STRUCTURE);
  const functions = new Map(source.functions.map((item) => [item.name, item] as const));
  const error = (code: string, message: string, span: SourceSpan): void => {
    diagnostics.push({ code, message, span });
  };

  checkUnderivableTargets(source, error);

  checkDuplicateDeclarationFacts(source, (fact) => factType(fact, functions), error);
  // Trait-less blocks edit the declaration facts (annot.traitless.declaration-facts).
  const fact = (expression: Expression): string => factType(expression, functions);
  const knownType = lineTypes(source, functions);
  // Typed facts get their expected types once trait-less lines are folded in
  // (annot.typed-fact.check).
  const program = withTypedFacts(
    withTraitLessBlocks(withInlinedListLines(source), structureVisible, fact, knownType, error),
    error,
  );
  for (const declaration of program.functions)
    if (!functions.has(declaration.name)) functions.set(declaration.name, declaration);
  const localTraits = new Map(program.traits.map((item) => [item.name, item] as const));
  const newtypes = new Map(
    (program.types ?? [])
      .filter((item) => item.base !== undefined)
      .map((item) => [item.name, item] as const),
  );
  const targets = new Map<string, Target>([
    ...program.data.map((item) => [item.name, { kind: "data", declaration: item }] as const),
    ...program.enums.map((item) => [item.name, { kind: "enum", declaration: item }] as const),
  ]);

  // Templates and derivation blocks.
  const { templates, tupleTemplates, blocks, kept } = sortImplementations(
    program,
    structureVisible,
    localTraits,
    error,
  );

  // `Structure` named outside a template (annot.structure.named-positions).
  if (structureVisible)
    checkStructureMentions(
      program,
      new Set([...templates, ...tupleTemplates].map(([, item]) => item)),
      error,
    );

  // `member` through a generic walker, describer, or source (annot.walker.generic-member-call).
  if (imported.size > 0) checkGenericMemberCalls(program, error);

  // Opt-ins.
  const derivations: Derivation[] = [];
  const derivedPairs = new Map<string, SourceSpan>();
  const newtypeDerivations: { trait: string; declaration: TypeDecl; span: SourceSpan }[] = [];
  // A std trait derives through its std template, which the program's
  // derivations instantiate with the walker or source implementations of
  // its module: `std.testing.Arbitrary` (spec/std/testing.md#derived-arbitrary),
  // and the comparison traits and `Hash` (spec/std/cmp.md, spec/std/hash.md).
  const arbitrary = program.uses
    .filter((use) => use.module === "std.testing")
    .flatMap((use) => use.names)
    .find((imported) => imported.name === "Arbitrary");
  const arbitraryName = arbitrary && (arbitrary.alias ?? arbitrary.name);
  const standardTemplates = new Map<string, { readonly support: readonly ImplDecl[] }>();
  const loadStandard = (name: string): void => {
    if (templates.has(name)) return;
    const local = localTraits.get(name);
    if (local && !local.standard) return;
    const standard = standardTemplate(program, name, renames);
    if (!standard) return;
    templates.set(name, standard.template);
    standardTemplates.set(name, standard);
  };
  const optIn = (target: Target | undefined, declaration: DataDecl | EnumDecl | TypeDecl): void => {
    for (const trait of declaration.decorators?.derives ?? []) {
      const name = trait.name;
      loadStandard(name);
      if (!templates.has(name)) {
        error(...deriveMissing(localTraits, name), trait.span);
        continue;
      }
      if (!target) {
        newtypeDerivations.push({
          trait: name,
          declaration: declaration as TypeDecl,
          span: trait.span,
        });
        continue;
      }
      if (target.kind === "enum" && isGadt(target.declaration)) {
        error(
          "gadt-derivation",
          `enum '${target.declaration.name}' is a GADT and cannot be derived through a template`,
          trait.span,
        );
        continue;
      }
      if (name === "Default" && target.kind === "enum") {
        // A derived enum's default is its variant marked `@default`
        // (std-ops.default.derive.one-variant): exactly one marker.
        const marked = target.declaration.variants.flatMap((variant) => {
          const mark = defaultVariantMark(variant.metadata ?? [], functions);
          return mark ? [{ variant, mark }] : [];
        });
        if (marked.length === 0) {
          error(
            "invalid-default-variant",
            `enum '${target.declaration.name}' derives Default with no variant marked @default; mark exactly one variant`,
            trait.span,
          );
          continue;
        }
        if (marked.length > 1) {
          error(
            "invalid-default-variant",
            `enum '${target.declaration.name}' marks more than one variant @default; keep exactly one`,
            marked[1]!.mark.span,
          );
          continue;
        }
      }
      derivedPairs.set(`${name} ${declaration.name}`, trait.span);
      derivations.push({ trait: name, target, lines: [], span: trait.span });
    }
  };
  for (const declaration of program.data) optIn(targets.get(declaration.name), declaration);
  for (const declaration of program.enums) optIn(targets.get(declaration.name), declaration);
  for (const declaration of newtypes.values()) optIn(undefined, declaration);

  for (const block of blocks) {
    const trait = headName(block.traitName!);
    const targetName = headName(block.targetName);
    if (newtypes.has(targetName)) {
      error(
        "misplaced-derivation",
        "a newtype derives only through its base type, with @derive",
        block.span,
      );
      continue;
    }
    loadStandard(trait);
    const target = targets.get(targetName);
    if (!templates.has(trait)) {
      error("underivable-trait", `trait '${trait}' has no derivation template`, block.span);
      continue;
    }
    if (!target) {
      error(
        "misplaced-derivation",
        `a derivation block must target a data type or enum declared in this module`,
        block.span,
      );
      continue;
    }
    if (derivedPairs.has(`${trait} ${targetName}`)) {
      error(
        "overlapping-impl",
        `'${trait}' is already derived for '${targetName}' by @derive`,
        block.span,
      );
      continue;
    }
    if (target.kind === "enum" && isGadt(target.declaration)) {
      error(
        "gadt-derivation",
        `enum '${targetName}' is a GADT and cannot be derived through a template`,
        block.span,
      );
      continue;
    }
    const lines = block.memberLines ?? [];
    if (!checkMemberLines(target, lines, fact, knownType, error)) continue;
    derivations.push({ trait, target, lines, block, span: block.span });
  }

  // Warnings: line drift and decorators attached to derivation blocks.
  lintDerivations(source, derivations, diagnostics);

  if (diagnostics.some((item) => item.severity !== "warning"))
    return { program, diagnostics, optInSpans, arbitraryOptIns };

  // Tuple templates (annot.template.tuple.*), and the std ones that the
  // program needs.
  const tuples = loadTupleTemplates(program, localTraits, tupleTemplates, kept, renames, error);
  const { standardTuples, shapes } = tuples;
  if (diagnostics.some((item) => item.severity !== "warning"))
    return { program, diagnostics, optInSpans, arbitraryOptIns };

  // Generation.
  const generated: Program[] = [];
  const implementations: ImplDecl[] = [...kept];

  // A newtype forwards to its base type, so only a type's derivation uses them.
  const usedStandard = new Set(derivations.map((derivation) => derivation.trait));
  const templateSupport = templateParts([
    ...[...standardTemplates].filter(([trait]) => usedStandard.has(trait)).map(([, item]) => item),
    ...(shapes.length > 0 ? standardTuples.values() : []),
  ]);

  const scope = selfRefScope(program);
  // Each template is checked once, as generic functions, whatever the
  // number of its derivations (annot.template.checked).
  const compiledTemplates = new Map<string, CompiledTemplate>();
  const compiledTemplate = (trait: string): CompiledTemplate => {
    let compiled = compiledTemplates.get(trait);
    if (!compiled) {
      compiled = compileTemplate(
        templates.get(trait)!,
        [...program.implementations, ...templateSupport],
        renames,
        error,
      );
      compiledTemplates.set(trait, compiled);
    }
    return compiled;
  };
  const definedParts = new Set<string>();
  checkLawPartners(program, error);
  derivations.forEach((derivation, index) => {
    // A member that fails derived `Arbitrary`'s bounds is
    // `unsatisfied-trait-bound` (std-testing.arbitrary.derive.not-derivable).
    // A compared or hashed member reports at the field instead
    // (trait.derive.field-missing-trait).
    const standard = standardTemplates.get(derivation.trait);
    const checked = standard !== undefined && DERIVE_CHECKED_TRAITS.has(derivation.trait);
    if (derivation.trait === arbitraryName)
      arbitraryOptIns.push({
        span: derivation.span,
        members: memberTypes(derivation.target, nullaryTypeAliases(source.types)),
      });
    else if (!checked) optInSpans.push(derivation.span);
    const result = generateDerivation(
      derivation,
      index,
      compiledTemplate(derivation.trait),
      standard ? sourceMemberBound(standard.support, structureName("Source")) : [],
      scope,
      checked,
      standard !== undefined && derivation.trait === DECLARED_DEFAULT_TEMPLATE,
      renames,
      definedParts,
    );
    if (!result) return;
    implementations.push(result.implementation, result.structure);
    generated.push(result.program);
  });
  const context = { program, templateSupport, renames, error, compiledTemplates };
  const instances = tupleInstances(
    { ...tuples, tupleTemplates },
    context,
    (item, index, compiled) =>
      generateDerivation(
        item,
        derivations.length + index,
        compiled,
        [],
        scope,
        false,
        false,
        renames,
        definedParts,
      ),
  );
  implementations.push(...instances.implementations);
  generated.push(...instances.programs);
  const templateDeclarations = [...compiledTemplates.values()].map((item) => item.declarations);
  const newtypeHelpers = new Map<string, FunctionDecl>();
  const standardNewtypeHelpers = new Map<string, boolean>();
  for (const item of newtypeDerivations) {
    // A std trait whose methods all take `self` forwards through a bounded
    // helper, so a base type without it is `derive-field-missing-trait`
    // (trait.derive.newtype.requires.error).
    const template = templates.get(item.trait)!;
    const checked =
      standardTemplates.has(item.trait) &&
      template.methods.every((method) => method.parameters[0]?.name === "self");
    const usedHelpers = new Set<string>();
    const result = forwardNewtype(item, template, checked, newtypeHelpers, usedHelpers, error);
    for (const name of usedHelpers) {
      standardNewtypeHelpers.set(
        name,
        (standardNewtypeHelpers.get(name) ?? true) && item.declaration.standard === true,
      );
    }
    if (result)
      implementations.push(
        item.declaration.standard ? { ...result, standard: true as const } : result,
      );
  }
  implementations.push(...templateSupport);
  if (diagnostics.some((item) => item.severity !== "warning"))
    return { program, diagnostics, optInSpans, arbitraryOptIns };

  const factFunctions = factCheckFunctions(program);
  return {
    program: {
      ...program,
      // Handle constants come first, so a module binding may call a derived method.
      statements: [...generated.flatMap((item) => item.statements), ...program.statements],
      uses: program.uses.filter((use) => use.module !== STRUCTURE_MODULE),
      data: [...program.data, ...generated.flatMap((item) => item.data)],
      enums: program.enums,
      traits: [...program.traits, ...templateDeclarations.map((item) => item.trait)],
      implementations: [...implementations, ...generated.flatMap((item) => item.implementations)],
      functions: [
        ...program.functions,
        ...[...newtypeHelpers.values()].map((declaration) =>
          standardNewtypeHelpers.get(declaration.name)
            ? { ...declaration, standard: true as const }
            : declaration,
        ),
        ...factFunctions,
        ...factsOfBuilders(program, structureName("Facts"), unscoped),
        ...templateDeclarations.flatMap((item) => item.functions),
        ...generated.flatMap((item) => item.functions),
      ],
    },
    diagnostics,
    optInSpans,
    arbitraryOptIns,
  };
}

/** The walker and source implementations of the std templates in use, once each. */
function templateParts(
  standards: readonly { readonly support: readonly ImplDecl[] }[],
): ImplDecl[] {
  const supportKeys = new Set<string>();
  const templateSupport: ImplDecl[] = [];
  for (const standard of standards) {
    for (const implementation of standard.support) {
      const key = `${implementation.traitName} ${implementation.targetName}`;
      if (supportKeys.has(key)) continue;
      supportKeys.add(key);
      templateSupport.push(implementation);
    }
  }
  return templateSupport;
}

/** The known type of a member line's right side (annot.line.right-typed). */
function lineTypes(
  source: Program,
  functions: ReadonlyMap<string, FunctionDecl>,
): (value: Expression) => string | undefined {
  const moduleBindings = new Map(
    source.statements.flatMap((statement) =>
      statement.kind === "binding" && statement.annotation
        ? [[statement.name, statement.annotation.name] as const]
        : [],
    ),
  );
  return (value) => {
    if (value.kind === "name") return moduleBindings.get(value.name);
    if (value.kind === "call" && value.callee.kind === "name") {
      const declaration = functions.get(value.callee.name);
      return declaration && !declaration.resultOmitted ? declaration.result.name : undefined;
    }
    return undefined;
  };
}

/** Each member's name and declared type, for a diagnostic that names the member. */
function memberTypes(
  target: Target,
  aliases: ReadonlyMap<string, string>,
): { readonly name: string; readonly type: string }[] {
  const fields =
    target.kind === "data"
      ? target.declaration.fields
      : target.declaration.variants.flatMap((variant) => variant.fields);
  // The bound failure spells the member's type expanded, so a member
  // written through an alias matches by its expansion (O-09).
  return fields.map((field) => ({
    name: field.name,
    type: expandTypeAlias(field.type.name, aliases),
  }));
}

/** Splits the implementations into templates, derivation blocks, and the rest. */
function sortImplementations(
  program: Program,
  structureVisible: boolean,
  localTraits: ReadonlyMap<string, TraitDecl>,
  error: (code: string, message: string, span: SourceSpan) => void,
): {
  templates: Map<string, ImplDecl>;
  tupleTemplates: Map<string, ImplDecl>;
  blocks: ImplDecl[];
  kept: ImplDecl[];
} {
  const templates = new Map<string, ImplDecl>();
  // `impl[T < Tuple] Trait for T by Structure` (annot.template.tuple.form).
  const tupleTemplates = new Map<string, ImplDecl>();
  const tupleName = localTupleName(program);
  const blocks: ImplDecl[] = [];
  const kept: ImplDecl[] = [];
  for (const implementation of program.implementations) {
    const traitHead = implementation.traitName && headName(implementation.traitName);
    if (traitHead === STRUCTURE && structureVisible) {
      error(
        "sealed-trait-implementation",
        "Structure is sealed: a type gets it only by opting in to a derivation",
        implementation.span,
      );
      continue;
    }
    if (!implementation.byStructure) {
      if (implementation.memberLines?.length)
        error(
          "misplaced-derivation",
          "a member line is valid only in a derivation block",
          implementation.memberLines[0]!.span,
        );
      kept.push(implementation);
      continue;
    }
    if (!structureVisible) {
      error(
        "unknown-trait",
        "unknown trait 'Structure'; import it with use std.structure.Structure",
        implementation.byStructure,
      );
      continue;
    }
    const isTemplate =
      implementation.genericParameters.length === 1 &&
      implementation.targetName === implementation.genericParameters[0];
    const isTuple =
      isTemplate &&
      implementation.genericBounds.some(
        (bound) =>
          bound.parameter === implementation.genericParameters[0] &&
          bound.traits.includes(tupleName),
      );
    const found = isTuple ? tupleTemplates : templates;
    if (isTemplate) {
      if (implementation.memberLines?.length) {
        error(
          "misplaced-derivation",
          "a member line is valid only in a derivation block, not in a template",
          implementation.memberLines[0]!.span,
        );
        continue;
      }
      const trait = traitHead!;
      if (!localTraits.has(trait)) {
        error(
          "misplaced-derivation",
          `the template of '${trait}' must be declared in the module that declares the trait`,
          implementation.span,
        );
        continue;
      }
      if (found.has(trait)) {
        error(
          "overlapping-impl",
          `trait '${trait}' already has a ${isTuple ? "tuple " : "derivation "}template`,
          implementation.span,
        );
        continue;
      }
      const parameter = implementation.genericParameters[0]!;
      if (
        !implementation.methods.some(
          (method) => method.body && callsTraversal(method.body, parameter),
        )
      ) {
        error(
          "marker-template",
          `the template of '${trait}' must have a method that walks, describes, or builds`,
          implementation.span,
        );
        continue;
      }
      found.set(trait, implementation);
    } else blocks.push(implementation);
  }
  return { templates, tupleTemplates, blocks, kept };
}

function checkStructureMentions(
  program: Program,
  templateSet: ReadonlySet<ImplDecl>,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const checkBounds = (
    bounds: readonly { traits: readonly string[]; span: SourceSpan }[],
    span?: SourceSpan,
  ): void => {
    for (const bound of bounds)
      if (bound.traits.some((trait) => headName(trait) === STRUCTURE))
        error(
          "structure-outside-template",
          "Structure may bound nothing outside a derivation template",
          span ?? bound.span,
        );
  };
  for (const declaration of program.functions) checkBounds(declaration.genericBounds);
  for (const implementation of program.implementations) {
    if (templateSet.has(implementation)) continue;
    checkBounds(implementation.genericBounds);
    for (const method of implementation.methods) checkBounds(method.genericBounds);
  }
  for (const trait of program.traits)
    for (const method of trait.methods) checkBounds(method.genericBounds);
  const scan = (node: unknown): void =>
    visit(node, (value) => {
      if (value.kind === "qualified-name" && value.owner === STRUCTURE)
        error(
          "structure-outside-template",
          "Structure may be named only inside a derivation template",
          value.span as SourceSpan,
        );
    });
  scan(program.functions.map((item) => item.body));
  scan(program.statements);
  for (const implementation of program.implementations)
    if (!templateSet.has(implementation)) scan(implementation.methods);
}

function checkGenericMemberCalls(
  program: Program,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const check = (
    genericBounds: readonly { parameter: string; traits: readonly string[] }[],
    parameters: readonly { name: string; type: TypeRef }[],
    body: readonly Statement[] | undefined,
  ): void => {
    const protocolParameters = new Set(
      genericBounds
        .filter((bound) => bound.traits.some((trait) => PROTOCOL_TRAITS.has(headName(trait))))
        .map((bound) => bound.parameter),
    );
    if (protocolParameters.size === 0 || !body) return;
    const genericLocals = new Set(
      parameters
        .filter((parameter) => protocolParameters.has(readonlyType(parameter.type.name)))
        .map((parameter) => parameter.name),
    );
    visit(body, (value) => {
      if (value.kind !== "call") return;
      const callee = value.callee as Expression;
      if (
        callee.kind === "member" &&
        (callee.name === "member" || callee.name === "rest") &&
        callee.receiver.kind === "name" &&
        genericLocals.has(callee.receiver.name)
      )
        error(
          "generic-member-call",
          `${callee.name} may be called through a generic walker, describer, or source only by generated code`,
          value.span as SourceSpan,
        );
    });
  };
  for (const declaration of program.functions)
    check(declaration.genericBounds, declaration.parameters, declaration.body);
  for (const implementation of program.implementations)
    for (const method of implementation.methods)
      check(
        [...implementation.genericBounds, ...method.genericBounds],
        method.parameters,
        method.body,
      );
}

function lintDerivations(
  program: Program,
  derivations: readonly Derivation[],
  diagnostics: Diagnostic[],
): void {
  const warn = (code: string, message: string, span: SourceSpan): void => {
    diagnostics.push({ code, message, span, severity: "warning" });
  };
  const byTarget = new Map<string, Derivation[]>();
  for (const derivation of derivations) {
    const name = derivation.target.declaration.name;
    byTarget.set(name, [...(byTarget.get(name) ?? []), derivation]);
  }
  for (const group of byTarget.values()) {
    const blocksHere = group.filter((item) => item.block);
    const hasDerive = group.some((item) => !item.block);
    const shapes = blocksHere.map((item) => JSON.stringify(stripSpans(item.lines)));
    blocksHere.forEach((item, index) => {
      if (index > 0 && shapes[index] !== shapes[0])
        warn(
          "derivation-line-drift",
          "this derivation block's member lines differ from an earlier block of the same package",
          item.block!.span,
        );
      else if (hasDerive && item.lines.length > 0)
        warn(
          "derivation-line-drift",
          "this derivation block has member lines, unlike a @derive of the same package",
          item.block!.span,
        );
    });
  }
  // A decorator before a derivation block attaches nothing a derivation
  // reads (annot.fact.unused-block-decorator); the fix-it moves it into the
  // block (annot.fact.unused-block-decorator.fix).
  for (const block of program.implementations) {
    if (block.byStructure === undefined || block.standard) continue;
    for (const fact of block.decorators?.facts ?? [])
      warn(
        "unused-derivation-fact",
        "no derivation reads a decorator before a derivation block; move it into the block as a `Self += [...]` line",
        fact.span,
      );
  }
}

/**
 * `@derive` stays an intrinsic for data types, enums, and newtypes; before a
 * function, trait, implementation, or method it is rejected
 * (annot.decorator.function-derive, annot.decorator.derive-targets).
 */
function checkUnderivableTargets(
  source: Program,
  error: (code: string, message: string, span: SourceSpan) => void,
): void {
  const underivable = (decorators: Decorators | undefined, what: string): void => {
    if (decorators && decorators.derives.length > 0)
      error(
        "decorator-not-annotator",
        `@derive applies to a data type, enum, or newtype, not to ${what}`,
        decorators.span,
      );
  };
  for (const declaration of source.functions) underivable(declaration.decorators, "a function");
  for (const declaration of source.traits) {
    underivable(declaration.decorators, "a trait");
    for (const method of declaration.methods) underivable(method.decorators, "a method");
  }
  for (const declaration of source.implementations) {
    underivable(declaration.decorators, "an implementation");
    for (const method of declaration.methods) underivable(method.decorators, "a method");
  }
}

/**
 * A typed fact whose check needs a generic scope is held as its value
 * alone, since the facts functions have none.
 */
function unscoped(fact: Expression): Expression {
  return fact.kind === "call" && fact.typedFactScope ? fact.arguments[0]! : fact;
}

/**
 * One requirement-free function per fact or metadata expression, so each is
 * checked as a compile-time expression (annot.fact.eval, annot.metadata.eval).
 */
function factCheckFunctions(program: Program): FunctionDecl[] {
  const factChecks = new Source_();
  let factCount = 0;
  const standardChecks = new Set<number>();
  const checkFacts = (expressions: readonly Expression[] | undefined, standard = false): void => {
    for (const expression of expressions ?? []) {
      // A typed fact on a generic target checks in its monomorphic scope
      // (annot.typed-fact.monomorphic).
      const scope = expression.kind === "call" ? (expression.typedFactScope ?? "") : "";
      factChecks.add(`fn hd__fact_check_${factCount}${scope}() -> void:`);
      factChecks.add(`    _ := ${factChecks.expression(expression)}`);
      if (standard) standardChecks.add(factCount);
      factCount += 1;
    }
  };
  for (const declaration of program.data) {
    checkFacts(declaration.decorators?.facts, declaration.standard);
    for (const field of declaration.fields) checkFacts(field.metadata, declaration.standard);
  }
  for (const declaration of program.enums) {
    checkFacts(declaration.decorators?.facts, declaration.standard);
    for (const variant of declaration.variants) {
      checkFacts(variant.metadata, declaration.standard);
      for (const field of variant.fields) checkFacts(field.metadata, declaration.standard);
    }
  }
  for (const declaration of program.functions) {
    checkFacts(declaration.decorators?.facts, declaration.standard);
    for (const parameter of declaration.parameters)
      checkFacts(parameter.metadata, declaration.standard);
  }
  // Values before a trait, implementation, newtype, or method
  // (annot.decorator.attach), and a method's parameter metadata.
  const methods = (list: readonly MethodDecl[], standard = false): void => {
    for (const method of list) {
      checkFacts(method.decorators?.facts, standard);
      for (const parameter of method.parameters) checkFacts(parameter.metadata, standard);
    }
  };
  for (const declaration of program.traits) {
    checkFacts(declaration.decorators?.facts, declaration.standard);
    methods(declaration.methods, declaration.standard);
  }
  for (const declaration of program.implementations) {
    checkFacts(declaration.decorators?.facts, declaration.standard);
    methods(declaration.methods, declaration.standard);
  }
  for (const declaration of program.types ?? [])
    checkFacts(declaration.decorators?.facts, declaration.standard);
  const factProgram = factCount > 0 ? factChecks.program(ZERO_SPAN) : undefined;
  return (factProgram?.functions ?? []).map((declaration, index): FunctionDecl => ({
    ...declaration,
    compilerGenerated: true,
    ...(standardChecks.has(index) ? { standard: true as const } : {}),
    span: (declaration.body[0] as { value?: Expression }).value?.span ?? declaration.span,
    defaultContext: { laterNames: [] },
  }));
}

// ---------------------------------------------------------------------------
// Generation of one derivation.

export interface Generated {
  readonly implementation: ImplDecl;
  /** The target's `Structure` for the template. */
  readonly structure: ImplDecl;
  readonly program: Program;
}

function generateDerivation(
  derivation: Derivation,
  index: number,
  compiled: CompiledTemplate,
  memberBound: readonly string[],
  scope: SelfRefScope,
  checked: boolean,
  /** `build` fills a member that declares a default from that default (DECLARED_DEFAULT_TEMPLATE). */
  fillsDeclaredDefaults: boolean,
  renames: ReadonlyMap<string, string>,
  defined: Set<string>,
): Generated | undefined {
  const owner = derivation.target.declaration.name;
  const tuple = derivation.tuple;
  // A compared or hashed member is reported at its field (trait.derive.field-missing-trait).
  const memberSpan = (member: MemberModel): SourceSpan | undefined =>
    checked && !tuple
      ? derivedFieldSpan(member.field, compiled.template.traitName!, owner)
      : undefined;
  const target = derivation.target;
  const declaration = target.declaration;
  const standard =
    declaration.standard === true || (tuple !== undefined && compiled.template.standard === true);
  const parameters = declaration.genericParameters;
  const targetType = tuple
    ? tuple.type
    : parameters.length > 0
      ? `${declaration.name}[${parameters.join(",")}]`
      : declaration.name;
  const variants = tuple ? [tuple.variant] : variantModels(target, derivation.lines, scope);
  const out = new Source_(defined);
  const prefix = `hd__d${index}`;
  // The handles, variants, and facts of a derivation without member lines
  // are the same in every such derivation of the target, so they share them.
  const part = derivation.block ? prefix : `hd__s_${declaration.name}`;
  const T = out.type(targetType);
  // Derived bounds (annot.bound.params): each parameter in a traversed member.
  const bounded = parameters.filter((parameter) =>
    variants.some((variant) =>
      variant.members.some(
        (member) => !member.omitted && new RegExp(`\\b${parameter}\\b`).test(member.declared),
      ),
    ),
  );
  // A std template's own `Source` may ask more of each member, as derived
  // `Arbitrary` asks `Arbitrary & Inspectable`; a parameter bound gets the
  // same traits (std-testing.arbitrary.derive.params).
  const traits = tuple
    ? tuple.elementBound
    : [derivation.trait, ...memberBound.filter((trait) => trait !== derivation.trait)];
  // A tuple's rest item type takes the bound its rest member needs.
  const boundOf = (parameter: string): readonly string[] =>
    tuple && parameter === TUPLE_REST ? tuple.restBound : traits;
  const targetBounds = parameters.map((p) => withBound(p, bounded.includes(p) ? boundOf(p) : []));
  // Helpers take the target's parameters unbounded, and every use names them.
  const plain = parameters.length > 0 ? `[${parameters.join(", ")}]` : "";
  const typeArgs = parameters.length > 0 ? `::[${parameters.join(", ")}]` : "";

  // Facts and information values.
  const factsCall = (name: string, facts: readonly Expression[]): string => {
    if (facts.length === 0 && name !== `${part}_facts`) return "Facts { items: [] }";
    out.define(name, () => [
      `fn ${name}() -> Facts:`,
      `    Facts { items: [${facts.map((fact) => (isSpreadFact(fact) ? `${out.expression(unscoped(fact))}...` : `${STRUCTURE_FACT}(${out.expression(unscoped(fact))})`)).join(", ")}] }`,
    ]);
    return `${name}()`;
  };
  factsCall(`${part}_facts`, effectiveFacts(target, derivation.lines, "Self").facts);
  const optionalString = (text: string | undefined): string =>
    text === undefined ? ".None" : `.Some(${out.string(text)})`;
  const memberInfo = (variant: VariantModel, member: MemberModel): string => {
    const facts = factsCall(`${part}_facts_${variant.index}_${member.position}`, member.facts);
    return `Member { name: ${out.string(member.name)}, position: ${member.position}, facts: ${facts}, doc: ${optionalString(member.doc)}, embedded: ${member.embedded}, positional: ${member.positional}, self_ref: SelfRef.${member.selfRef} }`;
  };
  const infos = new Map<string, string>();
  for (const variant of variants)
    for (const member of variant.members)
      infos.set(`${variant.index}_${member.position}`, memberInfo(variant, member));

  // Pattern and constructor text for a variant.
  const binding = (member: MemberModel): string => `hd_m${member.position}`;
  // `bind` selects the payload members the pattern binds; the rest are `_`.
  const pattern = (variant: VariantModel, bind: (member: MemberModel) => boolean): string => {
    if (variant.ofData) return "";
    if (variant.members.length === 0) return `${declaration.name}.${variant.name}`;
    const parts = variant.members.map((member) => (bind(member) ? binding(member) : "_"));
    return `${declaration.name}.${variant.name}(${parts.join(", ")})`;
  };

  // Variant handles.
  for (const variant of variants) {
    const variantFacts = factsCall(`${part}_vfacts_${variant.index}`, variant.facts);
    out.define(`${part}_holds_${variant.index}`, () =>
      variant.ofData
        ? [`fn ${part}_holds_${variant.index}${plain}(s: ${T}) -> bool:`, `    true`]
        : [
            `fn ${part}_holds_${variant.index}${plain}(s: ${T}) -> bool:`,
            `    match s:`,
            `        ${pattern(variant, () => false)} => true`,
            ...(variants.length > 1 ? [`        _ => false`] : []),
          ],
    );
    out.define(`${part}_variant_${variant.index}`, () => [
      `fn ${part}_variant_${variant.index}${plain}() -> Variant[${T}]:`,
      `    Variant::[${T}] { info: VariantInfo { name: ${out.string(variant.name)}, index: ${variant.index}, facts: ${variantFacts}, doc: ${optionalString(variant.doc)}, of_data: ${variant.ofData}, shared: [], self_ref: SelfRef.${variant.selfRef} }, hd_holds: ${part}_holds_${variant.index}${typeArgs} }`,
    ]);
  }

  // Member handles: `r` passes the read type, `d` the declared type.
  const handle = (variant: VariantModel, member: MemberModel, view: "r" | "d"): string => {
    const name = `${part}_${view}field_${variant.index}_${member.position}`;
    const memberType = view === "r" ? readonlyType(member.declared) : member.declared;
    const read = (value: string): string =>
      view === "d" && memberType !== readonlyType(memberType)
        ? `${STRUCTURE_AS_DECLARED}(${value})`
        : value;
    // A handle is a literal at its use; only its `get`, and a declared
    // default, are functions.
    out.define(name, () => {
      const F = out.type(memberType);
      const Fo = out.type(optionalType(memberType));
      return [
        `fn ${name}_get${plain}(s: ${T}) -> ${F}:`,
        ...(variant.ofData
          ? [`    ${read(`s.${member.access}`)}`]
          : [
              `    match s:`,
              `        ${pattern(variant, (other) => other === member)} => ${read(binding(member))}`,
              ...(variants.length > 1 ? [`        _ => ${STRUCTURE_MISMATCH}()`] : []),
            ]),
        ...(member.default
          ? [
              `fn ${name}_default${plain}() -> ${Fo}:`,
              `    .Some(${out.expression(member.default)})`,
            ]
          : []),
      ];
    });
    const F = out.type(memberType);
    const fallback = member.default ? `${name}_default${typeArgs}` : `hd__no_default::[${F}]`;
    const plainHandle =
      member.facts.length === 0 &&
      member.doc === undefined &&
      !member.embedded &&
      member.default === undefined;
    const witness = `${STRUCTURE_WITNESS}::[${F}]()`;
    const literal = plainHandle
      ? `hd__plain_field::[${T}, ${F}](${out.string(member.name)}, ${member.position}, ${member.positional}, SelfRef.${member.selfRef}, ${name}_get${typeArgs}, ${witness})`
      : `Field::[${T}, ${F}] { info: ${infos.get(`${variant.index}_${member.position}`)}, hd_get: ${name}_get${typeArgs}, hd_has_default: ${member.default !== undefined}, hd_default: ${fallback}, ${STRUCTURE_WITNESS_FIELD}: ${witness} }`;
    // A plain handle of a non-generic target is a module constant, built
    // once (annot.handle.constants). One with facts or a default is built
    // at its use, since evaluating them may call the derived code.
    if (parameters.length > 0 || !plainHandle) return literal;
    out.define(`${name}_constant`, () => [`let ${name}: Field[${T}, ${F}] = ${literal}`]);
    return name;
  };

  const generateTraversal = (
    traversal: string,
    name: string,
    visitor: string,
    errorType: string,
    { generics, typeArgs: siteArgs }: TraversalGenerics,
  ): void => {
    const V = out.type(visitor);
    const E = out.type(errorType);
    if (traversal === "walk") {
      out.add(`fn ${name}${generics}(value: ${T}, w: ${V}) -> Result[void, ${E}]:`);
      if (target.kind === "data") {
        const variant = variants[0]!;
        out.add(`    w.variant(${part}_variant_0${typeArgs}())?`);
        // A rest member goes to the walker's own `rest`, or, by `rest`'s
        // default, to its `member` (annot.walk.rest, annot.walk.rest.default).
        const rest = (member: MemberModel): boolean =>
          tuple !== undefined && tuple.rest && member === variant.members.at(-1);
        for (const member of variant.members)
          if (!member.omitted)
            out.add(
              `    w.${rest(member) && tuple!.implementsRest(visitor) ? "rest" : "member"}(${handle(variant, member, "r")}, value.${member.access})?`,
              memberSpan(member),
            );
      } else {
        out.add(`    match value:`);
        for (const variant of variants) {
          out.add(`        ${pattern(variant, (member) => !member.omitted)} =>`);
          out.add(`            w.variant(${part}_variant_${variant.index}${typeArgs}())?`);
          for (const member of variant.members)
            out.add(
              `            w.member(${handle(variant, member, "r")}, ${binding(member)})?`,
              memberSpan(member),
            );
        }
      }
      out.add(`    .Ok(())`);
      return;
    }
    if (traversal === "describe") {
      out.add(`fn ${name}${generics}(d: ${V}) -> Result[void, ${E}]:`);
      for (const variant of variants) {
        out.add(`    d.variant(${part}_variant_${variant.index}${typeArgs}())?`);
        for (const member of variant.members)
          if (!member.omitted) out.add(`    d.member(${handle(variant, member, "r")})?`);
      }
      out.add(`    .Ok(())`);
      return;
    }
    // build (annot.build.*); a tuple takes no `mut` (annot.tuple.build).
    const mutT = out.type(tuple ? targetType : `mut:${targetType}`);
    out.add(`fn ${name}${generics}(s: ${V}) -> Result[${mutT}, ${E}]:`);
    out.add(
      `    chosen := s.variant([${variants.map((variant) => `${part}_variant_${variant.index}${typeArgs}()`).join(", ")}])?`,
    );
    for (const variant of variants) {
      out.add(`    if chosen.info.index == ${variant.index}:`);
      out.add(`        return ${name}_v${variant.index}${siteArgs}(s)`);
    }
    out.add(`    ${STRUCTURE_MISMATCH}()`);
    for (const variant of variants) {
      out.add(`fn ${name}_v${variant.index}${generics}(s: ${V}) -> Result[${mutT}, ${E}]:`);
      const offered = variant.members.filter((member) => !member.omitted);
      for (const member of offered)
        out.add(`    let ${binding(member)}: ${out.type(optionalType(member.declared))} = .None`);
      out.add(
        `    members := Members::[${T}] { infos: [${offered.map((member) => infos.get(`${variant.index}_${member.position}`)).join(", ")}], hd_type: ${part}_holds_${variant.index}${typeArgs} }`,
      );
      out.add(`    while true:`);
      out.add(`        key := s.next(members)?`);
      out.add(`        if key.is_end:`);
      out.add(`            break`);
      if (offered.length === 0) out.add(`        ${STRUCTURE_MISMATCH}()`);
      offered.forEach((member, position) => {
        out.add(
          `        ${position === 0 ? "if" : "else if"} key.info.position == ${member.position}:`,
        );
        // Each member is read from the source (annot.build.member). Only
        // `Default`'s build fills a declared default without the bounded
        // `member` (member-bound.declared).
        if (member.default === undefined || !fillsDeclaredDefaults)
          out.add(
            `            ${binding(member)} = .Some(s.member(${handle(variant, member, "d")}, ${binding(member)})?)`,
          );
        else
          out.add(
            `            ${binding(member)} = ${handle(variant, member, "d")}.default().expect("a declared default is missing")`,
          );
      });
      if (offered.length > 0) {
        out.add(`        else:`);
        out.add(`            ${STRUCTURE_MISMATCH}()`);
      }
      for (const member of offered) {
        out.add(
          `    let ${binding(member)}_value: ${out.type(member.declared)} = match ${binding(member)}:`,
        );
        out.add(`        .Some(found) => found`);
        out.add(`        .None => s.missing(${handle(variant, member, "d")})?`);
      }
      const values = offered.map((member) => `${binding(member)}_value`);
      if (tuple) {
        // The rest member's list becomes the rest element's items (annot.tuple.build).
        const items = values.map((value, index) =>
          tuple.rest && index === values.length - 1 ? `${value}...` : value,
        );
        out.add(`    .Ok((${items.join(", ")}${items.length === 1 && !tuple.rest ? "," : ""}))`);
      } else if (variant.ofData) {
        // An embedded part is filled by copy (spec/lang/08-data-and-enums.md#data-embedding).
        const fields = offered.map(
          (member) => `${member.access}: ${member.embedded ? "..." : ""}${binding(member)}_value`,
        );
        out.add(`    .Ok(${declaration.name} { ${fields.join(", ")} })`);
      } else if (variant.members.length === 0)
        out.add(`    .Ok(${STRUCTURE_AS_DECLARED}(${declaration.name}.${variant.name}))`);
      else
        out.add(
          `    .Ok(${STRUCTURE_AS_DECLARED}(${declaration.name}.${variant.name}(${values.join(", ")})))`,
        );
    }
  };

  // The traversal of each template call site, specialized to the target
  // and to the walker, describer, or source type (annot.limit.specialize).
  const targetRenames = new Map([[compiled.parameter, targetType]]);
  compiled.sites.forEach((site, position) =>
    generateTraversal(
      site.traversal,
      `${prefix}_${site.traversal}_${position}`,
      renameWords(site.visitor, targetRenames),
      renameWords(site.errorType, targetRenames),
      traversalGenerics(targetBounds, parameters, site),
    ),
  );

  const { structure, implementation } = instanceImplementations({
    compiled,
    ...(derivation.block ? { block: derivation.block } : {}),
    // A tuple's name is "" (annot.tuple.name).
    declarationName: tuple ? "" : declaration.name,
    parameters,
    targetType,
    bounded: bounded.filter((parameter) => boundOf(parameter).length > 0),
    traits,
    ...(tuple ? { restBound: tuple.restBound } : {}),
    prefix,
    part,
    checked,
    ...(standard ? { standard: true } : {}),
    renames,
    span: derivation.span,
  });
  const generated = out.program(derivation.span, (text) => renameWords(text, renames));
  // A handle constant is visible in every generated function, which a
  // binding is only before its declaration.
  const statements = generated.statements.map((statement) => ({
    ...statement,
    ...(standard && statement.kind === "binding" ? { standard: true as const } : {}),
    span: BEFORE_SOURCE,
  }));
  const markStandard = <T>(items: readonly T[]): Array<T & { readonly standard: true }> =>
    items.map((item) => ({ ...item, standard: true as const }));
  const generatedFunctions = generated.functions.map((declaration) => ({
    ...declaration,
    compilerGenerated: true as const,
  }));
  return {
    implementation,
    structure,
    program: standard
      ? {
          ...generated,
          ...(generated.types ? { types: markStandard(generated.types) } : {}),
          data: markStandard(generated.data),
          enums: markStandard(generated.enums),
          traits: markStandard(generated.traits),
          implementations: markStandard(generated.implementations),
          functions: markStandard(generatedFunctions),
          statements,
        }
      : { ...generated, functions: generatedFunctions, statements },
  };
}

const BEFORE_SOURCE: SourceSpan = {
  start: { line: 1, column: 1, offset: -1 },
  end: { line: 1, column: 1, offset: -1 },
};
