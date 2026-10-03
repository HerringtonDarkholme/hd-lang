# Unsigned-Size Fixture Drift

Date: 2026-10-03

The `U32-SIZES` compiler repair implements `usize = u32`, unsigned built-in
list and string indices, unsigned slice bounds, and the decided std size
signatures. A full portable sweep exposed six fixtures that
still encode the retired signed-size behavior. Making the compiler accept them
would contradict the current unsigned rules, so they are tracked under
`SPEC-U32-DRIFT` until the specification/fixture session reconciles them.

| Fixture | Stale assumption | Required spec-session action |
| --- | --- | --- |
| `runtime/valid/assignment-place-before-value.hd` | Its evaluation-order helper returns `i32` and is used as a list index. | Change the helper/index expectation to `usize` without weakening the evaluation-order assertion. |
| `runtime/valid/evaluation-order-elements-and-indexing.hd` | Its indexing helper likewise returns `i32`. | Migrate the helper to `usize` while preserving the ordering checks. |
| `runtime/valid/string-byte-methods.hd` | `char_indices` offsets are collected into `List[i32]`. | Change the expected offset collection to `List[usize]`. |
| `runtime/valid/deque-ends.hd` | It expects `Deque.get(-1)` to return `.None`. | Remove or replace the negative-index assertion now that `Deque.get` takes `usize`. |
| `runtime/valid/text-prefix-helpers.hd` | Its `position` helper returns `i32` with `-1` as a sentinel, but `EscapeError.position` is now `usize`. | Return an optional `usize` or otherwise remove the signed sentinel while preserving the byte-offset checks. |
| `runtime/valid/regex-captures.hd` | Its loop index is an unannotated `0`, inferred as `i32`, then compared with `Captures.len()` and passed to `Captures.get`, which use `usize`. | Annotate the loop index as `usize`. |

These are specification artifacts, not accepted compiler exceptions. The
checker continues to report `type-mismatch` or `unsigned-negation` for their
signed indices/counts.
