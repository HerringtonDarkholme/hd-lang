import type {
  AssociatedTypeBinding,
  ComprehensionClause,
  DataDecl,
  DataField,
  Decorators,
  EnumDecl,
  Expression,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MatchArm,
  MethodDecl,
  Parameter,
  Pattern,
  Program,
  ProviderContextEntry,
  Statement,
  TraitDecl,
  TypeDecl,
  TypeRef,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";

type Renames = ReadonlyMap<string, string>;

/**
 * The start of a module scope's spelling for another module's top-level
 * name, which the module must not name (checker/module-paths.ts). No
 * identifier contains it.
 */
export const FOREIGN = "\u0000";

/** A name with its dotted segments, as `cmp.Ordering`. */
const QUALIFIED_NAME =
  /[\p{ID_Start}_][\p{ID_Continue}]*(?:\.[\p{ID_Start}_][\p{ID_Continue}]*)*/gu;

// A word after `::` names a member of the type before it, as `Error` in
// `W::Error`, so it is never a std module's top-level name. A word after a
// dot is renamed but never reported as another module's name.
function renameWords(
  text: string,
  size: number,
  rename: (word: string, member: boolean) => string,
): string {
  if (size === 0) return text;
  return text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word, offset: number) =>
    text.slice(Math.max(0, offset - 2), offset) === "::"
      ? word
      : rename(word, text[offset - 1] === "."),
  );
}

function without(names: Renames, hidden: readonly string[]): Renames {
  if (!hidden.some((name) => names.has(name))) return names;
  const result = new Map(names);
  hidden.forEach((name) => result.delete(name));
  return result;
}

export function patternBindings(pattern: Pattern): string[] {
  switch (pattern.kind) {
    case "binding":
      return [pattern.name];
    case "tuple":
      return pattern.elements.flatMap(patternBindings);
    case "data":
      return pattern.fields.flatMap((field) => patternBindings(field.pattern));
    case "result-variant":
    case "variant":
      return [
        ...pattern.bindings.flatMap((name) => (name === undefined ? [] : [name])),
        ...(pattern.payloadPatterns?.flatMap(patternBindings) ?? []),
      ];
    default:
      return [];
  }
}

/**
 * Give one parsed std module the names under which its declarations are
 * joined. Only binding references are rewritten. Source text, member and
 * variant names, fields, locals, parameters, and generic binders keep their
 * own identities.
 */
export function renameStandardBindings(program: Program, names: Renames): Program {
  return new BindingScope(names, names).program(program);
}

/** How module paths read in one module (checker/module-paths.ts). */
export interface PathScope {
  /**
   * The joined spelling of `member` of the module `module` names, or
   * undefined to keep the path as written, after reporting any error.
   * `position` is where the path stands: a value member keeps
   * `unknown-name`, a type text keeps `unknown-type`.
   */
  resolve(
    module: string,
    member: string,
    span: SourceSpan | undefined,
    position: "value" | "type",
  ): string | undefined;
  /**
   * Reports `name`, another module's top-level name that this module
   * neither declares nor imports, at `span`.
   */
  foreign?(name: string, span: SourceSpan | undefined, position: "value" | "type"): void;
}

/** A module's scope: its renames, and its module namespace names with what each names. */
export interface ModuleBindings {
  readonly names: Renames;
  readonly namespaces: Renames;
  readonly paths: PathScope;
}

/**
 * Applies each top-level item's module scope (`scopeOf`) to it: renames
 * the module's spellings to their joined ones and replaces each module path,
 * as `words.squash` or `cmp.Ordering`, that names a namespace in scope with
 * the spelling its resolver gives. Locals shadow a namespace as they shadow
 * any name. Uses stay as they are.
 */
export function resolveModuleBindings(
  program: Program,
  scopeOf: (span: SourceSpan) => ModuleBindings,
): Program {
  return BindingScope.inModules(program, scopeOf);
}

class BindingScope {
  static inModules(program: Program, scopeOf: (span: SourceSpan) => ModuleBindings): Program {
    const scope = (span: SourceSpan): BindingScope => {
      const { names, namespaces, paths } = scopeOf(span);
      // A name with no span of its own, as a trait in a bound, is reported
      // at its top-level item.
      const item: PathScope = {
        resolve: (module, member, at, position) =>
          paths.resolve(module, member, at ?? span, position),
        foreign: (name, at, position) => paths.foreign?.(name, at ?? span, position),
      };
      return new BindingScope(names, names, namespaces, item);
    };
    return {
      ...program,
      ...(program.types
        ? { types: program.types.map((item) => scope(item.span).typeDeclaration(item)) }
        : {}),
      data: program.data.map((item) => scope(item.span).data(item)),
      enums: program.enums.map((item) => scope(item.span).enum(item)),
      traits: program.traits.map((item) => scope(item.span).trait(item)),
      implementations: program.implementations.map((item) => scope(item.span).implementation(item)),
      functions: program.functions.map((item) => scope(item.span).function(item)),
      tests: program.tests.map((test) => scope(test.span).test(test)),
      // A top-level binding is a module declaration, not a local: it keeps the
      // module's renames for the statements after it.
      statements: program.statements.map((item) => scope(item.span).topLevel(item)),
    };
  }

  private readonly values: Map<string, string>;
  private readonly types: Map<string, string>;
  private readonly namespaces: Map<string, string>;
  private readonly paths: PathScope | undefined;
  constructor(values: Renames, types: Renames, namespaces: Renames = new Map(), paths?: PathScope) {
    // Every executable/type scope owns its maps: a binding expression mutates
    // the current value scope in evaluation order and must not leak out of a
    // branch, closure, comprehension, or other nested suite.
    this.values = new Map(values);
    this.types = new Map(types);
    this.namespaces = new Map(namespaces);
    this.paths = paths;
  }

  private shadowValues(hidden: readonly string[]): BindingScope {
    return new BindingScope(
      without(this.values, hidden),
      this.types,
      without(this.namespaces, hidden),
      this.paths,
    );
  }

  private shadowTypes(hidden: readonly string[]): BindingScope {
    return new BindingScope(this.values, without(this.types, hidden), this.namespaces, this.paths);
  }

  /** The joined spelling of the value name `name`, written at `span`. */
  private value(name: string, span: SourceSpan): string {
    return this.spelled(this.values, name, span, "value");
  }

  private spelled(
    names: Renames,
    name: string,
    span: SourceSpan | undefined | null,
    position: "value" | "type",
  ): string {
    const spelling = names.get(name);
    if (spelling === undefined) return name;
    if (!spelling.startsWith(FOREIGN)) return spelling;
    if (span !== null) this.paths?.foreign?.(name, span, position);
    return name;
  }

  private generics(parameters: readonly string[], rows: readonly string[] = []): BindingScope {
    return this.shadowTypes([...parameters, ...rows]);
  }

  /**
   * A type text with its names renamed. A dotted name whose first segment
   * is a module namespace in scope, as `cmp.Ordering`, is a module path:
   * its first two segments become the spelling the path resolves to.
   */
  private text(text: string, span?: SourceSpan): string {
    // A later segment of a dotted name, as a variant, is never another
    // module's top-level name: `null` renames it without reporting.
    const word = (name: string, member = false): string =>
      this.spelled(this.types, name, member ? null : span, "type");
    if (this.namespaces.size === 0 || !text.includes("."))
      return renameWords(text, this.types.size, word);
    return text.replace(QUALIFIED_NAME, (path) => {
      const [first, member, ...rest] = path.split(".");
      const module = member === undefined ? undefined : this.namespaces.get(first!);
      const resolved =
        module === undefined ? undefined : this.paths?.resolve(module, member!, span, "type");
      if (resolved !== undefined) return [resolved, ...rest].join(".");
      return path
        .split(".")
        .map((name, index) => word(name, index > 0))
        .join(".");
    });
  }

  private type(value: TypeRef): TypeRef {
    return { ...value, name: this.text(value.name, value.span) };
  }

  private test(value: Program["tests"][number]): Program["tests"][number] {
    return {
      ...value,
      ...(value.result ? { result: this.type(value.result) } : {}),
      body: this.statements(value.body),
    };
  }

  private topLevel(value: Statement): Statement {
    const renamed = this.statement(value);
    return renamed.kind === "binding"
      ? { ...renamed, name: this.values.get(renamed.name) ?? renamed.name }
      : renamed;
  }

  private binding(value: AssociatedTypeBinding): AssociatedTypeBinding {
    return { ...value, trait: this.text(value.trait), type: this.type(value.type) };
  }

  private bounds(values: readonly GenericBound[]): readonly GenericBound[] {
    return values.map((bound) => ({
      ...bound,
      traits: bound.traits.map((trait) => this.text(trait)),
      ...(bound.bindings ? { bindings: bound.bindings.map((value) => this.binding(value)) } : {}),
    }));
  }

  private defaults(
    values: Readonly<Record<string, TypeRef>> | undefined,
  ): Readonly<Record<string, TypeRef>> | undefined {
    return values
      ? Object.fromEntries(Object.entries(values).map(([name, value]) => [name, this.type(value)]))
      : undefined;
  }

  private decorators(value: Decorators | undefined): Decorators | undefined {
    return value
      ? {
          ...value,
          derives: value.derives.map((type) => this.type(type)),
          facts: value.facts.map((fact) => this.expression(fact)),
        }
      : undefined;
  }

  private parameter(value: Parameter): Parameter {
    return {
      ...value,
      type: this.type(value.type),
      ...(value.default ? { default: this.expression(value.default) } : {}),
      ...(value.metadata
        ? { metadata: value.metadata.map((metadata) => this.expression(metadata)) }
        : {}),
    };
  }

  private field(value: DataField): DataField {
    return {
      ...value,
      type: this.type(value.type),
      ...(value.default ? { default: this.expression(value.default) } : {}),
      ...(value.metadata
        ? { metadata: value.metadata.map((metadata) => this.expression(metadata)) }
        : {}),
    };
  }

  private method(value: MethodDecl): MethodDecl {
    const generic = this.generics(value.genericParameters, value.rowParameters);
    const body = generic.shadowValues(value.parameters.map((parameter) => parameter.name));
    return {
      ...value,
      genericBounds: generic.bounds(value.genericBounds),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      parameters: generic.parameters(value.parameters),
      result: generic.type(value.result),
      requirements: value.requirements.map((requirement) => generic.text(requirement)),
      ...(value.body ? { body: body.statements(value.body) } : {}),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private parameters(values: readonly Parameter[]): readonly Parameter[] {
    let scope = new BindingScope(this.values, this.types, this.namespaces, this.paths);
    return values.map((value) => {
      const renamed = scope.parameter(value);
      scope = scope.shadowValues([value.name]);
      return renamed;
    });
  }

  private function(value: FunctionDecl): FunctionDecl {
    const generic = this.generics(value.genericParameters, value.rowParameters);
    const body = generic.shadowValues(value.parameters.map((parameter) => parameter.name));
    return {
      ...value,
      name: this.values.get(value.name) ?? value.name,
      genericBounds: generic.bounds(value.genericBounds),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      parameters: generic.parameters(value.parameters),
      result: generic.type(value.result),
      requirements: value.requirements.map((requirement) => generic.text(requirement)),
      ...(value.writtenRequirements
        ? { writtenRequirements: value.writtenRequirements.map((item) => generic.text(item)) }
        : {}),
      body: body.statements(value.body),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private data(value: DataDecl): DataDecl {
    const generic = this.generics(value.genericParameters);
    return {
      ...value,
      name: this.types.get(value.name) ?? value.name,
      ...(value.genericBounds ? { genericBounds: generic.bounds(value.genericBounds) } : {}),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      fields: value.fields.map((field) => generic.field(field)),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private enum(value: EnumDecl): EnumDecl {
    const generic = this.generics(value.genericParameters);
    return {
      ...value,
      name: this.types.get(value.name) ?? value.name,
      ...(value.genericBounds ? { genericBounds: generic.bounds(value.genericBounds) } : {}),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      sharedFields: value.sharedFields.map((field) => generic.field(field)),
      variants: value.variants.map((variant) => {
        const local = generic.generics(variant.genericParameters ?? []);
        return {
          ...variant,
          ...(variant.genericBounds ? { genericBounds: local.bounds(variant.genericBounds) } : {}),
          fields: variant.fields.map((field) => local.field(field)),
          ...(variant.resultType ? { resultType: local.type(variant.resultType) } : {}),
          ...(variant.result
            ? { result: local.expression(variant.result) as typeof variant.result }
            : {}),
          ...(variant.metadata
            ? { metadata: variant.metadata.map((metadata) => generic.expression(metadata)) }
            : {}),
        };
      }),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private trait(value: TraitDecl): TraitDecl {
    const generic = this.generics(value.genericParameters);
    return {
      ...value,
      name: this.types.get(value.name) ?? value.name,
      ...(value.genericBounds ? { genericBounds: generic.bounds(value.genericBounds) } : {}),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      supertraits: value.supertraits.map((type) => generic.type(type)),
      ...(value.supertraitBindings
        ? { supertraitBindings: value.supertraitBindings.map((item) => generic.binding(item)) }
        : {}),
      associatedTypes: value.associatedTypes.map((associated) => ({
        ...associated,
        ...(associated.value ? { value: generic.type(associated.value) } : {}),
      })),
      methods: value.methods.map((method) => generic.method(method)),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private typeDeclaration(value: TypeDecl): TypeDecl {
    const generic = this.generics(value.genericParameters, value.rowParameters);
    return {
      ...value,
      name: this.types.get(value.name) ?? value.name,
      ...(value.genericBounds ? { genericBounds: generic.bounds(value.genericBounds) } : {}),
      ...(value.genericDefaults
        ? { genericDefaults: generic.defaults(value.genericDefaults) }
        : {}),
      ...(value.alias ? { alias: generic.type(value.alias) } : {}),
      ...(value.base ? { base: generic.type(value.base) } : {}),
      ...(value.row ? { row: value.row.map((item) => generic.text(item)) } : {}),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  private implementation(value: ImplDecl): ImplDecl {
    const generic = this.generics(value.genericParameters, value.rowParameters);
    return {
      ...value,
      genericBounds: generic.bounds(value.genericBounds),
      ...(value.traitName ? { traitName: generic.text(value.traitName, value.span) } : {}),
      targetName: generic.text(value.targetName, value.span),
      associatedTypes: value.associatedTypes.map((associated) => ({
        ...associated,
        ...(associated.value ? { value: generic.type(associated.value) } : {}),
      })),
      methods: value.methods.map((method) => generic.method(method)),
      ...(value.memberLines
        ? {
            memberLines: value.memberLines.map((line) => ({
              ...line,
              ...(line.value ? { value: generic.expression(line.value) } : {}),
            })),
          }
        : {}),
      ...(value.decorators ? { decorators: generic.decorators(value.decorators) } : {}),
    };
  }

  program(value: Program): Program {
    return {
      ...value,
      uses: [],
      ...(value.types ? { types: value.types.map((item) => this.typeDeclaration(item)) } : {}),
      data: value.data.map((item) => this.data(item)),
      enums: value.enums.map((item) => this.enum(item)),
      traits: value.traits.map((item) => this.trait(item)),
      implementations: value.implementations.map((item) => this.implementation(item)),
      functions: value.functions.map((item) => this.function(item)),
      tests: value.tests.map((test) => this.test(test)),
      statements: this.statements(value.statements),
    };
  }

  private pattern(value: Pattern): Pattern {
    switch (value.kind) {
      case "tuple":
        return { ...value, elements: value.elements.map((item) => this.pattern(item)) };
      case "data":
        return {
          ...value,
          typeName: this.text(value.typeName, value.span),
          fields: value.fields.map((field) => ({
            ...field,
            pattern: this.pattern(field.pattern),
          })),
        };
      case "variant":
        return {
          ...value,
          ...(value.enumName ? { enumName: this.text(value.enumName, value.span) } : {}),
          ...(value.payloadPatterns
            ? { payloadPatterns: value.payloadPatterns.map((item) => this.pattern(item)) }
            : {}),
        };
      case "result-variant":
        return {
          ...value,
          ...(value.payloadPatterns
            ? { payloadPatterns: value.payloadPatterns.map((item) => this.pattern(item)) }
            : {}),
        };
      default:
        return value;
    }
  }

  private provider(value: ProviderContextEntry): ProviderContextEntry {
    return value.kind === "binding"
      ? { ...value, key: this.text(value.key), value: this.expression(value.value) }
      : { ...value, value: this.expression(value.value) };
  }

  private clauses(values: readonly ComprehensionClause[]): {
    readonly clauses: readonly ComprehensionClause[];
    readonly scope: BindingScope;
  } {
    let scope = new BindingScope(this.values, this.types, this.namespaces, this.paths);
    const clauses = values.map((value) => {
      if (value.kind === "if") return { ...value, condition: scope.expression(value.condition) };
      const clause = {
        ...value,
        iterable: scope.expression(value.iterable),
        ...(value.pattern ? { pattern: scope.pattern(value.pattern) } : {}),
      };
      const bindings = value.pattern
        ? patternBindings(value.pattern)
        : value.bindings.map((binding) => binding.name);
      scope = scope.shadowValues(bindings);
      return clause;
    });
    return { clauses, scope };
  }

  private arm(value: MatchArm): MatchArm {
    const pattern = this.pattern(value.pattern);
    const scope = this.shadowValues(patternBindings(value.pattern));
    return {
      ...value,
      pattern,
      ...(value.guard ? { guard: scope.expression(value.guard) } : {}),
      body: scope.statements(value.body),
    };
  }

  private expression(value: Expression): Expression {
    const e = (item: Expression): Expression => this.expression(item);
    const typeArguments = (item: { readonly typeArguments?: readonly TypeRef[] }) =>
      item.typeArguments
        ? { typeArguments: item.typeArguments.map((type) => this.type(type)) }
        : {};
    switch (value.kind) {
      case "integer":
      case "float":
      case "string":
      case "character":
      case "boolean":
      case "contextual-variant":
        return value;
      case "name":
        return {
          ...value,
          name: this.value(value.name, value.span),
          ...typeArguments(value),
        };
      case "qualified-name":
        return {
          ...value,
          owner: this.text(value.owner, value.span),
          ...(value.genericTypeOwner
            ? { genericTypeOwner: this.text(value.genericTypeOwner, value.span) }
            : {}),
          ...(value.ownerTypeArguments
            ? { ownerTypeArguments: value.ownerTypeArguments.map((type) => this.type(type)) }
            : {}),
          ...typeArguments(value),
        };
      case "interpolated-string":
        return {
          ...value,
          segments: value.segments.map((segment) =>
            segment.kind === "expression"
              ? { ...segment, expression: e(segment.expression) }
              : segment,
          ),
        };
      case "binding-expression": {
        const renamed = e(value.value);
        value.bindings.forEach((binding) => {
          this.values.delete(binding.name);
          this.namespaces.delete(binding.name);
        });
        return { ...value, value: renamed };
      }
      case "list":
      case "tuple":
        return { ...value, elements: value.elements.map(e) };
      case "list-comprehension": {
        const { clauses, scope } = this.clauses(value.clauses);
        return { ...value, clauses, value: scope.expression(value.value) };
      }
      case "map-comprehension": {
        const { clauses, scope } = this.clauses(value.clauses);
        return {
          ...value,
          clauses,
          key: scope.expression(value.key),
          value: scope.expression(value.value),
        };
      }
      case "map":
        return {
          ...value,
          entries: value.entries.map((entry) => ({
            ...entry,
            key: e(entry.key),
            value: e(entry.value),
          })),
        };
      case "unary":
      case "propagate":
        return { ...value, operand: e(value.operand) };
      case "binary":
        return { ...value, left: e(value.left), right: e(value.right) };
      case "call":
      case "suspend-call":
        return {
          ...value,
          callee: e(value.callee),
          arguments: value.arguments.map(e),
          ...typeArguments(value),
        };
      case "data":
        return {
          ...value,
          name: this.text(value.name, value.span),
          ...typeArguments(value),
          ...(value.spread ? { spread: e(value.spread) } : {}),
          fields: value.fields.map((field) => ({ ...field, value: e(field.value) })),
        };
      case "member": {
        // `words.squash` selects a declaration through a module namespace
        // (05-expressions.md#r-expr.name.qualified).
        const module =
          value.receiver.kind === "name" ? this.namespaces.get(value.receiver.name) : undefined;
        const resolved =
          module === undefined
            ? undefined
            : this.paths?.resolve(module, value.name, value.span, "value");
        if (resolved !== undefined)
          return { kind: "name", name: resolved, ...typeArguments(value), span: value.span };
        return { ...value, receiver: e(value.receiver), ...typeArguments(value) };
      }
      case "index":
        return { ...value, receiver: e(value.receiver), index: e(value.index) };
      case "range":
        return {
          ...value,
          ...(value.start ? { start: e(value.start) } : {}),
          ...(value.end ? { end: e(value.end) } : {}),
        };
      case "pipe":
        return { ...value, value: e(value.value), step: e(value.step) };
      case "closure": {
        const scope = this.shadowValues(value.parameters.map((parameter) => parameter.name));
        return {
          ...value,
          parameters: value.parameters.map((parameter) => ({
            ...parameter,
            ...(parameter.type ? { type: this.type(parameter.type) } : {}),
          })),
          ...(value.result ? { result: this.type(value.result) } : {}),
          ...(value.requirements
            ? { requirements: value.requirements.map((item) => this.text(item)) }
            : {}),
          body: scope.statements(value.body),
        };
      }
      case "provider-use":
        return { ...value, key: this.text(value.key) };
      case "provider-context":
        return { ...value, entries: value.entries.map((entry) => this.provider(entry)) };
      case "provider-with":
        return {
          ...value,
          entries: value.entries.map((entry) => this.provider(entry)),
          body: this.statements(value.body),
        };
      case "if":
        return {
          ...value,
          condition: e(value.condition),
          thenBody: this.statements(value.thenBody),
          elseBody: this.statements(value.elseBody),
        };
      case "while":
        return {
          ...value,
          condition: e(value.condition),
          body: this.statements(value.body),
          elseBody: this.statements(value.elseBody),
        };
      case "for": {
        const pattern = value.pattern ? this.pattern(value.pattern) : undefined;
        const bindings = value.pattern
          ? patternBindings(value.pattern)
          : value.bindings.map((binding) => binding.name);
        return {
          ...value,
          ...(pattern ? { pattern } : {}),
          iterable: e(value.iterable),
          body: this.shadowValues(bindings).statements(value.body),
          elseBody: this.statements(value.elseBody),
        };
      }
      case "match":
        return {
          ...value,
          subject: e(value.subject),
          arms: value.arms.map((arm) => this.arm(arm)),
        };
      default:
        return unreachable(value);
    }
  }

  private typeArguments(value: { readonly typeArguments?: readonly TypeRef[] }): {
    readonly typeArguments?: readonly TypeRef[];
  } {
    return value.typeArguments
      ? { typeArguments: value.typeArguments.map((type) => this.type(type)) }
      : {};
  }

  private statement(value: Statement): Statement {
    const e = (item: Expression): Expression => this.expression(item);
    switch (value.kind) {
      case "pass":
      case "continue":
      case "local-implementation":
        return value;
      case "defer":
        return { ...value, body: this.statements(value.body) };
      case "binding":
      case "tuple-binding":
        return {
          ...value,
          ...(value.annotation ? { annotation: this.type(value.annotation) } : {}),
          value: e(value.value),
        };
      case "pattern-binding":
        return {
          ...value,
          pattern: this.pattern(value.pattern),
          ...(value.annotation ? { annotation: this.type(value.annotation) } : {}),
          value: e(value.value),
          ...(value.elseBody ? { elseBody: this.statements(value.elseBody) } : {}),
        };
      case "assignment":
        return {
          ...value,
          name: this.value(value.name, value.span),
          value: e(value.value),
        };
      case "discard":
        return { ...value, value: e(value.value) };
      case "return":
      case "break":
        return { ...value, ...(value.value ? { value: e(value.value) } : {}) };
      case "field-assignment":
        return {
          ...value,
          target: {
            ...value.target,
            receiver: e(value.target.receiver),
            ...this.typeArguments(value.target),
          },
          value: e(value.value),
        };
      case "index-assignment":
        return {
          ...value,
          target: {
            ...value.target,
            receiver: e(value.target.receiver),
            index: e(value.target.index),
          },
          value: e(value.value),
        };
      case "call-assignment":
        return {
          ...value,
          target: {
            ...value.target,
            callee: e(value.target.callee),
            arguments: value.target.arguments.map(e),
            ...this.typeArguments(value.target),
          },
          value: e(value.value),
        };
      case "expression":
        return { ...value, expression: e(value.expression) };
      case "local-declaration":
        return { ...value, declaration: this.localDeclaration(value.declaration) };
      default:
        return unreachable(value);
    }
  }

  private localDeclaration(
    value: DataDecl | EnumDecl | TraitDecl | TypeDecl | ImplDecl,
  ): DataDecl | EnumDecl | TraitDecl | TypeDecl | ImplDecl {
    if (value.kind === "impl") return this.implementation(value);
    const local = this.shadowTypes([value.name]);
    if (value.kind === "data") return local.data(value);
    if (value.kind === "enum") return local.enum(value);
    if (value.kind === "trait") return local.trait(value);
    if (value.kind === "type") return local.typeDeclaration(value);
    return unreachable(value);
  }

  private statements(values: readonly Statement[]): readonly Statement[] {
    let scope = new BindingScope(this.values, this.types, this.namespaces, this.paths);
    return values.map((value) => {
      const renamed = scope.statement(value);
      if (value.kind === "binding") scope = scope.shadowValues([value.name]);
      else if (value.kind === "tuple-binding")
        scope = scope.shadowValues(value.bindings.map((binding) => binding.name));
      else if (value.kind === "pattern-binding")
        scope = scope.shadowValues(patternBindings(value.pattern));
      else if (value.kind === "local-declaration" && value.declaration.kind !== "impl")
        scope = scope.shadowTypes([value.declaration.name]);
      return renamed;
    });
  }
}

function unreachable(value: never): never {
  throw new Error(`unknown source node: ${String(value)}`);
}

/**
 * Carry an early-declared std module's own uses into a program that declares
 * the module early (checker/typed-derivation.ts, checker/standard-traits.ts).
 * Each used name resolves against the declaration the library join already
 * gave its standard name. A synthesized `use` stays out: the join renamed
 * the declaration to its hidden name before the use could exist, so a late
 * local import desynchronizes the template names. `skippedModules` are uses
 * the caller binds itself (the derivation binds `std.inspect` explicitly).
 * The caller's own renames win over the carried ones.
 */
export function carriedLibraryUses(
  joined: Program,
  library: Program,
  skippedModules: readonly string[] = [],
): Map<string, string> {
  const declarations = [
    ...joined.data,
    ...joined.enums,
    ...joined.traits,
    ...(joined.types ?? []),
    ...joined.functions,
  ];
  const renames = new Map<string, string>();
  for (const use of library.uses) {
    if (skippedModules.includes(use.module)) continue;
    for (const imported of use.names) {
      const local = imported.alias ?? imported.name;
      const declaration = declarations.find(
        (item) => item.standardName === `${use.module}.${imported.name}`,
      );
      if (declaration) renames.set(local, declaration.name);
    }
  }
  return renames;
}
