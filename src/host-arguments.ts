import { isStringListArgument } from "./host-boundary.ts";
import type { ValueType } from "./hir.ts";

// The arguments of a host capability method call that cross after its
// `begin` import (emitter/host-providers.ts): a `string` crosses as its
// length at `begin` and then byte by byte, and a `List[string]` as its
// length at `begin`, then each element's length and bytes.

/** The bytes a call's streamed arguments fill, by argument index. */
export interface ArgumentBuffers {
  readonly argumentBytes: ReadonlyMap<number, Uint8Array>;
  /** The UTF-8 bytes of each element of a `List[string]` argument. */
  readonly argumentLists: ReadonlyMap<number, Uint8Array[]>;
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
  return { argumentBytes, argumentLists };
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

/** Puts the text of each streamed argument into `arguments_`. */
export function decodeStreamedArguments(
  buffers: ArgumentBuffers,
  arguments_: unknown[],
  decoder: TextDecoder,
): void {
  for (const [index, bytes] of buffers.argumentBytes) arguments_[index] = decoder.decode(bytes);
  for (const [index, list] of buffers.argumentLists)
    arguments_[index] = list.map((bytes) => decoder.decode(bytes));
}
