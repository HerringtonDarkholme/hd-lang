import type {
  ComprehensionClause,
  DataDecl,
  EnumDecl,
  Expression,
  FunctionDecl,
  ImplDecl,
  MethodDecl,
  Pattern,
  Program,
  Statement,
  TraitDecl,
  TypeDecl,
} from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { patternBindings } from "./standard-bindings.ts";

type Declaration = DataDecl | EnumDecl | TraitDecl | TypeDecl | ImplDecl;
type Scope = ReadonlySet<string>;

/**
 * No declaration within a type parameter's scope may reuse its name: no
 * nested type parameter, local declaration, local value, or parameter
 * (03-names-and-scopes.md#r-names.type-param.no-redeclare). A std
 * declaration is checked when std is written, not in every program.
 */
export function typeParameterRedeclarations(program: Program): readonly Diagnostic[] {
  const walk = new TypeParameterWalk();
  const none: Scope = new Set();
  for (const item of program.functions) if (!item.standard) walk.callable(item, none);
  for (const item of program.implementations) if (!item.standard) walk.declaration(item, none);
  for (const item of program.traits) if (!item.standard) walk.declaration(item, none);
  for (const item of [...program.data, ...program.enums, ...(program.types ?? [])])
    if (!item.standard) walk.declaration(item, none);
  for (const test of program.tests) walk.statements(test.body, none);
  walk.statements(program.statements, none);
  return walk.diagnostics;
}

class TypeParameterWalk {
  readonly diagnostics: Diagnostic[] = [];

  private name(name: string, scope: Scope, span: SourceSpan): void {
    if (!scope.has(name)) return;
    this.diagnostics.push({
      code: "duplicate-binding",
      severity: "error",
      message: `'${name}' is already a type parameter in this scope`,
      span,
    });
  }

  /** The scope inside a declaration with type parameters `own`, each checked against `scope`. */
  private inner(own: readonly string[], scope: Scope, span: SourceSpan): Scope {
    if (own.length === 0) return scope;
    own.forEach((name) => this.name(name, scope, span));
    return new Set([...scope, ...own]);
  }

  callable(item: FunctionDecl | MethodDecl, scope: Scope): void {
    const inner = this.inner(item.genericParameters, scope, item.span);
    for (const parameter of item.parameters) this.name(parameter.name, inner, parameter.span);
    if (item.body) this.statements(item.body, inner);
  }

  declaration(item: Declaration, scope: Scope): void {
    const inner = this.inner(item.genericParameters, scope, item.span);
    if (item.kind === "impl" || item.kind === "trait")
      for (const method of item.methods) this.callable(method, inner);
  }

  statements(items: readonly Statement[], scope: Scope): void {
    for (const item of items) this.statement(item, scope);
  }

  private pattern(pattern: Pattern, scope: Scope): void {
    if (scope.size === 0) return;
    for (const name of new Set(patternBindings(pattern))) this.name(name, scope, pattern.span);
  }

  private statement(item: Statement, scope: Scope): void {
    switch (item.kind) {
      case "binding":
        this.name(item.name, scope, item.span);
        return this.expression(item.value, scope);
      case "tuple-binding":
        item.bindings.forEach((binding) => this.name(binding.name, scope, binding.span));
        return this.expression(item.value, scope);
      case "pattern-binding":
        this.pattern(item.pattern, scope);
        this.expression(item.value, scope);
        return this.statements(item.elseBody ?? [], scope);
      case "local-declaration":
        if (item.declaration.kind !== "impl")
          this.name(item.declaration.name, scope, item.declaration.span);
        return this.declaration(item.declaration, scope);
      case "defer":
        return this.statements(item.body, scope);
      case "assignment":
      case "discard":
        return this.expression(item.value, scope);
      case "return":
      case "break":
        if (item.value) this.expression(item.value, scope);
        return;
      case "field-assignment":
      case "index-assignment":
      case "call-assignment":
        this.expression(item.target, scope);
        return this.expression(item.value, scope);
      case "expression":
        return this.expression(item.expression, scope);
      case "pass":
      case "continue":
      case "local-implementation":
        return;
      default:
        return unreachable(item);
    }
  }

  private clauses(items: readonly ComprehensionClause[], scope: Scope): void {
    for (const clause of items) {
      if (clause.kind === "if") {
        this.expression(clause.condition, scope);
        continue;
      }
      this.expression(clause.iterable, scope);
      if (clause.pattern) this.pattern(clause.pattern, scope);
      clause.bindings.forEach((binding) => this.name(binding.name, scope, binding.span));
    }
  }

  private expression(item: Expression, scope: Scope): void {
    const e = (value: Expression): void => this.expression(value, scope);
    switch (item.kind) {
      case "integer":
      case "float":
      case "string":
      case "character":
      case "boolean":
      case "contextual-variant":
      case "name":
      case "qualified-name":
      case "provider-use":
        return;
      case "interpolated-string":
        for (const segment of item.segments)
          if (segment.kind === "expression") e(segment.expression);
        return;
      case "binding-expression":
        item.bindings.forEach((binding) => this.name(binding.name, scope, binding.span));
        return e(item.value);
      case "list":
      case "tuple":
        return item.elements.forEach(e);
      case "list-comprehension":
        this.clauses(item.clauses, scope);
        return e(item.value);
      case "map-comprehension":
        this.clauses(item.clauses, scope);
        e(item.key);
        return e(item.value);
      case "map":
        return item.entries.forEach((entry) => (e(entry.key), e(entry.value)));
      case "unary":
      case "propagate":
        return e(item.operand);
      case "binary":
        e(item.left);
        return e(item.right);
      case "call":
      case "suspend-call":
        e(item.callee);
        return item.arguments.forEach(e);
      case "data":
        if (item.spread) e(item.spread);
        return item.fields.forEach((field) => e(field.value));
      case "member":
        return e(item.receiver);
      case "index":
        e(item.receiver);
        return e(item.index);
      case "range":
        if (item.start) e(item.start);
        if (item.end) e(item.end);
        return;
      case "pipe":
        e(item.value);
        return e(item.step);
      case "closure":
        item.parameters.forEach((parameter) => this.name(parameter.name, scope, parameter.span));
        return this.statements(item.body, scope);
      case "provider-context":
        return item.entries.forEach((entry) => e(entry.value));
      case "provider-with":
        item.entries.forEach((entry) => e(entry.value));
        return this.statements(item.body, scope);
      case "if":
        e(item.condition);
        this.statements(item.thenBody, scope);
        return this.statements(item.elseBody, scope);
      case "while":
        e(item.condition);
        this.statements(item.body, scope);
        return this.statements(item.elseBody, scope);
      case "for":
        e(item.iterable);
        if (item.pattern) this.pattern(item.pattern, scope);
        item.bindings.forEach((binding) => this.name(binding.name, scope, binding.span));
        this.statements(item.body, scope);
        return this.statements(item.elseBody, scope);
      case "match":
        e(item.subject);
        for (const arm of item.arms) {
          this.pattern(arm.pattern, scope);
          if (arm.guard) e(arm.guard);
          this.statements(arm.body, scope);
        }
        return;
      default:
        return unreachable(item);
    }
  }
}

function unreachable(value: never): never {
  throw new Error(`unknown source node: ${String(value)}`);
}
