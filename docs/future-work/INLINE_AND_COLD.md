# Inline And Cold Decorators

Status: accepted by the owner, 2026-10-26; domain: language feature. Phases 1-3 are implemented; phase 4, the std rare paths, measured a miss (#884, closed); phases 5-6 remain (#873).
Issues: [#873](https://github.com/HerringtonDarkholme/hd-lang/issues/873)
(`@inline`), [#874](https://github.com/HerringtonDarkholme/hd-lang/issues/874)
(`@cold`), payoff measured in
[#884](https://github.com/HerringtonDarkholme/hd-lang/issues/884). Under
review: [Prefix Decorators](../../spec/lang/14-annotations.md#prefix-decorators),
[Function Facts](../../spec/lang/14-annotations.md#function-facts), and the
release inliner ([tir-v2.md, "The inliner"](../compiler/tir-v2.md#the-inliner)).

## The Proposal

Two compiler decorators in `std.annotation`, recognized by qualified name
as `annotate` is. They change how calls are compiled, never what a program
does. The inliner's default rule (#856) stays for every undecorated function.

- `@inline`, short for `@inline(.Auto)`: tells the compiler to prefer
  inlining, not a hard guarantee. The function may be copied
  past the default rule: up to 64 instructions, with reference parameters.
  Where a copy is impossible (recursion, a body the copier refuses), it stays
  a call, with no error.
- `@inline(.Always)`: every call whose callee is known is replaced by a
  copy of the body, past the size limit and the reference-parameter rule.
- `@inline(.Never)`: hd's inliner never copies the function.
- `@cold`: `.Never`, and every direct call goes through the function
  table, so V8 does not inline it either.

```text
use std.annotation.{cold, inline}

# Runs once per input byte: always copy it, whatever the size limit says.
@inline(.Always)
fn mix(hash: u64, byte: u8) -> u64:
    (hash ^ u64(byte)).wrapping_mul(1099511628211)

# Runs only on bad input: never copy it, and keep the engine from copying it.
@cold
fn unexpected(at: usize, size: usize) -> string:
    if at >= size:
        return "unexpected end at byte $at"
    "unexpected byte at $at"

# A small reader the default rule refuses (it takes a reference): copy it
# where it can be copied, and keep the call where it can't.
@inline
fn first(input: &List[u8]) -> u8:
    input.get(0).unwrap_or(0)

# A long loop: keep it a function of its own, so the engine can optimize it.
@inline(.Never)
fn checksum(input: &List[u8]) -> u64:
    let hash: u64 = 14695981039346656037
    for byte in input:
        hash = mix(hash, byte)
    hash
```

Today this block fails only on the import: `unknown-import: module
'std.annotation' declares no 'cold'` (and `'inline'`). With the
declarations of [How It Works](#how-it-works) in place, it checks and runs.

## Questions And Answers

Each line is a question from #873 or #874, or one this design raises. The
answers were approved with the design on 2026-10-26.

1. Does `.Always` override the size limit, the reference-parameter rule, or both? **Both**; the bars of rule 6 in How It Works stay.
2. Does `.Always` hold in dev builds? **Yes, in every build and every package**: `.Always` means always. `.Auto` follows std's profile: it applies in every build too, so a decorated function is treated like std's. Only undecorated functions outside std are copied in release builds alone.
3. What does `.Always` on a recursive function report? **An error, `inline-always-uncopyable`**, when every function on the cycle is `.Always`; a cycle through an undecorated function breaks at that function.
4. Does the choice follow generic instances and trait impls? **Yes**, every instance, impl method and default body; calls through `&dyn` or a function value stay calls, with no diagnostic.
5. What does `@cold` change? **Inlining and call lowering**, not branch layout: V8 ignores branch hints by default.
6. Does `@cold` imply `.Never`? **Yes**; `@cold` with any `@inline` is an error, `invalid-inline-decorator`.
7. Is `panic` marked `@cold` in std? **No**: it is an intrinsic the emitter already lowers cold. std marks no rare paths: the #884 experiment measured a miss.
8. Values or compiler decorators? **Compiler decorators**, like `annotate` and `num_suffix`: they attach no value, so `facts_of` leaves them out.
9. Where do the names live? **`std.annotation`, imported**, like `annotate` and `facts_of`; not in the prelude.
10. How far does `.Auto` lift the default rule? **To 64 instructions, with reference parameters allowed**, measured on the 19 programs before it lands; a bound keeps a hint from bloating code the way `.Always` may.

## Problems It Solves

**1. The runtime's rare paths (#884).** V8 inlines the allocator's page
acquisition and the collector's buffer growth into json's run path. That
costs about 26 ms of TurboFan compile against MoonBit's 1.3 ms. Making only
the rare paths uninlinable saved 4 to 5 ms. But `lib/std/rt` has no way to
ask, and a std-only mechanism would be a special case. With `@cold`, std
marks `start`, `take`, `extend`, `claim`, `move_table` and `compact`, and
splits `push`'s growth out:

```text
# lib/std/rt/cycle.hd: `push` calls this when the array is full.
@cold
fn grow_array(at: usize, cap: usize) -> usize:
    bigger := alloc(8 + 8 * cap)
    copy(bigger, at, 8 + 4 * cap)
    store_u32(bigger + 4, u32(2 * cap))
    free_block(at)
    bigger
```

**2. An error path kept out of line by accident.** json's
`Parser.unexpected` has 20 call sites. It stays a call only because it
takes `&self`. When the #856 tuning allowed reference parameters, it was
copied, for 2.6 KB and no run time. `@cold` says so on purpose:

```text
use std.annotation.cold

enum JsonError:
    UnexpectedEnd(usize)
    UnexpectedCharacter(usize)

struct Parser:
    size: usize
    at: usize

impl Parser:
    @cold
    fn unexpected(&self, offset: usize) -> JsonError:
        if offset >= self.size:
            return JsonError.UnexpectedEnd(offset)
        JsonError.UnexpectedCharacter(offset)
```

**3. A hot helper near the limit.** spectral_norm's `a(i, j)` has 15
instructions, so it is copied. A limit of 12 left it a call and doubled
the run with `--liftoff-only` (262 to 543 ms, #856). One more term would
take it past 16 and make it a call, silently. `@inline(.Always)` keeps it copied and says why:

```text
use std.annotation.inline

@inline(.Always)
fn a(i: usize, j: usize) -> f64:
    1.0 / f64((i + j) * (i + j + 1) / 2 + i + 1)
```

**4. A callee that should tier up alone (#724).** Wasm has no on-stack
replacement, so a loop inlined into a function called once stays in
Liftoff. After bottom-up inlining copied `apply` into `drive`'s loop,
trait-dispatch was 4% slower on the default tier. `.Never` keeps `apply`
a function that V8 can optimize between calls (not yet measured):

```text
use std.annotation.inline

trait Step:
    fn step(self, x: i32) -> i32

@inline(.Never)
fn apply[S < Step](s: S, x: i32) -> i32:
    s.step(x)
```

## How It Works

`std.annotation` declares both decorators as plain hd, beside `annotate`:

```text
use std.annotation.annotate

@derive(Eq)
@annotate(.Fn, .Method)
pub enum InlineHint:
    Auto
    Always
    Never

pub fn inline(hint: InlineHint = .Auto) -> InlineHint:
    hint

@annotate(.Fn, .Method)
pub struct Cold: pass

pub fn cold() -> Cold:
    Cold {}
```

The rules, for [Prefix Decorators](../../spec/lang/14-annotations.md#prefix-decorators):

1. The compiler recognizes `std.annotation.inline` and `std.annotation.cold` by qualified name, through an alias or a path. They attach no value, so `facts_of` leaves them out.
2. They go on a module-level function or a method with a body. A trait requirement with no default body, or an `@intrinsic` declaration, is `invalid-inline-decorator`.
3. The argument of `inline` is written `.Auto`, `.Always` or `.Never`, or left out, and is read as written, as `annotate`'s kinds are. Anything else is `invalid-inline-decorator`. A bare `@inline` is `@inline(.Auto)`: a decorator that names a function whose parameters all have defaults calls it. The compiler does this today (probed: `facts_of` gives `.Auto`); the spec rule said "no parameters", and [`annot.decorator.bare-call-defaults`](../../spec/lang/14-annotations.md#r-annot.decorator.bare-call-defaults) now says "no parameters without a default".
3a. `.Auto`: in every build, as std's functions are, a call is a copy when the body has at most 64 instructions; reference parameters are allowed. A body that rule 6 makes uncopyable stays a call, with no diagnostic.
4. `@cold` with an `@inline` line on one declaration is `invalid-inline-decorator`.
5. `.Always`: in every build, dev and release, and in every package, every call whose callee is known once generic arguments are filled in is a copy. Size and reference parameters do not matter.
6. `.Always` is `inline-always-uncopyable` on a function that suspends, has a `defer`, makes a closure, has a requirement row or `with`, or is in `std.rt`. So is a cycle of calls whose every function is `.Always`.
7. `.Never` and `@cold`: no build copies the function.
8. `@cold`: in every build, each direct call of the function is a call through the function table.
9. None of them changes output, panics, or panic frames: a copy keeps its frame, as today.

| Decorator | hd's inliner | Wasm call | Builds |
| --- | --- | --- | --- |
| none | the #856 rule: at most 16 instructions, no reference parameter, not recursive | `call` | std every build; others release |
| `@inline` (`.Auto`) | copies up to 64 instructions, reference parameters too; otherwise a call, silently | `call` where not copied | every build |
| `@inline(.Always)` | always copies, any size, reference parameters too | none: the body is copied | every build |
| `@inline(.Never)` | never copies | `call` (V8 may still inline it) | every build |
| `@cold` | never copies | `call_indirect` through the table | every build |

The compiler changes: the resolver reads both lines as it reads
`annotate`'s (`crates/hd_resolve/src/lower/decorators.rs`). `inline_callee`
refuses `.Never` and `@cold` callees and lets `.Always` past `INLINE_LIMIT`
and `takes_reference` (`crates/hd_driver/src/own.rs`). The emitter lowers a
direct call of a `@cold` function as `Func::cold_helper` lowers a helper's
(`crates/hd_wasm_linear/src/module.rs`), from any function symbol.

## Cost

| Component | Estimate |
| --- | --- |
| Code removed | none; `Func::cold_helper` stays for the runtime's helpers |
| `std.annotation` | +25 hd lines |
| Resolver: read the two lines | +40 Rust lines |
| Checker: two error codes, the cycle walk over `.Always` functions only | +90 Rust lines |
| Inliner: `.Never`/`@cold` refused, `.Always` past both rules, cycles | +25 Rust lines |
| Copier: the body forms `.Always` needs that it refuses today | not sized; its phase starts with an estimate |
| Emitter: table calls for any function | +30 Rust lines |
| Spec and fixtures | +45 spec lines, about 8 fixtures |
| hd compile time | one flag test per call site: none measurable |
| Run time and size of undecorated code | none |
| Wasm size | about +4 bytes per `@cold` call site and one table entry per `@cold` function; `.Always` adds its copies |
| V8 compile | json: −4 to −5 ms of TurboFan with the rare paths alone (#884 experiment) |
| Web compiler gzip | about +2 KB, against the 2 MB cap |

Compiler gap: `hd_tir::inline::inlinable` refuses three body forms that
tir-v2.md gives no reason for. They are a `match` on an integer, char or
string, a `&dyn` call, and a host call. Until the copier takes them, rule 5
cannot hold for them (phase 6).

## Appendix A. Stress Test

Each hd case was checked with today's `hd check` and run, with the
[How It Works](#how-it-works) declarations pasted in place of the import.
Rows that say "not measured" are reasoning only.

| Case | What happened | What the design does |
| --- | --- | --- |
| hashbrown's `RawTable::reserve_rehash` (`#[cold] #[inline(never)]`), as `@cold fn grow(&mut self)` in a probing table | Checks and runs. `grow` calls `insert`, which calls `grow`: a cycle | Allowed: only an all-`.Always` cycle is an error |
| serde_json's `peek_error` (`#[cold]`), as `@cold fn error(&self, wanted: u8) -> string` | Checks and runs | A table call on the error path only |
| Go's `//go:noinline` on a benchmark body, as `@inline(.Never) fn sum_to(n: u64)` | Checks and runs | Copied nowhere, so the loop stays a function |
| The #884 rare paths in `lib/std/rt`, decorated, with `push`'s growth split out | std built with a local `cold` and the decorators; json, binary_trees, map-count, closures and nested-display print the same results | The first std users. `@cold` on all of `push` would make every push a table call: split first |
| `@cold` on a hot function by mistake | Not measured; each call adds a table bounds check and a signature check | No guard; std marks only with the perf report |
| `@inline(.Always)` on a 200-instruction function with 50 callers | Not measured; the copies multiply its size | No guard, as in Rust, Swift and GCC; the size report shows it |
| `.Always` on `StringBuilder.push` (`&mut self`) | #856 measured this copy: string_build 92 to 130 ms | Allowed; the caller's local becomes a counted cell. std never marks such a function without a measurement |
| `.Always` on a generic function, `clamp[T < Ord]` | Checks and runs | Holds at every instance |
| `.Always` on an impl method; `.Never` on a trait's default body | Checks and runs | Held for calls the compiler resolves to that body |
| `.Always` cycle closed only through a trait impl in a generic body | Not measured; `hd check` sees the bound call, not the impl | The call that closes the cycle stays a call, with no diagnostic, as SE-0496 does for protocol calls |
| `@cold` on a trait requirement with no body | Checks today: the decorator is an ordinary value | `invalid-inline-decorator` |
| `@cold` with `@inline(.Always)` | Checks today | `invalid-inline-decorator` |
| `.Always` on a self-recursive `depth(n)` | Checks today | `inline-always-uncopyable` |
| `.Always` on a bang function `wait!` | Checks today | `inline-always-uncopyable` |
| Two `@inline` lines | `duplicate-fact` today | Kept as is |
| `@inline` on a struct | `decorator-target-kind` today | Kept as is |
| `@inline` on a local closure | `decorator-not-top-level` today | Kept as is |
| V8 turns on `--wasm-inlining-call-indirect` by default | Not measured; table calls would become inlinable by speculation | `@cold`'s engine half weakens; its hd half holds. Re-measure #884 then |
| `@cold` on `main` or an exported function | Exports are not calls | Only calls inside the program change |

## Appendix B. Prior Art

**Rust.** Every form is a hint the compiler may ignore; `#[cold]` marks
a function unlikely to be called
([reference](https://doc.rust-lang.org/reference/attributes/codegen.html)).
hashbrown pairs the two on its growth path
([source](https://github.com/rust-lang/hashbrown/blob/master/src/raw.rs)):

```rust
// hashbrown, src/raw.rs
#[cold]
#[inline(never)]
unsafe fn reserve_rehash(&mut self, additional: usize, /* ... */) { /* ... */ }

// Illustrative
#[inline(always)]
fn top7(hash: u64) -> u8 { (hash >> 57) as u8 }
```

hd takes the names and the pairing, folded into one `@cold`. It rejects
"a hint the compiler may ignore": `.Always` is checked.

**C and C++.** GCC's `always_inline` reports a failure to inline as an
error; `noinline` stops it; `cold` also marks paths to its calls unlikely
([GCC](https://gcc.gnu.org/onlinedocs/gcc/Common-Attributes.html)). C++20
adds branch-level `[[likely]]` and `[[unlikely]]`
([cppreference](https://en.cppreference.com/w/cpp/language/attributes/likely)):

```c
__attribute__((always_inline)) static inline int sq(int x) { return x * x; }
__attribute__((noinline)) void bench_body(void);
__attribute__((cold)) void die(const char *msg);
```

hd takes the error. It leaves branch hints out, since its one engine
ignores them by default.

**Swift.** `@inline(__always)` is ignored when it cannot apply, such as on
recursion, and does nothing in debug builds
([UnderscoredAttributes.md](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/UnderscoredAttributes.md)).
The accepted SE-0496 `@inline(always)` is a guarantee: an error on a
cycle of always-inline functions, and no use-site error for protocol or
function-value calls
([SE-0496](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0496-inline-always.md)):

```swift
@inline(always) func clamp(_ x: Int, _ lo: Int, _ hi: Int) -> Int { min(max(x, lo), hi) }
@inline(never) func slowPath() { /* ... */ }
```

hd takes SE-0496's cycle rule and its dynamic-call rule. It keeps hd's
dev-build rule instead of SE-0496's every-mode guarantee.

**Kotlin.** `inline` is semantic: it copies lambda arguments, allows
non-local `return` and `reified` type parameters. Inlining a function
with no lambda parameter draws a warning
([docs](https://kotlinlang.org/docs/inline-functions.html)):

```kotlin
inline fun <T> measure(block: () -> T): T {
    val start = System.nanoTime()
    return block().also { println(System.nanoTime() - start) }
}
```

hd rejects meaning-changing inlining: a decorator must not change behavior.

**Go.** `//go:noinline` overrides the inliner; there is no force-inline
directive ([cmd/compile](https://pkg.go.dev/cmd/compile)):

```go
//go:noinline
func sumTo(n int) int { t := 0; for i := 0; i < n; i++ { t += i }; return t }
```

hd agrees that `.Never` is the more common need, and adds `.Always`
because #856 measured a helper that one limit drops.

**MoonBit.** `#inline` and `#inline(never)` are hints that do not change
behavior ([attributes](https://docs.moonbitlang.com/en/stable/language/attributes.html));
there is no cold attribute:

```moonbit
#inline
fn add_one(x : Int32) -> Int32 { x + 1 }

#inline(never)
fn keep_stack_frame(x : Int32) -> Int32 { x + 1 }
```

**V8.** It does not inline `call_indirect` by default
(`--wasm-inlining-call-indirect` is off), which `Func::cold_helper` relies
on. Branch hints (`metadata.code.branch_hint`) are standard, but
`--experimental-wasm-branch-hinting` is off in Node 24.19 (`node
--v8-options`).

## Appendix C. Alternatives Considered

- **No feature.** Take `&self` to dodge the inliner, as `Parser.unexpected`
  does by accident. That hides the intent and breaks when the rule changes.
- **A std-only mechanism for `lib/std/rt`.** It is a special case; #884
  says it needs approval for that reason. `@cold` covers it generally.
- **A higher limit.** #667 raised it to 40: `for` loops got slower and
  larger. #856 tuned 16 over 8, 12, 16 and 20.
- **`.Never` through the table as well.** #724 needs only hd's choice, and
  a `.Never` function is often hot, where a table call costs.
- **Branch hints for `@cold` call sites.** V8 ignores them by default.
  Revisit if it turns them on.
- **Ordinary fact values read by `facts_of`.** That works today (Appendix
  D), but then the values change compiled code while
  [`annot.decorator.attach`](../../spec/lang/14-annotations.md#r-annot.decorator.attach)
  says an ordinary value changes nothing.

## Appendix D. Today In hd

- **The inliner.** `inline_callee` and `Env::callee_copy` in
  `crates/hd_driver/src/own.rs` copy a callee of at most `INLINE_LIMIT` =
  16 instructions that takes no reference and is not on a cycle of calls.
  Release copies bottom-up. A dev build copies std only. `std.rt`, intrinsics
  and suspending functions are never copied. `hd_tir::inline::inlinable`
  also refuses `defer`, closures, providers, `&dyn` and host calls, and
  integer, char and string switches.
- **Cold calls.** `Func::cold_helper` in `crates/hd_wasm_linear/src/module.rs`
  calls a runtime helper through the function table: list copies and
  growth, formatted panics, the collector's entry points. It took
  spectral_norm `--no-liftoff` from 350 to 163 ms (#165). hd code cannot
  ask for it.
- **Decorators.** A decorator is a value (`annot.decorator.attach`).
  Compiler decorators attach none (`annot.facts-of.result`), and std types
  recognized by qualified name change compilation:
  `annot.target.recognized` for `Annotate`, `expr.literal-fn.marker` for
  `NumSuffix`. The resolver reads `@annotate`'s arguments as written.

Probes on `origin/main` at 8fbfcd74a, `target/debug/hd`:

| Probe | Result |
| --- | --- |
| Each `text` block above, as written | `unknown-import: module 'std.annotation' declares no 'cold'` (and `'inline'`); the rt block is std-only |
| The same, with the How It Works declarations pasted in | `check result: ok`; the runnable ones run |
| The rt block, inside `lib/std/rt` with a local `cold`, std rebuilt | builds; the five programs print the same results |
| `@cold` + `@inline(.Always)`; `.Always` on recursion, on a bang function; `@cold` on a requirement | all `check result: ok`: the gaps rules 2, 4 and 6 fill |
| Two `@inline` lines; `@inline` on a struct; on a local closure | `duplicate-fact`; `decorator-target-kind`; `decorator-not-top-level` |
| `facts_of(square).find::[InlineHint]()` with the declarations pasted in | `.Some(.Always)`; under rule 1 it would be `.None` |
| Bare `@inline` on a function, with `fn inline(hint: InlineHint = .Auto)` declared locally | checks and runs; `facts_of` gives `.Some(.Auto)`: the bare call already uses the default |
| `hd fmt --check` on every block | no change |

## Appendix E. Details

- **Grammar and formatting.** None new: decorator lines. `hd fmt` keeps them.
- **Typing.** `inline(hint: InlineHint = .Auto)` and `cold()` are ordinary calls; the
  argument is read as written.
- **Patterns, derives.** No interaction; derived impls carry no decorator.
- **Closures.** Not decorated (`decorator-not-top-level`). A closure passed
  to a `.Always` function stays a closure value in the copy.
- **Modules and packages.** The names are imported. Builds compile every
  package from source, so a `pub` `.Always` function copies across
  packages. If a package is ever used by its interface only, its
  interface carries `.Always` bodies, as
  [`module.interface.generic-bodies`](../../spec/lang/10-modules.md#r-module.interface.generic-bodies)
  lets it carry generic ones.
- **Ownership.** Copies run before the ownership passes, as today. A
  `.Always` reference parameter makes the caller's local a counted cell.
  Release summaries (RELEASE_INTERPROC.md) still summarize a `.Never` function.
- **Soundness.** Copying already preserves meaning; `.Always` only widens
  which bodies are copied. Before #856, std copies took reference parameters.
- **Diagnostics.** `invalid-inline-decorator` (placement, argument,
  `@cold` with `@inline`) and `inline-always-uncopyable` (body form,
  cycle, `std.rt`). Both are errors.
- **Spec.** Rules 1 to 9 under Prefix Decorators; `inline` and `cold` added
  to `annot.facts-of.result`'s list. Conformance: one fixture per error
  case, one accepted case per decorator.
- **The no-feature alternative.** Appendix C, first item.

Phases, each about an hour with its test:

1. `std.annotation` declarations; the resolver reads both; `invalid-inline-decorator`; spec rules and fixtures.
2. Inliner: `.Never` and `@cold` refused. Test: a `tir` stage case keeps the call.
3. Emitter: `@cold` calls through the table. Test: a `wat` stage case shows `call_indirect`.
4. std: the #884 rare paths marked, `push`'s growth split. Measured a miss; #884 closed, attempt on `park/884-cold-rt`.
5. `.Always`: past the limit and the reference rule; `inline-always-uncopyable`; cycles. Test: stage cases and fixtures.
6. The copier takes integer, char and string switches, `&dyn` calls and host calls. Starts with a size estimate; stops at 1.5x.

Phases 1 to 3 are implemented; phase 4 measured a miss.
