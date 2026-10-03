import type {
  DataDecl,
  EnumDecl,
  TraitDecl,
  TypeDecl,
  ImplDecl,
  MethodDecl,
  TypeRef,
  GenericBound,
  Parameter,
  DataField,
  Decorators,
  Expression,
  Statement,
  Program,
} from "../ast.ts";
import { mapScopeExpression, mapScopeStatement } from "./generic-scope-walk.ts";
import { renameScopeType } from "./generic-scope-types.ts";

type Declaration = DataDecl | EnumDecl | TraitDecl | TypeDecl | ImplDecl;
interface BinderNames {
  next: number;
  readonly localDeclarations: boolean;
  readonly spellings: Map<string, string>;
}

/** Distinguish colliding method binders before Self expands or local types hoist. */
export function withDistinctMethodBinders(
  program: Program,
  spellings = new Map<string, string>(),
): Program {
  if (!program.localDeclarations && !program.implementations.some(collidingBinders)) return program;
  const scope = new GenericMethodScope(new Map(), {
    next: 0,
    localDeclarations: program.localDeclarations === true,
    spellings,
  });
  return {
    ...program,
    implementations: program.implementations.map((impl) => scope.implementation(impl)),
    functions: program.functions.map((fn) => {
      if (!program.localDeclarations) return fn;
      const inner = scope.shadow(fn.genericParameters);
      return { ...fn, body: inner.body(fn.body) };
    }),
    statements: scope.body(program.statements),
    tests: program.tests.map((test) => ({ ...test, body: scope.body(test.body) })),
  };
}

export class GenericMethodScope {
  private readonly names: ReadonlyMap<string, string>;
  private readonly binders: BinderNames;
  constructor(names: ReadonlyMap<string, string>, binders: BinderNames) {
    this.names = names;
    this.binders = binders;
  }

  readonly text = (text: string): string => renameScopeType(text, this.names);
  readonly type = (type: TypeRef): TypeRef => ({ ...type, name: this.text(type.name) });
  expression(node: Expression): Expression {
    return mapScopeExpression(node, this);
  }
  body(nodes: readonly Statement[]): readonly Statement[] {
    let scope = new GenericMethodScope(this.names, this.binders);
    return nodes.map((node) => {
      if (node.kind === "local-declaration" && node.declaration.kind !== "impl")
        scope = scope.shadow([node.declaration.name]);
      return mapScopeStatement(node, scope);
    });
  }

  shadow(parameters: readonly string[]): GenericMethodScope {
    const names = new Map(this.names);
    parameters.forEach((name) => names.delete(name));
    return new GenericMethodScope(names, this.binders);
  }

  private decorators(value: Decorators | undefined): Decorators | undefined {
    return value
      ? {
          ...value,
          derives: value.derives.map(this.type),
          facts: value.facts.map((value) => this.expression(value)),
        }
      : undefined;
  }

  private bounds(values: readonly GenericBound[]): readonly GenericBound[] {
    return values.map((bound) => ({
      ...bound,
      parameter: this.names.get(bound.parameter) ?? bound.parameter,
      traits: bound.traits.map(this.text),
      ...(bound.bindings
        ? {
            bindings: bound.bindings.map((binding) => ({
              ...binding,
              trait: this.text(binding.trait),
              type: this.type(binding.type),
            })),
          }
        : {}),
    }));
  }

  private defaults(
    values: Readonly<Record<string, TypeRef>> | undefined,
  ): Readonly<Record<string, TypeRef>> | undefined {
    return values
      ? Object.fromEntries(
          Object.entries(values).map(([name, type]) => [
            this.names.get(name) ?? name,
            this.type(type),
          ]),
        )
      : undefined;
  }

  private parameter(value: Parameter): Parameter {
    return {
      ...value,
      type: this.type(value.type),
      ...(value.default ? { default: this.expression(value.default) } : {}),
      ...(value.metadata
        ? { metadata: value.metadata.map((value) => this.expression(value)) }
        : {}),
    };
  }

  private field(value: DataField): DataField {
    return {
      ...value,
      type: this.type(value.type),
      ...(value.default ? { default: this.expression(value.default) } : {}),
      ...(value.metadata
        ? { metadata: value.metadata.map((value) => this.expression(value)) }
        : {}),
    };
  }

  private method(
    method: MethodDecl,
    ownerParameters: readonly string[],
    inherent: boolean,
  ): MethodDecl {
    let inner = this.shadow(method.genericParameters);
    const collisions = inherent
      ? method.genericParameters.filter((name) => ownerParameters.includes(name))
      : [];
    if (collisions.length) {
      const names = new Map(inner.names);
      for (const name of collisions) {
        const identity = `%method${this.binders.next++}.${name}`;
        this.binders.spellings.set(identity, name);
        names.set(name, identity);
      }
      inner = new GenericMethodScope(names, this.binders);
    }
    return {
      ...method,
      genericParameters: method.genericParameters.map((name) => inner.names.get(name) ?? name),
      ...(method.rowParameters
        ? { rowParameters: method.rowParameters.map((name) => inner.names.get(name) ?? name) }
        : {}),
      ...(method.genericDefaults
        ? { genericDefaults: inner.defaults(method.genericDefaults) }
        : {}),
      genericBounds: inner.bounds(method.genericBounds),
      parameters: method.parameters.map((value) => inner.parameter(value)),
      result: inner.type(method.result),
      requirements: method.requirements.map(inner.text),
      ...(method.body ? { body: inner.body(method.body) } : {}),
      ...(method.decorators ? { decorators: inner.decorators(method.decorators) } : {}),
    };
  }

  implementation(value: ImplDecl): ImplDecl {
    if (this.names.size === 0 && !this.binders.localDeclarations && !collidingBinders(value))
      return value;
    const scope = this.shadow(value.genericParameters);
    return {
      ...value,
      targetName: scope.text(value.targetName),
      ...(value.traitName !== undefined ? { traitName: scope.text(value.traitName) } : {}),
      genericBounds: scope.bounds(value.genericBounds),
      associatedTypes: value.associatedTypes.map((type) => ({
        ...type,
        ...(type.value ? { value: scope.type(type.value) } : {}),
      })),
      methods: value.methods.map((method) =>
        scope.method(method, value.genericParameters, value.traitName === undefined),
      ),
      ...(value.memberLines
        ? {
            memberLines: value.memberLines.map((line) => ({
              ...line,
              ...(line.value ? { value: scope.expression(line.value) } : {}),
            })),
          }
        : {}),
      ...(value.decorators ? { decorators: scope.decorators(value.decorators) } : {}),
    };
  }

  declaration(value: Declaration): Declaration {
    if (value.kind === "impl") return this.implementation(value);
    const scope = this.shadow([value.name, ...value.genericParameters]);
    const common = {
      ...(value.genericDefaults ? { genericDefaults: scope.defaults(value.genericDefaults) } : {}),
      ...(value.genericBounds ? { genericBounds: scope.bounds(value.genericBounds) } : {}),
      ...(value.decorators ? { decorators: scope.decorators(value.decorators) } : {}),
    };
    switch (value.kind) {
      case "data":
        return { ...value, ...common, fields: value.fields.map((field) => scope.field(field)) };
      case "enum":
        return {
          ...value,
          ...common,
          sharedFields: value.sharedFields.map((field) => scope.field(field)),
          variants: value.variants.map((variant) => ({
            ...variant,
            fields: variant.fields.map((field) => scope.field(field)),
            ...(variant.result ? { result: scope.expression(variant.result) } : {}),
            ...(variant.metadata
              ? { metadata: variant.metadata.map((value) => scope.expression(value)) }
              : {}),
          })),
        };
      case "type":
        return {
          ...value,
          ...common,
          ...(value.alias ? { alias: scope.type(value.alias) } : {}),
          ...(value.base ? { base: scope.type(value.base) } : {}),
          ...(value.row ? { row: value.row.map(scope.text) } : {}),
        };
      case "trait":
        return {
          ...value,
          ...common,
          supertraits: value.supertraits.map(scope.type),
          ...(value.supertraitBindings
            ? {
                supertraitBindings: value.supertraitBindings.map((binding) => ({
                  ...binding,
                  trait: scope.text(binding.trait),
                  type: scope.type(binding.type),
                })),
              }
            : {}),
          associatedTypes: value.associatedTypes.map((type) => ({
            ...type,
            ...(type.value ? { value: scope.type(type.value) } : {}),
          })),
          methods: value.methods.map((method) =>
            scope.method(method, value.genericParameters, false),
          ),
        };
      default:
        return unreachable(value);
    }
  }
}

/** Internal binder identities are not source spellings in diagnostics. */
export function displayMethodBinderNames(
  text: string,
  spellings: ReadonlyMap<string, string>,
): string {
  for (const [identity, spelling] of spellings) text = text.replaceAll(identity, spelling);
  return text;
}

function collidingBinders(implementation: ImplDecl): boolean {
  return (
    implementation.traitName === undefined &&
    implementation.methods.some((method) =>
      method.genericParameters.some((name) => implementation.genericParameters.includes(name)),
    )
  );
}

function unreachable(node: never): never {
  throw new Error(`unknown declaration: ${String(node)}`);
}
