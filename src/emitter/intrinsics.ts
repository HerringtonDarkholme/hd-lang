import type { HirFunction, HirProgram, ValueType } from "../hir.ts";
import { localName } from "./shared.ts";
import { numericType } from "../numeric.ts";

// The compiler/library boundary (src/README.md#compilerlibrary-boundary).
// `lib/std` declares each primitive as an ordinary function with a std-only
// `@intrinsic("name")` line, and calls reach it as ordinary calls. Only its
// body comes from here:
//
// - a runtime primitive is a few instructions over the Wasm runtime's own
//   representation (`$hd.string` for `string`);
// - every other name is a host function. It is imported as `host:<name>`
//   through one generic path: scalar arguments and results cross as Wasm
//   numbers, and a `string` crosses as a host handle that `boundary.wat`
//   copies byte by byte. The host looks the name up in its function table
//   (src/host-functions.ts).

type Unbox = (value: string, type: ValueType) => string;

const RUNTIME_PRIMITIVES: Readonly<
  Record<string, (arguments_: readonly string[], unbox: (value: string) => string) => string>
> = {
  // The string primitives (spec/std/README.md#standard-library-primitives)
  // over `$hd.string`, a window on an immutable byte array.
  bytes_len: ([text]) => `(struct.get $hd.string $hd.string-length (ref.as_non_null ${text}))`,
  bytes_at: ([text, index]) => `(call $hd.string_get (ref.as_non_null ${text}) ${index})`,
  // The new window shares the bytes; std checks the offsets first, so a bad
  // one here is an index-out-of-bounds panic.
  bytes_slice: ([text, start, end]) =>
    [
      `(if (i32.or (i32.gt_u ${start} ${end}) (i32.gt_u ${end} (struct.get $hd.string $hd.string-length (ref.as_non_null ${text}))))`,
      `  (then (call $hd.panic (global.get $hd.panic-index-out-of-bounds)) unreachable))`,
      `(struct.new $hd.string`,
      `  (struct.get $hd.string $hd.string-bytes (ref.as_non_null ${text}))`,
      `  (i32.add (struct.get $hd.string $hd.string-start (ref.as_non_null ${text})) ${start})`,
      `  (i32.sub ${end} ${start}))`,
    ].join("\n"),
  bytes_concat: ([left, right]) =>
    `(call $hd.bytes_concat (ref.as_non_null ${left}) (ref.as_non_null ${right}))`,
  string_from_bytes: ([items]) => `(call $hd.string_from_bytes (ref.as_non_null ${items}))`,
  // A `char` is its scalar value at run time, both ways. A surrogate or a
  // value above 0x10FFFF has no `char`, so it gives the absent optional.
  char_from_scalar: ([point]) =>
    [
      `(if (result (ref $hd.variant))`,
      `  (i32.or (i32.lt_u (i32.sub ${point} (i32.const 0xD800)) (i32.const 0x800)) (i32.gt_u ${point} (i32.const 0x10FFFF)))`,
      `  (then (struct.new $hd.variant (i32.const 0) (ref.null any)))`,
      `  (else (struct.new $hd.variant (i32.const 1) (struct.new $hd.box-i32 ${point}))))`,
    ].join("\n"),
  char_scalar: ([value]) => value!,
  // `dbg`'s own body prints nothing, as a fetched dependency's call must
  // (spec/lang/10-modules.md#r-module.dbg.dependency). In the user's own code
  // the checker supplies the printing body, which needs each argument's type
  // and source text (checker/debug-print.ts).
  dbg: () => "(nop)",
  // Shrinks a list to `len` elements; std checks the bound first.
  list_truncate: ([items, length]) =>
    `(call $hd.vector_truncate (ref.as_non_null ${items}) ${length})`,
  // The polling frames of `race!` and `all!`
  // (11-requirements-and-suspension.md#r-req.combinator.intrinsic), whose
  // runtime is `$hd.combinator` (stored-suspension.ts).
  task_race_frame: ([tasks]) => `(call $hd.combinator_new (i32.const 1) ${tasks})`,
  task_all_frame: ([tasks]) => `(call $hd.combinator_new (i32.const 0) ${tasks})`,
};

/**
 * Intrinsics whose `lib/std` body never runs: `facts_of`, which the checker
 * lowers at every call (14-annotations.md#function-facts).
 */
const UNEMITTED_INTRINSICS: ReadonlySet<string> = new Set(["facts_of"]);

/** Whether `name` is a runtime primitive rather than a host function. */
export function isRuntimePrimitive(name: string): boolean {
  return Object.hasOwn(RUNTIME_PRIMITIVES, name) || UNEMITTED_INTRINSICS.has(name);
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
  // The checker rejects every call of one when emitting, so the body never runs.
  if (UNEMITTED_INTRINSICS.has(name)) return "unreachable";
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
  const panicDetail = program.hasPanicDetail;
  // An entry point's or test case's `.Err` report (checker/entry-error.ts)
  // crosses as a string.
  const entryError = program.functions.some(
    (declaration) => declaration.entryErrorRenderer !== undefined,
  );
  const imports = functions.map(
    (declaration) =>
      `  (import "hd" "host:${declaration.intrinsic}" (func ${hostImportName(declaration.intrinsic!)} ${hostSignature(declaration)}))`,
  );
  const boundary =
    panicDetail ||
    entryError ||
    functions.some(
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
  if (panicDetail)
    imports.push(
      `  (import "hd" "panic_with_message" (func $hd.panic_with_message (param i32 externref)))`,
    );
  if (entryError)
    imports.push(`  (import "hd" "entry_error" (func $hd.entry_error (param externref)))`);
  return { imports: imports.join("\n"), boundary };
}
