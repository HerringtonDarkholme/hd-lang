# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end. A resolved entry is removed: the specification holds what it decided, and
git history holds its decision table.

## Language Design Decisions

### Readings Waiting For Confirmation

Applying earlier batches left these points. The specification applies
the reading in the third column, and each point asks the owner to confirm
it.

| # | From | Question | Applied reading and **Recommendation** |
| --- | --- | --- | --- |
| INF-lit | batch 17 | Does an integer literal argument take the type solved from the other arguments in any position? Without that, `pick(1, large)` with an `i64` `large` is a `type-mismatch`, since the literal alone is `i32`, while `pick(large, 1)` checks. | **Recommendation:** yes: a literal is not a conversion, so it takes the solved type as its expected type in any position, as Rust's integer literals do. |
| INF-code | batch 17 | Which code does any other conflict get, such as a `List[mut User]` and a `List[User]` (variance), a `T` and a `T?`, or two child-trait values? The decision names `type-mismatch` for numbers and `no-common-type` for trait values. | **Recommendation:** `no-common-type` where the least common type also fails (trait values, supertrait widening); `type-mismatch` otherwise, as `choose(1, true)` already is. |
| SR-omit | batch 17 | Does a variant's `self_ref` count a member that the derivation block omits (`cache = pass`)? | **Recommendation:** no: count only the members the derivation sees, since an omitted member takes its default and is never walked or built. |
| Q7-code | batch 26 | The decision says a `mut` key "stays an error" but names no code. | It keeps `invalid-map-key` ([`types.map-key.no-mut`](../spec/lang/04-type-system.md#r-types.map-key.no-mut)), as variant B of Special Cases C8 proposed. **Recommendation:** keep it; the code names a rule no bound states. |
| VA-unbounded-code | batch 31a | An unbounded `Args` used as `Fn`'s inputs needs a code. | `generic-kind-mismatch`, as a non-tuple there already is, rather than `unsatisfied-trait-bound`. **Recommendation:** keep it; one rule covers both. |
| Q5-cycle-site | batch 32a | "Not just `embedding-too-deep`" leaves open whether a cycle also reports the depth code, and on which types. | `embedding-cycle` replaces `embedding-too-deep` for every type in the cycle, reported once per cycle on its first declared type, as `alias-cycle` is. A type outside the cycle that embeds into it still gets `embedding-too-deep`. **Recommendation:** keep it. |
| Q5-self-id | batch 32a | STYLE says a rule whose meaning changes gets a new ID, but the owner said to keep `data.embed.depth.self` while its code changes. | The ID is kept, as directed. **Recommendation:** keep it; the commit message records the code change. |
| Q5-hash | batch 34 | Lists have no `Hash` ([`types.map-key.no-hash`](../spec/lang/04-type-system.md#r-types.map-key.no-hash)), so hashing the rest as its list gives a rest tuple no `Hash`. | Applied as stated: a rest tuple is not a map key; a Note says so. **Recommendation:** keep it until lists get `Hash`. |
| Q6-plain | batch 34 | The owner chose a spread pattern over a plain subpattern for the rest. Whether `let (a, b, xs) = t` on a rest tuple is then an error is not stated. | `type-mismatch` ([`flow.match.spread.required`](../spec/lang/06-control-flow.md#r-flow.match.spread.required)), mirroring a spread pattern against a fixed tuple. **Recommendation:** keep it; the pattern then always shows the type's shape. |
| Q6-count | batch 34 | Whether a spread pattern may also take trailing fixed elements, as in `let (a, xs...) = t` with two fixed elements, is not stated. | No: the subpatterns before it must match the fixed elements one each, or `type-mismatch` ([`flow.match.spread.arity`](../spec/lang/06-control-flow.md#r-flow.match.spread.arity)). **Recommendation:** keep it; collecting would build a new list in a pattern. |
| Q6-form | batch 34 | The decision shows `xs...` only. | A spread pattern is a name or `_` before `...` ([`grammar.pattern.tuple-spread`](../spec/lang/02-grammar.md#r-grammar.pattern.tuple-spread)); `_...` binds nothing, and `mut xs...` works in `let`. hd has no list patterns, so no other pattern could match the list. **Recommendation:** keep it. |
| Q4-lone | batch 34 | A lone list spread into a rest-only `Args`, as in `call(k, xs...)` with `k(xs...: List[i32])`, is a positional spread of the whole vararg, so `xs` must be `Args`. | Kept: a lone spread passes the whole collected value ([`expr.call.spread.at-vararg`](../spec/lang/05-expressions.md#r-expr.call.spread.at-vararg)), so this is `type-mismatch`, and `call(k, (xs...,)...)` passes it. **Recommendation:** keep it; one spread meaning per position. |
| Q4-infer | batch 34 | With no other argument solving `Args`, [`fn.vararg.tuple-param.infer`](../spec/lang/07-functions.md#r-fn.vararg.tuple-param.infer) says one element per argument, but `pack(1, xs...)` ends in a list spread. | The tuple expression decides, as Q4 says: `Args` is `(i32, List[i32]...)`. **Recommendation:** keep it; reword `.infer` only if a reader finds it unclear. |
| Q6-default-char | batch 35 | The decision lists `char` with "(decide)". | `char` has no `Default`. **Recommendation:** give it `'\u{0}'`, as Rust does, only when a use needs one; no other char is more neutral. |
| Q6-default-derive | batch 35 | Whether `@derive(Default)` exists is not stated. | Not added: a data type implements `Default` by hand. **Recommendation:** wait; a template would need a rule for which enum variant is the default. |
| Q6-one-rest | batch 35 | Whether a rest tuple counts its rest's items when it decides on `(1,)` is not stated. | Yes: a rest tuple holding one value in all writes `(7,)`, as `debug` already wrote it the same as `(7,)` ([`expr.interp.std.tuple.one`](../spec/lang/05-expressions.md#r-expr.interp.std.tuple.one)). **Recommendation:** keep it; the text then mirrors the value's literal. |
| O7-mut | batch 36 | A walk or describe handle of a `hits: mut Counter` member has `F = Counter`, but the fact binds to `mut Counter`. | `h.fact` finds only an exact type, so that read misses ([`annot.handle.fact.exact`](../spec/lang/14-annotations.md#r-annot.handle.fact.exact)). **Recommendation:** keep it; `Arbitrary` reads through `build`'s declared-type handles. |
| O7-inspectable | batch 36 | Derived `Arbitrary` still requires every member to be `Inspectable`, a bound that existed only for the downcast. | Kept ([`std-testing.arbitrary.derive.member-bounds`](../spec/std/testing.md#r-std-testing.arbitrary.derive.member-bounds)); `with` itself dropped it. **Recommendation:** drop the member bound too. |
| O3-blocks | batch 36 | Templates allow derivation blocks, but comparison derivations were never configurable. | Still unconfigurable ([`trait.derive.cmp-every-member`](../spec/lang/09-traits.md#r-trait.derive.cmp-every-member)), so law partners stay consistent. **Recommendation:** keep it. |
| O3b-rest | batch 36 | A rest member typed `List[T]` cannot meet a `Display` walker's bound, and text writes its items inline. | `Walker` gains `rest`, whose default calls `member` ([`annot.walk.rest`](../spec/lang/14-annotations.md#r-annot.walk.rest)). **Recommendation:** keep it; the alternative is `Display` for `List`. |
| O3b-user | batch 36 | Whether a trait outside `std` may declare a tuple template is not stated. | Yes, under the template rules ([`annot.template.tuple.form`](../spec/lang/14-annotations.md#r-annot.template.tuple.form)). **Recommendation:** keep it; it adds no `std` special case. |
| K1-code | batch 43 | The decision says a generic `find::[M]()` requires `M < Inspectable`, but names no code. | `unsatisfied-trait-bound` ([`annot.structure.find-key`](../spec/lang/14-annotations.md#r-annot.structure.find-key)), as for any type argument that fails a bound. **Recommendation:** keep it. |
| K1-mentions | batch 43 | The decision names `find::[M]()` with `M` a type parameter. Whether `find::[Box[T]]()` also needs `T < Inspectable` is not stated. | Yes: each type parameter that the argument mentions needs the bound, as the `h.fact` exemption for the handle's own `F` implies. **Recommendation:** keep it. |
| EMPTY-RUN-fixture | batch 43 | Every `# expect-stdout:` line expects a newline, so no fixture can expect empty output from `run`. [`module.init.script-empty`](../spec/lang/10-modules.md#r-module.init.script-empty) therefore has no fixture. | None added. **Recommendation:** add a header that expects `run` to exit 0 with empty standard output. |
| RACE-EMPTY-forms | batch 44 | The decision makes `race!` with no tasks a compile-time error. Whether `race!(tasks=[])`, or a spread of a list that is empty at run time, is also covered is not stated. | Only a call written with no task argument is an error ([`req.combinator.race-empty`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-empty)); an empty list value is not checked. **Recommendation:** also reject an empty list literal, and state what an empty list at run time does. |
| ALL-EMPTY | batch 44 | `race!` with no tasks is now an error, but `all!()` is not mentioned. | [`req.combinator.all-typing`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.all-typing) gives it type `()`. **Recommendation:** keep it; an empty tuple of results is well defined, unlike a first result. |
| RANGE-PAT-LITERALS | batch 49 | The decision makes integer ranges count for exhaustiveness, but does not say whether literal arms count too. | Yes: literal and range patterns together cover an integer type ([`flow.match.cover.integer`](../spec/lang/06-control-flow.md#r-flow.match.cover.integer)), so a `u8` match with `0..=127` and `128..` needs no `_`. **Recommendation:** keep it, as Rust does. |
| RANGE-PAT-BOUND | batch 49 | The decision does not say how a pattern bound is typed, or what an empty pattern such as `5..5` reports. | A bound takes the subject's type, so `0..=300` on a `u8` is `integer-literal-range` ([`flow.match.range.bound-type`](../spec/lang/06-control-flow.md#r-flow.match.range.bound-type)); an empty pattern is `unreachable-match-arm` ([`flow.match.range.empty`](../spec/lang/06-control-flow.md#r-flow.match.range.empty)). **Recommendation:** keep both. |
| LISTVIEW-NAME | batch 49 | The decision leaves the view type's name to the agent. | `ListView[T]` in `std.collections`, not a prelude name, so code that names it writes `use std.collections.ListView` ([`std-collections.view.import`](../spec/std/collections.md#r-std-collections.view.import)). **Recommendation:** keep the name and the import, as the range types need one. |

### Codes Waiting For The Code Revamp

These readings name a diagnostic code that no decision chose. Each waits
for the error-code revamp, task #101, which may merge codes.

| # | From | Question | Applied reading |
| --- | --- | --- | --- |
| AT-code | batch 20 | [`annot.walker.obligation.error`](../spec/lang/14-annotations.md#r-annot.walker.obligation.error) gives `member-not-derivable` for a member that fails a source's bound. AT-with names `unsatisfied-trait-bound`, which [`std-testing.arbitrary.derive.not-derivable`](../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable) states. So two rules name different codes for one check. | **Recommendation:** `member-not-derivable`, the code every other template reports at the opt-in, naming the member. |
| LP-codes | LP1 | The decisions name no codes for a refutable pattern without `else`, an `else` block that falls through, or a pattern before `:=`. | Two new codes, `refutable-let-pattern` and `let-else-falls-through` ([Let Patterns](../spec/lang/06-control-flow.md#let-patterns)); a pattern before `:=` reuses `missing-let`. |
| TU2-code | batch 27 | TU2 names no code for `mut (A, B)`. | A new code, `mut-on-tuple`; `mut-on-primitive` would misname a tuple. |
| VA-type-code | batch 31a | The decisions name no code for a vararg of another type, as in `values...: i32`. | `type-mismatch` ([`fn.vararg.type.kinds`](../spec/lang/07-functions.md#r-fn.vararg.type.kinds)). |
| Q3-codes | batch 32b | The record lists `data.embed.unique` and `trait.by.invalid` as error detail, but each is the only rule that names its code. | Both stay numbered; the other seven error-detail rules became Notes. |
| TR-code | batch 33a | A rest element that is not a `List`, as in `(i32, i32...)`, needs a code. | `type-mismatch` ([`types.tuple.rest.list`](../spec/lang/04-type-system.md#r-types.tuple.rest.list)), as for a vararg of another type. Deferred to #101 by the owner (batch 34 Q7). |
| SC-Q2 | Special Cases Q2 | Four codes duplicate a partner: `suspending-defer`, `identity-needs-reference-bound`, `recursive-closure-needs-result-type`, and `mutable-embedded-field`. | All eight codes kept. **Recommendation:** merge all four into their partners; messages keep the context word. |
| SC-Q3 | Special Cases Q3 | Six codes report an operator with no meaning for its operands: `missing-eq`, `missing-partial-ord`, `unsupported-equality`, `nonnumeric-unary-plus`, `unsigned-negation`, and `mixed-numeric-types`. Every other operator reports `type-mismatch`. | All six kept. **Recommendation:** all six become `type-mismatch`, and `assert_equal`'s missing `Eq` becomes `unsatisfied-trait-bound`. |
| SINGLE-CODE | batch 46 | The decision names no code for a `pkg`, `dep`, `self`, or `super` use in a single-file program. | `unknown-module` ([`module.single-file.roots`](../spec/lang/10-modules.md#r-module.single-file.roots)), as a `super` above the test root is. |
| CLI-CODES | batches 46 to 48 | These errors have no code: `hd run` or `hd build` outside a package, `hd check` or `hd test` without a FILE outside a package, `hd run FILE`, `hd run` with no or several executables, an unknown `NAME`, a workspace `NAME` that no member or several members have, a bare `hd run` at a workspace root, a task and an executable with one name, `hd new` where `hd.toml` exists, a `src/mod.hd` ([`module.path.no-root-mod`](../spec/lang/10-modules.md#r-module.path.no-root-mod)), and a dependency on a package with no library ([`module.path.no-lib-dependency`](../spec/lang/10-modules.md#r-module.path.no-lib-dependency)). Two batch 47 readings wait here too: a use of `src/main.hd`, an integration test program, or a task from another module, and a `super` in a root file, report `unknown-module` ([`module.path.main-no-use`](../spec/lang/10-modules.md#r-module.path.main-no-use), [`module.test.integration.program-use`](../spec/lang/10-modules.md#r-module.test.integration.program-use), [`cli.task.program-use`](../spec/cli/command-line.md#r-cli.task.program-use)); and `cyclic-test-dependency` kept its name when `[test-dependencies]` became `[dev-dependencies]` ([`module.test.cyclic-dev-unit`](../spec/lang/10-modules.md#r-module.test.cyclic-dev-unit)). | None named for the CLI and manifest errors ([Command Line](../spec/cli/command-line.md)); `unknown-module` and `cyclic-test-dependency` kept. **Recommendation:** name the CLI and manifest errors with the manifest diagnostics, keep `unknown-module` with a message that says the file is a separate program or a root, and rename `cyclic-test-dependency` to `cyclic-dev-dependency`. |
| GR-24 | grammar audit | A line in a bracketed closure body that dedents to a column between the header and the body matches both [`lex.indent.unknown-column`](../spec/lang/01-lexical-structure.md#r-lex.indent.unknown-column) (`invalid-dedent`) and [`lex.closure.between`](../spec/lang/01-lexical-structure.md#r-lex.closure.between) (`syntax-error`). | The prototype reports `syntax-error`. **Recommendation:** `syntax-error`, the more specific rule; say so in a Note. |
| VIEW-CODE | batch 49 | The decision names no panic code for using an invalidated `ListView`. | `iterator-invalidated` ([`std-collections.view.invalid-use`](../spec/std/collections.md#r-std-collections.view.invalid-use)): the view checks the same structural-version counter as a list iterator, as Java's `subList` throws the same exception as its iterator. **Recommendation:** keep it, or rename the category `collection-invalidated` in the revamp. |

### Ranges And Slicing

Applying batch 49 left these open. Each form below is an error, or
behaves as the "Today" column says, until the owner decides it.

| # | Question | Today | **Recommendation** |
| --- | --- | --- | --- |
| RANGE-PAT-TO | Is `..b` a range pattern too? Rust 1.80 allows `a..b` and `..b` in patterns ([release notes](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0/)), but the decision lists `a..=b`, `a..b`, `a..`, and `..=b`. | `syntax-error` ([`grammar.pattern.range.no-to`](../spec/lang/02-grammar.md#r-grammar.pattern.range.no-to)) | Allow it, so every range expression form but a bare `..` is also a pattern. |
| VIEW-TIER | `List.view` must notice a later append, but `lib/std` cannot read a list's structural-version counter in plain hd, so `ListView` fails the tier test. | Stdlib tier ([Views](../spec/std/collections.md#views)) | Keep it in `spec/std/` and add one intrinsic that reads a list's version, as `List.append` is one. |

```text
fn sign(n: i32) -> string:
    match n:
        ..0 => "negative"  # hypothetical syntax (RANGE-PAT-TO)
        _ => "other"
```

### Type-Rule Gaps

The type audit left these points open. Each is a gap in chapters 04, 09,
11, 12, or 13; nothing below is decided. TQ-14 is decided and needs no
change, and is listed so the owner can close it.

| # | Question | **Recommendation** |
| --- | --- | --- |
| TQ-14 | Decided: assignability stays single-step, so `let wide: i64? = small_i8` and passing a `User` to a `Display?` parameter need explicit conversions. [`types.assign`](../spec/lang/04-type-system.md#r-types.assign) applies one rule at a time. Is that wording enough? | Yes; close it with no change. |
| TY-04 | An impl parameter that occurs in neither the trait arguments nor the target, as `T` in `impl[T < Display, I < mut Iterator[T]] Summary for I`, lets one impl apply twice to one type. Is that an error? | Yes, a new `unconstrained-impl-parameter`, as Rust's E0207. |
| TY-05 | A match arm of a GADT variant with a bounded existential, `Item[U < Display](value: U) -> Shown`, calls `Display` on `U`, but chapter 13 says a value carries only its tag and payload. Where does the evidence come from? | Construction stores the evidence for the variant's bounds in the value. |
| TY-07 | `impl[T < Eq] Parent for Box[T]` and `impl[T] Child for Box[T]` make `Box[fn() -> void]` a `Child` but not a `Parent`. Is that checked at the impl or at a use? | At the impl: its own bounds must prove every supertrait (`missing-supertrait-implementation`). |
| TY-08 | Inside `fn has_child[T < Child]`, may `T` be passed where `T < Parent` is required? Chapter 09 only says the supertrait's methods are found. | Yes: a bound implies each transitive supertrait bound. |
| TY-15 | May a requirement key name a trait that is not dynamically safe, such as one whose method returns `Self::Item`? | No: `trait-not-dynamically-safe`, reported at the row. |
| TY-17 | May an enum derive `Eq` or an ordering when a GADT variant has an existential parameter, as `Wrapped[U < Eq](value: U) -> Cell`? Two values may hold different `U`. | No for `Eq` and ordering; `Hash` only when every existential field is `Hash`-bounded. |
| TY-18 | Which module owns a derived impl, and what does a derivation beside a written impl of the same trait report? | The declaration's module; the pair is `overlapping-impl`. |
| TY-20a | The assignability list in chapter 04 omits `never`, though [`types.never.assignable`](../spec/lang/04-type-system.md#r-types.never.assignable) makes it assignable to every type. Should the list name it? | Yes, as one more rule. |
| TY-20b | May a `mut` trait value be built, as in `let edit: mut Display = mutable_user`? [`types.assign.trait-value`](../spec/lang/04-type-system.md#r-types.assign.trait-value) is silent. | Yes: the same rule, applied to the mutable view. |
| TY-21 | What is the least common type of `if ok: 1 else: return .None`, where one operand is `never`? | Drop `never` operands first; if all are `never`, the result is `never`. |
| TY-22 | Is `value.clear()` allowed on `value: T` with `T < mut Clear`, or on `Self` inside a `mut self` method? Mutable Paths covers only `mut U` types. | Yes: define "mutable access" once, covering both. |
| TY-23 | What do a bound on a pack, `Ts... < Display`, and an impl over a pack tuple, `impl[Ts... < Display] Display for (Ts...)`, mean? Packs are deferred, so this waits for them. | One obligation and one dictionary per element; a pack tuple head matches every arity. |
| TY-28 | The Mutable Paths prose gives `parent.child.rename("new")` on a readonly root `readonly-root`, while nine fixtures expect `mutable-receiver-required`. Which is right, and which code does a promoted `mut self` call get? | `mutable-receiver-required` for any `mut self` call without mutable access, including through promotion; align the prose. |
| TY-29 | `trait-method-visibility`, `local-impl-nonlocal-pair`, `missing-partial-eq`, `missing-partial-ord`, `duplicate-annotation-impl`, and `overlapping-annotation-impl` appear in no chapter. | Settle each in the error-code revamp, task #101: give it a rule or merge it. |
| TY-31 | Do `void` and `never` satisfy `Any`? Chapter 04 says every value type does. | No: neither is a value type. |

### Ideas Noted For Later

None of these is decided.

- **A cover grammar for `:=`.** As in JavaScript, Python, Rust's
  destructuring assignment, and Elixir, it would let `:=` take patterns
  too. With it would come data-literal field shorthand, `Point { x, y }`
  for `Point { x: x, y: y }`, so a pattern and a literal read the same.
  The owner deferred both in batch 26 ("we can add in future").
- **Teaching notes for `?.` and `is` (GR-21).** `a?.b` is propagation
  and then member access, so `.None` returns from the enclosing function;
  it is not Kotlin's or Swift's optional chaining. `is` is identity, as in
  Python, not a type test. Should the guide call both out?

### Bound And Row Operators

**Questions from applying them.** Each needs an owner answer; the spec
states the current behavior.

| Question | Applied now | Recommendation |
| --- | --- | --- |
| Does `$ A + B` inside `[...]` or a parameter list need precedence rules? | A row ends at the first `,`, `)`, or `]`; nested function types keep the innermost-owner rule | None needed. No ambiguity was found in type arguments, parameters, `$.Context[...]`, or closure headers. |
| Codes for other old row spellings | `$(A + B)`, `$(A)`, a `-` between keys, and the pre-2026-09-27 `Job[A + B]` and `$.Context[A + B]` are `syntax-error` | Keep `syntax-error`. Only the two decided codes carry fix-its. |
| `$.Context[$ A + B]` keeps its inner `$`, while one key is `$.Context[A]` | Kept: the context type takes a key or a row type argument | Keep it. It matches row type arguments such as `Job[$ A + B]`. |

### Mutable Host Providers

**Still open (raised 2026-09-28).** Nothing here is decided:

| Question | Effect | **Recommendation** |
| --- | --- | --- |
| A fixture for a pending host write | No [runtime profile](../spec/conformance/README.md#runtime-profiles) holds a `write_line!` pending and then completes it: `console` is ready on the first poll, and `pending-gate` never completes. So `module.console.println-drive.pending` and `block_on`'s own wait have only a prototype unit test. | Add a conformance profile whose gate is pending on its first poll and ready on the next. |
| A fixture for the `.Err` panic | `module.console.println-error.category` has no fixture, since no code can build a `ConsoleError` (follow-up 2 defers its constructor). | Add the fixture when `ConsoleError`'s constructor is settled. |

```text
fn report!() -> void $ Console:
    defer:
        println("done")   # error: suspension-forbidden-context
    println("working")    # valid: writes under the caller's driver
```

### Typed Derivation, Tool Adapters, And Secrets

**Waiting on other areas.** The spec lists these as
[undecided parts](../spec/lang/14-annotations.md#undecided-parts); each waits
for the owner, and Typed Derivation
gives their background:

| Question | What is undecided |
| --- | --- |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library (STDLIB). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library (STDLIB). |
| Function targets | Deriving for functions, as tool adapters need ([parked](#parked-tool-adapters)). A decorator before a function attaches a plain value that `facts_of(f).find::[M]()` reads ([Function Facts](../spec/lang/14-annotations.md#function-facts)). |

M30 deferred template constants, typed shared constants, and composing
templates until a real template needs them; they are not in the spec.

#### Parked: Tool Adapters

Parked with typed derivation (FN_TYPE decision 10); tools register
functions by hand for now. Background is in the archived
Nominal Function Types.

| Question | Options | **Recommendation** |
| --- | --- | --- |
| FN-Q9: how does a tool adapter get per-declaration data about a function? | A1, `shape_of(f)` passed beside the value; A2, a `fn_view(f)` intrinsic; B, per-declaration item types with a compiler-generated `FnStructure`, so `@derive(mcp.Tool)` works on functions. | B, or A2 if item types are too much surface. |
| FN-Q10: where are item types visible? | A, only where a generic parameter is inferred from the argument and in heads written `fn name`; B, everywhere, as in Rust. | A: bindings and list literals keep their function types. |

**Member-typed facts.** Testing AT-with (batch 20) chose option B, and
option D, member-typed facts, stayed open. Batch 36 (O7) then accepted
typed member facts, and batch 39 gave them their final form,
[Member-Typed Facts](../spec/lang/14-annotations.md#member-typed-facts).

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
(STDLIB decision 12, 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([Identity](../spec/lang/05-expressions.md#identity)) and no stable code
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
([`data.repr.runtime-only`](../spec/lang/08-data-and-enums.md#r-data.repr.runtime-only)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values (Durable Replay decision 11, in
Replay Rules, replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
(Durable Replay decision 14, in
Replay Rules).

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

### Confirmed Deferred Type Features

**Problem.** One surface remains intentionally unsupported and must be
diagnosed: direct permission weakening combined with generic variance.
Bound methods are now `value::name` references
([Method References](../spec/lang/07-functions.md#method-references), MR1). Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/lang/09-traits.md#runtime-type-identity)).

**Direction.** Keep weakening with variance deferred, and design it only
with a motivating requirement; it must preserve representation.
Negative implementations are likewise confirmed future work rather than an
implicit extension.

**Unblocks.** Implementer certainty today and a checklist for future proposals.

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

**Parked questions.** The owner does not want to discuss NonEscapable now.
These are the initial answers, not to be applied until the owner reopens
the topic.

| # | Question | Initial answer |
| --- | --- | --- |
| TQ-24 | How does NonEscapable propagate to a type holding a NonEscapable field, as `data HiddenFile` with `file: File`? Automatically, like Rust's auto traits, or declared and checked? | Declared and checked: such a type must itself be declared NonEscapable, and a generic type is NonEscapable exactly when an argument is. Erasure to `Any`, or to a trait value whose trait does not extend NonEscapable, is rejected. |
| TQ-25 | Does a generic parameter accept a NonEscapable argument by default? | No: a parameter opts in, as Swift's `~Escapable` does, so existing generic code stays valid. |
| TQ-26 | How does a NonEscapable result, as in `fn first_line(file: File) -> Line`, say which parameters it depends on? | No NonEscapable returns for now; later, depend on every NonEscapable parameter, and name them only when a real API needs it. |

### Closure Shorthand

**Deferred (Pipe Operator PL10, 2026-09-29).** Closures stay
`fn(v): v * 2`; there is no `_` lambda shorthand, and no `f(_, a)` capture
(PL13). The pipe owns `_` inside a step
([Pipe Expressions](../spec/lang/05-expressions.md#pipe-expressions)), `it` is
the prelude test function, and `$0` collides with requirements and
interpolation. `fn: _ * 2` would parse but needs a "not inside a pipe
step" exception. Revisit if [the hd writing log](../audit/hd-writing-log.md)
shows demand from cheap-model agents; adding `fn: _` then breaks no code.

### Iterator Performance

The flat-stage iterator design waits for a specializing compiler, task
#86 (Iterator Performance Study).

### Testing Open Points

From the archived Testing Redesign.

- **Generator parameter style.** Generators take `mut Choices` today. The
  owner is comparing a requirement-row style, `fn() -> T $ Choices`. It
  waits for task #76, re-evaluation on a working compiler.
- **A deferred fixture** (T54). A test-layout fixture package for
  `cyclic-test-dependency` is added when that rule needs coverage. The `# fixture-test-layout:`
  header exists ([Test Layouts](../spec/conformance/README.md#test-layouts)).

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/lang/08-data-and-enums.md#r-data.repr.runtime-only));
- the prototype's replay experiments in the
  Wasm GC compiler plan predate the
  decided Replay Rules: their
  identity is per function rather than per program, their site IDs contain
  byte offsets, and they stop at the end of a history instead of resuming;
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. Executables and their selection are specified in
  [Command Line](../spec/cli/command-line.md#executables);
- dependencies through version control hosts, with no registry:
  Dependencies decisions DEP1-DEP7
  are applied in [Package Manifest](../spec/lang/10-modules.md#package-manifest)
  (version tags, minimal version selection, `hd.sum`, workspaces,
  pseudo-versions), with DEP8-DEP19 after them. The manifest diagnostics
  wait for the manifest schema (DEP14,
  [`cli.tooling.package-schema`](../spec/cli/command-line.md#r-cli.tooling.package-schema)),
  and the tooling work is in
  Package Tooling;
- conformance fixtures for `missing-entry-point` and `unselected-main`,
  which need manifest input in the fixture format, so they wait for the
  manifest schema like the other manifest diagnostics;
- the fuzzer ([spec/tools/fuzz](../spec/tools/fuzz/CONTRACT.md)) still runs
  `run FILE` and `build FILE`, which [Command Line](../spec/cli/command-line.md) makes
  package-only; the CLI update should move it to `hd FILE`;
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/lang/10-modules.md#r-module.cycle.package));
- whether a panic's source location is "available"
  ([`flow.panic.report`](../spec/lang/06-control-flow.md#r-flow.panic.report))
  for a trap inside a runtime helper, which decides whether a conformance
  runner can judge a panic marker's line (F-155 in
  [src/KNOWN_ISSUES.md](../src/KNOWN_ISSUES.md));
- whether the specification defines one portable "unsupported feature"
  diagnostic category, so a conformance runner can tell "not implemented"
  from "wrong" (F-250);
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as Replay Rules
  state);
- stateful property testing in `std.testing`, which waits for the event
  log. The property-test API is decided and applied
  (Testing PT1-PT9,
  [Property Tests](../spec/std/testing.md#property-tests));
- doc tests and benchmarks, which no decision covers yet. The testing
  stress test (TS-15) found a direction: doc tests as fenced `hd` blocks in
  `##` comments of `pub` items, run with the `tests/` view, and benchmarks
  with a host clock and their own registration, like Go's `b.Loop` or a
  `benches/` root;
- final signatures, behavior, and the complete intrinsic set for the
  compiler-intrinsic `std.task` combinators, such as racing, timeout,
  and heterogeneous scheduling (`retry!` is a library loop, decided in
  batch 29: [Task](../spec/std/task.md#retry));
- the final `std.task` structured-scope API: `Task[T]` is decided as
  structured scopes only, with `scope!`, `start`, and `join!`
  (STDLIB decision 11), and must not weaken
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
