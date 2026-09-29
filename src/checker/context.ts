import { PRELUDE_NAMES } from "./prelude-names.ts";
import type { Expression, FunctionDecl, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
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
  HirProgram,
  HirStatement,
  HirTrait,
  HirBuiltinTraitImplementation,
  HirTraitDictionaryPlan,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import {
  compactKey,
  inspectKey,
  usesStandardInspect,
  type InspectEnvironment,
} from "./inspectable.ts";
import {
  collectionIterablePlan,
  isPermissionWeakening,
  rowUnionType,
  weakenBoundedGenericActual,
} from "./assignability.ts";
import { isRowSubsumption, mismatchMessage, rowDiagnostic } from "./row-rules.ts";
import { INSPECTABLE } from "./standard-traits.ts";
import * as termination from "./termination.ts";
import { builtinDebug, implementsDebug } from "./debug.ts";
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
  builtinTotallyOrdered,
  MAX_BOUND_DEPTH,
  mapKeyKind,
  numericWidening,
} from "./shared.ts";
import { generalizedShape } from "./shapes.ts";
import { findSupertraitPath, resolveTraitPath } from "./trait-paths.ts";
import {
  contextKeys,
  functionParts,
  iteratorCursorSource,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  resultParts,
  rowArgumentKeys,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import { narrowsTo, NUMERIC_TYPES, numericType } from "../numeric.ts";
import { derivedFieldDiagnostic } from "./derive-intrinsics.ts";

export interface CheckResult {
  readonly program?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface Signature {
  readonly name: string;
  readonly index: number;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly HirGenericBound[];
  readonly referenceParameters?: readonly string[];
  readonly valueParameters?: readonly string[];
  readonly rowParameters: readonly string[];
  /**
   * Every generic parameter in declared order, type and row alike, when a
   * row parameter is among them: the slots of an explicit type-argument list
   * (07-functions.md#r-fn.generic.explicit.complete).
   */
  readonly typeArgumentOrder?: readonly string[];
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly defaultFunctionNames: readonly (string | undefined)[];
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  /** Declared in a `tests:` block (spec/03-names-and-scopes.md#tests-blocks). */
  readonly testOnly?: boolean;
  /** A suffix function, marked `@num_suffix` (spec/05-expressions.md#r-expr.suffix.marker). */
  readonly numSuffix?: boolean;
  /** A prefix function, marked `@str_prefix` (spec/05-expressions.md#r-expr.prefix.marker). */
  readonly strPrefix?: { readonly templateParameter: boolean };
  readonly span: SourceSpan;
}

export interface InherentMethod {
  readonly targetType: ValueType;
  /** The implementation's generic parameters, which `targetType` may name. */
  readonly targetGenericParameters?: readonly string[];
  readonly name: string;
  /** `pub fn`: only a public inherent method of a part is promoted (spec 03). */
  readonly public: boolean;
  readonly associated: boolean;
  readonly receiverMutable: boolean;
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly variadic: boolean;
  readonly suspending: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly functionName: string;
  readonly span: SourceSpan;
}

export interface PlannedArgument {
  readonly parameterIndex: number;
  readonly argumentIndices: readonly number[];
  readonly kind: "single" | "vararg-elements";
}

export interface ResolvedTraitPath {
  readonly arguments: readonly ValueType[];
  readonly trait: HirTrait;
}

export interface FunctionCheckResult {
  readonly function?: HirFunction;
  readonly diagnostics: readonly Diagnostic[];
}

export class CheckFailure extends Error {}

function hirValueReadsLocal(value: unknown, local: HirLocal): boolean {
  if (Array.isArray(value)) return value.some((item) => hirValueReadsLocal(item, local));
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "local" && node.local === local) return true;
  return Object.entries(node).some(
    ([key, child]) =>
      key !== "span" && key !== "local" && key !== "bindings" && hirValueReadsLocal(child, local),
  );
}

const TYPE_NAMES = new Set<ValueType>([
  ...NUMERIC_TYPES.keys(),
  "bool",
  "char",
  "string",
  "void",
  "ConsoleError",
]);

export { PRELUDE_NAMES };

export { isPermissionWeakening, weakenBoundedGenericActual };

export { mapKeyKind };

export function isKnownType(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait> = new Map(),
): boolean {
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutable !== "void" && isKnownType(mutable, dataTypes, enumTypes, traitTypes);
  if (genericTypeName(type) || rowArgumentKeys(type)) return true;
  if (TYPE_NAMES.has(type)) return true;
  const plainData = dataTypes.get(type);
  if (plainData) return plainData.genericParameters.length === 0;
  const plainEnum = enumTypes.get(type);
  if (plainEnum) return plainEnum.genericParameters.length === 0;
  if (type.startsWith("trait:")) {
    const key = type.slice("trait:".length);
    const nominalTrait = nominalGenericParts(key);
    const trait = traitTypes.get(nominalTrait?.name ?? key);
    if (!trait) return false;
    if (!nominalTrait) return trait.genericParameters.length === 0;
    return (
      trait.genericParameters.length === nominalTrait.arguments.length &&
      nominalTrait.arguments.every((argument) =>
        isKnownType(argument, dataTypes, enumTypes, traitTypes),
      )
    );
  }
  if (type.startsWith("provider:")) return true;
  if (contextKeys(type)) return true;
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tuple.every(
      (element) => element !== "void" && isKnownType(element, dataTypes, enumTypes, traitTypes),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return optional !== "void" && isKnownType(optional, dataTypes, enumTypes, traitTypes);
  const result = resultParts(type);
  if (result)
    return (
      isKnownType(result.ok, dataTypes, enumTypes, traitTypes) &&
      result.error !== "void" &&
      isKnownType(result.error, dataTypes, enumTypes, traitTypes)
    );
  const nominal = nominalGenericParts(type);
  if (nominal) {
    if (nominal.name === "Suspend") {
      return (
        nominal.arguments.length === 1 &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "Iterator") {
      return (
        nominal.arguments.length === 1 &&
        nominal.arguments[0] !== "void" &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "List") {
      return (
        nominal.arguments.length === 1 &&
        nominal.arguments[0] !== "void" &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "Map") {
      return (
        nominal.arguments.length === 2 &&
        (mapKeyKind(nominal.arguments[0]!) !== undefined ||
          genericTypeName(nominal.arguments[0]!) !== undefined) &&
        nominal.arguments[1] !== "void" &&
        isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes)
      );
    }
    const declaration = dataTypes.get(nominal.name);
    if (declaration) {
      return (
        declaration.genericParameters.length === nominal.arguments.length &&
        nominal.arguments.every((argument) =>
          isKnownType(argument, dataTypes, enumTypes, traitTypes),
        )
      );
    }
    const enumDeclaration = enumTypes.get(nominal.name);
    return Boolean(
      enumDeclaration &&
      enumDeclaration.genericParameters.length === nominal.arguments.length &&
      nominal.arguments.every((argument) =>
        isKnownType(argument, dataTypes, enumTypes, traitTypes),
      ),
    );
  }
  const callable = functionParts(type);
  return Boolean(
    callable &&
    callable.parameters.every(
      (parameter) =>
        parameter !== "void" && isKnownType(parameter, dataTypes, enumTypes, traitTypes),
    ) &&
    isKnownType(callable.result, dataTypes, enumTypes, traitTypes),
  );
}

export abstract class CheckerContext {
  protected abstract checkStatement(
    statement: Statement,
    expected?: ValueType,
    valueContext?: boolean,
  ): HirStatement;
  protected abstract checkTupleBinding(
    statement: Extract<Statement, { kind: "tuple-binding" }>,
  ): HirStatement[];
  protected abstract checkCompoundAssignment(
    statement: Extract<Statement, { kind: "assignment" | "field-assignment" | "index-assignment" }>,
  ): HirStatement[];
  protected abstract checkExpression(expression: Expression, expected?: ValueType): HirExpression;
  protected abstract isIdentityType(type: ValueType): boolean;

  protected readonly declaration: FunctionDecl;
  protected readonly signature: Signature;
  protected readonly signatures: ReadonlyMap<string, Signature>;
  protected readonly dataTypes: ReadonlyMap<string, HirData>;
  protected readonly enumTypes: ReadonlyMap<string, HirEnum>;
  protected readonly traitTypes: ReadonlyMap<string, HirTrait>;
  protected readonly implementations: readonly HirTraitImplementation[];
  protected readonly inherentMethods: readonly InherentMethod[];
  protected readonly synthetic: boolean;
  protected readonly moduleBody: boolean;
  protected readonly closures: HirFunction[];
  protected readonly insideClosure: boolean;
  protected readonly availableCaptures: ReadonlyMap<string, HirLocal>;
  protected readonly availableProviders: ReadonlyMap<string, HirLocal>;
  protected readonly inferRequirements: boolean;
  protected readonly inferResult: boolean;
  protected readonly selfClosureLocal?: HirLocal;
  protected readonly imports: ReadonlyMap<string, string>;
  protected readonly globals: Map<string, HirGlobal>;
  protected pendingRecursiveClosure?: HirLocal;
  protected readonly inferredRequirements: string[] = [];
  protected inferredReturnType?: ValueType;
  protected readonly inferredPropagations: termination.InferredPropagation[] = [];
  protected readonly closureIndex: number;
  protected readonly captures = new Map<string, HirCapture>();
  protected readonly providerScopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly diagnostics: Diagnostic[] = [];
  protected readonly scopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly locals: HirLocal[] = [];
  protected readonly loopResults: Array<ValueType | undefined> = [];
  protected readonly unavailableBindingLocals = new Set<number>();
  protected readonly allowedConditionalBindingLocals = new Set<number>();
  protected deferDepth = 0;
  /**
   * The unsolved generic parameters of the call whose argument is being
   * checked as a generic function value; positions of the expected type that
   * mention them do not instantiate the value.
   */
  protected pendingCallGenerics?: ReadonlySet<string>;

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
  ) {
    this.declaration = declaration;
    this.signature = signature;
    this.signatures = signatures;
    this.dataTypes = dataTypes;
    this.enumTypes = enumTypes;
    this.traitTypes = traitTypes;
    this.implementations = implementations;
    this.inherentMethods = inherentMethods;
    this.synthetic = synthetic;
    this.moduleBody = moduleBody;
    this.closures = closures;
    this.insideClosure = insideClosure;
    this.availableCaptures = availableCaptures;
    this.availableProviders = availableProviders;
    this.inferRequirements = inferRequirements;
    this.inferResult = inferResult;
    this.selfClosureLocal = selfClosureLocal;
    this.imports = imports;
    this.globals = globals;
    this.closureIndex = closureIndex;
    this.providerScopes[0] = new Map(availableProviders);
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
        // `self` of an `impl ... for void` method, such as std.process's
        // `Termination` implementation (10-modules.md#exit-status), is void.
        if (type === "void" && !(parameter.name === "self" && index === 0))
          this.fail("void-parameter", "a parameter cannot have type void", parameter.span);
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
          this.recordInferredReturn(this.blockType(body), last?.span ?? this.declaration.span);
        result = this.inferredReturnType ?? "void";
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
        if (local.parameter || local.name.startsWith("$") || hirValueReadsLocal(body, local))
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
      };
    } catch (error) {
      if (!(error instanceof CheckFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
  }

  protected checkStatements(
    statements: readonly Statement[],
    scoped = false,
    expectedFinal?: ValueType,
    finalValueContext = false,
  ): HirStatement[] {
    if (scoped) this.scopes.push(new Map());
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
        if (statement.kind === "tuple-binding") {
          checked.push(...this.checkTupleBinding(statement));
          continue;
        }
        if (
          (statement.kind === "assignment" ||
            statement.kind === "field-assignment" ||
            statement.kind === "index-assignment") &&
          statement.compound
        ) {
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
      if (scoped) this.scopes.pop();
    }
  }

  protected coerce(
    value: HirExpression,
    expected: ValueType | undefined,
    span: SourceSpan,
    wrapOptional = true,
  ): HirExpression {
    if (!expected || value.type === expected || value.type === "never") return value;
    const widened = numericWidening(value, readonlyType(expected), span);
    if (widened) return widened;
    // Row subsumption adapts a function value like a weakening (r-req.row.subsume).
    if (isPermissionWeakening(value.type, expected) || isRowSubsumption(value.type, expected)) {
      return { kind: "permission-weaken", operand: value, type: expected, span };
    }
    const shape = generalizedShape(value, expected, this.dataTypes, span);
    if (shape) return shape;
    const variance = varianceConversion(value.type, expected, {
      data: this.dataTypes,
      enums: this.enumTypes,
    });
    if (variance === "representation-change")
      this.fail(
        "variance-representation-change",
        `'${value.type}' cannot become '${expected}': a variance conversion must not change a value's representation`,
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
    // An implementation converts to the `Iterator[T]` cursor as a dynamic value.
    const cursorSource = iteratorCursorSource(value.type, expected);
    const converted = cursorSource ? this.coerce(value, cursorSource, span, false) : undefined;
    if (converted?.kind === "trait-wrap") return { ...converted, type: expected };
    const traitName = traitTypeName(expected);
    const trait = traitName && this.traitTypes.get(traitName);
    if (trait) {
      const mutableTrait = mutableInner(expected) !== undefined;
      const inspectTarget = this.isStandardInspectable(trait);
      if (mutableTrait && mutableInner(value.type) === undefined) {
        if (inspectTarget && inspectKey(value.type, this.inspectEnvironment()))
          this.fail(
            "mutable-upgrade",
            `readonly type '${value.type}' cannot be erased to mut Inspectable`,
            span,
          );
        return value;
      }
      const expectedTraitKey = readonlyType(expected).slice("trait:".length);
      const expectedTraitArguments = nominalGenericParts(expectedTraitKey)?.arguments ?? [];
      const sourceTraitName = traitTypeName(value.type);
      const sourceTrait = sourceTraitName && this.traitTypes.get(sourceTraitName);
      const sourceTraitKey = sourceTrait
        ? readonlyType(value.type).slice("trait:".length)
        : undefined;
      const sourceTraitArguments = sourceTraitKey
        ? (nominalGenericParts(sourceTraitKey)?.arguments ?? [])
        : [];
      const supertraitPath = sourceTrait
        ? this.findSupertraitPath(
            sourceTrait,
            sourceTraitArguments,
            trait.index,
            expectedTraitArguments,
          )
        : undefined;
      if (sourceTrait && supertraitPath) {
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
      const erasedParameter = inspectTarget ? genericTypeName(implementationType) : undefined;
      if (erasedParameter) {
        // Erasing `x: T` needs `T < Inspectable`; the bound's dictionary
        // records the instantiated type (spec/09-traits.md#erasure-to-inspectable).
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
      if (implementation) {
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
        "recursive-trait-dictionary",
        `constructing the trait dictionary for '${targetType}' requires itself`,
        span,
      );
    // This plan proves a bound of depth `seen.size + 1` (09-traits.md#r-trait.bound.depth).
    if (seen.size >= MAX_BOUND_DEPTH)
      this.fail(
        "trait-resolution-depth",
        `proving the bound for '${targetType}' needs a bound deeper than ${MAX_BOUND_DEPTH}`,
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
    this.inferBoundAssociatedTypes(implementation, substitutions);
    const next = new Set([...seen, key]);
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
      const parent = this.implementations[parentIndex]!;
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
  ): HirExpression {
    const actual = substitutions.get(bound.parameter);
    if (!actual)
      this.fail(
        "unresolved-generic-placeholder",
        `could not infer implementation parameter ${bound.parameter}`,
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
      const boundIndex = this.signature.genericBounds.findIndex(
        (candidate) =>
          candidate.parameter === forwarded &&
          candidate.traitIndex === bound.traitIndex &&
          candidate.traitArguments.length === traitArguments.length &&
          candidate.traitArguments.every((argument, index) => argument === traitArguments[index]),
      );
      if (boundIndex < 0)
        this.fail(
          "unsatisfied-trait-bound",
          `generic parameter '${forwarded}' does not implement ${bound.traitName}, required by the implementation bound on '${bound.parameter}'`,
          span,
        );
      return {
        kind: "trait-bound-dictionary",
        traitIndex: bound.traitIndex,
        boundIndex,
        type: `trait:${traitKey}`,
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
        "unsatisfied-trait-bound",
        `type '${actual}' does not implement ${bound.traitName}, required by the implementation bound on '${bound.parameter}'`,
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

  /** The standard `Inspectable`, declared by a `std.inspect` or `std.error` import. */
  protected isStandardInspectable(trait: HirTrait): boolean {
    return trait.name === INSPECTABLE && usesStandardInspect(this.imports);
  }

  protected inspectEnvironment(): InspectEnvironment {
    const inspectable = this.traitTypes.get(INSPECTABLE);
    return {
      // A type declared in a block suite is not inspectable (09-traits.md#inspectable-types).
      nominal: (name) =>
        [this.dataTypes.get(name), this.enumTypes.get(name)].some(
          (declaration) => declaration !== undefined && !declaration.local,
        ),
      inspectableParameter: (name) =>
        this.signature.genericBounds.some(
          (bound) => bound.parameter === name && bound.traitIndex === inspectable?.index,
        ),
    };
  }

  // The standard library implements Display for the printable primitives and
  // string, Eq for primitives and equality-comparable built-in
  // composites, and PartialOrd for ordered ones (05-expressions.md). These
  // have no source `impl`, so the dictionary is built from the same
  // strategies the operators use.
  protected builtinTraitDictionaryPlan(
    traitIndex: number,
    targetType: ValueType,
    traitArguments: readonly ValueType[],
    span: SourceSpan,
  ): HirTraitDictionaryPlan | undefined {
    const type = readonlyType(targetType);
    const traitName = [...this.traitTypes.values()].find(
      (candidate) => candidate.index === traitIndex,
    )?.name;
    if (traitName === "Iterable") return collectionIterablePlan(traitIndex, type, traitArguments);
    if (traitArguments.length > 0) return undefined;
    const plan = (
      builtin: HirBuiltinTraitImplementation,
      bounds: readonly HirExpression[] = [],
    ): HirTraitDictionaryPlan => ({ bounds, implementationIndex: -1, supertraits: [], builtin });
    // Every value type implements `Any` (04-type-system.md#trait-values-and-any).
    if (traitName === "Any" && type !== "void" && type !== "never")
      return plan({ kind: "marker", traitIndex, targetType: type });
    if (genericTypeName(type)) return undefined;
    const trait = [...this.traitTypes.values()].find((candidate) => candidate.index === traitIndex);
    if (trait && this.isStandardInspectable(trait)) {
      const parts = inspectKey(type, this.inspectEnvironment());
      if (!parts) return undefined;
      const bounds: HirExpression[] = [];
      const positions = new Map<string, number>();
      const key = compactKey(parts).map((part) => {
        if (typeof part === "string") return part;
        let position = positions.get(part.generic);
        if (position === undefined) {
          position = bounds.length;
          positions.set(part.generic, position);
          bounds.push({
            kind: "trait-bound-dictionary",
            traitIndex,
            boundIndex: this.signature.genericBounds.findIndex(
              (bound) => bound.parameter === part.generic && bound.traitIndex === traitIndex,
            ),
            type: `trait:${INSPECTABLE}`,
            span,
          });
        }
        return { bound: position };
      });
      return plan(
        {
          kind: "inspectable",
          traitIndex,
          targetType: type,
          key,
          ...(mutableInner(targetType) !== undefined ? { outerMut: true as const } : {}),
        },
        bounds,
      );
    }
    if (traitName === "Debug")
      return builtinDebug(type) &&
        implementsDebug(type, this.traitTypes, this.implementations, this.signature.genericBounds)
        ? plan({ kind: "debug", traitIndex, targetType: type })
        : undefined;
    if (traitName === "Display") {
      return numericType(type) || ["bool", "char", "string"].includes(type)
        ? plan({ kind: "display", traitIndex, targetType: type })
        : undefined;
    }
    if (traitName === "Eq" || traitName === "PartialOrd" || traitName === "Ord") {
      if (traitName === "Ord" && !builtinTotallyOrdered(type)) return undefined;
      const strategy =
        traitName === "Eq" ? this.equalityStrategy(type) : this.orderingStrategy(type);
      if (!strategy || strategy.kind === "dispatch") return undefined;
      // It carries its supertrait's dictionary (09-traits.md#comparison-traits).
      const parent =
        traitName === "Eq"
          ? undefined
          : this.builtinTraitDictionaryPlan(
              this.traitTypes.get(traitName === "Ord" ? "PartialOrd" : "Eq")!.index,
              targetType,
              [],
              span,
            );
      if (traitName !== "Eq" && !parent) return undefined;
      const bounds: HirExpression[] = [];
      const renumbered = this.renumberBoundDispatches(strategy, bounds, new Map(), span);
      const kind =
        traitName === "Eq" ? "equality" : traitName === "Ord" ? "total-ordering" : "ordering";
      // The strategy kind matches `kind`: equality for Eq, ordering otherwise.
      const builtin = {
        kind,
        traitIndex,
        targetType: type,
        strategy: renumbered,
      } as HirBuiltinTraitImplementation;
      return { ...plan(builtin, bounds), supertraits: parent ? [parent] : [] };
    }
    return undefined;
  }

  // Composite strategies over a bounded generic element dispatch through the
  // caller's bound dictionaries. The built-in dictionary carries those in its
  // bound pack, so each distinct bound gets a pack position.
  private renumberBoundDispatches(
    strategy: HirEqualityStrategy | HirOrderingStrategy,
    bounds: HirExpression[],
    positions: Map<number, number>,
    span: SourceSpan,
  ): HirEqualityStrategy | HirOrderingStrategy {
    const visit = (value: unknown): unknown => {
      if (Array.isArray(value)) return value.map(visit);
      if (typeof value !== "object" || value === null) return value;
      const node = value as Record<string, unknown>;
      const dispatch = node.dispatch as HirEqualityDispatch | undefined;
      if (node.kind === "dispatch" && dispatch?.kind === "bound") {
        let position = positions.get(dispatch.boundIndex);
        if (position === undefined) {
          position = bounds.length;
          positions.set(dispatch.boundIndex, position);
          const bound = this.signature.genericBounds[dispatch.boundIndex]!;
          bounds.push({
            kind: "trait-bound-dictionary",
            traitIndex: dispatch.via?.traitIndex ?? dispatch.traitIndex,
            boundIndex: dispatch.boundIndex,
            type: `trait:${bound.traitName}`,
            span,
          });
        }
        return { ...node, dispatch: { ...dispatch, boundIndex: position } };
      }
      return Object.fromEntries(Object.entries(node).map(([key, entry]) => [key, visit(entry)]));
    };
    return visit(strategy) as HirEqualityStrategy | HirOrderingStrategy;
  }

  protected displayValue(
    value: HirExpression,
    span: SourceSpan,
    origin = "string interpolation",
  ): HirExpression {
    const type = readonlyType(value.type);
    if (type === "string") return value;
    if (numericType(type) || type === "bool" || type === "char") {
      return { kind: "display", operand: value, type: "string", span };
    }
    const trait = this.traitTypes.get("Display")!;
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
    const implementation = this.implementations.find(
      (candidate) => candidate.traitIndex === trait.index && candidate.targetType === type,
    );
    const mapping = implementation?.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
    const signature = mapping
      ? [...this.signatures.values()].find(({ index }) => index === mapping.functionIndex)
      : undefined;
    if (signature) {
      return {
        kind: "call",
        functionIndex: signature.index,
        functionName: signature.name,
        arguments: [this.coerce(value, type, span)],
        providers: [],
        type: "string",
        span,
      };
    }
    return this.fail(
      "unsatisfied-trait-bound",
      `type '${value.type}' does not implement Display, required by ${origin}`,
      span,
    );
  }

  protected equalityDispatch(type: ValueType): HirEqualityDispatch | undefined {
    return this.traitMethodDispatch(type, "Eq");
  }

  protected equalityStrategy(type: ValueType): HirEqualityStrategy | undefined {
    const comparedType = readonlyType(type);
    if (numericType(comparedType) || ["bool", "char", "string"].includes(comparedType))
      return { kind: "builtin" };
    const tuple = tupleParts(comparedType);
    if (tuple !== undefined) {
      const elements = tuple.map((element) => this.equalityStrategy(element));
      return elements.every((element) => element !== undefined)
        ? { kind: "tuple", elements: elements as HirEqualityStrategy[] }
        : undefined;
    }
    const optional = optionalInner(comparedType);
    if (optional !== undefined) {
      const value = this.equalityStrategy(optional);
      return value ? { kind: "optional", value } : undefined;
    }
    const result = resultParts(comparedType);
    if (result) {
      const ok = this.equalityStrategy(result.ok);
      const error = this.equalityStrategy(result.error);
      return ok && error ? { kind: "result", ok, error } : undefined;
    }
    const nominal = nominalGenericParts(comparedType);
    if (nominal?.name === "List" && nominal.arguments.length === 1) {
      const element = this.equalityStrategy(nominal.arguments[0]!);
      return element ? { kind: "list", element } : undefined;
    }
    if (
      nominal?.name === "Map" &&
      nominal.arguments.length === 2 &&
      mapKeyKind(nominal.arguments[0]!) !== undefined
    ) {
      const value = this.equalityStrategy(nominal.arguments[1]!);
      return value ? { kind: "map", value } : undefined;
    }
    const dispatch = this.equalityDispatch(comparedType);
    return dispatch ? { kind: "dispatch", dispatch } : undefined;
  }

  private traitMethodDispatch(type: ValueType, traitName: string): HirEqualityDispatch | undefined {
    const comparedType = readonlyType(type);
    const trait = this.traitTypes.get(traitName)!;
    const generic = genericTypeName(comparedType);
    const boundIndex = generic
      ? this.signature.genericBounds.findIndex(
          (bound) => bound.parameter === generic && bound.traitName === trait.name,
        )
      : -1;
    if (boundIndex >= 0) {
      return { kind: "bound", traitIndex: trait.index, methodIndex: 0, boundIndex };
    }
    // A bound whose trait extends the compared one, as `T < Integer` extends
    // `Ord` and so `PartialOrd` and `Eq` (09-traits.md#supertraits).
    if (generic)
      for (const [index, bound] of this.signature.genericBounds.entries()) {
        if (bound.parameter !== generic) continue;
        const boundTrait = this.traitTypes.get(bound.traitName);
        const path =
          boundTrait && this.findSupertraitPath(boundTrait, bound.traitArguments, trait.index, []);
        if (path)
          return {
            kind: "bound",
            traitIndex: trait.index,
            methodIndex: 0,
            boundIndex: index,
            via: { traitIndex: boundTrait.index, path },
          };
      }
    const implementation = this.implementations.find(
      (candidate) => candidate.traitIndex === trait.index && candidate.targetType === comparedType,
    );
    const mapping = implementation?.methodFunctions.find(({ methodIndex }) => methodIndex === 0);
    return mapping ? { kind: "function", functionIndex: mapping.functionIndex } : undefined;
  }

  protected orderingStrategy(type: ValueType): HirOrderingStrategy | undefined {
    const comparedType = readonlyType(type);
    if (numericType(comparedType) || ["char", "string"].includes(comparedType))
      return { kind: "builtin" };
    const tuple = tupleParts(comparedType);
    if (tuple !== undefined) {
      const elements = tuple.map((element) => this.orderingStrategy(element));
      return elements.every((element) => element !== undefined)
        ? { kind: "tuple", elements: elements as HirOrderingStrategy[] }
        : undefined;
    }
    const optional = optionalInner(comparedType);
    if (optional !== undefined) {
      const value = this.orderingStrategy(optional);
      return value ? { kind: "optional", value } : undefined;
    }
    const nominal = nominalGenericParts(comparedType);
    if (nominal?.name === "List" && nominal.arguments.length === 1) {
      const element = this.orderingStrategy(nominal.arguments[0]!);
      return element ? { kind: "list", element } : undefined;
    }
    const dispatch = this.traitMethodDispatch(comparedType, "PartialOrd");
    return dispatch ? { kind: "dispatch", dispatch } : undefined;
  }

  protected equalityExpression(
    left: HirExpression,
    right: HirExpression,
    span: SourceSpan,
  ): HirExpression | undefined {
    const strategy = this.equalityStrategy(left.type);
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
    // `never`, so it joins any other branch type (spec/06-control-flow.md).
    if (last?.kind === "return" || last?.kind === "break" || last?.kind === "continue")
      return "never";
    return last?.kind === "expression" ? last.expression.type : "void";
  }

  protected recordInferredReturn(type: ValueType, span: SourceSpan): void {
    const previous = this.inferredReturnType;
    if (type === "never" || previous === type) return;
    // Function values with different rows take their union (r-req.row.union.sites).
    const joined =
      previous === undefined || previous === "never" ? type : rowUnionType([previous, type]);
    if (joined === undefined)
      this.fail(
        "no-common-type",
        `closure return paths have types ${previous} and ${type} with no common type`,
        span,
      );
    this.inferredReturnType = joined;
  }

  protected checkFallthrough(body: readonly HirStatement[]): void {
    if (this.signature.result === "void") return;
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
    const declared = normalizeBoundProjections(kinded, this.signature.genericBounds);
    const nominal = nominalGenericParts(declared);
    if (
      nominal?.name === "Map" &&
      nominal.arguments.length === 2 &&
      isKnownType(nominal.arguments[0]!, this.dataTypes, this.enumTypes, this.traitTypes) &&
      isKnownType(nominal.arguments[1]!, this.dataTypes, this.enumTypes, this.traitTypes) &&
      mapKeyKind(nominal.arguments[0]!) === undefined
    ) {
      this.fail(
        "invalid-map-key",
        `type '${nominal.arguments[0]}' does not implement the MVP map-key contract`,
        type.span,
      );
    }
    if (!isKnownType(declared, this.dataTypes, this.enumTypes, this.traitTypes))
      this.fail("unknown-type", `unknown or unsupported type '${type.name}'`, type.span);
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
    const visible = new Map(this.availableCaptures);
    for (const scope of this.scopes) {
      for (const [name, local] of scope) visible.set(name, local);
    }
    for (const scope of this.providerScopes) {
      for (const local of scope.values()) visible.set(local.name, local);
    }
    return visible;
  }

  protected visibleProviders(): ReadonlyMap<string, HirLocal> {
    const visible = new Map<string, HirLocal>();
    for (const scope of this.providerScopes) {
      for (const [key, local] of scope) visible.set(key, local);
    }
    return visible;
  }

  protected referenceLocal(local: HirLocal, span: SourceSpan): HirExpression {
    if (this.locals.includes(local)) return { kind: "local", local, type: local.type, span };
    if (this.insideClosure && [...this.availableCaptures.values()].includes(local))
      return this.captureReference(local.name, local, span);
    return { kind: "local", local, type: local.type, span };
  }

  // A read of a captured binding inside a closure. A closure uses each capture
  // with the access the binding has in its enclosing scope
  // (07-functions.md#r-fn.capture.access).
  protected captureReference(name: string, source: HirLocal, span: SourceSpan): HirExpression {
    const fieldIndex = this.captureField(name, source);
    return {
      kind: "capture",
      closureIndex: this.closureIndex,
      fieldIndex,
      type: source.type,
      span,
    };
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
    if (this.locals.includes(source))
      return { kind: "local", local: source, type: source.type, span };
    if (this.insideClosure && this.availableCaptures.get(source.name) === source) {
      let capture = this.captures.get(source.name);
      if (!capture) {
        capture = { source, fieldIndex: this.captures.size };
        this.captures.set(source.name, capture);
      }
      return {
        kind: "capture",
        closureIndex: this.closureIndex,
        fieldIndex: capture.fieldIndex,
        type: source.type,
        span,
      };
    }
    throw new Error(`cannot materialize closure capture '${source.name}'`);
  }

  protected requireAssignable(actual: ValueType, expected: ValueType, span: SourceSpan): void {
    if (actual === "never" || actual === expected || isPermissionWeakening(actual, expected))
      return;
    if (mutableInner(expected) === actual)
      this.fail(
        "mutable-upgrade",
        `readonly type '${actual}' cannot be upgraded to '${expected}'`,
        span,
      );
    if (narrowsTo(actual, expected))
      this.fail(
        "implicit-narrowing",
        `'${actual}' does not convert implicitly to '${expected}'; write an explicit cast`,
        span,
      );
    this.fail("type-mismatch", mismatchMessage(actual, expected), span);
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

  protected fail(code: string, message: string, span: SourceSpan): never {
    ({ code, message } = rowDiagnostic(code, message, this.declaration));
    ({ code, message } = derivedFieldDiagnostic(code, message, span));
    this.diagnostics.push({ code, message, span });
    throw new CheckFailure(message);
  }

  // The test-case functions are used only as direct calls in test position, so
  // any other use of their names is misplaced (spec/10-modules.md#r-module.testing.direct-call).
  // An item of a `tests:` block is visible only inside the block
  // (spec/03-names-and-scopes.md#r-names.tests.inside-only).
  protected visibleSignature(name: string): Signature | undefined {
    const signature = this.signatures.get(name);
    return signature?.testOnly && !this.declaration.testOnly ? undefined : signature;
  }

  protected failUnknownName(name: string, message: string, span: SourceSpan): never {
    const imported = this.imports.get(name);
    if (name === "it" || (imported !== undefined && TEST_CASE_FUNCTIONS.has(imported)))
      this.fail(
        "misplaced-test-case",
        `${name}(...) registers a test case only as a direct call at the top level of a tests block`,
        span,
      );
    if (this.declaration.defaultContext?.laterNames.includes(name))
      this.fail(
        "binding-not-yet-visible",
        `a default cannot refer to the later parameter '${name}'`,
        span,
      );
    this.fail("unknown-name", message, span);
  }
}

// The imported test-case functions besides the prelude's `it`
// (spec/10-modules.md#r-module.testing.position-statements).
const TEST_CASE_FUNCTIONS: ReadonlySet<string> = new Set([
  "std.testing.it_each",
  "std.testing.it_prop",
  "std.testing.it_prop_with",
]);
