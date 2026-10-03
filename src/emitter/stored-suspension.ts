import type { HirProgram, ValueType } from "../hir.ts";
import { runtimePanicCode } from "../runtime-panic.ts";
import { mutableInner } from "../types.ts";
import { boxScalar } from "./scalars.ts";
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
    (type $hd.suspension-result-adapt-sig (func (param anyref) (result anyref)))
    (type $hd.suspension (struct
      (field $hd.suspension-inner anyref)
      (field $hd.suspension-poll (ref $hd.suspension-poll-sig))
      (field $hd.suspension-cancel (ref $hd.suspension-cancel-sig))
      (field $hd.suspension-result (ref $hd.suspension-result-sig))))
    (type $hd.combinator (struct
      (field $hd.combinator-race i32)
      (field $hd.combinator-tasks (ref $hd.list))
      (field $hd.combinator-results (ref $hd.list))
      (field $hd.combinator-done (ref $hd.bytes))
      (field $hd.combinator-state (mut i32))))`;

/** The polling frame of `race!` and `all!`, and its stored-suspension adapters. */
const COMBINATOR_FUNCTIONS = [
  "$hd.combinator_poll",
  "$hd.combinator_cancel",
  "$hd.combinator_result",
] as const;

// The polling frame of the `std.task` combinators
// (11-requirements-and-suspension.md#standard-combinators), a stored
// suspension over the list of tasks. Its state is 0 between polls, 1 while
// polling, 2 when complete, and 3 when cancelled. `all!` (race 0) polls,
// in order, the tasks not yet done, keeps each result, and completes with
// the results as a tuple once every task is done
// (req.schedule.all-unfinished, req.schedule.all-completed). `race!`
// (race 1) completes with the first result, after cancelling every other
// task in order (req.combinator.race-losers). Cancelling the frame cancels
// every unfinished task (req.combinator.cancel-children).
const COMBINATOR_RUNTIME = `(func $hd.combinator_new (param $race i32) (param $tasks (ref null $hd.vector)) (result (ref null $hd.suspension))
  (local $size i32)
  (local $children (ref $hd.list))
  (local.set $size (struct.get $hd.vector $hd.vector-size (local.get $tasks)))
  (local.set $children (array.new $hd.list (ref.null any) (local.get $size)))
  (array.copy $hd.list $hd.list (local.get $children) (i32.const 0)
    (struct.get $hd.vector $hd.vector-values (local.get $tasks)) (i32.const 0) (local.get $size))
  (struct.new $hd.suspension
    (struct.new $hd.combinator
      (local.get $race)
      (local.get $children)
      (array.new $hd.list (ref.null any) (select (i32.const 1) (local.get $size) (local.get $race)))
      (array.new_default $hd.bytes (local.get $size))
      (i32.const 0))
    (ref.func $hd.combinator_poll)
    (ref.func $hd.combinator_cancel)
    (ref.func $hd.combinator_result)))

(func $hd.combinator_task (param $frame (ref $hd.combinator)) (param $index i32) (result (ref null $hd.suspension))
  (ref.cast (ref null $hd.suspension)
    (array.get $hd.list (struct.get $hd.combinator $hd.combinator-tasks (local.get $frame)) (local.get $index))))

(func $hd.combinator_poll (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)
  (local $frame (ref $hd.combinator))
  (local $index i32)
  (local $other i32)
  (local $count i32)
  (local $waiting i32)
  (local $task (ref null $hd.suspension))
  (local.set $frame (ref.cast (ref $hd.combinator) (local.get $inner)))
  (if (i32.eq (struct.get $hd.combinator $hd.combinator-state (local.get $frame)) (i32.const 1))
    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-reentrant-poll")})) unreachable))
  (if (i32.ge_u (struct.get $hd.combinator $hd.combinator-state (local.get $frame)) (i32.const 2))
    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-invalid-state")})) unreachable))
  (struct.set $hd.combinator $hd.combinator-state (local.get $frame) (i32.const 1))
  (local.set $count (array.len (struct.get $hd.combinator $hd.combinator-tasks (local.get $frame))))
  (block $polled
    (loop $each
      (br_if $polled (i32.ge_u (local.get $index) (local.get $count)))
      (if (i32.eqz (array.get_u $hd.bytes (struct.get $hd.combinator $hd.combinator-done (local.get $frame)) (local.get $index)))
        (then
          (local.set $task (call $hd.combinator_task (local.get $frame) (local.get $index)))
          (if (i32.eq (call $hd.suspension_poll (local.get $task)) (i32.const 1))
            (then
              (array.set $hd.bytes (struct.get $hd.combinator $hd.combinator-done (local.get $frame)) (local.get $index) (i32.const 1))
              (if (struct.get $hd.combinator $hd.combinator-race (local.get $frame))
                (then
                  (array.set $hd.list (struct.get $hd.combinator $hd.combinator-results (local.get $frame))
                    (i32.const 0) (call $hd.suspension_result (local.get $task)))
                  (block $cancelled
                    (loop $cancel
                      (br_if $cancelled (i32.ge_u (local.get $other) (local.get $count)))
                      (if (i32.ne (local.get $other) (local.get $index))
                        (then (call $hd.suspension_cancel (call $hd.combinator_task (local.get $frame) (local.get $other)))))
                      (local.set $other (i32.add (local.get $other) (i32.const 1)))
                      (br $cancel)))
                  (struct.set $hd.combinator $hd.combinator-state (local.get $frame) (i32.const 2))
                  (return (i32.const 1))))
              (array.set $hd.list (struct.get $hd.combinator $hd.combinator-results (local.get $frame))
                (local.get $index) (call $hd.suspension_result (local.get $task))))
            (else (local.set $waiting (i32.const 1))))))
      (local.set $index (i32.add (local.get $index) (i32.const 1)))
      (br $each)))
  (if (i32.eqz (i32.or (local.get $waiting) (struct.get $hd.combinator $hd.combinator-race (local.get $frame))))
    (then
      (struct.set $hd.combinator $hd.combinator-state (local.get $frame) (i32.const 2))
      (return (i32.const 1))))
  (struct.set $hd.combinator $hd.combinator-state (local.get $frame) (i32.const 0))
  (i32.const 0))

(func $hd.combinator_cancel (type $hd.suspension-cancel-sig) (param $inner anyref)
  (local $frame (ref $hd.combinator))
  (local $index i32)
  (local.set $frame (ref.cast (ref $hd.combinator) (local.get $inner)))
  (if (i32.eq (struct.get $hd.combinator $hd.combinator-state (local.get $frame)) (i32.const 1))
    (then (call $hd.panic (i32.const ${runtimePanicCode("suspension-reentrant-poll")})) unreachable))
  (if (i32.eqz (struct.get $hd.combinator $hd.combinator-state (local.get $frame)))
    (then
      (block $cancelled
        (loop $cancel
          (br_if $cancelled
            (i32.ge_u (local.get $index) (array.len (struct.get $hd.combinator $hd.combinator-tasks (local.get $frame)))))
          (if (i32.eqz (array.get_u $hd.bytes (struct.get $hd.combinator $hd.combinator-done (local.get $frame)) (local.get $index)))
            (then (call $hd.suspension_cancel (call $hd.combinator_task (local.get $frame) (local.get $index)))))
          (local.set $index (i32.add (local.get $index) (i32.const 1)))
          (br $cancel)))
      (struct.set $hd.combinator $hd.combinator-state (local.get $frame) (i32.const 3)))))

(func $hd.combinator_result (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)
  (local $frame (ref $hd.combinator))
  (local.set $frame (ref.cast (ref $hd.combinator) (local.get $inner)))
  (if (result anyref) (struct.get $hd.combinator $hd.combinator-race (local.get $frame))
    (then (array.get $hd.list (struct.get $hd.combinator $hd.combinator-results (local.get $frame)) (i32.const 0)))
    (else (struct.get $hd.combinator $hd.combinator-results (local.get $frame)))))`;

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
  ;; \`block_on\` owns its loop until the suspension completes
  ;; (req.drive.block-on): a pending host operation is polled again, as the
  ;; entry driver does, since the prototype's host answers each poll itself.
  (block $ready
    (loop $drive
      (br_if $ready (i32.eq (call $hd.suspension_poll (local.get $frame)) (i32.const 1)))
      (br $drive)))
  (global.set $hd.driver-active (i32.const 0))
  (call $hd.suspension_result (local.get $frame)))

${COMBINATOR_RUNTIME}`;

function boxResult(value: string, type: ValueType): string {
  const mutable = mutableInner(type);
  if (mutable !== undefined) return boxResult(value, mutable);
  return boxScalar(value, type);
}

function suspensionIndex(declaration: HirProgram["functions"][number]): number {
  return declaration.suspensionIndex ?? declaration.index;
}

export function storedSuspensionAdapterReferences(program: HirProgram): readonly string[] {
  return [
    ...COMBINATOR_FUNCTIONS,
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
      const rawResult =
        declaration.result === "void"
          ? `(ref.null any)`
          : boxResult(`(struct.get $s${index} $s${index}result ${inner})`, declaration.result);
      const adapter = `(struct.get $s${index} $s${index}result_adapter ${inner})`;
      const result =
        declaration.result === "void"
          ? rawResult
          : `(if (result anyref) (ref.is_null ${adapter}) (then ${rawResult}) (else (call_ref $hd.suspension-result-adapt-sig ${rawResult} (ref.as_non_null ${adapter}))))`;
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
      const rawResult = boxResult(
        method.result === "void"
          ? `(ref.null any)`
          : `(call ${traitSuspensionResultName(trait.index, method.index)} ${inner})`,
        method.result,
      );
      const adapter = `(struct.get ${wrapper} ${wrapper}result_adapter ${inner})`;
      const result =
        method.result === "void"
          ? rawResult
          : `(if (result anyref) (ref.is_null ${adapter}) (then ${rawResult}) (else (call_ref $hd.suspension-result-adapt-sig ${rawResult} (ref.as_non_null ${adapter}))))`;
      return [
        `(func ${traitSuspensionWrapperPollAdapterName(trait.index, method.index)} (type $hd.suspension-poll-sig) (param $inner anyref) (result i32)\n  (call ${traitSuspensionPollName(trait.index, method.index)} ${inner}))`,
        `(func ${traitSuspensionWrapperCancelAdapterName(trait.index, method.index)} (type $hd.suspension-cancel-sig) (param $inner anyref)\n  (call ${traitSuspensionCancelName(trait.index, method.index)} ${inner}))`,
        `(func ${traitSuspensionWrapperResultAdapterName(trait.index, method.index)} (type $hd.suspension-result-sig) (param $inner anyref) (result anyref)\n  ${result})`,
      ];
    }),
  );
  return [...functionAdapters, ...traitAdapters].join("\n\n");
}
