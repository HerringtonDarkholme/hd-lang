import type {
  Expression,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MethodDecl,
  Statement,
  TraitDecl,
  TypeDecl,
  TypeRef,
} from "../ast.ts";
import { preserveSourceOrigin, type SourceSpan } from "../diagnostics.ts";
import { nominalGenericParts, readonlyType, displayType } from "../types.ts";
import { derivedBaseSpan, derivedImplementationSpan } from "./derive-intrinsics.ts";
import { NEWTYPE_FIELD } from "./type-declarations.ts";

// A typed-derivation template checked once, and the implementations that
// instantiate it for one target (spec/lang/14-annotations.md#templates), with
// the AST helpers they share with checker/typed-derivation.ts.

export const STRUCTURE = "Structure";

/** A tuple instance's parameter for the item type of its rest element `List[hd_R]...`. */
export const TUPLE_REST = "hd_R";

// ---------------------------------------------------------------------------
// Generic AST helpers.

const TYPE_KEYS = new Set(["type", "result", "annotation", "alias", "base"]);
const TYPE_LIST_KEYS = new Set(["typeArguments", "ownerTypeArguments", "supertraits"]);

export function isTypeRef(value: unknown): value is TypeRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    typeof (value as { name?: unknown }).name === "string" &&
    keys.includes("span") &&
    keys.every((key) => key === "name" || key === "span" || key === "written")
  );
}

export function renameWords(text: string, renames: ReadonlyMap<string, string>): string {
  if (renames.size === 0) return text;
  return text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word) => renames.get(word) ?? word);
}

/** Renames generic parameters in every type position of `node`. */
function renameTypes<T>(node: T, renames: ReadonlyMap<string, string>): T {
  if (Array.isArray(node)) return node.map((item) => renameTypes(item, renames)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [entry, value] of Object.entries(node as Record<string, unknown>)) {
    if (TYPE_KEYS.has(entry) && isTypeRef(value))
      result[entry] = { ...value, name: renameWords(value.name, renames) };
    else if (entry === "value" && isTypeRef(value) && "name" in (node as object))
      result[entry] = { ...value, name: renameWords(value.name, renames) };
    else if (TYPE_LIST_KEYS.has(entry) && Array.isArray(value))
      result[entry] = value.map((item) =>
        isTypeRef(item)
          ? { ...item, name: renameWords(item.name, renames) }
          : renameTypes(item, renames),
      );
    else if (entry === "traits" && Array.isArray(value))
      result[entry] = value.map((item) =>
        typeof item === "string" ? renameWords(item, renames) : item,
      );
    else result[entry] = renameTypes(value, renames);
  }
  return preserveSourceOrigin(node, result) as T;
}

/** Visits every object in `node`, depth first. */
export function visit(node: unknown, callback: (value: Record<string, unknown>) => void): void {
  if (Array.isArray(node)) {
    for (const item of node) visit(item, callback);
    return;
  }
  if (!node || typeof node !== "object") return;
  callback(node as Record<string, unknown>);
  for (const value of Object.values(node as Record<string, unknown>)) visit(value, callback);
}

/** Maps every object bottom-up; `callback` may replace it. */
export function transform(
  node: unknown,
  callback: (value: Record<string, unknown>) => unknown,
): unknown {
  if (Array.isArray(node)) return node.map((item) => transform(item, callback));
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [entry, value] of Object.entries(node as Record<string, unknown>))
    result[entry] = transform(value, callback);
  return callback(preserveSourceOrigin(node, result));
}

export function headName(type: string): string {
  return readonlyType(type).split("[")[0]!;
}

/** The declared type of local `name` in a method body. */
function localType(statements: readonly Statement[], name: string): string | undefined {
  let type: string | undefined;
  visit(statements, (value) => {
    if (value.kind !== "binding" || value.name !== name || type !== undefined) return;
    if (isTypeRef(value.annotation)) type = value.annotation.name;
    else {
      // `let mut w = Encoder { ... }` states its type through the literal.
      const literal = value.value as { kind?: unknown; name?: unknown; typeArguments?: unknown };
      if (value.mutableAccess === true && literal.kind === "data" && !literal.typeArguments)
        type = `mut:${String(literal.name)}`;
    }
  });
  return type;
}

/**
 * The strengthened bound of `member[F]` in a std template's `Source`
 * implementation (14-annotations.md#r-annot.walker.strengthen-member), read
 * from lib/std, as `Arbitrary & Inspectable` for derived `Arbitrary`.
 */
export function sourceMemberBound(support: readonly ImplDecl[], source: string): readonly string[] {
  for (const implementation of support) {
    if (headName(implementation.traitName ?? "") !== source) continue;
    const member = implementation.methods.find((method) => method.name === "member");
    const parameter = member?.genericParameters[0];
    const bound = member?.genericBounds.find((item) => item.parameter === parameter);
    if (bound) return bound.traits;
  }
  return [];
}

/** The `Error` type an implementation of `protocol` for `type` declares. */
function protocolError(
  implementations: readonly ImplDecl[],
  protocol: string,
  type: string,
): string | undefined {
  const actual = readonlyType(type);
  const actualParts = nominalGenericParts(actual);
  for (const implementation of implementations) {
    if (!implementation.traitName || headName(implementation.traitName) !== protocol) continue;
    if (headName(implementation.targetName) !== headName(actual)) continue;
    const error = implementation.associatedTypes.find((item) => item.name === "Error")?.value;
    if (!error) continue;
    const renames = new Map<string, string>();
    const declared = nominalGenericParts(implementation.targetName);
    declared?.arguments.forEach((argument, index) => {
      const value = actualParts?.arguments[index];
      if (implementation.genericParameters.includes(argument) && value !== undefined)
        renames.set(argument, value);
    });
    return renameWords(error.name, renames);
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// A template checked once.

/** One call of `walk`, `describe`, or `build` in a template. */
export interface TemplateSite {
  readonly traversal: "walk" | "describe" | "build";
  /** The walker, describer, or source type, written with the template's parameter. */
  readonly visitor: string;
  readonly errorType: string;
  /**
   * The template method's own type parameters that the visitor or its error
   * type mentions, as `W` in `SerializeWalker[T, W]`, with their bounds. The
   * traversal is generic over them.
   */
  readonly methodParameters: readonly string[];
  readonly methodBounds: readonly GenericBound[];
}

/** A traversal's generic parameter list and the type arguments that name them. */
export interface TraversalGenerics {
  readonly generics: string;
  readonly typeArgs: string;
}

/** `T < A & B`, or a bare `T` when `traits` is empty. */
export function withBound(parameter: string, traits: readonly string[]): string {
  return traits.length > 0 ? `${parameter} < ${traits.join(" & ")}` : parameter;
}

/**
 * The generics of one site's traversal: the target's parameters, each as
 * `targetBounds` writes it, then the template method's parameters that the
 * site's visitor mentions, as `W < Serializer`.
 */
export function traversalGenerics(
  targetBounds: readonly string[],
  targetParameters: readonly string[],
  site: TemplateSite,
): TraversalGenerics {
  const own = site.methodParameters.map((parameter) =>
    withBound(
      parameter,
      site.methodBounds
        .filter((bound) => bound.parameter === parameter)
        .flatMap((bound) => bound.traits),
    ),
  );
  const all = [...targetBounds, ...own];
  const names = [...targetParameters, ...site.methodParameters];
  return {
    generics: all.length > 0 ? `[${all.join(", ")}]` : "",
    typeArgs: names.length > 0 ? `::[${names.join(", ")}]` : "",
  };
}

/**
 * A template checked once (annot.template.checked): each method becomes a
 * generic function over the template's parameter, bounded by a trait that
 * stands for the parameter's `Structure` in this template. A derivation
 * implements that trait for its target and forwards each trait method to
 * the function.
 */
export interface CompiledTemplate {
  readonly template: ImplDecl;
  readonly parameter: string;
  /** The hidden trait of this template's `Structure`. */
  readonly structureTrait: string;
  readonly sites: readonly TemplateSite[];
  /** The function of each template method, by method name. */
  readonly functions: ReadonlyMap<string, string>;
  readonly declarations: { readonly trait: TraitDecl; readonly functions: FunctionDecl[] };
  /** A tuple template (annot.template.tuple.form): its `build` returns `Self`, since a tuple takes no `mut`. */
  readonly tuple: boolean;
}

/** The receiver of a template method, as a parameter of its function. */
const TEMPLATE_SELF = "hd_self";

export function compileTemplate(
  template: ImplDecl,
  implementations: readonly ImplDecl[],
  renames: ReadonlyMap<string, string>,
  error: (code: string, message: string, span: SourceSpan) => void,
  tuple = false,
): CompiledTemplate {
  const parameter = template.genericParameters[0]!;
  const traitHead = headName(template.traitName!);
  const key = `${tuple ? "tuple_" : ""}${traitHead.replace(/[^\p{ID_Continue}]/gu, "_")}`;
  const structureTrait = `hd__Structure_${key}`;
  const sites: TemplateSite[] = [];
  const functionNames = new Map<string, string>();
  const functions: FunctionDecl[] = [];
  // A call on the receiver or qualified by the derived trait needs the
  // trait's own implementation for the parameter (annot.template.qualified-self).
  let needsTrait = false;
  for (const templateMethod of template.methods) {
    const original = templateMethod.body ?? [];
    const body = transform(original, (value) => {
      if (value.kind === "name" && value.name === "self") return { ...value, name: TEMPLATE_SELF };
      if (value.kind !== "call") return value;
      const callee = value.callee as Expression;
      if (
        callee.kind === "member" &&
        callee.receiver.kind === "name" &&
        callee.receiver.name === TEMPLATE_SELF
      )
        needsTrait = true;
      if (callee.kind !== "qualified-name") return value;
      const args = value.arguments as Expression[];
      if (callee.owner === traitHead) {
        needsTrait = true;
        return { ...value, callee: { ...callee, owner: parameter } };
      }
      // `Structure::f(args)` is `T::f(args)`; `Structure::walk` takes its value.
      const owner = callee.owner === STRUCTURE && callee.name !== "walk" ? parameter : callee.owner;
      const traversal =
        owner === STRUCTURE && callee.name === "walk"
          ? "walk"
          : owner === parameter && (callee.name === "describe" || callee.name === "build")
            ? callee.name
            : owner === parameter && callee.name === "facts"
              ? "facts"
              : owner === parameter && callee.name === "name" && args.length === 0
                ? "name"
                : undefined;
      if (!traversal) return value;
      // `T::name()` is the target's declared name (annot.structure.name).
      if (traversal === "name" || traversal === "facts")
        return {
          ...value,
          callee: { ...callee, owner: parameter, name: `hd_${traversal}` },
          arguments: [],
        };
      const argument = traversal === "walk" ? args[1] : args[0];
      const local = argument?.kind === "name" ? localType(original, argument.name) : undefined;
      if (!local) {
        error(
          "unsupported-derivation",
          `the prototype needs the ${traversal === "walk" ? "walker" : traversal === "describe" ? "describer" : "source"} of ${traversal} in a local declared with its type`,
          value.span as SourceSpan,
        );
        return value;
      }
      const protocol =
        traversal === "walk" ? "Walker" : traversal === "describe" ? "Describer" : "Source";
      const site = `hd_${traversal}_${sites.length}`;
      const errorType =
        protocolError(implementations, renames.get(protocol) ?? protocol, local) ?? "never";
      const mentioned = templateMethod.genericParameters.filter((item) =>
        new RegExp(`\\b${item}\\b`).test(`${local} ${errorType}`),
      );
      sites.push({
        traversal,
        visitor: local,
        errorType,
        methodParameters: mentioned,
        methodBounds: templateMethod.genericBounds.filter((bound) =>
          mentioned.includes(bound.parameter),
        ),
      });
      if (traversal === "walk")
        return {
          ...value,
          callee: { kind: "member", receiver: args[0], name: site, span: callee.span },
          arguments: [args[1]],
        };
      return {
        ...value,
        callee: { ...callee, owner: parameter, name: site },
        arguments: [args[0]],
      };
    }) as Statement[];
    const functionName = `hd__template_${key}_${templateMethod.name}`;
    functionNames.set(templateMethod.name, functionName);
    functions.push(
      renameTypes(
        {
          kind: "function",
          compilerGenerated: true,
          ...(template.standard ? { standard: true as const } : {}),
          name: functionName,
          suspending: templateMethod.suspending,
          genericParameters: [parameter, ...templateMethod.genericParameters],
          genericBounds: [
            { parameter, traits: [structureTrait], span: template.span },
            ...templateMethod.genericBounds,
          ],
          parameters: templateMethod.parameters.map((item) =>
            item.name === "self" ? { ...item, name: TEMPLATE_SELF } : item,
          ),
          result: templateMethod.result,
          requirements: templateMethod.requirements,
          ...(templateMethod.requirementsOmitted ? { requirementsOmitted: true } : {}),
          ...(templateMethod.resultOmitted ? { resultOmitted: true } : {}),
          body,
          span: templateMethod.span,
        } satisfies FunctionDecl,
        new Map([["Self", parameter]]),
      ),
    );
  }
  const bounded = needsTrait
    ? functions.map((item): FunctionDecl => ({
        ...item,
        genericBounds: item.genericBounds.map((bound, position) =>
          position === 0 ? { ...bound, traits: [...bound.traits, template.traitName!] } : bound,
        ),
      }))
    : functions;
  const at = template.span;
  const toSelf = new Map([[parameter, "Self"]]);
  const signature = (
    name: string,
    parameters: readonly [string, string][],
    result: string,
    site?: TemplateSite,
  ): MethodDecl => ({
    name,
    suspending: false,
    genericParameters: site?.methodParameters ?? [],
    genericBounds: site?.methodBounds ?? [],
    parameters: parameters.map(([item, type]) => ({
      name: item,
      type: { name: type, span: at },
      span: at,
    })),
    result: { name: result, span: at },
    requirements: [],
    span: at,
  });
  const trait: TraitDecl = {
    kind: "trait",
    ...(template.standard ? { standard: true as const } : {}),
    name: structureTrait,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      signature("hd_name", [], "string"),
      signature("hd_facts", [], renames.get("Facts") ?? "Facts"),
      ...sites.map((site, position) => {
        const visitor = renameWords(site.visitor, toSelf);
        const errorType = renameWords(site.errorType, toSelf);
        const name = `hd_${site.traversal}_${position}`;
        if (site.traversal === "walk")
          return signature(
            name,
            [
              ["self", "Self"],
              ["w", visitor],
            ],
            `Result[void,${errorType}]`,
            site,
          );
        if (site.traversal === "describe")
          return signature(name, [["d", visitor]], `Result[void,${errorType}]`, site);
        return signature(
          name,
          [["s", visitor]],
          `Result[${tuple ? "" : "mut:"}Self,${errorType}]`,
          site,
        );
      }),
    ],
    span: at,
  };
  return {
    template,
    parameter,
    structureTrait,
    sites,
    functions: functionNames,
    declarations: { trait, functions: bounded },
    tuple,
  };
}

/** What one derivation's two implementations are built from. */
interface InstanceInput {
  readonly compiled: CompiledTemplate;
  readonly block?: ImplDecl;
  /** The target's declared name, its parameters, and its type with them. */
  readonly declarationName: string;
  readonly parameters: readonly string[];
  readonly targetType: string;
  /** The derived bounds: each bounded parameter gets `traits`. */
  readonly bounded: readonly string[];
  readonly traits: readonly string[];
  /** A tuple instance's bound on its rest item type, `TUPLE_REST`. */
  readonly restBound?: readonly string[];
  /** The names of the derivation's traversals and of its facts function. */
  readonly prefix: string;
  readonly part: string;
  readonly checked: boolean;
  /** The target belongs to std, so generated code is reachable only through use. */
  readonly standard?: boolean;
  readonly renames: ReadonlyMap<string, string>;
  readonly span: SourceSpan;
}

/**
 * The target's implementation of the template's hidden `Structure` trait,
 * and of the derived trait, whose methods call the template's functions.
 */
export function instanceImplementations(input: InstanceInput): {
  readonly structure: ImplDecl;
  readonly implementation: ImplDecl;
} {
  const { compiled, block, parameters, targetType, bounded, traits, prefix, part } = input;
  const { checked, renames } = input;
  const targetRenames = new Map([[compiled.parameter, targetType]]);
  const derivation = { block, span: input.span };
  const declaration = { name: input.declarationName };
  // Both implementations take the block's header, or the derived bounds.
  const genericParameters = derivation.block?.genericParameters.length
    ? derivation.block.genericParameters
    : parameters;
  const genericBounds = derivation.block?.genericParameters.length
    ? derivation.block.genericBounds
    : bounded.map((parameter) => ({
        parameter,
        traits: parameter === TUPLE_REST && input.restBound ? input.restBound : traits,
        span: derivation.span,
      }));
  const at = derivation.span;
  const typeArguments = parameters.map((parameter) => ({ name: parameter, span: at }));
  const name = (text: string): Expression => ({ kind: "name", name: text, span: at });
  const call = (callee: string, args: Expression[], types: readonly TypeRef[]): Expression => ({
    kind: "call",
    callee: name(callee),
    arguments: args,
    ...(types.length > 0 ? { typeArguments: types } : {}),
    span: at,
  });
  const method = (
    methodName: string,
    methodParameters: readonly [string, string][],
    result: string,
    body: Expression,
    site?: TemplateSite,
  ): MethodDecl => ({
    name: methodName,
    suspending: false,
    genericParameters: site?.methodParameters ?? [],
    genericBounds: site?.methodBounds ?? [],
    parameters: methodParameters.map(([parameter, type]) => ({
      name: parameter,
      type: { name: type, span: at },
      span: at,
    })),
    result: { name: result, span: at },
    requirements: [],
    body: [{ kind: "expression", expression: body, span: at }],
    span: at,
  });

  // The target's `Structure` for this template: its name, its facts, and
  // one method per traversal call site.
  const structure: ImplDecl = {
    kind: "impl",
    ...(input.standard ? { standard: true as const } : {}),
    genericParameters,
    genericBounds,
    traitName: compiled.structureTrait,
    targetName: targetType,
    associatedTypes: [],
    methods: [
      method("hd_name", [], "string", { kind: "string", value: declaration.name, span: at }),
      method("hd_facts", [], renames.get("Facts") ?? "Facts", call(`${part}_facts`, [], [])),
      ...compiled.sites.map((site, position) => {
        const visitor = renameWords(site.visitor, targetRenames);
        const errorType = renameWords(site.errorType, targetRenames);
        const traversal = `${prefix}_${site.traversal}_${position}`;
        const siteName = `hd_${site.traversal}_${position}`;
        const siteArguments = [
          ...typeArguments,
          ...site.methodParameters.map((parameter) => ({ name: parameter, span: at })),
        ];
        if (site.traversal === "walk")
          return method(
            siteName,
            [
              ["self", "Self"],
              ["w", visitor],
            ],
            `Result[void,${errorType}]`,
            call(traversal, [name("self"), name("w")], siteArguments),
            site,
          );
        const argument = site.traversal === "describe" ? "d" : "s";
        return method(
          siteName,
          [[argument, visitor]],
          site.traversal === "describe"
            ? `Result[void,${errorType}]`
            : `Result[${compiled.tuple ? "" : "mut:"}${targetType},${errorType}]`,
          call(traversal, [name(argument)], siteArguments),
          site,
        );
      }),
    ],
    span: at,
  };

  // The derived implementation: each template method calls the template's
  // function, checked once, with the target for its parameter; a block's
  // own methods replace the template's.
  const overridden = new Set((derivation.block?.methods ?? []).map((item) => item.name));
  const forwarded = compiled.template.methods
    .filter((item) => !overridden.has(item.name))
    .map((item): MethodDecl => {
      const signature = renameTypes({ ...item, body: undefined }, targetRenames);
      const args = item.parameters.map((parameter) => name(parameter.name));
      const types = [targetType, ...item.genericParameters].map((type) => ({
        name: type,
        span: at,
      }));
      return {
        ...signature,
        body: [
          {
            kind: "expression",
            expression: call(compiled.functions.get(item.name)!, args, types),
            span: at,
          },
        ],
        span: at,
      };
    });
  const implementation: ImplDecl = {
    kind: "impl",
    ...(input.standard ? { standard: true as const } : {}),
    genericParameters,
    genericBounds,
    traitName: renameWords(compiled.template.traitName!, targetRenames),
    targetName: targetType,
    associatedTypes: renameTypes(compiled.template.associatedTypes, targetRenames),
    methods: [...forwarded, ...(derivation.block?.methods ?? [])],
    // An unmet derived bound at a use is `missing-derived-bound` (trait.derive.bound-unmet).
    span: checked ? derivedImplementationSpan(derivation.span) : derivation.span,
  };
  return { structure, implementation };
}

// ---------------------------------------------------------------------------
// Newtypes derive through their base (annot.derive.means, trait.derive.newtype).

function forwardingAllowed(type: string): boolean {
  if (!/\bSelf\b/.test(type)) return true;
  const plain = readonlyType(type);
  return (
    plain === "Self" ||
    plain === "Self?" ||
    plain === "List[Self]" ||
    plain.startsWith("Result[Self,")
  );
}

/**
 * A generic function that calls a trait method through its bound, so a
 * newtype's base type without the trait is an unsatisfied bound at the call
 * (trait.derive.newtype.requires.error).
 */
function newtypeHelper(
  trait: string,
  method: MethodDecl,
  parameter: string,
  helpers: Map<string, FunctionDecl>,
): string {
  const name = `hd__newtype_${trait}_${method.name}`;
  if (helpers.has(name)) return name;
  const span = method.span;
  const parameters = method.parameters.map((item) =>
    item.name === "self" ? { ...item, name: TEMPLATE_SELF } : item,
  );
  const call: Expression = {
    kind: "call",
    callee: {
      kind: "member",
      receiver: { kind: "name", name: TEMPLATE_SELF, span },
      name: method.name,
      span,
    },
    arguments: parameters.slice(1).map((item) => ({ kind: "name", name: item.name, span })),
    span,
  };
  helpers.set(
    name,
    renameTypes(
      {
        kind: "function",
        compilerGenerated: true,
        name,
        suspending: false,
        genericParameters: [parameter],
        genericBounds: [{ parameter, traits: [trait], span }],
        parameters,
        result: method.result,
        requirements: [],
        body: [{ kind: "expression", expression: call, span }],
        span,
      } satisfies FunctionDecl,
      new Map([["Self", parameter]]),
    ),
  );
  return name;
}

export function forwardNewtype(
  item: { trait: string; declaration: TypeDecl; span: SourceSpan },
  template: ImplDecl,
  checked: boolean,
  helpers: Map<string, FunctionDecl>,
  usedHelpers: Set<string>,
  error: (code: string, message: string, span: SourceSpan) => void,
): ImplDecl | undefined {
  const parameter = template.genericParameters[0]!;
  const selfRenames = new Map([[parameter, "Self"]]);
  const methods: MethodDecl[] = [];
  for (const method of template.methods) {
    const positions = [
      ...method.parameters.filter((value) => value.name !== "self").map((value) => value.type.name),
      method.result.name,
    ].map((type) => renameWords(type, selfRenames));
    const bad = positions.find((type) => !forwardingAllowed(type));
    if (bad !== undefined) {
      error(
        "newtype-derivation-self",
        `method '${method.name}' has Self in '${displayType(bad)}'; a newtype forwards only the receiver, Self, Self?, Result[Self, E], and List[Self]`,
        item.span,
      );
      return undefined;
    }
    const unsupported = positions.find(
      (type) => /\bSelf\b/.test(type) && readonlyType(type) !== "Self",
    );
    if (unsupported !== undefined) {
      error(
        "unsupported-derivation",
        `the prototype forwards a newtype method only through the receiver and plain Self, not '${displayType(unsupported)}'`,
        item.span,
      );
      return undefined;
    }
    const span = item.span;
    const base = item.declaration.base!.name;
    const unwrap = (expression: Expression): Expression => ({
      kind: "member",
      receiver: expression,
      name: NEWTYPE_FIELD,
      span,
    });
    const receiver = method.parameters[0]?.name === "self";
    const args = method.parameters
      .filter((value) => value.name !== "self")
      .map((value): Expression => {
        const name: Expression = { kind: "name", name: value.name, span };
        return readonlyType(renameWords(value.type.name, selfRenames)) === "Self"
          ? unwrap(name)
          : name;
      });
    // A comparison trait calls the base type's method qualified by the trait,
    // so a base type without it is `derive-field-missing-trait` at the base
    // type (trait.derive.newtype.requires.error).
    const at = derivedBaseSpan(item.declaration, item.trait);
    const helper = checked ? newtypeHelper(item.trait, method, parameter, helpers) : undefined;
    if (helper) usedHelpers.add(helper);
    const call: Expression = helper
      ? {
          kind: "call",
          callee: {
            kind: "name",
            name: helper,
            span: at,
          },
          arguments: receiver ? [unwrap({ kind: "name", name: "self", span }), ...args] : args,
          span: at,
        }
      : receiver
        ? {
            kind: "call",
            callee: {
              kind: "member",
              receiver: unwrap({ kind: "name", name: "self", span }),
              name: method.name,
              span,
            },
            arguments: args,
            span,
          }
        : {
            kind: "call",
            callee: { kind: "qualified-name", owner: base, name: method.name, span },
            arguments: args,
            span,
          };
    const wrapped: Expression =
      readonlyType(renameWords(method.result.name, selfRenames)) === "Self"
        ? {
            kind: "call",
            callee: { kind: "name", name: item.declaration.name, span },
            arguments: [call],
            span,
          }
        : call;
    methods.push({
      ...renameTypes(method, new Map([[parameter, item.declaration.name]])),
      body: [{ kind: "expression", expression: wrapped, span }],
      span,
    });
  }
  return {
    kind: "impl",
    genericParameters: item.declaration.genericParameters,
    genericBounds: checked
      ? item.declaration.genericParameters.map((name) => ({
          parameter: name,
          traits: [item.trait],
          span: item.span,
        }))
      : [],
    traitName: item.trait,
    targetName: item.declaration.name,
    associatedTypes: [],
    methods,
    span: item.span,
  };
}
