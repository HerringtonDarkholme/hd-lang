import type {
  Expression,
  Statement,
  Pattern,
  ComprehensionClause,
  ProviderContextEntry,
} from "../ast.ts";
import type { GenericMethodScope } from "./generic-method-scope.ts";

export function mapScopeExpression(node: Expression, scope: GenericMethodScope): Expression {
  const e = (value: Expression): Expression => scope.expression(value);
  const body = (values: readonly Statement[]): readonly Statement[] => scope.body(values);
  const types = (
    values: Expression & { typeArguments?: readonly import("../ast.ts").TypeRef[] },
  ) => (values.typeArguments ? { typeArguments: values.typeArguments.map(scope.type) } : {});
  switch (node.kind) {
    case "integer":
    case "float":
    case "string":
    case "character":
    case "boolean":
    case "contextual-variant":
      return node;
    case "name":
      return { ...node, ...types(node) };
    case "qualified-name":
      return {
        ...node,
        ...(scope.text(node.owner) !== node.owner
          ? { genericTypeOwner: scope.text(node.owner) }
          : {}),
        ...types(node),
        ...(node.ownerTypeArguments
          ? { ownerTypeArguments: node.ownerTypeArguments.map(scope.type) }
          : {}),
      };
    case "interpolated-string":
      return {
        ...node,
        segments: node.segments.map((segment) =>
          segment.kind === "expression"
            ? { ...segment, expression: e(segment.expression) }
            : segment,
        ),
      };
    case "binding-expression":
      return { ...node, value: e(node.value) };
    case "list":
    case "tuple":
      return { ...node, elements: node.elements.map(e) };
    case "list-comprehension":
      return { ...node, clauses: node.clauses.map((c) => clause(c, scope)), value: e(node.value) };
    case "map-comprehension":
      return {
        ...node,
        clauses: node.clauses.map((c) => clause(c, scope)),
        key: e(node.key),
        value: e(node.value),
      };
    case "map":
      return {
        ...node,
        entries: node.entries.map((entry) => ({
          ...entry,
          key: e(entry.key),
          value: e(entry.value),
        })),
      };
    case "unary":
    case "propagate":
      return { ...node, operand: e(node.operand) };
    case "binary":
      return { ...node, left: e(node.left), right: e(node.right) };
    case "call":
    case "suspend-call":
      return { ...node, ...types(node), callee: e(node.callee), arguments: node.arguments.map(e) };
    case "data":
      return {
        ...node,
        name: scope.text(node.name),
        ...types(node),
        fields: node.fields.map((field) => ({ ...field, value: e(field.value) })),
        ...(node.spread ? { spread: e(node.spread) } : {}),
      };
    case "member":
      return { ...node, ...types(node), receiver: e(node.receiver) };
    case "index":
      return { ...node, receiver: e(node.receiver), index: e(node.index) };
    case "range":
      return {
        ...node,
        ...(node.start ? { start: e(node.start) } : {}),
        ...(node.end ? { end: e(node.end) } : {}),
      };
    case "pipe":
      return { ...node, value: e(node.value), step: e(node.step) };
    case "closure":
      return {
        ...node,
        parameters: node.parameters.map((p) => ({
          ...p,
          ...(p.type ? { type: scope.type(p.type) } : {}),
        })),
        ...(node.result ? { result: scope.type(node.result) } : {}),
        ...(node.requirements ? { requirements: node.requirements.map(scope.text) } : {}),
        body: body(node.body),
      };
    case "provider-use":
      return { ...node, key: scope.text(node.key) };
    case "provider-context":
      return { ...node, entries: node.entries.map((entry) => provider(entry, scope)) };
    case "provider-with":
      return {
        ...node,
        entries: node.entries.map((entry) => provider(entry, scope)),
        body: body(node.body),
      };
    case "if":
      return {
        ...node,
        condition: e(node.condition),
        thenBody: body(node.thenBody),
        elseBody: body(node.elseBody),
      };
    case "while":
      return {
        ...node,
        condition: e(node.condition),
        body: body(node.body),
        elseBody: body(node.elseBody),
      };
    case "for":
      return {
        ...node,
        ...(node.pattern ? { pattern: pattern(node.pattern, scope) } : {}),
        iterable: e(node.iterable),
        body: body(node.body),
        elseBody: body(node.elseBody),
      };
    case "match":
      return {
        ...node,
        subject: e(node.subject),
        arms: node.arms.map((arm) => ({
          ...arm,
          pattern: pattern(arm.pattern, scope),
          ...(arm.guard ? { guard: e(arm.guard) } : {}),
          body: body(arm.body),
        })),
      };
    default:
      return unreachable(node);
  }
}

export function mapScopeStatement(node: Statement, scope: GenericMethodScope): Statement {
  const e = (value: Expression): Expression => scope.expression(value);
  switch (node.kind) {
    case "pass":
    case "continue":
      return node;
    case "defer":
      return { ...node, body: scope.body(node.body) };
    case "binding":
    case "tuple-binding":
      return {
        ...node,
        ...(node.annotation ? { annotation: scope.type(node.annotation) } : {}),
        value: e(node.value),
      };
    case "pattern-binding":
      return {
        ...node,
        pattern: pattern(node.pattern, scope),
        ...(node.annotation ? { annotation: scope.type(node.annotation) } : {}),
        value: e(node.value),
        ...(node.elseBody ? { elseBody: scope.body(node.elseBody) } : {}),
      };
    case "assignment":
    case "discard":
      return { ...node, value: e(node.value) };
    case "return":
    case "break":
      return { ...node, ...(node.value ? { value: e(node.value) } : {}) };
    case "field-assignment":
      return {
        ...node,
        target: {
          ...node.target,
          receiver: e(node.target.receiver),
          ...(node.target.typeArguments
            ? { typeArguments: node.target.typeArguments.map(scope.type) }
            : {}),
        },
        value: e(node.value),
      };
    case "index-assignment":
      return {
        ...node,
        target: { ...node.target, receiver: e(node.target.receiver), index: e(node.target.index) },
        value: e(node.value),
      };
    case "call-assignment":
      return {
        ...node,
        target: {
          ...node.target,
          callee: e(node.target.callee),
          arguments: node.target.arguments.map(e),
          ...(node.target.typeArguments
            ? { typeArguments: node.target.typeArguments.map(scope.type) }
            : {}),
        },
        value: e(node.value),
      };
    case "expression":
      return { ...node, expression: e(node.expression) };
    case "local-declaration":
      return { ...node, declaration: scope.declaration(node.declaration) };
    case "local-implementation":
      return node;
    default:
      return unreachable(node);
  }
}

function pattern(node: Pattern, scope: GenericMethodScope): Pattern {
  switch (node.kind) {
    case "wildcard":
    case "boolean":
    case "integer":
    case "float":
    case "string":
    case "character":
    case "range":
    case "binding":
      return node;
    case "tuple":
      return { ...node, elements: node.elements.map((p) => pattern(p, scope)) };
    case "data":
      return {
        ...node,
        typeName: scope.text(node.typeName),
        fields: node.fields.map((field) => ({ ...field, pattern: pattern(field.pattern, scope) })),
      };
    case "result-variant":
    case "variant":
      return {
        ...node,
        ...(node.kind === "variant" && node.enumName
          ? { enumName: scope.text(node.enumName) }
          : {}),
        ...(node.payloadPatterns
          ? { payloadPatterns: node.payloadPatterns.map((p) => pattern(p, scope)) }
          : {}),
      };
    default:
      return unreachable(node);
  }
}

function clause(node: ComprehensionClause, scope: GenericMethodScope): ComprehensionClause {
  switch (node.kind) {
    case "if":
      return { ...node, condition: scope.expression(node.condition) };
    case "for":
      return {
        ...node,
        iterable: scope.expression(node.iterable),
        ...(node.pattern ? { pattern: pattern(node.pattern, scope) } : {}),
      };
    default:
      return unreachable(node);
  }
}

function provider(node: ProviderContextEntry, scope: GenericMethodScope): ProviderContextEntry {
  switch (node.kind) {
    case "binding":
      return { ...node, key: scope.text(node.key), value: scope.expression(node.value) };
    case "spread":
      return { ...node, value: scope.expression(node.value) };
    default:
      return unreachable(node);
  }
}

function unreachable(node: never): never {
  throw new Error(`unknown source node: ${String(node)}`);
}
