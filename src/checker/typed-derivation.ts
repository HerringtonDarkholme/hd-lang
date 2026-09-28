import type {
  DataDecl,
  DataField,
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
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import { Source_, ZERO_SPAN } from "./generated-source.ts";
import { deriveIntrinsics, intrinsicHelpers } from "./derive-intrinsics.ts";
import { nominalGenericParts, readonlyType } from "../types.ts";
import { NEWTYPE_FIELD } from "./type-declarations.ts";
import { debugWriterName } from "./standard-traits.ts";
import { checkDuplicateDeclarationFacts, isLiteralFact } from "./declaration-facts.ts";
import {
  checkMemberLines,
  declarationFacts,
  isSpreadFact,
  isTraitLess,
  withInlinedListLines,
  lineFacts,
  type Target,
  withTraitLessBlocks,
} from "./member-lines.ts";

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
// permission; the intrinsic derivations are in derive-intrinsics.ts; drift and
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

export function withTypedDerivation(source: Program): DerivationResult {
  const diagnostics: Diagnostic[] = [];
  const optInSpans: SourceSpan[] = [];
  const imported = structureImports(source);
  const structureVisible = imported.has(STRUCTURE);
  const functions = new Map(source.functions.map((item) => [item.name, item] as const));
  const error = (code: string, message: string, span: SourceSpan): void => {
    diagnostics.push({ code, message, span });
  };

  checkUnderivableTargets(source, error);

  checkDuplicateDeclarationFacts(source, (fact) => factType(fact, functions), error);
  // Trait-less blocks edit the declaration facts (annot.traitless.declaration-facts).
  const fact = (expression: Expression): string => factType(expression, functions);
  // The known type of a member line's right side (annot.line.right-typed).
  const moduleBindings = new Map(
    source.statements.flatMap((statement) =>
      statement.kind === "binding" && statement.annotation
        ? [[statement.name, statement.annotation.name] as const]
        : [],
    ),
  );
  const knownType = (value: Expression): string | undefined => {
    if (value.kind === "name") return moduleBindings.get(value.name);
    if (value.kind === "call" && value.callee.kind === "name") {
      const declaration = functions.get(value.callee.name);
      return declaration && !declaration.resultOmitted ? declaration.result.name : undefined;
    }
    return undefined;
  };
  const program = withTraitLessBlocks(
    withInlinedListLines(source),
    structureVisible,
    fact,
    knownType,
    error,
  );
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
  const newtypeIntrinsic: { trait: string; declaration: TypeDecl; span: SourceSpan }[] = [];
  const optIn = (target: Target | undefined, declaration: DataDecl | EnumDecl | TypeDecl): void => {
    for (const trait of declaration.decorators?.derives ?? []) {
      const name = trait.name;
      if (INTRINSIC_DERIVES.has(name)) {
        const span = trait.span;
        if (target) intrinsic.push({ trait: name, target, span });
        else if (name !== "Debug" && declaration.kind === "type")
          newtypeIntrinsic.push({ trait: name, declaration, span });
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
    if (!checkMemberLines(target, lines, fact, knownType, error)) continue;
    derivations.push({ trait, target, lines, block, span: block.span });
  }

  // Warnings: line drift and unused type-level facts.
  // Only a decorator's fact can be unused (annot.fact.unused-non-std).
  lintDerivations(source, derivations, diagnostics);

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
  const writer = debugWriterName(program.uses);
  implementations.push(...deriveIntrinsics(program, intrinsic, newtypeIntrinsic, writer, error));
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
        ...intrinsicHelpers(intrinsic, newtypeIntrinsic),
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
  // (annot.fact.unused-non-std). A fact built by a name imported from `std`,
  // such as `@annotate(.Field)`, has a standard type (annot.fact.unused-std).
  const standardNames = new Set(
    program.uses
      .filter((use) => use.module.startsWith("std."))
      .flatMap((use) => use.names.map((name) => name.alias ?? name.name)),
  );
  const standardFact = (fact: Expression): boolean =>
    (fact.kind === "call" && fact.callee.kind === "name" && standardNames.has(fact.callee.name)) ||
    (fact.kind === "data" && standardNames.has(fact.name));
  for (const declaration of [...program.data, ...program.enums]) {
    const facts = declaration.decorators?.facts ?? [];
    if (facts.length === 0 || byTarget.has(declaration.name)) continue;
    for (const fact of facts)
      if (!isLiteralFact(fact) && !standardFact(fact))
        warn(
          "unused-derivation-fact",
          `type '${declaration.name}' derives no template that could read this fact`,
          fact.span,
        );
  }
  // A trait-less block's `Self` line warns the same way, on the line
  // (annot.fact.unused-self-line).
  for (const block of program.implementations.filter(isTraitLess)) {
    const name = headName(block.targetName);
    if (byTarget.has(name)) continue;
    for (const line of block.memberLines ?? [])
      if (line.name === "Self" && line.value && lineFacts(line.value).some(nonLiteral))
        warn(
          "unused-derivation-fact",
          `type '${name}' derives no template that could read this fact`,
          line.span,
        );
  }
  // A per-trait block's `Self` line warns only when the fact's package does
  // not supply the block's trait (annot.fact.unused-self-line.per-trait).
  // The prototype compiles one package, whose templates are local, so such a
  // fact never occurs here.
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

function nonLiteral(fact: Expression): boolean {
  return !isLiteralFact(fact);
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
  for (const declaration of program.functions) {
    checkFacts(declaration.decorators?.facts);
    for (const parameter of declaration.parameters) checkFacts(parameter.metadata);
  }
  // Values before a trait, implementation, newtype, or method
  // (annot.decorator.attach), and a method's parameter metadata.
  const methods = (list: readonly MethodDecl[]): void => {
    for (const method of list) {
      checkFacts(method.decorators?.facts);
      for (const parameter of method.parameters) checkFacts(parameter.metadata);
    }
  };
  for (const declaration of program.traits) {
    checkFacts(declaration.decorators?.facts);
    methods(declaration.methods);
  }
  for (const declaration of program.implementations) {
    checkFacts(declaration.decorators?.facts);
    methods(declaration.methods);
  }
  for (const declaration of program.types ?? []) checkFacts(declaration.decorators?.facts);
  const factProgram = factCount > 0 ? factChecks.program(ZERO_SPAN) : undefined;
  return (factProgram?.functions ?? []).map((declaration): FunctionDecl => ({
    ...declaration,
    span: (declaration.body[0] as { value?: Expression }).value?.span ?? declaration.span,
    defaultContext: { laterNames: [] },
  }));
}

// ---------------------------------------------------------------------------
// Member lines (annot.line.*).

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
    else if (line.value)
      facts =
        line.operator === "+=" ? [...facts, ...lineFacts(line.value)] : [...lineFacts(line.value)];
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
      `    Facts { items: [${facts.map((fact) => `${out.expression(fact)}${isSpreadFact(fact) ? "..." : ""}`).join(", ")}] }`,
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
