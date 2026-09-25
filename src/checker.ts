import type { Expression, FunctionDecl, ImplDecl, Parameter, Pattern, Program, Statement, TraitDecl, TypeRef } from "./ast.ts";
import type { Diagnostic, SourceSpan } from "./diagnostics.ts";
import type {
  HirExpression,
  HirData,
  HirEnum,
  HirMatchArm,
  HirPatternAccessStep,
  HirFunction,
  HirLocal,
  HirProgram,
  HirProviderContextEntry,
  HirStatement,
  HirTrait,
  HirTraitImplementation,
  ValueType,
} from "./hir.ts";
import { contextKeys, contextType, functionParts, functionType, mutableInner, mutableType, nominalGenericParts, nominalGenericType, optionalInner, readonlyType, resultParts, suspensionParts, suspensionType, traitSuspensionParts, traitSuspensionType, tupleParts, tupleType } from "./types.ts";

export interface CheckResult {
  readonly program?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

interface Signature {
  readonly name: string;
  readonly index: number;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly { readonly parameter: string; readonly traitName: string; readonly traitIndex: number }[];
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

interface InherentMethod {
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

interface PlannedArgument {
  readonly parameterIndex: number;
  readonly argumentIndices: readonly number[];
  readonly kind: "single" | "vararg-elements";
}

class CheckFailure extends Error {}

function hirValueReadsLocal(value: unknown, local: HirLocal): boolean {
  if (Array.isArray(value)) return value.some((item) => hirValueReadsLocal(item, local));
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "local" && node.local === local) return true;
  return Object.entries(node).some(([key, child]) => key !== "span" && key !== "local" && key !== "bindings" && hirValueReadsLocal(child, local));
}

const TYPE_NAMES = new Set<ValueType>(["i32", "bool", "f64", "char", "string", "void", "ConsoleError"]);
const MVP_HOST_CAPABILITIES = new Set(["Console"]);
const PRELUDE_NAMES = new Set([
  "never", "bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "char", "string", "void",
  "list", "map", "Any", "Reference", "Result", "Ok", "Err", "panic", "Display", "PartialEq", "Eq", "PartialOrd", "Ord",
  "Ordering", "Hash", "Hasher", "Iterator", "Iterable", "Console", "ConsoleError", "println", "Suspend", "Poll", "PollContext",
  "Waker", "Annotation", "Annotate", "TypeAnnotator", "DataAnnotator", "EnumAnnotator", "FuncAnnotator", "FieldMetadata",
  "VariantMetadata", "ParamMetadata", "AnnotationRef", "ShapeMetadata", "DeclarationId", "DeclarationKind", "PrimitiveKind",
  "SourcePosition", "TypeShape", "DataShape", "FieldShape", "EnumShape", "VariantShape", "FnShape", "ParamShape",
]);

function mapKeyKind(type: ValueType): 0 | 1 | undefined {
  if (type === "i32" || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return undefined;
}

function supportsMvpEquality(type: ValueType): boolean {
  const readonly = readonlyType(type);
  if (["i32", "bool", "f64", "char", "string"].includes(readonly)) return true;
  const tuple = tupleParts(readonly);
  if (tuple !== undefined) return tuple.every(supportsMvpEquality);
  const nominal = nominalGenericParts(readonly);
  return Boolean(nominal?.name === "list" && nominal.arguments.length === 1 && supportsMvpEquality(nominal.arguments[0]!));
}

function isPermissionWeakening(actual: ValueType, expected: ValueType): boolean {
  const mutable = mutableInner(actual);
  if (mutable !== undefined) return mutable === expected || isPermissionWeakening(mutable, expected);
  const actualNominal = nominalGenericParts(actual);
  const expectedNominal = nominalGenericParts(expected);
  if (actualNominal?.name === "list" && expectedNominal?.name === "list"
    && actualNominal.arguments.length === 1 && expectedNominal.arguments.length === 1) {
    return actualNominal.arguments[0] === expectedNominal.arguments[0]
      || isPermissionWeakening(actualNominal.arguments[0]!, expectedNominal.arguments[0]!);
  }
  return false;
}

function weakenBoundedGenericActual(formal: ValueType, actual: ValueType, bounded: ReadonlySet<string>): ValueType {
  const generic = genericTypeName(formal);
  if (generic && bounded.has(generic)) return readonlyType(actual);
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(readonlyType(actual));
  if (formalNominal && actualNominal && formalNominal.name === actualNominal.name
    && formalNominal.arguments.length === actualNominal.arguments.length) {
    return nominalGenericType(actualNominal.name, actualNominal.arguments.map((argument, index) => (
      weakenBoundedGenericActual(formalNominal.arguments[index]!, argument, bounded)
    )));
  }
  return actual;
}

function isKnownType(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait> = new Map(),
): boolean {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutable !== "void" && isKnownType(mutable, dataTypes, enumTypes, traitTypes);
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
    return trait.genericParameters.length === nominalTrait.arguments.length
      && nominalTrait.arguments.every((argument) => isKnownType(argument, dataTypes, enumTypes, traitTypes));
  }
  if (type.startsWith("provider:")) return true;
  if (contextKeys(type)) return true;
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tuple.every((element) => element !== "void" && isKnownType(element, dataTypes, enumTypes, traitTypes));
  const optional = optionalInner(type);
  if (optional !== undefined) return optional !== "void" && isKnownType(optional, dataTypes, enumTypes, traitTypes);
  const result = resultParts(type);
  if (result) return isKnownType(result.ok, dataTypes, enumTypes, traitTypes) && result.error !== "void" && isKnownType(result.error, dataTypes, enumTypes, traitTypes);
  const nominal = nominalGenericParts(type);
  if (nominal) {
    if (nominal.name === "list") {
      return nominal.arguments.length === 1
        && nominal.arguments[0] !== "void"
        && isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes);
    }
    if (nominal.name === "map") {
      return nominal.arguments.length === 2
        && mapKeyKind(nominal.arguments[0]!) !== undefined
        && nominal.arguments[1] !== "void"
        && isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes);
    }
    const declaration = dataTypes.get(nominal.name);
    if (declaration) {
      return declaration.genericParameters.length === nominal.arguments.length
        && nominal.arguments.every((argument) => isKnownType(argument, dataTypes, enumTypes, traitTypes));
    }
    const enumDeclaration = enumTypes.get(nominal.name);
    return Boolean(enumDeclaration && enumDeclaration.genericParameters.length === nominal.arguments.length
      && nominal.arguments.every((argument) => isKnownType(argument, dataTypes, enumTypes, traitTypes)));
  }
  const callable = functionParts(type);
  return Boolean(callable && callable.parameters.every((parameter) => parameter !== "void" && isKnownType(parameter, dataTypes, enumTypes, traitTypes)) && isKnownType(callable.result, dataTypes, enumTypes, traitTypes));
}

class FunctionChecker {
  private readonly declaration: FunctionDecl;
  private readonly signature: Signature;
  private readonly signatures: ReadonlyMap<string, Signature>;
  private readonly dataTypes: ReadonlyMap<string, HirData>;
  private readonly enumTypes: ReadonlyMap<string, HirEnum>;
  private readonly traitTypes: ReadonlyMap<string, HirTrait>;
  private readonly implementations: readonly HirTraitImplementation[];
  private readonly inherentMethods: readonly InherentMethod[];
  private readonly synthetic: boolean;
  private readonly moduleBody: boolean;
  private readonly closures: HirFunction[];
  private readonly insideClosure: boolean;
  private readonly availableCaptures: ReadonlyMap<string, HirLocal>;
  private readonly availableProviders: ReadonlyMap<string, HirLocal>;
  private readonly inferRequirements: boolean;
  private readonly inferResult: boolean;
  private readonly selfClosureLocal?: HirLocal;
  private readonly imports: ReadonlyMap<string, string>;
  private pendingRecursiveClosure?: HirLocal;
  private readonly inferredRequirements: string[] = [];
  private inferredReturnType?: ValueType;
  private readonly closureIndex: number;
  private readonly captures = new Map<string, { source: HirLocal; fieldIndex: number }>();
  private readonly providerScopes: Map<string, HirLocal>[] = [new Map()];
  private readonly diagnostics: Diagnostic[] = [];
  private readonly scopes: Map<string, HirLocal>[] = [new Map()];
  private readonly locals: HirLocal[] = [];
  private readonly loopResults: Array<ValueType | undefined> = [];
  private deferDepth = 0;

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
    this.closureIndex = closureIndex;
    this.providerScopes[0] = new Map(availableProviders);
  }

  check(): { function?: HirFunction; diagnostics: readonly Diagnostic[] } {
    try {
      const parameters = this.declaration.parameters.map((parameter, index) => {
        if (PRELUDE_NAMES.has(parameter.name)) {
          this.fail("prelude-name-shadow", `parameter '${parameter.name}' shadows a prelude name`, parameter.span);
        }
        const type = this.signature.parameters[index] ?? this.resolveType(parameter.type);
        if (type === "void") this.fail("void-parameter", "a parameter cannot have type void", parameter.span);
        if (this.currentScope().has(parameter.name)) this.fail("duplicate-binding", `duplicate parameter '${parameter.name}'`, parameter.span);
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
      const body = this.checkStatements(this.declaration.body, false, this.inferResult ? undefined : this.signature.result);
      let result = this.signature.result;
      if (this.inferResult) {
        const last = body.at(-1);
        if (last?.kind !== "return") this.recordInferredReturn(this.blockType(body), last?.span ?? this.declaration.span);
        result = this.inferredReturnType ?? "void";
      } else {
        this.checkFallthrough(body);
      }
      for (const local of this.locals) {
        if (local.parameter || local.name.startsWith("$") || hirValueReadsLocal(body, local)) continue;
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
          requirements: this.inferRequirements ? [...this.inferredRequirements].sort() : this.signature.requirements,
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

  private checkStatements(statements: readonly Statement[], scoped = false, expectedFinal?: ValueType, finalValueContext = false): HirStatement[] {
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
        const result = this.checkStatement(statement, final ? expectedFinal : undefined, final && finalValueContext);
        checked.push(result);
        unreachable ||= statement.kind === "return"
          || statement.kind === "break"
          || statement.kind === "continue"
          || (result.kind === "expression" && result.expression.type === "never");
      }
      return checked;
    } finally {
      if (scoped) this.scopes.pop();
    }
  }

  private checkStatement(statement: Statement, expected?: ValueType, valueContext = false): HirStatement {
    switch (statement.kind) {
      case "defer": {
        if (this.moduleBody) {
          this.fail("defer-outside-cleanup-scope", "defer is only valid inside a function or closure body", statement.span);
        }
        this.deferDepth += 1;
        let body: readonly HirStatement[];
        try {
          body = this.checkStatements(statement.body, true);
        } finally {
          this.deferDepth -= 1;
        }
        return { kind: "defer", body, span: statement.span };
      }
      case "binding": {
        if (PRELUDE_NAMES.has(statement.name)) {
          this.fail("prelude-name-shadow", `local binding '${statement.name}' shadows a prelude name`, statement.span);
        }
        if (this.currentScope().has(statement.name)) {
          this.fail("duplicate-binding", `binding '${statement.name}' already exists in this function`, statement.span);
        }
        const mutableSuspensionResult = statement.annotation?.name.startsWith("mut-suspend:")
          ? statement.annotation.name.slice("mut-suspend:".length)
          : undefined;
        const annotation = statement.annotation && mutableSuspensionResult === undefined ? this.resolveType(statement.annotation) : undefined;
        let recursiveLocal: HirLocal | undefined;
        const recursiveClosure = statement.value.kind === "closure" && statementsReferenceName(statement.value.body, statement.name);
        if (recursiveClosure && statement.value.kind === "closure" && !statement.value.result && !(annotation && functionParts(annotation))) {
          this.fail("recursive-closure-needs-result-type", `recursive closure '${statement.name}' needs an explicit result type`, statement.value.span);
        }
        if (recursiveClosure && statement.value.kind === "closure") {
          let recursiveType = annotation && functionParts(annotation) ? annotation : undefined;
          if (!recursiveType && statement.value.result && statement.value.parameters.every((parameter) => parameter.type)) {
            recursiveType = functionType(
              statement.value.parameters.map((parameter) => this.resolveType(parameter.type!)),
              this.resolveType(statement.value.result),
              statement.value.requirements ?? [],
            );
          }
          if (recursiveType) {
            recursiveLocal = {
              name: statement.name,
              type: recursiveType,
              index: this.locals.length,
              mutable: statement.mutable,
              parameter: false,
              span: statement.span,
            };
            this.locals.push(recursiveLocal);
            this.currentScope().set(statement.name, recursiveLocal);
          }
        }
        const previousRecursiveClosure = this.pendingRecursiveClosure;
        this.pendingRecursiveClosure = recursiveLocal;
        let value: HirExpression;
        try {
          value = this.checkExpression(statement.value, annotation ?? recursiveLocal?.type);
        } finally {
          this.pendingRecursiveClosure = previousRecursiveClosure;
        }
        if (!statement.mutable && !annotation) {
          const readonly = mutableInner(value.type);
          if (readonly !== undefined) {
            value = ["data", "enum", "list", "map", "tuple", "closure"].includes(value.kind)
              ? { ...value, type: readonly }
              : this.requireCoercion(value, readonly, statement.value.span);
          }
        }
        const suspension = suspensionParts(value.type) ?? traitSuspensionParts(value.type);
        if (mutableSuspensionResult !== undefined && (!suspension || suspension.result !== mutableSuspensionResult)) {
          this.fail("type-mismatch", `expected mut Suspend[${mutableSuspensionResult}], found ${value.type}`, statement.value.span);
        }
        const type = mutableSuspensionResult !== undefined ? value.type : annotation ?? value.type;
        if (type === "never") this.fail("uninhabited-binding", "an inferred binding cannot have type never", statement.span);
        if (type === "void") this.fail("void-binding", "a binding cannot store a void value", statement.span);
        value = this.requireCoercion(value, type, statement.value.span);
        const local: HirLocal = recursiveLocal ?? {
          name: statement.name,
          type,
          index: this.locals.length,
          mutable: statement.mutable,
          drivable: mutableSuspensionResult !== undefined,
          parameter: false,
          span: statement.span,
        };
        if (!recursiveLocal) {
          this.locals.push(local);
          this.currentScope().set(statement.name, local);
        }
        return { kind: "binding", local, value, span: statement.span };
      }
      case "tuple-binding":
        throw new Error("tuple bindings are expanded by checkStatements");
      case "assignment": {
        const local = this.resolveLocal(statement.name);
        if (!local) this.fail("unknown-name", `unknown binding '${statement.name}'`, statement.span);
        if (!local.mutable) {
          const code = local.parameter ? "non-reassignable-parameter-binding" : "non-reassignable-binding";
          this.fail(code, `binding '${statement.name}' is not reassignable`, statement.span);
        }
        const value = this.requireCoercion(this.checkExpression(statement.value, local.type), local.type, statement.value.span);
        return { kind: "assignment", local, value, span: statement.span };
      }
      case "field-assignment": {
        const receiver = this.checkExpression(statement.target.receiver);
        const mutableReceiver = mutableInner(receiver.type);
        if (mutableReceiver === undefined) {
          let root: Expression = statement.target.receiver;
          while (root.kind === "member") root = root.receiver;
          const rootLocal = root.kind === "name" ? this.resolveLocal(root.name) : undefined;
          const code = rootLocal && mutableInner(rootLocal.type) !== undefined ? "readonly-edge" : "readonly-root";
          this.fail(code, `field '${statement.target.name}' cannot be assigned through readonly type '${receiver.type}'`, statement.target.span);
        }
        const nominal = nominalGenericParts(mutableReceiver);
        const declaration = this.dataTypes.get(nominal?.name ?? mutableReceiver);
        if (!declaration) this.fail("member-on-non-data", `type '${mutableReceiver}' has no assignable data fields`, statement.target.receiver.span);
        const field = declaration.fields.find((candidate) => candidate.name === statement.target.name);
        if (!field) this.fail("unknown-data-field", `type '${declaration.name}' has no field '${statement.target.name}'`, statement.target.span);
        const substitutions = new Map<string, ValueType>();
        if (nominal) declaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, nominal.arguments[index]!));
        const fieldType = substituteGenericType(field.type, substitutions);
        const value = this.requireCoercion(this.checkExpression(statement.value, fieldType), fieldType, statement.value.span);
        const expression: HirExpression = {
          kind: "field-set",
          receiver,
          value,
          dataIndex: declaration.index,
          fieldIndex: field.index,
          erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
          type: "void",
          span: statement.span,
        };
        return { kind: "expression", expression, span: statement.span };
      }
      case "index-assignment": {
        const receiver = this.checkExpression(statement.target.receiver);
        const mutableReceiver = mutableInner(receiver.type);
        if (mutableReceiver === undefined) {
          this.fail("readonly-root", `indexed assignment requires mutable access to '${receiver.type}'`, statement.target.receiver.span);
        }
        const nominal = nominalGenericParts(mutableReceiver);
        if (nominal?.name === "list" && nominal.arguments.length === 1) {
          const index = this.requireCoercion(this.checkExpression(statement.target.index, "i32"), "i32", statement.target.index.span);
          const value = this.requireCoercion(this.checkExpression(statement.value, nominal.arguments[0]), nominal.arguments[0]!, statement.value.span);
          return { kind: "expression", expression: { kind: "list-set", receiver, index, value, elementType: nominal.arguments[0]!, type: "void", span: statement.span }, span: statement.span };
        }
        if (nominal?.name === "map" && nominal.arguments.length === 2) {
          const key = this.requireCoercion(this.checkExpression(statement.target.index, nominal.arguments[0]), nominal.arguments[0]!, statement.target.index.span);
          const value = this.requireCoercion(this.checkExpression(statement.value, nominal.arguments[1]), nominal.arguments[1]!, statement.value.span);
          return { kind: "expression", expression: { kind: "map-set", receiver, key, value, keyType: nominal.arguments[0]!, valueType: nominal.arguments[1]!, type: "void", span: statement.span }, span: statement.span };
        }
        this.fail("not-indexable", `type '${receiver.type}' does not support indexed assignment`, statement.target.receiver.span);
      }
      case "discard":
        return { kind: "discard", value: this.checkExpression(statement.value), span: statement.span };
      case "return": {
        if (this.moduleBody) this.fail("return-outside-function", "return is not valid at module top level", statement.span);
        if (this.deferDepth > 0) this.fail("defer-control-flow", "a defer suite cannot return", statement.span);
        let value = statement.value && this.checkExpression(statement.value, this.inferResult ? undefined : this.signature.result);
        if (this.inferResult) this.recordInferredReturn(value?.type ?? "void", statement.span);
        else if (value) value = this.requireCoercion(value, this.signature.result, statement.span);
        else this.requireAssignable("void", this.signature.result, statement.span);
        return { kind: "return", value, span: statement.span };
      }
      case "break":
        if (this.deferDepth > 0) this.fail("defer-control-flow", "a defer suite cannot break", statement.span);
        if (this.loopResults.length === 0) this.fail("break-outside-loop", "break is only valid inside a loop", statement.span);
        const loopResult = this.loopResults.at(-1);
        if (loopResult === undefined && statement.value) {
          this.fail("break-value-context", "break values require a loop with an else suite", statement.span);
        }
        if (loopResult !== undefined && !statement.value) {
          this.fail("break-value-context", "a value-producing loop requires 'break value'", statement.span);
        }
        const value = statement.value && this.checkExpression(statement.value, loopResult);
        if (value && loopResult) this.requireAssignable(value.type, loopResult, statement.value!.span);
        return { kind: "break", value, span: statement.span };
      case "continue":
        if (this.deferDepth > 0) this.fail("defer-control-flow", "a defer suite cannot continue", statement.span);
        if (this.loopResults.length === 0) this.fail("continue-outside-loop", "continue is only valid inside a loop", statement.span);
        return { kind: "continue", span: statement.span };
      case "expression": {
        const expression = this.checkExpression(statement.expression, expected);
        if (!valueContext && (expected === undefined || expected === "void") && (optionalInner(expression.type) !== undefined || resultParts(expression.type) || suspensionParts(expression.type) || traitSuspensionParts(expression.type))) {
          this.fail("discarded-must-use-value", `a value of type '${expression.type}' must be used or explicitly discarded`, statement.span);
        }
        return { kind: "expression", expression, span: statement.span };
      }
      case "pass":
        return { kind: "pass", span: statement.span };
    }
  }

  private checkTupleBinding(statement: Extract<Statement, { kind: "tuple-binding" }>): HirStatement[] {
    const annotation = statement.annotation ? this.resolveType(statement.annotation) : undefined;
    const annotatedElements = annotation ? tupleParts(annotation) : undefined;
    if (annotation && annotatedElements === undefined) {
      this.fail("tuple-binding-annotation", `tuple binding annotation '${annotation}' is not a tuple type`, statement.annotation!.span);
    }
    const value = this.checkExpression(statement.value, annotation);
    const elements = tupleParts(value.type);
    if (elements === undefined) {
      this.fail("tuple-binding-requires-tuple", `tuple binding requires a tuple value, found '${value.type}'`, statement.value.span);
    }
    if (elements.length !== statement.bindings.length) {
      this.fail("tuple-binding-arity", `tuple binding has ${statement.bindings.length} names for ${elements.length} elements`, statement.span);
    }
    const names = new Set<string>();
    for (const binding of statement.bindings) {
      if (names.has(binding.name)) this.fail("duplicate-binding", `binding '${binding.name}' appears more than once`, binding.span);
      names.add(binding.name);
      if (PRELUDE_NAMES.has(binding.name)) this.fail("prelude-name-shadow", `local binding '${binding.name}' shadows a prelude name`, binding.span);
      if (this.currentScope().has(binding.name)) this.fail("duplicate-binding", `binding '${binding.name}' already exists in this function`, binding.span);
    }
    const tupleLocal: HirLocal = {
      name: `$tuple-binding${this.locals.length}`,
      type: value.type,
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span: statement.span,
    };
    this.locals.push(tupleLocal);
    const output: HirStatement[] = [{ kind: "binding", local: tupleLocal, value, span: statement.span }];
    for (const [index, binding] of statement.bindings.entries()) {
      const local: HirLocal = {
        name: binding.name,
        type: elements[index]!,
        index: this.locals.length,
        mutable: statement.mutable,
        parameter: false,
        span: binding.span,
      };
      this.locals.push(local);
      this.currentScope().set(binding.name, local);
      output.push({
        kind: "binding",
        local,
        value: {
          kind: "tuple-index",
          receiver: { kind: "local", local: tupleLocal, type: tupleLocal.type, span: statement.value.span },
          index,
          elementType: elements[index]!,
          type: elements[index]!,
          span: binding.span,
        },
        span: binding.span,
      });
    }
    return output;
  }

  private checkExpression(expression: Expression, expected?: ValueType): HirExpression {
    const value = this.checkExpressionRaw(expression, expected);
    return this.coerce(value, expected, expression.span);
  }

  private checkExpressionRaw(expression: Expression, expected?: ValueType): HirExpression {
    switch (expression.kind) {
      case "integer": {
        if (expression.value < -2_147_483_648n || expression.value > 2_147_483_647n) {
          this.fail("integer-literal-range", "integer literal is outside the i32 range", expression.span);
        }
        return { kind: "integer", value: Number(expression.value), type: "i32", span: expression.span };
      }
      case "float":
        if (!Number.isFinite(expression.value)) this.fail("float-literal-range", "floating-point literal is not finite", expression.span);
        return { ...expression, type: "f64" };
      case "string":
        return { kind: "string", bytes: [...new TextEncoder().encode(expression.value)], type: "string", span: expression.span };
      case "interpolated-string": {
        const segments = expression.segments.map((segment): HirExpression => {
          if (segment.kind === "text") {
            return { kind: "string", bytes: [...new TextEncoder().encode(segment.value)], type: "string", span: segment.span };
          }
          const operand = this.checkExpression(segment.expression);
          if (operand.type === "string") return operand;
          if (!["i32", "bool", "char"].includes(operand.type)) {
            this.fail("missing-display", `type '${operand.type}' does not implement Display in the executable MVP`, segment.span);
          }
          return { kind: "display", operand, type: "string", span: segment.span };
        });
        return { kind: "string-build", segments, type: "string", span: expression.span };
      }
      case "character":
        return { kind: "character", value: expression.value.codePointAt(0)!, type: "char", span: expression.span };
      case "boolean":
        return { ...expression, type: "bool" };
      case "nil": {
        if (!expected || optionalInner(expected) === undefined) {
          this.fail("nil-needs-optional-type", "nil requires an expected optional type", expression.span);
        }
        return { kind: "variant-wrap", variant: "optional-absent", type: expected, span: expression.span };
      }
      case "list": {
        const expectedDataType = expected ? readonlyType(expected) : undefined;
        const expectedNominal = expectedDataType ? nominalGenericParts(expectedDataType) : undefined;
        const contextualElement = expectedNominal?.name === "list" && expectedNominal.arguments.length === 1
          ? expectedNominal.arguments[0]
          : undefined;
        if (expression.elements.length === 0 && !contextualElement) {
          this.fail("empty-list-needs-context", "an empty list requires an expected list type", expression.span);
        }
        let elementType = contextualElement;
        const elements = expression.elements.map((element) => {
          const checked = this.checkExpression(element, contextualElement);
          if (!elementType) elementType = checked.type;
          if (!contextualElement && checked.type !== elementType) {
            this.fail("no-common-type", `list elements have no common type: ${elementType} and ${checked.type}`, element.span);
          }
          return this.requireCoercion(checked, elementType!, element.span);
        });
        const readonlyList = nominalGenericType("list", [elementType!]);
        const type = expected && mutableInner(expected) !== undefined || expected === undefined ? mutableType(readonlyList) : readonlyList;
        return { kind: "list", elements, elementType: elementType!, type, span: expression.span };
      }
      case "tuple": {
        const contextual = expected ? tupleParts(expected) : undefined;
        if (contextual && contextual.length !== expression.elements.length) {
          this.fail("tuple-arity", `expected a ${contextual.length}-element tuple, found ${expression.elements.length} elements`, expression.span);
        }
        const elements = expression.elements.map((element, index) => {
          const expectedElement = contextual?.[index];
          const checked = this.checkExpression(element, expectedElement);
          return expectedElement ? this.requireCoercion(checked, expectedElement, element.span) : checked;
        });
        const elementTypes = elements.map((element) => element.type);
        return { kind: "tuple", elements, elementTypes, type: tupleType(elementTypes), span: expression.span };
      }
      case "map": {
        const expectedNominal = expected ? nominalGenericParts(readonlyType(expected)) : undefined;
        const contextualKey = expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
          ? expectedNominal.arguments[0]
          : undefined;
        const contextualValue = expectedNominal?.name === "map" && expectedNominal.arguments.length === 2
          ? expectedNominal.arguments[1]
          : undefined;
        if (expression.entries.length === 0 && (!contextualKey || !contextualValue)) {
          this.fail("empty-map-needs-context", "an empty map requires an expected map type", expression.span);
        }
        let keyType = contextualKey;
        let valueType = contextualValue;
        const entries = expression.entries.map((entry) => {
          const checkedKey = this.checkExpression(entry.key, keyType);
          if (!keyType) keyType = checkedKey.type;
          const key = this.requireCoercion(checkedKey, keyType!, entry.key.span);
          const checkedValue = this.checkExpression(entry.value, valueType);
          if (!valueType) valueType = checkedValue.type;
          const value = this.requireCoercion(checkedValue, valueType!, entry.value.span);
          return { key, value };
        });
        const keyKind = mapKeyKind(keyType!);
        if (keyKind === undefined) {
          this.fail("unsupported-map-key", `type '${keyType}' does not have the MVP's built-in Eq and Hash support`, expression.span);
        }
        const readonlyMap = nominalGenericType("map", [keyType!, valueType!]);
        const type = expected && mutableInner(expected) !== undefined || expected === undefined ? mutableType(readonlyMap) : readonlyMap;
        return {
          kind: "map",
          entries,
          keyType: keyType!,
          valueType: valueType!,
          keyKind,
          type,
          span: expression.span,
        };
      }
      case "name": {
        const local = this.resolveLocal(expression.name);
        if (local) return { kind: "local", local, type: local.type, span: expression.span };
        const source = this.availableCaptures.get(expression.name);
        if (source) {
          if (source === this.selfClosureLocal) {
            return { kind: "closure-self", closureIndex: this.closureIndex, type: source.type, span: expression.span };
          }
          let capture = this.captures.get(expression.name);
          if (!capture) {
            capture = { source, fieldIndex: this.captures.size };
            this.captures.set(expression.name, capture);
          }
          return { kind: "capture", closureIndex: this.closureIndex, fieldIndex: capture.fieldIndex, type: source.type, span: expression.span };
        }
        const signature = this.signatures.get(expression.name);
        if (signature && !signature.suspending) {
          if (signature.genericParameters.length > 0 || signature.rowParameters.length > 0) {
            this.fail("generic-function-value-needs-arguments", `generic function '${signature.name}' needs inferred or explicit type arguments before it can be used as a value`, expression.span);
          }
          return {
            kind: "function-value",
            functionIndex: signature.index,
            functionName: signature.name,
            type: functionType(signature.parameters, signature.result, signature.requirements, signature.variadic),
            span: expression.span,
          };
        }
        this.fail("unknown-name", `unknown name '${expression.name}'`, expression.span);
      }
      case "unary": {
        if (expression.operator === "-" && expression.operand.kind === "integer" && expression.operand.value === 2_147_483_648n) {
          return { kind: "integer", value: -2_147_483_648, type: "i32", span: expression.span };
        }
        const operand = this.checkExpression(expression.operand);
        let type: ValueType;
        if (expression.operator === "not") {
          this.requireType(operand.type, "bool", expression.operand.span);
          type = "bool";
        } else if (expression.operator === "~") {
          this.requireType(operand.type, "i32", expression.operand.span);
          type = "i32";
        } else {
          if (operand.type !== "i32" && operand.type !== "f64") {
            const code = expression.operator === "+" ? "nonnumeric-unary-plus" : "invalid-unary-operand";
            this.fail(code, `operator '${expression.operator}' requires a numeric operand`, expression.span);
          }
          type = operand.type;
        }
        return { kind: "unary", operator: expression.operator, operand, type, span: expression.span };
      }
      case "binary": {
        const left = this.checkExpression(expression.left);
        const right = this.checkExpression(expression.right);
        if (expression.operator === "is") {
          if (left.type !== right.type) {
            this.fail("type-mismatch", `identity operands have types ${left.type} and ${right.type}`, expression.span);
          }
          const generic = genericTypeName(left.type);
          if (generic && !(this.signature.referenceParameters ?? []).includes(generic)) {
            this.fail("identity-needs-reference-bound", `generic parameter '${generic}' requires a Reference bound for identity comparison`, expression.span);
          }
          if (!this.isIdentityType(left.type)) {
            this.fail("identity-requires-references", `identity comparison does not accept '${left.type}'`, expression.span);
          }
          return { kind: "binary", operator: expression.operator, left, right, type: "bool", span: expression.span };
        }
        const logical = expression.operator === "and" || expression.operator === "or";
        const comparison = ["==", "!=", "<", "<=", ">", ">="].includes(expression.operator);
        const bitwise = ["&", "|", "^", "<<", ">>"].includes(expression.operator);
        const remainder = expression.operator === "%";
        const stringConcatenation = expression.operator === "+" && left.type === "string";
        if (logical) {
          this.requireType(left.type, "bool", left.span);
          this.requireType(right.type, "bool", right.span);
          return { kind: "binary", operator: expression.operator, left, right, type: "bool", span: expression.span };
        }
        if (left.type !== right.type) {
          if (expression.operator === "**") this.fail("mixed-numeric-types", "integer and floating-point power operands cannot be mixed", expression.span);
          this.fail("type-mismatch", `operator operands have types ${left.type} and ${right.type}`, expression.span);
        }
        if (comparison && this.dataTypes.has(nominalGenericParts(left.type)?.name ?? left.type)) {
          const equality = expression.operator === "==" || expression.operator === "!=";
          this.fail(
            equality ? "missing-partial-eq" : "missing-partial-ord",
            `type '${left.type}' does not implement ${equality ? "PartialEq" : "PartialOrd"}`,
            expression.span,
          );
        }
        if (comparison && functionParts(left.type)) {
          this.fail("unsupported-equality", `function values do not support operator '${expression.operator}'`, expression.span);
        }
        if (bitwise && left.type !== "i32") this.fail("invalid-binary-operands", `operator '${expression.operator}' requires i32 operands`, expression.span);
        if (remainder && left.type !== "i32") this.fail("invalid-binary-operands", "operator '%' requires integer operands", expression.span);
        if (comparison && left.type === "bool" && expression.operator !== "==" && expression.operator !== "!=") {
          this.fail("invalid-binary-operands", `operator '${expression.operator}' does not accept bool`, expression.span);
        }
        if (!bitwise && !remainder && !stringConcatenation && left.type !== "i32" && left.type !== "f64" && !(comparison && (left.type === "bool" || left.type === "char" || left.type === "string"))) {
          this.fail("invalid-binary-operands", `operator '${expression.operator}' does not accept ${left.type}`, expression.span);
        }
        return { kind: "binary", operator: expression.operator, left, right, type: comparison ? "bool" : left.type, span: expression.span };
      }
      case "call": {
        if (expression.callee.kind === "contextual-variant") {
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "enum constructors have no variadic parameter", expression.span);
          const nominal = expected ? nominalGenericParts(expected) : undefined;
          const declaration = expected && this.enumTypes.get(nominal?.name ?? expected);
          if (!declaration) {
            this.fail("missing-contextual-enum-type", `variant '.${expression.callee.name}' requires an expected enum type`, expression.span);
          }
          return this.checkEnumConstructor(declaration, expression.callee.name, expression, expected);
        }
        if (expression.callee.kind === "member") {
          const methodName = expression.callee.name;
          if (expression.callee.receiver.kind === "name") {
            const declaration = this.enumTypes.get(expression.callee.receiver.name);
            if (declaration) {
              if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "enum constructors have no variadic parameter", expression.span);
              return this.checkEnumConstructor(declaration, expression.callee.name, expression, expected);
            }
          }
          const receiver = this.checkExpression(expression.callee.receiver);
          const suspension = suspensionParts(receiver.type);
          const traitSuspension = traitSuspensionParts(receiver.type);
          if ((suspension || traitSuspension) && expression.callee.name === "cancel") {
            if (expression.arguments.length !== 0) this.fail("argument-count", "Suspend.cancel expects no arguments", expression.span);
            this.requireDrivableSuspension(expression.callee.receiver);
            return suspension
              ? { kind: "suspend-cancel", functionIndex: suspension.functionIndex, suspension: receiver, type: "void", span: expression.span }
              : { kind: "trait-suspend-cancel", traitIndex: traitSuspension!.traitIndex, methodIndex: traitSuspension!.methodIndex, suspension: receiver, type: "void", span: expression.span };
          }
          const fieldReceiverType = readonlyType(receiver.type);
          const fieldReceiverNominal = nominalGenericParts(fieldReceiverType);
          const fieldDeclaration = this.dataTypes.get(fieldReceiverNominal?.name ?? fieldReceiverType);
          const callableField = fieldDeclaration?.fields.find((field) => field.name === methodName);
          if (fieldDeclaration && callableField) {
            const substitutions = new Map<string, ValueType>();
            if (fieldReceiverNominal) fieldDeclaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, fieldReceiverNominal.arguments[index]!));
            const callableType = substituteGenericType(callableField.type, substitutions);
            const callable = functionParts(callableType);
            if (callable) {
              if (expression.argumentNames?.some((name) => name !== undefined)) {
                this.fail("named-argument-needs-declaration", "named arguments are unavailable through a stored function field", expression.span);
              }
              const fieldCallee: HirExpression = { kind: "member", receiver, dataIndex: fieldDeclaration.index, fieldIndex: callableField.index, erasedFieldType: genericTypeName(callableField.type) ? callableField.type : undefined, type: callableType, span: expression.callee.span };
              const parameterNames = callable.parameters.map((_, index) => `$${index}`);
              const checkedArguments = this.checkConcreteArguments(expression, callable.parameters, parameterNames, callable.variadic, "function field");
              const providers = callable.requirements.map((requirement) => this.resolveProvider(requirement, expression.span));
              const missing = callable.requirements.filter((_, index) => !providers[index]);
              if (missing.length > 0) this.fail("missing-requirement", `function field requires ${missing.join(" + ")}`, expression.span);
              return { kind: "closure-call", callee: fieldCallee, arguments: checkedArguments.arguments, providers: providers as HirExpression[], type: callable.result, span: expression.span };
            }
          }
          if (receiver.type === "string" && expression.callee.name === "len") {
            if (expression.arguments.length !== 0) this.fail("argument-count", "string.len expects no arguments", expression.span);
            return { kind: "string-length", receiver, type: "i32", span: expression.span };
          }
          if (receiver.type === "string" && expression.callee.name === "starts_with") {
            if (expression.arguments.length !== 1) this.fail("argument-count", "string.starts_with expects one argument", expression.span);
            if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "string.starts_with has no variadic parameter", expression.span);
            const argumentName = expression.argumentNames?.[0];
            if (argumentName && argumentName !== "prefix") this.fail("unknown-named-argument", `string.starts_with has no parameter named '${argumentName}'`, expression.arguments[0]!.span);
            const prefix = this.requireCoercion(this.checkExpression(expression.arguments[0]!, "string"), "string", expression.arguments[0]!.span);
            return { kind: "string-starts-with", receiver, prefix, type: "bool", span: expression.span };
          }
          const receiverNominal = nominalGenericParts(readonlyType(receiver.type));
          if (receiverNominal?.name === "list" && expression.callee.name === "len") {
            if (expression.arguments.length !== 0) this.fail("argument-count", "list.len expects no arguments", expression.span);
            return { kind: "list-length", receiver, type: "i32", span: expression.span };
          }
          if (receiverNominal?.name === "list" && expression.callee.name === "append") {
            if (mutableInner(receiver.type) === undefined) this.fail("mutable-receiver-required", "list.append requires mutable list access", expression.callee.receiver.span);
            if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "list.append has no variadic parameter", expression.span);
            if (expression.arguments.length !== 1) this.fail("argument-count", "list.append expects one value", expression.span);
            this.resolveArgumentMapping(expression, ["value"], "list.append");
            const elementType = receiverNominal.arguments[0]!;
            const value = this.requireCoercion(this.checkExpression(expression.arguments[0]!, elementType), elementType, expression.arguments[0]!.span);
            return { kind: "list-append", receiver, value, elementType, type: "void", span: expression.span };
          }
          if (receiverNominal?.name === "map" && expression.callee.name === "len") {
            if (expression.arguments.length !== 0) this.fail("argument-count", "map.len expects no arguments", expression.span);
            return { kind: "map-length", receiver, type: "i32", span: expression.span };
          }
          if (receiverNominal?.name === "map" && (expression.callee.name === "get" || expression.callee.name === "remove")) {
            const removing = expression.callee.name === "remove";
            if (removing && mutableInner(receiver.type) === undefined) {
              this.fail("mutable-receiver-required", "map.remove requires mutable map access", expression.callee.receiver.span);
            }
            if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", `map.${expression.callee.name} has no variadic parameter`, expression.span);
            if (expression.arguments.length !== 1) this.fail("argument-count", `map.${expression.callee.name} expects one key`, expression.span);
            this.resolveArgumentMapping(expression, ["key"], `map.${expression.callee.name}`);
            const keyType = receiverNominal.arguments[0]!;
            const valueType = receiverNominal.arguments[1]!;
            const key = this.requireCoercion(this.checkExpression(expression.arguments[0]!, keyType), keyType, expression.arguments[0]!.span);
            return {
              kind: removing ? "map-remove" : "map-index",
              receiver,
              key,
              keyType,
              valueType,
              type: `${valueType}?`,
              span: expression.span,
            };
          }
          const receiverGeneric = genericTypeName(readonlyType(receiver.type));
          const receiverBounds = receiverGeneric
            ? this.signature.genericBounds
                .map((bound, boundIndex) => ({ bound, boundIndex, trait: this.traitTypes.get(bound.traitName)! }))
                .filter(({ bound }) => bound.parameter === receiverGeneric)
            : [];
          const matchingBounds = receiverBounds.filter(({ trait }) => trait.methods.some((method) => method.name === methodName));
          if (matchingBounds.length > 1) {
            this.fail("ambiguous-bound-method", `method '${methodName}' is supplied by multiple bounds on ${receiverGeneric}`, expression.callee.span);
          }
          const receiverBound = matchingBounds[0];
          const dispatchReceiver: HirExpression = receiverBound
            ? {
                kind: "trait-bound",
                value: receiver,
                traitIndex: receiverBound.bound.traitIndex,
                boundIndex: receiverBound.boundIndex,
                type: mutableInner(receiver.type) !== undefined
                  ? mutableType(`trait:${receiverBound.bound.traitName}`)
                  : `trait:${receiverBound.bound.traitName}`,
                span: receiver.span,
              }
            : receiver;
          const dynamicTraitName = traitTypeName(dispatchReceiver.type);
          const dynamicTrait = dynamicTraitName && this.traitTypes.get(dynamicTraitName);
          if (dynamicTrait) {
            const method = dynamicTrait.methods.find((candidate) => candidate.name === methodName);
            if (!method) this.fail("unknown-method", `trait '${dynamicTrait.name}' has no method '${expression.callee.name}'`, expression.callee.span);
            if (method.receiverMutable && mutableInner(dispatchReceiver.type) === undefined) {
              this.fail("mutable-receiver-required", `method '${method.name}' requires mutable access to ${dynamicTrait.name}`, expression.callee.receiver.span);
            }
            const checkedArguments = this.checkConcreteArguments(expression, method.parameters, method.parameterNames, method.variadic, `method '${method.name}'`);
            const providers = method.requirements.map((requirement) => this.resolveProvider(requirement, expression.span));
            const missing = method.requirements.filter((_, index) => !providers[index]);
            if (missing.length > 0) this.fail("missing-requirement", `method '${method.name}' requires ${missing.join(" + ")}`, expression.span);
            return method.suspending
              ? {
                  kind: "trait-suspend-construct",
                  receiver: dispatchReceiver,
                  traitIndex: dynamicTrait.index,
                  methodIndex: method.index,
                  arguments: checkedArguments.arguments,
                  argumentParameterIndices: checkedArguments.parameterIndices,
                  providers: providers as HirExpression[],
                  type: traitSuspensionType(dynamicTrait.index, method.index, method.result),
                  span: expression.span,
                }
              : { kind: "trait-call", receiver: dispatchReceiver, traitIndex: dynamicTrait.index, methodIndex: method.index, arguments: checkedArguments.arguments, argumentParameterIndices: checkedArguments.parameterIndices, providers: providers as HirExpression[], type: method.result, span: expression.span };
          }
          const receiverImplementationType = readonlyType(receiver.type);
          const inherent = this.inherentMethods.find((method) => method.targetType === receiverImplementationType && method.name === methodName);
          if (inherent) return this.checkInherentMethodCall(expression, receiver, inherent);
          const receiverData = this.dataTypes.get(receiverImplementationType);
          const promoted = receiverData?.fields.flatMap((field) => {
            if (!field.embedded) return [];
            const fieldType = readonlyType(field.type);
            return this.inherentMethods
              .filter((method) => method.targetType === fieldType && method.name === methodName)
              .map((method) => ({ field, fieldType, method }));
          }) ?? [];
          if (promoted.length > 1) this.fail("ambiguous-method", `method '${methodName}' is promoted by multiple embedded fields`, expression.callee.span);
          if (promoted.length === 1) {
            const selected = promoted[0]!;
            const promotedReceiver: HirExpression = {
              kind: "member",
              receiver,
              dataIndex: receiverData!.index,
              fieldIndex: selected.field.index,
              erasedFieldType: genericTypeName(selected.field.type) ? selected.field.type : undefined,
              type: selected.fieldType,
              span: expression.callee.receiver.span,
            };
            return this.checkInherentMethodCall(expression, promotedReceiver, selected.method);
          }
          const candidates = this.implementations.flatMap((implementation) => {
            if (implementation.targetType !== receiverImplementationType) return [];
            const trait = [...this.traitTypes.values()].find((candidate) => candidate.index === implementation.traitIndex);
            const method = trait?.methods.find((candidate) => candidate.name === methodName);
            const mapping = method && implementation.methodFunctions.find((candidate) => candidate.methodIndex === method.index);
            return trait && method && mapping ? [{ trait, method, mapping }] : [];
          });
          if (candidates.length > 1) this.fail("ambiguous-method", `method '${expression.callee.name}' is supplied by multiple traits`, expression.callee.span);
          const candidate = candidates[0];
          if (candidate) {
            if (candidate.method.receiverMutable && mutableInner(receiver.type) === undefined) {
              this.fail("mutable-receiver-required", `method '${candidate.method.name}' requires mutable access to ${receiverImplementationType}`, expression.callee.receiver.span);
            }
            const receiverParameterType = candidate.method.receiverMutable
              ? mutableType(receiverImplementationType)
              : receiverImplementationType;
            const methodReceiver = this.requireCoercion(receiver, receiverParameterType, receiver.span);
            const checkedArguments = this.checkConcreteArguments(expression, candidate.method.parameters, candidate.method.parameterNames, candidate.method.variadic, `method '${candidate.method.name}'`);
            const signature = [...this.signatures.values()].find((value) => value.index === candidate.mapping.functionIndex)!;
            const providers = signature.requirements.map((requirement) => this.resolveProvider(requirement, expression.span));
            const missing = signature.requirements.filter((_, index) => !providers[index]);
            if (missing.length > 0) this.fail("missing-requirement", `method '${candidate.method.name}' requires ${missing.join(" + ")}`, expression.span);
            const implementationArgumentParameterIndices = checkedArguments.parameterIndices
              ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
              : undefined;
            return candidate.method.suspending
              ? {
                  kind: "suspend-construct",
                  functionIndex: signature.index,
                  functionName: signature.name,
                  arguments: [methodReceiver, ...checkedArguments.arguments],
                  argumentParameterIndices: implementationArgumentParameterIndices,
                  providers: providers as HirExpression[],
                  type: suspensionType(signature.index, signature.result),
                  span: expression.span,
                }
              : { kind: "call", functionIndex: signature.index, functionName: signature.name, arguments: [methodReceiver, ...checkedArguments.arguments], argumentParameterIndices: implementationArgumentParameterIndices, providers: providers as HirExpression[], type: signature.result, span: expression.span };
          }
          this.fail("unknown-method", `type '${receiver.type}' has no supported method '${expression.callee.name}'`, expression.callee.span);
        }
        if (expression.callee.kind !== "name" || this.resolveLocal(expression.callee.name) || this.availableCaptures.has(expression.callee.name)) {
          if (expression.argumentNames?.some((name) => name !== undefined)) {
            this.fail("named-argument-needs-declaration", "named arguments require a statically known function or method declaration", expression.span);
          }
          const callee = this.checkExpression(expression.callee);
          const callable = functionParts(callee.type);
          if (!callable) this.fail("not-callable", `type '${callee.type}' is not callable`, expression.callee.span);
          const parameterNames = callable.parameters.map((_, index) => `$${index}`);
          const checkedArguments = this.checkConcreteArguments(expression, callable.parameters, parameterNames, callable.variadic, "function value");
          const providers = callable.requirements.map((requirement) => this.resolveProvider(requirement, expression.span));
          const missing = callable.requirements.filter((_, index) => !providers[index]);
          if (missing.length > 0) {
            this.fail("missing-requirement", `closure call requires ${missing.join(" + ")}`, expression.span);
          }
          return { kind: "closure-call", callee, arguments: checkedArguments.arguments, providers: providers as HirExpression[], type: callable.result, span: expression.span };
        }
        if (this.imports.get(expression.callee.name) === "std.task.block_on") {
          if (this.deferDepth > 0 || this.moduleBody) {
            this.fail("suspension-forbidden-context", "block_on cannot start a suspension driver in this context", expression.span);
          }
          if (expression.typeArguments?.length) this.fail("unexpected-type-arguments", "block_on infers its result type", expression.span);
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "block_on has no variadic parameter", expression.span);
          if (expression.arguments.length !== 1) this.fail("argument-count", "block_on expects one mutable suspension", expression.span);
          this.resolveArgumentMapping(expression, ["s"], "block_on");
          const source = expression.arguments[0]!;
          this.requireDrivableSuspension(source);
          const suspension = this.checkExpression(source);
          const parts = suspensionParts(suspension.type);
          if (parts) {
            const signature = [...this.signatures.values()].find((candidate) => candidate.index === parts.functionIndex);
            return {
              kind: "suspend-drive",
              functionIndex: parts.functionIndex,
              suspension,
              erasedResultType: signature?.genericParameters.length ? signature.result : undefined,
              type: parts.result,
              span: expression.span,
            };
          }
          const traitParts = traitSuspensionParts(suspension.type);
          if (traitParts) {
            return {
              kind: "trait-suspend-drive",
              traitIndex: traitParts.traitIndex,
              methodIndex: traitParts.methodIndex,
              suspension,
              type: traitParts.result,
              span: expression.span,
            };
          }
          this.fail("type-mismatch", `block_on expects mut Suspend[T], found ${suspension.type}`, source.span);
        }
        if (this.imports.get(expression.callee.name) === "std.testing.assert_equal") {
          if (expression.typeArguments?.length) this.fail("unexpected-type-arguments", "assert_equal infers its value type", expression.span);
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "assert_equal has no variadic parameter", expression.span);
          if (expression.arguments.length !== 3) this.fail("argument-count", "assert_equal expects actual, expected, and reason", expression.span);
          const mapping = this.resolveArgumentMapping(expression, ["actual", "expected", "reason"], "assert_equal");
          const parameterIndex = (argumentIndex: number): number => mapping?.[argumentIndex] ?? argumentIndex;
          const sourceIndex = (parameter: number): number => expression.arguments.findIndex((_, index) => parameterIndex(index) === parameter);
          const actualIndex = sourceIndex(0);
          const expectedIndex = sourceIndex(1);
          const reasonIndex = sourceIndex(2);
          const actual = this.checkExpression(expression.arguments[actualIndex]!);
          if (!supportsMvpEquality(actual.type)) {
            this.fail("missing-partial-eq", `type '${actual.type}' does not implement PartialEq in the executable MVP`, actual.span);
          }
          const checkedByParameter = [
            actual,
            this.requireCoercion(this.checkExpression(expression.arguments[expectedIndex]!, actual.type), actual.type, expression.arguments[expectedIndex]!.span),
            this.requireCoercion(this.checkExpression(expression.arguments[reasonIndex]!, "string"), "string", expression.arguments[reasonIndex]!.span),
          ];
          const arguments_ = expression.arguments.map((_, index) => checkedByParameter[parameterIndex(index)]!);
          return { kind: "assert-equal", arguments: arguments_, argumentParameterIndices: mapping, valueType: actual.type, type: "void", span: expression.span };
        }
        if (expression.callee.name === "Ok" || expression.callee.name === "Err") {
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", `${expression.callee.name} has no variadic parameter`, expression.span);
          const parts = expected && resultParts(expected);
          if (!parts) this.fail("result-constructor-needs-context", `${expression.callee.name} requires an expected Result type`, expression.span);
          const ok = expression.callee.name === "Ok";
          const payloadType = ok ? parts.ok : parts.error;
          const expectedCount = ok && payloadType === "void" ? 0 : 1;
          if (expression.arguments.length !== expectedCount) {
            this.fail("argument-count", `${expression.callee.name} expects ${expectedCount} argument${expectedCount === 1 ? "" : "s"}`, expression.span);
          }
          this.resolveArgumentMapping(expression, expectedCount === 0 ? [] : [ok ? "value" : "error"], expression.callee.name);
          const payload = expectedCount === 1 ? this.checkExpression(expression.arguments[0]!, payloadType) : undefined;
          return {
            kind: "variant-wrap",
            variant: ok ? "result-ok" : "result-error",
            payload,
            payloadType,
            type: expected,
            span: expression.span,
          };
        }
        if (expression.callee.name === "panic") {
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "panic has no variadic parameter", expression.span);
          if (expression.arguments.length !== 1) this.fail("argument-count", "panic expects one message argument", expression.span);
          this.resolveArgumentMapping(expression, ["message"], "panic");
          const message = this.checkExpression(expression.arguments[0]!);
          this.requireAssignable(message.type, "string", message.span);
          return { kind: "panic", message, type: "never", span: expression.span };
        }
        if (expression.callee.name === "println") {
          if (expression.typeArguments) this.fail("unexpected-type-arguments", "println infers its Display type", expression.span);
          if (expression.argumentSpreads?.some(Boolean)) this.fail("positional-spread-needs-vararg", "println has no variadic parameter", expression.span);
          if (expression.arguments.length !== 1) this.fail("argument-count", "println expects one value argument", expression.span);
          this.resolveArgumentMapping(expression, ["value"], "println");
          const operand = this.checkExpression(expression.arguments[0]!);
          const value: HirExpression = operand.type === "string"
            ? operand
            : ["i32", "bool", "char"].includes(operand.type)
              ? { kind: "display", operand, type: "string", span: operand.span }
              : this.fail("missing-display", `type '${operand.type}' does not implement Display in the executable MVP`, operand.span);
          const provider = this.resolveProvider("Console", expression.span);
          if (!provider) this.fail("missing-requirement", "println requires Console", expression.span);
          return { kind: "console-print", provider, value, type: "void", span: expression.span };
        }
        if (expression.callee.name.startsWith("$enum-literal.")) {
          return this.checkInternalEnumLiteral(expression, expression.callee.name, expected);
        }
        const signature = this.signatures.get(expression.callee.name);
        if (!signature) this.fail("unknown-name", `unknown function '${expression.callee.name}'`, expression.callee.span);
        const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
        const { substitutions, rowSubstitutions } = checkedArguments;
        const unresolved = signature.genericParameters.filter((parameter) => !substitutions.has(parameter));
        if (unresolved.length > 0) this.fail("unresolved-generic-placeholder", `could not infer generic parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`, expression.span);
        const unresolvedRows = signature.rowParameters.filter((parameter) => !rowSubstitutions.has(parameter));
        if (unresolvedRows.length > 0) this.fail("unresolved-generic-placeholder", `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`, expression.span);
        this.warnAbsentRowSubtractions(signature.requirements, rowSubstitutions, expression.span);
        const { providers, missing } = this.resolveCallProviders(signature.requirements, substitutions, rowSubstitutions, expression.span);
        if (missing.length > 0) {
          this.fail("missing-requirement", `call to '${signature.name}' requires ${missing.join(" + ")}`, expression.span);
        }
        const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
        const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
        const defaultArguments = checkedArguments.defaultParameterIndices.map((parameterIndex) => ({
          parameterIndex,
          functionIndex: this.signatures.get(signature.defaultFunctionNames[parameterIndex]!)!.index,
        }));
        return signature.suspending
          ? {
              kind: "suspend-construct",
              functionIndex: signature.index,
              functionName: signature.name,
              arguments: checkedArguments.arguments,
              argumentParameterIndices: checkedArguments.parameterIndices,
              defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
              parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
              bounds,
              providers,
              erasedParameterTypes: signature.genericParameters.length > 0 || signature.rowParameters.length > 0 ? signature.parameters : undefined,
              type: suspensionType(signature.index, resultType),
              span: expression.span,
            }
          : {
              kind: "call",
              functionIndex: signature.index,
              functionName: signature.name,
              arguments: checkedArguments.arguments,
              argumentParameterIndices: checkedArguments.parameterIndices,
              defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
              parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
              bounds,
              providers,
              erasedParameterTypes: signature.genericParameters.length > 0 || signature.rowParameters.length > 0 ? signature.parameters : undefined,
              erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
              type: resultType,
              span: expression.span,
            };
      }
      case "suspend-call": {
        if (this.deferDepth > 0) {
          this.fail("suspending-defer", "a defer suite cannot make a bang call", expression.span);
        }
        if (!this.declaration.suspending) {
          this.fail("bang-call-outside-suspension", "a bang call requires a suspending driver context", expression.span);
        }
        if (expression.callee.kind === "member") {
          const suspension = this.checkExpression({
            kind: "call",
            callee: expression.callee,
            arguments: expression.arguments,
            argumentNames: expression.argumentNames,
            argumentSpreads: expression.argumentSpreads,
            span: expression.span,
          });
          if (suspension.kind === "suspend-construct") {
            const result = suspensionParts(suspension.type)!.result;
            return { kind: "suspend-drive", functionIndex: suspension.functionIndex, suspension, type: result, span: expression.span };
          }
          if (suspension.kind === "trait-suspend-construct") {
            const parts = traitSuspensionParts(suspension.type)!;
            return {
              kind: "trait-suspend-drive",
              traitIndex: parts.traitIndex,
              methodIndex: parts.methodIndex,
              suspension,
              type: parts.result,
              span: expression.span,
            };
          }
          this.fail("not-suspending", `method '${expression.callee.name}' is not suspending`, expression.span);
        }
        if (expression.callee.kind === "name" && !this.resolveLocal(expression.callee.name)) {
          const signature = this.signatures.get(expression.callee.name);
          if (!signature && (expression.callee.name === "all" || expression.callee.name === "race")) {
            this.fail(
              "unsupported-task-combinator",
              `std.task.${expression.callee.name}! remains unavailable until its standard signature is resolved`,
              expression.callee.span,
            );
          }
          if (!signature) this.fail("unknown-name", `unknown function '${expression.callee.name}'`, expression.callee.span);
          if (!signature.suspending) this.fail("not-suspending", `function '${signature.name}' is not suspending`, expression.span);
          const checkedArguments = this.checkSignatureArguments(expression, signature, expected);
          const { substitutions, rowSubstitutions } = checkedArguments;
          const unresolved = signature.genericParameters.filter((parameter) => !substitutions.has(parameter));
          if (unresolved.length > 0) this.fail("unresolved-generic-placeholder", `could not infer generic parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`, expression.span);
          const unresolvedRows = signature.rowParameters.filter((parameter) => !rowSubstitutions.has(parameter));
          if (unresolvedRows.length > 0) this.fail("unresolved-generic-placeholder", `could not infer requirement-row parameter${unresolvedRows.length === 1 ? "" : "s"} ${unresolvedRows.join(", ")}`, expression.span);
          this.warnAbsentRowSubtractions(signature.requirements, rowSubstitutions, expression.span);
          const { providers, missing } = this.resolveCallProviders(signature.requirements, substitutions, rowSubstitutions, expression.span);
          if (missing.length > 0) this.fail("missing-requirement", `call to '${signature.name}' requires ${missing.join(" + ")}`, expression.span);
          const resultType = substituteGenericType(signature.result, substitutions, rowSubstitutions);
          const bounds = this.resolveBoundDictionaries(signature, substitutions, expression.span);
          const defaultArguments = checkedArguments.defaultParameterIndices.map((parameterIndex) => ({
            parameterIndex,
            functionIndex: this.signatures.get(signature.defaultFunctionNames[parameterIndex]!)!.index,
          }));
          const suspension: HirExpression = {
            kind: "suspend-construct",
            functionIndex: signature.index,
            functionName: signature.name,
            arguments: checkedArguments.arguments,
            argumentParameterIndices: checkedArguments.parameterIndices,
            defaultArguments: defaultArguments.length > 0 ? defaultArguments : undefined,
            parameterTypes: defaultArguments.length > 0 ? signature.parameters : undefined,
            bounds,
            providers,
            erasedParameterTypes: signature.genericParameters.length > 0 || signature.rowParameters.length > 0 ? signature.parameters : undefined,
            type: suspensionType(signature.index, resultType),
            span: expression.span,
          };
          return {
            kind: "suspend-drive",
            functionIndex: signature.index,
            suspension,
            erasedResultType: signature.genericParameters.length > 0 ? signature.result : undefined,
            type: resultType,
            span: expression.span,
          };
        }
        if (expression.arguments.length !== 0) this.fail("argument-count", "driving a stored suspension takes no arguments", expression.span);
        this.requireDrivableSuspension(expression.callee);
        const suspension = this.checkExpression(expression.callee);
        const parts = suspensionParts(suspension.type);
        if (parts) {
          const signature = [...this.signatures.values()].find((candidate) => candidate.index === parts.functionIndex);
          return {
            kind: "suspend-drive",
            functionIndex: parts.functionIndex,
            suspension,
            erasedResultType: signature?.genericParameters.length ? signature.result : undefined,
            type: parts.result,
            span: expression.span,
          };
        }
        const traitParts = traitSuspensionParts(suspension.type);
        if (traitParts) {
          return {
            kind: "trait-suspend-drive",
            traitIndex: traitParts.traitIndex,
            methodIndex: traitParts.methodIndex,
            suspension,
            type: traitParts.result,
            span: expression.span,
          };
        }
        this.fail("not-suspending", `type '${suspension.type}' is not a suspension`, expression.span);
      }
      case "data": {
        const declaration = this.dataTypes.get(expression.name);
        if (!declaration) this.fail("unknown-type", `unknown data type '${expression.name}'`, expression.span);
        const outerMutableExpected = expected !== undefined && mutableInner(expected) !== undefined;
        const supplied = new Map<string, Expression>();
        for (const field of expression.fields) {
          if (supplied.has(field.name)) this.fail("duplicate-field", `field '${field.name}' is supplied more than once`, field.span);
          if (!declaration.fields.some((candidate) => candidate.name === field.name)) {
            this.fail("unknown-data-field", `type '${declaration.name}' has no field '${field.name}'`, field.span);
          }
          supplied.set(field.name, field.value);
        }
        const substitutions = new Map<string, ValueType>();
        const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
        if (expectedNominal?.name === declaration.name && expectedNominal.arguments.length === declaration.genericParameters.length) {
          declaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, expectedNominal.arguments[index]!));
        }
        const spread = expression.spread ? this.checkExpression(expression.spread) : undefined;
        if (spread) {
          if (declaration.genericParameters.length === 0) {
            this.requireAssignable(spread.type, declaration.name, expression.spread!.span);
          } else {
            const spreadNominal = nominalGenericParts(spread.type);
            if (spreadNominal?.name !== declaration.name || spreadNominal.arguments.length !== declaration.genericParameters.length) {
              this.fail("type-mismatch", `copy-update for '${declaration.name}' requires the same data type, found ${spread.type}`, expression.spread!.span);
            }
            const conflict = inferGenericType(
              nominalGenericType(declaration.name, declaration.genericParameters),
              spread.type,
              substitutions,
            );
            if (conflict) this.fail("generic-type-mismatch", conflict, expression.spread!.span);
          }
        }
        const missingFields = declaration.fields.filter((field) => !supplied.has(field.name));
        const missingRequired = expression.spread ? undefined : missingFields.find((field) => !field.defaultFunctionName);
        if (missingRequired) this.fail("missing-required-field", `missing required field '${missingRequired.name}'`, expression.span);
        const initiallyChecked = expression.fields.map((entry) => {
          const field = declaration.fields.find((candidate) => candidate.name === entry.name)!;
          const value = entry.value;
          const inferredField = substituteGenericType(field.type, substitutions);
          const directMutable = mutableInner(field.type) !== undefined;
          const contextualField = directMutable && !outerMutableExpected
            ? expected === undefined ? undefined : mutableInner(inferredField)
            : inferredField;
          const checked = this.checkExpression(value, contextualField && !containsGenericType(contextualField) ? contextualField : undefined);
          const conflict = inferGenericType(field.type, checked.type, substitutions);
          if (conflict) {
            if (mutableInner(inferredField) === checked.type) {
              this.fail("mutable-upgrade", `readonly type '${checked.type}' cannot initialize generic field '${inferredField}'`, value.span);
            }
            this.fail("generic-type-mismatch", conflict, value.span);
          }
          return checked;
        });
        const unresolved = declaration.genericParameters.filter((parameter) => !substitutions.has(parameter));
        if (unresolved.length > 0) this.fail("unresolved-generic-placeholder", `could not infer data parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`, expression.span);
        const explicitFieldIndices = expression.fields.map((entry) => declaration.fields.find((field) => field.name === entry.name)!.index);
        const explicitFields = initiallyChecked.map((field, sourceIndex) => {
          const declarationField = declaration.fields[explicitFieldIndices[sourceIndex]!]!;
          const fieldType = substituteGenericType(declarationField.type, substitutions);
          const directMutable = mutableInner(declarationField.type) !== undefined;
          if (directMutable && !outerMutableExpected && field.type === mutableInner(fieldType)) return field;
          return this.requireCoercion(field, fieldType, field.span);
        });
        const defaultFields = (expression.spread ? [] : missingFields).map((field) => {
          const expectedField = substituteGenericType(field.type, substitutions);
          const call: Expression = {
            kind: "call",
            callee: { kind: "name", name: field.defaultFunctionName!, span: expression.span },
            arguments: [],
            span: expression.span,
          };
          return this.requireCoercion(this.checkExpression(call, expectedField), expectedField, expression.span);
        });
        const fields = [...explicitFields, ...defaultFields];
        const fieldIndices = [...explicitFieldIndices, ...(expression.spread ? [] : missingFields.map((field) => field.index))];
        const readonlyResult = declaration.genericParameters.length > 0
          ? nominalGenericType(declaration.name, declaration.genericParameters.map((parameter) => substitutions.get(parameter)!))
          : declaration.name;
        const mutableDirectFields = declaration.fields.filter((field) => mutableInner(field.type) !== undefined);
        const explicitByIndex = new Map(explicitFieldIndices.map((fieldIndex, sourceIndex) => [fieldIndex, explicitFields[sourceIndex]!] as const));
        const canProduceMutable = mutableDirectFields.every((field) => {
          const explicit = explicitByIndex.get(field.index);
          if (explicit) return explicit.type === substituteGenericType(field.type, substitutions);
          if (expression.spread) return mutableInner(spread!.type) !== undefined;
          return true;
        });
        if (outerMutableExpected && !canProduceMutable) {
          this.fail("mutable-upgrade", `construction of '${readonlyResult}' does not retain mutable access for every direct mutable field`, expression.span);
        }
        const type = outerMutableExpected || (expected === undefined && canProduceMutable)
          ? mutableType(readonlyResult)
          : readonlyResult;
        return {
          kind: "data",
          dataIndex: declaration.index,
          spread,
          fields,
          fieldIndices,
          erasedFieldTypes: declaration.genericParameters.length > 0 ? declaration.fields.map((field) => field.type) : undefined,
          type,
          span: expression.span,
        };
      }
      case "member": {
        if (expression.receiver.kind === "name") {
          const enumType = this.enumTypes.get(expression.receiver.name);
          if (enumType) {
            const variant = enumType.variants.find((candidate) => candidate.name === expression.name);
            if (variant && variant.fields.length > 0) {
              this.fail("unsaturated-enum-constructor", `variant '${variant.name}' requires ${variant.fields.length} argument${variant.fields.length === 1 ? "" : "s"}`, expression.span);
            }
            return this.checkEnumConstructor(enumType, expression.name, { kind: "call", callee: expression, arguments: [], span: expression.span }, expected);
          }
        }
        const receiver = this.checkExpression(expression.receiver);
        const receiverReadonly = readonlyType(receiver.type);
        const tuple = tupleParts(receiverReadonly);
        if (tuple) {
          if (!/^[0-9]+$/.test(expression.name)) {
            this.fail("unknown-tuple-member", `tuple type '${receiver.type}' has no member '${expression.name}'`, expression.span);
          }
          const index = Number(expression.name);
          if (!Number.isSafeInteger(index) || index >= tuple.length) {
            this.fail("tuple-index-range", `tuple index ${expression.name} is outside a ${tuple.length}-element tuple`, expression.span);
          }
          return { kind: "tuple-index", receiver, index, elementType: tuple[index]!, type: tuple[index]!, span: expression.span };
        }
        const nominal = nominalGenericParts(receiverReadonly);
        const typeName = nominal?.name ?? receiverReadonly;
        const dataDeclaration = this.dataTypes.get(typeName);
        if (dataDeclaration) {
          const field = dataDeclaration.fields.find((candidate) => candidate.name === expression.name);
          if (!field) this.fail("unknown-data-field", `type '${dataDeclaration.name}' has no field '${expression.name}'`, expression.span);
          const substitutions = new Map<string, ValueType>();
          if (nominal) dataDeclaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, nominal.arguments[index]!));
          const declaredType = substituteGenericType(field.type, substitutions);
          const type = mutableInner(receiver.type) !== undefined || genericTypeName(field.type)
            ? declaredType
            : readonlyType(declaredType);
          return { kind: "member", receiver, dataIndex: dataDeclaration.index, fieldIndex: field.index, erasedFieldType: genericTypeName(field.type) ? field.type : undefined, type, span: expression.span };
        }
        const enumDeclaration = this.enumTypes.get(typeName);
        if (enumDeclaration) {
          const field = enumDeclaration.sharedFields.find((candidate) => candidate.name === expression.name);
          if (!field) this.fail("unknown-data-field", `enum '${enumDeclaration.name}' has no shared field '${expression.name}'`, expression.span);
          const substitutions = new Map<string, ValueType>();
          if (nominal) enumDeclaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, nominal.arguments[index]!));
          const type = substituteGenericType(field.type, substitutions);
          return { kind: "enum-member", receiver, enumIndex: enumDeclaration.index, fieldIndex: field.index, erasedFieldType: genericTypeName(field.type) ? field.type : undefined, type, span: expression.span };
        }
        this.fail("member-on-non-data", `type '${receiver.type}' has no data fields`, expression.receiver.span);
      }
      case "contextual-variant": {
        const nominal = expected ? nominalGenericParts(expected) : undefined;
        const declaration = expected && this.enumTypes.get(nominal?.name ?? expected);
        if (!declaration) {
          this.fail("missing-contextual-enum-type", `variant '.${expression.name}' requires an expected enum type`, expression.span);
        }
        const variant = declaration.variants.find((candidate) => candidate.name === expression.name);
        if (variant && variant.fields.length > 0) {
          this.fail("unsaturated-enum-constructor", `variant '${variant.name}' requires ${variant.fields.length} argument${variant.fields.length === 1 ? "" : "s"}`, expression.span);
        }
        return this.checkEnumConstructor(declaration, expression.name, { kind: "call", callee: expression, arguments: [], span: expression.span }, expected);
      }
      case "index": {
        const receiver = this.checkExpression(expression.receiver);
        const nominal = nominalGenericParts(readonlyType(receiver.type));
        if (nominal?.name === "list" && nominal.arguments.length === 1) {
          const index = this.checkExpression(expression.index, "i32");
          this.requireAssignable(index.type, "i32", expression.index.span);
          return { kind: "list-index", receiver, index, elementType: nominal.arguments[0]!, type: nominal.arguments[0]!, span: expression.span };
        }
        if (nominal?.name === "map" && nominal.arguments.length === 2) {
          const key = this.checkExpression(expression.index, nominal.arguments[0]);
          this.requireAssignable(key.type, nominal.arguments[0]!, expression.index.span);
          return {
            kind: "map-index",
            receiver,
            key,
            keyType: nominal.arguments[0]!,
            valueType: nominal.arguments[1]!,
            type: `${nominal.arguments[1]}?`,
            span: expression.span,
          };
        }
        if (receiver.type === "string") {
          this.fail("unsupported-string-indexing", "strings are not indexable; iterate Unicode scalars explicitly", expression.span);
        }
        this.fail("not-indexable", `type '${receiver.type}' does not support indexing`, expression.receiver.span);
      }
      case "if": {
        const condition = this.checkExpression(expression.condition);
        this.requireType(condition.type, "bool", condition.span);
        const thenBody = this.checkStatements(expression.thenBody, true, expected);
        const elseBody = expression.elseBody.length > 0 ? this.checkStatements(expression.elseBody, true, expected) : [];
        if (elseBody.length === 0) {
          return { kind: "if", condition, thenBody, elseBody, type: "void", span: expression.span };
        }
        const thenType = this.blockType(thenBody);
        const elseType = this.blockType(elseBody);
        if (thenType !== elseType && thenType !== "never" && elseType !== "never") {
          this.fail("if-branch-type", `if branches have types ${thenType} and ${elseType}`, expression.span);
        }
        const type = thenType === "never" ? elseType : thenType;
        return { kind: "if", condition, thenBody, elseBody, type, span: expression.span };
      }
      case "for": {
        const iterable = this.checkExpression(expression.iterable);
        const nominal = nominalGenericParts(readonlyType(iterable.type));
        let iteratorKind: "list" | "map";
        let yieldType: ValueType;
        if (nominal?.name === "list" && nominal.arguments.length === 1) {
          iteratorKind = "list";
          yieldType = nominal.arguments[0]!;
        } else if (nominal?.name === "map" && nominal.arguments.length === 2) {
          iteratorKind = "map";
          yieldType = tupleType(nominal.arguments);
        } else {
          this.fail("not-iterable", `type '${iterable.type}' does not implement the MVP iteration protocol`, expression.iterable.span);
        }
        const bindingTypes = expression.bindings.length === 1 ? [yieldType] : tupleParts(yieldType);
        if (!bindingTypes || bindingTypes.length !== expression.bindings.length) {
          this.fail("for-binding-arity", `loop binding has ${expression.bindings.length} names but '${yieldType}' yields ${bindingTypes?.length ?? 1} value${bindingTypes?.length === 1 ? "" : "s"}`, expression.span);
        }
        const elseBody = expression.elseBody.length > 0 ? this.checkStatements(expression.elseBody, true, expected) : [];
        const result = elseBody.length > 0 ? this.blockType(elseBody) : undefined;
        this.loopResults.push(result);
        this.scopes.push(new Map());
        let bindings: HirLocal[];
        let body: readonly HirStatement[];
        try {
          const seen = new Set<string>();
          bindings = expression.bindings.map((binding, index) => {
            if (seen.has(binding.name)) this.fail("duplicate-binding", `loop binding '${binding.name}' appears more than once`, binding.span);
            seen.add(binding.name);
            if (PRELUDE_NAMES.has(binding.name)) this.fail("prelude-name-shadow", `loop binding '${binding.name}' shadows a prelude name`, binding.span);
            const local: HirLocal = { name: binding.name, type: bindingTypes[index]!, index: this.locals.length, mutable: false, parameter: false, span: binding.span };
            this.locals.push(local);
            this.currentScope().set(binding.name, local);
            return local;
          });
          body = this.checkStatements(expression.body, false);
        } finally {
          this.scopes.pop();
          this.loopResults.pop();
        }
        return { kind: "for", iterable, iteratorKind, yieldType, bindings, body, elseBody, type: result ?? "void", span: expression.span };
      }
      case "while": {
        const condition = this.checkExpression(expression.condition);
        this.requireType(condition.type, "bool", condition.span);
        const elseBody = expression.elseBody.length > 0 ? this.checkStatements(expression.elseBody, true, expected) : [];
        const result = elseBody.length > 0 ? this.blockType(elseBody) : undefined;
        this.loopResults.push(result);
        let body: readonly HirStatement[];
        try {
          body = this.checkStatements(expression.body, true);
        } finally {
          this.loopResults.pop();
        }
        return { kind: "while", condition, body, elseBody, type: result ?? "void", span: expression.span };
      }
      case "match": {
        const subject = this.checkExpression(expression.subject);
        const subjectNominal = nominalGenericParts(subject.type);
        const declaration = this.enumTypes.get(subjectNominal?.name ?? subject.type);
        const dataDeclaration = this.dataTypes.get(subject.type);
        const optional = optionalInner(subject.type);
        const result = resultParts(subject.type);
        const boolean = subject.type === "bool";
        const scalar = new Set<ValueType>(["bool", "i32", "f64", "char", "string"]).has(subject.type);
        if (!declaration && !dataDeclaration && optional === undefined && !result && !scalar) {
          this.fail("unsupported-match-subject", `matching '${subject.type}' is not implemented in this MVP slice`, expression.subject.span);
        }
        const covered = new Set<number | string>();
        let catchAll = false;
        let resultType: ValueType | undefined;
        const arms: HirMatchArm[] = [];
        for (const arm of expression.arms) {
          if (catchAll) this.fail("unreachable-match-arm", "a match arm follows an unguarded catch-all", arm.span);
          this.scopes.push(new Map());
          const bindings: { local: HirLocal; fieldIndex: number; type: ValueType; erasedFieldType?: ValueType; path?: readonly { dataIndex: number; fieldIndex: number }[]; enumFieldIndex?: number; enumFieldType?: ValueType; enumErasedFieldType?: ValueType; accessPath?: readonly HirPatternAccessStep[] }[] = [];
          const tests: { path?: readonly { dataIndex: number; fieldIndex: number }[]; enumFieldIndex?: number; erasedFieldType?: ValueType; valueType?: ValueType; accessPath?: readonly HirPatternAccessStep[]; tag?: number; tagEnumIndex?: number; literal?: HirExpression }[] = [];
          let tag: number | undefined;
          let literal: HirExpression | undefined;
          const guarded = arm.guard !== undefined;
          try {
            if (dataDeclaration && arm.pattern.kind === "data") {
              const irrefutable = this.checkDataPattern(arm.pattern, dataDeclaration, [], bindings, tests);
              if (irrefutable && !guarded) catchAll = true;
            } else if (scalar && ["boolean", "integer", "float", "string", "character"].includes(arm.pattern.kind)) {
              literal = this.checkExpression(arm.pattern as Extract<Expression, { kind: "boolean" | "integer" | "float" | "string" | "character" }>);
              this.requireType(literal.type, subject.type, arm.pattern.span);
              const key = literal.kind === "string"
                ? `string:${literal.bytes.join(",")}`
                : `${literal.type}:${"value" in literal ? literal.value : ""}`;
              if (covered.has(key)) this.fail("unreachable-match-arm", "literal pattern is already covered", arm.pattern.span);
              if (!guarded) covered.add(key);
            } else if (declaration && arm.pattern.kind === "variant") {
              const pattern = arm.pattern;
              if (pattern.enumName !== undefined && pattern.enumName !== declaration.name) {
                this.fail("pattern-type-mismatch", `pattern names '${pattern.enumName}', expected '${declaration.name}'`, pattern.span);
              }
              const variant = declaration.variants.find((candidate) => candidate.name === pattern.variantName);
              if (!variant) this.fail("unknown-variant", `enum '${declaration.name}' has no variant '${pattern.variantName}'`, pattern.span);
              if (covered.has(variant.tag)) this.fail("unreachable-match-arm", `variant '${variant.name}' is already covered`, pattern.span);
              const payloadPatterns = pattern.payloadPatterns ?? pattern.bindings.map((name) => name
                ? { kind: "binding" as const, name, span: pattern.span }
                : { kind: "wildcard" as const, span: pattern.span });
              if (payloadPatterns.length !== variant.fields.length) {
                this.fail("pattern-arity", `variant '${variant.name}' expects ${variant.fields.length} payload patterns`, pattern.span);
              }
              const bindingNames = pattern.bindingNames ?? payloadPatterns.map(() => undefined);
              let nextPositional = 0;
              const seenFields = new Set<number>();
              const fieldIndices = bindingNames.map((fieldName) => {
                const fieldIndex = fieldName === undefined
                  ? nextPositional++
                  : variant.fields.findIndex((field) => field.name === fieldName);
                if (fieldIndex < 0) this.fail("unknown-variant-pattern-field", `variant '${variant.name}' has no payload field '${fieldName}'`, pattern.span);
                if (seenFields.has(fieldIndex)) this.fail("duplicate-variant-pattern-field", `payload field '${variant.fields[fieldIndex]!.name}' appears more than once`, pattern.span);
                seenFields.add(fieldIndex);
                return fieldIndex;
              });
              tag = variant.tag;
              let payloadRefutable = false;
              const substitutions = new Map<string, ValueType>();
              if (subjectNominal) declaration.genericParameters.forEach((parameter, parameterIndex) => substitutions.set(parameter, subjectNominal.arguments[parameterIndex]!));
              payloadPatterns.forEach((payloadPattern, index) => {
                const fieldIndex = fieldIndices[index]!;
                const field = variant.fields[fieldIndex]!;
                const fieldType = substituteGenericType(field.type, substitutions);
                if (payloadPattern.kind === "binding") {
                  const name = payloadPattern.name;
                  if (bindingNames[index] === undefined && name !== field.name && variant.fields.some((candidate, candidateIndex) => candidateIndex !== fieldIndex && candidate.name === name)) {
                    this.diagnostics.push({
                      code: "variant-binding-name-mismatch",
                      message: `positional binding '${name}' occupies payload field '${field.name}'`,
                      span: payloadPattern.span,
                      severity: "warning",
                    });
                  }
                }
                const accessPath: HirPatternAccessStep[] = [{
                  kind: "enum",
                  typeIndex: declaration.index,
                  fieldIndex: field.index,
                  erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
                  valueType: fieldType,
                }];
                payloadRefutable ||= !this.checkNestedPattern(payloadPattern, fieldType, accessPath, bindings, tests);
              });
              if (!guarded && !payloadRefutable) covered.add(tag);
            } else if (optional !== undefined && arm.pattern.kind === "nil") {
              tag = 0;
              if (covered.has(tag)) this.fail("unreachable-match-arm", "nil is already covered", arm.pattern.span);
              if (!guarded) covered.add(tag);
            } else if (optional !== undefined && arm.pattern.kind === "optional-present") {
              tag = 1;
              if (covered.has(tag)) this.fail("unreachable-match-arm", "the present optional case is already covered", arm.pattern.span);
              if (!guarded) covered.add(tag);
              bindings.push({ local: this.addPatternLocal(arm.pattern.name, optional, arm.pattern.span), fieldIndex: 0, type: optional });
            } else if (result && arm.pattern.kind === "result-variant") {
              const ok = arm.pattern.variantName === "Ok";
              tag = ok ? 0 : 1;
              if (covered.has(tag)) this.fail("unreachable-match-arm", `${arm.pattern.variantName} is already covered`, arm.pattern.span);
              const payloadType = ok ? result.ok : result.error;
              const payloadPatterns = arm.pattern.payloadPatterns ?? arm.pattern.bindings.map((name) => name
                ? { kind: "binding" as const, name, span: arm.pattern.span }
                : { kind: "wildcard" as const, span: arm.pattern.span });
              const voidSuccess = ok && payloadType === "void";
              if (payloadPatterns.length !== (voidSuccess ? 0 : 1)
                && !(voidSuccess && payloadPatterns.length === 1 && payloadPatterns[0]?.kind === "wildcard")) {
                this.fail("pattern-arity", `${arm.pattern.variantName} expects ${voidSuccess ? 0 : 1} payload patterns`, arm.pattern.span);
              }
              let payloadRefutable = false;
              if (!voidSuccess) {
                const payloadPattern = payloadPatterns[0]!;
                if (payloadPattern.kind === "binding") {
                  bindings.push({ local: this.addPatternLocal(payloadPattern.name, payloadType, payloadPattern.span), fieldIndex: 0, type: payloadType });
                } else if (payloadPattern.kind !== "wildcard") {
                  const accessPath: HirPatternAccessStep[] = [{
                    kind: "erased-variant",
                    typeIndex: -1,
                    fieldIndex: 0,
                    valueType: payloadType,
                  }];
                  payloadRefutable = !this.checkNestedPattern(payloadPattern, payloadType, accessPath, bindings, tests);
                }
              }
              if (!guarded && !payloadRefutable) covered.add(tag);
            } else if (arm.pattern.kind === "wildcard" || arm.pattern.kind === "binding") {
              const bindingName = arm.pattern.kind === "binding" ? arm.pattern.name : undefined;
              if (declaration && bindingName !== undefined && declaration.variants.some((variant) => variant.name === bindingName)) {
                this.fail("bare-variant-pattern", `bare variant '${bindingName}' must be written as '.${bindingName}' or '${declaration.name}.${bindingName}'`, arm.pattern.span);
              }
              if (!guarded) catchAll = true;
              if (arm.pattern.kind === "binding") {
                bindings.push({ local: this.addPatternLocal(arm.pattern.name, subject.type, arm.pattern.span), fieldIndex: -1, type: subject.type });
              }
            } else if (arm.pattern.kind === "optional-present") {
              this.fail("optional-pattern-requires-optional", `the present pattern requires an optional subject, found '${subject.type}'`, arm.pattern.span);
            } else {
              this.fail("pattern-type-mismatch", `pattern is not valid for '${subject.type}'`, arm.pattern.span);
            }
            const guard = arm.guard && this.checkExpression(arm.guard);
            if (guard) this.requireType(guard.type, "bool", arm.guard!.span);
            const body = this.checkStatements(arm.body, false, expected);
            const armType = this.blockType(body);
            if (resultType === undefined || resultType === "never") resultType = armType;
            else if (armType !== "never" && resultType !== armType) {
              this.fail("match-arm-type", `match arms have types ${resultType} and ${armType}`, arm.span);
            }
            arms.push({ tag, literal, guard, tests, bindings, body, span: arm.span });
          } finally {
            this.scopes.pop();
          }
        }
        const requiredCases = declaration?.variants.length ?? 2;
        const finiteCoverage = Boolean(declaration || optional !== undefined || result || boolean);
        if (!catchAll && (!finiteCoverage || covered.size !== requiredCases)) {
          const missing = declaration
            ? declaration.variants.filter((variant) => !covered.has(variant.tag)).map((variant) => variant.name)
            : optional !== undefined
              ? [!covered.has(0) && "nil", !covered.has(1) && "present"].filter(Boolean)
              : result
                ? [!covered.has(0) && "Ok", !covered.has(1) && "Err"].filter(Boolean)
                : boolean
                  ? [!covered.has("bool:false") && "false", !covered.has("bool:true") && "true"].filter(Boolean)
                  : ["catch-all"];
          this.fail("nonexhaustive-match", `match does not cover: ${missing.join(", ")}`, expression.span);
        }
        return {
          kind: "match",
          subject,
          representation: declaration ? "enum" : dataDeclaration ? "data" : scalar ? "scalar" : "erased-variant",
          enumIndex: declaration?.index,
          arms,
          type: resultType ?? "void",
          span: expression.span,
        };
      }
      case "propagate": {
        const operand = this.checkExpression(expression.operand);
        const optional = optionalInner(operand.type);
        if (optional !== undefined) {
          if (optionalInner(this.signature.result) === undefined) {
            this.fail("invalid-optional-propagation", `function '${this.signature.name}' must return an optional type`, expression.span);
          }
          return { kind: "propagate", operand, payloadType: optional, successTag: 1, returnType: this.signature.result, type: optional, span: expression.span };
        }
        const parts = resultParts(operand.type);
        const target = resultParts(this.signature.result);
        if (!parts || !target || parts.error !== target.error) {
          this.fail("invalid-result-propagation", "Result propagation requires a function with a compatible Result error type", expression.span);
        }
        return { kind: "propagate", operand, payloadType: parts.ok, successTag: 0, returnType: this.signature.result, type: parts.ok, span: expression.span };
      }
      case "closure": {
        const expectedCallable = expected ? functionParts(expected) : undefined;
        if (expectedCallable && expectedCallable.parameters.length !== expression.parameters.length) {
          this.fail("argument-count", `expected a closure with ${expectedCallable.parameters.length} parameters, found ${expression.parameters.length}`, expression.span);
        }
        const parameterTypes = expression.parameters.map((parameter, index) => {
          if (parameter.type) return this.resolveType(parameter.type);
          const inferred = expectedCallable?.parameters[index];
          if (!inferred) this.fail("closure-parameter-needs-annotation", `closure parameter '${parameter.name}' needs a type annotation or an expected function type`, parameter.span);
          return inferred;
        });
        let result = expression.result
          ? this.resolveType(expression.result)
          : expectedCallable?.result;
        const inferResult = result === undefined;
        const provisionalResult = result ?? "void";
        const parameters: Parameter[] = expression.parameters.map((parameter, index) => ({
          name: parameter.name,
          type: parameter.type ?? { name: parameterTypes[index]!, span: parameter.span },
          span: parameter.span,
        }));
        const provisionalResultRef = expression.result ?? { name: provisionalResult, span: expression.span };
        const closureIndex = this.closures.length;
        this.closures.push(undefined as unknown as HirFunction);
        const baseDeclaration: FunctionDecl = {
          kind: "function",
          name: `$closure${closureIndex}`,
          suspending: false,
          genericParameters: [],
          genericBounds: [],
          parameters,
          result: provisionalResultRef,
          requirements: expression.requirements ?? [],
          body: expression.body,
          span: expression.span,
        };
        let requirements = expression.requirements;
        if (requirements === undefined || inferResult) {
          const discoverySignature: Signature = {
            name: baseDeclaration.name,
            index: closureIndex,
            suspending: false,
            genericParameters: [],
            genericBounds: [],
            rowParameters: [],
            parameters: parameterTypes,
            parameterNames: parameters.map((parameter) => parameter.name),
            defaultFunctionNames: parameters.map(() => undefined),
            variadic: false,
            result: provisionalResult,
            requirements: [],
            span: expression.span,
          };
          const discovery = new FunctionChecker(
            baseDeclaration,
            discoverySignature,
            this.signatures,
            this.dataTypes,
            this.enumTypes,
            this.traitTypes,
            this.implementations,
            this.inherentMethods,
            true,
            false,
            this.closures,
            true,
            this.visibleCaptureSources(),
            closureIndex,
            this.visibleProviders(),
            requirements === undefined,
            inferResult,
            this.pendingRecursiveClosure,
            this.imports,
          ).check();
          this.closures.length = closureIndex;
          if (!discovery.function) {
            this.diagnostics.push(...discovery.diagnostics);
            throw new CheckFailure("closure requirement inference failed");
          }
          if (requirements === undefined) requirements = discovery.function.requirements;
          if (inferResult) result = discovery.function.result;
          this.closures.push(undefined as unknown as HirFunction);
        }
        if (!result) this.fail("closure-result-needs-annotation", "a closure result could not be inferred", expression.span);
        const declaration: FunctionDecl = { ...baseDeclaration, result: expression.result ?? { name: result, span: expression.span }, requirements };
        const signature: Signature = { name: declaration.name, index: closureIndex, suspending: false, genericParameters: [], genericBounds: [], rowParameters: [], parameters: parameterTypes, parameterNames: parameters.map((parameter) => parameter.name), defaultFunctionNames: parameters.map(() => undefined), variadic: false, result, requirements, span: expression.span };
        const checked = new FunctionChecker(
          declaration,
          signature,
          this.signatures,
          this.dataTypes,
          this.enumTypes,
          this.traitTypes,
          this.implementations,
          this.inherentMethods,
          true,
          false,
          this.closures,
          true,
          this.visibleCaptureSources(),
          closureIndex,
          this.visibleProviders(),
          false,
          false,
          this.pendingRecursiveClosure,
          this.imports,
        ).check();
        if (!checked.function) {
          this.diagnostics.push(...checked.diagnostics);
          throw new CheckFailure("closure checking failed");
        }
        this.closures[closureIndex] = checked.function;
        const captures = checked.function.captures.map((capture) => this.captureValue(capture.source, expression.span));
        const type = functionType(parameterTypes, result, requirements);
        if (expected && expected !== type && !functionTypeMatchesRowPattern(expected, type)) {
          this.fail("type-mismatch", `expected ${expected}, found ${type}`, expression.span);
        }
        return { kind: "closure", closureIndex, captures, type, span: expression.span };
      }
      case "provider-use": {
        const key = this.canonicalProviderKey(expression.key, expression.span);
        const provider = this.resolveProvider(key, expression.span);
        if (!provider) this.fail("missing-requirement", `provider '${key}' is not available in the current context`, expression.span);
        return provider;
      }
      case "provider-context": {
        const { entries, providers } = this.checkProviderEntries(expression.entries);
        const keys = [...providers.keys()].sort();
        return { kind: "provider-context", keys, entries, type: contextType(keys), span: expression.span };
      }
      case "provider-with": {
        const { entries, providers } = this.checkProviderEntries(expression.entries);
        this.providerScopes.push(providers);
        let body: readonly HirStatement[];
        try {
          body = this.checkStatements(expression.body, true, expected, true);
        } finally {
          this.providerScopes.pop();
        }
        return { kind: "provider-with", entries, body, type: this.blockType(body), span: expression.span };
      }
    }
  }

  private resolveProvider(key: string, span: SourceSpan): HirExpression | undefined {
    for (let index = this.providerScopes.length - 1; index >= 0; index -= 1) {
      const local = this.providerScopes[index]!.get(key);
      if (local) return this.referenceLocal(local, span);
    }
    const providerIndex = this.signature.requirements.indexOf(key);
    if (providerIndex >= 0) return { kind: "provider-use", providerIndex, key, type: rowParameterName(key) ? `provider-row:${rowParameterName(key)}` : this.providerValueType(key), span };
    const requestedRow = rowParameterName(key);
    if (requestedRow) {
      const availableIndex = this.signature.requirements.findIndex((requirement) => rowParameterName(requirement) === requestedRow);
      if (availableIndex >= 0) {
        const available = this.signature.requirements[availableIndex]!;
        const requestedExcluded = new Set(requirementExclusions(key));
        const restoredKeys = requirementExclusions(available).filter((excluded) => !requestedExcluded.has(excluded));
        const restoredProviders = restoredKeys.map((restored) => this.resolveProvider(restored, span));
        if (restoredProviders.every((provider) => provider !== undefined)) {
          const base: HirExpression = {
            kind: "provider-use",
            providerIndex: availableIndex,
            key: available,
            type: `provider-row:${requestedRow}`,
            span,
          };
          return restoredKeys.length === 0
            ? base
            : {
                kind: "provider-pack",
                keys: restoredKeys,
                providers: restoredProviders as HirExpression[],
                bases: [base],
                type: `provider-row:${requestedRow}`,
                span,
              };
        }
      }
    }
    if (!this.inferRequirements) return undefined;
    let inferredIndex = this.inferredRequirements.indexOf(key);
    if (inferredIndex < 0) {
      this.inferredRequirements.push(key);
      inferredIndex = this.inferredRequirements.length - 1;
    }
    return { kind: "provider-use", providerIndex: inferredIndex, key, type: this.providerValueType(key), span };
  }

  private resolveCallProviders(
    requirements: readonly string[],
    substitutions: ReadonlyMap<string, ValueType>,
    rowSubstitutions: ReadonlyMap<string, readonly string[]>,
    span: SourceSpan,
  ): { providers: HirExpression[]; missing: string[] } {
    const providers: HirExpression[] = [];
    const missing: string[] = [];
    for (const sourceRequirement of requirements) {
      const requirement = rowParameterName(sourceRequirement)
        ? sourceRequirement
        : substituteGenericType(sourceRequirement, substitutions, rowSubstitutions);
      const row = rowParameterName(requirement);
      if (!row) {
        const provider = this.resolveProvider(requirement, span);
        if (provider) providers.push(provider);
        else missing.push(requirement);
        continue;
      }
      const substitution = rowSubstitutions.get(row);
      const keys = substitution && instantiateRowRequirement(requirement, substitution)
        .map((key) => rowParameterName(key) ? key : substituteGenericType(key, substitutions, rowSubstitutions));
      if (!keys) {
        missing.push(requirement);
        continue;
      }
      const symbolicKeys = keys.filter((key) => rowParameterName(key));
      const concreteKeys = keys.filter((key) => !rowParameterName(key));
      if (symbolicKeys.length === 1 && concreteKeys.length === 0) {
        const provider = this.resolveProvider(symbolicKeys[0]!, span);
        if (provider) providers.push(provider);
        else missing.push(symbolicKeys[0]!);
        continue;
      }
      const bases = symbolicKeys.map((key) => this.resolveProvider(key, span));
      symbolicKeys.forEach((key, index) => {
        if (!bases[index]) missing.push(key);
      });
      const rowProviders = concreteKeys.map((key) => this.resolveProvider(key, span));
      concreteKeys.forEach((key, index) => {
        if (!rowProviders[index]) missing.push(key);
      });
      providers.push({
        kind: "provider-pack",
        keys: concreteKeys,
        providers: rowProviders.filter((provider): provider is HirExpression => provider !== undefined),
        bases: bases.filter((provider): provider is HirExpression => provider !== undefined),
        type: `provider-row:${row}`,
        span,
      });
    }
    return { providers, missing };
  }

  private warnAbsentRowSubtractions(
    requirements: readonly string[],
    rowSubstitutions: ReadonlyMap<string, readonly string[]>,
    span: SourceSpan,
  ): void {
    for (const requirement of requirements) {
      const row = rowParameterName(requirement);
      if (!row) continue;
      const substitution = rowSubstitutions.get(row);
      if (!substitution || substitution.some((entry) => rowParameterName(entry))) continue;
      for (const excluded of requirementExclusions(requirement)) {
        if (!substitution.includes(excluded)) {
          this.diagnostics.push({
            code: "requirement-subtract-absent",
            message: `requirement '${excluded}' is absent from inferred row ${row}`,
            span,
            severity: "warning",
          });
        }
      }
    }
  }

  private requireDrivableSuspension(expression: Expression): void {
    if (expression.kind !== "name") return;
    const local = this.resolveLocal(expression.name);
    if (local && !local.drivable) {
      this.fail("mutable-receiver-required", "a stored suspension must have an explicit mut Suspend[T] binding to be driven or cancelled", expression.span);
    }
  }

  private resolveArgumentMapping(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterNames: readonly string[],
    callable: string,
  ): readonly number[] | undefined {
    if (!expression.argumentNames) return undefined;
    const mapping: number[] = [];
    const assigned = new Set<number>();
    let positionalIndex = 0;
    expression.argumentNames.forEach((name, argumentIndex) => {
      const parameterIndex = name === undefined ? positionalIndex++ : parameterNames.indexOf(name);
      if (parameterIndex < 0) {
        this.fail("unknown-named-argument", `${callable} has no parameter named '${name}'`, expression.arguments[argumentIndex]!.span);
      }
      if (assigned.has(parameterIndex)) {
        this.fail("duplicate-argument", `parameter '${parameterNames[parameterIndex]}' is supplied more than once`, expression.arguments[argumentIndex]!.span);
      }
      assigned.add(parameterIndex);
      mapping.push(parameterIndex);
    });
    return mapping.every((parameterIndex, argumentIndex) => parameterIndex === argumentIndex) ? undefined : mapping;
  }

  private planArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterNames: readonly string[],
    variadic: boolean,
    callable: string,
    defaultParameterIndices: ReadonlySet<number> = new Set(),
  ): readonly PlannedArgument[] {
    const fixedCount = variadic ? parameterNames.length - 1 : parameterNames.length;
    const names = expression.argumentNames ?? expression.arguments.map(() => undefined);
    const spreads = expression.argumentSpreads ?? expression.arguments.map(() => false);
    const entries: PlannedArgument[] = [];
    const assigned = new Set<number>();
    let positionalIndex = 0;
    let varargElements: number[] = [];

    const flushVarargElements = (): void => {
      if (varargElements.length === 0) return;
      const parameterIndex = parameterNames.length - 1;
      if (assigned.has(parameterIndex)) {
        this.fail("duplicate-argument", `parameter '${parameterNames[parameterIndex]}' is supplied more than once`, expression.arguments[varargElements[0]!]!.span);
      }
      assigned.add(parameterIndex);
      entries.push({ parameterIndex, argumentIndices: varargElements, kind: "vararg-elements" });
      varargElements = [];
    };

    expression.arguments.forEach((argument, argumentIndex) => {
      const name = names[argumentIndex];
      const spread = spreads[argumentIndex] ?? false;
      if (name !== undefined) {
        flushVarargElements();
        const parameterIndex = parameterNames.indexOf(name);
        if (parameterIndex < 0) {
          this.fail("unknown-named-argument", `${callable} has no parameter named '${name}'`, argument.span);
        }
        if (assigned.has(parameterIndex)) {
          this.fail("duplicate-argument", `parameter '${parameterNames[parameterIndex]}' is supplied more than once`, argument.span);
        }
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (spread) {
        if (!variadic) {
          this.fail("positional-spread-needs-vararg", `${callable} has no variadic parameter for this positional spread`, argument.span);
        }
        flushVarargElements();
        const parameterIndex = parameterNames.length - 1;
        if (assigned.has(parameterIndex)) {
          this.fail("duplicate-argument", `parameter '${parameterNames[parameterIndex]}' is supplied more than once`, argument.span);
        }
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (positionalIndex < fixedCount) {
        const parameterIndex = positionalIndex++;
        assigned.add(parameterIndex);
        entries.push({ parameterIndex, argumentIndices: [argumentIndex], kind: "single" });
        return;
      }
      if (!variadic) {
        this.fail("argument-count", `${callable} expects ${parameterNames.length} arguments, received ${expression.arguments.length}`, argument.span);
      }
      const parameterIndex = parameterNames.length - 1;
      if (assigned.has(parameterIndex)) {
        this.fail("duplicate-argument", `parameter '${parameterNames[parameterIndex]}' is supplied more than once`, argument.span);
      }
      varargElements.push(argumentIndex);
    });
    flushVarargElements();

    const missing = parameterNames.slice(0, fixedCount).filter((_, index) => !assigned.has(index) && !defaultParameterIndices.has(index));
    if (missing.length > 0) {
      this.fail("argument-count", `${callable} is missing argument${missing.length === 1 ? "" : "s"} ${missing.join(", ")}`, expression.span);
    }
    if (variadic && !assigned.has(parameterNames.length - 1)) {
      entries.push({ parameterIndex: parameterNames.length - 1, argumentIndices: [], kind: "vararg-elements" });
    }
    return entries;
  }

  private checkInherentMethodCall(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    receiver: HirExpression,
    method: InherentMethod,
  ): HirExpression {
    if (method.receiverMutable && mutableInner(receiver.type) === undefined) {
      this.fail("mutable-receiver-required", `method '${method.name}' requires mutable access to ${method.targetType}`, expression.callee.span);
    }
    const receiverParameterType = method.receiverMutable ? mutableType(method.targetType) : method.targetType;
    const methodReceiver = this.requireCoercion(receiver, receiverParameterType, receiver.span);
    const checkedArguments = this.checkConcreteArguments(expression, method.parameters, method.parameterNames, method.variadic, `method '${method.name}'`);
    const signature = this.signatures.get(method.functionName)!;
    const providers = signature.requirements.map((requirement) => this.resolveProvider(requirement, expression.span));
    const missing = signature.requirements.filter((_, index) => !providers[index]);
    if (missing.length > 0) this.fail("missing-requirement", `method '${method.name}' requires ${missing.join(" + ")}`, expression.span);
    const argumentParameterIndices = checkedArguments.parameterIndices
      ? [0, ...checkedArguments.parameterIndices.map((parameterIndex) => parameterIndex + 1)]
      : undefined;
    return method.suspending
      ? {
          kind: "suspend-construct",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: [methodReceiver, ...checkedArguments.arguments],
          argumentParameterIndices,
          providers: providers as HirExpression[],
          type: suspensionType(signature.index, signature.result),
          span: expression.span,
        }
      : {
          kind: "call",
          functionIndex: signature.index,
          functionName: signature.name,
          arguments: [methodReceiver, ...checkedArguments.arguments],
          argumentParameterIndices,
          providers: providers as HirExpression[],
          type: signature.result,
          span: expression.span,
        };
  }

  private checkConcreteArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    parameterTypes: readonly ValueType[],
    parameterNames: readonly string[],
    variadic: boolean,
    callable: string,
  ): { readonly arguments: readonly HirExpression[]; readonly parameterIndices?: readonly number[] } {
    if (expression.typeArguments) this.fail("unexpected-type-arguments", `${callable} is not generic`, expression.span);
    const plan = this.planArguments(expression, parameterNames, variadic, callable);
    const arguments_ = plan.map((entry): HirExpression => {
      const formal = parameterTypes[entry.parameterIndex]!;
      if (entry.kind === "single") {
        const source = expression.arguments[entry.argumentIndices[0]!]!;
        const checked = this.checkExpression(source, formal);
        if (mutableInner(formal) === checked.type) {
          this.fail("readonly-argument-to-mutable-parameter", `readonly argument '${checked.type}' cannot satisfy mutable parameter '${formal}'`, source.span);
        }
        return this.requireCoercion(checked, formal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementType = nominal?.name === "list" ? nominal.arguments[0]! : "void";
      const elements = entry.argumentIndices.map((argumentIndex) => {
        const source = expression.arguments[argumentIndex]!;
        return this.requireCoercion(this.checkExpression(source, elementType), elementType, source.span);
      });
      return { kind: "list", elements, elementType, type: formal, span: expression.span };
    });
    const mapping = plan.map((entry) => entry.parameterIndex);
    return {
      arguments: arguments_,
      parameterIndices: mapping.every((parameterIndex, argumentIndex) => parameterIndex === argumentIndex) ? undefined : mapping,
    };
  }

  private checkSignatureArguments(
    expression: Extract<Expression, { kind: "call" | "suspend-call" }>,
    signature: Signature,
    expected?: ValueType,
  ): {
    readonly arguments: readonly HirExpression[];
    readonly parameterIndices?: readonly number[];
    readonly defaultParameterIndices: readonly number[];
    readonly substitutions: ReadonlyMap<string, ValueType>;
    readonly rowSubstitutions: ReadonlyMap<string, readonly string[]>;
  } {
    const substitutions = new Map<string, ValueType>();
    const rowSubstitutions = new Map<string, readonly string[]>();
    if (expression.typeArguments) {
      if (expression.typeArguments.length !== signature.genericParameters.length) {
        const code = expression.typeArguments.length < signature.genericParameters.length
          ? "partial-generic-arguments"
          : "generic-argument-count";
        this.fail(code, `function '${signature.name}' expects ${signature.genericParameters.length} type arguments, received ${expression.typeArguments.length}`, expression.span);
      }
      expression.typeArguments.forEach((argument, index) => {
        if (argument.name === "_") return;
        substitutions.set(signature.genericParameters[index]!, this.resolveType(argument));
      });
    }
    if (expected) inferGenericType(signature.result, expected, substitutions, rowSubstitutions);
    const defaultParameters = new Set(signature.defaultFunctionNames.flatMap((name, index) => name ? [index] : []));
    const plan = this.planArguments(expression, signature.parameterNames, signature.variadic, `function '${signature.name}'`, defaultParameters);
    const arguments_ = plan.map((entry): HirExpression => {
      const formal = signature.parameters[entry.parameterIndex]!;
      if (entry.kind === "single") {
        const source = expression.arguments[entry.argumentIndices[0]!]!;
        const inferredFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        const checked = this.checkExpression(source, containsGenericType(inferredFormal) ? undefined : inferredFormal);
        const boundedParameters = new Set(signature.genericBounds.map((bound) => bound.parameter));
        const inferredActual = weakenBoundedGenericActual(formal, checked.type, boundedParameters);
        const conflict = inferGenericType(formal, inferredActual, substitutions, rowSubstitutions);
        if (conflict) this.fail("generic-type-mismatch", conflict, source.span);
        const instantiatedFormal = substituteGenericType(formal, substitutions, rowSubstitutions);
        if (mutableInner(instantiatedFormal) === checked.type) {
          this.fail("readonly-argument-to-mutable-parameter", `readonly argument '${checked.type}' cannot satisfy mutable parameter '${instantiatedFormal}'`, source.span);
        }
        return this.requireCoercion(checked, instantiatedFormal, source.span);
      }
      const nominal = nominalGenericParts(formal);
      const elementFormal = nominal?.name === "list" ? nominal.arguments[0]! : "void";
      const elements = entry.argumentIndices.map((argumentIndex) => {
        const source = expression.arguments[argumentIndex]!;
        const inferredElement = substituteGenericType(elementFormal, substitutions, rowSubstitutions);
        const checked = this.checkExpression(source, containsGenericType(inferredElement) ? undefined : inferredElement);
        const conflict = inferGenericType(elementFormal, checked.type, substitutions, rowSubstitutions);
        if (conflict) this.fail("generic-type-mismatch", conflict, source.span);
        return this.requireCoercion(checked, substituteGenericType(elementFormal, substitutions, rowSubstitutions), source.span);
      });
      const elementType = substituteGenericType(elementFormal, substitutions, rowSubstitutions);
      return { kind: "list", elements, elementType, type: nominalGenericType("list", [elementType]), span: expression.span };
    });
    const mapping = plan.map((entry) => entry.parameterIndex);
    const supplied = new Set(mapping);
    return {
      arguments: arguments_,
      parameterIndices: mapping.every((parameterIndex, argumentIndex) => parameterIndex === argumentIndex) ? undefined : mapping,
      defaultParameterIndices: [...defaultParameters].filter((parameterIndex) => !supplied.has(parameterIndex)),
      substitutions,
      rowSubstitutions,
    };
  }

  private checkProviderEntries(entries: Extract<Expression, { kind: "provider-context" | "provider-with" }>["entries"]): {
    entries: HirProviderContextEntry[];
    providers: Map<string, HirLocal>;
  } {
    const checked: HirProviderContextEntry[] = [];
    const providers = new Map<string, HirLocal>();
    for (const entry of entries) {
      let value = this.checkExpression(entry.value);
      if (entry.kind === "binding") {
        const key = this.canonicalProviderKey(entry.key, entry.span);
        this.rejectProviderKeyCollision(key, providers.keys(), entry.span);
        const trait = this.traitTypes.get(traitKeyName(key));
        if (trait) value = this.requireCoercion(value, `trait:${key}`, entry.value.span);
        else if (!value.type.startsWith("provider:")) {
          this.fail("provider-type-mismatch", `provider binding '${key}' requires an opaque provider value`, entry.value.span);
        }
        const local = this.addProviderLocal(key, entry.span);
        checked.push({ kind: "binding", key, local, value });
        providers.set(key, local);
        continue;
      }
      const keys = contextKeys(value.type);
      if (!keys) this.fail("context-spread-type", `context spread requires a $.Context value, found '${value.type}'`, entry.value.span);
      const contextLocal: HirLocal = {
        name: "$context-spread",
        type: value.type,
        index: this.locals.length,
        mutable: false,
        parameter: false,
        span: entry.span,
      };
      this.locals.push(contextLocal);
      const spreadProviders = keys.map((key, fieldIndex) => {
        this.rejectProviderKeyCollision(key, providers.keys(), entry.span);
        const local = this.addProviderLocal(key, entry.span);
        providers.set(key, local);
        return { key, local, fieldIndex };
      });
      checked.push({ kind: "spread", contextLocal, value, providers: spreadProviders });
    }
    return { entries: checked, providers };
  }

  private addProviderLocal(key: string, span: SourceSpan): HirLocal {
    const local: HirLocal = {
      name: `$provider-${key}`,
      type: this.providerValueType(key),
      index: this.locals.length,
      mutable: false,
      parameter: false,
      span,
    };
    this.locals.push(local);
    return local;
  }

  private providerValueType(key: string): ValueType {
    return this.traitTypes.has(traitKeyName(key)) ? `trait:${key}` : `provider:${key}`;
  }

  private rejectProviderKeyCollision(key: string, currentKeys: Iterable<string>, span: SourceSpan): void {
    const visibleKeys = [
      ...this.signature.requirements.filter((requirement) => !rowParameterName(requirement)),
      ...this.providerScopes.flatMap((scope) => [...scope.keys()]),
      ...currentKeys,
    ];
    const collision = visibleKeys.find((visible) => requirementKeysMayCollide(visible, key));
    if (collision) {
      this.fail("generic-requirement-key-collision", `provider keys '${collision}' and '${key}' can become identical after generic substitution`, span);
    }
  }

  private canonicalProviderKey(key: string, span: SourceSpan): string {
    const resolved = resolveGenericType(key, new Set(this.signature.genericParameters), new Set(this.signature.rowParameters));
    const nominal = nominalGenericParts(resolved);
    if (!nominal) return resolved;
    const trait = this.traitTypes.get(nominal.name);
    if (!trait) this.fail("unknown-requirement", `unknown generic requirement key '${key}'`, span);
    if (trait.genericParameters.length !== nominal.arguments.length) {
      this.fail("generic-arity", `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`, span);
    }
    if (!nominal.arguments.every((argument) => isKnownType(argument, this.dataTypes, this.enumTypes, this.traitTypes))) {
      this.fail("unknown-type", `requirement key '${key}' contains an unknown type`, span);
    }
    return resolved;
  }

  private resolveBoundDictionaries(
    signature: Signature,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): HirExpression[] {
    for (const parameter of signature.referenceParameters ?? []) {
      const actual = substitutions.get(parameter);
      if (!actual) this.fail("unresolved-generic-placeholder", `could not infer generic parameter ${parameter}`, span);
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        if (!(this.signature.referenceParameters ?? []).includes(forwarded)) {
          this.fail("missing-trait-implementation", `generic parameter '${forwarded}' does not satisfy Reference`, span);
        }
      } else if (!this.isIdentityType(actual)) {
        this.fail("missing-trait-implementation", `type '${actual}' does not implement Reference`, span);
      }
    }
    return signature.genericBounds.map((bound) => {
      const actual = substitutions.get(bound.parameter);
      if (!actual) this.fail("unresolved-generic-placeholder", `could not infer generic parameter ${bound.parameter}`, span);
      const forwarded = genericTypeName(actual);
      if (forwarded) {
        const boundIndex = this.signature.genericBounds.findIndex((candidate) => candidate.parameter === forwarded && candidate.traitIndex === bound.traitIndex);
        if (boundIndex < 0) {
          this.fail("missing-trait-implementation", `generic parameter '${forwarded}' does not satisfy ${bound.traitName}`, span);
        }
        return {
          kind: "trait-bound-dictionary",
          traitIndex: bound.traitIndex,
          boundIndex,
          type: `trait:${bound.traitName}`,
          span,
        };
      }
      const implementation = this.implementations.find((candidate) => candidate.traitIndex === bound.traitIndex && candidate.targetType === actual);
      if (!implementation) {
        this.fail("missing-trait-implementation", `type '${actual}' does not implement ${bound.traitName}`, span);
      }
      return {
        kind: "trait-dictionary",
        traitIndex: bound.traitIndex,
        implementationIndex: implementation.index,
        type: `trait:${bound.traitName}`,
        span,
      };
    });
  }

  private isIdentityType(type: ValueType): boolean {
    const generic = genericTypeName(type);
    if (generic) return (this.signature.referenceParameters ?? []).includes(generic);
    if (type.startsWith("trait:")) return true;
    if (functionParts(type) || suspensionParts(type) || traitSuspensionParts(type) || contextKeys(type)) return true;
    const nominal = nominalGenericParts(type);
    if (nominal?.name === "list" || nominal?.name === "map") return true;
    if (nominal && (this.dataTypes.has(nominal.name) || this.enumTypes.has(nominal.name))) return true;
    return this.dataTypes.has(type) || this.enumTypes.has(type);
  }

  private checkEnumConstructor(
    declaration: HirEnum,
    variantName: string,
    expression: Extract<Expression, { kind: "call" }>,
    expected?: ValueType,
  ): HirExpression {
    const span = expression.span;
    const variant = declaration.variants.find((candidate) => candidate.name === variantName);
    if (!variant) this.fail("unknown-variant", `enum '${declaration.name}' has no variant '${variantName}'`, span);
    if (variant.factoryFunctionName) {
      return this.checkExpression({
        ...expression,
        callee: { kind: "name", name: variant.factoryFunctionName, span: expression.callee.span },
      }, expected);
    }
    const plan = this.planArguments(expression, variant.fields.map((field) => field.name), false, `variant '${variantName}'`);
    const substitutions = new Map<string, ValueType>();
    const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
    if (expectedNominal?.name === declaration.name && expectedNominal.arguments.length === declaration.genericParameters.length) {
      declaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, expectedNominal.arguments[index]!));
    }
    const fields = plan.map((entry) => {
      const argument = expression.arguments[entry.argumentIndices[0]!]!;
      const field = variant.fields[entry.parameterIndex]!;
      const inferredField = substituteGenericType(field.type, substitutions);
      const checked = this.checkExpression(argument, containsGenericType(inferredField) ? undefined : inferredField);
      const conflict = inferGenericType(field.type, checked.type, substitutions);
      if (conflict) this.fail("generic-type-mismatch", conflict, argument.span);
      return this.requireCoercion(checked, substituteGenericType(field.type, substitutions), argument.span);
    });
    const unresolved = declaration.genericParameters.filter((parameter) => !substitutions.has(parameter));
    if (unresolved.length > 0) {
      this.fail("generic-enum-needs-context", `could not infer generic enum parameter${unresolved.length === 1 ? "" : "s"} ${unresolved.join(", ")}`, span);
    }
    const type = declaration.genericParameters.length > 0
      ? nominalGenericType(declaration.name, declaration.genericParameters.map((parameter) => substitutions.get(parameter)!))
      : declaration.name;
    return {
      kind: "enum",
      enumIndex: declaration.index,
      tag: variant.tag,
      fields,
      fieldIndices: plan.map((entry) => variant.fields[entry.parameterIndex]!.index),
      fieldTypes: declaration.fields.map((field) => field.type),
      erasedFieldTypes: declaration.genericParameters.length > 0 ? declaration.fields.map((field) => field.type) : undefined,
      type,
      span,
    };
  }

  private checkInternalEnumLiteral(
    expression: Extract<Expression, { kind: "call" }>,
    internalName: string,
    expected?: ValueType,
  ): HirExpression {
    const [, enumName, variantName] = internalName.split(".");
    const declaration = enumName ? this.enumTypes.get(enumName) : undefined;
    const variant = declaration?.variants.find((candidate) => candidate.name === variantName);
    if (!declaration || !variant) this.fail("internal-enum-literal", `invalid internal enum literal '${internalName}'`, expression.span);
    const sourceFields = [...declaration.sharedFields, ...variant.fields];
    if (expression.arguments.length !== sourceFields.length) {
      this.fail("internal-enum-literal", `internal enum literal '${internalName}' has the wrong field count`, expression.span);
    }
    const fields = expression.arguments.map((argument, index) => this.requireCoercion(
      this.checkExpression(argument, sourceFields[index]!.type),
      sourceFields[index]!.type,
      argument.span,
    ));
    const expectedNominal = expected ? nominalGenericParts(expected) : undefined;
    const type = expectedNominal?.name === declaration.name
      ? expected!
      : declaration.genericParameters.length > 0
        ? nominalGenericType(declaration.name, declaration.genericParameters.map((parameter) => `generic:${parameter}`))
        : declaration.name;
    if (expected) this.requireAssignable(type, expected, expression.span);
    return {
      kind: "enum",
      enumIndex: declaration.index,
      tag: variant.tag,
      fields,
      fieldIndices: sourceFields.map((field) => field.index),
      fieldTypes: declaration.fields.map((field) => field.type),
      erasedFieldTypes: declaration.genericParameters.length > 0 ? declaration.fields.map((field) => field.type) : undefined,
      type,
      span: expression.span,
    };
  }

  private checkNestedPattern(
    pattern: Pattern,
    type: ValueType,
    accessPath: readonly HirPatternAccessStep[],
    bindings: Array<HirMatchArm["bindings"][number]>,
    tests: Array<NonNullable<HirMatchArm["tests"]>[number]>,
  ): boolean {
    if (pattern.kind === "wildcard") return true;
    if (pattern.kind === "binding") {
      bindings.push({ local: this.addPatternLocal(pattern.name, type, pattern.span), fieldIndex: -1, type, accessPath });
      return true;
    }
    if (["boolean", "integer", "float", "string", "character"].includes(pattern.kind)) {
      const literal = this.checkExpression(pattern as Extract<Expression, { kind: "boolean" | "integer" | "float" | "string" | "character" }>, type);
      this.requireType(literal.type, type, pattern.span);
      tests.push({ accessPath, literal });
      return false;
    }
    const nominal = nominalGenericParts(type);
    if (pattern.kind === "data") {
      const declaration = this.dataTypes.get(nominal?.name ?? type);
      if (!declaration || pattern.typeName !== declaration.name) {
        this.fail("pattern-type-mismatch", `pattern names '${pattern.typeName}', expected '${type}'`, pattern.span);
      }
      const substitutions = new Map<string, ValueType>();
      if (nominal) declaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, nominal.arguments[index]!));
      const seen = new Set<string>();
      let irrefutable = true;
      for (const entry of pattern.fields) {
        if (seen.has(entry.name)) this.fail("duplicate-data-pattern-field", `field '${entry.name}' appears more than once`, entry.span);
        seen.add(entry.name);
        const field = declaration.fields.find((candidate) => candidate.name === entry.name);
        if (!field) this.fail("unknown-data-field", `type '${declaration.name}' has no field '${entry.name}'`, entry.span);
        const fieldType = substituteGenericType(field.type, substitutions);
        const nextPath: HirPatternAccessStep[] = [...accessPath, {
          kind: "data",
          typeIndex: declaration.index,
          fieldIndex: field.index,
          erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
          valueType: fieldType,
        }];
        irrefutable = this.checkNestedPattern(entry.pattern, fieldType, nextPath, bindings, tests) && irrefutable;
      }
      return irrefutable;
    }
    if (pattern.kind === "variant") {
      const declaration = this.enumTypes.get(nominal?.name ?? type);
      if (!declaration || (pattern.enumName !== undefined && pattern.enumName !== declaration.name)) {
        this.fail("pattern-type-mismatch", `variant pattern does not match '${type}'`, pattern.span);
      }
      const variant = declaration.variants.find((candidate) => candidate.name === pattern.variantName);
      if (!variant) this.fail("unknown-variant", `enum '${declaration.name}' has no variant '${pattern.variantName}'`, pattern.span);
      const payloadPatterns = pattern.payloadPatterns ?? pattern.bindings.map((name) => name
        ? { kind: "binding" as const, name, span: pattern.span }
        : { kind: "wildcard" as const, span: pattern.span });
      if (payloadPatterns.length !== variant.fields.length) {
        this.fail("pattern-arity", `variant '${variant.name}' expects ${variant.fields.length} payload patterns`, pattern.span);
      }
      const names = pattern.bindingNames ?? payloadPatterns.map(() => undefined);
      let nextPositional = 0;
      const seen = new Set<number>();
      const fieldIndices = names.map((name) => {
        const index = name === undefined ? nextPositional++ : variant.fields.findIndex((field) => field.name === name);
        if (index < 0) this.fail("unknown-variant-pattern-field", `variant '${variant.name}' has no payload field '${name}'`, pattern.span);
        if (seen.has(index)) this.fail("duplicate-variant-pattern-field", `payload field '${variant.fields[index]!.name}' appears more than once`, pattern.span);
        seen.add(index);
        return index;
      });
      tests.push({ accessPath, tag: variant.tag, tagEnumIndex: declaration.index });
      const substitutions = new Map<string, ValueType>();
      if (nominal) declaration.genericParameters.forEach((parameter, index) => substitutions.set(parameter, nominal.arguments[index]!));
      payloadPatterns.forEach((payloadPattern, sourceIndex) => {
        const fieldIndex = fieldIndices[sourceIndex]!;
        const field = variant.fields[fieldIndex]!;
        if (payloadPattern.kind === "binding" && names[sourceIndex] === undefined && payloadPattern.name !== field.name
          && variant.fields.some((candidate, candidateIndex) => candidateIndex !== fieldIndex && candidate.name === payloadPattern.name)) {
          this.diagnostics.push({ code: "variant-binding-name-mismatch", message: `positional binding '${payloadPattern.name}' occupies payload field '${field.name}'`, span: payloadPattern.span, severity: "warning" });
        }
        const fieldType = substituteGenericType(field.type, substitutions);
        const nextPath: HirPatternAccessStep[] = [...accessPath, {
          kind: "enum",
          typeIndex: declaration.index,
          fieldIndex: field.index,
          erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
          valueType: fieldType,
        }];
        this.checkNestedPattern(payloadPattern, fieldType, nextPath, bindings, tests);
      });
      return false;
    }
    this.fail("unsupported-nested-variant-pattern", `pattern '${pattern.kind}' is not supported for nested type '${type}'`, pattern.span);
  }

  private checkDataPattern(
    pattern: Extract<Pattern, { kind: "data" }>,
    declaration: HirData,
    path: readonly { dataIndex: number; fieldIndex: number }[],
    bindings: Array<{ local: HirLocal; fieldIndex: number; type: ValueType; path?: readonly { dataIndex: number; fieldIndex: number }[]; enumFieldIndex?: number; enumFieldType?: ValueType; enumErasedFieldType?: ValueType; accessPath?: readonly HirPatternAccessStep[] }>,
    tests: Array<{ path?: readonly { dataIndex: number; fieldIndex: number }[]; enumFieldIndex?: number; erasedFieldType?: ValueType; valueType?: ValueType; accessPath?: readonly HirPatternAccessStep[]; tag?: number; tagEnumIndex?: number; literal?: HirExpression }>,
  ): boolean {
    if (pattern.typeName !== declaration.name) {
      this.fail("pattern-type-mismatch", `pattern names '${pattern.typeName}', expected '${declaration.name}'`, pattern.span);
    }
    const seen = new Set<string>();
    let irrefutable = true;
    for (const entry of pattern.fields) {
      if (seen.has(entry.name)) this.fail("duplicate-data-pattern-field", `field '${entry.name}' appears more than once`, entry.span);
      seen.add(entry.name);
      const field = declaration.fields.find((candidate) => candidate.name === entry.name);
      if (!field) this.fail("unknown-data-field", `type '${declaration.name}' has no field '${entry.name}'`, entry.span);
      const fieldPath = [...path, { dataIndex: declaration.index, fieldIndex: field.index }];
      const nested = entry.pattern;
      if (nested.kind === "wildcard") continue;
      if (nested.kind === "binding") {
        bindings.push({ local: this.addPatternLocal(nested.name, field.type, nested.span), fieldIndex: -1, type: field.type, path: fieldPath });
        continue;
      }
      if (nested.kind === "data") {
        const nestedDeclaration = this.dataTypes.get(field.type);
        if (!nestedDeclaration) this.fail("pattern-type-mismatch", `field '${entry.name}' has non-data type '${field.type}'`, nested.span);
        irrefutable = this.checkDataPattern(nested, nestedDeclaration, fieldPath, bindings, tests) && irrefutable;
        continue;
      }
      if (["boolean", "integer", "float", "string", "character"].includes(nested.kind)) {
        const literal = this.checkExpression(nested as Extract<Expression, { kind: "boolean" | "integer" | "float" | "string" | "character" }>);
        this.requireType(literal.type, field.type, nested.span);
        tests.push({ path: fieldPath, literal });
        irrefutable = false;
        continue;
      }
      this.fail("pattern-type-mismatch", `pattern is not valid for field '${entry.name}' of type '${field.type}'`, nested.span);
    }
    return irrefutable;
  }

  private addPatternLocal(name: string, type: ValueType, span: SourceSpan): HirLocal {
    if (PRELUDE_NAMES.has(name)) this.fail("prelude-name-shadow", `pattern binding '${name}' shadows a prelude name`, span);
    if (this.currentScope().has(name)) this.fail("duplicate-binding", `pattern binding '${name}' appears more than once`, span);
    const local: HirLocal = { name, type, index: this.locals.length, mutable: false, parameter: false, span };
    this.locals.push(local);
    this.currentScope().set(name, local);
    return local;
  }

  private coerce(value: HirExpression, expected: ValueType | undefined, span: SourceSpan): HirExpression {
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
      const implementation = this.implementations.find((candidate) => candidate.traitIndex === trait.index && candidate.targetType === implementationType);
      if (implementation) {
        const receiverType = mutableTrait ? mutableType(implementationType) : implementationType;
        const wrappedValue = receiverType === value.type ? value : this.coerce(value, receiverType, span);
        return { kind: "trait-wrap", value: wrappedValue, traitIndex: trait.index, implementationIndex: implementation.index, type: expected, span };
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

  private blockType(statements: readonly HirStatement[]): ValueType {
    const last = statements.at(-1);
    return last?.kind === "expression" ? last.expression.type : "void";
  }

  private recordInferredReturn(type: ValueType, span: SourceSpan): void {
    if (type === "never") return;
    if (this.inferredReturnType === undefined || this.inferredReturnType === "never") {
      this.inferredReturnType = type;
      return;
    }
    if (this.inferredReturnType !== type) {
      this.fail("closure-result-type", `closure return paths have types ${this.inferredReturnType} and ${type}`, span);
    }
  }

  private checkFallthrough(body: readonly HirStatement[]): void {
    if (this.signature.result === "void") return;
    const last = body.at(-1);
    if (last?.kind === "return") return;
    const actual = last?.kind === "expression" ? last.expression.type : "void";
    if (actual === "void") this.fail("missing-return-value", `function '${this.signature.name}' may complete without an ${this.signature.result} value`, last?.span ?? this.declaration.span);
    this.requireAssignable(actual, this.signature.result, last?.span ?? this.declaration.span);
  }

  private resolveType(type: TypeRef): ValueType {
    const resolved = resolveGenericType(type.name, new Set(this.signature.genericParameters), new Set(this.signature.rowParameters));
    const declared = resolveTraitType(resolved, this.traitTypes);
    const nominal = nominalGenericParts(declared);
    if (nominal?.name === "map"
      && nominal.arguments.length === 2
      && isKnownType(nominal.arguments[0]!, this.dataTypes, this.enumTypes, this.traitTypes)
      && isKnownType(nominal.arguments[1]!, this.dataTypes, this.enumTypes, this.traitTypes)
      && mapKeyKind(nominal.arguments[0]!) === undefined) {
      this.fail("invalid-map-key", `type '${nominal.arguments[0]}' does not implement the MVP map-key contract`, type.span);
    }
    if (!isKnownType(declared, this.dataTypes, this.enumTypes, this.traitTypes)) this.fail("unknown-type", `unknown or unsupported type '${type.name}'`, type.span);
    return declared;
  }

  private currentScope(): Map<string, HirLocal> {
    return this.scopes.at(-1)!;
  }

  private resolveLocal(name: string): HirLocal | undefined {
    for (let index = this.scopes.length - 1; index >= 0; index -= 1) {
      const local = this.scopes[index]!.get(name);
      if (local) return local;
    }
    return undefined;
  }

  private visibleCaptureSources(): ReadonlyMap<string, HirLocal> {
    const visible = new Map(this.availableCaptures);
    for (const scope of this.scopes) {
      for (const [name, local] of scope) visible.set(name, local);
    }
    for (const scope of this.providerScopes) {
      for (const local of scope.values()) visible.set(local.name, local);
    }
    return visible;
  }

  private visibleProviders(): ReadonlyMap<string, HirLocal> {
    const visible = new Map<string, HirLocal>();
    for (const scope of this.providerScopes) {
      for (const [key, local] of scope) visible.set(key, local);
    }
    return visible;
  }

  private referenceLocal(local: HirLocal, span: SourceSpan): HirExpression {
    if (this.locals.includes(local)) return { kind: "local", local, type: local.type, span };
    if (this.insideClosure && [...this.availableCaptures.values()].includes(local)) {
      let capture = this.captures.get(local.name);
      if (!capture) {
        capture = { source: local, fieldIndex: this.captures.size };
        this.captures.set(local.name, capture);
      }
      return { kind: "capture", closureIndex: this.closureIndex, fieldIndex: capture.fieldIndex, type: local.type, span };
    }
    return { kind: "local", local, type: local.type, span };
  }

  private captureValue(source: HirLocal, span: SourceSpan): HirExpression {
    if (this.locals.includes(source)) return { kind: "local", local: source, type: source.type, span };
    if (this.insideClosure && this.availableCaptures.get(source.name) === source) {
      let capture = this.captures.get(source.name);
      if (!capture) {
        capture = { source, fieldIndex: this.captures.size };
        this.captures.set(source.name, capture);
      }
      return { kind: "capture", closureIndex: this.closureIndex, fieldIndex: capture.fieldIndex, type: source.type, span };
    }
    throw new Error(`cannot materialize closure capture '${source.name}'`);
  }

  private requireAssignable(actual: ValueType, expected: ValueType, span: SourceSpan): void {
    if (actual === "never" || actual === expected || isPermissionWeakening(actual, expected)) return;
    if (mutableInner(expected) === actual) this.fail("mutable-upgrade", `readonly type '${actual}' cannot be upgraded to '${expected}'`, span);
    this.fail("type-mismatch", `expected ${expected}, found ${actual}`, span);
  }

  private requireCoercion(expression: HirExpression, expected: ValueType, span: SourceSpan): HirExpression {
    const coerced = this.coerce(expression, expected, span);
    this.requireAssignable(coerced.type, expected, span);
    return coerced;
  }

  private requireType(actual: ValueType, expected: ValueType, span: SourceSpan): void {
    this.requireAssignable(actual, expected, span);
  }

  private fail(code: string, message: string, span: SourceSpan): never {
    this.diagnostics.push({ code, message, span });
    throw new CheckFailure(message);
  }
}

function statementsReferenceName(statements: readonly Statement[], name: string): boolean {
  return statements.some((statement) => nodeReferencesName(statement, name));
}

function nodeReferencesName(value: unknown, name: string): boolean {
  if (Array.isArray(value)) return value.some((item) => nodeReferencesName(item, name));
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "name" && node.name === name) return true;
  if (node.kind === "closure") {
    const parameters = node.parameters as readonly { readonly name: string }[];
    if (parameters.some((parameter) => parameter.name === name)) return false;
  }
  return Object.entries(node).some(([key, child]) => key !== "span" && nodeReferencesName(child, name));
}

function pureFunctionNames(program: Program): ReadonlySet<string> {
  const candidates = new Map(program.functions
    .filter((declaration) => !declaration.suspending && declaration.requirements.length === 0)
    .map((declaration) => [declaration.name, declaration] as const));
  const pure = new Set(candidates.keys());
  let changed = true;
  while (changed) {
    changed = false;
    for (const [name, declaration] of candidates) {
      if (!pure.has(name)) continue;
      const locals = new Set(declaration.parameters.map((parameter) => parameter.name));
      if (!statementsArePure(declaration.body, locals, pure, program)) {
        pure.delete(name);
        changed = true;
      }
    }
  }
  return pure;
}

function statementsArePure(statements: readonly Statement[], inheritedLocals: ReadonlySet<string>, pureFunctions: ReadonlySet<string>, program: Program): boolean {
  const locals = new Set(inheritedLocals);
  for (const statement of statements) {
    switch (statement.kind) {
      case "binding":
        if (!expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        locals.add(statement.name);
        break;
      case "tuple-binding":
        if (!expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        statement.bindings.forEach((binding) => locals.add(binding.name));
        break;
      case "assignment":
        if (!locals.has(statement.name) || !expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        break;
      case "discard":
        if (!expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        break;
      case "return":
      case "break":
        if (statement.value && !expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        break;
      case "expression":
        if (!expressionIsPure(statement.expression, locals, pureFunctions, program)) return false;
        break;
      case "defer":
        return false;
      case "continue":
      case "pass":
        break;
    }
  }
  return true;
}

function expressionIsPure(expression: Expression, locals: ReadonlySet<string>, pureFunctions: ReadonlySet<string>, program: Program): boolean {
  switch (expression.kind) {
    case "integer":
    case "float":
    case "string":
    case "interpolated-string":
    case "character":
    case "boolean":
    case "nil":
    case "contextual-variant":
      return expression.kind === "interpolated-string"
        ? expression.segments.every((segment) => segment.kind === "text" || expressionIsPure(segment.expression, locals, pureFunctions, program))
        : true;
    case "name":
      return locals.has(expression.name) || pureFunctions.has(expression.name);
    case "list":
      return expression.elements.every((element) => expressionIsPure(element, locals, pureFunctions, program));
    case "tuple":
      return expression.elements.every((element) => expressionIsPure(element, locals, pureFunctions, program));
    case "map":
      return expression.entries.every((entry) => expressionIsPure(entry.key, locals, pureFunctions, program) && expressionIsPure(entry.value, locals, pureFunctions, program));
    case "unary":
    case "propagate":
      return expressionIsPure(expression.operand, locals, pureFunctions, program);
    case "binary":
      return expressionIsPure(expression.left, locals, pureFunctions, program) && expressionIsPure(expression.right, locals, pureFunctions, program);
    case "call": {
      const argumentsPure = expression.arguments.every((argument) => expressionIsPure(argument, locals, pureFunctions, program));
      if (!argumentsPure) return false;
      if (expression.callee.kind === "name") {
        const calleeName = expression.callee.name;
        return ["Ok", "Err", "panic"].includes(calleeName)
          || pureFunctions.has(calleeName)
          || program.data.some((declaration) => declaration.name === calleeName);
      }
      if (expression.callee.kind === "member") {
        if (["len", "starts_with"].includes(expression.callee.name)) {
          return expressionIsPure(expression.callee.receiver, locals, pureFunctions, program)
            && expression.arguments.every((argument) => expressionIsPure(argument, locals, pureFunctions, program));
        }
        const receiver = expression.callee.receiver;
        return receiver.kind === "name"
          && program.enums.some((declaration) => declaration.name === receiver.name);
      }
      if (expression.callee.kind === "contextual-variant") return true;
      return false;
    }
    case "suspend-call":
    case "provider-use":
    case "provider-context":
    case "provider-with":
      return false;
    case "data":
      return (!expression.spread || expressionIsPure(expression.spread, locals, pureFunctions, program))
        && expression.fields.every((field) => expressionIsPure(field.value, locals, pureFunctions, program));
    case "member":
      return expressionIsPure(expression.receiver, locals, pureFunctions, program);
    case "index":
      return expressionIsPure(expression.receiver, locals, pureFunctions, program) && expressionIsPure(expression.index, locals, pureFunctions, program);
    case "closure":
      return true;
    case "if":
      return expressionIsPure(expression.condition, locals, pureFunctions, program)
        && statementsArePure(expression.thenBody, locals, pureFunctions, program)
        && statementsArePure(expression.elseBody, locals, pureFunctions, program);
    case "for":
      return expressionIsPure(expression.iterable, locals, pureFunctions, program)
        && statementsArePure(expression.body, new Set([...locals, ...expression.bindings.map((binding) => binding.name)]), pureFunctions, program)
        && statementsArePure(expression.elseBody, locals, pureFunctions, program);
    case "while":
      return expressionIsPure(expression.condition, locals, pureFunctions, program)
        && statementsArePure(expression.body, locals, pureFunctions, program)
        && statementsArePure(expression.elseBody, locals, pureFunctions, program);
    case "match":
      return expressionIsPure(expression.subject, locals, pureFunctions, program)
        && expression.arms.every((arm) => (!arm.guard || expressionIsPure(arm.guard, locals, pureFunctions, program))
          && statementsArePure(arm.body, locals, pureFunctions, program));
  }
}

function substituteGenericType(
  type: ValueType,
  substitutions: ReadonlyMap<string, ValueType>,
  rowSubstitutions: ReadonlyMap<string, readonly string[]> = new Map(),
): ValueType {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(substituteGenericType(mutable, substitutions, rowSubstitutions));
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map((element) => substituteGenericType(element, substitutions, rowSubstitutions)));
  const optional = optionalInner(type);
  if (optional !== undefined) return `${substituteGenericType(optional, substitutions, rowSubstitutions)}?`;
  const result = resultParts(type);
  if (result) return `Result[${substituteGenericType(result.ok, substitutions, rowSubstitutions)},${substituteGenericType(result.error, substitutions, rowSubstitutions)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal) return nominalGenericType(nominal.name, nominal.arguments.map((argument) => substituteGenericType(argument, substitutions, rowSubstitutions)));
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) => substituteGenericType(parameter, substitutions, rowSubstitutions)),
      substituteGenericType(callable.result, substitutions, rowSubstitutions),
      callable.requirements.flatMap((requirement) => {
        const row = rowParameterName(requirement);
        return row ? instantiateRowRequirement(requirement, rowSubstitutions.get(row) ?? [requirement]) : [requirement];
      }),
      callable.variadic,
    );
  }
  const generic = genericTypeName(type);
  if (generic) return substitutions.get(generic) ?? type;
  return type;
}

function genericTypeName(type: ValueType): string | undefined {
  const match = /^generic:([^?\[\](),]+)$/.exec(type);
  return match?.[1];
}

function traitTypeName(type: ValueType): string | undefined {
  const readonly = readonlyType(type);
  if (!readonly.startsWith("trait:")) return undefined;
  const key = readonly.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
}

function traitKeyName(key: string): string {
  return nominalGenericParts(key)?.name ?? key;
}

function requirementKeysMayCollide(left: string, right: string): boolean {
  if (left === right) return false;
  const substitutions = new Map<string, ValueType>();
  const resolve = (type: ValueType): ValueType => {
    const generic = genericTypeName(type);
    const substitution = generic && substitutions.get(generic);
    return substitution ? resolve(substitution) : type;
  };
  const children = (type: ValueType): { readonly head: string; readonly values: readonly ValueType[] } => {
    const tuple = tupleParts(type);
    if (tuple !== undefined) return { head: `tuple:${tuple.length}`, values: tuple };
    const optional = optionalInner(type);
    if (optional !== undefined) return { head: "optional", values: [optional] };
    const result = resultParts(type);
    if (result) return { head: "Result", values: [result.ok, result.error] };
    const nominal = nominalGenericParts(type);
    if (nominal) return { head: `nominal:${nominal.name}`, values: nominal.arguments };
    const callable = functionParts(type);
    if (callable) return { head: `function:${callable.variadic}:${callable.requirements.join("+")}`, values: [...callable.parameters, callable.result] };
    return { head: `plain:${type}`, values: [] };
  };
  const occurs = (name: string, type: ValueType): boolean => {
    const resolved = resolve(type);
    if (genericTypeName(resolved) === name) return true;
    return children(resolved).values.some((child) => occurs(name, child));
  };
  const unifyTypes = (firstType: ValueType, secondType: ValueType): boolean => {
    const first = resolve(firstType);
    const second = resolve(secondType);
    if (first === second) return true;
    const firstGeneric = genericTypeName(first);
    if (firstGeneric) {
      if (occurs(firstGeneric, second)) return false;
      substitutions.set(firstGeneric, second);
      return true;
    }
    const secondGeneric = genericTypeName(second);
    if (secondGeneric) {
      if (occurs(secondGeneric, first)) return false;
      substitutions.set(secondGeneric, first);
      return true;
    }
    const firstChildren = children(first);
    const secondChildren = children(second);
    return firstChildren.head === secondChildren.head
      && firstChildren.values.length === secondChildren.values.length
      && firstChildren.values.every((child, index) => unifyTypes(child, secondChildren.values[index]!));
  };
  return unifyTypes(left, right);
}

function containsGenericType(type: ValueType): boolean {
  if (genericTypeName(type)) return true;
  const mutable = mutableInner(type);
  if (mutable !== undefined) return containsGenericType(mutable);
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tuple.some(containsGenericType);
  const optional = optionalInner(type);
  if (optional !== undefined) return containsGenericType(optional);
  const result = resultParts(type);
  if (result) return containsGenericType(result.ok) || containsGenericType(result.error);
  const nominal = nominalGenericParts(type);
  if (nominal) return nominal.arguments.some(containsGenericType);
  const callable = functionParts(type);
  return Boolean(callable && (callable.parameters.some(containsGenericType) || containsGenericType(callable.result)));
}

function inferGenericType(
  formal: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
  rowSubstitutions: Map<string, readonly string[]> = new Map(),
): string | undefined {
  const generic = genericTypeName(formal);
  if (generic) {
    const existing = substitutions.get(generic);
    if (existing && existing !== actual) return `generic parameter '${generic}' was inferred as both ${existing} and ${actual}`;
    substitutions.set(generic, actual);
    return undefined;
  }
  const formalMutable = mutableInner(formal);
  const actualMutable = mutableInner(actual);
  if (formalMutable !== undefined && actualMutable !== undefined) {
    return inferGenericType(formalMutable, actualMutable, substitutions, rowSubstitutions);
  }
  const formalTuple = tupleParts(formal);
  const actualTuple = tupleParts(actual);
  if (formalTuple !== undefined && actualTuple !== undefined && formalTuple.length === actualTuple.length) {
    for (let index = 0; index < formalTuple.length; index += 1) {
      const conflict = inferGenericType(formalTuple[index]!, actualTuple[index]!, substitutions, rowSubstitutions);
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalOptional = optionalInner(formal);
  const actualOptional = optionalInner(actual);
  if (formalOptional !== undefined && actualOptional !== undefined) return inferGenericType(formalOptional, actualOptional, substitutions, rowSubstitutions);
  const formalResult = resultParts(formal);
  const actualResult = resultParts(actual);
  if (formalResult && actualResult) {
    return inferGenericType(formalResult.ok, actualResult.ok, substitutions, rowSubstitutions)
      ?? inferGenericType(formalResult.error, actualResult.error, substitutions, rowSubstitutions);
  }
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(actual);
  if (formalNominal && actualNominal && formalNominal.name === actualNominal.name && formalNominal.arguments.length === actualNominal.arguments.length) {
    for (let index = 0; index < formalNominal.arguments.length; index += 1) {
      const conflict = inferGenericType(formalNominal.arguments[index]!, actualNominal.arguments[index]!, substitutions, rowSubstitutions);
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalCallable = functionParts(formal);
  const actualCallable = functionParts(actual);
  if (formalCallable && actualCallable && formalCallable.variadic === actualCallable.variadic && formalCallable.parameters.length === actualCallable.parameters.length) {
    for (let index = 0; index < formalCallable.parameters.length; index += 1) {
      const conflict = inferGenericType(formalCallable.parameters[index]!, actualCallable.parameters[index]!, substitutions, rowSubstitutions);
      if (conflict) return conflict;
    }
    const resultConflict = inferGenericType(formalCallable.result, actualCallable.result, substitutions, rowSubstitutions);
    return resultConflict ?? inferRequirementRows(formalCallable.requirements, actualCallable.requirements, rowSubstitutions);
  }
  return undefined;
}

function rowParameterName(requirement: string): string | undefined {
  return requirement.startsWith("row:") ? requirement.slice("row:".length).split("\\")[0] : undefined;
}

function requirementExclusions(requirement: string): readonly string[] {
  return requirement.split("\\").slice(1);
}

function symbolicRequirement(name: string, exclusions: readonly string[] = []): string {
  return [`row:${name}`, ...normalizedRequirements(exclusions)].join("\\");
}

function instantiateRowRequirement(requirement: string, substitution: readonly string[]): readonly string[] {
  const exclusions = new Set(requirementExclusions(requirement));
  return substitution.flatMap((entry) => {
    const nested = rowParameterName(entry);
    if (nested) return [symbolicRequirement(nested, [...requirementExclusions(entry), ...exclusions])];
    return exclusions.has(entry) ? [] : [entry];
  });
}

function normalizedRequirements(requirements: readonly string[]): readonly string[] {
  return [...new Set(requirements)].sort();
}

function sameRequirements(left: readonly string[], right: readonly string[]): boolean {
  const normalizedLeft = normalizedRequirements(left);
  const normalizedRight = normalizedRequirements(right);
  return normalizedLeft.length === normalizedRight.length
    && normalizedLeft.every((requirement, index) => requirement === normalizedRight[index]);
}

function inferRequirementRows(
  formal: readonly string[],
  actual: readonly string[],
  substitutions: Map<string, readonly string[]>,
): string | undefined {
  const rowNames = [...new Set(formal.map(rowParameterName).filter((name): name is string => name !== undefined))];
  if (rowNames.length === 0) return undefined;
  const concrete = formal.filter((requirement) => !rowParameterName(requirement));
  const actualSet = new Set(actual);
  const missingConcrete = concrete.filter((requirement) => !actualSet.has(requirement));
  if (missingConcrete.length > 0) {
    return `callable requirement row is missing ${missingConcrete.join(" + ")}`;
  }
  const boundRequirements = formal.flatMap((requirement) => {
    const name = rowParameterName(requirement);
    return name && substitutions.has(name) ? instantiateRowRequirement(requirement, substitutions.get(name)!) : [];
  });
  const unbound = rowNames.filter((name) => !substitutions.has(name));
  if (unbound.length > 1) return undefined;
  if (unbound.length === 1) {
    const occupied = new Set([...concrete, ...boundRequirements]);
    substitutions.set(unbound[0]!, normalizedRequirements(actual.filter((requirement) => !occupied.has(requirement))));
  }
  const instantiated = normalizedRequirements([
    ...concrete,
    ...formal.flatMap((requirement) => {
      const name = rowParameterName(requirement);
      return name ? instantiateRowRequirement(requirement, substitutions.get(name) ?? []) : [];
    }),
  ]);
  if (!sameRequirements(instantiated, actual)) {
    const names = rowNames.map((name) => `'${name}'`).join(" and ");
    return `requirement-row parameter${rowNames.length === 1 ? "" : "s"} ${names} cannot match both ${instantiated.join(" + ") || "$()"} and ${normalizedRequirements(actual).join(" + ") || "$()"}`;
  }
  return undefined;
}

function functionTypeMatchesRowPattern(formalType: ValueType, actualType: ValueType): boolean {
  const formal = functionParts(formalType);
  const actual = functionParts(actualType);
  if (!formal || !actual || formal.variadic !== actual.variadic || formal.parameters.length !== actual.parameters.length) return false;
  if (!formal.parameters.every((parameter, index) => parameter === actual.parameters[index]) || formal.result !== actual.result) return false;
  return inferRequirementRows(formal.requirements, actual.requirements, new Map()) === undefined;
}

function resolveGenericType(type: ValueType, genericParameters: ReadonlySet<string>, rowParameters: ReadonlySet<string> = new Set()): ValueType {
  if (genericParameters.has(type)) return `generic:${type}`;
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(resolveGenericType(mutable, genericParameters, rowParameters));
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map((element) => resolveGenericType(element, genericParameters, rowParameters)));
  const optional = optionalInner(type);
  if (optional !== undefined) return `${resolveGenericType(optional, genericParameters, rowParameters)}?`;
  const result = resultParts(type);
  if (result) return `Result[${resolveGenericType(result.ok, genericParameters, rowParameters)},${resolveGenericType(result.error, genericParameters, rowParameters)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal) return nominalGenericType(nominal.name, nominal.arguments.map((argument) => resolveGenericType(argument, genericParameters, rowParameters)));
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) => resolveGenericType(parameter, genericParameters, rowParameters)),
      resolveGenericType(callable.result, genericParameters, rowParameters),
      callable.requirements.flatMap((requirement) => resolveGenericRequirement(requirement, rowParameters)),
      callable.variadic,
    );
  }
  return type;
}

function collectRowParameterReferences(type: ValueType, genericParameters: ReadonlySet<string>, output: Set<string>): void {
  const mutable = mutableInner(type);
  if (mutable !== undefined) {
    collectRowParameterReferences(mutable, genericParameters, output);
    return;
  }
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    tuple.forEach((element) => collectRowParameterReferences(element, genericParameters, output));
    return;
  }
  const callable = functionParts(type);
  if (callable) {
    for (const requirement of callable.requirements) {
      const base = requirement.split("\\")[0]!;
      if (genericParameters.has(base)) output.add(base);
    }
    callable.parameters.forEach((parameter) => collectRowParameterReferences(parameter, genericParameters, output));
    collectRowParameterReferences(callable.result, genericParameters, output);
    return;
  }
  const optional = optionalInner(type);
  if (optional !== undefined) collectRowParameterReferences(optional, genericParameters, output);
  const result = resultParts(type);
  if (result) {
    collectRowParameterReferences(result.ok, genericParameters, output);
    collectRowParameterReferences(result.error, genericParameters, output);
  }
}

function resolveGenericRequirement(requirement: string, rowParameters: ReadonlySet<string>): readonly string[] {
  const [base, ...excluded] = requirement.split("\\");
  if (rowParameters.has(base!)) return [symbolicRequirement(base!, excluded)];
  return excluded.includes(base!) ? [] : [base!];
}

function firstPrivateSignatureType(type: ValueType, program: Program): string | undefined {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return firstPrivateSignatureType(mutable, program);
  const tuple = tupleParts(type);
  if (tuple !== undefined) {
    for (const element of tuple) {
      const privateType = firstPrivateSignatureType(element, program);
      if (privateType) return privateType;
    }
    return undefined;
  }
  const optional = optionalInner(type);
  if (optional !== undefined) return firstPrivateSignatureType(optional, program);
  const result = resultParts(type);
  if (result) return firstPrivateSignatureType(result.ok, program) ?? firstPrivateSignatureType(result.error, program);
  const callable = functionParts(type);
  if (callable) {
    for (const parameter of callable.parameters) {
      const privateType = firstPrivateSignatureType(parameter, program);
      if (privateType) return privateType;
    }
    return firstPrivateSignatureType(callable.result, program);
  }
  const nominal = nominalGenericParts(type);
  const base = nominal?.name ?? type;
  const declaration = program.data.find((candidate) => candidate.name === base)
    ?? program.enums.find((candidate) => candidate.name === base)
    ?? program.traits.find((candidate) => candidate.name === base);
  if (declaration && !declaration.public) return base;
  if (nominal) {
    for (const argument of nominal.arguments) {
      const privateType = firstPrivateSignatureType(argument, program);
      if (privateType) return privateType;
    }
  }
  return undefined;
}

function typeName(
  type: TypeRef,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait>,
  diagnostics: Diagnostic[],
  genericParameters: ReadonlySet<string> = new Set(),
  rowParameters: ReadonlySet<string> = new Set(),
): ValueType | undefined {
  const resolved = resolveTraitType(resolveGenericType(type.name, genericParameters, rowParameters), traitTypes);
  const nominal = nominalGenericParts(resolved);
  if (nominal?.name === "map"
    && nominal.arguments.length === 2
    && isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes)
    && isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes)
    && mapKeyKind(nominal.arguments[0]!) === undefined) {
    diagnostics.push({ code: "invalid-map-key", message: `type '${nominal.arguments[0]}' does not implement the MVP map-key contract`, span: type.span });
    return undefined;
  }
  if (!isKnownType(resolved, dataTypes, enumTypes, traitTypes)) {
    diagnostics.push({ code: "unknown-type", message: `unknown or unsupported type '${type.name}'`, span: type.span });
    return undefined;
  }
  return resolved;
}

function resolveTraitType(type: ValueType, traitTypes: ReadonlyMap<string, HirTrait>): ValueType {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(resolveTraitType(mutable, traitTypes));
  if (traitTypes.has(type)) return `trait:${type}`;
  const tuple = tupleParts(type);
  if (tuple !== undefined) return tupleType(tuple.map((element) => resolveTraitType(element, traitTypes)));
  const optional = optionalInner(type);
  if (optional !== undefined) return `${resolveTraitType(optional, traitTypes)}?`;
  const result = resultParts(type);
  if (result) return `Result[${resolveTraitType(result.ok, traitTypes)},${resolveTraitType(result.error, traitTypes)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal) {
    const arguments_ = nominal.arguments.map((argument) => resolveTraitType(argument, traitTypes));
    const resolved = nominalGenericType(nominal.name, arguments_);
    return traitTypes.has(nominal.name) ? `trait:${resolved}` : resolved;
  }
  const callable = functionParts(type);
  if (callable) return functionType(
    callable.parameters.map((parameter) => resolveTraitType(parameter, traitTypes)),
    resolveTraitType(callable.result, traitTypes),
    callable.requirements,
    callable.variadic,
  );
  return type;
}

export function check(program: Program): CheckResult {
  const diagnostics: Diagnostic[] = [];
  const imports = new Map<string, string>();
  for (const declaration of program.uses) {
    for (const imported of declaration.names) {
      const localName = imported.alias ?? imported.name;
      if (PRELUDE_NAMES.has(localName)) {
        diagnostics.push({ code: "prelude-name-shadow", message: `import '${localName}' shadows a prelude name`, span: declaration.span });
        continue;
      }
      if (imports.has(localName)) {
        diagnostics.push({ code: "duplicate-module-name", message: `imported name '${localName}' is declared more than once`, span: declaration.span });
        continue;
      }
      imports.set(localName, `${declaration.module}.${imported.name}`);
    }
  }
  const pureFunctions = pureFunctionNames(program);
  for (const declaration of program.functions) {
    let sawDefault = false;
    const earlierParameters = new Set<string>();
    for (const parameter of declaration.parameters) {
      if (parameter.default) {
        sawDefault = true;
        if (!expressionIsPure(parameter.default, earlierParameters, pureFunctions, program)) {
          diagnostics.push({ code: "impure-parameter-default", message: `default for '${declaration.name}.${parameter.name}' is not compile-time pure`, span: parameter.default.span });
        }
      } else if (sawDefault && !parameter.variadic) {
        diagnostics.push({ code: "parameter-default-order", message: `parameter '${parameter.name}' follows a parameter with a default`, span: parameter.span });
      }
      earlierParameters.add(parameter.name);
    }
  }
  for (const declaration of program.data) {
    for (const field of declaration.fields) {
      if (field.default && !expressionIsPure(field.default, new Set(), pureFunctions, program)) {
        diagnostics.push({ code: "impure-data-default", message: `default for '${declaration.name}.${field.name}' is not compile-time pure`, span: field.default.span });
      }
    }
  }
  for (const declaration of program.enums) {
    let sawDefault = false;
    const earlierFields = new Set<string>();
    for (const field of declaration.sharedFields) {
      if (field.default) {
        sawDefault = true;
        if (!expressionIsPure(field.default, earlierFields, pureFunctions, program)) {
          diagnostics.push({ code: "impure-enum-default", message: `default for '${declaration.name}.${field.name}' is not compile-time pure`, span: field.default.span });
        }
      } else if (sawDefault) {
        diagnostics.push({ code: "enum-default-order", message: `shared enum field '${field.name}' follows a field with a default`, span: field.span });
      }
      if (!/^\d/.test(field.name)) earlierFields.add(field.name);
    }
  }
  const dataTypes = new Map<string, HirData>();
  const enumTypes = new Map<string, HirEnum>();
  const traitTypes = new Map<string, HirTrait>();
  program.data.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({ code: "prelude-name-shadow", message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`, span: declaration.span });
      return;
    }
    if (dataTypes.has(declaration.name)) {
      diagnostics.push({ code: "duplicate-type", message: `type '${declaration.name}' is already declared`, span: declaration.span });
      return;
    }
    dataTypes.set(declaration.name, { name: declaration.name, index, genericParameters: declaration.genericParameters, fields: [], span: declaration.span });
  });
  program.enums.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({ code: "prelude-name-shadow", message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`, span: declaration.span });
      return;
    }
    if (dataTypes.has(declaration.name) || enumTypes.has(declaration.name)) {
      diagnostics.push({ code: "duplicate-type", message: `type '${declaration.name}' is already declared`, span: declaration.span });
      return;
    }
    enumTypes.set(declaration.name, { name: declaration.name, index, genericParameters: declaration.genericParameters, sharedFields: [], variants: [], fields: [], span: declaration.span });
  });
  program.traits.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({ code: "prelude-name-shadow", message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`, span: declaration.span });
      return;
    }
    if (dataTypes.has(declaration.name) || enumTypes.has(declaration.name) || traitTypes.has(declaration.name)) {
      diagnostics.push({ code: "duplicate-type", message: `type '${declaration.name}' is already declared`, span: declaration.span });
      return;
    }
    traitTypes.set(declaration.name, { name: declaration.name, index, genericParameters: declaration.genericParameters, methods: [], span: declaration.span });
  });
  let nextEnumIndex = program.enums.length;
  for (const [localName, importedName] of imports) {
    if (importedName !== "std.resource.ResourceError") continue;
    const declaration = program.uses.find((useDeclaration) => useDeclaration.names.some((name) => (name.alias ?? name.name) === localName));
    const span = declaration?.span ?? program.span;
    if (dataTypes.has(localName) || enumTypes.has(localName) || traitTypes.has(localName)) {
      diagnostics.push({ code: "duplicate-module-name", message: `imported type '${localName}' conflicts with a local type`, span });
      continue;
    }
    const operationField = { name: "error", type: "generic:E", index: 0, span };
    enumTypes.set(localName, {
      name: localName,
      index: nextEnumIndex++,
      genericParameters: ["E"],
      sharedFields: [],
      variants: [
        { name: "Operation", tag: 0, fields: [operationField], span },
        { name: "Disposed", tag: 1, fields: [], span },
      ],
      fields: [operationField],
      span,
    });
  }
  for (const declaration of program.data) {
    const data = dataTypes.get(declaration.name);
    if (!data) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter)) diagnostics.push({ code: "prelude-name-shadow", message: `generic parameter '${parameter}' shadows a prelude name`, span: declaration.span });
    }
    const names = new Set<string>();
    const fields = declaration.fields.map((field, index) => {
      if (names.has(field.name)) diagnostics.push({ code: field.embedded ? "duplicate-embedded-field" : "duplicate-data-field", message: `field '${field.name}' is declared more than once`, span: field.span });
      names.add(field.name);
      const type = typeName(field.type, dataTypes, enumTypes, traitTypes, diagnostics, new Set(declaration.genericParameters)) ?? "void";
      if (type === "void") diagnostics.push({ code: "void-data-field", message: "a data field cannot have type void", span: field.span });
      return {
        name: field.name,
        type,
        index,
        embedded: field.embedded,
        defaultFunctionName: field.default ? `$default.${declaration.name}.${field.name}` : undefined,
        span: field.span,
      };
    });
    dataTypes.set(declaration.name, { ...data, fields });
  }
  for (const declaration of program.enums) {
    const enumType = enumTypes.get(declaration.name);
    if (!enumType) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter)) diagnostics.push({ code: "prelude-name-shadow", message: `generic parameter '${parameter}' shadows a prelude name`, span: declaration.span });
    }
    const variantNames = new Set<string>();
    const allFields: HirData["fields"][number][] = [];
    const sharedNames = new Set<string>();
    const sharedFields = declaration.sharedFields.map((field) => {
      if (sharedNames.has(field.name)) diagnostics.push({ code: "duplicate-data-field", message: `shared enum field '${field.name}' is declared more than once`, span: field.span });
      sharedNames.add(field.name);
      const type = typeName(field.type, dataTypes, enumTypes, traitTypes, diagnostics, new Set(declaration.genericParameters)) ?? "void";
      if (type === "void") diagnostics.push({ code: "void-data-field", message: "a shared enum field cannot have type void", span: field.span });
      const checked = {
        name: field.name,
        type,
        index: allFields.length,
        defaultFunctionName: field.default ? `$enum-default.${declaration.name}.${field.name}` : undefined,
        span: field.span,
      };
      allFields.push(checked);
      return checked;
    });
    const variants = declaration.variants.map((variant, tag) => {
      if (variantNames.has(variant.name)) diagnostics.push({ code: "duplicate-variant", message: `variant '${variant.name}' is declared more than once`, span: variant.span });
      variantNames.add(variant.name);
      const fieldNames = new Set<string>();
      const fields = variant.fields.map((field) => {
        if (sharedNames.has(field.name)) diagnostics.push({ code: "duplicate-data-field", message: `payload field '${field.name}' duplicates a shared enum field`, span: field.span });
        if (fieldNames.has(field.name)) diagnostics.push({ code: "duplicate-data-field", message: `payload field '${field.name}' is declared more than once`, span: field.span });
        fieldNames.add(field.name);
        const type = typeName(field.type, dataTypes, enumTypes, traitTypes, diagnostics, new Set(declaration.genericParameters)) ?? "void";
        if (type === "void") diagnostics.push({ code: "void-data-field", message: "an enum payload cannot have type void", span: field.span });
        const checked = { name: field.name, type, index: allFields.length, span: field.span };
        allFields.push(checked);
        return checked;
      });
      return {
        name: variant.name,
        tag,
        fields,
        factoryFunctionName: sharedFields.length > 0 ? `$enum-variant.${declaration.name}.${variant.name}` : undefined,
        span: variant.span,
      };
    });
    enumTypes.set(declaration.name, { ...enumType, sharedFields, variants, fields: allFields });
  }
  for (const declaration of program.traits) {
    const trait = traitTypes.get(declaration.name);
    if (!trait) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter)) diagnostics.push({ code: "prelude-name-shadow", message: `generic parameter '${parameter}' shadows a prelude name`, span: declaration.span });
    }
    if (declaration.genericParameters.length > 0 && declaration.methods.length > 0) {
      diagnostics.push({ code: "unsupported-generic-trait-method", message: `generic trait '${declaration.name}' is currently supported only as a provider marker`, span: declaration.span });
      continue;
    }
    const names = new Set<string>();
    const methods = declaration.methods.map((method, index) => {
      if (names.has(method.name)) diagnostics.push({ code: "duplicate-trait-member", message: `trait member '${method.name}' is declared more than once`, span: method.span });
      names.add(method.name);
      if (method.parameters[0]?.name !== "self") diagnostics.push({ code: "unsupported-associated-function", message: `trait method '${method.name}' requires a self receiver in the initial dictionary slice`, span: method.span });
      const sourceParameters = method.parameters.slice(1);
      sourceParameters.forEach((parameter, parameterIndex) => {
        if (parameter.variadic && parameterIndex !== sourceParameters.length - 1) {
          diagnostics.push({ code: "nonfinal-vararg", message: `variadic parameter '${parameter.name}' must be the final parameter`, span: parameter.span });
        }
      });
      const parameters = sourceParameters.map((parameter) => {
        const type = typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics);
        return type && parameter.variadic ? nominalGenericType("list", [type]) : type;
      });
      if (method.result.name === "Self" || method.parameters.slice(1).some((parameter) => parameter.type.name === "Self")) {
        diagnostics.push({ code: "unsafe-dynamic-trait", message: "Self may appear only as the receiver of a dynamic trait method", span: method.span });
      }
      const result = typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
      return { name: method.name, index, suspending: method.suspending, receiverMutable: method.parameters[0]?.type.name === "mut:Self", parameters: parameters.map((parameter) => parameter ?? "void"), parameterNames: sourceParameters.map((parameter) => parameter.name), variadic: sourceParameters.at(-1)?.variadic === true, result, requirements: method.requirements, span: method.span };
    });
    traitTypes.set(declaration.name, { ...trait, methods });
  }

  const implementationPreparations: Array<{
    declaration: ImplDecl;
    trait: HirTrait;
    targetType: ValueType;
    methods: Array<{ methodIndex: number; declaration: FunctionDecl }>;
  }> = [];
  const inherentMethods: InherentMethod[] = [];
  const inherentDeclarations: FunctionDecl[] = [];
  const inherentMethodKeys = new Set<string>();
  const implementationPairs = new Set<string>();
  const orderedImplementationEntries = [...program.implementations.entries()].sort((left, right) => (
    Number(left[1].traitName !== undefined) - Number(right[1].traitName !== undefined)
  ));
  for (const [implementationIndex, implementation] of orderedImplementationEntries) {
    if (implementation.traitName === undefined) {
      const target = dataTypes.get(implementation.targetName) ?? enumTypes.get(implementation.targetName);
      if (!target) {
        diagnostics.push({ code: "unknown-type", message: `unknown inherent implementation target '${implementation.targetName}'`, span: implementation.span });
        continue;
      }
      if (target.genericParameters.length > 0) {
        diagnostics.push({ code: "unsupported-generic-impl", message: `inherent implementation of generic type '${implementation.targetName}' requires explicit generic parameters`, span: implementation.span });
        continue;
      }
      for (const method of implementation.methods) {
        const key = `${implementation.targetName}.${method.name}`;
        if (inherentMethodKeys.has(key)) {
          diagnostics.push({ code: "duplicate-inherent-member", message: `inherent method '${key}' is declared more than once`, span: method.span });
          continue;
        }
        inherentMethodKeys.add(key);
        if (method.parameters[0]?.name !== "self") {
          diagnostics.push({ code: "unsupported-associated-function", message: `inherent member '${key}' requires a self receiver in the MVP method slice`, span: method.span });
          continue;
        }
        const sourceParameters = method.parameters.slice(1);
        sourceParameters.forEach((parameter, parameterIndex) => {
          if (parameter.variadic && parameterIndex !== sourceParameters.length - 1) {
            diagnostics.push({ code: "nonfinal-vararg", message: `variadic parameter '${parameter.name}' must be the final parameter`, span: parameter.span });
          }
        });
        const parameters = sourceParameters.map((parameter) => {
          const resolved = typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
          return parameter.variadic ? nominalGenericType("list", [resolved]) : resolved;
        });
        const result = typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
        const functionName = `$inherent${implementationIndex}.${method.name}`;
        inherentMethods.push({
          targetType: implementation.targetName,
          name: method.name,
          receiverMutable: method.parameters[0]!.type.name === "mut:Self",
          parameters,
          parameterNames: sourceParameters.map((parameter) => parameter.name),
          variadic: sourceParameters.at(-1)?.variadic === true,
          suspending: method.suspending,
          result,
          requirements: method.requirements,
          functionName,
          span: method.span,
        });
        inherentDeclarations.push({
          kind: "function",
          name: functionName,
          suspending: method.suspending,
          genericParameters: [],
          genericBounds: [],
          parameters: method.parameters.map((parameter) => ({
            ...parameter,
            type: parameter.name === "self"
              ? { name: parameter.type.name === "mut:Self" ? mutableType(implementation.targetName) : implementation.targetName, span: parameter.type.span }
              : parameter.type,
          })),
          result: method.result,
          requirements: method.requirements,
          body: method.body ?? [],
          span: method.span,
        });
      }
      continue;
    }
    const trait = traitTypes.get(implementation.traitName);
    if (!trait) {
      diagnostics.push({ code: "unknown-trait", message: `unknown trait '${implementation.traitName}'`, span: implementation.span });
      continue;
    }
    if (trait.genericParameters.length > 0) {
      diagnostics.push({ code: "unsupported-generic-impl", message: `implementation of generic trait '${trait.name}' requires explicit trait arguments`, span: implementation.span });
      continue;
    }
    if (!dataTypes.has(implementation.targetName) && !enumTypes.has(implementation.targetName)) {
      diagnostics.push({ code: "unknown-type", message: `unknown implementation target '${implementation.targetName}'`, span: implementation.span });
      continue;
    }
    const pair = `${trait.index}:${implementation.targetName}`;
    if (implementationPairs.has(pair)) {
      diagnostics.push({ code: "duplicate-trait-impl", message: `${implementation.targetName} implements ${trait.name} more than once`, span: implementation.span });
      continue;
    }
    implementationPairs.add(pair);
    const supplied = new Map<string, typeof implementation.methods[number]>();
    for (const method of implementation.methods) {
      if (supplied.has(method.name)) diagnostics.push({ code: "duplicate-impl-member", message: `implementation member '${method.name}' is declared more than once`, span: method.span });
      supplied.set(method.name, method);
    }
    for (const method of implementation.methods) {
      if (!trait.methods.some((required) => required.name === method.name)) {
        diagnostics.push({ code: "extra-trait-method", message: `method '${method.name}' is not declared by trait ${trait.name}`, span: method.span });
      }
    }
    const methods: Array<{ methodIndex: number; declaration: FunctionDecl }> = [];
    for (const required of trait.methods) {
      const suppliedMethod = supplied.get(required.name);
      const defaultMethod = program.traits[trait.index]!.methods[required.index];
      let method = suppliedMethod ?? (defaultMethod?.body ? defaultMethod : undefined);
      if (!method) {
        const target = dataTypes.get(implementation.targetName);
        const promoted = target?.fields.flatMap((field) => {
          if (!field.embedded) return [];
          const embeddedType = readonlyType(field.type);
          return inherentMethods
            .filter((candidate) => candidate.targetType === embeddedType && candidate.name === required.name)
            .map((candidate) => ({ field, candidate }));
        }) ?? [];
        if (required.receiverMutable && promoted.length > 0) {
          diagnostics.push({ code: "promoted-mutable-requirement", message: `mutable requirement '${trait.name}.${required.name}' cannot be filled through an embedded readonly edge`, span: implementation.span });
          continue;
        }
        const compatible = promoted.filter(({ candidate }) => !candidate.receiverMutable
          && candidate.suspending === required.suspending
          && candidate.parameters.length === required.parameters.length
          && candidate.parameters.every((parameter, index) => parameter === required.parameters[index])
          && candidate.result === required.result
          && sameRequirements(candidate.requirements, required.requirements));
        if (compatible.length === 1 && defaultMethod) {
          const promotedMethod = compatible[0]!;
          const receiver: Expression = {
            kind: "member",
            receiver: { kind: "name", name: "self", span: defaultMethod.span },
            name: promotedMethod.field.name,
            span: defaultMethod.span,
          };
          const call: Expression = {
            kind: required.suspending ? "suspend-call" : "call",
            callee: { kind: "member", receiver, name: required.name, span: defaultMethod.span },
            arguments: defaultMethod.parameters.slice(1).map((parameter) => ({ kind: "name" as const, name: parameter.name, span: parameter.span })),
            span: defaultMethod.span,
          };
          method = { ...defaultMethod, body: [{ kind: "expression", expression: call, span: defaultMethod.span }] };
        }
      }
      if (!method) {
        diagnostics.push({ code: "missing-trait-method", message: `${implementation.targetName} does not implement ${trait.name}.${required.name}`, span: implementation.span });
        continue;
      }
      if (suppliedMethod) {
        const parameterTypes = method.parameters.map((parameter) => {
          if (parameter.name === "self") {
            return parameter.type.name === "mut:Self"
              ? mutableType(implementation.targetName)
              : implementation.targetName;
          }
          const type = typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
          return parameter.variadic ? nominalGenericType("list", [type]) : type;
        });
        const expectedParameters = [required.receiverMutable ? mutableType(implementation.targetName) : implementation.targetName, ...required.parameters];
        const result = typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
        if (method.parameters[0]?.name !== "self"
          || parameterTypes.length !== expectedParameters.length
          || parameterTypes.some((parameter, index) => parameter !== expectedParameters[index])
          || (method.parameters.at(-1)?.variadic === true) !== required.variadic
          || result !== required.result
          || method.suspending !== required.suspending
          || !sameRequirements(method.requirements, required.requirements)) {
          diagnostics.push({ code: "trait-method-signature", message: `method '${method.name}' does not match ${trait.name}.${required.name}`, span: method.span });
        }
      }
      methods.push({
        methodIndex: required.index,
        declaration: {
          kind: "function",
          name: `$impl${implementationIndex}.${method.name}`,
          suspending: method.suspending,
          genericParameters: [],
          genericBounds: [],
          parameters: method.parameters.map((parameter) => ({
            ...parameter,
            type: parameter.name === "self"
              ? { name: parameter.type.name === "mut:Self" ? mutableType(implementation.targetName) : implementation.targetName, span: parameter.type.span }
              : parameter.type,
          })),
          result: method.result,
          requirements: method.requirements,
          body: method.body ?? [],
          span: method.span,
        },
      });
    }
    implementationPreparations.push({ declaration: implementation, trait, targetType: implementation.targetName, methods });
  }
  const defaultDeclarations: FunctionDecl[] = program.data.flatMap((declaration) => declaration.fields.flatMap((field) => field.default ? [{
    kind: "function" as const,
    name: `$default.${declaration.name}.${field.name}`,
    suspending: false,
    genericParameters: declaration.genericParameters,
    genericBounds: [],
    parameters: [],
    result: field.type,
    requirements: [],
    body: [{ kind: "expression" as const, expression: field.default, span: field.default.span }],
    span: field.default.span,
  }] : []));
  const parameterDefaultDeclarations: FunctionDecl[] = program.functions.flatMap((declaration) => declaration.parameters.flatMap((parameter, parameterIndex) => parameter.default ? [{
    kind: "function" as const,
    name: `$parameter-default.${declaration.name}.${parameter.name}`,
    suspending: false,
    genericParameters: declaration.genericParameters,
    genericBounds: declaration.genericBounds,
    parameters: declaration.parameters.slice(0, parameterIndex).map((earlier) => ({
      name: earlier.name,
      type: earlier.type,
      span: earlier.span,
    })),
    result: parameter.type,
    requirements: [],
    body: [{ kind: "expression" as const, expression: parameter.default, span: parameter.default.span }],
    span: parameter.default.span,
  }] : []));
  const enumDefaultDeclarations: FunctionDecl[] = program.enums.flatMap((declaration) => declaration.sharedFields.flatMap((field, fieldIndex) => field.default ? [{
    kind: "function" as const,
    name: `$enum-default.${declaration.name}.${field.name}`,
    suspending: false,
    genericParameters: declaration.genericParameters,
    genericBounds: [],
    parameters: declaration.sharedFields.slice(0, fieldIndex).map((earlier, earlierIndex) => ({
      name: /^\d/.test(earlier.name) ? `$enumShared${earlierIndex}` : earlier.name,
      type: earlier.type,
      span: earlier.span,
    })),
    result: field.type,
    requirements: [],
    body: [{ kind: "expression" as const, expression: field.default, span: field.default.span }],
    span: field.default.span,
  }] : []));
  const enumVariantDeclarations: FunctionDecl[] = [];
  for (const declaration of program.enums) {
    for (const variant of declaration.variants) {
      if (declaration.sharedFields.length === 0) {
        if (variant.result) diagnostics.push({ code: "unsupported-gadt-result", message: "explicit variant results without shared enum data are outside the current MVP slice", span: variant.result.span });
        continue;
      }
      if (!variant.result) {
        diagnostics.push({ code: "missing-variant-result", message: `variant '${variant.name}' must initialize shared enum data`, span: variant.span });
        continue;
      }
      if (variant.result.kind !== "call" || variant.result.callee.kind !== "name" || variant.result.callee.name !== declaration.name) {
        diagnostics.push({ code: "variant-result-owner", message: `variant '${variant.name}' must construct ${declaration.name}`, span: variant.result.span });
        continue;
      }
      const argumentNames = variant.result.argumentNames ?? variant.result.arguments.map(() => undefined);
      let nextPositional = 0;
      const seen = new Set<number>();
      const explicit: Array<{ readonly fieldIndex: number; readonly value: Expression }> = [];
      let valid = true;
      variant.result.arguments.forEach((value, argumentIndex) => {
        const name = argumentNames[argumentIndex];
        const fieldIndex = name === undefined
          ? nextPositional++
          : declaration.sharedFields.findIndex((field) => field.name === name);
        if (fieldIndex < 0 || fieldIndex >= declaration.sharedFields.length) {
          diagnostics.push({ code: name === undefined ? "argument-count" : "unknown-named-argument", message: name === undefined ? `${declaration.name} received too many positional arguments` : `${declaration.name} has no shared field '${name}'`, span: value.span });
          valid = false;
          return;
        }
        if (seen.has(fieldIndex)) {
          diagnostics.push({ code: "duplicate-argument", message: `shared field '${declaration.sharedFields[fieldIndex]!.name}' is initialized more than once`, span: value.span });
          valid = false;
          return;
        }
        seen.add(fieldIndex);
        explicit.push({ fieldIndex, value });
      });
      const missing = declaration.sharedFields.filter((_, fieldIndex) => !seen.has(fieldIndex));
      const missingRequired = missing.find((field) => !field.default);
      if (missingRequired) {
        diagnostics.push({ code: "missing-required-field", message: `variant '${variant.name}' does not initialize shared field '${missingRequired.name}'`, span: variant.result.span });
        valid = false;
      }
      if (!valid) continue;
      const localName = (fieldIndex: number): string => `$enumShared${fieldIndex}`;
      const body: Statement[] = explicit.map(({ fieldIndex, value }) => ({
        kind: "binding",
        name: localName(fieldIndex),
        annotation: declaration.sharedFields[fieldIndex]!.type,
        mutable: false,
        value,
        span: value.span,
      }));
      for (const field of missing) {
        const fieldIndex = declaration.sharedFields.indexOf(field);
        const call: Expression = {
          kind: "call",
          callee: { kind: "name", name: `$enum-default.${declaration.name}.${field.name}`, span: field.span },
          arguments: declaration.sharedFields.slice(0, fieldIndex).map((_, earlierIndex) => ({ kind: "name" as const, name: localName(earlierIndex), span: field.span })),
          span: field.span,
        };
        body.push({ kind: "binding", name: localName(fieldIndex), annotation: field.type, mutable: false, value: call, span: field.span });
      }
      const resultType: TypeRef = {
        name: declaration.genericParameters.length > 0
          ? nominalGenericType(declaration.name, declaration.genericParameters)
          : declaration.name,
        span: variant.span,
      };
      const literal: Expression = {
        kind: "call",
        callee: { kind: "name", name: `$enum-literal.${declaration.name}.${variant.name}`, span: variant.span },
        arguments: [
          ...declaration.sharedFields.map((_, fieldIndex) => ({ kind: "name" as const, name: localName(fieldIndex), span: variant.span })),
          ...variant.fields.map((field) => ({ kind: "name" as const, name: field.name, span: field.span })),
        ],
        span: variant.span,
      };
      body.push({ kind: "expression", expression: literal, span: variant.span });
      enumVariantDeclarations.push({
        kind: "function",
        name: `$enum-variant.${declaration.name}.${variant.name}`,
        suspending: false,
        genericParameters: declaration.genericParameters,
        genericBounds: [],
        parameters: variant.fields.map((field) => ({ name: field.name, type: field.type, span: field.span })),
        result: resultType,
        requirements: [],
        body,
        span: variant.span,
      });
    }
  }
  const testDeclarations: FunctionDecl[] = program.tests.map((test, index) => ({
    kind: "function",
    name: `$test.${index}`,
    suspending: true,
    genericParameters: [],
    genericBounds: [],
    parameters: [],
    result: { name: "void", span: test.span },
    requirements: [],
    body: test.body,
    span: test.span,
  }));
  const declarations = [...program.functions, ...testDeclarations, ...parameterDefaultDeclarations, ...defaultDeclarations, ...enumDefaultDeclarations, ...enumVariantDeclarations, ...inherentDeclarations];
  if (program.statements.length > 0) {
    if (declarations.some((declaration) => declaration.name === "main")) {
      diagnostics.push({ code: "main-conflict", message: "top-level statements conflict with a declared main function", span: program.statements[0]!.span });
      return { diagnostics };
    }
    declarations.push({
      kind: "function",
      name: "main",
      suspending: false,
      genericParameters: [],
      genericBounds: [],
      parameters: [],
      result: { name: "void", span: program.span },
      requirements: [],
      body: program.statements,
      span: program.span,
    });
  }
  for (const implementation of implementationPreparations) {
    for (const method of implementation.methods) declarations.push(method.declaration);
  }

  const signatures = new Map<string, Signature>();
  declarations.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({ code: "prelude-name-shadow", message: `function '${declaration.name}' shadows a prelude name`, span: declaration.span });
      return;
    }
    if (signatures.has(declaration.name)) {
      diagnostics.push({ code: "duplicate-module-name", message: `function '${declaration.name}' is declared more than once`, span: declaration.span });
      return;
    }
    if (dataTypes.has(declaration.name) || enumTypes.has(declaration.name) || traitTypes.has(declaration.name)) {
      diagnostics.push({ code: "duplicate-module-name", message: `'${declaration.name}' is already declared as a type`, span: declaration.span });
      return;
    }
    const declaredGenerics = new Set(declaration.genericParameters);
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter)) diagnostics.push({ code: "prelude-name-shadow", message: `generic parameter '${parameter}' shadows a prelude name`, span: declaration.span });
    }
    const rowParameterSet = new Set<string>();
    for (const requirement of declaration.requirements) {
      const base = requirement.split("\\")[0]!;
      if (declaredGenerics.has(base)) rowParameterSet.add(base);
    }
    declaration.parameters.forEach((parameter) => collectRowParameterReferences(parameter.type.name, declaredGenerics, rowParameterSet));
    collectRowParameterReferences(declaration.result.name, declaredGenerics, rowParameterSet);
    const rowParameters = declaration.genericParameters.filter((parameter) => rowParameterSet.has(parameter));
    const typeParameters = declaration.genericParameters.filter((parameter) => !rowParameterSet.has(parameter));
    const referenceParameters = new Set<string>();
    const genericBounds = declaration.genericBounds.flatMap((bound) => {
      if (rowParameterSet.has(bound.parameter)) {
        diagnostics.push({ code: "generic-kind-conflict", message: `generic parameter '${bound.parameter}' cannot be both a type and a requirement row`, span: bound.span });
        return [];
      }
      const seen = new Set<string>();
      return bound.traits.flatMap((traitName) => {
        if (seen.has(traitName)) {
          diagnostics.push({ code: "duplicate-trait-bound", message: `trait '${traitName}' bounds '${bound.parameter}' more than once`, span: bound.span });
          return [];
        }
        seen.add(traitName);
        if (traitName === "Reference") {
          referenceParameters.add(bound.parameter);
          return [];
        }
        const trait = traitTypes.get(traitName);
        if (!trait) {
          diagnostics.push({ code: "unknown-trait", message: `unknown trait '${traitName}'`, span: bound.span });
          return [];
        }
        return [{ parameter: bound.parameter, traitName: trait.name, traitIndex: trait.index }];
      });
    });
    if (declaration.genericParameters.length > 0 && declaration.name === "main") {
      diagnostics.push({ code: "generic-entry-point", message: "main cannot declare generic parameters", span: declaration.span });
      return;
    }
    const genericParameters = new Set(typeParameters);
    declaration.parameters.forEach((parameter, parameterIndex) => {
      if (parameter.variadic && parameterIndex !== declaration.parameters.length - 1) {
        diagnostics.push({ code: "nonfinal-vararg", message: `variadic parameter '${parameter.name}' must be the final parameter`, span: parameter.span });
      }
    });
    const parameters = declaration.parameters.map((parameter) => {
      const type = typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics, genericParameters, new Set(rowParameters));
      return type && parameter.variadic ? nominalGenericType("list", [type]) : type;
    });
    const result = typeName(declaration.result, dataTypes, enumTypes, traitTypes, diagnostics, genericParameters, new Set(rowParameters));
    if (parameters.some((type) => type === undefined) || !result) return;
    const requirements = declaration.requirements
      .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameterSet))
      .map((requirement) => rowParameterName(requirement) ? requirement : resolveGenericType(requirement, genericParameters, new Set(rowParameters)));
    for (const requirement of requirements) {
      if (rowParameterName(requirement)) continue;
      const nominal = nominalGenericParts(requirement);
      if (!nominal) continue;
      const trait = traitTypes.get(nominal.name);
      if (!trait) {
        diagnostics.push({ code: "unknown-requirement", message: `unknown generic requirement key '${requirement}'`, span: declaration.span });
        continue;
      }
      if (trait.genericParameters.length !== nominal.arguments.length) {
        diagnostics.push({ code: "generic-arity", message: `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`, span: declaration.span });
      }
    }
    if (declaration.public) {
      const privateType = declaration.parameters
        .map((parameter) => firstPrivateSignatureType(parameter.type.name, program))
        .find((candidate) => candidate !== undefined)
        ?? firstPrivateSignatureType(declaration.result.name, program);
      const privateRequirement = declaration.requirements
        .map((requirement) => firstPrivateSignatureType(requirement.split("\\")[0]!, program))
        .find((candidate) => candidate !== undefined);
      if (privateType || privateRequirement) {
        const leaked = privateType ?? privateRequirement!;
        diagnostics.push({ code: "private-type-leak", message: `public function '${declaration.name}' exposes private type or trait '${leaked}'`, span: declaration.span });
      }
      if (declaration.name === "main") {
        const nonhost = requirements.find((requirement) => !rowParameterName(requirement) && !MVP_HOST_CAPABILITIES.has(requirement));
        if (nonhost) {
          diagnostics.push({ code: "nonhost-entry-requirement", message: `entry point requirement '${nonhost}' is not supplied by the MVP host profile`, span: declaration.span });
        }
        const entryResult = resultParts(result);
        if (result !== "void" && !(entryResult?.ok === "void" && entryResult.error === "ConsoleError")) {
          diagnostics.push({ code: "entry-error-not-display", message: "public main must return void or Result[void, E] with a supported Display error", span: declaration.result.span });
        }
        if (declaration.parameters.length > 0) {
          diagnostics.push({ code: "entry-point-parameters", message: "public main cannot declare source-level parameters", span: declaration.span });
        }
      }
    }
    signatures.set(declaration.name, { name: declaration.name, index, suspending: declaration.suspending, genericParameters: typeParameters, genericBounds, referenceParameters: [...referenceParameters], rowParameters, parameters: parameters as ValueType[], parameterNames: declaration.parameters.map((parameter) => parameter.name), defaultFunctionNames: declaration.parameters.map((parameter) => parameter.default ? `$parameter-default.${declaration.name}.${parameter.name}` : undefined), variadic: declaration.parameters.at(-1)?.variadic === true, result, requirements, span: declaration.span });
  });
  if (diagnostics.length > 0) return { diagnostics };

  const implementations: HirTraitImplementation[] = implementationPreparations.map((implementation, index) => ({
    index,
    traitIndex: implementation.trait.index,
    traitName: implementation.trait.name,
    targetType: implementation.targetType,
    methodFunctions: implementation.methods.map((method) => ({
      methodIndex: method.methodIndex,
      functionIndex: signatures.get(method.declaration.name)!.index,
    })),
    span: implementation.declaration.span,
  }));

  const functions: HirFunction[] = [];
  const closures: HirFunction[] = [];
  declarations.forEach((declaration) => {
    const signature = signatures.get(declaration.name)!;
    const moduleBody = program.statements.length > 0 && declaration.body === program.statements;
    const checked = new FunctionChecker(declaration, signature, signatures, dataTypes, enumTypes, traitTypes, implementations, inherentMethods, !program.functions.includes(declaration), moduleBody, closures, false, new Map(), -1, new Map(), false, false, undefined, imports).check();
    diagnostics.push(...checked.diagnostics);
    if (checked.function) functions.push(checked.function);
  });
  return diagnostics.some((diagnostic) => diagnostic.severity !== "warning")
    ? { diagnostics }
    : { program: { data: [...dataTypes.values()], enums: [...enumTypes.values()], traits: [...traitTypes.values()], implementations, functions, closures }, diagnostics };
}
