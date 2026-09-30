import type { HirFunction, HirProgram, ValueType } from "../hir.ts";
import { localName } from "./shared.ts";
import { numericType } from "../numeric.ts";

// The compiler/library boundary (src/README.md#compilerlibrary-boundary).
// `lib/std` declares each primitive as an ordinary function with a std-only
// `@intrinsic("name")` line, and calls reach it as ordinary calls. Only its
// body comes from here:
//
// - a runtime primitive is a few instructions over the Wasm runtime's own
//   representation (`$hd.bytes` for `string`);
// - every other name is a host function. It is imported as `host:<name>`
//   through one generic path: scalar arguments and results cross as Wasm
//   numbers, and a `string` crosses as a host handle that `boundary.wat`
//   copies byte by byte. The host looks the name up in its function table
//   (src/host-functions.ts).

type Unbox = (value: string, type: ValueType) => string;

const RUNTIME_PRIMITIVES: Readonly<
  Record<string, (arguments_: readonly string[], unbox: (value: string) => string) => string>
> = {
  string_byte_len: ([text]) => `(array.len (ref.as_non_null ${text}))`,
  string_byte_at: ([text, index]) => `(array.get_u $hd.bytes (ref.as_non_null ${text}) ${index})`,
  string_byte_slice: ([text, start, end]) =>
    `(call $hd.string_slice (ref.as_non_null ${text}) ${start} ${end})`,
  // A `char` is its scalar value at run time.
  char_from_scalar: ([point]) => point!,
  index_out_of_bounds: () =>
    `(call $hd.panic (global.get $hd.panic-index-out-of-bounds))\nunreachable`,
};

/** Whether `name` is a runtime primitive rather than a host function. */
export function isRuntimePrimitive(name: string): boolean {
  return Object.hasOwn(RUNTIME_PRIMITIVES, name);
}

/** The Wasm type a boundary value crosses as. */
function boundaryWatType(type: ValueType): string | undefined {
  if (type === "string") return "externref";
  const numeric = numericType(type);
  if (numeric) return numeric.wasm;
  if (["bool", "char"].includes(type)) return "i32";
  return undefined;
}

function hostImportName(name: string): string {
  return `$host.${name}`;
}

function hostFunctions(program: HirProgram): readonly HirFunction[] {
  const seen = new Set<string>();
  return program.functions.filter((declaration) => {
    const name = declaration.intrinsic;
    if (!name || isRuntimePrimitive(name) || seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

function hostSignature(declaration: HirFunction): string {
  const parameters = declaration.parameters.map((parameter) => {
    const type = boundaryWatType(parameter.type);
    if (!type)
      throw new Error(
        `host function '${declaration.intrinsic}' has a non-boundary parameter '${parameter.type}'`,
      );
    return `(param ${type})`;
  });
  const resultType =
    declaration.result === "void" || declaration.result === "never"
      ? undefined
      : boundaryWatType(declaration.result);
  if (declaration.result !== "void" && declaration.result !== "never" && !resultType)
    throw new Error(
      `host function '${declaration.intrinsic}' has a non-boundary result '${declaration.result}'`,
    );
  return `${parameters.join(" ")}${resultType ? ` (result ${resultType})` : ""}`;
}

/** The body of a `lib/std` primitive. */
export function emitIntrinsicBody(declaration: HirFunction, unbox: Unbox): string {
  const name = declaration.intrinsic!;
  const arguments_ = declaration.parameters.map(
    (parameter) => `(local.get ${localName(parameter.index)})`,
  );
  const primitive = RUNTIME_PRIMITIVES[name];
  if (primitive) return primitive(arguments_, (value) => unbox(value, declaration.result));
  const lowered = declaration.parameters.map((parameter, index) =>
    parameter.type === "string"
      ? `(call $hd.string_to_host ${arguments_[index]})`
      : arguments_[index]!,
  );
  const call = `(call ${hostImportName(name)}${lowered.length ? " " + lowered.join(" ") : ""})`;
  if (declaration.result === "never") return `${call}\nunreachable`;
  return declaration.result === "string" ? `(call $hd.string_from_host ${call})` : call;
}

/** The generic host-function imports and whether the string boundary runtime is needed. */
export function emitHostFunctionImports(program: HirProgram): {
  readonly imports: string;
  readonly boundary: boolean;
} {
  const functions = hostFunctions(program);
  const imports = functions.map(
    (declaration) =>
      `  (import "hd" "host:${declaration.intrinsic}" (func ${hostImportName(declaration.intrinsic!)} ${hostSignature(declaration)}))`,
  );
  const boundary = functions.some(
    (declaration) =>
      declaration.result === "string" ||
      declaration.parameters.some((parameter) => parameter.type === "string"),
  );
  if (boundary)
    imports.push(
      `  (import "hd" "host_string_new" (func $hd.host_string_new (param i32) (result externref)))`,
      `  (import "hd" "host_string_set" (func $hd.host_string_set (param externref i32 i32)))`,
      `  (import "hd" "host_string_length" (func $hd.host_string_length (param externref) (result i32)))`,
      `  (import "hd" "host_string_get" (func $hd.host_string_get (param externref i32) (result i32)))`,
    );
  return { imports: imports.join("\n"), boundary };
}
