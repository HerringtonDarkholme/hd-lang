import type { SourceSpan } from "../diagnostics.ts";
import type {
  HirEnum,
  HirEnumVariant,
  HirExpression,
  HirGenericBound,
  HirLocal,
  HirMatchBinding,
  HirPatternAccessStep,
  ValueType,
} from "../hir.ts";
import { displayType } from "../types.ts";
import { CallChecker } from "./calls.ts";
import type { Signature } from "./context-types.ts";
import {
  boundTraitType,
  refineVariant,
  subjectArguments,
  variantShape,
  type VariantRefinement,
} from "./gadt.ts";
import { substituteGenericType } from "./shared.ts";

/** What the patterns of one match arm learned about the subject's types (13-gadts.md#pattern-refinement). */
interface ArmRefinement {
  readonly equalities: Map<string, ValueType>;
  /** The arm's fresh existential parameters (13-gadts.md#r-gadt.existential.fresh). */
  readonly parameters: string[];
  /** Their bounds, each with its evidence local (13-gadts.md#r-gadt.runtime.evidence.match). */
  readonly bounds: { readonly bound: HirGenericBound; readonly local: number }[];
}

// GADT checking that needs the checker: refinement scopes, refinement
// conversions, and existential evidence (spec/lang/13-gadts.md).
export abstract class GadtChecker extends CallChecker {
  /** The type equalities of the enclosing GADT arms, by type parameter. */
  protected armEqualities: ReadonlyMap<string, ValueType> = new Map();
  /** The refinement the current arm's patterns collect, before its body is checked. */
  protected pendingRefinement: ArmRefinement | undefined;
  /** Every existential bound the function's arms introduced, after the signature's own. */
  protected existentialBounds: HirGenericBound[] = [];
  /** The evidence local of each entry of `existentialBounds`. */
  protected existentialBoundLocals: number[] = [];

  protected abstract addPatternLocal(name: string, type: ValueType, span: SourceSpan): HirLocal;

  /** `type` under the enclosing arms' equalities. */
  protected refineArmType(type: ValueType): ValueType {
    return this.armEqualities.size > 0 ? substituteGenericType(type, this.armEqualities) : type;
  }

  /**
   * Inside a GADT arm, two types equal under its equalities convert to each
   * other (13-gadts.md#r-gadt.refine.scope); the conversion is no runtime
   * cast (r[gadt.unify.no-cast]), only an erased parameter's boxing.
   */
  protected override coerce(
    value: HirExpression,
    expected: ValueType | undefined,
    span: SourceSpan,
    wrapOptional = true,
  ): HirExpression {
    if (
      this.armEqualities.size === 0 ||
      !expected ||
      value.type === expected ||
      value.type === "never"
    )
      return super.coerce(value, expected, span, wrapOptional);
    const from = this.refineArmType(value.type);
    const to = this.refineArmType(expected);
    if (from === value.type && to === expected)
      return super.coerce(value, expected, span, wrapOptional);
    const source = from === value.type ? value : refinement(value, from, span);
    const coerced = super.coerce(source, to, span, wrapOptional);
    if (coerced.type !== to) return value;
    return to === expected ? coerced : refinement(coerced, expected, span);
  }

  /** A read of a binding, seen at its type under the arm's equalities (13-gadts.md#r-gadt.refine.equalities). */
  protected refinedRead(read: HirExpression): HirExpression {
    const type = this.refineArmType(read.type);
    return type === read.type ? read : refinement(read, type, read.span);
  }

  /** Whether `variant` can construct a value of `subject`'s type (13-gadts.md#r-gadt.refine.exhaustive). */
  protected variantInhabits(
    declaration: HirEnum,
    variant: HirEnumVariant,
    subject: ValueType,
  ): boolean {
    if (!variant.gadt) return true;
    const arguments_ = subjectArguments(declaration, this.refineArmType(subject));
    return (
      arguments_ === undefined ||
      refineVariant(arguments_, variant.gadt, (name) => name) !== undefined
    );
  }

  /**
   * The payload substitutions of `variant` matched against a subject of
   * `subject`'s type. A GADT variant adds its equalities and existentials to
   * the arm's pending refinement and binds the evidence of its existential
   * bounds; one that cannot unify with the subject is
   * `impossible-gadt-pattern` (13-gadts.md#r-gadt.refine.impossible).
   */
  protected variantPatternSubstitutions(
    declaration: HirEnum,
    variant: HirEnumVariant,
    subject: ValueType,
    accessPath: readonly HirPatternAccessStep[],
    bindings: HirMatchBinding[],
    span: SourceSpan,
  ): Map<string, ValueType> {
    const refinedSubject = this.refineArmType(subject);
    const arguments_ = subjectArguments(declaration, refinedSubject);
    if (!variant.gadt || arguments_ === undefined) {
      const substitutions = new Map<string, ValueType>();
      declaration.genericParameters.forEach((parameter, index) => {
        const argument = arguments_?.[index];
        if (argument !== undefined) substitutions.set(parameter, argument);
      });
      return substitutions;
    }
    const pending = this.pendingRefinement;
    const taken = new Set([
      ...this.signature.genericParameters,
      ...(pending?.parameters ?? []),
      ...this.existentialBounds.map((bound) => bound.parameter),
    ]);
    const solved = refineVariant(arguments_, variant.gadt, (name) => {
      let fresh = name;
      for (let suffix = 2; taken.has(fresh); suffix += 1) fresh = `${name}${suffix}`;
      taken.add(fresh);
      return fresh;
    });
    if (!solved)
      this.fail(
        "impossible-gadt-pattern",
        `variant '${variant.name}' constructs '${displayType(variantShapeType(declaration, variant))}', which cannot be '${displayType(refinedSubject)}'`,
        span,
      );
    if (pending)
      this.recordRefinement(pending, declaration, variant, solved, accessPath, bindings, span);
    return new Map(solved.substitutions);
  }

  private recordRefinement(
    pending: ArmRefinement,
    declaration: HirEnum,
    variant: HirEnumVariant,
    solved: VariantRefinement,
    accessPath: readonly HirPatternAccessStep[],
    bindings: HirMatchBinding[],
    span: SourceSpan,
  ): void {
    for (const [name, type] of solved.equalities) pending.equalities.set(name, type);
    pending.parameters.push(...solved.existentials.values());
    for (const evidence of variant.gadt?.evidence ?? []) {
      const written = variant.gadt!.bounds[evidence.bound]!;
      const bound: HirGenericBound = {
        ...written,
        parameter: solved.existentials.get(written.parameter) ?? written.parameter,
        traitArguments: written.traitArguments.map((argument) =>
          substituteGenericType(argument, solved.substitutions),
        ),
      };
      const type = boundTraitType(bound);
      const local = this.addPatternLocal(`$evidence.${this.locals.length}`, type, span);
      bindings.push({
        local,
        fieldIndex: evidence.fieldIndex,
        type,
        accessPath: [
          ...accessPath,
          {
            kind: "enum",
            typeIndex: declaration.index,
            fieldIndex: evidence.fieldIndex,
            valueType: type,
          },
        ],
      });
      pending.bounds.push({ bound, local: local.index });
    }
  }

  /** Starts collecting the refinement of a match arm's patterns. */
  protected beginArmRefinement(): ArmRefinement | undefined {
    const outer = this.pendingRefinement;
    this.pendingRefinement = { equalities: new Map(), parameters: [], bounds: [] };
    return outer;
  }

  /**
   * Runs `check` under the refinement the arm's patterns collected: its
   * equalities, and its existential parameters with their bounds, whose
   * dictionaries the evidence locals hold (13-gadts.md#r-gadt.refine.arm-local).
   */
  protected withArmRefinement<T>(outer: ArmRefinement | undefined, check: () => T): T {
    const refinement = this.pendingRefinement!;
    this.pendingRefinement = outer;
    const savedEqualities = this.armEqualities;
    const savedSignature = this.signature;
    if (refinement.equalities.size > 0) {
      const equalities = new Map(savedEqualities);
      // An earlier equality reads through the new ones, so nested arms compose
      // (13-gadts.md#r-gadt.unify.nested).
      for (const [name, type] of equalities)
        equalities.set(name, substituteGenericType(type, refinement.equalities));
      for (const [name, type] of refinement.equalities) equalities.set(name, type);
      this.armEqualities = equalities;
    }
    if (refinement.parameters.length > 0 || refinement.bounds.length > 0) {
      const ownBounds = savedSignature.genericBounds.filter(
        (bound) => !this.existentialBounds.includes(bound),
      );
      for (const { bound, local } of refinement.bounds) {
        this.existentialBounds.push(bound);
        this.existentialBoundLocals.push(local);
      }
      this.signature = {
        ...savedSignature,
        genericParameters: [...savedSignature.genericParameters, ...refinement.parameters],
        genericBounds: [...ownBounds, ...this.existentialBounds],
      } satisfies Signature;
    }
    try {
      return check();
    } finally {
      this.armEqualities = savedEqualities;
      this.signature = savedSignature;
    }
  }

  /**
   * The dictionaries a constructed variant stores for its existential
   * bounds, under the solved variables (13-gadts.md#r-gadt.runtime.evidence).
   */
  protected variantEvidence(
    declaration: HirEnum,
    variant: HirEnumVariant,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): { readonly fieldIndices: number[]; readonly values: HirExpression[] } {
    const evidence = variant.gadt?.evidence ?? [];
    if (evidence.length === 0) return { fieldIndices: [], values: [] };
    const bounds = evidence.map((entry) => variant.gadt!.bounds[entry.bound]!);
    const values = this.resolveBoundDictionaries(
      {
        name: `${declaration.name}.${variant.name}`,
        index: -1,
        suspending: false,
        genericParameters: variant.gadt!.variables,
        genericBounds: bounds,
        rowParameters: [],
        parameters: [],
        parameterNames: [],
        defaultFunctionNames: [],
        variadic: false,
        result: "void",
        requirements: [],
        span: variant.span,
      },
      substitutions,
      span,
    );
    return { fieldIndices: evidence.map((entry) => entry.fieldIndex), values };
  }
}

function refinement(value: HirExpression, type: ValueType, span: SourceSpan): HirExpression {
  return { kind: "permission-weaken", operand: value, refinement: true, type, span };
}

/** A variant's result as written, for a diagnostic. */
function variantShapeType(declaration: HirEnum, variant: HirEnumVariant): ValueType {
  const { resultArguments } = variantShape(declaration, variant);
  return resultArguments.length > 0
    ? `${declaration.name}[${resultArguments.join(",")}]`
    : declaration.name;
}
