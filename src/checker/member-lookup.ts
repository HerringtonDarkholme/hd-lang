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

  /** Whether a known implementation for `type` supplies a trait method `name`. */
  private traitMemberPresent(type: ValueType, name: string): boolean {
    return this.implementations.some((implementation) => {
      if (!matchGenericTypePattern(implementation.targetType, type, new Map())) return false;
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      return trait?.methods.some((method) => !method.associated && method.name === name) ?? false;
    });
  }

  /**
   * Spec 03 Member Resolution: a field or inherent method is visible when it is
   * declared in the calling module or marked `pub`. The prototype compiles a
   * single module, so every member is declared in the calling module.
   */
  private memberVisible(_member: HirDataField | InherentMethod): boolean {
    return true;
  }

  /**
   * Spec 03 Member Resolution. `x.name` uses field lookup and `x.name(args)`
   * uses method lookup; the two namespaces never interact (M2). Each lookup
   * checks the receiver's own members first, then embedded fields breadth
   * first. Embedded types offer fields to field lookup and inherent methods
   * to method lookup; their trait methods are skipped. Members that are not
   * visible are skipped (P2) and reported only when nothing visible matches.
   */
  protected selectField(receiverType: ValueType, name: string, span: SourceSpan): MemberSelection {
    return this.selectMember(receiverType, name, span, false);
  }

  protected selectMethod(receiverType: ValueType, name: string, span: SourceSpan): MemberSelection {
    return this.selectMember(receiverType, name, span, true);
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
    let skipped = false;
    if (method) {
      const inherent = this.findInherentMethod(type, name);
      const trait = this.traitMemberPresent(type, name);
      if (trait && traitDefaultDeclarations.has(this.declaration)) return { kind: "trait" };
      if (inherent && this.memberVisible(inherent))
        return { kind: "inherent", steps: [], method: inherent };
      if (trait) return { kind: "trait" };
      if (inherent) skipped = true;
    } else {
      const field = declaration?.fields.find((candidate) => candidate.name === name);
      if (declaration && field && this.memberVisible(field))
        return { kind: "field", steps: [], final: { declaration, field, substitutions } };
      if (field) skipped = true;
    }
    if (!declaration) return { kind: "none" };
    let frontier: {
      declaration: HirData;
      substitutions: ReadonlyMap<string, ValueType>;
      steps: readonly MemberStep[];
      pathVisible: boolean;
    }[] = [{ declaration, substitutions, steps: [], pathVisible: true }];
    for (let depth = 1; depth <= MAX_EMBEDDING_DEPTH && frontier.length > 0; depth += 1) {
      const next: typeof frontier = [];
      const matches: MemberSelection[] = [];
      for (const node of frontier) {
        for (const embedded of node.declaration.fields) {
          if (!embedded.embedded) continue;
          const embeddedType = readonlyType(
            substituteGenericType(embedded.type, node.substitutions),
          );
          const embeddedNominal = nominalGenericParts(embeddedType);
          const embeddedDeclaration = this.dataTypes.get(embeddedNominal?.name ?? embeddedType);
          if (!embeddedDeclaration) continue;
          const steps = [
            ...node.steps,
            { declaration: node.declaration, field: embedded, substitutions: node.substitutions },
          ];
          const pathVisible = node.pathVisible && this.memberVisible(embedded);
          const embeddedSubstitutions = this.dataSubstitutions(embeddedDeclaration, embeddedType);
          if (method) {
            const promotedMethod = this.findInherentMethod(embeddedType, name);
            if (promotedMethod && pathVisible && this.memberVisible(promotedMethod))
              matches.push({ kind: "inherent", steps, method: promotedMethod });
            else if (promotedMethod) skipped = true;
          } else {
            const promotedField = embeddedDeclaration.fields.find(
              (candidate) => candidate.name === name,
            );
            if (promotedField && pathVisible && this.memberVisible(promotedField))
              matches.push({
                kind: "field",
                steps,
                final: {
                  declaration: embeddedDeclaration,
                  field: promotedField,
                  substitutions: embeddedSubstitutions,
                },
              });
            else if (promotedField) skipped = true;
          }
          next.push({
            declaration: embeddedDeclaration,
            substitutions: embeddedSubstitutions,
            steps,
            pathVisible,
          });
        }
      }
      if (matches.length > 1)
        this.fail(
          "ambiguous-promoted-member",
          `'${name}' is promoted by ${matches.length} embedded paths at depth ${depth}; qualify it through an embedded field`,
          span,
        );
      if (matches.length === 1) return matches[0]!;
      frontier = next;
    }
    if (skipped)
      this.fail(
        "private-member",
        `${method ? "method" : "field"} '${name}' is not visible from this module`,
        span,
      );
    return { kind: "none" };
  }

  /** Whether field lookup would find `name`, for the `(x.name)(args)` hint. */
  protected hasFieldNamed(receiverType: ValueType, name: string): boolean {
    const type = readonlyType(receiverType);
    const declaration = this.dataTypes.get(nominalGenericParts(type)?.name ?? type);
    return declaration?.fields.some((field) => field.name === name) ?? false;
  }
}
