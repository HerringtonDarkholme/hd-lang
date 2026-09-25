import type { Expression, FunctionDecl, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type {
  HirExpression,
  HirData,
  HirEnum,
  HirFunction,
  HirGenericBound,
  HirGlobal,
  HirLocal,
  HirProgram,
  HirStatement,
  HirTrait,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import { genericTypeName, resolveGenericType, resolveTraitType, traitTypeName } from "./shared.ts";
import {
  contextKeys,
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
} from "../types.ts";

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
  readonly rowParameters: readonly string[];
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly defaultFunctionNames: readonly (string | undefined)[];
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly span: SourceSpan;
}

export interface InherentMethod {
  readonly targetType: ValueType;
  readonly name: string;
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
  "i32",
  "bool",
  "f64",
  "char",
  "string",
  "void",
  "ConsoleError",
]);
export const MVP_HOST_CAPABILITIES = new Set(["Console"]);
export const PRELUDE_NAMES = new Set([
  "never",
  "bool",
  "i8",
  "i16",
  "i32",
  "i64",
  "u8",
  "u16",
  "u32",
  "u64",
  "f32",
  "f64",
  "char",
  "string",
  "void",
  "list",
  "map",
  "Any",
  "Reference",
  "Result",
  "Ok",
  "Err",
  "panic",
  "Display",
  "PartialEq",
  "Eq",
  "PartialOrd",
  "Ord",
  "Ordering",
  "Hash",
  "Hasher",
  "Iterator",
  "Iterable",
  "Console",
  "ConsoleError",
  "println",
  "Suspend",
  "Poll",
  "PollContext",
  "Waker",
  "Annotation",
  "Annotate",
  "TypeAnnotator",
  "DataAnnotator",
  "EnumAnnotator",
  "FuncAnnotator",
  "FieldMetadata",
  "VariantMetadata",
  "ParamMetadata",
  "AnnotationRef",
  "ShapeMetadata",
  "DeclarationId",
  "DeclarationKind",
  "PrimitiveKind",
  "SourcePosition",
  "TypeShape",
  "DataShape",
  "FieldShape",
  "EnumShape",
  "VariantShape",
  "FnShape",
  "ParamShape",
]);

export function mapKeyKind(type: ValueType): 0 | 1 | undefined {
  if (type === "i32" || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return undefined;
}

export function supportsMvpEquality(type: ValueType): boolean {
  const readonly = readonlyType(type);
  if (["i32", "bool", "f64", "char", "string"].includes(readonly)) return true;
  const tuple = tupleParts(readonly);
  if (tuple !== undefined) return tuple.every(supportsMvpEquality);
  const nominal = nominalGenericParts(readonly);
  return Boolean(
    nominal?.name === "list" &&
    nominal.arguments.length === 1 &&
    supportsMvpEquality(nominal.arguments[0]!),
  );
}

export function isPermissionWeakening(actual: ValueType, expected: ValueType): boolean {
  const mutable = mutableInner(actual);
  if (mutable !== undefined)
    return mutable === expected || isPermissionWeakening(mutable, expected);
  const actualNominal = nominalGenericParts(actual);
  const expectedNominal = nominalGenericParts(expected);
  if (
    actualNominal?.name === "list" &&
    expectedNominal?.name === "list" &&
    actualNominal.arguments.length === 1 &&
    expectedNominal.arguments.length === 1
  ) {
    return (
      actualNominal.arguments[0] === expectedNominal.arguments[0] ||
      isPermissionWeakening(actualNominal.arguments[0]!, expectedNominal.arguments[0]!)
    );
  }
  return false;
}

export function weakenBoundedGenericActual(
  formal: ValueType,
  actual: ValueType,
  bounded: ReadonlySet<string>,
): ValueType {
  const generic = genericTypeName(formal);
  if (generic && bounded.has(generic)) return readonlyType(actual);
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(readonlyType(actual));
  if (
    formalNominal &&
    actualNominal &&
    formalNominal.name === actualNominal.name &&
    formalNominal.arguments.length === actualNominal.arguments.length
  ) {
    return nominalGenericType(
      actualNominal.name,
      actualNominal.arguments.map((argument, index) =>
        weakenBoundedGenericActual(formalNominal.arguments[index]!, argument, bounded),
      ),
    );
  }
  return actual;
}

export function isKnownType(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait> = new Map(),
): boolean {
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutable !== "void" && isKnownType(mutable, dataTypes, enumTypes, traitTypes);
  if (genericTypeName(type)) return true;
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
    if (nominal.name === "list") {
      return (
        nominal.arguments.length === 1 &&
        nominal.arguments[0] !== "void" &&
        isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
      );
    }
    if (nominal.name === "map") {
      return (
        nominal.arguments.length === 2 &&
        mapKeyKind(nominal.arguments[0]!) !== undefined &&
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
  protected readonly closureIndex: number;
  protected readonly captures = new Map<string, { source: HirLocal; fieldIndex: number }>();
  protected readonly providerScopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly diagnostics: Diagnostic[] = [];
  protected readonly scopes: Map<string, HirLocal>[] = [new Map()];
  protected readonly locals: HirLocal[] = [];
  protected readonly loopResults: Array<ValueType | undefined> = [];
  protected deferDepth = 0;

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
        if (type === "void")
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
      );
      let result = this.signature.result;
      if (this.inferResult) {
        const last = body.at(-1);
        if (last?.kind !== "return")
          this.recordInferredReturn(this.blockType(body), last?.span ?? this.declaration.span);
        result = this.inferredReturnType ?? "void";
      } else {
        this.checkFallthrough(body);
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
  ): HirExpression {
    if (!expected || value.type === expected || value.type === "never") return value;
    if (isPermissionWeakening(value.type, expected)) {
      return { kind: "permission-weaken", operand: value, type: expected, span };
    }
    const traitName = traitTypeName(expected);
    const trait = traitName && this.traitTypes.get(traitName);
    if (trait) {
      const mutableTrait = mutableInner(expected) !== undefined;
      if (mutableTrait && mutableInner(value.type) === undefined) return value;
      const implementationType = readonlyType(value.type);
      const implementation = this.implementations.find(
        (candidate) =>
          candidate.traitIndex === trait.index && candidate.targetType === implementationType,
      );
      if (implementation) {
        const receiverType = mutableTrait ? mutableType(implementationType) : implementationType;
        const wrappedValue =
          receiverType === value.type ? value : this.coerce(value, receiverType, span);
        return {
          kind: "trait-wrap",
          value: wrappedValue,
          traitIndex: trait.index,
          implementationIndex: implementation.index,
          type: expected,
          span,
        };
      }
    }
    const inner = optionalInner(expected);
    if (inner !== undefined) {
      const payload = this.coerce(value, inner, span);
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

  protected blockType(statements: readonly HirStatement[]): ValueType {
    const last = statements.at(-1);
    return last?.kind === "expression" ? last.expression.type : "void";
  }

  protected recordInferredReturn(type: ValueType, span: SourceSpan): void {
    if (type === "never") return;
    if (this.inferredReturnType === undefined || this.inferredReturnType === "never") {
      this.inferredReturnType = type;
      return;
    }
    if (this.inferredReturnType !== type) {
      this.fail(
        "closure-result-type",
        `closure return paths have types ${this.inferredReturnType} and ${type}`,
        span,
      );
    }
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
    const declared = resolveTraitType(resolved, this.traitTypes);
    const nominal = nominalGenericParts(declared);
    if (
      nominal?.name === "map" &&
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
    if (this.insideClosure && [...this.availableCaptures.values()].includes(local)) {
      let capture = this.captures.get(local.name);
      if (!capture) {
        capture = { source: local, fieldIndex: this.captures.size };
        this.captures.set(local.name, capture);
      }
      return {
        kind: "capture",
        closureIndex: this.closureIndex,
        fieldIndex: capture.fieldIndex,
        type: local.type,
        span,
      };
    }
    return { kind: "local", local, type: local.type, span };
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
    this.fail("type-mismatch", `expected ${expected}, found ${actual}`, span);
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
    this.diagnostics.push({ code, message, span });
    throw new CheckFailure(message);
  }
}
