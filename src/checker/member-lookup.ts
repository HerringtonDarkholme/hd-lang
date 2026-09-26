import type { FunctionDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField, HirExpression, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";
import { mutableInner, nominalGenericParts, readonlyType } from "../types.ts";
import { genericTypeName, matchGenericTypePattern, substituteGenericType } from "./shared.ts";

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
export const traitDefaultDeclarations = new WeakSet<FunctionDecl>();

export abstract class MemberLookupChecker extends ExpressionOperatorChecker {
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
    const type =
      mutableInner(receiver.type) !== undefined || genericTypeName(field.type)
        ? declaredType
        : readonlyType(declaredType);
    return {
      kind: "member",
      receiver,
      dataIndex: declaration.index,
      fieldIndex: field.index,
      erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
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
      (method) => !method.associated && method.targetType === type && method.name === name,
    );
  }

  /**
   * The available trait of a known implementation for `type` that supplies a
   * method `name`. A trait that is not available at the call is invisible to
   * method lookup (spec 03 Member Resolution, Rust-style trait lookup).
   */
  private traitWithMember(type: ValueType, name: string): string | undefined {
    for (const implementation of this.implementations) {
      if (!matchGenericTypePattern(implementation.targetType, type, new Map())) continue;
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
   * Spec 03 Member Resolution: a field or inherent method is visible when it is
   * declared in the calling module or marked `pub`. Embedded fields are always
   * public, so the path never matters. The prototype compiles a single module,
   * so every member is declared in the calling module.
   */
  private memberVisible(_member: HirDataField | InherentMethod): boolean {
    return true;
  }

  /**
   * Spec 03 Member Resolution. `x.name` uses field lookup and `x.name(args)`
   * uses method lookup; the two namespaces never interact (M2). Own fields and
   * inherent methods are at depth 0 and each part's fields and inherent
   * methods at its depth; the shallowest member with a name hides deeper ones.
   * Two members at the smallest depth are rejected at the data declaration
   * (`program-embedding.ts`), so lookup meets at most one. Parts' trait
   * methods are ignored. The receiver's available trait methods are
   * candidates beside the promoted method: both at once are `ambiguous-method`
   * (Rust-style trait lookup, TQ-36). An unavailable trait is invisible.
   * Members that are not visible are skipped (P2); only an invisible own
   * member of the receiver's type is reported, and only when nothing matches.
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
      if (traitCandidate && traitDefaultDeclarations.has(this.declaration))
        return { kind: "trait" };
      if (inherent && this.memberVisible(inherent))
        return { kind: "inherent", steps: [], method: inherent };
      if (inherent) ownInvisible = true;
    } else {
      const field = declaration?.fields.find((candidate) => candidate.name === name);
      if (declaration && field && this.memberVisible(field))
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

  /** The visible promoted members named `name` at the smallest depth that has one. */
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
        // An invisible inherent method of an embedded type is ignored entirely.
        if (promotedMethod && this.memberVisible(promotedMethod))
          matches.push({ kind: "inherent", steps: part.steps, method: promotedMethod });
      } else {
        const promotedField = part.declaration.fields.find((candidate) => candidate.name === name);
        // An invisible field of an embedded type is ignored entirely.
        if (promotedField && this.memberVisible(promotedField))
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
