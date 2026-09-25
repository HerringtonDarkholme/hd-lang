import type { HirProgram, ValueType } from "../hir.ts";
import { runtimePanicCode } from "../runtime-panic.ts";
import { mutableInner } from "../types.ts";
import {
  suspensionWrapperCancelAdapterName,
  suspensionWrapperPollAdapterName,
  suspensionWrapperResultAdapterName,
  traitSuspensionCancelName,
  traitSuspensionName,
  traitSuspensionPollName,
  traitSuspensionResultName,
  traitSuspensionWrapperCancelAdapterName,
  traitSuspensionWrapperPollAdapterName,
  traitSuspensionWrapperResultAdapterName,
} from "./shared.ts";

export const STORED_SUSPENSION_TYPES = `    (type $hd.suspension-poll-sig (func (param anyref) (result i32)))
    (type $hd.suspension-cancel-sig (func (param anyref)))
    (type $hd.suspension-result-sig (func (param anyref) (result anyref)))
    (type $hd.suspension (struct
      (field $hd.suspension-inner anyref)
      (field $hd.suspension-poll (ref $hd.suspension-poll-sig))
      (field $hd.suspension-cancel (ref $hd.suspension-cancel-sig))
      (field $hd.suspension-result (ref $hd.suspension-result-sig))))`;

export const STORED_SUSPENSION_RUNTIME = `(func $hd.suspension_poll (param $frame (ref null $hd.suspension)) (result i32)
  (call_ref $hd.suspension-poll-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-poll (local.get $frame))))

(func $hd.suspension_cancel (param $frame (ref null $hd.suspension))
  (call_ref $hd.suspension-cancel-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-cancel (local.get $frame))))

(func $hd.suspension_result (param $frame (ref null $hd.suspension)) (result anyref)
  (call_ref $hd.suspension-result-sig
    (struct.get $hd.suspension $hd.suspension-inner (local.get $frame))
    (struct.get $hd.suspension $hd.suspension-result (local.get $frame))))

(func $hd.suspension_drive (param $frame (ref null $hd.suspension)) (result anyref)
  (if (global.get $hd.driver-active)
    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-competing-driver")})) unreachable))
  (global.set $hd.driver-active (i32.const 1))
  (block $ready
    (loop $drive
      (br_if $ready (i32.eq (call $hd.suspension_poll (local.get $frame)) (i32.const 1)))
      (br $drive)))
  (global.set $hd.driver-active (i32.const 0))
  (call $hd.suspension_result (local.get $frame)))`;

function boxResult(value: string, type: ValueType): string {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return boxResult(value, mutable);
  if (type === "i32" || type === "bool" || type === "char")
    return `(struct.new $hd.box-i32 ${value})`;
  if (type === "f64") return `(struct.new $hd.box-f64 ${value})`;
  if (type === "void") return `(ref.null any)`;
  return value;
}

function suspensionIndex(declaration: HirProgram["functions"][number]): number {
  return declaration.suspensionIndex ?? declaration.index;
}

export function storedSuspensionAdapterReferences(program: HirProgram): readonly string[] {
  return [
    ...[...program.functions, ...program.closures]
      .filter((declaration) => declaration.suspending)
      .flatMap((declaration) => [
        suspensionWrapperPollAdapterName(suspensionIndex(declaration)),
        suspensionWrapperCancelAdapterName(suspensionIndex(declaration)),
        suspensionWrapperResultAdapterName(suspensionIndex(declaration)),
      ]),
    ...program.traits.flatMap((trait) =>
      trait.methods.flatMap((method) =>
        method.suspending
          ? [
              traitSuspensionWrapperPollAdapterName(trait.index, method.index),
              traitSuspensionWrapperCancelAdapterName(trait.index, method.index),
              traitSuspensionWrapperResultAdapterName(trait.index, method.index),
            ]
          : [],
      ),
    ),
  ];
}

export function emitStoredSuspensionAdapters(program: HirProgram): string {
  const functionAdapters = [...program.functions, ...program.closures]
    .filter((declaration) => declaration.suspending)
    .flatMap((declaration) => {
      const index = suspensionIndex(declaration);
      const inner = `(ref.cast (ref $s${index}) (local.get $inner))`;
      const result =
        declaration.result === "void"
          ? `(ref.null any)`
          : boxResult(`(struct.get $s${index} $s${index}result ${inner})`, declaration.result);
      return [
        `(func ${suspensionWrapperPollAdapterName(index)} (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)\n  (call $poll${index} ${inner}))`,
        `(func ${suspensionWrapperCancelAdapterName(index)} (type $hd.suspension-cancel-sig) (param $inner anyref)\n  (call $cancel${index} ${inner}))`,
        `(func ${suspensionWrapperResultAdapterName(index)} (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)\n  ${result})`,
      ];
    });
  const traitAdapters = program.traits.flatMap((trait) =>
    trait.methods.flatMap((method) => {
      if (!method.suspending) return [];
      const wrapper = traitSuspensionName(trait.index, method.index);
      const inner = `(ref.cast (ref ${wrapper}) (local.get $inner))`;
      const result = boxResult(
        method.result === "void"
          ? `(ref.null any)`
          : `(call ${traitSuspensionResultName(trait.index, method.index)} ${inner})`,
        method.result,
      );
      return [
        `(func ${traitSuspensionWrapperPollAdapterName(trait.index, method.index)} (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)\n  (call ${traitSuspensionPollName(trait.index, method.index)} ${inner}))`,
        `(func ${traitSuspensionWrapperCancelAdapterName(trait.index, method.index)} (type $hd.suspension-cancel-sig) (param $inner anyref)\n  (call ${traitSuspensionCancelName(trait.index, method.index)} ${inner}))`,
        `(func ${traitSuspensionWrapperResultAdapterName(trait.index, method.index)} (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)\n  ${result})`,
      ];
    }),
  );
  return [...functionAdapters, ...traitAdapters].join("\n\n");
}
