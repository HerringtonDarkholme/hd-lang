# Dogfood #199: useful programs on the current compiler

Four programs in `examples/dogfood/`, each with `hd check`, `hd run` and
`hd test` passing: `calc.hd` (5 tests incl. one property test),
`orders.hd` (3), `textstats.hd` (3), `tasks.hd` (7). Each file also has a
`main`. Every mistake or
compiler problem is a row in [hd-writing-log.md](hd-writing-log.md) (task
`dogfood #199: ...`). `hd build calc.hd` took about 1 s and wrote a 138,506
byte `calc.wasm`.

## Features exercised

| Feature | Status | Notes |
| --- | --- | --- |
| enum with payloads, exhaustive `match`, nested patterns | works | calc, tasks |
| `Result` and `?` with positions in the error | works | calc, tasks |
| `data` with `mut self` methods, mutable parser cursor | works | needs `let mut p = ...` at the call site |
| `@derive(Eq, Debug)` | works | used as `assert_equal` operands |
| `@derive(Arbitrary)` + `it_prop` with shrinking | works | found two real bugs in my round trip (negative literals, arbitrary `char` op) |
| `impl Display` for an error enum, `"$e"` interpolation | works | orders, tasks |
| requirement traits, `$.with(A=..., B=...)`, `$.use` | works | orders, tasks; mutable provider via `mut self` method |
| `std.time.Clock` with `ManualClock` | works with workaround | `Timestamp.millis` is private; use `since(epoch).as_milliseconds()` |
| `fn!` suspension in tests and `main!` | works | tests call `x!()` directly |
| `Map` get / index assign / iterate | works with workaround | the field must be declared `mut Map[..]` for `m[k] = v` in a `mut self` method; `keys()` missing (earlier log row) |
| `List` building | works with workaround | `append`, not `push`; no `pop`; nested lists are not mutable through `out[i].append` |
| closures, `filter`, `map`, `fold`, `sorted_by`, `chunks`, comprehension | works | textstats |
| string methods `lines`, `split`, `trim`, `lower`, `contains`, `slice` | works | textstats |
| triple-quoted string literal | works | textstats |
| `defer:` inside a loop body | works | tasks |
| `all!` | works with workaround | fixed arity only (spec forbids spreads); batches of 3 in `run_wave!` |
| `hd build` | works | 1 s, 138 KB for calc |
| `hd test` with a `main` in the file | oddity | runs `main` and prints its output; the passed count is one above the `it` count |

## Three worst usability problems

1. Mutability messages. The binding forms themselves are fine and the
   compiler enforces the spec: a typed mutable binding is
   `let x: mut T = v`, an untyped one `let mut x = v`; `let mut x: T`
   (`let-mut-readonly-type`) and a bare `x: mut T =` (`missing-let`) are
   errors with good fix-its, and `let mut x: mut T` warns
   (`redundant-let-mut`). The writer's slips were between these forms.
   The real problems are other messages: a readonly `List` yields
   `unknown-method ... 'push'` with no hint about the readonly binding or
   about `append`; several diagnostics print `mut:List[char]` instead of
   `mut List[char]`; and `Walk { state: {} }` against a `mut Map` field
   reports `cannot-infer-type` although the field type is known.
2. Thin std. `std.fs`/`json`/`path` are not loaded, `List` has no `pop`;
   `Map` has no `keys`; `char` has no
   `is_digit`; `min`/`max` need `use std.cmp.max`; `Timestamp.millis` is
   private; `all!` takes no list. Every program needed a helper for
   something other languages ship.
3. Syntax traps between sibling forms: postfix list spread `[xs..., y]` vs
   prefix data spread `...x`, tuple access `t._0`, and `hd test` running
   `main`. The compiler messages for the tuple and `let` cases are good and
   gave the fix; the spread one did not.

## Three best things

1. Property testing works out of the box: `@derive(Arbitrary)` plus
   `it_prop` found a real round-trip bug and shrank it to a one-node tree.
2. Requirements read well: `$.with(Inventory=fresh_inventory(), Clock=ManualClock::new(...))`
   gives deterministic tests of suspending code with no mocking framework.
3. Exhaustive `match` on enums and `?` with positioned errors made the
   calculator and the cycle detector short and correct on the first run
   once the types checked; most diagnostics name the fix.

## Verdict

Yes, you can write useful, tested programs in hd today, as long as they stay
inside pure logic, enums, requirements and collections: all four programs
run, test, and build to WebAssembly. The cost is friction, not blockers.
The mutable/readonly distinction is the main source of compile-fix cycles,
and the standard library is small enough that you write your own helpers
(digit check, pop, max, fan-out). Nothing blocked a program, and no compiler
bug was found that required leaving a program out.

## Findings and where they go

| # | Finding | Kind | Owner |
| --- | --- | --- | --- |
| F1 | `unknown-method` on a readonly receiver hides the cause: it should say the binding is readonly (or suggest the right method name) | diagnostic | compiler session, error-code revamp #101 |
| F2 | types print as `mut:List[char]` instead of `mut List[char]` in diagnostics | diagnostic bug | compiler session |
| F3 | `Walk { state: {} }` with a field typed `mut Map[string, i32]` reports `cannot-infer-type`; the expected field type should type the empty literal | checker bug | compiler session |
| F4 | `hd test FILE` also runs `main` and counts it as a passed case | CLI bug | compiler session |
| F7 | `List` had no `pop` and used `append` | std | spec pass 76 (#228): renamed to `push`, `pop` added |
| F8 | `all!` cannot await a runtime-sized list | std | spec pass 76 (#228): `std.task.all_list!` |
| F11 | `min`/`max` need `use std.cmp.max`; not in the prelude | std, by design (the prelude does not grow) | no action |
| F13 | list spread is postfix (`[xs..., y]`) but a data spread is prefix (`...x`); `[...xs]` gives a bare `expected-expression` | diagnostic | compiler session: suggest the postfix form |

