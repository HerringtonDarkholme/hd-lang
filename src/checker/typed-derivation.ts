import type {
  DataDecl,
  DataField,
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
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";
import { NEWTYPE_FIELD } from "./type-declarations.ts";
import { debugWriterName } from "./standard-traits.ts";
import { checkDuplicateDeclarationFacts, isLiteralFact } from "./declaration-facts.ts";

// Typed derivation (spec/14-annotations.md#typed-derivation), lowered before
// checking. The prototype compiles one module, so this pass rewrites every
// derivation into ordinary source: each `@derive(X)` of a trait with a
// template, and each derivation block `impl X for T by Structure:`, becomes
// one ordinary `impl X for T:` whose methods are the template's with `T`
// replaced. `Structure::walk(self, w)`, `T::describe(d)`, `T::build(s)`, and
// `T::facts()` become calls of generated module functions specialized to the
// target and to the walker, describer, or source type, which the template
// must hold in a local declared with its type. The handles, facts, and
// member information are values of the `std.structure` declarations below,
// written in hd with hidden fields for the compiler-supplied bodies.
//
// Prototype gaps: `Facts` holds `Inspectable` values rather than `Any`, so a
// fact must be inspectable; `VariantInfo.shared` is always empty; a build
// handle's `get` returns the member's declared type whatever the argument's
// permission; the comparison traits and `Debug` are accepted in `@derive` but
// only `Eq` is generated (every type already counts as `Debug`); drift and
// unused-fact warnings treat every local trait as one package.

const STRUCTURE_MODULE = "std.structure";
const STRUCTURE = "Structure";
const PROTOCOL_TRAITS = new Set(["Walker", "Describer", "Source"]);
const INTRINSIC_DERIVES = new Set(["Eq", "PartialOrd", "Ord", "Hash", "Debug"]);
const DOWNCAST = "hd__downcast_val";

/** The checker intrinsic that panics with `structure-variant-mismatch`. */
export const STRUCTURE_MISMATCH = "hd__structure_variant_mismatch";
/** The checker intrinsic that gives a build handle its member's declared type. */
export const STRUCTURE_AS_DECLARED = "hd__structure_as_declared";

// The handle declarations name their parameters `HdS` and `HdF`: the
// prototype cannot infer an inherent method's parameters from a receiver
// whose type arguments are the caller's parameters of the same names.
const STRUCTURE_SOURCE = `data Facts:
    pub items: List[Inspectable]

impl Facts:
    pub fn find[F < Inspectable](self) -> F?:
        for item in self.items:
            match ${DOWNCAST}[F](item):
                .Some(found) => return .Some(found)
                .None => pass
        .None

data Member:
    pub name: string
    pub position: i32
    pub facts: Facts
    pub doc: string?
    pub embedded: bool
    pub positional: bool

data VariantInfo:
    pub name: string
    pub index: i32
    pub facts: Facts
    pub doc: string?
    pub of_data: bool
    pub shared: List[(string, Inspectable)]

data Field[HdS, HdF]:
    pub info: Member
    hd_get: fn(HdS) -> HdF
    hd_has_default: bool
    hd_default: fn() -> HdF?

impl[HdS, HdF] Field[HdS, HdF]:
    pub fn get(self, s: HdS) -> HdF:
        (self.hd_get)(s)

    pub fn has_default(self) -> bool:
        self.hd_has_default

    pub fn default(self) -> HdF?:
        (self.hd_default)()

data Variant[HdS]:
    pub info: VariantInfo
    hd_holds: fn(HdS) -> bool

impl[HdS] Variant[HdS]:
    pub fn holds(self, s: HdS) -> bool:
        (self.hd_holds)(s)

data Key[HdS]:
    pub info: Member
    pub is_end: bool
    hd_type: fn(HdS) -> bool

data Members[HdS]:
    pub infos: List[Member]
    hd_type: fn(HdS) -> bool

impl[HdS] Members[HdS]:
    pub fn end(self) -> Key[HdS]:
        Key { info: Member { name: "", position: -1, facts: Facts { items: [] }, doc: .None, embedded: false, positional: false }, is_end: true, hd_type: self.hd_type }

    pub fn at(self, position: i32) -> Key[HdS]:
        for info in self.infos:
            if info.position == position:
                return Key { info: info, is_end: false, hd_type: self.hd_type }
        self.end()

    pub fn find(self, matches: fn(Member) -> bool) -> Key[HdS]:
        for info in self.infos:
            if matches(info):
                return Key { info: info, is_end: false, hd_type: self.hd_type }
        self.end()

trait Walker[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

trait Describer[S]:
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]

trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn next(mut self, members: Members[S]) -> Result[Key[S], Self::Error]
    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, Self::Error]
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
`;

export interface DerivationResult {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
  /** Opt-in spans whose generated member calls report `member-not-derivable`. */
  readonly optInSpans: readonly SourceSpan[];
}

type Target =
  | { readonly kind: "data"; readonly declaration: DataDecl }
  | { readonly kind: "enum"; readonly declaration: EnumDecl };

interface Derivation {
  readonly trait: string;
  readonly target: Target;
  readonly lines: readonly MemberLine[];
  readonly block?: ImplDecl;
  readonly span: SourceSpan;
}

interface MemberModel {
  readonly name: string;
  /** The field or payload name the generated code reads and constructs. */
  readonly access: string;
  readonly declared: string;
  readonly position: number;
  readonly facts: readonly Expression[];
  readonly doc?: string;
  readonly embedded: boolean;
  readonly positional: boolean;
  readonly default?: Expression;
  readonly omitted: boolean;
}

interface VariantModel {
  readonly name: string;
  readonly index: number;
  readonly facts: readonly Expression[];
  readonly doc?: string;
  readonly ofData: boolean;
  readonly members: readonly MemberModel[];
}

// ---------------------------------------------------------------------------
// Generic AST helpers.

const TYPE_KEYS = new Set(["type", "result", "annotation", "alias", "base"]);
const TYPE_LIST_KEYS = new Set(["typeArguments", "ownerTypeArguments", "supertraits"]);

function isTypeRef(value: unknown): value is TypeRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    typeof (value as { name?: unknown }).name === "string" &&
    keys.includes("span") &&
    keys.every((key) => key === "name" || key === "span")
  );
}

function renameWords(text: string, renames: ReadonlyMap<string, string>): string {
  if (renames.size === 0) return text;
  return text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word) => renames.get(word) ?? word);
}

/** Renames generic parameters in every type position of `node`. */
function renameTypes<T>(node: T, renames: ReadonlyMap<string, string>): T {
  if (Array.isArray(node)) return node.map((item) => renameTypes(item, renames)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [entry, value] of Object.entries(node as Record<string, unknown>)) {
    if (TYPE_KEYS.has(entry) && isTypeRef(value))
      result[entry] = { ...value, name: renameWords(value.name, renames) };
    else if (entry === "value" && isTypeRef(value) && "name" in (node as object))
      result[entry] = { ...value, name: renameWords(value.name, renames) };
    else if (TYPE_LIST_KEYS.has(entry) && Array.isArray(value))
      result[entry] = value.map((item) =>
        isTypeRef(item)
          ? { ...item, name: renameWords(item.name, renames) }
          : renameTypes(item, renames),
      );
    else if (entry === "traits" && Array.isArray(value))
      result[entry] = value.map((item) =>
        typeof item === "string" ? renameWords(item, renames) : item,
      );
    else result[entry] = renameTypes(value, renames);
  }
  return result as T;
}

/** Visits every object in `node`, depth first. */
function visit(node: unknown, callback: (value: Record<string, unknown>) => void): void {
  if (Array.isArray(node)) {
    for (const item of node) visit(item, callback);
    return;
  }
  if (!node || typeof node !== "object") return;
  callback(node as Record<string, unknown>);
  for (const value of Object.values(node as Record<string, unknown>)) visit(value, callback);
}

/** Maps every object bottom-up; `callback` may replace it. */
function transform(node: unknown, callback: (value: Record<string, unknown>) => unknown): unknown {
  if (Array.isArray(node)) return node.map((item) => transform(item, callback));
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [entry, value] of Object.entries(node as Record<string, unknown>))
    result[entry] = transform(value, callback);
  return callback(result);
}

function headName(type: string): string {
  return readonlyType(type).split("[")[0]!;
}

// ---------------------------------------------------------------------------
// Generated source with placeholders for user expressions and types.

class Source_ {
  readonly lines: string[] = [];
  /** Helper functions, emitted after `lines` so a helper never splits a body. */
  readonly definitions: string[] = [];
  private readonly defined = new Set<string>();
  private readonly expressions: Expression[] = [];
  private readonly types: string[] = [];

  expression(expression: Expression): string {
    this.expressions.push(expression);
    return `hdexpr${this.expressions.length - 1}`;
  }

  type(type: string): string {
    this.types.push(type);
    return `HDTYPE${this.types.length - 1}X`;
  }

  string(text: string): string {
    return this.expression({ kind: "string", value: text, span: ZERO_SPAN });
  }

  add(text: string): void {
    this.lines.push(text);
  }

  /** Emits a helper function once; returns false when it already exists. */
  define(name: string, lines: () => readonly string[]): void {
    if (this.defined.has(name)) return;
    this.defined.add(name);
    this.definitions.push(...lines());
  }

  /** Parses the collected source and patches placeholders and spans. */
  program(span: SourceSpan): Program {
    const source = `${[...this.lines, ...this.definitions].join("\n")}\n`;
    const parsed = parse(source);
    if (!parsed.program)
      throw new Error(
        `typed derivation generated invalid source: ${parsed.diagnostics.map((item) => `${item.code} ${item.message} at ${item.span.start.line}`).join("; ")}\n${source}`,
      );
    const types = this.types;
    const expressions = this.expressions;
    const patch = (node: unknown, key?: string): unknown => {
      if (Array.isArray(node)) return node.map((item) => patch(item));
      if (typeof node === "string")
        return key === "value"
          ? node
          : node.replace(/HDTYPE(\d+)X/g, (_, index: string) => types[Number(index)]!);
      if (!node || typeof node !== "object") return node;
      const record = node as Record<string, unknown>;
      if (record.kind === "name" && typeof record.name === "string") {
        const match = /^hdexpr(\d+)$/.exec(record.name);
        if (match) return expressions[Number(match[1])];
      }
      const result: Record<string, unknown> = {};
      for (const [entry, value] of Object.entries(record))
        result[entry] = entry === "span" ? span : patch(value, entry);
      return result;
    };
    return patch(parsed.program) as Program;
  }
}

const ZERO_POSITION = { line: 1, column: 1, offset: 0 };
const ZERO_SPAN: SourceSpan = { start: ZERO_POSITION, end: ZERO_POSITION };

// ---------------------------------------------------------------------------

function structureImports(program: Program): Set<string> {
  const names = new Set<string>();
  for (const use of program.uses)
    if (use.module === STRUCTURE_MODULE) for (const name of use.names) names.add(name.name);
  return names;
}

function isGadt(declaration: EnumDecl): boolean {
  return declaration.variants.some(
    (variant) => variant.result !== undefined && variant.result.kind !== "call",
  );
}

/** The concrete type a fact expression evaluates to, when it is evident from syntax. */
function factType(expression: Expression, functions: ReadonlyMap<string, FunctionDecl>): string {
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

/** The declared type of local `name` in a method body. */
function localType(statements: readonly Statement[], name: string): string | undefined {
  let type: string | undefined;
  visit(statements, (value) => {
    if (
      value.kind === "binding" &&
      value.name === name &&
      isTypeRef(value.annotation) &&
      type === undefined
    )
      type = value.annotation.name;
  });
  return type;
}

/** The `Error` type an implementation of `protocol` for `type` declares. */
function protocolError(
  implementations: readonly ImplDecl[],
  protocol: string,
  type: string,
): string | undefined {
  const actual = readonlyType(type);
  const actualParts = nominalGenericParts(actual);
  for (const implementation of implementations) {
    if (!implementation.traitName || headName(implementation.traitName) !== protocol) continue;
    if (headName(implementation.targetName) !== headName(actual)) continue;
    const error = implementation.associatedTypes.find((item) => item.name === "Error")?.value;
    if (!error) continue;
    const renames = new Map<string, string>();
    const declared = nominalGenericParts(implementation.targetName);
    declared?.arguments.forEach((argument, index) => {
      const value = actualParts?.arguments[index];
      if (implementation.genericParameters.includes(argument) && value !== undefined)
        renames.set(argument, value);
    });
    return renameWords(error.name, renames);
  }
  return undefined;
}

// ---------------------------------------------------------------------------

export function withTypedDerivation(program: Program): DerivationResult {
  const diagnostics: Diagnostic[] = [];
  const optInSpans: SourceSpan[] = [];
  const imported = structureImports(program);
  const structureVisible = imported.has(STRUCTURE);
  const functions = new Map(program.functions.map((item) => [item.name, item] as const));
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
  const error = (code: string, message: string, span: SourceSpan): void => {
    diagnostics.push({ code, message, span });
  };

  // Decorators before a function wait for function targets (annot.decorator.function).
  for (const declaration of program.functions)
    if (declaration.decorators)
      error(
        "decorator-not-annotator",
        "a decorator before a function is rejected until function targets are decided",
        declaration.decorators.span,
      );

  checkDuplicateDeclarationFacts(program, (fact) => factType(fact, functions), error);

  // Templates and derivation blocks.
  const templates = new Map<string, ImplDecl>();
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
      if (templates.has(trait)) {
        error(
          "overlapping-impl",
          `trait '${trait}' already has a derivation template`,
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
      templates.set(trait, implementation);
    } else blocks.push(implementation);
  }

  // `Structure` named outside a template (annot.structure.named-positions).
  if (structureVisible) checkStructureMentions(program, new Set(templates.values()), error);

  // `member` through a generic walker, describer, or source (annot.walker.generic-member-call).
  if (imported.size > 0) checkGenericMemberCalls(program, error);

  // Opt-ins.
  const derivations: Derivation[] = [];
  const derivedPairs = new Map<string, SourceSpan>();
  const intrinsic: { trait: string; target: Target; span: SourceSpan }[] = [];
  const newtypeDerivations: { trait: string; declaration: TypeDecl; span: SourceSpan }[] = [];
  const optIn = (target: Target | undefined, declaration: DataDecl | EnumDecl | TypeDecl): void => {
    for (const trait of declaration.decorators?.derives ?? []) {
      const name = trait.name;
      if (INTRINSIC_DERIVES.has(name)) {
        if (target) intrinsic.push({ trait: name, target, span: trait.span });
        continue;
      }
      if (!templates.has(name)) {
        error(
          "underivable-trait",
          name === "Error"
            ? "Error has no template and is not intrinsic; an error type uses @error"
            : `trait '${name}' has neither a template nor an intrinsic derivation`,
          trait.span,
        );
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
    if (!templates.has(trait)) {
      error("underivable-trait", `trait '${trait}' has no derivation template`, block.span);
      continue;
    }
    const target = targets.get(targetName);
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
    if (!checkMemberLines(target, lines, functions, error)) continue;
    derivations.push({ trait, target, lines, block, span: block.span });
  }

  // Warnings: line drift and unused type-level facts.
  lintDerivations(program, derivations, diagnostics);

  if (diagnostics.some((item) => item.severity !== "warning"))
    return { program, diagnostics, optInSpans };

  // Generation.
  const generated: Program[] = [];
  const implementations: ImplDecl[] = [...kept];

  derivations.forEach((derivation, index) => {
    optInSpans.push(derivation.span);
    const result = generateDerivation(
      derivation,
      index,
      templates.get(derivation.trait)!,
      program.implementations,
      error,
    );
    if (!result) return;
    implementations.push(result.implementation);
    generated.push(result.program);
  });
  for (const item of newtypeDerivations) {
    const result = forwardNewtype(item, templates.get(item.trait)!, error);
    if (result) implementations.push(result);
  }
  for (const item of intrinsic) {
    if (item.trait === "Eq") implementations.push(deriveEq(item.target, item.span));
    if (item.trait === "Debug")
      implementations.push(deriveDebug(item.target, debugWriterName(program.uses), item.span));
  }
  if (diagnostics.some((item) => item.severity !== "warning"))
    return { program, diagnostics, optInSpans };

  const factFunctions = factCheckFunctions(program);

  const needsStructure = imported.size > 0 || derivations.length > 0;
  const structure = needsStructure ? parse(STRUCTURE_SOURCE).program : undefined;
  if (needsStructure && !structure) throw new Error("std.structure source does not parse");
  const structureUse: UseDecl = {
    kind: "use",
    module: "std.inspect",
    names: [{ name: "Inspectable" }, { name: "downcast_val", alias: DOWNCAST }],
    span: program.span,
  };
  const structureTraits = (structure?.traits ?? []).map((trait): TraitDecl => ({
    ...trait,
    strengthenableMembers: ["member"],
  }));
  const alreadyImportsInspectable = program.uses.some(
    (use) => use.module === "std.inspect" && use.names.some((name) => name.name === "Inspectable"),
  );
  return {
    program: {
      ...program,
      uses: [
        ...program.uses.filter((use) => use.module !== STRUCTURE_MODULE),
        ...(needsStructure
          ? [
              alreadyImportsInspectable
                ? { ...structureUse, names: [structureUse.names[1]!] }
                : structureUse,
            ]
          : []),
      ],
      data: [
        ...program.data,
        ...(structure?.data ?? []),
        ...generated.flatMap((item) => item.data),
      ],
      enums: program.enums,
      traits: [...program.traits, ...structureTraits],
      implementations: [
        ...implementations,
        ...(structure?.implementations ?? []),
        ...generated.flatMap((item) => item.implementations),
      ],
      functions: [
        ...program.functions,
        ...factFunctions,
        ...generated.flatMap((item) => item.functions),
      ],
    },
    diagnostics,
    optInSpans,
  };
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
        callee.name === "member" &&
        callee.receiver.kind === "name" &&
        genericLocals.has(callee.receiver.name)
      )
        error(
          "generic-member-call",
          "member may be called through a generic walker, describer, or source only by generated code",
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
  // Only a fact whose type comes from a package other than `std` warns
  // (annot.fact.unused-non-std).
  for (const declaration of [...program.data, ...program.enums]) {
    const facts = declaration.decorators?.facts ?? [];
    if (facts.length === 0 || byTarget.has(declaration.name)) continue;
    for (const fact of facts)
      if (!isLiteralFact(fact))
        warn(
          "unused-derivation-fact",
          `type '${declaration.name}' derives no template that could read this fact`,
          fact.span,
        );
  }
}

/**
 * One requirement-free function per fact or metadata expression, so each is
 * checked as a compile-time expression (annot.fact.eval, annot.metadata.eval).
 */
function factCheckFunctions(program: Program): FunctionDecl[] {
  const factChecks = new Source_();
  let factCount = 0;
  const checkFacts = (expressions: readonly Expression[] | undefined): void => {
    for (const expression of expressions ?? []) {
      factChecks.add(`fn hd__fact_check_${factCount}() -> void:`);
      factChecks.add(`    _ := ${factChecks.expression(expression)}`);
      factCount += 1;
    }
  };
  for (const declaration of program.data) {
    checkFacts(declaration.decorators?.facts);
    for (const field of declaration.fields) checkFacts(field.metadata);
  }
  for (const declaration of program.enums) {
    checkFacts(declaration.decorators?.facts);
    for (const variant of declaration.variants) {
      checkFacts(variant.metadata);
      for (const field of variant.fields) checkFacts(field.metadata);
    }
  }
  for (const declaration of program.functions)
    for (const parameter of declaration.parameters) checkFacts(parameter.metadata);
  const factProgram = factCount > 0 ? factChecks.program(ZERO_SPAN) : undefined;
  return (factProgram?.functions ?? []).map((declaration): FunctionDecl => ({
    ...declaration,
    span: (declaration.body[0] as { value?: Expression }).value?.span ?? declaration.span,
    defaultContext: { laterNames: [] },
  }));
}

// ---------------------------------------------------------------------------
// Member lines (annot.line.*).

function directMembers(target: Target): string[] {
  return target.kind === "data"
    ? target.declaration.fields.map((field) => field.name)
    : target.declaration.variants.map((variant) => variant.name);
}

function declarationFacts(target: Target, name: string): readonly Expression[] {
  if (name === "Self") return target.declaration.decorators?.facts ?? [];
  if (target.kind === "data")
    return target.declaration.fields.find((field) => field.name === name)?.metadata ?? [];
  return target.declaration.variants.find((variant) => variant.name === name)?.metadata ?? [];
}

function checkMemberLines(
  target: Target,
  lines: readonly MemberLine[],
  functions: ReadonlyMap<string, FunctionDecl>,
  error: (code: string, message: string, span: SourceSpan) => void,
): boolean {
  let valid = true;
  const fail = (code: string, message: string, span: SourceSpan): void => {
    error(code, message, span);
    valid = false;
  };
  const members = new Set(directMembers(target));
  const current = new Map<string, string[]>();
  for (const line of lines) {
    if (line.name !== "Self" && !members.has(line.name)) {
      fail(
        "unknown-annotation-member",
        target.kind === "enum"
          ? `'${line.name}' is not a variant of '${target.declaration.name}'; a member line names a whole variant`
          : `'${line.name}' is not a member of '${target.declaration.name}'`,
        line.nameSpan,
      );
      continue;
    }
    if (line.pass) {
      if (line.operator === "+=" || line.name === "Self" || target.kind === "enum") {
        fail(
          "invalid-member-line",
          line.name === "Self"
            ? "Self takes a fact list, not pass"
            : target.kind === "enum"
              ? "a whole variant cannot be omitted"
              : "pass follows only '='",
          line.span,
        );
        continue;
      }
      if (target.kind === "data") {
        const field = target.declaration.fields.find((item) => item.name === line.name);
        if (field && !field.default) {
          fail(
            "omitted-member-without-default",
            `member '${line.name}' has no default, so it cannot be omitted`,
            line.span,
          );
          continue;
        }
      }
      continue;
    }
    if (!line.value || line.value.kind !== "list") {
      fail("invalid-member-line", "a member line's right side must be a list or pass", line.span);
      continue;
    }
    const before =
      current.get(line.name) ??
      declarationFacts(target, line.name).map((fact) => factType(fact, functions));
    const added = line.value.elements.map((fact) => factType(fact, functions));
    const next = line.operator === "+=" ? [...before, ...added] : added;
    if (new Set(next).size !== next.length) {
      fail(
        "duplicate-fact",
        `member '${line.name}' would hold two facts of the same concrete type; use '=' to change it`,
        line.span,
      );
      continue;
    }
    current.set(line.name, next);
  }
  return valid;
}

/** The facts one derivation sees for a member, variant, or `Self`. */
function effectiveFacts(
  target: Target,
  lines: readonly MemberLine[],
  name: string,
): { facts: readonly Expression[]; omitted: boolean } {
  let facts = [...declarationFacts(target, name)];
  let omitted = false;
  for (const line of lines) {
    if (line.name !== name) continue;
    if (line.pass) omitted = true;
    else if (line.value?.kind === "list")
      facts =
        line.operator === "+=" ? [...facts, ...line.value.elements] : [...line.value.elements];
  }
  return { facts, omitted };
}

function variantModels(target: Target, lines: readonly MemberLine[]): VariantModel[] {
  const member = (
    field: DataField,
    position: number,
    facts: readonly Expression[],
    omitted: boolean,
  ): MemberModel => ({
    name: field.positional ? `_${field.name}` : field.name,
    access: field.name,
    declared: field.type.name,
    position,
    facts,
    ...(field.doc ? { doc: field.doc } : {}),
    embedded: field.embedded === true,
    positional: field.positional === true,
    ...(field.default ? { default: field.default } : {}),
    omitted,
  });
  if (target.kind === "data") {
    const declaration = target.declaration;
    return [
      {
        name: declaration.name,
        index: 0,
        facts: [],
        ...(declaration.doc ? { doc: declaration.doc } : {}),
        ofData: true,
        members: declaration.fields.map((field, position) => {
          const { facts, omitted } = effectiveFacts(target, lines, field.name);
          return member(field, position, facts, omitted);
        }),
      },
    ];
  }
  return target.declaration.variants.map((variant, index) => ({
    name: variant.name,
    index,
    facts: effectiveFacts(target, lines, variant.name).facts,
    ...(variant.doc ? { doc: variant.doc } : {}),
    ofData: false,
    members: variant.fields.map((field, position) =>
      member(field, position, field.metadata ?? [], false),
    ),
  }));
}

// ---------------------------------------------------------------------------
// Generation of one derivation.

interface Generated {
  readonly implementation: ImplDecl;
  readonly program: Program;
}

function generateDerivation(
  derivation: Derivation,
  index: number,
  template: ImplDecl,
  implementations: readonly ImplDecl[],
  error: (code: string, message: string, span: SourceSpan) => void,
): Generated | undefined {
  const target = derivation.target;
  const declaration = target.declaration;
  const parameters = declaration.genericParameters;
  const targetType =
    parameters.length > 0 ? `${declaration.name}[${parameters.join(",")}]` : declaration.name;
  const variants = variantModels(target, derivation.lines);
  const out = new Source_();
  const prefix = `hd__d${index}`;
  const T = out.type(targetType);
  // Derived bounds (annot.bound.params): each parameter in a traversed member.
  const bounded = parameters.filter((parameter) =>
    variants.some((variant) =>
      variant.members.some(
        (member) => !member.omitted && new RegExp(`\\b${parameter}\\b`).test(member.declared),
      ),
    ),
  );
  const generics =
    parameters.length > 0
      ? `[${parameters.map((parameter) => (bounded.includes(parameter) ? `${parameter} < ${derivation.trait}` : parameter)).join(", ")}]`
      : "";
  // Helpers take the target's parameters unbounded, and every use names them.
  const plain = parameters.length > 0 ? `[${parameters.join(", ")}]` : "";

  // Facts and information values.
  const factsCall = (name: string, facts: readonly Expression[]): string => {
    out.define(name, () => [
      `fn ${name}() -> Facts:`,
      `    Facts { items: [${facts.map((fact) => out.expression(fact)).join(", ")}] }`,
    ]);
    return `${name}()`;
  };
  factsCall(`${prefix}_facts`, effectiveFacts(target, derivation.lines, "Self").facts);
  const optionalString = (text: string | undefined): string =>
    text === undefined ? ".None" : `.Some(${out.string(text)})`;
  const memberInfo = (variant: VariantModel, member: MemberModel): string => {
    const facts = factsCall(`${prefix}_facts_${variant.index}_${member.position}`, member.facts);
    return `Member { name: ${out.string(member.name)}, position: ${member.position}, facts: ${facts}, doc: ${optionalString(member.doc)}, embedded: ${member.embedded}, positional: ${member.positional} }`;
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
    const variantFacts = factsCall(`${prefix}_vfacts_${variant.index}`, variant.facts);
    out.define(`${prefix}_holds_${variant.index}`, () =>
      variant.ofData
        ? [`fn ${prefix}_holds_${variant.index}${plain}(s: ${T}) -> bool:`, `    true`]
        : [
            `fn ${prefix}_holds_${variant.index}${plain}(s: ${T}) -> bool:`,
            `    match s:`,
            `        ${pattern(variant, () => false)} => true`,
            ...(variants.length > 1 ? [`        _ => false`] : []),
          ],
    );
    out.define(`${prefix}_variant_${variant.index}`, () => [
      `fn ${prefix}_variant_${variant.index}${plain}() -> Variant[${T}]:`,
      `    Variant[${T}] { info: VariantInfo { name: ${out.string(variant.name)}, index: ${variant.index}, facts: ${variantFacts}, doc: ${optionalString(variant.doc)}, of_data: ${variant.ofData}, shared: [] }, hd_holds: ${prefix}_holds_${variant.index}${plain} }`,
    ]);
  }

  // Member handles: `r` passes the read type, `d` the declared type.
  const handle = (variant: VariantModel, member: MemberModel, view: "r" | "d"): string => {
    const name = `${prefix}_${view}field_${variant.index}_${member.position}`;
    const memberType = view === "r" ? readonlyType(member.declared) : member.declared;
    const read = (value: string): string =>
      view === "d" && memberType !== readonlyType(memberType)
        ? `${STRUCTURE_AS_DECLARED}(${value})`
        : value;
    out.define(name, () => {
      const F = out.type(memberType);
      const Fo = out.type(`${memberType}?`);
      return [
        `fn ${name}_get${plain}(s: ${T}) -> ${F}:`,
        ...(variant.ofData
          ? [`    ${read(`s.${member.access}`)}`]
          : [
              `    match s:`,
              `        ${pattern(variant, (other) => other === member)} => ${read(binding(member))}`,
              ...(variants.length > 1 ? [`        _ => ${STRUCTURE_MISMATCH}()`] : []),
            ]),
        `fn ${name}_default${plain}() -> ${Fo}:`,
        `    ${member.default ? `.Some(${out.expression(member.default)})` : ".None"}`,
        `fn ${name}${plain}() -> Field[${T}, ${F}]:`,
        `    Field[${T}, ${F}] { info: ${infos.get(`${variant.index}_${member.position}`)}, hd_get: ${name}_get${plain}, hd_has_default: ${member.default !== undefined}, hd_default: ${name}_default${plain} }`,
      ];
    });
    return `${name}${plain}()`;
  };

  // Traversal functions, one per call site.
  let callCount = 0;
  const rewriteCalls = (method: MethodDecl): MethodDecl => {
    const templateParameter = template.genericParameters[0]!;
    const renames = new Map([[templateParameter, targetType]]);
    const body = transform(method.body ?? [], (value) => {
      if (value.kind === "call") {
        const callee = value.callee as Expression;
        if (callee.kind !== "qualified-name") return value;
        const args = value.arguments as Expression[];
        const traversal =
          callee.owner === STRUCTURE && callee.name === "walk"
            ? "walk"
            : callee.owner === templateParameter &&
                (callee.name === "describe" || callee.name === "build")
              ? callee.name
              : callee.owner === templateParameter && callee.name === "facts"
                ? "facts"
                : undefined;
        if (!traversal) return value;
        if (traversal === "facts")
          return {
            ...value,
            callee: { kind: "name", name: `${prefix}_facts`, span: callee.span },
            arguments: [],
          };
        const argument = traversal === "walk" ? args[1] : args[0];
        const local =
          argument?.kind === "name" ? localType(method.body ?? [], argument.name) : undefined;
        if (!local) {
          error(
            "unsupported-derivation",
            `the prototype needs the ${traversal === "walk" ? "walker" : traversal === "describe" ? "describer" : "source"} of ${traversal} in a local declared with its type`,
            value.span as SourceSpan,
          );
          return value;
        }
        const protocol =
          traversal === "walk" ? "Walker" : traversal === "describe" ? "Describer" : "Source";
        const localTypeText = renameWords(local, renames);
        const errorType = protocolError(implementations, protocol, localTypeText) ?? "never";
        const name = `${prefix}_${traversal}_${callCount}`;
        callCount += 1;
        generateTraversal(traversal, name, localTypeText, errorType);
        return {
          ...value,
          callee: { kind: "name", name, span: callee.span },
          arguments: traversal === "walk" ? args : [args[0]],
        };
      }
      return value;
    }) as Statement[];
    return renameTypes({ ...method, body }, renames);
  };

  const generateTraversal = (
    traversal: string,
    name: string,
    visitor: string,
    errorType: string,
  ): void => {
    const V = out.type(visitor);
    const E = out.type(errorType);
    if (traversal === "walk") {
      out.add(`fn ${name}${generics}(value: ${T}, w: ${V}) -> Result[void, ${E}]:`);
      if (target.kind === "data") {
        const variant = variants[0]!;
        out.add(`    w.variant(${prefix}_variant_0${plain}())?`);
        for (const member of variant.members)
          if (!member.omitted)
            out.add(`    w.member(${handle(variant, member, "r")}, value.${member.access})?`);
      } else {
        out.add(`    match value:`);
        for (const variant of variants) {
          out.add(`        ${pattern(variant, (member) => !member.omitted)} =>`);
          out.add(`            w.variant(${prefix}_variant_${variant.index}${plain}())?`);
          for (const member of variant.members)
            out.add(`            w.member(${handle(variant, member, "r")}, ${binding(member)})?`);
        }
      }
      out.add(`    .Ok()`);
      return;
    }
    if (traversal === "describe") {
      out.add(`fn ${name}${generics}(d: ${V}) -> Result[void, ${E}]:`);
      for (const variant of variants) {
        out.add(`    d.variant(${prefix}_variant_${variant.index}${plain}())?`);
        for (const member of variant.members)
          if (!member.omitted) out.add(`    d.member(${handle(variant, member, "r")})?`);
      }
      out.add(`    .Ok()`);
      return;
    }
    // build (annot.build.*)
    const mutT = out.type(`mut:${targetType}`);
    out.add(`fn ${name}${generics}(s: ${V}) -> Result[${mutT}, ${E}]:`);
    out.add(
      `    chosen := s.variant([${variants.map((variant) => `${prefix}_variant_${variant.index}${plain}()`).join(", ")}])?`,
    );
    for (const variant of variants) {
      out.add(`    if chosen.info.index == ${variant.index}:`);
      out.add(`        return ${name}_v${variant.index}${plain}(s)`);
    }
    out.add(`    ${STRUCTURE_MISMATCH}()`);
    for (const variant of variants) {
      out.add(`fn ${name}_v${variant.index}${generics}(s: ${V}) -> Result[${mutT}, ${E}]:`);
      const offered = variant.members.filter((member) => !member.omitted);
      for (const member of offered)
        out.add(`    let ${binding(member)}: ${out.type(`${member.declared}?`)} = .None`);
      out.add(
        `    members := Members[${T}] { infos: [${offered.map((member) => infos.get(`${variant.index}_${member.position}`)).join(", ")}], hd_type: ${prefix}_holds_${variant.index}${plain} }`,
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
        out.add(
          `            ${binding(member)} = .Some(s.member(${handle(variant, member, "d")}, ${binding(member)})?)`,
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
      if (variant.ofData) {
        // An embedded part is filled by copy (spec/08-data-and-enums.md#data-embedding).
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

  // The implementation: the template's methods, with the block's overrides.
  const overridden = new Set((derivation.block?.methods ?? []).map((method) => method.name));
  const templateRenames = new Map([[template.genericParameters[0]!, targetType]]);
  const methods = [
    ...template.methods.filter((method) => !overridden.has(method.name)).map(rewriteCalls),
    ...(derivation.block?.methods ?? []),
  ];
  const implementation: ImplDecl = {
    kind: "impl",
    genericParameters: derivation.block?.genericParameters.length
      ? derivation.block.genericParameters
      : parameters,
    genericBounds: derivation.block?.genericParameters.length
      ? derivation.block.genericBounds
      : bounded.map((parameter) => ({
          parameter,
          traits: [derivation.trait],
          span: derivation.span,
        })),
    traitName: renameWords(template.traitName!, templateRenames),
    targetName: targetType,
    associatedTypes: renameTypes(template.associatedTypes, templateRenames),
    methods,
    span: derivation.span,
  };
  return { implementation, program: out.program(derivation.span) };
}

// ---------------------------------------------------------------------------
// Newtypes derive through their base (annot.derive.means, trait.derive.newtype).

function forwardingAllowed(type: string): boolean {
  if (!/\bSelf\b/.test(type)) return true;
  const plain = readonlyType(type);
  return (
    plain === "Self" ||
    plain === "Self?" ||
    plain === "List[Self]" ||
    plain.startsWith("Result[Self,")
  );
}

function forwardNewtype(
  item: { trait: string; declaration: TypeDecl; span: SourceSpan },
  template: ImplDecl,
  error: (code: string, message: string, span: SourceSpan) => void,
): ImplDecl | undefined {
  const parameter = template.genericParameters[0]!;
  const selfRenames = new Map([[parameter, "Self"]]);
  const methods: MethodDecl[] = [];
  for (const method of template.methods) {
    const positions = [
      ...method.parameters.filter((value) => value.name !== "self").map((value) => value.type.name),
      method.result.name,
    ].map((type) => renameWords(type, selfRenames));
    const bad = positions.find((type) => !forwardingAllowed(type));
    if (bad !== undefined) {
      error(
        "newtype-derivation-self",
        `method '${method.name}' has Self in '${bad.replace(/mut:/g, "mut ")}'; a newtype forwards only the receiver, Self, Self?, Result[Self, E], and List[Self]`,
        item.span,
      );
      return undefined;
    }
    const unsupported = positions.find(
      (type) => /\bSelf\b/.test(type) && readonlyType(type) !== "Self",
    );
    if (unsupported !== undefined) {
      error(
        "unsupported-derivation",
        `the prototype forwards a newtype method only through the receiver and plain Self, not '${unsupported}'`,
        item.span,
      );
      return undefined;
    }
    const span = item.span;
    const base = item.declaration.base!.name;
    const unwrap = (expression: Expression): Expression => ({
      kind: "member",
      receiver: expression,
      name: NEWTYPE_FIELD,
      span,
    });
    const receiver = method.parameters[0]?.name === "self";
    const args = method.parameters
      .filter((value) => value.name !== "self")
      .map((value): Expression => {
        const name: Expression = { kind: "name", name: value.name, span };
        return readonlyType(renameWords(value.type.name, selfRenames)) === "Self"
          ? unwrap(name)
          : name;
      });
    const call: Expression = receiver
      ? {
          kind: "call",
          callee: {
            kind: "member",
            receiver: unwrap({ kind: "name", name: "self", span }),
            name: method.name,
            span,
          },
          arguments: args,
          span,
        }
      : {
          kind: "call",
          callee: { kind: "qualified-name", owner: base, name: method.name, span },
          arguments: args,
          span,
        };
    const wrapped: Expression =
      readonlyType(renameWords(method.result.name, selfRenames)) === "Self"
        ? {
            kind: "call",
            callee: { kind: "name", name: item.declaration.name, span },
            arguments: [call],
            span,
          }
        : call;
    methods.push({
      ...renameTypes(method, new Map([[parameter, item.declaration.name]])),
      body: [{ kind: "expression", expression: wrapped, span }],
      span,
    });
  }
  return {
    kind: "impl",
    genericParameters: item.declaration.genericParameters,
    genericBounds: [],
    traitName: item.trait,
    targetName: item.declaration.name,
    associatedTypes: [],
    methods,
    span: item.span,
  };
}

// ---------------------------------------------------------------------------
// The intrinsic `@derive(Eq)` (spec/09-traits.md#comparison-traits) and
// `@derive(Debug)` (#debug-trait). The spec leaves `DebugWriter`'s builder
// calls to the standard library, so a derived `debug` writes nothing.

/** Starts `impl[T < Trait] Trait for Target:` and returns the target's placeholder. */
function derivedImpl(target: Target, trait: string, out: Source_): string {
  const { name, genericParameters: parameters } = target.declaration;
  const T = out.type(parameters.length > 0 ? `${name}[${parameters.join(",")}]` : name);
  const bounds = parameters.map((parameter) => `${parameter} < ${trait}`).join(", ");
  out.add(`impl${parameters.length > 0 ? `[${bounds}]` : ""} ${trait} for ${T}:`);
  return T;
}

function deriveDebug(target: Target, writer: string, span: SourceSpan): ImplDecl {
  const out = new Source_();
  derivedImpl(target, "Debug", out);
  out.add(`    fn debug(self, out: mut ${writer}) -> void: pass`);
  return out.program(span).implementations[0]!;
}

function deriveEq(target: Target, span: SourceSpan): ImplDecl {
  const declaration = target.declaration;
  const out = new Source_();
  const T = derivedImpl(target, "Eq", out);
  out.add(`    fn eq(self, other: ${T}) -> bool:`);
  if (target.kind === "data") {
    const fields = target.declaration.fields;
    out.add(
      `        ${fields.length === 0 ? "true" : fields.map((field) => `self.${field.name} == other.${field.name}`).join(" && ")}`,
    );
  } else {
    const variants = target.declaration.variants;
    out.add(`        match (self, other):`);
    for (const variant of variants) {
      const names = (side: string): string[] =>
        variant.fields.map((field, position) => `${side}${position}`);
      const pattern = (side: string): string =>
        variant.fields.length === 0
          ? `${declaration.name}.${variant.name}`
          : `${declaration.name}.${variant.name}(${variant.fields.map((field, position) => names(side)[position]).join(", ")})`;
      const compare =
        variant.fields.length === 0
          ? "true"
          : names("l")
              .map((left, position) => `${left} == ${names("r")[position]}`)
              .join(" && ");
      out.add(`            (${pattern("l")}, ${pattern("r")}) => ${compare}`);
    }
    if (variants.length > 1) out.add(`            _ => false`);
  }
  return out.program(span).implementations[0]!;
}
