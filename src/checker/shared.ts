import type { Expression, Program, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type {
  HirData,
  HirEnum,
  HirExpression,
  HirTrait,
  HirTraitImplementation,
  ValueType,
} from "../hir.ts";
import {
  contextKeys,
  functionParts,
  functionType,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  optionalInner,
  readonlyType,
  resultParts,
  tupleParts,
  tupleType,
} from "../types.ts";

interface NamedParameter {
  readonly name: string;
}

interface TypeChildren {
  readonly head: string;
  readonly values: readonly ValueType[];
}

export interface BindingExpressionFlow {
  readonly all: ReadonlySet<string>;
  readonly always: ReadonlySet<string>;
  readonly whenFalse: ReadonlySet<string>;
  readonly whenTrue: ReadonlySet<string>;
}

export interface IterableInfo {
  readonly iteratorFunctionIndex?: number;
  readonly iteratorKind: "iterator" | "list" | "map" | "trait";
  readonly yieldType: ValueType;
}

export function iterableInfo(
  iterable: HirExpression,
  implementations: readonly HirTraitImplementation[],
): IterableInfo | undefined {
  const nominal = nominalGenericParts(readonlyType(iterable.type));
  if (nominal?.name === "list" && nominal.arguments.length === 1)
    return { iteratorKind: "list", yieldType: nominal.arguments[0]! };
  if (nominal?.name === "map" && nominal.arguments.length === 2)
    return { iteratorKind: "map", yieldType: tupleType(nominal.arguments) };
  if (
    nominal?.name === "Iterator" &&
    nominal.arguments.length === 1 &&
    mutableInner(iterable.type) !== undefined
  )
    return { iteratorKind: "iterator", yieldType: nominal.arguments[0]! };
  const implementation = implementations.find(
    (candidate) =>
      candidate.traitName === "Iterator" &&
      readonlyType(candidate.targetType) === readonlyType(iterable.type) &&
      candidate.traitArguments.length === 1,
  );
  const iteratorFunctionIndex = implementation?.methodFunctions.find(
    (method) => method.methodIndex === 0,
  )?.functionIndex;
  if (!implementation || iteratorFunctionIndex === undefined) return undefined;
  return {
    iteratorKind: "trait",
    iteratorFunctionIndex,
    yieldType: implementation.traitArguments[0]!,
  };
}

function unionNames(...sets: readonly ReadonlySet<string>[]): Set<string> {
  return new Set(sets.flatMap((set) => [...set]));
}

function intersectNames(left: ReadonlySet<string>, right: ReadonlySet<string>): Set<string> {
  return new Set([...left].filter((name) => right.has(name)));
}

function sequentialBindingFlow(expressions: readonly Expression[]): BindingExpressionFlow {
  const flows = expressions.map(bindingExpressionFlow);
  const all = unionNames(...flows.map((flow) => flow.all));
  const always = unionNames(...flows.map((flow) => flow.always));
  return { all, always, whenFalse: always, whenTrue: always };
}

export function bindingExpressionFlow(expression: Expression): BindingExpressionFlow {
  if (expression.kind === "binding-expression") {
    const nested = bindingExpressionFlow(expression.value);
    const names = new Set(expression.bindings.map((binding) => binding.name));
    return {
      all: unionNames(nested.all, names),
      always: unionNames(nested.always, names),
      whenFalse: unionNames(nested.whenFalse, names),
      whenTrue: unionNames(nested.whenTrue, names),
    };
  }
  if (expression.kind === "unary" && expression.operator === "not") {
    const operand = bindingExpressionFlow(expression.operand);
    return { ...operand, whenFalse: operand.whenTrue, whenTrue: operand.whenFalse };
  }
  if (expression.kind === "binary" && expression.operator === "and") {
    const left = bindingExpressionFlow(expression.left);
    const right = bindingExpressionFlow(expression.right);
    return {
      all: unionNames(left.all, right.all),
      always: intersectNames(left.whenFalse, unionNames(left.whenTrue, right.always)),
      whenFalse: intersectNames(left.whenFalse, unionNames(left.whenTrue, right.whenFalse)),
      whenTrue: unionNames(left.whenTrue, right.whenTrue),
    };
  }
  if (expression.kind === "binary" && expression.operator === "or") {
    const left = bindingExpressionFlow(expression.left);
    const right = bindingExpressionFlow(expression.right);
    return {
      all: unionNames(left.all, right.all),
      always: intersectNames(left.whenTrue, unionNames(left.whenFalse, right.always)),
      whenFalse: unionNames(left.whenFalse, right.whenFalse),
      whenTrue: intersectNames(left.whenTrue, unionNames(left.whenFalse, right.whenTrue)),
    };
  }
  return sequentialBindingFlow(eagerExpressionChildren(expression));
}

function eagerExpressionChildren(expression: Expression): readonly Expression[] {
  switch (expression.kind) {
    case "interpolated-string":
      return expression.segments.flatMap((segment) =>
        segment.kind === "expression" ? [segment.expression] : [],
      );
    case "list":
    case "tuple":
      return expression.elements;
    case "map":
      return expression.entries.flatMap((entry) => [entry.key, entry.value]);
    case "unary":
    case "propagate":
      return [expression.operand];
    case "binary":
      return [expression.left, expression.right];
    case "call":
    case "suspend-call":
      return [expression.callee, ...expression.arguments];
    case "data":
      return [
        ...(expression.spread ? [expression.spread] : []),
        ...expression.fields.map((field) => field.value),
      ];
    case "member":
      return [expression.receiver];
    case "index":
      return [expression.receiver, expression.index];
    case "provider-context":
      return expression.entries.map((entry) => entry.value);
    case "provider-with":
      return expression.entries.map((entry) => entry.value);
    case "if":
    case "while":
      return [expression.condition];
    case "for":
      return [expression.iterable];
    case "match":
      return [expression.subject];
    case "integer":
    case "float":
    case "string":
    case "character":
    case "boolean":
    case "nil":
    case "name":
    case "qualified-name":
    case "contextual-variant":
    case "closure":
    case "list-comprehension":
    case "map-comprehension":
    case "binding-expression":
    case "provider-use":
      return [];
  }
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

export function mapKeyKind(type: ValueType): 0 | 1 | undefined {
  if (type === "i32" || type === "bool" || type === "char") return 0;
  if (type === "string") return 1;
  return undefined;
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

export function statementsReferenceName(statements: readonly Statement[], name: string): boolean {
  return statements.some((statement) => nodeReferencesName(statement, name));
}

export function nodeReferencesName(value: unknown, name: string): boolean {
  if (Array.isArray(value)) return value.some((item) => nodeReferencesName(item, name));
  if (!value || typeof value !== "object") return false;
  const node = value as Record<string, unknown>;
  if (node.kind === "name" && node.name === name) return true;
  if (node.kind === "closure") {
    const parameters = node.parameters as readonly NamedParameter[];
    if (parameters.some((parameter) => parameter.name === name)) return false;
  }
  return Object.entries(node).some(
    ([key, child]) => key !== "span" && nodeReferencesName(child, name),
  );
}

export function pureFunctionNames(program: Program): ReadonlySet<string> {
  const candidates = new Map(
    program.functions
      .filter((declaration) => !declaration.suspending && declaration.requirements.length === 0)
      .map((declaration) => [declaration.name, declaration] as const),
  );
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

export function statementsArePure(
  statements: readonly Statement[],
  inheritedLocals: ReadonlySet<string>,
  pureFunctions: ReadonlySet<string>,
  program: Program,
): boolean {
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
        if (
          !locals.has(statement.name) ||
          !expressionIsPure(statement.value, locals, pureFunctions, program)
        )
          return false;
        break;
      case "discard":
        if (!expressionIsPure(statement.value, locals, pureFunctions, program)) return false;
        break;
      case "return":
      case "break":
        if (statement.value && !expressionIsPure(statement.value, locals, pureFunctions, program))
          return false;
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

export function expressionIsPure(
  expression: Expression,
  locals: ReadonlySet<string>,
  pureFunctions: ReadonlySet<string>,
  program: Program,
): boolean {
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
        ? expression.segments.every(
            (segment) =>
              segment.kind === "text" ||
              expressionIsPure(segment.expression, locals, pureFunctions, program),
          )
        : true;
    case "name":
      return locals.has(expression.name) || pureFunctions.has(expression.name);
    case "qualified-name":
      return false;
    case "binding-expression": {
      if (!expressionIsPure(expression.value, locals, pureFunctions, program)) return false;
      if (locals instanceof Set) expression.bindings.forEach((binding) => locals.add(binding.name));
      return true;
    }
    case "list":
      return expression.elements.every((element) =>
        expressionIsPure(element, locals, pureFunctions, program),
      );
    case "tuple":
      return expression.elements.every((element) =>
        expressionIsPure(element, locals, pureFunctions, program),
      );
    case "map":
      return expression.entries.every(
        (entry) =>
          expressionIsPure(entry.key, locals, pureFunctions, program) &&
          expressionIsPure(entry.value, locals, pureFunctions, program),
      );
    case "list-comprehension":
    case "map-comprehension": {
      const comprehensionLocals = new Set(locals);
      for (const clause of expression.clauses) {
        if (clause.kind === "for") {
          if (!expressionIsPure(clause.iterable, comprehensionLocals, pureFunctions, program))
            return false;
          clause.bindings.forEach((binding) => comprehensionLocals.add(binding.name));
        } else if (
          !expressionIsPure(clause.condition, comprehensionLocals, pureFunctions, program)
        ) {
          return false;
        }
      }
      if (expression.kind === "list-comprehension")
        return expressionIsPure(expression.value, comprehensionLocals, pureFunctions, program);
      return (
        expressionIsPure(expression.key, comprehensionLocals, pureFunctions, program) &&
        expressionIsPure(expression.value, comprehensionLocals, pureFunctions, program)
      );
    }
    case "unary":
    case "propagate":
      return expressionIsPure(expression.operand, locals, pureFunctions, program);
    case "binary":
      return (
        expressionIsPure(expression.left, locals, pureFunctions, program) &&
        expressionIsPure(expression.right, locals, pureFunctions, program)
      );
    case "call": {
      const argumentsPure = expression.arguments.every((argument) =>
        expressionIsPure(argument, locals, pureFunctions, program),
      );
      if (!argumentsPure) return false;
      if (expression.callee.kind === "name") {
        const calleeName = expression.callee.name;
        return (
          ["Ok", "Err", "panic"].includes(calleeName) ||
          pureFunctions.has(calleeName) ||
          program.data.some((declaration) => declaration.name === calleeName)
        );
      }
      if (expression.callee.kind === "member") {
        if (["len", "starts_with"].includes(expression.callee.name)) {
          return (
            expressionIsPure(expression.callee.receiver, locals, pureFunctions, program) &&
            expression.arguments.every((argument) =>
              expressionIsPure(argument, locals, pureFunctions, program),
            )
          );
        }
        const receiver = expression.callee.receiver;
        return (
          receiver.kind === "name" &&
          program.enums.some((declaration) => declaration.name === receiver.name)
        );
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
      return (
        (!expression.spread ||
          expressionIsPure(expression.spread, locals, pureFunctions, program)) &&
        expression.fields.every((field) =>
          expressionIsPure(field.value, locals, pureFunctions, program),
        )
      );
    case "member":
      return expressionIsPure(expression.receiver, locals, pureFunctions, program);
    case "index":
      return (
        expressionIsPure(expression.receiver, locals, pureFunctions, program) &&
        expressionIsPure(expression.index, locals, pureFunctions, program)
      );
    case "closure":
      return true;
    case "if":
      return (
        expressionIsPure(expression.condition, locals, pureFunctions, program) &&
        statementsArePure(expression.thenBody, locals, pureFunctions, program) &&
        statementsArePure(expression.elseBody, locals, pureFunctions, program)
      );
    case "for":
      return (
        expressionIsPure(expression.iterable, locals, pureFunctions, program) &&
        statementsArePure(
          expression.body,
          new Set([...locals, ...expression.bindings.map((binding) => binding.name)]),
          pureFunctions,
          program,
        ) &&
        statementsArePure(expression.elseBody, locals, pureFunctions, program)
      );
    case "while":
      return (
        expressionIsPure(expression.condition, locals, pureFunctions, program) &&
        statementsArePure(expression.body, locals, pureFunctions, program) &&
        statementsArePure(expression.elseBody, locals, pureFunctions, program)
      );
    case "match":
      return (
        expressionIsPure(expression.subject, locals, pureFunctions, program) &&
        expression.arms.every(
          (arm) =>
            (!arm.guard || expressionIsPure(arm.guard, locals, pureFunctions, program)) &&
            statementsArePure(arm.body, locals, pureFunctions, program),
        )
      );
  }
}

export function substituteGenericType(
  type: ValueType,
  substitutions: ReadonlyMap<string, ValueType>,
  rowSubstitutions: ReadonlyMap<string, readonly string[]> = new Map(),
): ValueType {
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutableType(substituteGenericType(mutable, substitutions, rowSubstitutions));
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(
      tuple.map((element) => substituteGenericType(element, substitutions, rowSubstitutions)),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return `${substituteGenericType(optional, substitutions, rowSubstitutions)}?`;
  const result = resultParts(type);
  if (result)
    return `Result[${substituteGenericType(result.ok, substitutions, rowSubstitutions)},${substituteGenericType(result.error, substitutions, rowSubstitutions)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal)
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument) =>
        substituteGenericType(argument, substitutions, rowSubstitutions),
      ),
    );
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) =>
        substituteGenericType(parameter, substitutions, rowSubstitutions),
      ),
      substituteGenericType(callable.result, substitutions, rowSubstitutions),
      callable.requirements.flatMap((requirement) => {
        const row = rowParameterName(requirement);
        return row
          ? instantiateRowRequirement(requirement, rowSubstitutions.get(row) ?? [requirement])
          : [requirement];
      }),
      callable.variadic,
      callable.suspending,
    );
  }
  const generic = genericTypeName(type);
  if (generic) return substitutions.get(generic) ?? type;
  return type;
}

export function genericTypeName(type: ValueType): string | undefined {
  const match = /^generic:([^?[\](),]+)$/.exec(type);
  return match?.[1];
}

export function traitTypeName(type: ValueType): string | undefined {
  const readonly = readonlyType(type);
  if (!readonly.startsWith("trait:")) return undefined;
  const key = readonly.slice("trait:".length);
  return nominalGenericParts(key)?.name ?? key;
}

export function traitKeyName(key: string): string {
  return nominalGenericParts(key)?.name ?? key;
}

export function requirementKeysMayCollide(left: string, right: string): boolean {
  if (left === right) return false;
  const substitutions = new Map<string, ValueType>();
  const resolve = (type: ValueType): ValueType => {
    const generic = genericTypeName(type);
    const substitution = generic && substitutions.get(generic);
    return substitution ? resolve(substitution) : type;
  };
  const children = (type: ValueType): TypeChildren => {
    const tuple = tupleParts(type);
    if (tuple !== undefined) return { head: `tuple:${tuple.length}`, values: tuple };
    const optional = optionalInner(type);
    if (optional !== undefined) return { head: "optional", values: [optional] };
    const result = resultParts(type);
    if (result) return { head: "Result", values: [result.ok, result.error] };
    const nominal = nominalGenericParts(type);
    if (nominal) return { head: `nominal:${nominal.name}`, values: nominal.arguments };
    const callable = functionParts(type);
    if (callable)
      return {
        head: `function:${callable.variadic}:${callable.requirements.join("+")}`,
        values: [...callable.parameters, callable.result],
      };
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
    return (
      firstChildren.head === secondChildren.head &&
      firstChildren.values.length === secondChildren.values.length &&
      firstChildren.values.every((child, index) => unifyTypes(child, secondChildren.values[index]!))
    );
  };
  return unifyTypes(left, right);
}

export function containsGenericType(type: ValueType): boolean {
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
  return Boolean(
    callable &&
    (callable.parameters.some(containsGenericType) || containsGenericType(callable.result)),
  );
}

export function inferGenericType(
  formal: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
  rowSubstitutions: Map<string, readonly string[]> = new Map(),
): string | undefined {
  const generic = genericTypeName(formal);
  if (generic) {
    const existing = substitutions.get(generic);
    if (existing && existing !== actual)
      return `generic parameter '${generic}' was inferred as both ${existing} and ${actual}`;
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
  if (
    formalTuple !== undefined &&
    actualTuple !== undefined &&
    formalTuple.length === actualTuple.length
  ) {
    for (let index = 0; index < formalTuple.length; index += 1) {
      const conflict = inferGenericType(
        formalTuple[index]!,
        actualTuple[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalOptional = optionalInner(formal);
  const actualOptional = optionalInner(actual);
  if (formalOptional !== undefined && actualOptional !== undefined)
    return inferGenericType(formalOptional, actualOptional, substitutions, rowSubstitutions);
  const formalResult = resultParts(formal);
  const actualResult = resultParts(actual);
  if (formalResult && actualResult) {
    return (
      inferGenericType(formalResult.ok, actualResult.ok, substitutions, rowSubstitutions) ??
      inferGenericType(formalResult.error, actualResult.error, substitutions, rowSubstitutions)
    );
  }
  const formalNominal = nominalGenericParts(formal);
  const actualNominal = nominalGenericParts(actual);
  if (
    formalNominal &&
    actualNominal &&
    formalNominal.name === actualNominal.name &&
    formalNominal.arguments.length === actualNominal.arguments.length
  ) {
    for (let index = 0; index < formalNominal.arguments.length; index += 1) {
      const conflict = inferGenericType(
        formalNominal.arguments[index]!,
        actualNominal.arguments[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    return undefined;
  }
  const formalCallable = functionParts(formal);
  const actualCallable = functionParts(actual);
  if (
    formalCallable &&
    actualCallable &&
    formalCallable.suspending === actualCallable.suspending &&
    formalCallable.variadic === actualCallable.variadic &&
    formalCallable.parameters.length === actualCallable.parameters.length
  ) {
    for (let index = 0; index < formalCallable.parameters.length; index += 1) {
      const conflict = inferGenericType(
        formalCallable.parameters[index]!,
        actualCallable.parameters[index]!,
        substitutions,
        rowSubstitutions,
      );
      if (conflict) return conflict;
    }
    const resultConflict = inferGenericType(
      formalCallable.result,
      actualCallable.result,
      substitutions,
      rowSubstitutions,
    );
    return (
      resultConflict ??
      inferRequirementRows(
        formalCallable.requirements,
        actualCallable.requirements,
        rowSubstitutions,
      )
    );
  }
  return undefined;
}

export function matchGenericTypePattern(
  pattern: ValueType,
  actual: ValueType,
  substitutions: Map<string, ValueType>,
): boolean {
  const generic = genericTypeName(pattern);
  if (generic) {
    const existing = substitutions.get(generic);
    if (existing) return existing === actual;
    substitutions.set(generic, actual);
    return true;
  }
  if (pattern === actual) return true;
  const patternMutable = mutableInner(pattern);
  const actualMutable = mutableInner(actual);
  if (patternMutable !== undefined || actualMutable !== undefined)
    return (
      patternMutable !== undefined &&
      actualMutable !== undefined &&
      matchGenericTypePattern(patternMutable, actualMutable, substitutions)
    );
  const patternTuple = tupleParts(pattern);
  const actualTuple = tupleParts(actual);
  if (patternTuple !== undefined || actualTuple !== undefined)
    return Boolean(
      patternTuple &&
      actualTuple &&
      patternTuple.length === actualTuple.length &&
      patternTuple.every((element, index) =>
        matchGenericTypePattern(element, actualTuple[index]!, substitutions),
      ),
    );
  const patternNominal = nominalGenericParts(pattern);
  const actualNominal = nominalGenericParts(actual);
  return Boolean(
    patternNominal &&
    actualNominal &&
    patternNominal.name === actualNominal.name &&
    patternNominal.arguments.length === actualNominal.arguments.length &&
    patternNominal.arguments.every((argument, index) =>
      matchGenericTypePattern(argument, actualNominal.arguments[index]!, substitutions),
    ),
  );
}

interface TraitImplementationPattern {
  readonly targetType: ValueType;
  readonly traitArguments: readonly ValueType[];
  readonly trait?: { readonly index: number };
  readonly traitIndex?: number;
}

export function matchTraitImplementation(
  implementation: TraitImplementationPattern,
  traitIndex: number,
  targetType: ValueType,
  traitArguments: readonly ValueType[],
): Map<string, ValueType> | undefined {
  if (
    (implementation.traitIndex ?? implementation.trait?.index) !== traitIndex ||
    implementation.traitArguments.length !== traitArguments.length
  )
    return undefined;
  const substitutions = new Map<string, ValueType>();
  if (!matchGenericTypePattern(implementation.targetType, targetType, substitutions))
    return undefined;
  return implementation.traitArguments.every((argument, index) =>
    matchGenericTypePattern(argument, traitArguments[index]!, substitutions),
  )
    ? substitutions
    : undefined;
}

export function rowParameterName(requirement: string): string | undefined {
  return requirement.startsWith("row:")
    ? requirement.slice("row:".length).split("\\")[0]
    : undefined;
}

export function requirementExclusions(requirement: string): readonly string[] {
  return requirement.split("\\").slice(1);
}

export function symbolicRequirement(name: string, exclusions: readonly string[] = []): string {
  return [`row:${name}`, ...normalizedRequirements(exclusions)].join("\\");
}

export function instantiateRowRequirement(
  requirement: string,
  substitution: readonly string[],
): readonly string[] {
  const exclusions = new Set(requirementExclusions(requirement));
  return substitution.flatMap((entry) => {
    const nested = rowParameterName(entry);
    if (nested)
      return [symbolicRequirement(nested, [...requirementExclusions(entry), ...exclusions])];
    return exclusions.has(entry) ? [] : [entry];
  });
}

export function normalizedRequirements(requirements: readonly string[]): readonly string[] {
  return [...new Set(requirements)].sort();
}

export function sameRequirements(left: readonly string[], right: readonly string[]): boolean {
  const normalizedLeft = normalizedRequirements(left);
  const normalizedRight = normalizedRequirements(right);
  return (
    normalizedLeft.length === normalizedRight.length &&
    normalizedLeft.every((requirement, index) => requirement === normalizedRight[index])
  );
}

export function inferRequirementRows(
  formal: readonly string[],
  actual: readonly string[],
  substitutions: Map<string, readonly string[]>,
): string | undefined {
  const rowNames = [
    ...new Set(formal.map(rowParameterName).filter((name): name is string => name !== undefined)),
  ];
  if (rowNames.length === 0) return undefined;
  const concrete = formal.filter((requirement) => !rowParameterName(requirement));
  const actualSet = new Set(actual);
  const missingConcrete = concrete.filter((requirement) => !actualSet.has(requirement));
  if (missingConcrete.length > 0) {
    return `callable requirement row is missing ${missingConcrete.join(" + ")}`;
  }
  const boundRequirements = formal.flatMap((requirement) => {
    const name = rowParameterName(requirement);
    return name && substitutions.has(name)
      ? instantiateRowRequirement(requirement, substitutions.get(name)!)
      : [];
  });
  const unbound = rowNames.filter((name) => !substitutions.has(name));
  if (unbound.length > 1) return undefined;
  if (unbound.length === 1) {
    const occupied = new Set([...concrete, ...boundRequirements]);
    substitutions.set(
      unbound[0]!,
      normalizedRequirements(actual.filter((requirement) => !occupied.has(requirement))),
    );
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

export function functionTypeMatchesRowPattern(
  formalType: ValueType,
  actualType: ValueType,
): boolean {
  const formal = functionParts(formalType);
  const actual = functionParts(actualType);
  if (
    !formal ||
    !actual ||
    formal.variadic !== actual.variadic ||
    formal.suspending !== actual.suspending ||
    formal.parameters.length !== actual.parameters.length
  )
    return false;
  if (
    !formal.parameters.every((parameter, index) => parameter === actual.parameters[index]) ||
    formal.result !== actual.result
  )
    return false;
  return inferRequirementRows(formal.requirements, actual.requirements, new Map()) === undefined;
}

export function resolveGenericType(
  type: ValueType,
  genericParameters: ReadonlySet<string>,
  rowParameters: ReadonlySet<string> = new Set(),
): ValueType {
  if (genericParameters.has(type)) return `generic:${type}`;
  const projection = /^([^:]+)::([A-Za-z_][A-Za-z0-9_]*)$/.exec(type);
  if (projection && genericParameters.has(projection[1]!)) return `generic:${type}`;
  const mutable = mutableInner(type);
  if (mutable !== undefined)
    return mutableType(resolveGenericType(mutable, genericParameters, rowParameters));
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(
      tuple.map((element) => resolveGenericType(element, genericParameters, rowParameters)),
    );
  const optional = optionalInner(type);
  if (optional !== undefined)
    return `${resolveGenericType(optional, genericParameters, rowParameters)}?`;
  const result = resultParts(type);
  if (result)
    return `Result[${resolveGenericType(result.ok, genericParameters, rowParameters)},${resolveGenericType(result.error, genericParameters, rowParameters)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal)
    return nominalGenericType(
      nominal.name,
      nominal.arguments.map((argument) =>
        resolveGenericType(argument, genericParameters, rowParameters),
      ),
    );
  const callable = functionParts(type);
  if (callable) {
    return functionType(
      callable.parameters.map((parameter) =>
        resolveGenericType(parameter, genericParameters, rowParameters),
      ),
      resolveGenericType(callable.result, genericParameters, rowParameters),
      callable.requirements.flatMap((requirement) =>
        resolveGenericRequirement(requirement, rowParameters),
      ),
      callable.variadic,
      callable.suspending,
    );
  }
  return type;
}

export function collectRowParameterReferences(
  type: ValueType,
  genericParameters: ReadonlySet<string>,
  output: Set<string>,
): void {
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
    callable.parameters.forEach((parameter) =>
      collectRowParameterReferences(parameter, genericParameters, output),
    );
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

export function resolveGenericRequirement(
  requirement: string,
  rowParameters: ReadonlySet<string>,
): readonly string[] {
  const [base, ...excluded] = requirement.split("\\");
  if (rowParameters.has(base!)) return [symbolicRequirement(base!, excluded)];
  return excluded.includes(base!) ? [] : [base!];
}

export function firstPrivateSignatureType(type: ValueType, program: Program): string | undefined {
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
  if (result)
    return (
      firstPrivateSignatureType(result.ok, program) ??
      firstPrivateSignatureType(result.error, program)
    );
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
  const declaration =
    program.data.find((candidate) => candidate.name === base) ??
    program.enums.find((candidate) => candidate.name === base) ??
    program.traits.find((candidate) => candidate.name === base);
  if (declaration && !declaration.public) return base;
  if (nominal) {
    for (const argument of nominal.arguments) {
      const privateType = firstPrivateSignatureType(argument, program);
      if (privateType) return privateType;
    }
  }
  return undefined;
}

export function typeName(
  type: TypeRef,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
  traitTypes: ReadonlyMap<string, HirTrait>,
  diagnostics: Diagnostic[],
  genericParameters: ReadonlySet<string> = new Set(),
  rowParameters: ReadonlySet<string> = new Set(),
): ValueType | undefined {
  const resolved = resolveTraitType(
    resolveGenericType(type.name, genericParameters, rowParameters),
    traitTypes,
  );
  const dynamicTraitName = traitTypeName(resolved);
  const dynamicTrait = dynamicTraitName && traitTypes.get(dynamicTraitName);
  if (dynamicTrait && !traitIsDynamicallySafe(dynamicTrait, traitTypes)) {
    diagnostics.push({
      code: "trait-not-dynamically-safe",
      message: `trait '${dynamicTrait.name}' cannot be used as a dynamic value`,
      span: type.span,
    });
  }
  const nominal = nominalGenericParts(resolved);
  if (
    nominal?.name === "map" &&
    nominal.arguments.length === 2 &&
    isKnownType(nominal.arguments[0]!, dataTypes, enumTypes, traitTypes) &&
    isKnownType(nominal.arguments[1]!, dataTypes, enumTypes, traitTypes) &&
    mapKeyKind(nominal.arguments[0]!) === undefined
  ) {
    diagnostics.push({
      code: "invalid-map-key",
      message: `type '${nominal.arguments[0]}' does not implement the MVP map-key contract`,
      span: type.span,
    });
    return undefined;
  }
  if (!isKnownType(resolved, dataTypes, enumTypes, traitTypes)) {
    diagnostics.push({
      code: "unknown-type",
      message: `unknown or unsupported type '${type.name}'`,
      span: type.span,
    });
    return undefined;
  }
  return resolved;
}

function traitIsDynamicallySafe(
  trait: HirTrait,
  traitTypes: ReadonlyMap<string, HirTrait>,
  seen: ReadonlySet<number> = new Set(),
): boolean {
  if (seen.has(trait.index)) return true;
  if (
    trait.associatedTypes.length > 0 ||
    trait.methods.some(
      (method) =>
        method.associated ||
        method.genericParameters.length > 0 ||
        method.parameters.some((parameter) => parameter.includes("generic:Self")) ||
        method.result.includes("generic:Self"),
    )
  )
    return false;
  const next = new Set([...seen, trait.index]);
  return trait.supertraits.every((supertrait) => {
    const parent = [...traitTypes.values()].find(
      (candidate) => candidate.index === supertrait.traitIndex,
    );
    return !parent || traitIsDynamicallySafe(parent, traitTypes, next);
  });
}

export function resolveTraitType(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
): ValueType {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return mutableType(resolveTraitType(mutable, traitTypes));
  if (traitTypes.has(type)) return `trait:${type}`;
  const tuple = tupleParts(type);
  if (tuple !== undefined)
    return tupleType(tuple.map((element) => resolveTraitType(element, traitTypes)));
  const optional = optionalInner(type);
  if (optional !== undefined) return `${resolveTraitType(optional, traitTypes)}?`;
  const result = resultParts(type);
  if (result)
    return `Result[${resolveTraitType(result.ok, traitTypes)},${resolveTraitType(result.error, traitTypes)}]`;
  const nominal = nominalGenericParts(type);
  if (nominal) {
    const arguments_ = nominal.arguments.map((argument) => resolveTraitType(argument, traitTypes));
    const resolved = nominalGenericType(nominal.name, arguments_);
    // Iterator[T] is also the concrete cursor type returned by the MVP list and
    // map runtime. Keep type annotations nominal while implementations still
    // resolve Iterator as the synthesized protocol trait.
    if (nominal.name === "Iterator") return resolved;
    return traitTypes.has(nominal.name) ? `trait:${resolved}` : resolved;
  }
  const callable = functionParts(type);
  if (callable)
    return functionType(
      callable.parameters.map((parameter) => resolveTraitType(parameter, traitTypes)),
      resolveTraitType(callable.result, traitTypes),
      callable.requirements,
      callable.variadic,
      callable.suspending,
    );
  return type;
}
