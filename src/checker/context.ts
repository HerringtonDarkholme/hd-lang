import { captureSources, isCaptureSource } from "./capture-view.ts";
import { intrinsicDictionaryPlan } from "./intrinsic-dictionaries.ts";
import { TRIAL_STATE, type TrialSnapshot } from "./call-speculation.ts";
import { snapshotCheckerState } from "./checker-trial-state.ts";
import { mapKeyKind } from "./map-keys.ts";
import { traitKeyParts, traitValueBindings } from "./associated-bindings.ts";
import { PRELUDE_NAMES } from "./prelude-names.ts";
import { standardImportHint } from "./standard-uses.ts";
import type { AssignmentStatement, Expression, FunctionDecl, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, DiagnosticFix, SourceSpan } from "../diagnostics.ts";
import type {
  HirExpression,
  HirEqualityDispatch,
  HirEqualityStrategy,
  HirCapture,
  HirData,
  HirEnum,
  HirFunction,
  HirGenericBound,
  HirGlobal,
  HirLocal,
  HirOrderingStrategy,
  HirStatement,
  HirTrait,
  HirBuiltinTraitImplementation,
  HirTraitDictionaryPlan,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import {
  HANDLE_TYPE,
  inspectableBuiltin,
  inspectKey,
  printedName,
  usesStandardInspect,
  type InspectEnvironment,
} from "./inspectable.ts";
import { isPermissionWeakening, weakenBoundedGenericActual } from "./assignability.ts";
import { isRowSubsumption, mismatchMessage, rowDiagnostic } from "./row-rules.ts";
import { requirementKeyDiagnosticsInType } from "./requirement-keys.ts";
import { INSPECTABLE } from "./standard-traits.ts";
import * as termination from "./termination.ts";
import { varianceConversion } from "./variance.ts";
import {
  genericTypeName,
  findImpl,
  matchTraitImplementation,
  normalizeBoundProjections,
  normalizeRowArguments,
  resolveGenericType,
  resolveTraitType,
  substituteGenericType,
  traitTypeName,
  MAX_BOUND_DEPTH,
} from "./shared.ts";
import {
  dynamicTraitProblemInType,
  signatureBoundScope,
  writtenApplicationBoundProblem,
  writtenBoundProblem,
  writtenTypeProblem,
} from "./written-type-validation.ts";
import {
  enclosingBoundProof,
  type BoundProof,
  findSupertraitPath,
  resolveTraitPath,
} from "./trait-paths.ts";
import {
  mutableInner,
  mutableType,
  nominalGenericType,
  optionalInner,
  readonlyType,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  displayType,
  uniqueTypes,
} from "../types.ts";
import { narrowsTo, numericType, sameWidthNumeric, widensTo } from "../numeric.ts";
import { captureOf, coerceLiteral, finalValueOf, type InferredReturn } from "./literal-join.ts";
import { joinedLeastCommonType } from "./literal-join.ts";
import { derivedFieldDiagnostic, isDerivedImplementation } from "./derive-intrinsics.ts";
import type {
  FunctionCheckResult,
  InherentMethod,
  ResolvedTraitPath,
  Signature,
} from "./context-types.ts";

export type {
  CheckResult,
  FunctionCheckResult,
  InherentMethod,
  PlannedArgument,
  ResolvedTraitPath,
  Signature,
} from "./context-types.ts";

export class CheckFailure extends Error {}

export { PRELUDE_NAMES, isPermissionWeakening, weakenBoundedGenericActual };

export { mapKeyKind };

import { isKnownType } from "./known-types.ts";
import { ZERO_SPAN } from "./generated-source.ts";
import {
  type InferredBinding,
  unresolvedCallFailure,
  unresolvedTypeFailure,
} from "./cannot-infer.ts";

export { isKnownType };

export abstract class CheckerContext {
  [TRIAL_STATE](snapshot: TrialSnapshot): undefined {
    return snapshotCheckerState(this, snapshot);
  }

  protected abstract checkStatement(
    statement: Statement,
    expected?: ValueType,
    valueContext?: boolean,
  ): HirStatement;
  protected abstract checkDestructuring(
    statement: Extract<Statement, { kind: "tuple-binding" | "pattern-binding" }>,
  ): HirStatement[];
  protected abstract checkCompoundAssignment(statement: AssignmentStatement): HirStatement[];
  protected abstract checkExpression(expression: Expression, expected?: ValueType): HirExpression;
  protected abstract checkExpressionHint(
    expression: Expression,
    expected: ValueType,
  ): HirExpression;
  protected abstract get expectedIsHint(): boolean;
  protected abstract isIdentityType(type: ValueType): boolean;

  protected readonly declaration: FunctionDecl;
  protected signature: Signature;
  protected readonly signatures: ReadonlyMap<string, Signature>;
  protected readonly dataTypes: ReadonlyMap<string, HirData>;
  protected readonly enumTypes: ReadonlyMap<string, HirEnum>;
  protected readonly traitTypes: ReadonlyMap<string, HirTrait>;
  protected implementations: readonly HirTraitImplementation[];
  protected inherentMethods: readonly InherentMethod[];
  protected readonly allImplementations: readonly HirTraitImplementation[];
  protected readonly allInherentMethods: readonly InherentMethod[];
  private readonly localImplementationScopes: Set<number>[];
  private readonly hasLocalImplementations: boolean;
  protected readonly synthetic: boolean;
  protected readonly moduleBody: boolean;
  protected readonly closures: HirFunction[];
  protected readonly insideClosure: boolean;
  protected readonly availableCaptures: ReadonlyMap<string, HirLocal>;
  protected readonly availableProviders: ReadonlyMap<string, HirLocal>;
  protected readonly inferRequirements: boolean;
  /** Whether this function's declared or inferred row is available at a source location. */
  protected readonly requirementsAvailableAt: ((span: SourceSpan) => boolean) | undefined;
  protected readonly inferResult: boolean;
  protected readonly selfClosureLocal?: HirLocal;
  protected readonly imports: ReadonlyMap<string, string>;
  /** The program is an integration test program, which may call `hd_run!` (spec/std/testing.md#r-std-testing.hd-run.integration-only). */
  protected readonly integrationTest: boolean;
  protected readonly globals: Map<string, HirGlobal>;
  protected pendingRecursiveClosure?: HirLocal;
  /** The `name := value` binding whose initializer is being checked without an annotation. */
  protected inferredBinding?: InferredBinding;
  protected readonly inferredRequirements: string[] = [];
  protected readonly inferredReturns: InferredReturn[] = [];
  protected readonly inferredPropagations: termination.InferredPropagation[] = [];
  protected readonly closureIndex: number;
  protected readonly captures = new Map<string, HirCapture>();
  protected readonly providerScopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly diagnostics: Diagnostic[] = [];
  protected readonly scopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly locals: HirLocal[] = [];
  /**
   * Every local a checked `kind: "local"` node reads, collected while
   * checking instead of walked afterward. Each construction site of an
   * inserted local node records its local here; speculative trials roll
   * the set back with the rest of the checker state.
   */
  protected readonly readLocals = new Set<HirLocal>();
  protected readonly loopResults: Array<ValueType | undefined> = [];
  protected readonly unavailableBindingLocals = new Set<number>();
  protected readonly allowedConditionalBindingLocals = new Set<number>();
  protected deferDepth = 0;
  /** Unsolved call generics; expected-type positions mentioning them do not instantiate the value. */
  protected pendingCallGenerics?: ReadonlySet<string>;
  /** In `h.fact::[M]()`, the handle's `F` and its witness (expression-inspect.ts). */
  protected handleWitness?: { readonly type: ValueType; readonly dictionary: HirExpression };
  /** A private field read synthesized by the checker rather than written in source. */
  protected compilerPrivateMember = false;
  /** Whether checking this function produced a panic node carrying a message. */
  protected hasPanicDetail = false;
  /** Whether every type is inspectable, for a fact value or a handle witness. */
  protected anyTypeInspectable = false;

  /** Takes the pending call generics, as a test for types that mention them. */
  protected takePendingCallGenerics(): (type: ValueType) => boolean {
    const pending = this.pendingCallGenerics;
    this.pendingCallGenerics = undefined;
    if (!pending || pending.size === 0) return () => false;
    const marker = new Map([...pending].map((name) => [name, "$pending"] as const));
    return (type) => substituteGenericType(type, marker) !== type;
  }

  constructor(
    declaration: FunctionDecl,
    signature: Signature,
    signatures: ReadonlyMap<string, Signature>,
    dataTypes: ReadonlyMap<string, HirData>,
    enumTypes: ReadonlyMap<string, HirEnum>,
    traitTypes: ReadonlyMap<string, HirTrait>,
    implementations: readonly HirTraitImplementation[],
    inherentMethods: readonly InherentMethod[],
    synthetic: boolean,
    moduleBody: boolean,
    closures: HirFunction[],
    insideClosure = false,
    availableCaptures: ReadonlyMap<string, HirLocal> = new Map(),
    closureIndex = -1,
    availableProviders: ReadonlyMap<string, HirLocal> = new Map(),
    inferRequirements = false,
    inferResult = false,
    selfClosureLocal?: HirLocal,
    imports: ReadonlyMap<string, string> = new Map(),
    globals: Map<string, HirGlobal> = new Map(),
    localImplementations: ReadonlySet<number> = new Set(),
    integrationTest = false,
    requirementsAvailableAt?: (span: SourceSpan) => boolean,
  ) {
    this.declaration = declaration;
    this.signature = signature;
    this.signatures = signatures;
    this.dataTypes = dataTypes;
    this.enumTypes = enumTypes;
    this.traitTypes = traitTypes;
    this.allImplementations = implementations;
    this.allInherentMethods = inherentMethods;
    this.implementations = implementations;
    this.inherentMethods = inherentMethods;
    this.hasLocalImplementations =
      implementations.some((implementation) => implementation.localImplementation !== undefined) ||
      inherentMethods.some((method) => method.localImplementation !== undefined);
    this.localImplementationScopes = [new Set(localImplementations)];
    this.refreshImplementations();
    this.synthetic = synthetic;
    this.moduleBody = moduleBody;
    this.closures = closures;
    this.insideClosure = insideClosure;
    this.availableCaptures = availableCaptures;
    this.availableProviders = availableProviders;
    this.inferRequirements = inferRequirements;
    this.requirementsAvailableAt = requirementsAvailableAt;
    this.inferResult = inferResult;
    this.selfClosureLocal = selfClosureLocal;
    this.imports = imports;
    this.globals = globals;
    this.closureIndex = closureIndex;
    this.providerScopes[0] = new Map(availableProviders);
    this.integrationTest = integrationTest;
  }

  /** The local implementations visible at the current lexical point. */
  protected visibleLocalImplementations(): ReadonlySet<number> {
    return new Set(this.localImplementationScopes.flatMap((scope) => [...scope]));
  }

  /** Makes a hoisted local implementation available from its marker onward. */
  protected activateLocalImplementation(implementation: number): void {
    this.localImplementationScopes.at(-1)!.add(implementation);
    this.refreshImplementations();
  }

  private refreshImplementations(): void {
    if (!this.hasLocalImplementations) return;
    const visible = this.visibleLocalImplementations();
    this.implementations = this.allImplementations.filter(
      (implementation) =>
        implementation.localImplementation === undefined ||
        visible.has(implementation.localImplementation),
    );
    this.inherentMethods = this.allInherentMethods.filter(
      (method) =>
        method.localImplementation === undefined || visible.has(method.localImplementation),
    );
  }

  check(): FunctionCheckResult {
    try {
      const parameters = this.declaration.parameters.map((parameter, index) => {
        if (PRELUDE_NAMES.has(parameter.name)) {
          this.fail(
            "prelude-name-shadow",
            `parameter '${parameter.name}' shadows a prelude name`,
            parameter.span,
          );
        }
        const type = this.signature.parameters[index] ?? this.resolveType(parameter.type);
        if (this.currentScope().has(parameter.name))
          this.fail("duplicate-binding", `duplicate parameter '${parameter.name}'`, parameter.span);
        const local: HirLocal = {
          name: parameter.name,
          type,
          index,
          mutable: false,
          parameter: true,
          span: parameter.span,
        };
        this.currentScope().set(parameter.name, local);
        this.locals.push(local);
        return local;
      });
      const body = this.checkStatements(
        this.declaration.body,
        false,
        this.inferResult ? undefined : this.signature.result,
        this.inferResult,
      );
      let result = this.signature.result;
      if (this.inferResult) {
        const last = body.at(-1);
        if (last?.kind !== "return")
          this.recordInferredReturn(this.blockType(body), last?.span, finalValueOf(body));
        const inferred = joinedLeastCommonType(this.inferredReturns, {
          data: this.dataTypes,
          enums: this.enumTypes,
        });
        if (!("type" in inferred)) {
          const listed = uniqueTypes(this.inferredReturns).map(displayType).join(", ");
          this.fail(
            inferred.code,
            inferred.code === "no-common-type"
              ? `function return paths have no common type: ${listed}`
              : `function return paths have no unique least common type: ${listed}; declare the result type`,
            this.inferredReturns.at(-1)?.span ?? this.declaration.span,
          );
        }
        result = inferred.type;
        const mismatch = termination.mismatchedPropagation(result, this.inferredPropagations);
        if (mismatch) this.fail("no-common-type", mismatch.message, mismatch.span);
      } else {
        this.checkFallthrough(body);
        const failure = termination.resultFailure(
          this.declaration,
          this.synthetic,
          result,
          this.traitTypes,
          this.implementations,
          this.imports,
        );
        if (failure) this.fail("unsatisfied-trait-bound", failure.message, failure.span);
      }
      for (const local of this.locals) {
        if (
          local.parameter ||
          local.name.startsWith("$") ||
          local.name.startsWith("_") ||
          this.readLocals.has(local)
        )
          continue;
        this.diagnostics.push({
          code: "unused-local-binding",
          message: `local binding '${local.name}' is never read`,
          span: local.span,
          severity: "warning",
        });
      }
      return {
        function: {
          ...(this.declaration.standard ? { standard: true as const } : {}),
          name: this.declaration.name,
          index: this.signature.index,
          suspending: this.declaration.suspending,
          ...(this.declaration.suspending && this.insideClosure
            ? {
                suspensionIndex:
                  Math.max(-1, ...[...this.signatures.values()].map(({ index }) => index)) +
                  1 +
                  this.closureIndex,
              }
            : {}),
          variadic: this.signature.variadic,
          genericParameters: this.signature.genericParameters,
          genericBounds: this.signature.genericBounds,
          rowParameters: this.signature.rowParameters,
          parameters,
          result,
          requirements: this.inferRequirements
            ? [...this.inferredRequirements].sort()
            : this.signature.requirements,
          locals: this.locals,
          body,
          span: this.declaration.span,
          synthetic: this.synthetic,
          closure: this.insideClosure,
          captures: [...this.captures.values()],
          ...(this.declaration.testOptions ? { testOptions: this.declaration.testOptions } : {}),
          ...(this.declaration.intrinsic ? { intrinsic: this.declaration.intrinsic } : {}),
        },
        diagnostics: this.diagnostics,
        hasPanicDetail: this.hasPanicDetail,
      };
    } catch (error) {
      if (!(error instanceof CheckFailure)) throw error;
      return {
        diagnostics: this.diagnostics,
        hasPanicDetail: this.hasPanicDetail,
        inferredRequirements: [...this.inferredRequirements],
      };
    }
  }

  protected checkStatements(
    statements: readonly Statement[],
    scoped = false,
    expectedFinal?: ValueType,
    finalValueContext = false,
  ): HirStatement[] {
    if (scoped) {
      this.scopes.push(new Map());
      this.localImplementationScopes.push(new Set());
    }
    try {
      const checked: HirStatement[] = [];
      let unreachable = false;
      for (const [index, statement] of statements.entries()) {
        if (unreachable) {
          this.diagnostics.push({
            code: "unreachable-code",
            message: "this statement is unreachable",
            span: statement.span,
            severity: "warning",
          });
        }
        if (statement.kind === "tuple-binding" || statement.kind === "pattern-binding") {
          checked.push(...this.checkDestructuring(statement));
          continue;
        }
        if ("compound" in statement && statement.compound) {
          checked.push(...this.checkCompoundAssignment(statement));
          continue;
        }
        const final = index === statements.length - 1;
        const result = this.checkStatement(
          statement,
          final ? expectedFinal : undefined,
          final && finalValueContext,
        );
        checked.push(result);
        unreachable ||=
          statement.kind === "return" ||
          statement.kind === "break" ||
          statement.kind === "continue" ||
          (result.kind === "expression" && result.expression.type === "never");
      }
      return checked;
    } finally {
      if (scoped) {
        this.scopes.pop();
        this.localImplementationScopes.pop();
        this.refreshImplementations();
      }
    }
  }

  protected coerce(
    value: HirExpression,
    expected: ValueType | undefined,
    span: SourceSpan,
    wrapOptional = true,
  ): HirExpression {
    if (!expected || value.type === expected || value.type === "never") return value;
    const widened = coerceLiteral(value, readonlyType(expected), span, this.fail.bind(this));
    if (widened) return widened;
    // Row subsumption adapts a function value like a weakening (r-req.row.subsume).
    if (isPermissionWeakening(value.type, expected) || isRowSubsumption(value.type, expected)) {
      return { kind: "permission-weaken", operand: value, type: expected, span };
    }
    const variance = varianceConversion(value.type, expected, {
      data: this.dataTypes,
      enums: this.enumTypes,
    });
    if (variance === "representation-change")
      this.fail(
        "variance-representation-change",
        `'${displayType(value.type)}' cannot become '${displayType(expected)}': a variance conversion must not change a value's representation`,
        span,
      );
    if (variance) return { kind: "permission-weaken", operand: value, type: expected, span };
    const storedSuspension = storedSuspensionParts(expected);
    const concreteSuspension = suspensionParts(value.type) ?? traitSuspensionParts(value.type);
    if (storedSuspension && concreteSuspension?.result === storedSuspension.result) {
      return {
        kind: "suspension-wrap",
        suspension: value,
        type: expected,
        span,
      };
    }
    const traitName = traitTypeName(expected);
    const trait = traitName && this.traitTypes.get(traitName);
    if (trait) {
      const mutableTrait = mutableInner(expected) !== undefined;
      const inspectTarget = this.isStandardInspectable(trait);
      if (mutableTrait && mutableInner(value.type) === undefined) {
        if (inspectTarget && inspectKey(value.type, this.inspectEnvironment()))
          this.fail(
            "mutable-upgrade",
            `readonly type '${displayType(value.type)}' cannot be erased to mut Inspectable`,
            span,
          );
        return value;
      }
      const expectedTraitKey = readonlyType(expected).slice("trait:".length);
      const expectedTraitArguments = traitKeyParts(expectedTraitKey).positional;
      // A trait value type's bindings must agree with the value's
      // (09-traits.md#r-trait.dyn.binding.convert, #r-trait.dyn.binding.widen).
      const expectedBindings = traitValueBindings(expected);
      const sourceTraitName = traitTypeName(value.type);
      const sourceTrait = sourceTraitName && this.traitTypes.get(sourceTraitName);
      const sourceTraitKey = sourceTrait
        ? readonlyType(value.type).slice("trait:".length)
        : undefined;
      const sourceTraitArguments = sourceTraitKey ? traitKeyParts(sourceTraitKey).positional : [];
      const sourceBindings = traitValueBindings(value.type);
      const supertraitPath = sourceTrait
        ? this.findSupertraitPath(
            sourceTrait,
            sourceTraitArguments,
            trait.index,
            expectedTraitArguments,
          )
        : undefined;
      if (
        sourceTrait &&
        supertraitPath &&
        [...expectedBindings].every(([name, bound]) => sourceBindings.get(name) === bound)
      ) {
        return {
          kind: "trait-upcast",
          value,
          sourceTraitIndex: sourceTrait.index,
          targetTraitIndex: trait.index,
          supertraitPath,
          type: expected,
          span,
        };
      }
      const implementationType = readonlyType(value.type);
      const erasedParameter =
        inspectTarget || !mutableTrait ? genericTypeName(implementationType) : undefined;
      if (erasedParameter) {
        // Erasing `x: T` needs `T < Inspectable`; the bound's dictionary
        // records the instantiated type (spec/lang/09-traits.md#erasure-to-inspectable).
        // A value of `T < Trait` erases to `Trait` through the same dictionary.
        const boundIndex = this.signature.genericBounds.findIndex(
          (bound) => bound.parameter === erasedParameter && bound.traitIndex === trait.index,
        );
        if (boundIndex >= 0)
          return {
            kind: "trait-bound",
            value,
            traitIndex: trait.index,
            boundIndex,
            type: expected,
            span,
          };
      }
      const implementation = this.implementations.find((candidate) => {
        return Boolean(
          matchTraitImplementation(
            candidate,
            trait.index,
            implementationType,
            expectedTraitArguments,
          ),
        );
      });
      if (
        implementation &&
        [...expectedBindings].every(
          ([name, bound]) =>
            this.implementationAssociatedType(
              implementationType,
              trait,
              expectedTraitArguments,
              name,
            ) === bound,
        )
      ) {
        const receiverType = mutableTrait ? mutableType(implementationType) : implementationType;
        const wrappedValue =
          receiverType === value.type ? value : this.coerce(value, receiverType, span);
        return {
          kind: "trait-wrap",
          value: wrappedValue,
          traitIndex: trait.index,
          dictionary: this.traitDictionaryPlan(
            implementation,
            implementationType,
            expectedTraitArguments,
            span,
          ),
          type: expected,
          span,
        };
      }
      const builtin =
        mutableTrait && !inspectTarget && trait.name !== "Any"
          ? undefined
          : this.builtinTraitDictionaryPlan(
              trait.index,
              implementationType,
              expectedTraitArguments,
              span,
            );
      if (builtin) {
        return {
          kind: "trait-wrap",
          value:
            value.type === implementationType
              ? value
              : this.coerce(value, implementationType, span),
          traitIndex: trait.index,
          dictionary: builtin,
          type: expected,
          span,
        };
      }
    }
    const inner = optionalInner(expected);
    if (inner !== undefined && wrapOptional) {
      // The implicit wrap adds one layer only: `T` is a `T?`, never a `T??`.
      const payload = this.coerce(value, inner, span, false);
      if (payload.type === inner) {
        return {
          kind: "variant-wrap",
          variant: "optional-present",
          payload,
          payloadType: inner,
          type: expected,
          span,
        };
      }
    }
    return value;
  }

  /**
   * The associated type `name` of a concrete type's implementation of `trait`
   * or of the supertrait of `trait` that declares it
   * (09-traits.md#r-trait.binding.name-reach.meaning).
   */
  protected implementationAssociatedType(
    type: ValueType,
    trait: HirTrait,
    traitArguments: readonly ValueType[],
    name: string,
    depth = 0,
  ): ValueType | undefined {
    const bound = traitValueBindings(type).get(name);
    if (bound !== undefined) return bound;
    const index = trait.associatedTypes.findIndex((associated) => associated.name === name);
    if (index >= 0) {
      for (const candidate of this.implementations) {
        const substitutions = matchTraitImplementation(
          candidate,
          trait.index,
          readonlyType(type),
          traitArguments,
        );
        if (substitutions)
          return substituteGenericType(candidate.associatedTypes[index]!, substitutions);
      }
      return undefined;
    }
    if (depth > 64) return undefined;
    const parameters = new Map(
      trait.genericParameters.map((parameter, position) => [parameter, traitArguments[position]!]),
    );
    parameters.set("Self", readonlyType(type));
    for (const supertrait of trait.supertraits) {
      const parent = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === supertrait.traitIndex,
      );
      if (!parent) continue;
      const found = this.implementationAssociatedType(
        type,
        parent,
        supertrait.traitArguments.map((argument) => substituteGenericType(argument, parameters)),
        name,
        depth + 1,
      );
      if (found !== undefined) return found;
    }
    return undefined;
  }

  protected findSupertraitPath(
    trait: HirTrait,
    traitArguments: readonly ValueType[],
    targetIndex: number,
    targetArguments: readonly ValueType[],
  ): readonly number[] | undefined {
    return findSupertraitPath(this.traitTypes, trait, traitArguments, targetIndex, targetArguments);
  }

  protected resolveTraitPath(
    trait: HirTrait,
    traitArguments: readonly ValueType[],
    path: readonly number[],
  ): ResolvedTraitPath {
    return resolveTraitPath(this.traitTypes, trait, traitArguments, path);
  }

  protected traitDictionaryPlan(
    implementation: HirTraitImplementation,
    targetType: ValueType,
    traitArguments: readonly ValueType[],
    span: SourceSpan,
    seen: ReadonlySet<string> = new Set(),
  ): HirTraitDictionaryPlan {
    const key = `${implementation.index}:${targetType}:${traitArguments.join(",")}`;
    if (seen.has(key))
      this.fail(
        "trait-resolution-depth",
        `constructing the trait dictionary for '${displayType(targetType)}' requires itself`,
        span,
      );
    // This plan proves a bound of depth `seen.size + 1` (09-traits.md#r-trait.bound.depth).
    if (seen.size >= MAX_BOUND_DEPTH)
      this.fail(
        "trait-resolution-depth",
        `proving the bound for '${displayType(targetType)}' needs a bound deeper than ${MAX_BOUND_DEPTH}`,
        span,
      );
    const substitutions = matchTraitImplementation(
      implementation,
      implementation.traitIndex,
      targetType,
      traitArguments,
    );
    if (!substitutions)
      throw new Error(`implementation ${implementation.index} does not match ${targetType}`);
    const next = new Set([...seen, key]);
    if (implementation.intrinsic)
      return intrinsicDictionaryPlan(
        implementation,
        this.traitTypes.get(implementation.traitName)!,
        targetType,
        substitutions,
        (traitIndex, traitArguments) => {
          const type = readonlyType(targetType);
          const found = findImpl(this.implementations, traitIndex, type, traitArguments);
          const plan = found
            ? this.traitDictionaryPlan(found.impl, found.type, traitArguments, span, next)
            : this.builtinTraitDictionaryPlan(traitIndex, type, traitArguments, span);
          if (!plan) throw new Error(`no implementation of supertrait ${traitIndex} for ${type}`);
          return plan;
        },
      );
    this.inferBoundAssociatedTypes(implementation, substitutions);
    const bounds = implementation.genericBounds.map((bound) =>
      this.boundDictionaryExpression(bound, substitutions, span, next),
    );
    const trait = this.traitTypes.get(implementation.traitName)!;
    const specializedTraitArguments = implementation.traitArguments.map((argument) =>
      substituteGenericType(argument, substitutions),
    );
    const traitSubstitutions = new Map(
      trait.genericParameters.map(
        (parameter, index) => [parameter, specializedTraitArguments[index]!] as const,
      ),
    );
    traitSubstitutions.set("Self", readonlyType(targetType));
    const supertraits = implementation.supertraitImplementations.map((parentIndex, index) => {
      const parentArguments = trait.supertraits[index]!.traitArguments.map((argument) =>
        substituteGenericType(argument, traitSubstitutions),
      );
      if (parentIndex < 0) {
        // A compiler-supplied supertrait such as Inspectable has no source impl.
        const builtin = this.builtinTraitDictionaryPlan(
          trait.supertraits[index]!.traitIndex,
          targetType,
          parentArguments,
          span,
        );
        if (builtin) return builtin;
        throw new Error(`no implementation of supertrait ${index} for ${targetType}`);
      }
      const parent = this.implementations.find((candidate) => candidate.index === parentIndex);
      if (!parent)
        this.fail(
          "unsatisfied-trait-bound",
          `the required supertrait '${displayType(trait.supertraits[index]!.traitName)}' is not visible here`,
          span,
        );
      return this.traitDictionaryPlan(parent, targetType, parentArguments, span, next);
    });
    return { bounds, implementationIndex: implementation.index, supertraits };
  }

  /** Infers implementation parameters fixed only by `Name = type` bindings on concrete bounds. */
  private inferBoundAssociatedTypes(
    implementation: HirTraitImplementation,
    substitutions: Map<string, ValueType>,
  ): void {
    for (const bound of implementation.genericBounds) {
      const actual = substitutions.get(bound.parameter);
      if (!actual || genericTypeName(actual) || !bound.associatedBindings) continue;
      const traitArguments = bound.traitArguments.map((argument) =>
        substituteGenericType(argument, substitutions),
      );
      const provider = this.implementations.find((candidate) =>
        Boolean(matchTraitImplementation(candidate, bound.traitIndex, actual, traitArguments)),
      );
      const trait = this.traitTypes.get(bound.traitName);
      if (!provider || !trait) continue;
      const providerSubstitutions = matchTraitImplementation(
        provider,
        bound.traitIndex,
        actual,
        traitArguments,
      )!;
      for (const binding of bound.associatedBindings) {
        const index = trait.associatedTypes.findIndex(
          (associated) => associated.name === binding.name,
        );
        const free = genericTypeName(binding.type);
        if (index < 0 || !free || substitutions.has(free)) continue;
        substitutions.set(
          free,
          substituteGenericType(provider.associatedTypes[index]!, providerSubstitutions),
        );
      }
    }
  }

  private boundDictionaryExpression(
    bound: HirGenericBound,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
    seen: ReadonlySet<string>,
    code = "unsatisfied-trait-bound",
  ): HirExpression {
    const actual = substitutions.get(bound.parameter);
    if (!actual)
      this.fail(
        "cannot-infer-type",
        `could not infer implementation parameter ${displayType(bound.parameter)}`,
        span,
      );
    const traitArguments = bound.traitArguments.map((argument) =>
      substituteGenericType(argument, substitutions),
    );
    const traitKey =
      traitArguments.length > 0
        ? nominalGenericType(bound.traitName, traitArguments)
        : bound.traitName;
    const forwarded = genericTypeName(actual);
    if (forwarded) {
      const found = this.parameterBound(forwarded, bound.traitIndex, traitArguments);
      if (!found)
        this.fail(
          code,
          `generic parameter '${displayType(forwarded)}' does not implement ${displayType(bound.traitName)}, required by the implementation bound on '${displayType(bound.parameter)}'`,
          span,
        );
      const { boundIndex, supertrait } = found;
      const type = `trait:${traitKey}`;
      return {
        kind: "trait-bound-dictionary",
        traitIndex: bound.traitIndex,
        boundIndex,
        supertrait,
        type,
        span,
      };
    }
    const found = findImpl(this.implementations, bound.traitIndex, actual, traitArguments);
    if (!found) {
      const builtin = this.builtinTraitDictionaryPlan(
        bound.traitIndex,
        actual,
        traitArguments,
        span,
      );
      if (builtin)
        return {
          kind: "trait-dictionary",
          traitIndex: bound.traitIndex,
          dictionary: builtin,
          type: `trait:${traitKey}`,
          span,
        };
      this.fail(
        code,
        `type '${displayType(actual)}' does not implement ${displayType(bound.traitName)}, required by the implementation bound on '${displayType(bound.parameter)}'`,
        span,
      );
    }
    return {
      kind: "trait-dictionary",
      traitIndex: bound.traitIndex,
      dictionary: this.traitDictionaryPlan(found.impl, found.type, traitArguments, span, seen),
      type: `trait:${traitKey}`,
      span,
    };
  }

  /**
   * The bound of type parameter `parameter` on trait `traitIndex`, or on a
   * trait that extends it, as `T < Integer` extends `Ord` and so `PartialOrd`
   * and `Eq` (09-traits.md#supertraits), with the supertrait path to it.
   */
  private parameterBound(
    parameter: string,
    traitIndex: number,
    traitArguments: readonly ValueType[],
  ): BoundProof | undefined {
    return enclosingBoundProof(
      this.signature.genericBounds,
      this.traitTypes,
      parameter,
      traitIndex,
      traitArguments,
    );
  }

  /**
   * Whether a value of `type` has mutable access: its type is `mut U`, or it
   * is a type parameter of this function bounded by `mut Trait` or `mut Any`
   * (04-type-system.md#r-types.path.access.mut-bound).
   */
  protected hasMutableAccess(type: ValueType): boolean {
    if (mutableInner(type) !== undefined) return true;
    const parameter = genericTypeName(type);
    return (
      parameter !== undefined &&
      (this.signature.genericBounds.some(
        (bound) => bound.parameter === parameter && bound.mutable,
      ) ||
        (this.signature.mutableParameters ?? []).includes(parameter))
    );
  }

  /** The standard `Inspectable`, declared by a `std.inspect` or `std.error` import. */
  protected isStandardInspectable(trait: HirTrait): boolean {
    return trait.name === INSPECTABLE && usesStandardInspect(this.imports);
  }

  protected inspectEnvironment(): InspectEnvironment {
    const inspectable = this.traitTypes.get(INSPECTABLE);
    return {
      // A type declared in a block suite is not inspectable (09-traits.md#inspectable-types).
      nominal: (name) => {
        const declaration = this.dataTypes.get(name) ?? this.enumTypes.get(name);
        return declaration === undefined || declaration.local
          ? undefined
          : printedName(name, declaration);
      },
      trait: (name) => printedName(name, this.traitTypes.get(name)),
      inspectableParameter: (name) =>
        this.inspectableBound(name, inspectable?.index ?? -1, ZERO_SPAN) !== undefined,
      ...(this.handleWitness ? { handleType: this.handleWitness.type } : {}),
      ...(this.anyTypeInspectable ? { anyType: true } : {}),
    };
  }

  /**
   * The Inspectable dictionary of type parameter `name`: its own `Inspectable`
   * bound, or one whose trait extends it, as `E < Error` does.
   */
  protected inspectableBound(
    name: string,
    traitIndex: number,
    span: SourceSpan,
  ): HirExpression | undefined {
    const bounds = this.signature.genericBounds;
    const type = `trait:${INSPECTABLE}`;
    const direct = bounds.findIndex(
      (bound) => bound.parameter === name && bound.traitIndex === traitIndex,
    );
    if (direct >= 0)
      return { kind: "trait-bound-dictionary", traitIndex, boundIndex: direct, type, span };
    for (const [boundIndex, bound] of bounds.entries()) {
      const trait = bound.parameter === name ? this.traitTypes.get(bound.traitName) : undefined;
      const path = trait && this.findSupertraitPath(trait, bound.traitArguments, traitIndex, []);
      if (trait && path)
        return {
          kind: "trait-bound-dictionary",
          traitIndex,
          boundIndex,
          supertrait: { sourceTraitIndex: trait.index, path },
          type,
          span,
        };
    }
    return undefined;
  }

  // The compiler-supplied `Any` and `Inspectable`, which have no source `impl`.
  protected builtinTraitDictionaryPlan(
    traitIndex: number,
    targetType: ValueType,
    traitArguments: readonly ValueType[],
    span: SourceSpan,
  ): HirTraitDictionaryPlan | undefined {
    const type = readonlyType(targetType);
    const trait = [...this.traitTypes.values()].find((candidate) => candidate.index === traitIndex);
    const traitName = trait?.name;
    if (traitArguments.length > 0) return undefined;
    const plan = (
      builtin: HirBuiltinTraitImplementation,
      bounds: readonly HirExpression[] = [],
    ): HirTraitDictionaryPlan => ({ bounds, implementationIndex: -1, supertraits: [], builtin });
    // Every value type implements `Any`, `void` included (04-type-system.md#trait-values-and-any).
    if (traitName === "Any" && type !== "never")
      return plan({ kind: "marker", traitIndex, targetType: type });
    const inspectable = trait !== undefined && this.isStandardInspectable(trait);
    if (genericTypeName(type) && !(inspectable && this.anyTypeInspectable)) return undefined;
    if (inspectable) {
      const parts = inspectKey(type, this.inspectEnvironment());
      const dictionary = parts
        ? inspectableBuiltin(parts, traitIndex, targetType, (generic) =>
            generic === HANDLE_TYPE
              ? this.handleWitness!.dictionary
              : this.inspectableBound(generic, traitIndex, span)!,
          )
        : undefined;
      return dictionary && plan(dictionary.builtin, dictionary.bounds);
    }
    return undefined;
  }

  protected displayValue(
    value: HirExpression,
    span: SourceSpan,
    origin = "string interpolation",
  ): HirExpression {
    const type = readonlyType(value.type);
    if (type === "string") return value;
    const missing = `type '${displayType(value.type)}' does not implement Display, required by ${origin}`;
    // A program that mentions no `Display` has none declared (spec/lang/10-modules.md#prelude).
    const trait = this.traitTypes.get("Display");
    if (!trait) return this.fail("unsatisfied-trait-bound", missing, span);
    const generic = genericTypeName(type);
    // A bound on `Display`, or one whose trait extends it, as `T < Num` does
    // (09-traits.md#r-trait.num.num-ordered).
    for (const [boundIndex, bound] of this.signature.genericBounds.entries()) {
      const boundTrait = generic === bound.parameter && this.traitTypes.get(bound.traitName);
      const path = !boundTrait
        ? undefined
        : boundTrait.index === trait.index
          ? []
          : this.findSupertraitPath(boundTrait, bound.traitArguments, trait.index, []);
      if (!boundTrait || !path) continue;
      const receiver: HirExpression = {
        kind: "trait-bound",
        value,
        traitIndex: boundTrait.index,
        boundIndex,
        type: `trait:${bound.traitName}`,
        span,
      };
      return {
        kind: "trait-call",
        receiver,
        traitIndex: trait.index,
        methodIndex: 0,
        ...(path.length > 0 ? { supertraitPath: path } : {}),
        arguments: [],
        providers: [],
        type: "string",
        span,
      };
    }
    if (traitTypeName(type) === trait.name) {
      return {
        kind: "trait-call",
        receiver: value,
        traitIndex: trait.index,
        methodIndex: 0,
        arguments: [],
        providers: [],
        type: "string",
        span,
      };
    }
    // An implementation, generic ones too, such as lib/std's tuple `Display`,
    // with its bounds' dictionaries (05-expressions.md#r-expr.interp.std.tuple.template).
    const dispatch = this.traitMethodDispatch(type, "Display", span);
    const signature =
      dispatch?.kind === "function"
        ? [...this.signatures.values()].find(({ index }) => index === dispatch.functionIndex)
        : undefined;
    if (dispatch?.kind === "function" && signature) {
      return {
        kind: "call",
        functionIndex: signature.index,
        functionName: signature.name,
        arguments: [this.coerce(value, type, span)],
        providers: [],
        bounds: dispatch.bounds,
        erasedParameterTypes: signature.parameters,
        erasedResultType: signature.result,
        type: "string",
        span,
      };
    }
    return this.fail("unsatisfied-trait-bound", missing, span);
  }

  /**
   * How a map with key type `keyType` compares its keys, once the key type
   * meets `Map[K < Eq & Hash, V]` (04-type-system.md#map-key-types): a type
   * parameter key, kind 3, through its `Eq` bound's dictionary.
   */
  /**
   * The written-bound scope over the signature's bounds, through the same
   * parameter lookup calls use.
   */
  private boundScope() {
    return signatureBoundScope(
      this.dataTypes,
      this.enumTypes,
      this.traitTypes,
      this.signature.genericParameters,
      (parameter, traitIndex) => this.parameterBound(parameter, traitIndex, []) !== undefined,
    );
  }

  protected mapKey(
    keyType: ValueType,
    span: SourceSpan,
  ): {
    readonly keyKind: 0 | 1 | 2 | 3;
    readonly keyDispatch?: HirEqualityDispatch;
    readonly keyDictionary?: HirExpression;
  } {
    const generic = genericTypeName(keyType);
    const problem = writtenApplicationBoundProblem("Map", [keyType], this.boundScope());
    if (problem) this.fail(problem.code, problem.message, span);
    if (generic) return { keyKind: 3, keyDispatch: this.equalityDispatch(keyType) };
    // A key type whose `Eq` is a generic implementation, as a tuple's
    // template instance is, compares through its dictionary, as kind 3.
    const keyKind = mapKeyKind(keyType);
    const eq = keyKind === 2 ? this.traitTypes.get("Eq") : undefined;
    const found = eq && findImpl(this.implementations, eq.index, readonlyType(keyType), []);
    if (!found || found.impl.targetType === found.type) return { keyKind };
    return {
      keyKind: 3,
      keyDictionary: {
        kind: "trait-dictionary",
        traitIndex: eq.index,
        dictionary: this.traitDictionaryPlan(found.impl, found.type, [], span),
        type: "trait:Eq",
        span,
      },
    };
  }

  protected equalityDispatch(type: ValueType): HirEqualityDispatch | undefined {
    return this.traitMethodDispatch(type, "Eq");
  }

  /** With a `span`, an implementation whose bounds `type` does not meet is an error there. */
  protected equalityStrategy(type: ValueType, span?: SourceSpan): HirEqualityStrategy | undefined {
    const comparedType = readonlyType(type);
    if (numericType(comparedType) || ["bool", "char", "string"].includes(comparedType))
      return { kind: "builtin" };
    const dispatch = this.traitMethodDispatch(comparedType, "Eq", span);
    return dispatch ? { kind: "dispatch", dispatch } : undefined;
  }

  protected traitMethodDispatch(
    type: ValueType,
    traitName: string,
    span?: SourceSpan,
  ): HirEqualityDispatch | undefined {
    const comparedType = readonlyType(type);
    const trait = this.traitTypes.get(traitName);
    if (!trait) return undefined;
    const generic = genericTypeName(comparedType);
    const bound = generic && this.parameterBound(generic, trait.index, []);
    if (bound) {
      const { boundIndex, supertrait } = bound;
      const via = supertrait && { traitIndex: supertrait.sourceTraitIndex, path: supertrait.path };
      return { kind: "bound", traitIndex: trait.index, methodIndex: 0, boundIndex, via };
    }
    // An implementation, generic ones too, such as std's `impl[T < Eq] Eq for
    // List[T]` (05-expressions.md#r-expr.eq.std). The call passes its bounds'
    // dictionaries. Without a use `span`, an unmet bound means no implementation.
    const found = findImpl(this.implementations, trait.index, comparedType, []);
    const mapping = found?.impl.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
    if (!found || !mapping) return undefined;
    const substitutions = matchTraitImplementation(found.impl, trait.index, found.type, [])!;
    // spec/lang/09-traits.md#r-trait.derive.bound-unmet
    const code = isDerivedImplementation(this.traitTypes, found.impl.span)
      ? "missing-derived-bound"
      : "unsatisfied-trait-bound";
    const diagnosticCount = this.diagnostics.length;
    try {
      const bounds = found.impl.genericBounds.map((bound) =>
        this.boundDictionaryExpression(bound, substitutions, span ?? ZERO_SPAN, new Set(), code),
      );
      return { kind: "function", functionIndex: mapping.functionIndex, bounds };
    } catch (error) {
      if (!(error instanceof CheckFailure) || span) throw error;
      this.diagnostics.length = diagnosticCount;
      return undefined;
    }
  }

  protected orderingStrategy(type: ValueType, span?: SourceSpan): HirOrderingStrategy | undefined {
    const comparedType = readonlyType(type);
    if (numericType(comparedType) || ["char", "string"].includes(comparedType))
      return { kind: "builtin" };
    const dispatch = this.traitMethodDispatch(comparedType, "PartialOrd", span);
    return dispatch ? { kind: "dispatch", dispatch } : undefined;
  }

  protected equalityExpression(
    left: HirExpression,
    right: HirExpression,
    span: SourceSpan,
  ): HirExpression | undefined {
    const strategy = this.equalityStrategy(left.type, span);
    if (!strategy) return undefined;
    return {
      kind: "value-equality",
      left,
      right,
      valueType: left.type,
      strategy,
      type: "bool",
      span,
    };
  }

  protected blockType(statements: readonly HirStatement[]): ValueType {
    const last = statements.at(-1);
    // A suite that ends by leaving it (`return`, `break`, `continue`) has type
    // `never`, so it joins any other branch type (spec/lang/06-control-flow.md).
    if (last?.kind === "return" || last?.kind === "break" || last?.kind === "continue")
      return "never";
    return last?.kind === "expression" ? last.expression.type : "void";
  }

  protected recordInferredReturn(type: ValueType, span?: SourceSpan, value?: HirExpression): void {
    this.inferredReturns.push({ type, span: span ?? this.declaration.span, value });
  }

  protected checkFallthrough(body: readonly HirStatement[]): void {
    if (this.signature.result === "void" || this.declaration.bodiless) return;
    const last = body.at(-1);
    if (last?.kind === "return") return;
    const actual = last?.kind === "expression" ? last.expression.type : "void";
    if (actual === "void")
      this.fail(
        "missing-return-value",
        `function '${this.signature.name}' may complete without an ${this.signature.result} value`,
        last?.span ?? this.declaration.span,
      );
    this.requireAssignable(actual, this.signature.result, last?.span ?? this.declaration.span);
  }

  protected resolveType(type: TypeRef): ValueType {
    const resolved = resolveGenericType(
      type.name,
      new Set(this.signature.genericParameters),
      new Set(this.signature.rowParameters),
    );
    const kinded = normalizeRowArguments(
      resolveTraitType(resolved, this.traitTypes),
      this.dataTypes,
      new Set(this.signature.rowParameters),
    );
    if (typeof kinded !== "string") this.fail("generic-kind-mismatch", kinded.mismatch, type.span);
    const problem = writtenTypeProblem(kinded, this.signature.genericBounds, this.traitTypes);
    if (problem) this.fail(problem.code, problem.message, type.span);
    const declared = normalizeBoundProjections(kinded, this.signature.genericBounds);
    const requirementDiagnostic = requirementKeyDiagnosticsInType(
      declared,
      this.traitTypes,
      type.span,
      (argument) =>
        isKnownType(
          resolveTraitType(argument, this.traitTypes),
          this.dataTypes,
          this.enumTypes,
          this.traitTypes,
        ),
    )[0];
    if (requirementDiagnostic)
      this.fail(requirementDiagnostic.code, requirementDiagnostic.message, type.span);
    const dynamicProblem = dynamicTraitProblemInType(declared, this.traitTypes);
    if (dynamicProblem) this.fail(dynamicProblem.code, dynamicProblem.message, type.span);
    if (!isKnownType(declared, this.dataTypes, this.enumTypes, this.traitTypes))
      this.fail(
        "unknown-type",
        `unknown or unsupported type '${displayType(type.name)}'${standardImportHint(type.name, "type")}`,
        type.span,
      );
    // A written application meets its declaration's bounds, nested ones
    // included (trait.bound.no-implied).
    const boundProblem = writtenBoundProblem(declared, this.boundScope());
    if (boundProblem) this.fail(boundProblem.code, boundProblem.message, type.span);
    return declared;
  }

  protected currentScope(): Map<string, HirLocal> {
    return this.scopes.at(-1)!;
  }

  protected resolveLocal(name: string): HirLocal | undefined {
    for (let index = this.scopes.length - 1; index >= 0; index -= 1) {
      const local = this.scopes[index]!.get(name);
      if (local) return local;
    }
    return undefined;
  }

  protected resolveGlobal(name: string): HirGlobal | undefined {
    const global = this.globals.get(name);
    if (!global) return undefined;
    if (this.moduleBody || global.span.start.offset < this.declaration.span.start.offset)
      return global;
    return undefined;
  }

  protected visibleCaptureSources(): ReadonlyMap<string, HirLocal> {
    return captureSources(this.availableCaptures, this.scopes, this.providerScopes);
  }

  protected visibleProviders(): ReadonlyMap<string, HirLocal> {
    const visible = new Map<string, HirLocal>();
    for (const scope of this.providerScopes) {
      for (const [key, local] of scope) visible.set(key, local);
    }
    return visible;
  }

  protected referenceLocal(local: HirLocal, span: SourceSpan): HirExpression {
    this.readLocals.add(local);
    if (this.locals.includes(local)) return { kind: "local", local, type: local.type, span };
    if (this.insideClosure && isCaptureSource(this.availableCaptures, local))
      return this.captureReference(local.name, local, span);
    return { kind: "local", local, type: local.type, span };
  }

  // A read of a captured binding inside a closure, with the access the binding
  // has in its enclosing scope (07-functions.md#r-fn.capture.access).
  protected captureReference(name: string, source: HirLocal, span: SourceSpan): HirExpression {
    const fieldIndex = this.captureField(name, source);
    return captureOf(source, {
      kind: "capture",
      closureIndex: this.closureIndex,
      fieldIndex,
      type: source.type,
      span,
    });
  }

  /** The environment field of a captured binding, registering it on first use. */
  protected captureField(name: string, source: HirLocal): number {
    let capture = this.captures.get(name);
    if (!capture) {
      capture = { source, fieldIndex: this.captures.size };
      this.captures.set(name, capture);
    }
    return capture.fieldIndex;
  }

  protected captureValue(source: HirLocal, span: SourceSpan): HirExpression {
    if (this.locals.includes(source)) {
      this.readLocals.add(source);
      return { kind: "local", local: source, type: source.type, span };
    }
    if (this.insideClosure && this.availableCaptures.get(source.name) === source) {
      let capture = this.captures.get(source.name);
      if (!capture) {
        capture = { source, fieldIndex: this.captures.size };
        this.captures.set(source.name, capture);
      }
      return captureOf(source, {
        kind: "capture",
        closureIndex: this.closureIndex,
        fieldIndex: capture.fieldIndex,
        type: source.type,
        span,
      });
    }
    throw new Error(`cannot materialize closure capture '${source.name}'`);
  }
  protected requireAssignable(actual: ValueType, expected: ValueType, span: SourceSpan): void {
    if (actual === "never" || actual === expected || isPermissionWeakening(actual, expected))
      return;
    const found = displayType(actual);
    const wanted = displayType(expected);
    const upgrade = `readonly type '${found}' cannot be upgraded to '${wanted}'`;
    if (mutableInner(expected) === actual) this.fail("mutable-upgrade", upgrade, span);
    const cast = `'${found}' does not convert implicitly to '${wanted}'; write an explicit cast`;
    if (narrowsTo(actual, expected)) this.fail("implicit-narrowing", cast, span);
    const widen = `'${found}' does not widen implicitly to '${wanted}'; write ${wanted}(...)`;
    if (widensTo(actual, expected)) this.failWithConversion(widen, expected, span);
    // Two types of one family and width, as `u32` and `usize` (04-type-system.md#r-types.num.same-width).
    const convert = `'${found}' does not convert implicitly to '${wanted}'; write ${wanted}(...)`;
    if (sameWidthNumeric(actual, expected)) this.failWithConversion(convert, expected, span);
    this.fail("type-mismatch", mismatchMessage(actual, expected), span);
  }

  /** `type-mismatch` for a narrower number, with a fix-it writing the conversion (04-type-system.md#r-types.num.no-implicit.fix). */
  protected failWithConversion(message: string, target: ValueType, span: SourceSpan): never {
    const shownTarget = displayType(target);
    let code = "type-mismatch";
    ({ code, message } = rowDiagnostic(code, message, this.declaration));
    ({ code, message } = derivedFieldDiagnostic(this.traitTypes, code, message, span));
    if (code !== "type-mismatch") this.fail(code, message, span);
    this.diagnostics.push({
      code,
      message,
      span,
      fix: {
        message: `write '${shownTarget}(...)'`,
        edits: [
          { span: { start: span.start, end: span.start }, replacement: `${shownTarget}(` },
          { span: { start: span.end, end: span.end }, replacement: ")" },
        ],
      },
    });
    throw new CheckFailure(message);
  }

  protected requireCoercion(
    expression: HirExpression,
    expected: ValueType,
    span: SourceSpan,
  ): HirExpression {
    const coerced = this.coerce(expression, expected, span);
    this.requireAssignable(coerced.type, expected, span);
    return coerced;
  }

  protected requireType(actual: ValueType, expected: ValueType, span: SourceSpan): void {
    this.requireAssignable(actual, expected, span);
  }

  /** Fails for a use of `type` that leaves `unresolved` unsolved (checker/cannot-infer.ts). */
  protected failUnresolvedType(
    unresolved: readonly string[],
    type: ValueType,
    span: SourceSpan,
    solved: ReadonlyMap<string, ValueType> = new Map(),
  ): never {
    const binding = this.inferredBinding;
    this.fail(...unresolvedTypeFailure(unresolved, type, span, binding, solved), span);
  }

  /** Fails for a call of `callee` that leaves `unresolved` unsolved (checker/cannot-infer.ts). */
  protected failUnresolvedCall(
    unresolved: readonly string[],
    callee: string,
    span: SourceSpan,
    solved: ReadonlyMap<string, ValueType> = new Map(),
  ): never {
    this.fail(...unresolvedCallFailure(unresolved, callee, solved), span);
  }

  /** Fails with `code`; a `fix` stays only while no rewrite changes the code. */
  protected fail(code: string, message: string, span: SourceSpan, fix?: DiagnosticFix): never {
    const written = code;
    ({ code, message } = rowDiagnostic(code, message, this.declaration));
    ({ code, message } = derivedFieldDiagnostic(this.traitTypes, code, message, span));
    this.diagnostics.push({ code, message, span, ...(fix && code === written ? { fix } : {}) });
    throw new CheckFailure(message);
  }

  // The test-case functions are used only as direct calls in test position, so
  // any other use of their names is misplaced (spec/lang/10-modules.md#r-module.testing.direct-call).
  // An item of a `tests:` block is visible only inside the block
  // (spec/lang/03-names-and-scopes.md#r-names.tests.inside-only).
  protected visibleSignature(name: string): Signature | undefined {
    const signature = this.signatures.get(name);
    return signature?.testOnly && !this.declaration.testOnly ? undefined : signature;
  }
}

// The test registration functions under an import of any name, `it`
// included (spec/lang/10-modules.md#r-module.testing.reg.identity).
export const TEST_CASE_FUNCTIONS: ReadonlySet<string> = new Set([
  "std.testing.it",
  "std.testing.it_each",
  "std.testing.it_prop",
  "std.testing.it_prop_with",
]);
