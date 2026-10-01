import type { DataDecl, EnumDecl, Expression, FunctionDecl, Program, TypeDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import {
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  tupleParts,
} from "../types.ts";
import type { HirData, HirExpression, ValueType } from "../hir.ts";
import { Source_ } from "./generated-source.ts";
import { factType } from "./typed-derivation.ts";

// The shape intrinsics (spec/14-annotations.md#shape-intrinsics).
//
// The shape types are ordinary `lib/std/annotation.hd` declarations. This
// pass generates, for each target the program names in `shape::[T]()` or
// `shape_of(f)`, an ordinary hd builder function over them, and the
// specialized shape types as hidden data types; the checker lowers each
// intrinsic call to a builder call (`checkShapeIntrinsicCall`), so no HIR
// node is shape-specific.
//
// - The specialized data shape type of `D` is `hd__DataShape_D`: it embeds
//   `DataShape`, so every `DataShape` member is promoted, and adds `fields`,
//   a `hd__ShapeFields_D` record with one `FieldShape` per direct field. The
//   checker converts it to `DataShape` by reading the embedded value.
//   Enums are the same with `EnumShape`, `variants`, and `VariantShape`.
// - `metadata::[M]()` on a shape is `hd__metadata_M()`, an inherent method of
//   each concrete shape type. It looks the attached value up by
//   `DeclarationId` in a generated table of the member metadata and type-level
//   facts whose type is `M` (annot.metadata.duplicate keeps one per type).
// - `DeclarationId`s number the declarations and members of the one module
//   the prototype compiles. `SourcePosition.file` is empty: the checker does
//   not know the file name.

const DATA_SHAPE = "hd__DataShape_";
const ENUM_SHAPE = "hd__EnumShape_";
const SHAPE_FIELDS = "hd__ShapeFields_";
const SHAPE_VARIANTS = "hd__ShapeVariants_";

/** The concrete shape types, which implement the sealed `ShapeMetadata`. */
export const SHAPE_METADATA_TYPES = new Set([
  "DataShape",
  "FieldShape",
  "EnumShape",
  "VariantShape",
  "FnShape",
  "ParamShape",
]);

function mangle(text: string): string {
  return normalized(text).replace(
    /[^A-Za-z0-9]/g,
    (character) => `_${character.charCodeAt(0).toString(16)}_`,
  );
}

/** A written type with the spacing of a checker `ValueType`. */
function normalized(text: string): string {
  return text
    .replace(/\bmut\s+/g, "mut§")
    .replace(/\s+/g, "")
    .replace(/§/g, " ");
}

export function shapeBuilderName(typeText: string): string {
  return `hd__shape_${mangle(typeText)}`;
}

export function shapeOfBuilderName(functionName: string): string {
  return `hd__shape_of_${functionName}`;
}

export function metadataMethodName(typeText: string): string {
  return `hd__metadata_${mangle(typeText)}`;
}

/** The generic shape type a specialized shape type converts to. */
export function specializedShapeBase(type: ValueType): string | undefined {
  const name = readonlyType(type);
  if (name.startsWith(DATA_SHAPE)) return "DataShape";
  if (name.startsWith(ENUM_SHAPE)) return "EnumShape";
  return undefined;
}

/**
 * A specialized shape is assignable to its generic shape type
 * (spec/14-annotations.md#shape-intrinsics): the embedded generic value.
 */
export function generalizedShape(
  value: HirExpression,
  expected: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  span: SourceSpan,
): HirExpression | undefined {
  const base = specializedShapeBase(value.type);
  if (!base || readonlyType(expected) !== base) return undefined;
  const declaration = dataTypes.get(readonlyType(value.type));
  const field = declaration?.fields.find((candidate) => candidate.name === base);
  if (!declaration || !field) return undefined;
  const { index: dataIndex } = declaration;
  return { kind: "member", receiver: value, dataIndex, fieldIndex: field.index, type: base, span };
}

/** Whether a data type is a specialized `fields` or `variants` record. */
export function isShapeMemberRecord(name: string): boolean {
  return name.startsWith(SHAPE_FIELDS) || name.startsWith(SHAPE_VARIANTS);
}

interface ShapeUses {
  readonly types: Set<string>;
  readonly functions: Set<string>;
  readonly metadata: Set<string>;
}

/** Every `shape::[T]()`, `shape_of(f)`, and `.metadata::[M]()` in the program. */
function shapeUses(program: Program): ShapeUses {
  const uses: ShapeUses = { types: new Set(), functions: new Set(), metadata: new Set() };
  const generics = new Set<string>();
  const visit = (node: unknown): void => {
    if (Array.isArray(node)) {
      for (const item of node) visit(item);
      return;
    }
    if (!node || typeof node !== "object") return;
    const record = node as Record<string, unknown>;
    const declared = Array.isArray(record.genericParameters)
      ? (record.genericParameters as string[]).filter((name) => !generics.has(name))
      : [];
    for (const name of declared) generics.add(name);
    if (record.kind === "call") {
      const call = record as unknown as Extract<Expression, { kind: "call" }>;
      const [typeArgument] = call.typeArguments ?? [];
      if (call.callee.kind === "name" && call.callee.name === "shape" && typeArgument) {
        // A type parameter's shape needs its runtime descriptor, which the
        // prototype does not pass; the checker reports it.
        const words = typeArgument.name.match(/\w+/g) ?? [];
        if (!words.some((word) => generics.has(word))) uses.types.add(typeArgument.name);
      }
      const [argument] = call.arguments;
      if (
        call.callee.kind === "name" &&
        call.callee.name === "shape_of" &&
        argument?.kind === "name"
      )
        uses.functions.add(argument.name);
      if (call.callee.kind === "member" && call.callee.name === "metadata" && typeArgument)
        uses.metadata.add(typeArgument.name);
    }
    for (const [key, child] of Object.entries(record)) if (key !== "span") visit(child);
    for (const name of declared) generics.delete(name);
  };
  visit(program);
  return uses;
}

const PRIMITIVES: ReadonlyMap<string, string> = new Map([
  ["bool", "PrimitiveKind.Bool"],
  ["char", "PrimitiveKind.Char"],
  ["string", "PrimitiveKind.String"],
  ["void", "PrimitiveKind.Void"],
  ["never", "PrimitiveKind.Never"],
  ...[8, 16, 32, 64].flatMap((bits) => [
    [`i${bits}`, `PrimitiveKind.Signed(${bits})`] as const,
    [`u${bits}`, `PrimitiveKind.Unsigned(${bits})`] as const,
  ]),
  ["f32", "PrimitiveKind.Float(32)"],
  ["f64", "PrimitiveKind.Float(64)"],
]);

/** What a `TypeShape` needs to know about a named type. */
interface ShapeTypes {
  /** The base type of a newtype. */
  newtypeBase(name: string): string | undefined;
  isTrait(name: string): boolean;
}

/** The shape facts of a program's declarations, before checking. */
function programShapeTypes(program: Program): ShapeTypes {
  const newtypes = new Map<string, TypeDecl>(
    (program.types ?? []).filter((item) => item.base).map((item) => [item.name, item] as const),
  );
  const traits = new Set(program.traits.map((item) => item.name));
  return {
    newtypeBase: (name) => newtypes.get(name)?.base?.name,
    isTrait: (name) => traits.has(name),
  };
}

/**
 * The `DeclarationId` number of a declaration or member, by qualified name: a
 * 31-bit FNV-1a hash, so the builders and the checker agree without sharing
 * state. The prototype compiles one module, so the name is unique.
 */
function declarationNumber(key: string): number {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(key)) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash & 0x7fffffff || 1;
}

class ShapeSource {
  readonly out = new Source_();

  private readonly types: ShapeTypes;

  constructor(types: ShapeTypes) {
    this.types = types;
  }

  id(key: string): string {
    return `DeclarationId { value: ${declarationNumber(key)} }`;
  }

  private text(value: string): string {
    return this.out.string(value);
  }

  private doc(value: string | undefined): string {
    return value === undefined ? ".None" : this.text(value);
  }

  private source(span: SourceSpan): string {
    return `SourcePosition { file: ${this.text("")}, line: ${span.start.line}, column: ${span.start.column} }`;
  }

  /** The `TypeShape` of a written or checked type, as an hd expression. */
  typeShape(written: string): string {
    const type = normalized(written);
    const list = (items: readonly string[]): string =>
      `[${items.map((item) => this.typeShape(item)).join(", ")}]`;
    const mutable = mutableInner(type);
    if (mutable !== undefined) return `TypeShape.Mut(${this.typeShape(mutable)})`;
    const function_ = functionParts(type);
    if (function_)
      return `TypeShape.Fn(${list(function_.parameters)}, ${this.typeShape(function_.result)}, ${function_.suspending}, ${list(function_.requirements)})`;
    const optional = optionalInner(type);
    if (optional !== undefined) return `TypeShape.Optional(${this.typeShape(optional)})`;
    const tuple = tupleParts(type);
    if (tuple) return `TypeShape.Tuple(${list(tuple)})`;
    const primitive = PRIMITIVES.get(type);
    if (primitive) return `TypeShape.Primitive(${primitive})`;
    if (type === "Any") return "TypeShape.Any";
    const nominal = nominalGenericParts(type);
    const name = nominal?.name ?? type;
    const args = nominal?.arguments ?? [];
    if (name === "List" && args.length === 1) return `TypeShape.List(${this.typeShape(args[0]!)})`;
    if (name === "Map" && args.length === 2)
      return `TypeShape.Map(${this.typeShape(args[0]!)}, ${this.typeShape(args[1]!)})`;
    if (name === "Suspend" && args.length === 1)
      return `TypeShape.Suspend(${this.typeShape(args[0]!)})`;
    const base = this.types.newtypeBase(name);
    if (base !== undefined) return `TypeShape.Newtype(${this.id(name)}, ${this.typeShape(base)})`;
    const traitName = name.startsWith("trait:") ? name.slice("trait:".length) : name;
    if (this.types.isTrait(traitName) || name.startsWith("trait:"))
      return `TypeShape.Trait(${this.id(traitName)}, ${list(args)})`;
    return `TypeShape.Named(${this.id(name)}, ${list(args)})`;
  }

  private fieldShape(
    owner: string,
    field: {
      name: string;
      type: { name: string };
      doc?: string;
      positional?: boolean;
      span: SourceSpan;
    },
    position: number,
  ): string {
    const name = field.positional ? `_${field.name}` : field.name;
    const qualified = `${owner}.${name}`;
    return `FieldShape { id: ${this.id(qualified)}, name: ${this.text(name)}, qualified_name: ${this.text(qualified)}, source: ${this.source(field.span)}, position: ${position}, doc: ${this.doc(field.doc)}, field_type: ${this.typeShape(field.type.name)} }`;
  }

  dataShape(declaration: DataDecl): void {
    const name = declaration.name;
    const fields = declaration.fields.filter((field) => !field.positional);
    this.out.add(`data ${SHAPE_FIELDS}${name}:`);
    if (fields.length === 0) this.out.add("    pass");
    for (const field of fields) this.out.add(`    ${field.name}: FieldShape`);
    this.out.add(`data ${DATA_SHAPE}${name}:`);
    this.out.add("    DataShape");
    this.out.add(`    fields: ${SHAPE_FIELDS}${name}`);
    const list = declaration.fields.map((field, position) =>
      this.fieldShape(name, field, position),
    );
    this.out.add(`fn ${shapeBuilderName(name)}() -> ${DATA_SHAPE}${name}:`);
    this.out.add(
      `    base := DataShape { id: ${this.id(name)}, name: ${this.text(name)}, qualified_name: ${this.text(name)}, source: ${this.source(declaration.span)}, doc: ${this.doc(declaration.doc)}, field_list: [${list.join(", ")}] }`,
    );
    const members = fields.map(
      (field) => `${field.name}: base.field_list[${declaration.fields.indexOf(field)}]`,
    );
    this.out.add(
      `    ${DATA_SHAPE}${name} { DataShape: ...base, fields: ${SHAPE_FIELDS}${name} { ${members.join(", ")} } }`,
    );
  }

  enumShape(declaration: EnumDecl): void {
    const name = declaration.name;
    this.out.add(`data ${SHAPE_VARIANTS}${name}:`);
    if (declaration.variants.length === 0) this.out.add("    pass");
    for (const variant of declaration.variants) this.out.add(`    ${variant.name}: VariantShape`);
    this.out.add(`data ${ENUM_SHAPE}${name}:`);
    this.out.add("    EnumShape");
    this.out.add(`    variants: ${SHAPE_VARIANTS}${name}`);
    const list = declaration.variants.map((variant, position) => {
      const qualified = `${name}.${variant.name}`;
      const payload = variant.fields.map((field, index) =>
        this.fieldShape(qualified, field, index),
      );
      return `VariantShape { id: ${this.id(qualified)}, name: ${this.text(variant.name)}, qualified_name: ${this.text(qualified)}, source: ${this.source(variant.span)}, position: ${position}, doc: ${this.doc(variant.doc)}, payload: [${payload.join(", ")}] }`;
    });
    this.out.add(`fn ${shapeBuilderName(name)}() -> ${ENUM_SHAPE}${name}:`);
    this.out.add(
      `    base := EnumShape { id: ${this.id(name)}, name: ${this.text(name)}, qualified_name: ${this.text(name)}, source: ${this.source(declaration.span)}, doc: ${this.doc(declaration.doc)}, variant_list: [${list.join(", ")}] }`,
    );
    const members = declaration.variants.map(
      (variant, position) => `${variant.name}: base.variant_list[${position}]`,
    );
    this.out.add(
      `    ${ENUM_SHAPE}${name} { EnumShape: ...base, variants: ${SHAPE_VARIANTS}${name} { ${members.join(", ")} } }`,
    );
  }

  typeShapeBuilder(typeText: string): void {
    this.out.add(`fn ${shapeBuilderName(typeText)}() -> TypeShape:`);
    this.out.add(`    ${this.typeShape(typeText)}`);
  }

  /**
   * `shape_of(f)`. The result and requirement row come from the checked
   * signature, since either may be inferred, so the checker passes them.
   */
  functionShape(declaration: FunctionDecl): void {
    const name = declaration.name;
    const params = declaration.parameters.map((parameter, position) => {
      const qualified = `${name}.${parameter.name}`;
      return `ParamShape { id: ${this.id(qualified)}, name: ${this.text(parameter.name)}, qualified_name: ${this.text(qualified)}, source: ${this.source(parameter.span)}, position: ${position}, doc: ${this.doc(parameter.doc)}, param_type: ${this.typeShape(parameter.type.name)}, has_default: ${parameter.default !== undefined} }`;
    });
    this.out.add(
      `fn ${shapeOfBuilderName(name)}(result: TypeShape, requirements: List[TypeShape]) -> FnShape:`,
    );
    this.out.add(
      `    FnShape { id: ${this.id(name)}, name: ${this.text(name)}, qualified_name: ${this.text(name)}, source: ${this.source(declaration.span)}, doc: ${this.doc(declaration.doc)}, params: [${params.join(", ")}], result: result, suspending: ${declaration.suspending}, requirements: requirements }`,
    );
  }

  /** Every attached value in the program, keyed like the shapes' ids. */
  attached(program: Program): readonly { readonly key: string; readonly fact: Expression }[] {
    const result: { key: string; fact: Expression }[] = [];
    const add = (key: string, facts: readonly Expression[] | undefined): void => {
      for (const fact of facts ?? []) result.push({ key, fact });
    };
    for (const declaration of program.data) {
      add(declaration.name, declaration.decorators?.facts);
      for (const field of declaration.fields)
        add(
          `${declaration.name}.${field.positional ? `_${field.name}` : field.name}`,
          field.metadata,
        );
    }
    for (const declaration of program.enums) {
      add(declaration.name, declaration.decorators?.facts);
      for (const variant of declaration.variants) {
        const qualified = `${declaration.name}.${variant.name}`;
        add(qualified, variant.metadata);
        for (const field of variant.fields)
          add(`${qualified}.${field.positional ? `_${field.name}` : field.name}`, field.metadata);
      }
    }
    for (const declaration of program.functions) {
      // A function's decorator values (annot.decorator.fn-read).
      add(declaration.name, declaration.decorators?.facts);
      for (const parameter of declaration.parameters)
        add(`${declaration.name}.${parameter.name}`, parameter.metadata);
    }
    return result;
  }

  /** `metadata::[M]()` for every concrete shape type, over one lookup table. */
  metadataLookup(program: Program, typeText: string): void {
    const functions = new Map(program.functions.map((item) => [item.name, item] as const));
    const wanted = readonlyType(normalized(typeText));
    const lookup = `hd__shape_metadata_${mangle(typeText)}`;
    const type = typeText;
    this.out.add(`fn ${lookup}(id: DeclarationId) -> ${type}?:`);
    for (const { key, fact } of this.attached(program)) {
      if (normalized(factType(fact, functions)) !== wanted) continue;
      this.out.add(`    if id.value == ${declarationNumber(key)}:`);
      this.out.add(`        return ${this.out.expression(fact)}`);
    }
    this.out.add("    .None");
    for (const shapeType of SHAPE_METADATA_TYPES) {
      this.out.add(`impl ${shapeType}:`);
      this.out.add(`    pub fn ${metadataMethodName(typeText)}(self) -> ${type}?:`);
      this.out.add(`        ${lookup}(self.id)`);
    }
  }
}

/** The program with the builders its shape intrinsics call. */
export function withShapes(program: Program): Program {
  const uses = shapeUses(program);
  if (uses.types.size === 0 && uses.functions.size === 0 && uses.metadata.size === 0)
    return program;
  const source = new ShapeSource(programShapeTypes(program));
  const data = new Map(program.data.map((item) => [item.name, item] as const));
  const enums = new Map(program.enums.map((item) => [item.name, item] as const));
  const functions = new Map(program.functions.map((item) => [item.name, item] as const));
  for (const typeText of uses.types) {
    const declaration = data.get(typeText);
    const enumeration = enums.get(typeText);
    if (declaration && declaration.genericParameters.length === 0 && !declaration.newtype)
      source.dataShape(declaration);
    else if (enumeration && enumeration.genericParameters.length === 0)
      source.enumShape(enumeration);
    else source.typeShapeBuilder(typeText);
  }
  for (const name of uses.functions) {
    const declaration = functions.get(name);
    if (declaration && !name.startsWith("hd__")) source.functionShape(declaration);
  }
  for (const typeText of uses.metadata) source.metadataLookup(program, typeText);
  if (source.out.lines.length === 0) return program;
  const generated = source.out.program(program.span);
  return {
    ...program,
    data: [...program.data, ...generated.data],
    implementations: [
      ...program.implementations,
      ...generated.implementations.map((item) => ({ ...item, standard: true })),
    ],
    functions: [...program.functions, ...generated.functions],
  };
}

/**
 * The builder call for `shape_of(f)`, with the checked result type and
 * requirement row of `f` as `TypeShape` arguments.
 */
export function shapeOfCall(
  types: ShapeTypes,
  functionName: string,
  result: ValueType,
  requirements: readonly string[],
  span: SourceSpan,
): Expression {
  const source = new ShapeSource(types);
  const row = requirements.map((requirement) => source.typeShape(requirement)).join(", ");
  source.out.add(`fn hd__shape_of_call() -> FnShape:`);
  source.out.add(`    ${shapeOfBuilderName(functionName)}(${source.typeShape(result)}, [${row}])`);
  const parsed = source.out.program(span);
  const statement = parsed.functions[0]!.body[0]!;
  if (statement.kind !== "expression") throw new Error("shape_of call did not parse");
  return statement.expression;
}
