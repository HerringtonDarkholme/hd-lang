import type { Expression, FunctionDecl, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField, HirExpression, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";
import { mutableInner, mutableType, nominalGenericParts, readonlyType } from "../types.ts";
import { numericType } from "../numeric.ts";
import {
  containsGenericType,
  genericTypeName,
  matchGenericTypePattern,
  matchImplementationTarget,
  orderedTypeSubstitutions,
  substituteGenericType,
} from "./shared.ts";
import { implementationsFor } from "./implementation-index.ts";
import { NEWTYPE_FIELD } from "./type-declarations.ts";

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
 * Trait default bodies instantiated for an implementation. The prototype checks
 * them with a concrete `Self`; inside them a trait method named like a field of
 * `Self` is taken as the trait method (the body was written against the trait).
 */
export const traitDefaultDeclarations = new WeakMap<FunctionDecl, number>();

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
    if (numericType(name) && !newtype?.newtype) {
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
      const target = literal && numericType(name)?.family !== "float" ? name : undefined;
      const value = this.checkExpression(argument, target);
      if (numericType(readonlyType(value.type)))
        return {
          kind: "unary",
          operator: "cast",
          operand: value,
          type: name,
          span: expression.span,
        };
      if (!unwraps)
        this.fail(
          "type-mismatch",
          `'${name}(...)' casts a numeric value, found '${value.type}'`,
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
        `'${name}(...)' unwraps a newtype over '${name}', found '${value.type}'`,
        expression.span,
      );
    const substitutions = this.dataSubstitutions(declaration, value.type);
    const base = substituteGenericType(field.type, substitutions);
    if (constructorOf(base) !== name)
      this.fail(
        "type-mismatch",
        `'${name}(...)' cannot unwrap '${value.type}', a newtype over '${base}'`,
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
    return {
      kind: "member",
      receiver,
      dataIndex: declaration.index,
      fieldIndex: field.index,
      erasedFieldType:
        containsGenericType(field.type) || field.type.includes("row:") ? field.type : undefined,
      erasedTypeSubstitutions: orderedTypeSubstitutions(
        declaration.genericParameters,
        substitutions,
      ),
      type,
      span,
    };
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
      (method) =>
        !method.associated &&
        method.name === name &&
        (method.targetType === type ||
          (method.targetGenericParameters !== undefined &&
            matchGenericTypePattern(method.targetType, type, new Map()))),
    );
  }

  /**
   * The available trait of a known implementation for `type` that supplies a
   * method `name`. A trait that is not available at the call is invisible to
   * method lookup (spec 03 Member Resolution, Rust-style trait lookup).
   */
  private traitWithMember(type: ValueType, name: string): string | undefined {
    for (const implementation of implementationsFor(this.implementations, type)) {
      if (!matchImplementationTarget(implementation, type, new Map())) continue;
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
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
   * Spec 09 Method Resolution: a trait is available when it is declared in or
   * imported into the calling module, or supplied by the prelude. The
   * prototype compiles a single module without trait imports, so every trait
   * it knows is declared there or in the prelude.
   */
  private traitAvailable(_trait: string): boolean {
    return true;
  }

  /**
   * Spec 03 Member Resolution: an own field or inherent method is visible when
   * it is declared in the calling module or marked `pub`. Promoted members are
   * always `pub`. The prototype compiles a single module, so every own member
   * is declared in the calling module.
   */
  /**
   * The prototype checks one module, so every member of the program's own
   * types is visible. Only a `lib/std` type's private field is hidden, from
   * code outside std (08-data-and-enums.md#field-visibility).
   */
  private memberVisible(member: HirDataField | InherentMethod, owner?: HirData): boolean {
    return (
      member.public === true ||
      owner?.standard !== true ||
      this.declaration.standard === true ||
      this.compilerPrivateMember
    );
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
        `'${name}' is a ${traitCandidate} method of this type and also a method promoted from the embedded field '${path}'; call it as ${traitCandidate}::${name}(x, ...) or x.${path}.${name}(...)`,
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
    if (seen.has(traitIndex)) return false;
    seen.add(traitIndex);
    const trait = [...this.traitTypes.values()].find((candidate) => candidate.index === traitIndex);
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
