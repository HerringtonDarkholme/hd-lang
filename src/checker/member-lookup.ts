import type { Expression, FunctionDecl, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField, HirExpression, HirTrait, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericParts,
  readonlyType,
  displayType,
} from "../types.ts";
import { numericType } from "../numeric.ts";
import {
  erasedFieldType,
  genericTypeName,
  matchGenericTypePattern,
  matchImplementationTarget,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";
import { implementationsFor } from "./implementation-index.ts";
import { NEWTYPE_FIELD } from "./type-declarations.ts";
import { standardCoreTypeAlias } from "./standard-core.ts";
import { registeredPackageOwnership } from "./package-ownership.ts";
import { respelled, spellAs, spelledFieldType } from "./spelling.ts";
import { builtInMethodNames } from "./built-in-methods.ts";

import { ExpressionOperatorChecker } from "./expression-operators.ts";

/** One field read on a member path: `declaration.field` under `substitutions`. */
interface MemberStep {
  readonly declaration: HirData;
  readonly field: HirDataField;
  readonly substitutions: ReadonlyMap<string, ValueType>;
}

/**
 * The result of spec 03 Member Resolution for `x.name`. `steps` are the embedded
 * fields a promoted member is reached through; they are empty for own members.
 */
type MemberSelection =
  | { readonly kind: "field"; readonly steps: readonly MemberStep[]; readonly final: MemberStep }
  | {
      readonly kind: "inherent";
      readonly steps: readonly MemberStep[];
      readonly method: InherentMethod;
    }
  | { readonly kind: "trait" }
  | { readonly kind: "none" };

/** A part reached through embedded fields, at its depth (spec 03 Member Resolution). */
interface EmbeddedPart {
  readonly depth: number;
  readonly declaration: HirData;
  readonly type: ValueType;
  readonly substitutions: ReadonlyMap<string, ValueType>;
  readonly steps: readonly MemberStep[];
}

type PromotedSelection = Extract<MemberSelection, { readonly kind: "field" | "inherent" }>;

const MAX_EMBEDDING_DEPTH = 64;

/**
 * The Damerau-Levenshtein distance between two method names: a transposition
 * counts as one edit, so `psuh` is one edit from `push`.
 */
function nameDistance(left: string, right: string): number {
  const leftChars = [...left];
  const rightChars = [...right];
  const rows = leftChars.length + 1;
  const columns = rightChars.length + 1;
  const distance: number[][] = Array.from({ length: rows }, (_, row) =>
    Array.from({ length: columns }, (_, column) => (row === 0 ? column : column === 0 ? row : 0)),
  );
  for (let row = 1; row < rows; row += 1)
    for (let column = 1; column < columns; column += 1) {
      const cost = leftChars[row - 1] === rightChars[column - 1] ? 0 : 1;
      distance[row]![column] = Math.min(
        distance[row - 1]![column]! + 1,
        distance[row]![column - 1]! + 1,
        distance[row - 1]![column - 1]! + cost,
      );
      if (
        row > 1 &&
        column > 1 &&
        leftChars[row - 1] === rightChars[column - 2] &&
        leftChars[row - 2] === rightChars[column - 1]
      )
        distance[row]![column] = Math.min(
          distance[row]![column]!,
          distance[row - 2]![column - 2]! + 1,
        );
    }
  return distance[leftChars.length]![rightChars.length]!;
}

/** Whether an inherent `method` applies to a receiver of `type`. */
function inherentTargetMatches(method: InherentMethod, type: ValueType): boolean {
  return (
    method.targetType === type ||
    (method.targetGenericParameters !== undefined &&
      matchGenericTypePattern(method.targetType, type, new Map()))
  );
}

/**
 * Trait default bodies instantiated for an implementation. The prototype checks
 * them with a concrete `Self`; inside them a trait method named like a field of
 * `Self` is taken as the trait method (the body was written against the trait).
 */
export const traitDefaultDeclarations = new WeakMap<FunctionDecl, number>();

interface TraitLookupIndex {
  readonly byIndex: ReadonlyMap<number, HirTrait>;
  readonly byMember: ReadonlyMap<string, ReadonlySet<number>>;
  readonly sourceSize: number;
}

const traitLookupCache = new WeakMap<ReadonlyMap<string, HirTrait>, TraitLookupIndex>();

function traitLookupIndex(traitTypes: ReadonlyMap<string, HirTrait>): TraitLookupIndex {
  const cached = traitLookupCache.get(traitTypes);
  if (cached && cached.sourceSize === traitTypes.size) return cached;
  const byIndex = new Map<number, HirTrait>();
  const byMember = new Map<string, Set<number>>();
  for (const trait of traitTypes.values()) {
    byIndex.set(trait.index, trait);
    for (const method of trait.methods) {
      if (method.associated) continue;
      let owners = byMember.get(method.name);
      if (!owners) {
        owners = new Set();
        byMember.set(method.name, owners);
      }
      owners.add(trait.index);
    }
  }
  const index: TraitLookupIndex = { byIndex, byMember, sourceSize: traitTypes.size };
  traitLookupCache.set(traitTypes, index);
  return index;
}

export abstract class MemberLookupChecker extends ExpressionOperatorChecker {
  /**
   * `Name(value)` constructs the newtype `Name`, and `Base(value)` unwraps a
   * newtype over `Base` (04-type-system.md#transparent-aliases-and-newtypes).
   * Returns undefined for any other call.
   */
  protected checkNewtypeCall(
    expression: Extract<Expression, { kind: "call" }> & {
      readonly callee: { readonly name: string };
    },
    expected?: ValueType,
  ): HirExpression | undefined {
    const name = expression.callee.name;
    const numericName = standardCoreTypeAlias(this.imports.get(name) ?? name) ?? name;
    const newtype = this.dataTypes.get(name);
    const constructorOf = (type: ValueType): string =>
      nominalGenericParts(readonlyType(type))?.name ?? readonlyType(type);
    // Only a program with a newtype over `name` reads `name(value)` as an unwrap.
    const unwraps = [...this.dataTypes.values()].some(
      (declaration) =>
        declaration.newtype && constructorOf(declaration.fields[0]?.type ?? "") === name,
    );
    const single =
      expression.arguments.length === 1 &&
      !expression.argumentNames?.some(Boolean) &&
      !expression.argumentSpreads?.some(Boolean);
    // A constructor-style numeric cast (04 Numeric Casts).
    if (numericType(numericName) && !newtype?.newtype) {
      if (!single)
        this.fail("argument-count", `a numeric cast to '${name}' takes one value`, expression.span);
      // An integer literal argument, alone or under unary `-` or `+`, is
      // checked against the target range (types.cast.literal-range).
      const argument = expression.arguments[0]!;
      const literal =
        argument.kind === "integer" ||
        (argument.kind === "unary" &&
          (argument.operator === "-" || argument.operator === "+") &&
          argument.operand.kind === "integer");
      const target =
        literal && numericType(numericName)?.family !== "float" ? numericName : undefined;
      const value = this.checkExpression(argument, target);
      if (numericType(readonlyType(value.type)))
        // `usize(x)` prints as `usize` (04-type-system.md#r-types.alias.usize.display).
        return spellAs<HirExpression>(
          {
            kind: "unary",
            operator: "cast",
            operand: value,
            type: numericName,
            span: expression.span,
          },
          name,
        );
      if (!unwraps)
        this.fail(
          "type-mismatch",
          `'${name}(...)' casts a numeric value, found '${displayType(value.type)}'`,
          expression.span,
        );
      return this.unwrapNewtype(name, value, expression.span);
    }
    if (!newtype?.newtype && !unwraps) return undefined;
    if (newtype?.newtype) {
      if (!single)
        this.fail(
          "argument-count",
          `newtype '${name}' is constructed from exactly one positional value`,
          expression.span,
        );
      const typeArguments = (expression.callee as { readonly typeArguments?: readonly TypeRef[] })
        .typeArguments;
      const constructed = this.checkExpression(
        {
          kind: "data",
          name,
          ...(typeArguments ? { typeArguments } : {}),
          fields: [{ name: NEWTYPE_FIELD, value: expression.arguments[0]!, span: expression.span }],
          span: expression.span,
        },
        expected,
      );
      // A newtype over an `AnyVal` base is a value, not a fresh mutable
      // object (04-type-system.md#r-types.newtype.construct-value). Over an
      // `AnyRef` base it wraps the base value, so it carries that value's
      // permission (04-type-system.md#r-types.newtype.construct-permission).
      // The field stores the base as its declared readonly type, so the
      // supplied value's permission is under that weakening.
      const field = constructed.kind === "data" ? constructed.fields[0] : undefined;
      const base = field?.kind === "permission-weaken" ? field.operand : field;
      const mutable =
        this.isIdentityType(constructed.type) &&
        base !== undefined &&
        mutableInner(base.type) !== undefined;
      return mutable
        ? constructed
        : this.coerce(constructed, readonlyType(constructed.type), expression.span);
    }
    if (!single) return undefined;
    return this.unwrapNewtype(
      name,
      this.checkExpression(expression.arguments[0]!),
      expression.span,
    );
  }

  /** `name(value)` unwrapping a newtype whose base is `name`. */
  private unwrapNewtype(name: string, value: HirExpression, span: SourceSpan): HirExpression {
    const constructorOf = (type: ValueType): string =>
      nominalGenericParts(readonlyType(type))?.name ?? readonlyType(type);
    const expression = { span };
    const view = readonlyType(value.type);
    const declaration = this.dataTypes.get(nominalGenericParts(view)?.name ?? view);
    const field = declaration?.newtype ? declaration.fields[0] : undefined;
    if (!declaration || !field)
      this.fail(
        "type-mismatch",
        `'${name}(...)' unwraps a newtype over '${name}', found '${displayType(value.type)}'`,
        expression.span,
      );
    const substitutions = this.dataSubstitutions(declaration, value.type);
    const base = substituteGenericType(field.type, substitutions);
    if (constructorOf(base) !== name)
      this.fail(
        "type-mismatch",
        `'${name}(...)' cannot unwrap '${displayType(value.type)}', a newtype over '${displayType(base)}'`,
        expression.span,
      );
    const member = this.dataMember(value, declaration, field, substitutions, expression.span);
    // Unwrapping a newtype over an `AnyRef` base carries the newtype value's
    // permission (04-type-system.md#r-types.newtype.unwrap-permission).
    return mutableInner(value.type) !== undefined && this.isIdentityType(member.type)
      ? { ...member, type: mutableType(readonlyType(member.type)) }
      : member;
  }

  protected dataSubstitutions(
    declaration: HirData,
    type: ValueType,
  ): ReadonlyMap<string, ValueType> {
    const substitutions = new Map<string, ValueType>();
    const nominal = nominalGenericParts(readonlyType(type));
    if (nominal?.name === declaration.name) {
      declaration.genericParameters.forEach((parameter, index) =>
        substitutions.set(parameter, nominal.arguments[index]!),
      );
    }
    return substitutions;
  }

  protected dataMember(
    receiver: HirExpression,
    declaration: HirData,
    field: HirDataField,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): HirExpression {
    const declaredType = substituteGenericType(field.type, substitutions);
    // An embedded field follows its container's access (04 Mutable Paths).
    const type = field.embedded
      ? mutableInner(receiver.type) !== undefined
        ? mutableType(readonlyType(declaredType))
        : readonlyType(declaredType)
      : mutableInner(receiver.type) !== undefined || genericTypeName(field.type)
        ? declaredType
        : readonlyType(declaredType);
    return spellAs<HirExpression>(
      {
        kind: "member",
        receiver,
        dataIndex: declaration.index,
        fieldIndex: field.index,
        erasedFieldType: erasedFieldType(field.type),
        erasedTypeSubstitutions: orderedTypeSubstitutions(
          declaration.genericParameters,
          substitutions,
        ),
        type,
        span,
      },
      respelled(type, substituteGenericType(spelledFieldType(field) ?? field.type, substitutions)),
    );
  }

  /** Reads the embedded fields of a promoted member's path, outermost first. */
  protected memberPath(
    receiver: HirExpression,
    steps: readonly MemberStep[],
    span: SourceSpan,
  ): HirExpression {
    return steps.reduce(
      (current, step) =>
        this.dataMember(current, step.declaration, step.field, step.substitutions, span),
      receiver,
    );
  }

  private findInherentMethod(type: ValueType, name: string): InherentMethod | undefined {
    return this.inherentMethods.find(
      (method) => !method.associated && method.name === name && inherentTargetMatches(method, type),
    );
  }

  /**
   * The available trait of a known implementation for `type` that supplies a
   * method `name`. A trait that is not available at the call is invisible to
   * method lookup (spec 03 Member Resolution, Rust-style trait lookup).
   */
  private traitWithMember(type: ValueType, name: string): string | undefined {
    const index = traitLookupIndex(this.traitTypes);
    if (!index.byMember.has(name)) return undefined;
    for (const implementation of implementationsFor(this.implementations, type)) {
      if (!matchImplementationTarget(implementation, type, new Map())) continue;
      const trait = index.byIndex.get(implementation.traitIndex);
      if (
        trait &&
        this.traitAvailable(trait.name) &&
        trait.methods.some((method) => !method.associated && method.name === name)
      )
        return trait.name;
    }
    return undefined;
  }

  /**
   * Spec 09 Trait Availability: a trait is available when it is declared in
   * or imported into the calling module, or supplied by the prelude
   * (09-traits.md#r-trait.avail.module). A trait of another package is
   * available only where a use imports it (checker/package-ownership.ts).
   * The prototype still treats every trait of the calling module's own
   * package, and every std trait, as available.
   */
  protected traitAvailable(name: string): boolean {
    const ownership = registeredPackageOwnership(this.traitTypes);
    if (!ownership || this.declaration.standard === true) return true;
    const trait = this.traitTypes.get(name);
    if (!trait || trait.standardName !== undefined) return true;
    const here = this.declaration.span;
    return (
      ownership.packageOf(trait.span) === ownership.packageOf(here) ||
      ownership.imports(here, trait.name)
    );
  }

  /**
   * Spec 03 Member Resolution: an own field or inherent method is visible when
   * it is declared in the calling module or marked `pub`
   * (10-modules.md#r-module.vis.members). A `lib/std` type's private member
   * is hidden from code outside std (08-data-and-enums.md#field-visibility),
   * and another package's from code outside that package
   * (checker/package-ownership.ts). The prototype still shows every member
   * of the calling module's own package.
   */
  private memberVisible(member: HirDataField | InherentMethod, owner?: HirData): boolean {
    if (
      member.public === true ||
      this.declaration.standard === true ||
      this.declaration.privateAccess === true ||
      this.compilerPrivateMember
    )
      return true;
    if (owner?.standard === true) return false;
    const ownership = registeredPackageOwnership(this.traitTypes);
    if (!ownership) return true;
    let declared = owner?.span;
    if (!owner) {
      // An inherent method lives in its target's package (09-traits.md#r-trait.own.inherent).
      const target = nominalGenericParts(readonlyType((member as InherentMethod).targetType));
      const name = target?.name ?? readonlyType((member as InherentMethod).targetType);
      const data = this.dataTypes.get(name);
      const enumType = this.enumTypes.get(name);
      if (data?.standard === true || (!data && enumType?.standardName !== undefined)) return true;
      if (!data && !enumType) return true;
      declared = (member as InherentMethod).span;
    }
    return ownership.packageOf(declared!) === ownership.packageOf(this.declaration.span);
  }

  /**
   * Spec 03 Member Resolution. `x.name` uses field lookup and `x.name(args)`
   * uses method lookup; the two namespaces never interact (M2). Own fields and
   * inherent methods are at depth 0 and each part's `pub` fields and inherent
   * methods at its depth; the shallowest member with a name hides deeper ones.
   * Every module sees the same members (single view). Two members at the
   * smallest depth, or a private own member beside a promoted one, are
   * rejected at the declaration (`program-embedding.ts`), so lookup meets at
   * most one. Parts' trait methods are ignored. The receiver's available
   * trait methods are candidates beside the promoted method: both at once are
   * `ambiguous-method` (Rust-style trait lookup, TQ-36). An unavailable trait
   * is invisible. An own member that is not visible is skipped and reported
   * as `private-member` only when nothing matches.
   */
  protected selectField(receiverType: ValueType, name: string, span: SourceSpan): MemberSelection {
    return this.selectMember(receiverType, name, span, false);
  }

  protected selectMethod(receiverType: ValueType, name: string, span: SourceSpan): MemberSelection {
    return this.selectMember(receiverType, name, span, true);
  }

  /** The parts of `declaration` in order of depth, each with its embedded-field path. */
  private *embeddedParts(
    declaration: HirData,
    substitutions: ReadonlyMap<string, ValueType>,
  ): Generator<EmbeddedPart> {
    let frontier: EmbeddedPart[] = [
      { depth: 0, declaration, type: declaration.name, substitutions, steps: [] },
    ];
    for (let depth = 1; depth <= MAX_EMBEDDING_DEPTH && frontier.length > 0; depth += 1) {
      const next: EmbeddedPart[] = [];
      for (const node of frontier) {
        for (const embedded of node.declaration.fields) {
          if (!embedded.embedded) continue;
          const type = readonlyType(substituteGenericType(embedded.type, node.substitutions));
          const partDeclaration = this.dataTypes.get(nominalGenericParts(type)?.name ?? type);
          if (!partDeclaration) continue;
          const part: EmbeddedPart = {
            depth,
            declaration: partDeclaration,
            type,
            substitutions: this.dataSubstitutions(partDeclaration, type),
            steps: [
              ...node.steps,
              { declaration: node.declaration, field: embedded, substitutions: node.substitutions },
            ],
          };
          yield part;
          next.push(part);
        }
      }
      frontier = next;
    }
  }

  private selectMember(
    receiverType: ValueType,
    name: string,
    span: SourceSpan,
    method: boolean,
  ): MemberSelection {
    const type = readonlyType(receiverType);
    const nominal = nominalGenericParts(type);
    const declaration = this.dataTypes.get(nominal?.name ?? type);
    const substitutions = declaration
      ? this.dataSubstitutions(declaration, type)
      : new Map<string, ValueType>();
    // An own member of `S` that is not visible; the only source of private-member.
    let ownInvisible = false;
    // An available trait of `S` supplying a method `name`: the trait candidates.
    let traitCandidate: string | undefined;
    if (method) {
      const inherent = this.findInherentMethod(type, name);
      traitCandidate = this.traitWithMember(type, name);
      const defaultTrait = traitDefaultDeclarations.get(this.declaration);
      if (defaultTrait !== undefined && this.declaration.parameters[0]?.name === "self") {
        // A default body sees, through `self`, only the members of its trait
        // and supertraits (09-traits.md#default-method-bodies).
        const selfType = readonlyType(this.signature.parameters[0]!);
        if (type === selfType) {
          if (this.traitDeclaresMethod(defaultTrait, name)) return { kind: "trait" };
          this.fail(
            "unknown-method",
            `a default method body sees only the members of its trait and supertraits, which declare no method '${name}'`,
            span,
          );
        }
      }
      if (traitCandidate && defaultTrait !== undefined) return { kind: "trait" };
      if (inherent && this.memberVisible(inherent))
        return { kind: "inherent", steps: [], method: inherent };
      if (inherent) ownInvisible = true;
    } else {
      const field = declaration?.fields.find((candidate) => candidate.name === name);
      if (declaration && field && this.memberVisible(field, declaration))
        return { kind: "field", steps: [], final: { declaration, field, substitutions } };
      if (field) ownInvisible = true;
    }
    const promoted = declaration
      ? this.promotedMember(declaration, substitutions, name, method)
      : [];
    if (promoted.length > 1)
      // Unreachable after the declaration check; kept so lookup never guesses.
      this.fail(
        "ambiguous-promoted-member",
        `'${name}' is promoted by ${promoted.length} embedded paths at one depth; qualify it through an embedded field`,
        span,
      );
    const selected = promoted[0];
    if (selected && traitCandidate) {
      const path = selected.steps.map((step) => step.field.name).join(".");
      this.fail(
        "ambiguous-method",
        `'${name}' is a ${traitCandidate} method here and also a method promoted from the embedded field '${path}'; call it as ${traitCandidate}::${name}(...) or x.${path}.${name}(...)`,
        span,
      );
    }
    if (selected) return selected;
    if (traitCandidate) return { kind: "trait" };
    if (ownInvisible)
      this.fail(
        "private-member",
        `${method ? "method" : "field"} '${name}' is not visible from this module`,
        span,
      );
    return { kind: "none" };
  }

  /** Whether trait `traitIndex` or one of its transitive supertraits declares method `name`. */
  private traitDeclaresMethod(
    traitIndex: number,
    name: string,
    seen: Set<number> = new Set(),
  ): boolean {
    const index = traitLookupIndex(this.traitTypes);
    if (!index.byMember.has(name)) return false;
    if (seen.has(traitIndex)) return false;
    seen.add(traitIndex);
    const trait = index.byIndex.get(traitIndex);
    if (!trait) return false;
    return (
      trait.methods.some((method) => !method.associated && method.name === name) ||
      trait.supertraits.some((parent) => this.traitDeclaresMethod(parent.traitIndex, name, seen))
    );
  }

  /** The promoted (`pub`) members named `name` at the smallest depth that has one. */
  private promotedMember(
    declaration: HirData,
    substitutions: ReadonlyMap<string, ValueType>,
    name: string,
    method: boolean,
  ): PromotedSelection[] {
    const matches: PromotedSelection[] = [];
    let matchDepth: number | undefined;
    for (const part of this.embeddedParts(declaration, substitutions)) {
      if (matchDepth !== undefined && part.depth > matchDepth) break;
      if (method) {
        const promotedMethod = this.findInherentMethod(part.type, name);
        // Only a `pub` inherent method of a part is promoted, even in its own module.
        if (promotedMethod?.public)
          matches.push({ kind: "inherent", steps: part.steps, method: promotedMethod });
      } else {
        const promotedField = part.declaration.fields.find((candidate) => candidate.name === name);
        // Only a `pub` field of a part is promoted, even in its own module.
        if (promotedField?.public)
          matches.push({
            kind: "field",
            steps: part.steps,
            final: {
              declaration: part.declaration,
              field: promotedField,
              substitutions: part.substitutions,
            },
          });
      }
      if (matches.length > 0) matchDepth = part.depth;
    }
    return matches;
  }

  /**
   * The embedded-field path of the shallowest part whose type has a trait
   * method `name`, for the `x.Part.name(args)` hint of `unknown-method`.
   */
  /**
   * `unknown-method` for `name` on `receiverType`, with hints: a field of
   * that name, a trait method of an embedded type, and each trait in
   * `unavailable` that supplies the method but is not available at the
   * call, which needs a use (names.method-lookup.hint.use).
   */
  protected failUnknownMethod(
    receiverType: ValueType,
    name: string,
    span: SourceSpan,
    unavailable: ReadonlySet<string>,
  ): never {
    const traitPath = this.embeddedTraitMethodPath(receiverType, name);
    const similar = this.similarMethodNames(receiverType, name);
    const hints = [
      this.hasFieldNamed(receiverType, name)
        ? `; to call the function stored in the field, write (value.${name})(...)`
        : "",
      traitPath
        ? `; trait methods of embedded types are not promoted, so call it as value.${traitPath}.${name}(...)`
        : "",
      unavailable.size > 0
        ? `; the trait ${[...unavailable].map((trait) => `'${displayType(trait)}'`).join(", ")} supplies it, but this module does not import it: add a use declaration for it`
        : "",
      similar.length > 0
        ? `; did you mean ${similar.map((method) => `'${method}'`).join(", ")}?`
        : "",
    ];
    return this.fail(
      "unknown-method",
      `type '${displayType(receiverType)}' has no supported method '${name}'${hints.join("")}`,
      span,
    );
  }

  /**
   * Up to three supported method names of `receiverType` closest to `name`
   * by edit distance: inherent methods of its target and methods of the
   * available traits implemented for it, supertraits included.
   */
  private similarMethodNames(receiverType: ValueType, name: string): string[] {
    // Method lookup matches the readonly view, as selectMember does.
    const type = readonlyType(receiverType);
    const candidates = new Set<string>();
    for (const method of this.inherentMethods)
      if (!method.associated && inherentTargetMatches(method, type)) candidates.add(method.name);
    const head = nominalGenericParts(type)?.name;
    for (const intrinsic of builtInMethodNames(head)) candidates.add(intrinsic);
    const index = traitLookupIndex(this.traitTypes);
    for (const implementation of implementationsFor(this.implementations, type)) {
      if (!matchImplementationTarget(implementation, type, new Map())) continue;
      const trait = index.byIndex.get(implementation.traitIndex);
      if (trait && this.traitAvailable(trait.name))
        this.collectTraitMethods(trait, candidates, new Set());
    }
    candidates.delete(name);
    const allowed = Math.max(1, Math.floor(Math.max(name.length, 1) / 3));
    const scored = [...candidates]
      .map((candidate) => ({ candidate, distance: nameDistance(name, candidate) }))
      .filter(({ distance }) => distance <= allowed);
    scored.sort(
      (left, right) =>
        left.distance - right.distance || (left.candidate < right.candidate ? -1 : 1),
    );
    return scored.slice(0, 3).map(({ candidate }) => candidate);
  }

  /** The non-associated methods `trait` and its transitive supertraits declare. */
  private collectTraitMethods(trait: HirTrait, names: Set<string>, seen: Set<number>): void {
    if (seen.has(trait.index)) return;
    seen.add(trait.index);
    for (const method of trait.methods) if (!method.associated) names.add(method.name);
    const index = traitLookupIndex(this.traitTypes);
    for (const parent of trait.supertraits) {
      const declaration = index.byIndex.get(parent.traitIndex);
      if (declaration) this.collectTraitMethods(declaration, names, seen);
    }
  }

  protected embeddedTraitMethodPath(receiverType: ValueType, name: string): string | undefined {
    const type = readonlyType(receiverType);
    const declaration = this.dataTypes.get(nominalGenericParts(type)?.name ?? type);
    if (!declaration) return undefined;
    for (const part of this.embeddedParts(declaration, this.dataSubstitutions(declaration, type)))
      if (this.traitWithMember(part.type, name))
        return part.steps.map((step) => step.field.name).join(".");
    return undefined;
  }

  /** Whether field lookup would find `name`, for the `(x.name)(args)` hint. */
  protected hasFieldNamed(receiverType: ValueType, name: string): boolean {
    const type = readonlyType(receiverType);
    const declaration = this.dataTypes.get(nominalGenericParts(type)?.name ?? type);
    return declaration?.fields.some((field) => field.name === name) ?? false;
  }
}
