# Remaining conformance failures of the List switch (dev, lane areas)

Rule: #2369 step 1 changes no program's acceptance or output.

Today (lane 299 WIP `lane299/switch-wip` 80cac9bff, local only, dev pipeline over std/collections, std/iter,
lang/references, lang/data, lang/functions, lang/control-flow,
lang/expressions): 4,365 pass, 40 fail. Beside issues 1-3 (20 runtime,
8 no-diagnostic, 3 memory):

- `list-literal-spread-of-any-iterable-into-a-list.hd`,
  `...-of-a-non-iterable-into-a-list.hd`: `list_expr` still requires a
  spread operand to be a list (`expr.list.spread.type`); the builder
  drains any iterable (`expr.list.spread.iterable`). Owner question,
  issue 7.
- `list-spread-an-int-and-a-spread-string-list-have-no-common-type.hd`:
  the join's message changed with the spread conversion moving into the
  drain.
- `copy-list-literal-of-mut-holder.hd`,
  `a-list-literal-of-a-newtype-over-a-not-copy-type.hd`: the `Copy`
  bound is now reported at the builder call's span, not the element.
- `reference-copy-out.hd`, `std-list-extend-counted.hd`: a write
  through `&List[*mut U]`/`&Bag` element takes `IndexMut` where the
  builtin wrote the element place through the pointer it holds.
- `element-receiver-push-argument-takes-list.hd`: `xs[0].push(xs...)`
  conflict not reported (the push-through-`IndexMut` receiver path).
- four `conflicting-access` false positives:
  `for-element-references-through-mut-parameter-push-conflict.hd`
  (wrong message), `mut-reference-two-typed-reborrows-conflict.hd`,
  `a-pointer-loan-keeps-its-independent-owner-owned-copy.hd`,
  `a-copied-box-keeps-a-dependent-reference-independent-of-its-list.hd`.

Not run yet: the other conformance areas, checked and optimized modes,
`std_runtime*`, `checked_rooms*`, stage and unit tests.

Where: `crates/hd_check/src/expr.rs` (`list_expr`, `build_list`),
`call.rs` (the push-through-`IndexMut` branch),
`dep_results/constraints/exclusive.rs` (`call_conflicts`, `list_loop`).

Done: zero failures in the lane areas in dev, checked and optimized.
