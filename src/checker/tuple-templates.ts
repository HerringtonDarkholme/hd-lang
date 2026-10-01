import type { DataDecl, ImplDecl, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { restInner, tupleParts, tupleType } from "../types.ts";
import type { MemberModel, VariantModel } from "./derivation-models.ts";
import type { Target } from "./member-lines.ts";
import {
  mentionedNames,
  standardSupertraits,
  standardTemplate,
  standardTupleTraits,
  withStandardLibrary,
  type StandardTemplate,
} from "./standard-library.ts";
import { withStandardTraits } from "./standard-traits.ts";
import {
  compileTemplate,
  headName,
  isTypeRef,
  TUPLE_REST,
  visit,
  type CompiledTemplate,
} from "./template-instances.ts";
import type { Generated } from "./typed-derivation.ts";

// Tuple templates (spec/14-annotations.md#tuple-templates) are instantiated
// once per tuple shape, its number of fixed elements and whether it has a
// rest element, as a generic implementation over the element types:
// `impl[hd_E0 < Eq, hd_E1 < Eq] Eq for (hd_E0, hd_E1)`. That is one
// implementation for every tuple type of the shape, so the pass needs only
// the shapes a program has, never an inferred element type. Every tuple
// type is written, is a tuple expression, or comes from a std declaration
// the program uses, so the shapes are read from the program as the std join
// will declare it (`tupleDemand`). Only the std tuple templates of traits
// that program mentions are instantiated, with their supertraits.

/** A tuple's shape: its fixed elements, and whether a rest element `List[T]...` follows them. */
export interface TupleShape {
  readonly fixed: number;
  readonly rest: boolean;
}

/** What `generateDerivation` needs to instantiate a tuple template for one shape. */
export interface TupleInstance extends TupleShape {
  /** The tuple type over the instance's parameters, as `(hd_E0,List[hd_R]...)`. */
  readonly type: string;
  readonly variant: VariantModel;
  /** The bound on each element type, which the members' obligation gives. */
  readonly elementBound: readonly string[];
  /** The bound on the rest item type `hd_R`, which the rest member's obligation gives. */
  readonly restBound: readonly string[];
  /** Whether the walker type implements `rest` itself (annot.walk.rest.default). */
  readonly implementsRest: (visitor: string) => boolean;
}

const shapeKey = (shape: TupleShape): string => `${shape.fixed}${shape.rest ? "+" : ""}`;

/** The tuple shapes in each type text, tuple expression, and implementation target of `node`. */
export function tupleShapes(node: unknown): TupleShape[] {
  const shapes = new Map<string, TupleShape>();
  const add = (shape: TupleShape): void => {
    shapes.set(shapeKey(shape), shape);
  };
  const scanType = (text: string): void => {
    for (let index = text.indexOf("("); index >= 0; index = text.indexOf("(", index + 1)) {
      // A function type's inputs are not a tuple value.
      if (/fn!?$/.test(text.slice(0, index))) continue;
      let depth = 0;
      let end = index;
      for (; end < text.length; end += 1) {
        const character = text[end];
        if (character === "(" || character === "[") depth += 1;
        else if (character === ")" || character === "]") depth -= 1;
        if (depth === 0) break;
      }
      const parts = tupleParts(text.slice(index, end + 1));
      if (!parts) continue;
      const rest = parts.length > 0 && restInner(parts.at(-1)!) !== undefined;
      add({ fixed: parts.length - (rest ? 1 : 0), rest });
    }
  };
  visit(node, (value) => {
    if (isTypeRef(value)) scanType(value.name);
    else if (value.kind === "impl" && typeof value.targetName === "string")
      scanType(value.targetName);
    else if (value.kind === "tuple" && Array.isArray(value.elements)) {
      // A tuple expression, or a pattern, which has the shape of the tuple it matches.
      const rest = value.spread === true;
      add({ fixed: value.elements.length - (rest ? 1 : 0), rest });
    }
  });
  return [...shapes.values()].sort(
    (left, right) => left.fixed - right.fixed || Number(left.rest) - Number(right.rest),
  );
}

/**
 * The tuple shapes and the names that the program mentions once the std
 * join declares what it uses, such as `enumerate`'s `(i32, T)` and
 * `assert_equal`'s `Debug`.
 */
export function tupleDemand(program: Program): {
  readonly shapes: readonly TupleShape[];
  readonly mentioned: ReadonlySet<string>;
} {
  const joined = withStandardLibrary(withStandardTraits(program));
  // A std implementation's body works on its own type parameters, so it
  // needs no tuple implementation that its signature does not show; and a
  // declaration's own name is not a use of it.
  const used = [
    joined.statements,
    joined.tests,
    joined.implementations.filter((implementation) => !implementation.standard),
    ...[
      ...(joined.types ?? []),
      ...joined.data,
      ...joined.enums,
      ...joined.traits,
      ...joined.functions,
    ].map(
      ({
        name: _name,
        standardName: _standard,
        ...rest
      }: {
        name: string;
        standardName?: string;
      }) => rest,
    ),
  ];
  const mentioned = new Set<string>();
  mentionedNames(used, mentioned);
  return { shapes: tupleShapes(used), mentioned };
}

/** The synthetic target, type, and variant of a tuple shape (annot.tuple.*). */
export function tupleTarget(
  shape: TupleShape,
  span: SourceSpan,
): { readonly target: Target; readonly type: string; readonly variant: VariantModel } {
  const elements = Array.from({ length: shape.fixed }, (_, index) => `hd_E${index}`);
  const types = [...elements, ...(shape.rest ? [`List[${TUPLE_REST}]`] : [])];
  const declaration: DataDecl = {
    kind: "data",
    name: `hd__tuple${shapeKey(shape).replace("+", "r")}`,
    genericParameters: [...elements, ...(shape.rest ? [TUPLE_REST] : [])],
    fields: types.map((type, position) => ({
      name: `_${position}`,
      type: { name: type, span },
      positional: true,
      span,
    })),
    span,
  };
  const members = declaration.fields.map((field, position): MemberModel => ({
    name: field.name,
    access: field.name,
    declared: field.type.name,
    position,
    facts: [],
    embedded: false,
    positional: true,
    omitted: false,
    selfRef: "Absent",
    field,
  }));
  return {
    target: { kind: "data", declaration },
    type: tupleType(
      types.map((type, index) => (shape.rest && index === types.length - 1 ? `${type}...` : type)),
    ),
    variant: { name: "", index: 0, facts: [], ofData: true, members, selfRef: "Absent" },
  };
}

/** The `rest` method of the walker implementation for `visitor`, if it declares one. */
function restMethod(implementations: readonly ImplDecl[], walker: string, visitor: string) {
  const head = headName(visitor);
  return implementations
    .find(
      (implementation) =>
        implementation.traitName !== undefined &&
        headName(implementation.traitName) === walker &&
        headName(implementation.targetName) === head,
    )
    ?.methods.find((method) => method.name === "rest");
}

/** The `member` method of the `protocol` implementation for each call site's visitor type. */
function memberMethods(
  compiled: CompiledTemplate,
  implementations: readonly ImplDecl[],
  renames: ReadonlyMap<string, string>,
): ImplDecl["methods"][number][] {
  return compiled.sites.flatMap((site) => {
    const protocol =
      site.traversal === "walk" ? "Walker" : site.traversal === "describe" ? "Describer" : "Source";
    const name = renames.get(protocol) ?? protocol;
    const member = implementations
      .find(
        (implementation) =>
          implementation.traitName !== undefined &&
          headName(implementation.traitName) === name &&
          headName(implementation.targetName) === headName(site.visitor),
      )
      ?.methods.find((method) => method.name === "member");
    return member ? [member] : [];
  });
}

/** The bound on a method's first type parameter, as `Eq` in `member[F < Eq]`. */
function firstBound(method: ImplDecl["methods"][number]): readonly string[] {
  const parameter = method.genericParameters[0];
  return method.genericBounds.find((item) => item.parameter === parameter)?.traits ?? [];
}

/**
 * The bound on each element type: the strengthened bounds of the walkers,
 * describers, and sources that the template passes
 * (annot.template.tuple.implements, annot.walker.obligation).
 */
export function elementBound(
  compiled: CompiledTemplate,
  implementations: readonly ImplDecl[],
  renames: ReadonlyMap<string, string>,
): readonly string[] {
  return [...new Set(memberMethods(compiled, implementations, renames).flatMap(firstBound))];
}

/**
 * The bound on a rest item type `T`, or undefined when no rest tuple meets
 * the template's obligation (annot.walker.obligation.rest): a walker that
 * implements `rest` puts its bound on `T`; otherwise the rest member is a
 * `List[T]` member, so `T` takes the bounds of each element trait's
 * implementation for `List[T]`, and with none, as for `Hash`, no rest
 * tuple has the trait.
 */
export function restBound(
  compiled: CompiledTemplate,
  elements: readonly string[],
  implementations: readonly ImplDecl[],
  walker: string,
): readonly string[] | undefined {
  const bound = new Set<string>();
  const listBound = (): readonly string[] | undefined => {
    const traits: string[] = [];
    for (const trait of elements) {
      const list = implementations.find(
        (implementation) =>
          implementation.traitName === trait && headName(implementation.targetName) === "List",
      );
      if (!list) return undefined;
      const parameter = list.genericParameters[0];
      traits.push(
        ...(list.genericBounds.find((item) => item.parameter === parameter)?.traits ?? []),
      );
    }
    return traits;
  };
  for (const site of compiled.sites) {
    if (site.traversal === "describe") continue;
    const rest =
      site.traversal === "walk" ? restMethod(implementations, walker, site.visitor) : undefined;
    const traits = rest ? firstBound(rest) : listBound();
    if (!traits) return undefined;
    for (const item of traits) bound.add(item);
  }
  return [...bound];
}

/** Whether the walker implementation for `visitor` declares `rest`. */
export function implementsRest(
  implementations: readonly ImplDecl[],
  walker: string,
): (visitor: string) => boolean {
  return (visitor) => restMethod(implementations, walker, visitor) !== undefined;
}

/** The program's name for `std.function.Tuple`. */
export function localTupleName(program: Program): string {
  for (const use of program.uses)
    if (use.module === "std.function")
      for (const name of use.names) if (name.name === "Tuple") return name.alias ?? name.name;
  return "Tuple";
}

/**
 * The std tuple templates of the traits that the program mentions, with
 * their supertraits, and the program's tuple shapes. A hand-written
 * implementation of a trait with a tuple template for a tuple type is
 * `overlapping-impl` (annot.template.tuple.overlap).
 */
export function loadTupleTemplates(
  program: Program,
  localTraits: ReadonlyMap<string, unknown>,
  tupleTemplates: ReadonlyMap<string, ImplDecl>,
  kept: readonly ImplDecl[],
  renames: ReadonlyMap<string, string>,
  error: (code: string, message: string, span: SourceSpan) => void,
): {
  readonly standardTuples: ReadonlyMap<string, StandardTemplate>;
  readonly shapes: readonly TupleShape[];
} {
  const candidates = standardTupleTraits(program).filter(
    (name) => !localTraits.has(name) && !tupleTemplates.has(name),
  );
  const standardTuples = new Map<string, StandardTemplate>();
  let shapes: readonly TupleShape[] = [];
  if (tupleTemplates.size > 0 || candidates.length > 0) {
    const demand = tupleDemand(program);
    shapes = demand.shapes;
    const needed = new Set<string>();
    const need = (name: string): void => {
      if (needed.has(name) || !candidates.includes(name)) return;
      needed.add(name);
      for (const parent of standardSupertraits(program, name)) need(parent);
    };
    if (shapes.length > 0)
      for (const name of candidates) if (demand.mentioned.has(name)) need(name);
    for (const name of needed) {
      const standard = standardTemplate(program, name, renames, true);
      if (standard) standardTuples.set(name, standard);
    }
  }
  for (const implementation of kept) {
    const trait = implementation.traitName && headName(implementation.traitName);
    if (
      trait &&
      implementation.targetName.startsWith("(") &&
      (tupleTemplates.has(trait) || candidates.includes(trait))
    )
      error(
        "overlapping-impl",
        `'${trait}' is implemented for every tuple type by its tuple template`,
        implementation.span,
      );
  }
  return { standardTuples, shapes };
}

/**
 * Each tuple template once per shape, as a generic implementation over the
 * element types (annot.template.tuple.instance), through `generate`, which
 * is `generateDerivation`. A shape whose rest member fails the obligation
 * has no instance (annot.template.tuple.unmet).
 */
export function tupleInstances(
  tuples: {
    readonly tupleTemplates: ReadonlyMap<string, ImplDecl>;
    readonly standardTuples: ReadonlyMap<string, StandardTemplate>;
    readonly shapes: readonly TupleShape[];
  },
  context: {
    readonly program: Program;
    readonly templateSupport: readonly ImplDecl[];
    readonly renames: ReadonlyMap<string, string>;
    readonly error: (code: string, message: string, span: SourceSpan) => void;
    readonly compiledTemplates: Map<string, CompiledTemplate>;
  },
  generate: (
    derivation: {
      readonly trait: string;
      readonly target: Target;
      readonly lines: readonly [];
      readonly span: SourceSpan;
      readonly tuple: TupleInstance;
    },
    index: number,
    compiled: CompiledTemplate,
  ) => Generated | undefined,
): { readonly implementations: ImplDecl[]; readonly programs: Program[] } {
  const { program, templateSupport, renames } = context;
  const { tupleTemplates, standardTuples, shapes } = tuples;
  const implementations: ImplDecl[] = [];
  const programs: Program[] = [];
  if (shapes.length === 0) return { implementations, programs };
  let index = 0;
  const walker = renames.get("Walker") ?? "Walker";
  for (const trait of [...tupleTemplates.keys(), ...standardTuples.keys()]) {
    const standard = standardTuples.get(trait);
    const compiled = compileTemplate(
      standard?.template ?? tupleTemplates.get(trait)!,
      [...program.implementations, ...templateSupport],
      renames,
      context.error,
      true,
    );
    context.compiledTemplates.set(`tuple ${trait}`, compiled);
    const known = [
      ...(standard?.implementations ?? []),
      ...program.implementations,
      ...templateSupport,
    ];
    const elements = elementBound(compiled, known, renames);
    const bound = restBound(compiled, elements, known, walker);
    for (const shape of shapes) {
      if (shape.rest && bound === undefined) continue;
      const { target, type, variant } = tupleTarget(shape, program.span);
      const tuple: TupleInstance = {
        ...shape,
        type,
        variant,
        elementBound: elements,
        restBound: bound ?? [],
        implementsRest: implementsRest(known, walker),
      };
      const result = generate(
        { trait, target, lines: [], span: program.span, tuple },
        index++,
        compiled,
      );
      if (!result) continue;
      // `std` owns its tuple templates' traits, so an instance is never an orphan.
      implementations.push(
        standard ? { ...result.implementation, standard: true } : result.implementation,
        result.structure,
      );
      programs.push(result.program);
    }
  }
  return { implementations, programs };
}
