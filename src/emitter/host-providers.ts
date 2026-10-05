import {
  boundaryShape,
  isBoundaryScalar,
  payloadlessSingletonEnum,
  programLookups,
  isStringListArgument,
  resultSides,
  structuralHostArgument,
  structuralHostResult,
} from "../host-boundary.ts";
import type { HirEnum, HirProgram, HirTrait, HirTraitMethod, ValueType } from "../hir.ts";
import { numericType } from "../numeric.ts";
import { runtimePanicCode } from "../runtime-panic.ts";
import { substituteTypeParameters } from "../types.ts";
import { boxScalar, scalarWasm } from "./scalars.ts";
import { traitMethodKey } from "./reachability.ts";
import { isGenericValueType } from "./shared.ts";

interface HostProviderEmission {
  readonly functions: string;
  readonly imports: string;
  readonly references: readonly string[];
  readonly types: string;
}

interface HostMethod {
  readonly emitter: HostValueEmitter;
  /** The encoder function of each structural argument type (emitArgumentEncoders). */
  readonly encoders: ReadonlyMap<ValueType, string>;
  readonly enums: readonly HirEnum[];
  readonly method: HirTraitMethod;
  readonly program: HirProgram;
  readonly trait: HirTrait;
}

interface HostValueEmitter {
  boxErasedValue(value: string, type: ValueType): string;
  unboxErasedValue(payload: string, type: ValueType): string;
  defaultValue(type: ValueType): string;
  watType(type: ValueType): string;
}

/**
 * The `[T, E]` of a `Result` that crosses as a tag and a scalar payload;
 * undefined for any other type, a `Result` that crosses as a node tree among them.
 */
function scalarSides(
  program: HirProgram,
  type: ValueType,
): readonly [ValueType, ValueType] | undefined {
  return structuralHostResult(program, type) ? undefined : resultSides(type);
}

/** Whether `type`'s value crosses as UTF-8 bytes, alone or as a scalar result side. */
function crossesAsString(program: HirProgram, type: ValueType): boolean {
  return type === "string" || (scalarSides(program, type)?.includes("string") ?? false);
}

function boundaryWatType(type: ValueType): string {
  return numericType(type) ? scalarWasm(type) : "i32";
}

function methodName(trait: HirTrait, method: HirTraitMethod): string {
  return `$hd.host_trait${trait.index}_method${method.index}`;
}

function frameName(trait: HirTrait, method: HirTraitMethod): string {
  return `$hd.host_frame${trait.index}_${method.index}`;
}

function pollName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_poll`;
}

function cancelName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_cancel`;
}

function resultName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_result`;
}

function importName(
  trait: HirTrait,
  method: HirTraitMethod,
  operation:
    | "argument_byte"
    | "argument_element"
    | "argument_element_byte"
    | "begin"
    | "cancel"
    | "poll"
    | "result"
    | "result_byte"
    | "result_err"
    | "result_length"
    | "result_node"
    | "result_ok"
    | "result_tag",
): string {
  return `host_${trait.index}_${method.index}_${operation}`;
}

function stringResultName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_decode_string`;
}

function variantResultName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_decode_result`;
}

function structuralResultName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_decode_structural`;
}

function callField(trait: HirTrait, method: HirTraitMethod): string {
  const frame = frameName(trait, method);
  return `(struct.get ${frame} ${frame}call (local.get $frame))`;
}

function emitPoll({ program, trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const readyResult =
    method.result === "void"
      ? []
      : [
          `      (struct.set ${frame} ${frame}result (local.get $frame)`,
          `        ${
            method.result === "string"
              ? `(call ${stringResultName(trait, method)} ${callField(trait, method)})`
              : scalarSides(program, method.result)
                ? `(call ${variantResultName(trait, method)} ${callField(trait, method)})`
                : structuralHostResult(program, method.result)
                  ? `(call ${structuralResultName(trait, method)} (call $hd.${importName(trait, method, "result_node")} ${callField(trait, method)}))`
                  : `(call $hd.${importName(trait, method, "result")} ${callField(trait, method)})`
          })`,
        ];
  return [
    `(func ${pollName(trait, method)} (type $tspollsig${trait.index}_${method.index}) (param $inner anyref) (result i32)`,
    `  (local $frame (ref ${frame}))`,
    `  (local $ready i32)`,
    `  (local.set $frame (ref.cast (ref ${frame}) (local.get $inner)))`,
    `  (if (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 1))`,
    `    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-reentrant-poll")})) unreachable))`,
    `  (if (i32.or`,
    `        (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 2))`,
    `        (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 3)))`,
    `    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-invalid-state")})) unreachable))`,
    `  (struct.set ${frame} ${frame}state (local.get $frame) (i32.const 1))`,
    `  (local.set $ready (call $hd.${importName(trait, method, "poll")} ${callField(trait, method)}))`,
    `  (if (local.get $ready)`,
    `    (then`,
    ...readyResult,
    `      (struct.set ${frame} ${frame}state (local.get $frame) (i32.const 2)))`,
    `    (else`,
    `      (struct.set ${frame} ${frame}state (local.get $frame) (i32.const 4))))`,
    `  (local.get $ready)`,
    `)`,
  ].join("\n");
}

// A `Result[T, E]` boundary result: the host reports the tag, then the
// active side's payload, boxed as the erased variant payload.
function emitVariantResult({ enums, program, trait, method }: HostMethod): string {
  const sides = scalarSides(program, method.result);
  if (!sides) return "";
  const payload = (type: ValueType, side: "result_ok" | "result_err"): string => {
    const value = `(call $hd.${importName(trait, method, side)} (local.get $call))`;
    if (type === "string") return `(call ${stringResultName(trait, method)} (local.get $call))`;
    if (isBoundaryScalar(type)) return boxScalar(value, type);
    const singleton = payloadlessSingletonEnum(enums, type);
    if (singleton) return `(global.get $e${singleton.enumIndex}v${singleton.tag})`;
    return "(ref.null any)";
  };
  return [
    `(func ${variantResultName(trait, method)} (param $call externref) (result (ref null $hd.variant))`,
    `  (local $tag i32)`,
    `  (local.set $tag (call $hd.${importName(trait, method, "result_tag")} (local.get $call)))`,
    `  (struct.new $hd.variant (local.get $tag)`,
    `    (if (result anyref) (local.get $tag)`,
    `      (then ${payload(sides[1], "result_err")})`,
    `      (else ${payload(sides[0], "result_ok")})))`,
    `)`,
  ].join("\n");
}

function emitStringResult({ program, trait, method }: HostMethod): string {
  if (!crossesAsString(program, method.result)) return "";
  const lengthImport = importName(trait, method, "result_length");
  const byteImport = importName(trait, method, "result_byte");
  return [
    `(func ${stringResultName(trait, method)} (param $call externref) (result (ref null $hd.string))`,
    `  (local $length i32)`,
    `  (local $index i32)`,
    `  (local $result (ref $hd.bytes))`,
    `  (local.set $length (call $hd.${lengthImport} (local.get $call)))`,
    `  (local.set $result (array.new_default $hd.bytes (local.get $length)))`,
    `  (block $done`,
    `    (loop $copy`,
    `      (br_if $done (i32.ge_u (local.get $index) (local.get $length)))`,
    `      (array.set $hd.bytes (local.get $result) (local.get $index)`,
    `        (call $hd.${byteImport} (local.get $call) (local.get $index)))`,
    `      (local.set $index (i32.add (local.get $index) (i32.const 1)))`,
    `      (br $copy)))`,
    `  (struct.new $hd.string (local.get $result) (i32.const 0) (local.get $length))`,
    `)`,
  ].join("\n");
}

function emitStructuralResult(host: HostMethod): string {
  const { emitter, method, program, trait } = host;
  if (!structuralHostResult(program, method.result)) return "";
  const functions: string[] = [];
  const decoderByType = new Map<ValueType, string>();
  const name = (path: string): string => `${structuralResultName(trait, method)}_${path}`;
  const child = (node: string, index: string | number): string =>
    `(call $hd.host_boundary_child ${node} ${typeof index === "number" ? `(i32.const ${index})` : index})`;
  const decode = (type: ValueType, path: string): string => {
    const existing = decoderByType.get(type);
    if (existing) return existing;
    const functionName = name(path);
    // Register before descending so a recursive data shape calls back into
    // the decoder currently being emitted instead of expanding forever.
    decoderByType.set(type, functionName);
    const shape = boundaryShape(type, ...programLookups(program));
    if (shape.kind === "result") {
      // Tag 0 is `.Ok`, tag 1 `.Err`; the payload is the active side's value,
      // stored erased, as every `Result` payload is.
      const payload = (side: ValueType, sidePath: string): string =>
        side === "void"
          ? "(ref.null any)"
          : emitter.boxErasedValue(
              `(call ${decode(side, `${path}_${sidePath}`)} ${child("(local.get $node)", 0)})`,
              side,
            );
      functions.push(
        [
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})`,
          `  (local $tag i32)`,
          `  (local.set $tag (call $hd.host_boundary_tag (local.get $node)))`,
          `  (struct.new $hd.variant (local.get $tag)`,
          `    (if (result anyref) (local.get $tag)`,
          `      (then ${payload(shape.err, "err")})`,
          `      (else ${payload(shape.ok, "ok")})))`,
          `)`,
        ].join("\n"),
      );
      return functionName;
    }
    if (shape.kind === "enum") {
      // Each variant builds the enum's struct: its tag, its own fields from
      // the node's children, and a default in every other variant's fields.
      const enumeration = shape.declaration;
      const variantValue = (variant: (typeof enumeration.variants)[number]): string => {
        if (variant.fields.length === 0)
          return `(global.get $e${enumeration.index}v${variant.tag})`;
        const values = enumeration.fields.map((field) => {
          const position = variant.fields.findIndex(({ index }) => index === field.index);
          if (position === -1) return emitter.defaultValue(field.type);
          const decoder = decode(field.type, `${path}_${variant.tag}_${position}`);
          return `(call ${decoder} ${child("(local.get $node)", position)})`;
        });
        return `(struct.new $e${enumeration.index} (i32.const ${variant.tag}) ${values.join(" ")})`;
      };
      const branches = enumeration.variants.reduceRight(
        (otherwise, variant) =>
          `(if (result ${emitter.watType(type)}) (i32.eq (local.get $tag) (i32.const ${variant.tag}))\n    (then ${variantValue(variant)})\n    (else ${otherwise}))`,
        "(unreachable)",
      );
      functions.push(
        [
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})`,
          `  (local $tag i32)`,
          `  (local.set $tag (call $hd.host_boundary_tag (local.get $node)))`,
          `  ${branches}`,
          `)`,
        ].join("\n"),
      );
      return functionName;
    }
    if (shape.kind === "string") {
      functions.push(
        [
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})`,
          `  (local $length i32)`,
          `  (local $index i32)`,
          `  (local $bytes (ref $hd.bytes))`,
          `  (local.set $length (call $hd.host_boundary_string_length (local.get $node)))`,
          `  (local.set $bytes (array.new_default $hd.bytes (local.get $length)))`,
          `  (block $done`,
          `    (loop $copy`,
          `      (br_if $done (i32.ge_u (local.get $index) (local.get $length)))`,
          `      (array.set $hd.bytes (local.get $bytes) (local.get $index)`,
          `        (call $hd.host_boundary_string_byte (local.get $node) (local.get $index)))`,
          `      (local.set $index (i32.add (local.get $index) (i32.const 1)))`,
          `      (br $copy)))`,
          `  (struct.new $hd.string (local.get $bytes) (i32.const 0) (local.get $length))`,
          `)`,
        ].join("\n"),
      );
      return functionName;
    }
    if (shape.kind === "scalar") {
      const wasm = numericType(type)?.wasm ?? "i32";
      const accessor =
        wasm === "i64" ? "i64" : wasm === "f32" ? "f32" : wasm === "f64" ? "f64" : "i32";
      functions.push(
        `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})\n  (call $hd.host_boundary_${accessor} (local.get $node))\n)`,
      );
      return functionName;
    }
    if (shape.kind === "optional") {
      const valueDecoder = decode(shape.inner, `${path}_value`);
      const payload = emitter.boxErasedValue(
        `(call ${valueDecoder} ${child("(local.get $node)", 0)})`,
        shape.inner,
      );
      functions.push(
        [
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})`,
          `  (local $tag i32)`,
          `  (local.set $tag (call $hd.host_boundary_tag (local.get $node)))`,
          `  (struct.new $hd.variant (local.get $tag)`,
          `    (if (result anyref) (local.get $tag)`,
          `      (then ${payload})`,
          `      (else (ref.null any))))`,
          `)`,
        ].join("\n"),
      );
      return functionName;
    }
    if (shape.kind === "tuple") {
      const elements = shape.elements.map((element, index) => {
        const decoder = decode(element, `${path}_${index}`);
        return emitter.boxErasedValue(
          `(call ${decoder} ${child("(local.get $node)", index)})`,
          element,
        );
      });
      functions.push(
        `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})\n  (array.new_fixed $hd.list ${elements.length}${elements.length ? " " + elements.join(" ") : ""})\n)`,
      );
      return functionName;
    }
    if (shape.kind === "list") {
      const element = shape.element;
      const decoder = decode(element, `${path}_element`);
      const boxed = emitter.boxErasedValue(
        `(call ${decoder} ${child("(local.get $node)", "(local.get $index)")})`,
        element,
      );
      functions.push(
        [
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})`,
          `  (local $length i32)`,
          `  (local $index i32)`,
          `  (local $values (ref $hd.list))`,
          `  (local.set $length (call $hd.host_boundary_length (local.get $node)))`,
          `  (local.set $values (array.new_default $hd.list (local.get $length)))`,
          `  (block $done`,
          `    (loop $copy`,
          `      (br_if $done (i32.ge_u (local.get $index) (local.get $length)))`,
          `      (array.set $hd.list (local.get $values) (local.get $index) ${boxed})`,
          `      (local.set $index (i32.add (local.get $index) (i32.const 1)))`,
          `      (br $copy)))`,
          `  (struct.new $hd.vector (local.get $length) (local.get $values))`,
          `)`,
        ].join("\n"),
      );
      return functionName;
    }
    if (shape.kind === "data") {
      const data = shape.declaration;
      if (data.fields.length === 0) {
        functions.push(
          `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})\n  (drop (local.get $node))\n  (global.get $d${data.index}c)\n)`,
        );
        return functionName;
      }
      const substitutions = new Map(
        data.genericParameters.map(
          (parameter, index) => [parameter, shape.arguments[index]!] as const,
        ),
      );
      const fields = data.fields.map((field, index) => {
        const fieldType = substituteTypeParameters(field.type, substitutions);
        const decoder = decode(fieldType, `${path}_${index}`);
        const value = `(call ${decoder} ${child("(local.get $node)", index)})`;
        return isGenericValueType(field.type) ? emitter.boxErasedValue(value, fieldType) : value;
      });
      functions.push(
        `(func ${functionName} (param $node externref) (result ${emitter.watType(type)})\n  (struct.new $d${data.index} ${fields.join(" ")})\n)`,
      );
      return functionName;
    }
    throw new Error(`cannot emit a structural host result decoder for '${type}'`);
  };
  const root = decode(method.result, "root");
  functions.push(
    `(func ${structuralResultName(trait, method)} (param $node externref) (result ${emitter.watType(method.result)})\n  (call ${root} (local.get $node))\n)`,
  );
  return functions.join("\n\n");
}

function emitCancel({ trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  return [
    `(func ${cancelName(trait, method)} (type $tscancelsig${trait.index}_${method.index}) (param $inner anyref)`,
    `  (local $frame (ref ${frame}))`,
    `  (local.set $frame (ref.cast (ref ${frame}) (local.get $inner)))`,
    `  (if (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 1))`,
    `    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-reentrant-poll")})) unreachable))`,
    `  (if (i32.or`,
    `        (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 0))`,
    `        (i32.eq (struct.get ${frame} ${frame}state (local.get $frame)) (i32.const 4)))`,
    `    (then`,
    `      (call $hd.${importName(trait, method, "cancel")} ${callField(trait, method)})`,
    `      (struct.set ${frame} ${frame}state (local.get $frame) (i32.const 3))))`,
    `)`,
  ].join("\n");
}

function emitResult({ emitter, trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const result = method.result === "void" ? "" : ` (result ${emitter.watType(method.result)})`;
  const value =
    method.result === "void"
      ? ""
      : `  (struct.get ${frame} ${frame}result (ref.cast (ref ${frame}) (local.get $inner)))`;
  return [
    `(func ${resultName(trait, method)} (type $tsresultsig${trait.index}_${method.index}) (param $inner anyref)${result}`,
    value,
    `)`,
  ]
    .filter(Boolean)
    .join("\n");
}

function emitMethod({ emitter, encoders, program, trait, method }: HostMethod): string {
  const parameters = method.parameters.map(
    (parameter, index) => `(param $argument${index} ${emitter.watType(parameter)})`,
  );
  // A structural argument crosses after `begin`, as the token stream its
  // encoder writes (emitArgumentEncoders); `begin` receives a placeholder.
  const beginArguments = method.parameters.map((parameter, index) =>
    parameter === "string"
      ? `(struct.get $hd.string $hd.string-length (ref.as_non_null (local.get $argument${index})))`
      : isStringListArgument(parameter)
        ? `(struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get $argument${index})))`
        : structuralHostArgument(parameter)
          ? `(i32.const 0)`
          : `(local.get $argument${index})`,
  );
  const encodeArguments = method.parameters.flatMap((parameter, index) =>
    structuralHostArgument(parameter)
      ? [`  (call ${encoders.get(parameter)!} (local.get $call) (local.get $argument${index}))`]
      : [],
  );
  // A `List[string]` argument crosses as its length, then each element's
  // length and bytes (src/compiler.ts).
  const lists = method.parameters
    .map((parameter, index) => ({ index, parameter }))
    .filter(({ parameter }) => isStringListArgument(parameter));
  const element = (index: number): string =>
    `(ref.cast (ref $hd.string) (array.get $hd.list (struct.get $hd.vector $hd.vector-values (ref.as_non_null (local.get $argument${index}))) (local.get $element-index)))`;
  const streamLists = lists.flatMap(({ index }) => [
    `  (local.set $element-index (i32.const 0))`,
    `  (block $argument${index}-elements-done`,
    `    (loop $argument${index}-elements`,
    `      (br_if $argument${index}-elements-done`,
    `        (i32.ge_u (local.get $element-index)`,
    `          (struct.get $hd.vector $hd.vector-size (ref.as_non_null (local.get $argument${index})))))`,
    `      (local.set $element ${element(index)})`,
    `      (call $hd.${importName(trait, method, "argument_element")}`,
    `        (local.get $call) (i32.const ${index}) (local.get $element-index)`,
    `        (struct.get $hd.string $hd.string-length (local.get $element)))`,
    `      (local.set $byte-index (i32.const 0))`,
    `      (block $argument${index}-bytes-done`,
    `        (loop $argument${index}-bytes`,
    `          (br_if $argument${index}-bytes-done`,
    `            (i32.ge_u (local.get $byte-index) (struct.get $hd.string $hd.string-length (local.get $element))))`,
    `          (call $hd.${importName(trait, method, "argument_element_byte")}`,
    `            (local.get $call) (i32.const ${index}) (local.get $element-index) (local.get $byte-index)`,
    `            (call $hd.string_get (ref.as_non_null (local.get $element)) (local.get $byte-index)))`,
    `          (local.set $byte-index (i32.add (local.get $byte-index) (i32.const 1)))`,
    `          (br $argument${index}-bytes)))`,
    `      (local.set $element-index (i32.add (local.get $element-index) (i32.const 1)))`,
    `      (br $argument${index}-elements)))`,
  ]);
  const strings = method.parameters
    .map((parameter, index) => ({ index, parameter }))
    .filter(({ parameter }) => parameter === "string");
  const streamArguments = strings.flatMap(({ index }) => [
    `  (local.set $byte-index (i32.const 0))`,
    `  (block $argument${index}-done`,
    `    (loop $argument${index}-copy`,
    `      (br_if $argument${index}-done`,
    `        (i32.ge_u (local.get $byte-index)`,
    `          (struct.get $hd.string $hd.string-length (ref.as_non_null (local.get $argument${index})))))`,
    `      (call $hd.${importName(trait, method, "argument_byte")}`,
    `        (local.get $call) (i32.const ${index}) (local.get $byte-index)`,
    `        (call $hd.string_get (ref.as_non_null (local.get $argument${index}))`,
    `          (local.get $byte-index)))`,
    `      (local.set $byte-index (i32.add (local.get $byte-index) (i32.const 1)))`,
    `      (br $argument${index}-copy)))`,
  ]);
  const begin = [
    `  (local.set $call`,
    `    (call $hd.${importName(trait, method, "begin")}`,
    `      (struct.get $hd.box-extern $hd.box-extern-value (ref.cast (ref $hd.box-extern) (local.get $receiver)))`,
    `      (global.get $hd.host-call-function) (global.get $hd.host-call-site)${beginArguments.length ? " " + beginArguments.join(" ") : ""}))`,
    ...streamArguments,
    ...streamLists,
    ...encodeArguments,
  ];
  const locals = [
    `  (local $call externref)`,
    ...(strings.length > 0 || lists.length > 0 ? [`  (local $byte-index i32)`] : []),
    ...(lists.length > 0
      ? [`  (local $element-index i32)`, `  (local $element (ref null $hd.string))`]
      : []),
  ];
  const header = `(func ${methodName(trait, method)} (type $tsig${trait.index}_${method.index}) (param $receiver anyref) (param $dictionary anyref)${parameters.length ? " " + parameters.join(" ") : ""}`;
  // A plain method is answered at once: the host never leaves it pending
  // (src/compiler.ts), so it reads the result right after the one poll.
  if (!method.suspending) {
    const result = method.result === "void" ? "" : ` (result ${emitter.watType(method.result)})`;
    const value =
      method.result === "void"
        ? []
        : [
            `  ${
              method.result === "string"
                ? `(call ${stringResultName(trait, method)} (local.get $call))`
                : scalarSides(program, method.result)
                  ? `(call ${variantResultName(trait, method)} (local.get $call))`
                  : structuralHostResult(program, method.result)
                    ? `(call ${structuralResultName(trait, method)} (call $hd.${importName(trait, method, "result_node")} (local.get $call)))`
                    : `(call $hd.${importName(trait, method, "result")} (local.get $call))`
            }`,
          ];
    return [
      `${header}${result}`,
      ...locals,
      ...begin,
      `  (drop (call $hd.${importName(trait, method, "poll")} (local.get $call)))`,
      ...value,
      `)`,
    ].join("\n");
  }
  const defaultResult = method.result === "void" ? "" : ` ${emitter.defaultValue(method.result)}`;
  return [
    `${header} (result (ref null $ts${trait.index}_${method.index}))`,
    ...locals,
    ...begin,
    `  (struct.new $ts${trait.index}_${method.index}`,
    `    (struct.new ${frameName(trait, method)}`,
    `      (local.get $call)`,
    `      (i32.const 0)${defaultResult})`,
    `    (ref.func ${pollName(trait, method)})`,
    `    (ref.func ${cancelName(trait, method)})`,
    `    (ref.func ${resultName(trait, method)})`,
    `    (ref.null $hd.suspension-result-adapt-sig))`,
    `)`,
  ].join("\n");
}

const ARGUMENT_TOKENS = ["i32", "i64", "f32", "f64"] as const;

/**
 * The encoders of the structural arguments of `methods`: one function per
 * argument type, which writes the value as a stream of scalar tokens in
 * preorder. A tag precedes an optional's, a `Result`'s, or an enum's
 * payload, a length precedes a list's elements and a string's bytes, and a
 * data value or a tuple writes its fields in order. The host rebuilds the
 * value from the stream and the declared type (src/host-arguments.ts).
 */
function emitArgumentEncoders(
  program: HirProgram,
  methods: readonly { readonly method: HirTraitMethod }[],
  emitter: HostValueEmitter,
): { readonly functions: readonly string[]; readonly names: ReadonlyMap<ValueType, string> } {
  const functions: string[] = [];
  const names = new Map<ValueType, string>();
  const token = (wasm: string, value: string): string =>
    `(call $hd.host_argument_${wasm} (local.get $call) ${value})`;
  const encode = (type: ValueType): string => {
    const existing = names.get(type);
    if (existing) return existing;
    const name = `$hd.host_encode${names.size}`;
    // Register before descending, so a recursive shape calls back into the
    // encoder being emitted.
    names.set(type, name);
    const call = (inner: ValueType, value: string): string =>
      `(call ${encode(inner)} (local.get $call) ${value})`;
    const shape = boundaryShape(type, ...programLookups(program));
    const tagged = (sides: readonly ValueType[]): string[] => {
      const payload = `(struct.get $hd.variant $hd.variant-payload (local.get $value))`;
      return [
        `  (local $tag i32)`,
        `  (local.set $tag (struct.get $hd.variant $hd.variant-tag (local.get $value)))`,
        `  ${token("i32", "(local.get $tag)")}`,
        ...sides.flatMap((side, tag) =>
          side === "void"
            ? []
            : [
                `  (if (i32.eq (local.get $tag) (i32.const ${tag}))`,
                `    (then ${call(side, emitter.unboxErasedValue(payload, side))}))`,
              ],
        ),
      ];
    };
    const counted = (length: string, element: string): string[] => [
      `  (local $index i32)`,
      `  (local $length i32)`,
      `  (local.set $length ${length})`,
      `  ${token("i32", "(local.get $length)")}`,
      `  (block $done`,
      `    (loop $copy`,
      `      (br_if $done (i32.ge_u (local.get $index) (local.get $length)))`,
      `      ${element}`,
      `      (local.set $index (i32.add (local.get $index) (i32.const 1)))`,
      `      (br $copy)))`,
    ];
    let body: string[];
    switch (shape.kind) {
      case "scalar":
        body = [`  ${token(scalarWasm(type), "(local.get $value)")}`];
        break;
      case "string":
        body = counted(
          `(struct.get $hd.string $hd.string-length (ref.as_non_null (local.get $value)))`,
          token(
            "i32",
            "(call $hd.string_get (ref.as_non_null (local.get $value)) (local.get $index))",
          ),
        );
        break;
      case "optional":
        body = tagged(["void", shape.inner]);
        break;
      case "result":
        body = tagged([shape.ok, shape.err]);
        break;
      case "tuple":
        body = shape.elements.map((element, index) => {
          const value = `(array.get $hd.list (local.get $value) (i32.const ${index}))`;
          return `  ${call(element, emitter.unboxErasedValue(value, element))}`;
        });
        break;
      case "list": {
        const element = `(array.get $hd.list (struct.get $hd.vector $hd.vector-values (local.get $value)) (local.get $index))`;
        body = counted(
          `(struct.get $hd.vector $hd.vector-size (local.get $value))`,
          call(shape.element, emitter.unboxErasedValue(element, shape.element)),
        );
        break;
      }
      case "data": {
        const data = shape.declaration;
        const substitutions = new Map(
          data.genericParameters.map(
            (parameter, index) => [parameter, shape.arguments[index]!] as const,
          ),
        );
        body = data.fields.map((field, index) => {
          const fieldType = substituteTypeParameters(field.type, substitutions);
          const value = `(struct.get $d${data.index} $d${data.index}f${index} (local.get $value))`;
          const typed = isGenericValueType(field.type)
            ? emitter.unboxErasedValue(value, fieldType)
            : value;
          return `  ${call(fieldType, typed)}`;
        });
        break;
      }
      case "enum": {
        const enumeration = shape.declaration;
        const field = (index: number): string =>
          `(struct.get $e${enumeration.index} $e${enumeration.index}f${index} (local.get $value))`;
        body = [
          `  (local $tag i32)`,
          `  (local.set $tag (struct.get $e${enumeration.index} $e${enumeration.index}tag (local.get $value)))`,
          `  ${token("i32", "(local.get $tag)")}`,
          ...enumeration.variants
            .filter((variant) => variant.fields.length > 0)
            .flatMap((variant) => [
              `  (if (i32.eq (local.get $tag) (i32.const ${variant.tag}))`,
              `    (then`,
              ...variant.fields.map(
                (payload) => `      ${call(payload.type, field(payload.index))}`,
              ),
              `    ))`,
            ]),
        ];
        break;
      }
      case "other":
        throw new Error(`cannot emit a host argument encoder for '${type}'`);
    }
    functions.push(
      [
        `(func ${name} (param $call externref) (param $value ${emitter.watType(type)})`,
        ...body,
        `)`,
      ].join("\n"),
    );
    return name;
  };
  for (const { method } of methods)
    for (const parameter of method.parameters)
      if (structuralHostArgument(parameter)) encode(parameter);
  return { functions, names };
}

/**
 * A host dictionary keeps one typed slot per trait method. Leave an unreachable
 * slot null without importing or adapting a method that no live HIR call can
 * reach. If reachability is wrong, dispatch traps instead of silently invoking
 * a different host operation.
 */
function emitTraitFactory(trait: HirTrait, called: ReadonlySet<string>): string {
  return [
    `(func $hd.host_trait${trait.index} (param $provider externref) (result (ref null $trait${trait.index}))`,
    `  (struct.new $trait${trait.index}`,
    `    (struct.new $hd.box-extern (local.get $provider))`,
    `    (ref.null $hd.list)`,
    ...trait.methods
      .filter((method) => called.has(traitMethodKey(trait.index, method.index)))
      .map((method) => `    (ref.func ${methodName(trait, method)})`),
    ...trait.supertraits.map((supertrait) => `    (ref.null $trait${supertrait.traitIndex})`),
    `  )`,
    `)`,
  ].join("\n");
}

function emitImports({ program, trait, method }: HostMethod): readonly string[] {
  const parameters = method.parameters.map((parameter) => `(param ${boundaryWatType(parameter)})`);
  const argumentByteImport = method.parameters.includes("string")
    ? [
        `  (import "hd" "${importName(trait, method, "argument_byte")}" (func $hd.${importName(trait, method, "argument_byte")} (param externref i32 i32 i32)))`,
      ]
    : [];
  const listImports = method.parameters.some(isStringListArgument)
    ? [
        `  (import "hd" "${importName(trait, method, "argument_element")}" (func $hd.${importName(trait, method, "argument_element")} (param externref i32 i32 i32)))`,
        `  (import "hd" "${importName(trait, method, "argument_element_byte")}" (func $hd.${importName(trait, method, "argument_element_byte")} (param externref i32 i32 i32 i32)))`,
      ]
    : [];
  const scalarImport = (
    operation: "result" | "result_err" | "result_ok" | "result_tag",
    type: ValueType,
  ): string =>
    `  (import "hd" "${importName(trait, method, operation)}" (func $hd.${importName(trait, method, operation)} (param externref) (result ${boundaryWatType(type)})))`;
  const sides = scalarSides(program, method.result);
  const structural = structuralHostResult(program, method.result);
  const resultImport = [
    ...(crossesAsString(program, method.result)
      ? [
          `  (import "hd" "${importName(trait, method, "result_length")}" (func $hd.${importName(trait, method, "result_length")} (param externref) (result i32)))`,
          `  (import "hd" "${importName(trait, method, "result_byte")}" (func $hd.${importName(trait, method, "result_byte")} (param externref i32) (result i32)))`,
        ]
      : []),
    ...(structural
      ? [
          `  (import "hd" "${importName(trait, method, "result_node")}" (func $hd.${importName(trait, method, "result_node")} (param externref) (result externref)))`,
        ]
      : sides
        ? [
            scalarImport("result_tag", "i32"),
            ...(isBoundaryScalar(sides[0]) ? [scalarImport("result_ok", sides[0])] : []),
            ...(isBoundaryScalar(sides[1]) ? [scalarImport("result_err", sides[1])] : []),
          ]
        : method.result === "void" || method.result === "string"
          ? []
          : [scalarImport("result", method.result)]),
  ];
  return [
    `  (import "hd" "${importName(trait, method, "begin")}" (func $hd.${importName(trait, method, "begin")} (param externref i32 i32)${parameters.length ? " " + parameters.join(" ") : ""} (result externref)))`,
    `  (import "hd" "${importName(trait, method, "poll")}" (func $hd.${importName(trait, method, "poll")} (param externref) (result i32)))`,
    `  (import "hd" "${importName(trait, method, "cancel")}" (func $hd.${importName(trait, method, "cancel")} (param externref)))`,
    ...argumentByteImport,
    ...listImports,
    ...resultImport,
  ];
}

function emitFrameType({ emitter, trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const result =
    method.result === "void"
      ? ""
      : `\n      (field ${frame}result (mut ${emitter.watType(method.result)}))`;
  return `    (type ${frame} (struct
      (field ${frame}call externref)
      (field ${frame}state (mut i32))${result}))`;
}

/**
 * Host-bound capability traits, the prelude `Console` among them
 * (spec/lang/10-modules.md#console): each method call goes out through its own
 * generic imports, and the host answers through one callback keyed by trait
 * and method name (src/compiler.ts, src/host-functions.ts).
 */
export function emitHostProviders(
  program: HirProgram,
  called: ReadonlySet<string>,
  emitter: HostValueEmitter,
): HostProviderEmission {
  const capabilities = new Set(program.hostCapabilities);
  const traits = program.traits.filter((trait) => capabilities.has(trait.name));
  if (traits.every((trait) => trait.methods.length === 0))
    return { functions: "", imports: "", references: [], types: "" };
  const live = traits.flatMap((trait) =>
    trait.methods
      .filter((method) => called.has(traitMethodKey(trait.index, method.index)))
      .map((method) => ({ trait, method })),
  );
  const encoders = emitArgumentEncoders(program, live, emitter);
  const liveMethods = live.map(({ trait, method }) => ({
    emitter,
    encoders: encoders.names,
    enums: program.enums,
    program,
    trait,
    method,
  }));
  const structural = liveMethods.some(({ method }) => structuralHostResult(program, method.result));
  const boundaryImports = structural
    ? [
        `  (import "hd" "host_boundary_tag" (func $hd.host_boundary_tag (param externref) (result i32)))`,
        `  (import "hd" "host_boundary_length" (func $hd.host_boundary_length (param externref) (result i32)))`,
        `  (import "hd" "host_boundary_child" (func $hd.host_boundary_child (param externref i32) (result externref)))`,
        `  (import "hd" "host_boundary_i32" (func $hd.host_boundary_i32 (param externref) (result i32)))`,
        `  (import "hd" "host_boundary_i64" (func $hd.host_boundary_i64 (param externref) (result i64)))`,
        `  (import "hd" "host_boundary_f32" (func $hd.host_boundary_f32 (param externref) (result f32)))`,
        `  (import "hd" "host_boundary_f64" (func $hd.host_boundary_f64 (param externref) (result f64)))`,
        `  (import "hd" "host_boundary_string_length" (func $hd.host_boundary_string_length (param externref) (result i32)))`,
        `  (import "hd" "host_boundary_string_byte" (func $hd.host_boundary_string_byte (param externref i32) (result i32)))`,
      ]
    : [];
  const argumentImports =
    encoders.names.size > 0
      ? ARGUMENT_TOKENS.map(
          (wasm) =>
            `  (import "hd" "host_argument_${wasm}" (func $hd.host_argument_${wasm} (param externref ${wasm})))`,
        )
      : [];
  const imports = [
    ...boundaryImports,
    ...argumentImports,
    ...liveMethods.flatMap(emitImports),
  ].join("\n");
  // A suspending method returns a frame that the caller polls; a plain one
  // needs only the method itself and its result decoders.
  const suspending = liveMethods.filter(({ method }) => method.suspending);
  const functions = [
    ...encoders.functions,
    ...liveMethods.flatMap((hostMethod) => [
      ...(hostMethod.method.suspending
        ? [emitPoll(hostMethod), emitCancel(hostMethod), emitResult(hostMethod)]
        : []),
      emitStringResult(hostMethod),
      emitVariantResult(hostMethod),
      emitStructuralResult(hostMethod),
      emitMethod(hostMethod),
    ]),
    ...traits.map((trait) => emitTraitFactory(trait, called)),
  ].join("\n\n");
  const references = liveMethods.flatMap(({ trait, method }) => [
    methodName(trait, method),
    ...(method.suspending
      ? [pollName(trait, method), cancelName(trait, method), resultName(trait, method)]
      : []),
  ]);
  const types = suspending.map(emitFrameType).filter(Boolean).join("\n");
  return { functions, imports, references, types };
}
