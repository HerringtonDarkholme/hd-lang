# Diagnostics From The Writing Log (`Q21`, 2026-10-08)

For every `no` / `partly` row of `audit/hd-writing-log.md` (117 rows:
48 no, 69 partly), the same mistake was retried against the new Rust
compiler (`compiler/target/release/hd`, current main) as a minimal
single-file repro (`hd FILE.hd`, or `hd test FILE.hd` for `tests:`
blocks). Report only; the compiler is untouched. Rows are grouped by
mistake and ranked by row count. "Helped now?" judges the NEW message
against the OLD complaint only.

## Improved or gone

| Mistake (rows) | Old message | New message | Helped now? | Spec rule | Proposed better message |
| --- | --- | --- | --- | --- | --- |
| `let mut` on primitives, 5 (`02_max`, slug lib, `sum` loop, `total: i64`, `shapes f64`) | `mut-on-primitive: ... no mutable state ...` | same code, same shape (`probe/shapes` wording: `takes no 'mut'`) | yes | `types.view` mut rules | keep; the wording already converges |
| `println` without `$ Console`, 3 (`M6 tests:`, `M6 main`, `#200 main`) | `missing-requirement: call to 'println' requires Console` | same code | yes | `module.init` entry rows | keep |
| contextual enum variants (`.Some`/`.Less`/`.Ok`), 3 (`missing<.Some`, `largest==.Some`, pair `(.Ok,.Ok)`) | `missing-contextual-enum-type: variant ... requires an expected enum type` | bare `.Some(1)` accepted with no diagnostic | yes | `types.enum.contextual` | keep; no message needed where none fires |
| `mut`-upgrade on fresh values, 1 (`Money::of`) | `mutable-upgrade: ... cannot be upgraded ...` | `mutable-upgrade: 'let mut' needs mutable access, but the value is a readonly M` | yes | `types.assign.mutable` | keep |
| big literals, 3 (`hi<=…`, `magnitude`, `threshold`) | `integer-literal-range: ... declare it i64 ...` (names `i64` even for `u64` contexts) | `integer-literal-range: 9223372036854775807 does not fit i32` (no wrong-width hint) | yes | `types.literal.range` | name the expected width from context |
| `trim` with an argument, 1 (words) | `argument-count: method 'trim' expects 0 arguments` | `argument-count: 'trim' takes 0 arguments` | yes | `cli`/`std` arg rules | keep |
| associated functions both spellings, 1 (`Tree.arbitrary`) | `unknown-method` on `Tree.arbitrary()` | `T.make()` and `T::make()` both print `1` | yes | `fn.ref` call forms | keep |
| qualified calls, 1 (`Add::add`) | `generic-arity: trait 'Add' expects 1 type arguments` | `A::f(D {…})` prints `1` | yes | `trait.resolve` qualified calls | keep |
| trait-signature detail, 1 (fake `Store`) | `trait-method-signature: method 'lookup' does not match` | `trait-method-signature: 'get' differs from the trait's declaration: the result type ...` | yes | `trait.impl.signature` | keep |
| `mut` date/literal compare, 3 (`day_of==Date`, `policy==Backoff`, plus `mut:Date` shape) | `type-mismatch: ... Date and mut:Date` (confusing) | `type-mismatch: D does not implement 'Eq' for this operand` (names the real gap) | yes | `cmp.eq` | keep |
| `@intrinsic` in user code, 2 (index, panic) | internal compiler error with stack trace | clean `syntax-error` at the attribute | yes | — (intrinsics are std-only) | keep rejecting, ideally naming `@intrinsic` as std-only |
| `hd test` runs `main`, 1 (calc) | main's output printed, count off by one | `1 passed`, main's `hi` not printed | yes | `cli.test` selection | keep |
| `hd run` bare, 1 (args-as-task) | string taken as a task name | `hd run` with no package: `` `hd run` works on a package, and no `hd.toml` is at or above here; create a package with `hd new` `` | yes | `cli.run` forms | keep |

## Same message, still partly helpful

| Mistake (rows) | Old message | New message | Helped now? | Spec rule | Proposed better message |
| --- | --- | --- | --- | --- | --- |
| mut-`self` method on readonly binding, 5 (filter, `next` in closure, `Tens.push`, `draws.int`, `Mailer.push`) | `mutable-receiver-required: ... requires mutable access ...` | same code (message names the need, never suggests `let mut`) | partly | `types.access.receiver` | append `write 'let mut name = ...'` |
| `List` `+`, 1 (`keys + [key]`) | `type-mismatch: ... needs std.ops.Add` | `unsatisfied-trait-bound: List[i32] does not implement std/ops/Add` (plus follow-on Display noise) | partly | `ops.add` | name `append` for lists |
| unknown `Map`/`List` methods (`insert`, `push`, `append`, `join`, `keys`), 5 (`m.insert`, `chars.push`, `word_list.append`, `lines.join`, `m.keys`) | `unknown-method: ... has no supported method ...` | same shape, still no did-you-mean | partly | std method tables | add did-you-mean (`push` for `append`, member list for unknown names) |
| `assert` without import, 1 (P1j) | `unknown-name: unknown function 'assert'` | `unknown-name: unknown-name 'assert'` | partly | `names.visible` | name `use std.testing.{assert}` |
| `Clock.now()` as value, 1 (P1j) | `type-used-as-value: 'Clock' names a type, not a value` | `unknown-name 'Clock'` (no import in a single file) | partly | `names.category` | with the import present, say `$.use(Clock).now()` |
| `read_text!` in a `use`, 1 (P1j) | `syntax-error: expected '}', found '!'` | `syntax-error` at the `!` | partly | `grammar.use` | say `!` marks calls, never imports: `import read_text, call read_text!(...)` |
| `cannot-infer-type` on `[]`, 4 (`Walk`, `Deque`, `Set`, `pieces`) | `cannot-infer-type: ... annotate the binding: 'let pieces: List[T] = ...'` | `cannot-infer-type: annotate this value's type` | partly | `types.infer.empty` | keep the suggested form but add the `mut` the later `push` needs |
| `(...[T])` spreads, `..=` values, 2 (`all!`, `to == ..=5`) | `expected-expression: expected an expression, found '...'` | `...` in a call is `syntax-error`; `..=5` as a value is accepted | partly | `expr.spread`, `types.range` | say where each form is legal (`...` postfix, `..=` needs a comparison) |
| `loop:` for endless loops, 2 (CLI probe, todo) | `unknown-name: unknown function 'loop'` | parses as a call, then `unsupported: Body: expression TrailingCallExpr` | partly | — (no `loop` construct) | reject at parse with `write 'while true:'` |
| `use` inside a function, 1 (probe 5) | `syntax-error: expected a line ending, found 'std'` | same `syntax-error` | partly | `module.use.position` | say uses go at the top of the file |
| one-line `if` in a match arm, 1 (probe 6) | `syntax-error: expected a line ending, found '=>'` | accepted, prints `1` | yes | `grammar.match.arm` | keep |
| `..=5` as a value, 1 (probe 6) | `expected-expression: expected an expression, found '..='` | accepted (unused warning) | yes | `types.range.value` | keep |
| top-level `LIMIT := 10`, 1 (paper) | none (spec code `missing-let`) | accepted, prints `10` | yes | `module.top.bindings` | confirm top-level `:=` stands |
| `(dt, key) := case`, 1 (paper) | `syntax-error` (Q1 notes it valid) | `missing-let` | no | `grammar.let.destructure` | confirm which spelling stands |
| `zero: f64`, `nothing: i32?` bindings, 2 (paper, JSON) | none / `missing-let` | `missing-let` in both | yes | `grammar.let.annotated` | keep |
| `mut out :=`, `fn f(mut`, `elif`, `map comprehensions`, `Reverse(v:)`, `(7 as u8)`, `{...}` arms, `not`, `const`, `Expr.Not =>`, invented `Expr[T]` patterns, `read_text!` imports, `if-and`, `spread ...` in calls, 13 syntax rows | `syntax-error` at the spot (or paper-only) | `syntax-error` at the spot in all 13 | no | grammar chapters | keep rejecting |
| `use FsRead, read_text!`, 1 (P1j) | `syntax-error: expected '}', found '!'` | `syntax-error` at the `!` | partly | `grammar.use` | say `!` marks calls, never imports |
| `loop:` endless loop, 2 (CLI probe, todo) | `unknown-name: unknown function 'loop'` | parses as a call, then `unsupported: Body: expression TrailingCallExpr` | partly | — (no `loop` construct) | reject at parse with `write 'while true:'` |
| `eprintln` without import, 1 (differential) | `missing-requirement` naming `println` | `unknown-name 'eprintln'` | no | `names.visible` | name `use std.console.eprintln` |
| `Result` matched with `.Some`, 1 (wordcount) | `unknown-variant: enum 'Result' has no variant 'Some'` | `unsupported: Body: a variant pattern on a value whose type is not known` | no | `types.result.pattern` | resolve the scrutinee type first, then report the bad variant |
| `Error::new`, 1 (probe 3) | `unknown-method: trait 'Error' has no method 'new'` | `unknown-name 'Error'` (not in scope single-file) | no | `error.ctor` | with `Error` in scope, suggest the existing constructor |
| top-level effectful loop, 1 (script `for`) | `top-level-read-before-initialization` | invalid Wasm: V8 `CompileError: not enough arguments on the stack` | no | `module.init.script` | diagnose, never emit uninstantiable modules |
| `///` doc comments, 1 (probe 6) | `invalid-token: a backtick must enclose ...` | `syntax-error` at the first `/` | partly | `lex.doc` (`##`) | say doc comments use `##`, not `///` |
| second `tests:` block, 1 (probe 5/6 shape) | `duplicate-tests-block` | accepted: two blocks ran `2 passed` | partly | `module.test.one-block` | either reject per spec or bless merging; today it silently accepts |
| leading-dot continuation, 2 (Describer, `Node.Group`+`.Ok`) | `unknown-method ... has no supported method 'Ok'` | same family (method resolution on the joined line) | partly | `grammar.line-join` | say a line starting with `.` continues the previous expression |
| non-`pub` `main` runs nothing, 3 (`M6 Display`, `#201 bench`, single-file entry) | silent exit 0 | the new compiler RUNS it and prints `42` | partly | `module.entry` single-file rules | single-file `main` needs no `pub`, but say so: `note: single-file entry needs no 'pub'` |
| `it_prop` bool bodies, 1 (paper) | no diagnostic (syntax-only check) | `unknown-import 'it_prop' in 'std.testing'` | partly | `std.testing.prop` | implement or diagnose the missing harness explicitly |

## Not helped

| Mistake (rows) | Old message | New message | Helped now? | Spec rule | Proposed better message |
| --- | --- | --- | --- | --- | --- |
| panics carry no location, 2 (map key, `usize` underflow) | `index-out-of-bounds` / `integer-overflow` with no key or location | `panic: key-not-found: map key not found`, `panic: arithmetic-overflow: integer overflow`, both with no location | no | `flow.panic.locations` | attach the call span: `at file.hd:LINE:COL` |
| float literals at runtime, 1 (`decode::[Shape]` f64) | host crash `the host has no function 'parse_f64'` | `panic: explicit-panic: intrinsic` on `println(1.5)` | no | `std.num.float` | implement or reject floats at check time, never panic-print |
| `race!(f(), g!())` children, 2 (probe 5/6) | `type-mismatch: expected mut Suspend[string], found string` | `unknown-name 'race'` (single file; harness out of scope) | no | `req.combinator.race` | with the import present, say plain calls make cold children: `race!(f(), g())` |
| invented `Expr[T]` patterns, 1 (probe 3) | `syntax-error: expected '=>', found '['` | `syntax-error` at the `[` | no | `grammar.pattern.gadt` | show the tour's `Expr.IntLit(value)` shape |
| `not x`, `const`, `mut` params, `{...}` arms, `Reverse(v:)`, map comprehensions, `(7 as u8)`, `elif`, `mut out :=`, `zero: f64`, `nothing: i32?`, `if-and`, `Expr.Not =>` (13 paper/syntax rows) | `syntax-error` with positions, or nothing (paper) | `syntax-error` with positions (all 13 still rejected; `(dt,key) :=` still says `missing-let` despite the Q1 note) | no | grammar chapters | keep rejecting; `(dt,key) :=` disagrees with the Q1-cited valid spelling — confirm which stands |

## Not transferable (prototype internals, guide friction, paper checks)

No new-compiler run: the mistake has no new-compiler counterpart to
compare against. `std loader renames` (Clock helper, `\r` escape,
derive order — loader textually renames; the new compiler has no
loader), `join`/`Eq` inference semantics (`day_of`, `policy`,
`Backoff`, `listed` — prototype `==` internals; the new message already
improved, see above), `ManualClock` sleep/timeout semantics (prototype
runtime behavior), `report_of` chains (no error to compare),
`hd test` main-output counting (fixed above), `all!` spread design
(accepted language restriction), `FLOAT-PARSE`/`ZIP-ARG`/`METHOD-DEFAULT`
known std gaps, `missing-eq Ordering`, `u8.cmp`/`Map.keys` loader-gated
methods, `mut:Date` joins, `downcast`/`Inspectable` bounds,
`Debug`/`Display` std impl gaps, `never`-ICE (fixed shape above),
`invalid-map-key` MVP contract, `Structure::name`, generator/`Drawer`
erasure, closure-capture/tuple-closure design removals, `Tree.arbitrary`
beyond spelling (covered), bool property bodies beyond `it_prop`
(covered), guide friction with no error (JSON `usize` modeling, nested
`match` verbosity, `ManualClock` docs), paper-only rows with no message
(`zero: f64`, `if-and`, `{a==b}` braces), silent-entry rows (covered by
the `priv-main` run above).

## Ranking

By row count: `mut`-receiver/`let mut` guidance (5), contextual enum
variants (3), big literals (3), `cannot-infer-type` on `[]` (4),
unknown std methods without did-you-mean (5), `println`/`$ Console`
entry (5, mostly fixed), unlocated panics (2), float support (1),
top-level effects (1), `race!` children (2). The single most frequent
actionable gap is missing locations on runtime panics, followed by
did-you-mean on unknown methods and the `let mut` suggestion.
