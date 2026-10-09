# Triage: Wrong Run Results (R2)

Method: same log as R1, rerun at `42dbf1ac`
(`HD_CONFORMANCE_ONLY=.hd cargo test -q --release -p hd_driver --test
conformance -- --nocapture`), kept the 46 lines with expectation
`accept` and verdict `fail:runtime-exit` (36) or `fail:stdout` (10).
(The brief said 57+10; the current tree shows 36+10.) Each case was
reproduced in a scratch package (`/tmp/r2run`, `hd run` for `main`
fixtures with byte-exact stdout comparison, `hd test` for `tests:`
fixtures). Read only: no compiler or fixture edits.

## A. Driver artifact, not a compiler bug (4)

`fail:stdout` where the program's bytes are exactly right per the
README's Standard Output decoding (`\\` → backslash, `\t` → U+0009,
`\u{H}` → scalar). The Rust driver's `expected_stdout`
(`hd_driver/tests/conformance.rs`) joins the raw directive lines
without decoding, so any expectation containing an escape fails.
Byte-verified (`od -c`, decoded comparison matches): crlf-line-endings
(CR bytes correct), multiline-string-literals, prefixed-string-template,
tab-only-as-content. Suspected stage: test harness (driver), not Emit
or the host. Recommendation: decode in `expected_stdout` (orchestrator
lane).

## B. Real wrong output (6)

| Root cause | Cases | Minimal program | Spec rule | Suspected stage |
| --- | --- | --- | --- | --- |
| `\'` in a char literal evaluates to `\` (0x5C) instead of `'` (2) | escape-sequences (line 4: long `&&` chain goes `false`), string-and-char-literal-contents (line 2: `\` for `'`) | `pub fn main() -> void $ Console: println('\'')` → prints `\` (verified; `\\` and `\u{5C}` in char position are fine) | lex.escape.simple in char literals | Lexer escape decoding (value surfaces at runtime) |
| Nested quotes double in interpolation; map value prints raw (2) | interpolation-expression-spacing, interpolation-forms | `nested "quoted""quoted" text` for expected `nested "quoted" text`; `map has ab2` for expected `map has 2` (from fixture output) | String interpolation / Display | Emit (string building) or runtime host |
| Bracket/header expression off by one element (1) | header-and-bracket-expression-positions | `a a5` for expected `a 5` (from fixture output) | Header/bracket evaluation order | Emit |
| Type-expression forms evaluate wrong (1) | type-expression-forms | `10` for expected `11`; `x2 y4` for expected `2 4` (from fixture output) | Type expressions as values | Emit |

## C. Wasm validation traps: fallthru stack has 1 element (5)

`WebAssembly.compile(): Compiling function …$Body failed: expected 0
elements on the stack for fallthru, found 1` — Emit leaves a value on
the stack at function end. Cases: comprehension-bang-calls,
console-error-line-override, scripted-input,
suspending-call-in-scoped-defer, suspending-calls-in-loops. At least
three involve suspending (`!`) calls in a body position; the common
thread is unproven — first step of the compiler task is shrinking one
(e.g. `suspending-calls-in-loops` main). Suspected stage: Emit. These
are Wasm traps (host `node` rejects the module), not hd panics.

### R12 shrinks (2026-10-09): one shared shape

All five shrink to the same construct: a suspending (`!`) call inside
a branching position leaves its value on the stack at the join. Plain
statement position (`_ := inner!()` as a body tail) and `for` loops
with suspending calls validate fine; `while` loops, `if` branches,
`break` values and guarded comprehensions do not. Each minimal below
was verified to fail validation (`fallthru, found 1`) via `hd test`
in a scratch package; each stated smaller variant passes.

1. **Guarded comprehension** (`comprehension-bang-calls`):
   `fn fetch!(n: i32) -> i32: n` +
   `fn names!(ids: List[i32]) -> List[i32]: [for id in ids if id != 2 => fetch!(id)]`.
   Without the `if` guard it passes; with a plain (non-`!`) call it
   fails differently (`local.set` type error — a second Emit bug in
   the same construct).
2. **Discarded call in a trailing `if`** (`console-error-line-override`):
   `fn inner!() -> i32: 1` +
   `fn work!() -> void: if true: _ := inner!()`.
   Without the `if` it passes; with a trailing `0` (non-void) it
   still fails.
3. **Discarded call in a `while` body** (`scripted-input`):
   `fn gen!() -> i32?: .None` +
   `fn count!() -> i32: let count = +0; while count < 1: _ := gen!(); count = count + 1; count`.
   No match or `return` needed (the fixture's match/`return` reduce
   away); a `for` loop with the same body passes.
4. **Suspending call as a `break` value** (`suspending-calls-in-loops`):
   `fn step!(value: i32) -> i32: value` +
   `fn main!() -> i32: total := while true: break step!(1); else: 0; total`.
5. **Suspending call as a branch tail value**
   (`suspending-call-in-scoped-defer`):
   `fn child!() -> i32: 2` +
   `fn main!() -> i32: if true: child!() else: 0`.
   The fixture's `defer` reduces away.

Likely one cause in `Await` lowering: the suspension state machine
leaves the call's value (or resume state) on the operand stack when
the call sits in a branch/loop arm rather than in tail or plain
statement position. The `for`-loop and plain-statement positions
already balance the stack, so the fix is in whatever joins the
suspending arms, not in call emission itself.

## D. Sub-word-width arithmetic runs at full width, checked (7)

All integer arithmetic behaves as checked 32/64-bit: no wrapping, no
masking to the operand width, no saturation. Observed as hd panics
with the wrong code (`integer-overflow` where wrapping is specified)
or wrong values:

- num-bit-counts: `count_ones`/`leading_zeros` always 32 (width ignored)
- num-every-width: `i8`/`i16`/`u8` trap; `u16` yields 4294967295
- num-rotate: shift without wraparound (300 for 45; −255 for 1)
- num-saturating: saturating add traps instead of clamping
- narrowing-cast-wraps: `300 as u8` stays 300, expected 44
- digest-sha256-long, digest-sha256-vectors: wrapping `u32` multiply traps (plus one wrong-word assertion downstream of the same cause)

Suspected stage: Emit (arithmetic ops) and/or the checked-ops policy.
One compiler task: width-aware wrapping/saturating arithmetic.

## E. Unimplemented runtime intrinsic: `explicit-panic: intrinsic` (18)

The program compiles, then panics with `explicit-panic: intrinsic` —
a placeholder for an operation the runtime does not implement yet.
Sub-areas (one task each, or one sweep):

- Generic-data destructuring (3): generic-data-let-pattern (panics
  before any output), generic-data-match-pattern (prints `origin`,
  then panics), generic-data-pattern-in-generic-function (prints
  `two 1`, then panics)
- `dbg()` (1): dbg-prints-void panics on entry (`dbg(find(7))`)
- List mutation (2): list-pop (`pop`), list-insert-remove-clear
  (`remove_at`, `clear`)
- f64 text conversion (5): num-parse-f64-round-trip,
  num-parse-f64-specials, num-parse-f64-values, num-to-fixed,
  signed-zero-and-infinity-through-generics
- String/display methods (7): display-tuples, empty-string-operations,
  interpolation-display-order, multibyte-scalar-strings,
  replace-empty-old, string-more-methods, string-trim-and-lower

Suspected stage: runtime host / intrinsic table. The exact intrinsic
per case was not extracted (the panic names only `intrinsic`) —
first step of the task is logging which intrinsic each site calls.

## F. Other wrong values (5, one row each)

- bounded-blanket-supertraits: supertrait dispatch yields 0, expected
  42 (`assertion-failed: actual 0, expected 42`). Suspect: Check/Emit
  (dictionary).
- literal-patterns: `main` returns 29, expected 32. Suspect: Emit
  (literal patterns).
- map-lookup-and-duplicate-keys: duplicate keys keep the FIRST value
  (20, expected 40). Suspect: runtime host (map insert).
- nested-closure-captures: `main` returns 2, expected 42. Suspect:
  Emit (captures).
- embedded-part-follows-container: stamp copy yields 1700000300,
  expected 5. Suspect: Emit/host (interior sharing).

## Unreproduced (1)

- pipe-nested-placeholder: `fail:runtime-exit` in the log, but passes
  (`1 passed`) under `hd test` in the scratch package. Possibly
  runner/host flakiness or environment; needs an orchestrator rerun,
  not a compiler task yet.
