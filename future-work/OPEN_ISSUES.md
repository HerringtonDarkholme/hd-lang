# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end.

## Language Design Decisions

### Local Mutability: `let mut` As An Inference Helper

**The owner's summary (2026-09-29).** The spec and guide should state the
rule this way:
- `a := ...` makes a name that can't be reassigned, with a read-only type.
- `let a = ...` makes a reassignable name, with a read-only type by
  default.
- For a mutable type, either add `mut` after `let` (`let mut a = ...`) to
  require mutability, or state the type (`let a: mut T = ...`).
- **Rule of thumb:** prefer `:=`. Use `let` when you need to reassign.
  Write `mut` at most once for a mutable type.
- There is no form for a fixed name holding a mutable object (like
  Kotlin's `val xs = mutableListOf()`). The owner confirmed this is
  acceptable on 2026-09-29: a mutated local uses `let mut`, even when it is
  never reassigned.

**Decided (owner, 2026-09-29, final); not yet applied.** This replaces
the Kotlin-style inference recorded earlier the same day (89c6a11).

1. `mut` stays a permission in the type (`x: mut T`, `List[mut User]`), as
   decided in August (SYNTAX_NOTES).
2. **`let mut` is only a mutability inference helper.** It tells the
   compiler to infer the binding's root as `mut`:
   - `let mut a = User { ... }` gives `a: mut User`;
   - with an annotation, the annotation must agree. `let mut a: User = ...`
     and `let mut xs: List[i32] = []` are errors, because the type says
     read-only while `let mut` asks for `mut`. The fix-it adds `mut` to the
     type or drops the `mut` after `let` (owner, the same day);
   - `let (mut log, db) = $.use(Log, Db)` works per name in patterns.

   It can't upgrade a read-only value: `let mut c = readonly_source()` is
   `mutable-upgrade` (`spec/04`), because a function returning `T` may be
   lending a view of an object it still owns. For a mutable copy, write a
   fresh literal: `let mut b = User { ...readonly_source() }`. `Clone`
   stays as M22 decided (owner, 2026-09-29): `clone(self) -> Self` and
   `clone_mut(mut self) -> mut Self`. The owner declined making
   `clone` return `mut Self`, so a spread literal is the way to get a
   mutable copy of a read-only value. A spread from a read-only value only
   covers fields without `mut`. Each `mut` field (`friend: mut User`) needs
   a `mut` value (the existing field-initialization rule), which a
   read-only source can't give. So a type with `mut` fields must override
   them with fresh values, as in `User { ...r, friend: fresh_friend() }`,
   or offer a constructor (owner observation, 2026-09-29).
3. A plain `let` without `mut` infers read-only, unless its annotation
   says `mut` (`let a: mut List[i32] = []` stays valid). Fresh literals are
   not silently `mut`.
4. `:=` stays read-only.
5. `let mut a: mut User = User { ... }` is allowed but redundant (owner,
   the same day). At most a style lint suggests removing one `mut`.

Why: it removes the repeated type (`let a: mut User = User {...}`) while
the declaration still says which locals change, and `mut` keeps a single
meaning, a permission in the type.

### Dependency Cycles

**Decided (owner, 2026-09-29); applied 2026-09-29** as
[Dependency Cycles](../spec/10-modules.md#dependency-cycles) and
[Initialization Order](../spec/10-modules.md#initialization-order). Points 1,
3, and 4 are the rule; point 2's file-level check is implied by point 3; and
point 5 became a leaf *folder* with a fix-it
([record](DEPENDENCY_CYCLES.md#owner-decisions)). The direction as first
given (2026-09-28):
1. Package dependency cycles are forbidden.
2. A module stays one file with explicit `use` lines. A file import cycle
   is allowed only when every file in it is in the same folder. The
   compiler finds each group of files that import each other in a loop
   (a strongly connected component), and a group spanning folders is an
   error that prints the loop.
3. The folder-level dependency graph must also be acyclic (confirmed
   again 2026-09-29, DEPENDENCY_CYCLES DC2 final).
4. Mutually recursive declarations inside one file stay allowed, and cyclic
   run-time values are unaffected.
5. Shared items that children need go in a leaf such as `common.hd` or
   `types.hd`, which children import instead of their facade.

The apply pass answered the design points (#40): initialization inside a
loop follows Go's rule, test code makes no folder edge, and the code is
`folder-cycle`. Entry modules inside a loop and multi-file fixtures are
listed in the record's [Still Open](DEPENDENCY_CYCLES.md#still-open).

### Casts, Property Discards, Type Names As Values, Std Scope

**Follow-ups decided (owner, 2026-09-29).**
- A float-to-integer cast out of range **saturates**, like Rust `as` since
  1.45: it clamps to the target's minimum or maximum, and NaN becomes 0.
  There is no panic.
- Same-width sign changes wrap, and a negative literal cast to an unsigned
  type stays `unsigned-negation`.
- A saved regression stream is replayed first, doesn't count toward
  `cases`, and its file is kept after the test passes.
- `@derive` on a newtype whose base lacks the trait uses
  `derive-field-missing-trait`, not `missing-partial-eq`.
- A type alias used as a value is a prototype gap, not a spec question.

**Follow-ups applied 2026-09-29.** Saturation is
[`types.cast.saturate`](../spec/04-type-system.md#r-types.cast.saturate),
[`types.cast.float-int-saturate`](../spec/04-type-system.md#r-types.cast.float-int-saturate)
and [`types.cast.no-panic`](../spec/04-type-system.md#r-types.cast.no-panic),
which retire `types.cast.float-int` and `types.cast.float-int-panic`. The
newtype code is
[`trait.derive.newtype.requires.error`](../spec/09-traits.md#r-trait.derive.newtype.requires.error).
The other three follow-ups keep the applied behavior. Points this pass met:

| Question | Applied now | **Recommendation** |
| --- | --- | --- |
| Which line a newtype's missing-trait error names: the `@derive` line or the `type` line | The base type on the `type` line, as a derived field's error is at the field | Keep: the base type is the newtype's one field. |
| `@derive(Debug)` on a newtype whose base lacks `Debug` | The rule covers it; the prototype reports it since 2026-09-29 | A prototype gap, not a spec question. |

**Decided (owner, 2026-09-28); applied 2026-09-28.** The rules are
[`types.cast.wrap`](../spec/04-type-system.md#r-types.cast.wrap) and
[`types.cast.literal-range`](../spec/04-type-system.md#r-types.cast.literal-range)
(item 1), [`module.testing.prop.discard`](../spec/10-modules.md#r-module.testing.prop.discard)
and [`discard-limit`](../spec/10-modules.md#r-module.testing.prop.discard-limit)
(item 2), [`names.type-as-value`](../spec/03-names-and-scopes.md#r-names.type-as-value)
(item 3), [`trait.derive.field-missing-trait`](../spec/09-traits.md#r-trait.derive.field-missing-trait)
and [`trait.derive.bound-unmet`](../spec/09-traits.md#r-trait.derive.bound-unmet)
(item 5), [`module.testing.prop.debug`](../spec/10-modules.md#r-module.testing.prop.debug)
(item 6), [`module.testing.prop.regression-file`](../spec/10-modules.md#r-module.testing.prop.regression-file)
(item 7), and [`trait.debug.derive-builders.mixed`](../spec/09-traits.md#r-trait.debug.derive-builders.mixed)
(item 8). Item 4 reworded four rules without changing behavior.

1. **Narrowing integer casts wrap** at run time, as Go conversions and
   Rust `as` do: `u8(x)` with `x = 300` gives 44. This replaces
   `types.cast.range-check` and `types.cast.out-of-range`. A literal
   argument that is out of range stays a compile error, as Go reports a
   constant overflow. Checked conversion remains a library function
   returning `Result` (`types.cast.fallible`).
2. **`assume` discards are counted separately.** They don't count toward
   `cases`, and a property test fails when discards exceed 10 times
   `cases`, like Hypothesis's `filter_too_much`.
3. **Using a type name as a value** (`let x = User`) is a new spec/03 rule
   with the code `type-used-as-value`.
4. **Std scope:** see AGENTS.md "Spec Scope For The Standard Library".
   `Set[T]` and a default hasher are not declared in the spec. Reword the
   two `Set[T]` examples (`trait.derive.newtype.self-error`,
   `annot.bound.more`) to use `Map`. Reword the "standard `Hasher`" rules
   (`req.determinism.hash-seeded`, `trait.derive.hash.seeded`) to describe
   hash seeds the runtime provides, without naming a std type.
5. **Derivation codes:** one generic code, `derive-field-missing-trait`,
   names the trait and the field, together with `missing-derived-bound`.
   There is no per-trait code: the fixtures expecting `field-not-eq` and
   `field-not-hash` move to the generic code.
6. **`it_prop` and `it_prop_with` require `T < Debug`,** so a failure
   prints the shrunk input.
7. **Regression files** under `__regressions__/<module>/<test-slug>` store
   the shrunk choice stream, one number per line. It is replayed first on
   the next run.
8. **`@derive(Debug)` on a mixed variant** such as `Mixed(i32, label:
   string)` uses the struct builder, with positional fields named `_0`,
   `_1` and so on: `Mixed { _0: 1, label: "x" }`.

**Questions from applying items 1 to 8.** The follow-ups above answer
each one (2026-09-29).

| Question | Applied now | **Recommendation** |
| --- | --- | --- |
| Item 1 says "narrowing"; does a same-width sign change wrap too, as `u64(x)` with `x: i64 = -1`? | Every integer-to-integer cast wraps, so that gives the `u64` maximum, as in Go and Rust | Keep: both cited languages treat every integer conversion alike. |
| A float-to-integer cast out of range, such as `i8(300.0)` | Panicked (`types.cast.float-int-panic`, retired 2026-09-29 for saturation); the category is unnamed (audit F-253) | Keep the panic, and name `integer-overflow` as F-253 recommends. Rust saturates instead ([reference](https://doc.rust-lang.org/reference/expressions/operator-expr.html#numeric-cast)); Go leaves the value implementation-dependent ([spec](https://go.dev/ref/spec#Conversions)). |
| The code for an out-of-range literal cast argument | `integer-literal-range`, and `u8(-1)` gets the negated-literal error `unsigned-negation` | Keep: the literal is checked as under an expected type. |
| Whether a replayed regression stream counts toward `cases`, and what happens to a saved stream that passes | It runs before new cases and does not count; the file stays | Keep, as proptest keeps its regression files ([docs](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html)). |
| A newtype whose base type lacks a derived trait, as `@derive(Eq) type Wrapped(Opaque)` | Not changed: the prototype reports `missing-partial-eq` on the `@derive` line | Use `derive-field-missing-trait` there too, naming the base type, since `trait.derive.newtype.requires` says "as a derived field must". |
| A type alias name used as a value, as `x := Id` for `type Id = i32` | The rule covers it; the prototype expands aliases before checking and reports `unknown-name` | Keep the rule; the prototype gap is recorded in `src/README.md`. |

### Bound And Row Operators

**Decided (owner, 2026-09-28); items 1 to 4 applied 2026-09-28.** This
supersedes the 2026-09-27 comma-list row decision (e176f5d). The spec text
is in [Multiple Bounds](../spec/02-grammar.md#multiple-bounds),
[Requirement Clauses](../spec/02-grammar.md#requirement-clauses),
[Row Operators](../spec/02-grammar.md#row-operators), and
[Least Row Solutions](../spec/11-requirements-and-suspension.md#least-row-solutions).

1. Multiple trait bounds use `&`, which means both at once (an
   intersection), in every bound position: generic parameters
   (`T < Eq & Hash`), supertrait lists (`trait Ord < Eq & PartialOrd`) and
   impl bounds. Precedent: Java `<T extends A & B>`, TypeScript and Scala 3
   `A & B`, Swift `P & Q`. Bounds appear only in type positions, so bitwise
   `&` in expressions doesn't conflict.
2. Requirement rows use `+` again: `fn f() -> O $ Db + Clock`. The
   parenthesized row form `$(A, B)` is dropped too, so row type arguments
   and function types use the same bare form, as in `Task[T, $ A + B]` and
   `fn() -> void $ A + B`. (The owner confirmed this on 2026-09-28.) The 2026-09-27 reason for commas
   was to keep `+` for bounds only. Moving bounds to `&` frees `+`, so each
   operator has one meaning.
3. One spelling only. The old forms are errors with a fix-it hint:
   `$ A, B` is `old-row-separator` (write `A + B`), and `T < A + B` is
   `old-bound-operator` (write `A & B`). These replace the existing
   `old-row-operator` code.

4. **Provider-installing functions (decided 2026-09-28).** Rows still have
   no subtraction. A function that discharges a key extends its callback's
   row, as in `fn gen[R](f: fn() -> () $ R + Clock) -> fn() -> () $ R`.
   Relaxed rule: a callback whose row lacks the extra key also matches,
   with `R` set to its own row. Needing fewer requirements is always safe,
   so it is no longer a `type-mismatch`. Callers never write `R`: it is
   inferred at each call.
5. **No row subtraction, confirmed (2026-09-28).** Two generic rows work
   with `+` alone. `R1 + R2` from independent arguments is a union. A
   generic key or bundle fixed by another argument, as in
   `provide[T, K, R](provider: K, f: fn() -> T $ R + K) -> T $ R`, is
   solved by the least-row rule. A pattern with two unknown rows, such as
   `f: fn() $ R1 + R2`, has no single best solution; Koka forbids it too.
   Candidate rule for the apply pass: each row pattern has at most one
   unknown row variable. Adding `-` later would be backward compatible.

```text
trait Ord < Eq & PartialOrd: ...
fn f[T < Eq & Hash](x: T) -> void $ Db + Clock: ...
```

**Questions from applying items 1 to 4.** Each needs an owner answer; the
spec states the current behavior.

| Question | Applied now | Recommendation |
| --- | --- | --- |
| The empty row, once `$(A, B)` is gone | `$()` stays the empty row, in headers, `Fn[..., $()]`, and `$.Context[$()]` | Decided: [RU7](REQUIREMENT_REUSE.md#owner-decisions) keeps `$()`. |
| Does `$ A + B` inside `[...]` or a parameter list need precedence rules? | A row ends at the first `,`, `)`, or `]`; nested function types keep the innermost-owner rule | None needed. No ambiguity was found in type arguments, parameters, `$.Context[...]`, or closure headers. |
| Codes for other old row spellings | `$(A + B)`, `$(A)`, a `-` between keys, and the pre-2026-09-27 `Job[A + B]` and `$.Context[A + B]` are `syntax-error` | Keep `syntax-error`. Only the two decided codes carry fix-its. |
| Item 5's rule, at most one unknown row variable per pattern | Applied 2026-09-28 as [RU4](REQUIREMENT_REUSE.md#owner-decisions): [`req.row.least.ambiguous`](../spec/11-requirements-and-suspension.md#r-req.row.least.ambiguous), code `ambiguous-row-pattern` | Done. |
| `$.Context[$ A + B]` keeps its inner `$`, while one key is `$.Context[A]` | Kept: the context type takes a key or a row type argument | Keep it. It matches row type arguments such as `Job[$ A + B]`. |

[REQUIREMENT_REUSE.md](REQUIREMENT_REUSE.md) still names the comma
spelling in its prose. It is a stress report whose options compare row
spellings, so it was left as written.

### Replay Determinism And Durable Workflows

**Decided.** Every question in
[Durable Replay](DURABLE_REPLAY.md#owner-decisions) is decided (decisions 1
to 15). The runtime rules are in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules), and the determinism
clause is in
[Runtime Boundary](../spec/11-requirements-and-suspension.md#runtime-boundary).

**Status.** The replay experiments in the
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) predate the
decided rules: their identity is per function rather than per program, their
site IDs contain byte offsets, and they stop at the end of a history instead
of resuming.

### Mutable Host Providers

**Decided and applied 2026-09-27: access follows from the trait.** A
requirement is one provider shared by its whole call tree, so no row or call
tracks its access with `mut`. A requirement trait that declares or inherits a
`mut self` method is a mutable requirement trait: `$.use(K)` yields `mut K`,
a `$.with` or `$.context` binding for it needs a `mut` value
(`mutable-upgrade` otherwise), and a runtime profile binds its host provider
mutable. Every other provider is readonly. `$ mut K`, `$.use(mut K)`, and
`mut K=value` are `syntax-error`, and profiles no longer mark traits. The
rules are in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers).
This supersedes the `$ mut K` rows of [STDLIB decision 14](STDLIB.md#owner-decisions)
and the same-day "always mutable" answer.

**Decided and applied 2026-09-27: `Console.write_line!` takes `mut self`**
(option 2 of the former question). `Console` is therefore a mutable
requirement trait, and a recording [`BufferConsole`](STDLIB.md#stdconsole)
appends through it. `println` and its `$ Console` row are unchanged. Code
that keeps the console in a local writes `let console: mut Console`, since a
`:=` binding stays readonly; no new rule was added. The rule is
[`module.console.write-line-mut`](../spec/10-modules.md#r-module.console.write-line-mut).
The other options were keeping `self`, so a recording console was not
expressible, and letting a `:=` binding of `$.use(K)` keep mutable access.

```text
fn greet!() -> void $ Console:
    let console: mut Console = $.use(Console)
    _ := console.write_line!("hi")
```

**Decided and applied 2026-09-27: two tasks may share one mutable
provider,** with no new rule. Children of `std.task.all!` in one provider
scope retrieve the same `mut K`, and each sees the others' changes between
its `!` calls; code that needs isolation installs a provider per task. The
spec states this as a note in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers),
beside a second note: adding a `mut self` method to a published requirement
trait is a breaking change
([Packages](PACKAGES.md#33-the-checked-compatibility-rule)).

**Raised by the prototype pass (2026-09-28), now decided below.** The prototype checks
`Console` as a prelude trait. It runs direct `write_line!` calls on the host
console and on a program-defined provider, such as the toy
`std.console.BufferConsole`. `println` still writes only through the host
console: when it reaches a program-defined provider, the run stops with
`unsupported-console-provider`, because the spec leaves two questions open.

1. **How `println` calls `write_line!`.** `println` is non-suspending
   (`-> void $ Console`), but `write_line!` suspends, so a recording
   `BufferConsole` has no specified way to receive the line. Options: drive
   the call to completion inside `println`, as `block_on` does, with a
   pending host suspension a panic; or make `println` suspending
   (`println!`). **Recommendation:** drive it to completion, since Rust's
   `println!` and Go's `fmt.Println` are synchronous and every caller keeps
   its signature.
2. **What `println` does with `.Err(ConsoleError)`.** The result is dropped
   today. Options: ignore it, or panic. **Recommendation:** panic, as Rust's
   `println!` does when writing to stdout fails; code that must handle the
   error calls `write_line!` itself.

```text
fn greet() -> void $ Console:
    println("hello")   # which write_line! runs, and what if it fails?
```

**Decided and applied (owner, 2026-09-28).** Both recommendations are accepted.
1. `println` drives `write_line!` to completion inside itself, as
   `block_on` does, and stays non-suspending. A pending host suspension
   there is a panic.
2. `println` panics when `write_line!` returns `.Err(ConsoleError)`. Code
   that must handle the error calls `write_line!` directly.

**Decided, second round (owner, 2026-09-28).**
- `println` is exactly `block_on(write_line!(...))` plus the `.Err` panic.
  A pending host write is driven until it finishes, as `block_on` does.
  The earlier "a pending host suspension is a panic" rule is dropped.
- `println`'s own panic is an ordinary std `panic` call, so its category
  follows the general rules (`explicit-panic`).
- `println` at the top level of an entry-module script (REPL, playground)
  is valid: no driver is running there, and only non-entry module
  initialization bans `block_on`.

**Second round applied (2026-09-28)** in
[`module.console.println-drive.block-on`](../spec/10-modules.md#r-module.console.println-drive.block-on),
[`module.console.println-drive.pending`](../spec/10-modules.md#r-module.console.println-drive.pending),
[`module.console.println-error.category`](../spec/10-modules.md#r-module.console.println-error.category),
and
[`module.console.println-script`](../spec/10-modules.md#r-module.console.println-script).
`module.console.println-drive` and `module.console.println-pending` are
retired. The prototype's `block_on` now polls a pending host write again
until it finishes. It infers no script entry requirement row, so the
top-level fixture is a known failure tagged MHP-1.

**Decided follow-ups (owner, 2026-09-28).**
1. `println` is an ordinary std prelude function. Its panics are ordinary
   panics raised by std code, with a message std defines. The language adds
   no panic category for them.
2. `ConsoleError`'s variants and constructor are settled together with the
   other std error types.
3. `println` inherits all of `block_on`'s rules. That includes the panic
   when another driver is already running, and the ban in `defer` suites
   and default expressions. So `println` inside `main!` or a test body
   (both run under a driver) panics. Suspending code writes with
   `$.use(Console).write_line!` instead. The owner confirmed this
   consequence.

**Decided (owner, 2026-09-28): option 1, keep today's behavior.** Console
calls stay out of the record, and a replay writes the lines again.

**Was open (raised by the prototype, 2026-09-28): should a replay capture
console output?** The host console now goes through the same host
capability bridge as every other host provider, and that bridge records
each host call for replay. The prototype leaves `Console` out of
recording, which keeps today's behavior: `hd record` stores no console
lines, and `hd replay` writes each line again to the live console.

1. Keep console calls out of the record, so a replay prints again.
2. Record console calls like any provider, so a replay checks the written
   text against the record and prints nothing.
3. Record them and also print during replay, so a replay both checks and
   shows the output.

**Recommendation:** option 1 for now. Console output is an effect the
replayed program repeats rather than an input it reads, and `write_line!`
returns no data the program depends on. A mismatch check (option 3) can be
added when [durable replay](DURABLE_REPLAY.md) specifies effect logging.

```text
pub fn main!() -> Result[void, ConsoleError] $ Console:
    let console: mut Console = $.use(Console)
    console.write_line!("step 1")?  # recorded, or written again on replay?
    .Ok()
```

The rules are
[`module.console.println-write`](../spec/10-modules.md#r-module.console.println-write)
through
[`module.console.println-script`](../spec/10-modules.md#r-module.console.println-script).

The applying pass of MHP-1 raised three questions: the panic category,
constructing a `ConsoleError`, and whether `block_on`'s restrictions
carry over. The decided follow-ups above answer them: follow-up 1 answers
the first, follow-up 2 defers the second to the standard library's error
types, and follow-up 3 answers the third.

**Follow-ups 1 and 3 applied (2026-09-28)** in
[`module.console.println-std`](../spec/10-modules.md#r-module.console.println-std)
through
[`module.console.println-block-on.contexts`](../spec/10-modules.md#r-module.console.println-block-on.contexts).
`lib/std/console.hd` drives `write_line!` with `std.task.block_on` and
raises the error panic with `panic`, so the checker has no `println`
case. Fixtures, examples, tests, and the guides that called `println`
under a driver now write with `$.use(Console).write_line!` or run from a
non-suspending `main`. Applying them raised three readings: the panic
category, the pending-host panic, and top-level `println`. The second
round above answers all three.

**Still open after the second round (raised 2026-09-28).** Nothing here is
decided:

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| A fixture for a pending host write | No [runtime profile](../spec/conformance/README.md#runtime-profiles) holds a `write_line!` pending and then completes it: `console` is ready on the first poll, and `pending-gate` never completes. So `module.console.println-drive.pending` and `block_on`'s own wait have only a prototype unit test. | Add a conformance profile whose gate is pending on its first poll and ready on the next. |
| A fixture for the `.Err` panic | `module.console.println-error.category` has no fixture, since no code can build a `ConsoleError` (follow-up 2 defers its constructor). | Add the fixture when `ConsoleError`'s constructor is settled. |

```text
fn report!() -> void $ Console:
    defer:
        println("done")   # error: suspension-forbidden-context
    println("working")    # panics: suspension-nested-driver under a driver
```

### Typed Derivation, Tool Adapters, And Secrets

**Decided.** Owner decisions M1-M29 in
[Typed Derivation: Survey And Design Options](TYPED_DERIVATION.md) are
applied in [Typed Derivation](../spec/14-annotations.md#typed-derivation);
M26 replaced the `annotate` block with the
[trait-less derivation block](../spec/14-annotations.md#trait-less-derivation-blocks),
and M28 settled its generic header, one block per type, the `Self` line
warning and list-typed member lines; M29 settled renamed parameters, non-list
right sides, and per-trait `Self` lines.
The prototype implements M1-M29 by lowering; its gaps are rows of
`test/portable/KNOWN_FAILURES.tsv`, mostly `K1` (the shape intrinsics),
`F-250`, and one `M29` fixture that needs package roles. The questions
the prototype pass
raised are in
[Still Open](TYPED_DERIVATION.md#still-open-after-the-prototype-pass), and
those the M27 and M28 apply pass raised are in
[Still Open After M27 And M28](TYPED_DERIVATION.md#still-open-after-m27-and-m28). Error derivation is the separate
`@error` intrinsic
([Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions)).

**Open questions.** The spec lists these as
[undecided parts](../spec/14-annotations.md#undecided-parts); each waits
for the owner:

| Question | What is undecided |
| --- | --- |
| Fact check hook (M15) | The form of a fact type's compile-time `check`, and whether it covers cross-member and type-level checks (round 2 R8). |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| Plan constants (M21 R3-4) | The declaration and reference syntax of a template's compile-time constant, and the evaluator's limits. |
| Typed shared constants (M21 R3-7) | Typed handles for shared constructor data, beyond `(name, Any)` pairs. |
| `T -> U` mapping (M14) | Whether derivation between two types is in scope. |
| Name clashes (round 2 R13) | How `walk`, `describe`, and `build` interact with trait methods of the same name. |
| Derived bound (round 2 R14) | Whether a derived bound names the trait or the walker's strengthened bound, where they differ. |
| `default()` allocation (round 2 R15) | Whether `h.default()` may allocate for every member type. |
| Composing templates (round 1 P16) | How a wrapper walker forwards to an inner walker's `member`. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library ([STDLIB](STDLIB.md#clone)). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library ([STDLIB](STDLIB.md#derived-function-cache)). |
| Function targets | Deriving for functions, as tool adapters need ([FN_TYPE](FN_TYPE.md) questions 9 and 10). A decorator before a function attaches a plain value that `shape_of(f).metadata[M]()` reads ([Decorators](DECORATORS.md#owner-decisions) D1 and D7). |

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
([STDLIB decision 12](STDLIB.md#owner-decisions), 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([FN_TYPE decision 9](FN_TYPE.md#owner-decisions)) and no stable code
identity, serializable capture contract, cache invalidation rule, or
graph-lifetime mechanism. Since `mut fn` was removed, a function type also
does not say whether a callback mutates its captures, so an incremental
computation cannot demand a write-pure callback through its type.

**Options.** (1) Use a content hash for code identity, require a `Durable`
capture bound, and reject captured providers or mutable state. (2) Require
explicit user IDs and an explicit capture record. (3) Keep closures
process-local and expose only named registered computations.

**Recommendation.** Begin with option 3 for a small dependable surface, then
adopt option 1 when durable replay identity is settled. Provide weak references
inside the standard runtime, or explicit disposal, for incremental graph
nodes; user-visible finalizers are ruled out
([Durable Replay decision 12](DURABLE_REPLAY.md#owner-decisions)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values ([Durable Replay decision 11](DURABLE_REPLAY.md#owner-decisions)
replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
([Durable Replay decision 14](DURABLE_REPLAY.md#owner-decisions)).

**Options.** (1) Carry task-local storage in `PollContext`, with hooks at
compiler-generated adapters for registered boundaries plus host-boundary
events. (2) Model tracing only as explicit requirement providers. (3) Let
hosts instrument Wasm calls without language-level correlation.

**Recommendation.** Option 1, while keeping exporters and policy behind
ordinary providers. The hook must honor `Secret[T]`/`Redact` once defined.

**Unblocks.** Trace propagation across suspension, workflow event correlation,
structured metrics, and enforceable redaction.

**Decided 2026-09-27, not yet applied: option 1.** The runtime carries trace
context task-locally in the poll context; hooks fire at host-boundary calls
and suspension points; exporters and policy stay ordinary providers. The
redaction clause waits for `Secret[T]`, which is removed for now.

### Access Control And Tenancy Expressibility

**Problem.** Requirement rows show which service is reachable, not the
principal, tenant, delegation, or attenuation under which it is used.

**Direction.** Access control and tenancy are modeled in hd-lang code, such as
requirement traits, provider values, and library types, rather than by
dedicated language features. The concrete library design is deferred.

**Open question.** Whether the current language can express the needed
patterns without new features: an explicit principal requirement, attenuated
provider views such as `db.for_tenant(tenant)`, delegation, and redacted
output. A worked tool example should show authentication, principal lookup,
tenant attenuation, a database call, and redacted output. Any gap it exposes
becomes a separate language issue.

**Unblocks.** Multi-tenant tools, least-privilege review, delegated authority,
and access-control testing.

**Status.** Deferred 2026-09-27 until the core specification settles; the
redacted-output part also waits for `Secret[T]`.

### Scope Reduction And Distinctive Requirements

**Problem.** GADTs, declared variance,
variadic generics, much of the permission system, loop-`else` values, binding
expressions, trailing blocks, and the multiple-closure form carry substantial
implementation and teaching cost without demonstrated product requirements.
Conformance effort is concentrated away from requirement/suspension behavior.

**Options.** (1) Keep the surface and demand motivating end-to-end examples and
proportionate conformance coverage. (2) Move GADTs out, make user generics
invariant, replace packs with homogeneous task lists, simplify `mut`, preserve
mutable freshness for new suspensions, and forbid `return` in trailing blocks.
(3) Define a smaller initial language profile while retaining the designs as
documented experiments.

**Recommendation.** Option 2 unless a concrete requirement justifies each
feature. Select two distinguishing outcomes—recommended: durable replay and
per-tool requirement reports—specify them end to end, mark the rest as initial
release non-goals in `USE_SCENARIOS.md`, and add chapter 11 fixtures before
expanding other chapters.

**Unblocks.** A smaller compiler, clearer teaching material, faster
conformance, and evidence that complexity serves the language thesis.

**Status.** Closed 2026-09-27: every feature stays. The first
[Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) deliberately does
not implement these features, and they are implemented soon after it rather
than removed. The recommendation above relied on per-tool requirement
reports, which the owner dropped.

### Annotation Locality And Inspection

**Status.** Superseded 2026-09-27. Typed derivation removes `Annotation` and
`Annotate` ([TYPED_DERIVATION](TYPED_DERIVATION.md) rule 10), derivation
blocks live in the type's module, a foreign type is derived through a local
mirror type or newtype, and the root-application orphan exception is
dropped. So there are no foreign-target annotations to place or mark, and
TQ-18's marker question is moot. The text below is kept as history.

**Problem.** The effective annotation for a target can be assembled
across distant files and dependencies, making review and provenance difficult
even with global coherence.

**Direction.** An annotation for `X` must be declared in `X`'s defining module,
with an explicit form for foreign targets. The compiler provides a command that
prints the complete materialized `Info` plan and its provenance for a target
without executing effectful code. Until the rule is specified, the existing
package-global placement rules in Annotations (since removed) stay in
force.

**Open questions.**

1. Where a foreign-target annotation may appear. Candidates: the facet's
   defining module, mirroring the trait orphan rule at module granularity, and
   the root application package's existing orphan exception.
2. How a foreign-target annotation is marked. Candidates: no marker when the
   facet's module declares it, and an explicit marker only for the root
   orphan case, or one marker for every foreign target.
3. Whether the locality rule applies equally to an explicit
   `impl Annotate[Facet] for Target`, which occupies the same coherence slot.
   Without that, the rule could be bypassed.

**Unblocks.** Local review, understandable generated schemas, annotation
debugging, and safer dependency upgrades.

### Runtime Type Identity And `reified`

**Status.** The direction is specified in
[Runtime Type Identity](../spec/09-traits.md#runtime-type-identity): the
sealed `std.inspect.Inspectable` with `runtime_type() -> TypeId` and the
default methods `downcast` and `downcast_mut` (bounded by
`T < AnyRef & Inspectable`, no `reified`), `TypeId::of[T]()`,
`downcast_val` for value types, exact matching in which an inner `mut`
counts and the outer `mut` is ignored, generic erasure through
`T < Inspectable`, `inspectable-requirement`, and
`std.error.Error < Display & Inspectable`.
[INSPECTABLE.md](INSPECTABLE.md) keeps the design record and owner decisions
1 to 16.

Two former items are now specified. Static (receiverless) calls resolve
through a bound, as in `T::create()` with `T < Factory`, and never through a
`TypeId` (TQ-9,
[Associated Function Calls](../spec/09-traits.md#associated-function-calls)).
`TypeShape` has `Trait(decl, args)` and `Any` cases (TQ-23,
[Common Shape Representation](../spec/14-annotations.md#common-shape-representation)).

What remains open is the checked-downcast option in
[Typed Derivation](#typed-derivation-tool-adapters-and-secrets).

**Unblocks.** Typed reflection that recovers a concrete value from an erased
one.

### Confirmed Deferred Type Features

**Problem.** Two surfaces remain intentionally unsupported and must be
diagnosed: first-class bound methods, and direct permission weakening combined
with generic variance. Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)).

**Direction.** Keep bound methods and weakening with variance deferred, and
design each only with a motivating requirement. Bound methods must settle
receiver capture; weakening with variance must preserve representation. The
spellings `Type::name` (the unbound method function, receiver first) and
`x::name` (the bound method value) are reserved for them and diagnosed today
as `deferred-method-value`
([Unsupported Function Extensions](../spec/07-functions.md#unsupported-function-extensions)).
Negative implementations and additional pack operations are likewise confirmed
future work rather than implicit extensions.

**Unblocks.** Implementer certainty today and a checklist for future proposals.

### Associated Type Bindings Beyond Direct Bounds

**Problem.** An associated type binding such as `I < Supplier[Item = T]`
names only an associated type the bound trait itself declares, and it is
accepted only in generic parameter bounds and supertrait lists. Binding a
supertrait's associated type through a subtrait
(`I < NamedSupplier[Item = T]`) is rejected, and dynamic trait value types
and `impl` headers take no bindings.

**Options.** (1) Keep the current rule; users add a separate bound on the
supertrait. (2) Let a binding name any associated type reachable through the
supertrait graph, rejecting ambiguous names. (3) Also accept bindings in
supertrait lists, so `trait Names < Supplier[Item = string]` fixes the item
type for every implementation.

**Recommendation.** Option 1 until a library needs option 2.

**Option 3 decided and applied (2026-09-29)** by
[Operator Traits](OPERATOR_TRAITS.md#owner-decisions) OP8: a supertrait list
binds associated types, as in `trait Summable < Add[Self, Out = Self]`
([Supertrait Bindings](../spec/09-traits.md#supertrait-bindings)). Option 2
stays open.

**Unblocks.** Shorter bounds for trait hierarchies with associated types.

### Resource Non-Escape And Cleanup Policy

**Problem.** Block-scoped `defer` provides deterministic synchronous cleanup on
ordinary control-flow exits and cancellation, but it does not stop a handle
alias from escaping into a global, field, closure, or suspension. The language
also has no settled policy for asynchronous or fallible cleanup.

**Options.** (1) Add a compiler-recognized `NonEscapable` locality category,
propagate it through containers and captures, and use `defer` at the cleanup
boundary. General dependent returns would also need provenance rather than only
a binary marker. (2) Add affine/owned handle types with borrow checking. (3)
Add a scoped callback protocol whose handle cannot escape. (4) Keep unrestricted
aliasing and rely on checked `ResourceError.Disposed` results.

**Direction.** Option 1 is chosen, with `defer` as the cleanup mechanism.
A suspension frame may hold a `NonEscapable` value across a suspension point;
the frame is then itself non-escapable (Shape B in
[Ownership and Escape Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md#shape-b-kotlin-style-locality-plus-a-suspension-exception)).
Still open: the propagation rules, dependent-return provenance, whether
provider values can be `NonEscapable`, and asynchronous or fallible cleanup.
Retain checked disposal errors.

**Unblocks.** Leak-resistant files/sockets, safe cancellation, fallible cleanup
design, stronger sandbox guarantees, and possibly complete per-tool authority
reports: provider values are ordinary values that may escape today, and a
`NonEscapable` provider category is the likely way to close that gap.

### Error Entry-Point Follow-Ups

**Superseded 2026-09-27 by [Testing T8](TESTING.md#owner-decisions), and
applied.** Exit codes follow Rust's model: `std.process` declares
`type ExitCode(u8)` and `Termination`, whose `report` gives `main`'s or a
test's exit code
([Exit Status](../spec/10-modules.md#exit-status)). A failing
`Result[void, E]` prints the error chain
([Entry Results](../spec/10-modules.md#entry-results)) and exits with 1; a
program that wants another code returns `ExitCode` or `Result[ExitCode, E]`.
[Error conversion decisions](ERROR_CONVERSION.md#owner-decisions) 13 and 14
still hold; 16 and 17 are superseded by Testing T4 and T8.

The four questions of this entry are closed:

| Question | Earlier answer | Now |
| --- | --- | --- |
| 1: the status type | `StatusCode`, a `u8` wrapper that is never 0 | removed; `ExitCode(u8)` holds any `u8`, and 0 means success |
| 2: `ExitStatus` on an erased `Error` | the static error type decides | removed; no error type picks a code |
| 3: how `StatusCode` keeps out 0 | open | moot: no zero check |
| 4: spelling a constant status | open | `ExitCode(2)` |

```text
use std.process.ExitCode

pub fn main() -> ExitCode:
    ExitCode(2)
```

### Literal Suffixes

**Decided and applied (2026-09-28 and 2026-09-29).** Owner decisions
L1-L22: a suffix is a function marked `@num_suffix` (L11, with the names of
Decorators D9), imported by `use`, and `250ms` calls `ms(250)`
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes)). Only
decimal and float literals take a suffix, `5else` is `invalid-token`, a
suffixed literal is plain call sugar, the test `timeout` option takes any
`Duration`, and `Duration` is whole milliseconds with the suffixes
`ms s min h`. `sql"a $x"` calls a function marked `@str_prefix` with a
`std.ops.Template`, and `r"..."` is the std prefix `std.text.r` (L20)
([Prefixed Strings](../spec/05-expressions.md#prefixed-strings)). A marked
function declares exactly one parameter, checked at its definition (L21,
L22).

**Open.** Six points, each with a recommendation, are in
[Literal Suffixes](LITERAL_SUFFIXES.md#still-open): how `$` and `\` behave
in a prefixed string (15-17), a prefixed test name (23), a defaulted
suffix parameter (28), and the line a shape error names (29).

**Raised by the docs sweep (2026-09-29).** Nothing here is decided.

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| Is `$5` inside a prefixed string text? [`lex.prefix.plain-dollar`](../spec/01-lexical-structure.md#r-lex.prefix.plain-dollar) makes a `$` text when "neither an identifier character nor `{`" follows. A digit continues an identifier but cannot start one, which [`lex.interp.stray-dollar`](../spec/01-lexical-structure.md#r-lex.interp.stray-dollar) spells out for interpreted strings. | [Learn in 10 Minutes](../guide/LEARN_IN_10_MINUTES.md#bindings-and-values) writes `r"\d+ costs $5"`, and the reference parser and the prototype read `$5` as text. Read literally, the rule leaves it neither text nor a name. | Say "a character that can start an identifier", as `lex.interp.stray-dollar` does, so `$5` stays text. |

```text
use std.text.r

price := r"\d+ costs $5"   # text: a digit cannot start a name
```

### Operator Traits

**Decided direction (2026-09-28).** hd plans operator traits in `std.ops`
(Rust's model: `Add`, `Sub`, `Mul`, `Div`, `Neg`, comparison already via
`Eq`/`Ord`), so library types such as `Duration` support `5s + 3s` and `-d`.
Until OP1-OP9, hd had no operator overloading. `std.ops` already held the
literal-suffix marker `NumSuffix`
([Literal Suffixes](LITERAL_SUFFIXES.md#owner-decisions) L5, L11).

**Decided and applied (2026-09-29).** Owner decisions OP1-OP9 in
[Operator Traits](OPERATOR_TRAITS.md#owner-decisions): Rust-shaped
`std.ops` traits (`Add[Rhs]` with an associated `Out`) for twelve
operators, std implementations for the primitive numbers, `AddAssign`-style
compound assignment, `Index` and `IndexSet`, supertrait `Out` bindings, and
the sealed `std.num` traits `Num`, `Integer`, and `Float`
([Operator Traits](../spec/05-expressions.md#operator-traits),
[Compound Assignment](../spec/05-expressions.md#compound-assignment),
[Numeric Traits](../spec/09-traits.md#numeric-traits)). Comparison stays
on `Eq` and `PartialOrd`.

**Open.** Fifteen points from the apply pass, each with a recommendation,
are in [Operator Traits](OPERATOR_TRAITS.md#still-open): among them whether
`a += b` falls back to `a = a + b`, how primitives relate to the assign
traits, floating `%`, and the `Index` signatures.

### Decorators

**Decided and applied (2026-09-28 and 2026-09-29).** Owner decisions
D1-D10: a decorator is a plain compile-time value on any item or member,
`@annotate(.Fn)` on a fact type limits only the target kind, readers check
signatures, and a bare marker name such as `@num_suffix` is called
([Prefix Decorators](../spec/14-annotations.md#prefix-decorators),
[Target Kinds](../spec/14-annotations.md#target-kinds)). D10 adds the
`.Newtype` kind and the code `decorator-target-kind`.

**Open.** One point is in [Decorators](DECORATORS.md#still-open): `@error`
is an intrinsic (D6), but its rules wait for the Error Conversion apply
pass.

### Requirement Reuse

**Decided (owner, 2026-09-28 and 2026-09-29) and applied.** RU1-RU15 in
[Requirement Reuse](REQUIREMENT_REUSE.md#owner-decisions): row aliases, row
parameters on functions only, one unknown row variable per pattern, row
subsumption for function values, a bare row alias in a one-key slot, and
the union row at every common-type site. One point from the RU15 apply
pass, with a recommendation, is in
[Still Open](REQUIREMENT_REUSE.md#still-open).

### Iterator Adapters

**Decided (owner, 2026-09-29) and partly applied.** STDLIB question 14
makes iterator adapters default methods of the prelude `Iterator`, as in
Rust. `filter`, `take`, `enumerate`, and `collect` are in
[Iterator Adapters](../spec/06-control-flow.md#iterator-adapters).

**Open.** `map[U]` and `fold[A]` would make `Iterator` not dynamically
safe, so `mut Iterator[T]` could not be a value type. That point and six
`std`-only ones, each with a recommendation, are in
[STDLIB Still Open](STDLIB.md#still-open).

### Cross-Feature Stress Test (2026-09-29)

**Open.** [STRESS_2026_09_29](STRESS_2026_09_29.md#questions-for-the-owner) asks 11 questions across OP, L, D, RU, and DC, led by the `+=` fallback and a checked `from_i64`.

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([Durable Replay decision 12](DURABLE_REPLAY.md#owner-decisions));
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. The manifest shape, executable-main selection, caret
  version ranges, the PubGrub-style resolver, and the lockfile are decided in
  [Packages decisions 1-14](PACKAGES.md#owner-decisions) and drafted there;
  the root-application orphan exception for a package with both a library
  and executables (decision 4) is still open;
- dependencies through version control hosts, with no registry (owner,
  2026-09-28): identity, versions, resolution, and integrity are open in
  [Dependencies questions DP1-DP14](DEPENDENCIES.md#questions-for-the-owner);
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, [Durable Replay decision 15](DURABLE_REPLAY.md#owner-decisions));
- property-testing strategies, shrinking, replay artifacts, and correlated or
  stateful generators in `std.testing`;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, retry, timeout,
  and heterogeneous scheduling;
- the final `std.task` structured-scope API: `Task[T]` is decided as
  structured scopes only, with `scope!`, `start`, and `join!`
  ([STDLIB decision 11](STDLIB.md#owner-decisions)), and must not weaken
  one-shot `Suspend[T]` semantics;
- the complete standard host capability-trait catalog and provider
  configuration format;
- exporter configuration, sampling, storage, and operational privacy policy
  after the observability hook is designed; and
- which generated artifacts—JSON Schema, OpenAPI, MCP, clients, or
  documentation—ship first after typed derivation is resolved.

## Resolution Process

After resolving an item:

1. update the owning specification or design document;
2. remove or narrow the item here;
3. add valid and invalid conformance fixtures where applicable;
4. record the resolution in the owning document's history when applicable; and
5. run `spec/check.sh`.
