import type { HostBoundaryValue } from "./compiler.ts";
import {
  boundaryShape,
  isStringListArgument,
  programLookups,
  structuralHostArgument,
} from "./host-boundary.ts";
import type { HirProgram, ValueType } from "./hir.ts";
import { hostArgumentValue } from "./host-values.ts";
import { substituteTypeParameters } from "./types.ts";

// The arguments of a host capability method call that cross after its
// `begin` import (emitter/host-providers.ts): a `string` crosses as its
// length at `begin` and then byte by byte, and a `List[string]` as its
// length at `begin`, then each element's length and bytes. Any other
// argument that is not a scalar, such as a `Path` or a `List[u8]`, crosses
// as a stream of scalar tokens that the shared `host_argument_*` imports
// collect, and the host rebuilds it from its declared type.

/** The bytes a call's streamed arguments fill, by argument index. */
export interface ArgumentBuffers {
  readonly argumentBytes: ReadonlyMap<number, Uint8Array>;
  /** The UTF-8 bytes of each element of a `List[string]` argument. */
  readonly argumentLists: ReadonlyMap<number, Uint8Array[]>;
  /** The tokens of the structural arguments, in argument order. */
  readonly argumentTokens: (number | bigint)[];
}

/** Empty buffers for a call whose `begin` received `lengths`. */
export function argumentBuffers(
  parameters: readonly ValueType[],
  lengths: readonly unknown[],
): ArgumentBuffers {
  const argumentBytes = new Map<number, Uint8Array>();
  const argumentLists = new Map<number, Uint8Array[]>();
  parameters.forEach((parameter, index) => {
    if (parameter === "string") argumentBytes.set(index, new Uint8Array(Number(lengths[index])));
    else if (isStringListArgument(parameter))
      argumentLists.set(
        index,
        Array.from({ length: Number(lengths[index]) }, () => new Uint8Array(0)),
      );
  });
  return { argumentBytes, argumentLists, argumentTokens: [] };
}

/** The imports that fill a call's buffers, for a method whose name `name` errors show. */
export function streamedArgumentImports(
  prefix: string,
  name: string,
  parameters: readonly ValueType[],
): Record<string, (...arguments_: unknown[]) => void> {
  const imports: Record<string, (...arguments_: unknown[]) => void> = {};
  const invalid = (what: string): Error =>
    new Error(`host provider ${name} received an invalid ${what}`);
  const fill = (bytes: Uint8Array | undefined, byteIndex: unknown, byte: unknown): void => {
    if (!bytes || Number(byteIndex) >= bytes.length) throw invalid("byte");
    bytes[Number(byteIndex)] = Number(byte);
  };
  if (parameters.includes("string"))
    imports[`${prefix}_argument_byte`] = (value, argumentIndex, byteIndex, byte) =>
      fill((value as ArgumentBuffers).argumentBytes.get(Number(argumentIndex)), byteIndex, byte);
  if (parameters.some(isStringListArgument)) {
    const elements = (value: unknown, argumentIndex: unknown): Uint8Array[] => {
      const list = (value as ArgumentBuffers).argumentLists.get(Number(argumentIndex));
      if (!list) throw invalid("list");
      return list;
    };
    imports[`${prefix}_argument_element`] = (value, argumentIndex, elementIndex, length) => {
      const list = elements(value, argumentIndex);
      if (Number(elementIndex) >= list.length) throw invalid("element");
      list[Number(elementIndex)] = new Uint8Array(Number(length));
    };
    imports[`${prefix}_argument_element_byte`] = (
      value,
      argumentIndex,
      elementIndex,
      byteIndex,
      byte,
    ) => fill(elements(value, argumentIndex)[Number(elementIndex)], byteIndex, byte);
  }
  return imports;
}

/** The four token imports every structural argument encoder shares. */
export function argumentTokenImports(): Record<string, (...arguments_: unknown[]) => void> {
  const push = (value: unknown, token: unknown): void => {
    (value as ArgumentBuffers).argumentTokens.push(token as number | bigint);
  };
  return {
    host_argument_i32: push,
    host_argument_i64: push,
    host_argument_f32: push,
    host_argument_f64: push,
  };
}

/** Puts the value of each streamed argument into `arguments_`. */
export function decodeStreamedArguments(
  program: HirProgram,
  parameters: readonly ValueType[],
  buffers: ArgumentBuffers,
  arguments_: unknown[],
  decoder: TextDecoder,
): void {
  for (const [index, bytes] of buffers.argumentBytes) arguments_[index] = decoder.decode(bytes);
  for (const [index, list] of buffers.argumentLists)
    arguments_[index] = list.map((bytes) => decoder.decode(bytes));
  const tokens = buffers.argumentTokens;
  let cursor = 0;
  const next = (): number | bigint => {
    if (cursor >= tokens.length) throw new Error("host argument stream ended early");
    return tokens[cursor++]!;
  };
  const count = (): number => Number(next()) >>> 0;
  const lookups = programLookups(program);
  // The value as a host provider sees it, the same view a structural host
  // result takes (compiler.ts checkedHostBoundaryNode).
  const read = (type: ValueType): HostBoundaryValue => {
    const shape = boundaryShape(type, ...lookups);
    switch (shape.kind) {
      case "scalar":
        return hostArgumentValue(type, next());
      case "string":
        return decoder.decode(Uint8Array.from({ length: count() }, () => Number(next())));
      case "optional":
        return count() === 0 ? { tag: "none" } : { tag: "some", value: read(shape.inner) };
      case "result": {
        const ok = count() === 0;
        const side = ok ? shape.ok : shape.err;
        const tag = ok ? "ok" : "err";
        return (side === "void" ? { tag } : { tag, value: read(side) }) as HostBoundaryValue;
      }
      case "tuple":
        return shape.elements.map(read);
      case "list":
        return Array.from({ length: count() }, () => read(shape.element));
      case "data": {
        const data = shape.declaration;
        const substitutions = new Map(
          data.genericParameters.map(
            (parameter, index) => [parameter, shape.arguments[index]!] as const,
          ),
        );
        const fields = data.fields.map(
          (field) =>
            [field.name, read(substituteTypeParameters(field.type, substitutions))] as const,
        );
        // A newtype, as `Path`, crosses as its base value.
        if (data.newtype) return fields[0]![1];
        return Object.fromEntries(fields);
      }
      case "enum": {
        const tag = count();
        const variant = shape.declaration.variants.find((candidate) => candidate.tag === tag);
        if (!variant) throw new Error(`host argument has an invalid '${type}' tag ${tag}`);
        return Object.fromEntries([
          ["tag", variant.name],
          ...variant.fields.map((field) => [field.name, read(field.type)] as const),
        ]) as HostBoundaryValue;
      }
      case "other":
        throw new Error(`the host cannot read a '${type}' argument`);
    }
  };
  parameters.forEach((parameter, index) => {
    if (structuralHostArgument(parameter)) arguments_[index] = read(parameter);
  });
  if (cursor !== tokens.length) throw new Error("host argument stream has extra tokens");
}
