import type { HirProgram, HirTrait, HirTraitMethod, ValueType } from "../hir.ts";
import { runtimePanicCode } from "../runtime-panic.ts";

export interface HostProviderEmission {
  readonly functions: string;
  readonly imports: string;
  readonly references: readonly string[];
  readonly types: string;
}

interface HostMethod {
  readonly method: HirTraitMethod;
  readonly trait: HirTrait;
}

function watType(type: ValueType): string {
  if (type === "f64") return "f64";
  if (type === "string") return "(ref null $hd.bytes)";
  return "i32";
}

function boundaryWatType(type: ValueType): string {
  return type === "f64" ? "f64" : "i32";
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
    | "begin"
    | "cancel"
    | "poll"
    | "result"
    | "result_byte"
    | "result_length",
): string {
  return `host_${trait.index}_${method.index}_${operation}`;
}

function stringResultName(trait: HirTrait, method: HirTraitMethod): string {
  return `${methodName(trait, method)}_decode_string`;
}

function callField(trait: HirTrait, method: HirTraitMethod): string {
  const frame = frameName(trait, method);
  return `(struct.get ${frame} ${frame}call (local.get $frame))`;
}

function emitPoll({ trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const readyResult =
    method.result === "void"
      ? []
      : [
          `      (struct.set ${frame} ${frame}result (local.get $frame)`,
          `        ${
            method.result === "string"
              ? `(call ${stringResultName(trait, method)} ${callField(trait, method)})`
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
    `    (else (struct.set ${frame} ${frame}state (local.get $frame) (i32.const 4))))`,
    `  (local.get $ready)`,
    `)`,
  ].join("\n");
}

function emitStringResult({ trait, method }: HostMethod): string {
  if (method.result !== "string") return "";
  const lengthImport = importName(trait, method, "result_length");
  const byteImport = importName(trait, method, "result_byte");
  return [
    `(func ${stringResultName(trait, method)} (param $call externref) (result (ref null $hd.bytes))`,
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
    `  (local.get $result)`,
    `)`,
  ].join("\n");
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

function emitResult({ trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const result = method.result === "void" ? "" : ` (result ${watType(method.result)})`;
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

function emitMethod({ trait, method }: HostMethod): string {
  const parameters = method.parameters.map(
    (parameter, index) => `(param $argument${index} ${watType(parameter)})`,
  );
  const beginArguments = method.parameters.map((parameter, index) =>
    parameter === "string"
      ? `(array.len (ref.as_non_null (local.get $argument${index})))`
      : `(local.get $argument${index})`,
  );
  const strings = method.parameters
    .map((parameter, index) => ({ index, parameter }))
    .filter(({ parameter }) => parameter === "string");
  const streamArguments = strings.flatMap(({ index }) => [
    `  (local.set $byte-index (i32.const 0))`,
    `  (block $argument${index}-done`,
    `    (loop $argument${index}-copy`,
    `      (br_if $argument${index}-done`,
    `        (i32.ge_u (local.get $byte-index)`,
    `          (array.len (ref.as_non_null (local.get $argument${index})))))`,
    `      (call $hd.${importName(trait, method, "argument_byte")}`,
    `        (local.get $call) (i32.const ${index}) (local.get $byte-index)`,
    `        (array.get_u $hd.bytes (ref.as_non_null (local.get $argument${index}))`,
    `          (local.get $byte-index)))`,
    `      (local.set $byte-index (i32.add (local.get $byte-index) (i32.const 1)))`,
    `      (br $argument${index}-copy)))`,
  ]);
  const defaultResult =
    method.result === "void"
      ? ""
      : method.result === "f64"
        ? " (f64.const 0)"
        : method.result === "string"
          ? " (ref.null $hd.bytes)"
          : " (i32.const 0)";
  return [
    `(func ${methodName(trait, method)} (type $tsig${trait.index}_${method.index}) (param $receiver anyref) (param $dictionary anyref)${parameters.length ? " " + parameters.join(" ") : ""} (result (ref null $ts${trait.index}_${method.index}))`,
    `  (local $call externref)`,
    ...(strings.length > 0 ? [`  (local $byte-index i32)`] : []),
    `  (local.set $call`,
    `    (call $hd.${importName(trait, method, "begin")}`,
    `      (struct.get $hd.box-extern $hd.box-extern-value (ref.cast (ref $hd.box-extern) (local.get $receiver)))`,
    `      (global.get $hd.host-call-site)${beginArguments.length ? " " + beginArguments.join(" ") : ""}))`,
    ...streamArguments,
    `  (struct.new $ts${trait.index}_${method.index}`,
    `    (struct.new ${frameName(trait, method)}`,
    `      (local.get $call)`,
    `      (i32.const 0)${defaultResult})`,
    `    (ref.func ${pollName(trait, method)})`,
    `    (ref.func ${cancelName(trait, method)})`,
    `    (ref.func ${resultName(trait, method)}))`,
    `)`,
  ].join("\n");
}

function emitTraitFactory(trait: HirTrait): string {
  return [
    `(func $hd.host_trait${trait.index} (param $provider externref) (result (ref null $trait${trait.index}))`,
    `  (struct.new $trait${trait.index}`,
    `    (struct.new $hd.box-extern (local.get $provider))`,
    `    (ref.null $hd.list)`,
    ...trait.methods.map((method) => `    (ref.func ${methodName(trait, method)})`),
    ...trait.supertraits.map((supertrait) => `    (ref.null $trait${supertrait.traitIndex})`),
    `  )`,
    `)`,
  ].join("\n");
}

function emitImports({ trait, method }: HostMethod): readonly string[] {
  const parameters = method.parameters.map((parameter) => `(param ${boundaryWatType(parameter)})`);
  const argumentByteImport = method.parameters.includes("string")
    ? [
        `  (import "hd" "${importName(trait, method, "argument_byte")}" (func $hd.${importName(trait, method, "argument_byte")} (param externref i32 i32 i32)))`,
      ]
    : [];
  const resultImport =
    method.result === "void"
      ? []
      : method.result === "string"
        ? [
            `  (import "hd" "${importName(trait, method, "result_length")}" (func $hd.${importName(trait, method, "result_length")} (param externref) (result i32)))`,
            `  (import "hd" "${importName(trait, method, "result_byte")}" (func $hd.${importName(trait, method, "result_byte")} (param externref i32) (result i32)))`,
          ]
        : [
            `  (import "hd" "${importName(trait, method, "result")}" (func $hd.${importName(trait, method, "result")} (param externref) (result ${boundaryWatType(method.result)})))`,
          ];
  return [
    `  (import "hd" "${importName(trait, method, "begin")}" (func $hd.${importName(trait, method, "begin")} (param externref i32)${parameters.length ? " " + parameters.join(" ") : ""} (result externref)))`,
    `  (import "hd" "${importName(trait, method, "poll")}" (func $hd.${importName(trait, method, "poll")} (param externref) (result i32)))`,
    `  (import "hd" "${importName(trait, method, "cancel")}" (func $hd.${importName(trait, method, "cancel")} (param externref)))`,
    ...argumentByteImport,
    ...resultImport,
  ];
}

function emitFrameType({ trait, method }: HostMethod): string {
  const frame = frameName(trait, method);
  const result =
    method.result === "void"
      ? ""
      : `\n      (field ${frame}result (mut ${watType(method.result)}))`;
  return `    (type ${frame} (struct
      (field ${frame}call externref)
      (field ${frame}state (mut i32))${result}))`;
}

export function emitHostProviders(program: HirProgram): HostProviderEmission {
  const capabilities = new Set(program.hostCapabilities);
  const traits = program.traits.filter((trait) => capabilities.has(trait.name));
  const methods = traits.flatMap((trait) => trait.methods.map((method) => ({ trait, method })));
  if (methods.length === 0) return { functions: "", imports: "", references: [], types: "" };
  const imports = methods.flatMap(emitImports).join("\n");
  const functions = [
    ...methods.flatMap((hostMethod) => [
      emitPoll(hostMethod),
      emitCancel(hostMethod),
      emitStringResult(hostMethod),
      emitResult(hostMethod),
      emitMethod(hostMethod),
    ]),
    ...traits.map(emitTraitFactory),
  ].join("\n\n");
  const references = methods.flatMap(({ trait, method }) => [
    methodName(trait, method),
    pollName(trait, method),
    cancelName(trait, method),
    resultName(trait, method),
  ]);
  const types = methods.map(emitFrameType).join("\n");
  return { functions, imports, references, types };
}
