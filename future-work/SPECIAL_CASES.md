# Special Cases In hd: Inventory And Simplification Review

Status: simplification review, 2026-09-30. It changes no decision, spec
text, or prototype code, and nothing in it is accepted behavior. It covers
chapters [01](../spec/01-lexical-structure.md) through
[14](../spec/14-annotations.md). It reads them against the
[Revision Notes](../spec/README.md#revision-notes),
[Open Issues](OPEN_ISSUES.md), the other records here, and the design
records in git history.

The owner asked: "can you give me a list of special rule, special
intrinsic, and special stuff in hd? evaluate if they can be simplified".
This record answers in two parts. The [Inventory](#inventory) lists every
special case by category, with its anchor and the reason it exists. The
[Cut Candidates](#cut-candidates) propose removals or merges, following the
[complexity-reducer](../.agents/skills/complexity-reducer/SKILL.md) method
and the [Design Cost Order](../AGENTS.md#design-cost-order).

## Summary

### Counts

| Category | Entries | Where |
| --- | ---: | --- |
| Compiler intrinsics | 30 | [Compiler Intrinsics](#compiler-intrinsics) |
| Rule exceptions | 62 | [Rule Exceptions](#rule-exceptions) |
| Special syntax | 42 | [Special Syntax](#special-syntax) |
| Magic names and prelude members | 24 | [Magic Names](#magic-names) |
| Special diagnostics | 113 codes in 33 groups | [Special Diagnostics](#special-diagnostics) |
| Cut candidates | 9 | [Cut Candidates](#cut-candidates) |
| Keep entries | 52: 46 decided, 6 by reason | [Keep](#keep) |

About 256 of the 3,968 rule IDs in chapters 01-14 contain the word
"only", and 25 contain "except" or "exception". Most of the "only" rules
limit a form to its position and are not special cases. The inventory
lists the ones that single out one type, one name, or one context.

### Top 5 Recommended Cuts

| Rank | Cut | Absorbed by | Removed | Cost | Soundness |
| ---: | --- | --- | --- | --- | --- |
| 1 | [C1](#c1-one-code-for-an-operator-with-no-meaning) Operator-operand codes become `type-mismatch` | [`expr.op.no-impl`](../spec/05-expressions.md#r-expr.op.no-impl), [`trait.bound.unsatisfied`](../spec/09-traits.md#r-trait.bound.unsatisfied) | 6 codes, 2 of them with no rule today | coarser code; the message still names the trait | holds |
| 2 | [C2](#c2-diagnostic-twins) Four pairs of twin codes merge | the partner code of each pair | 4 codes | one fixture marker each | holds |
| 3 | [C3](#c3-value-packs-follow-the-vararg-rule) Value packs follow the vararg finality rule | [`fn.vararg.final`](../spec/07-functions.md#r-fn.vararg.final) | 2 codes, 2 rules | none for valid code | holds while hd has no named-only parameters |
| 4 | [C4](#c4-parenthesized-names-in-for) `for (k, v) in m:` like `(a, b) :=` and `let (a, b)` | `grammar.stmt.bind-list` (now [`grammar.stmt.short-binding.one-name`](../spec/02-grammar.md#r-grammar.stmt.short-binding.one-name)) | 1 grammar exception, 1 of 3 list spellings | about 25 loops gain parentheses | holds |
| 5 | [C5](#c5-one-dollar-rule-for-every-string) One `$` rule for every string | `lex.prefix.plain-dollar-start` | half of one rule | `"costs $5"` becomes valid | holds |

The full ranking, with four more cuts, is in [Cut Candidates](#cut-candidates).
Ranks weigh rules, codes, and forms removed against risk and churn. A cut
that changes the meaning of valid source ranks below one that does not.

### Findings Outside The Cuts

| Finding | Effect | Where |
| --- | --- | --- |
| `missing-partial-ord` and `nonnumeric-unary-plus` are stable codes with fixtures, but no numbered rule names either one. | A second implementation cannot learn from the chapters when to report them. | [Diagnostics](../spec/README.md#diagnostics), `typing/invalid/bool-ordering.hd`, `typing/invalid/nonnumeric-unary-plus.hd`; [C1](#c1-one-code-for-an-operator-with-no-meaning) removes both |
| No chapter or record defines `SelfRef`. | The request named it; the nearest item is the undecided `Ref[T]` of the derived-function cache. | [STDLIB Derived Function Cache](STDLIB.md#derived-function-cache) |
| The `Map` implementations of `FromIterator` and `Iterable` are ordinary `std` code, not compiler-supplied. | Nothing to cut: batch 15 (Q-map) already removed the std-only exception. | [Collect Targets](../spec/std/iter.md#collect-targets) |
| `assert` and `assert_equal` are ordinary `std.testing` functions. The compiler checks only their bounds. | Nothing to cut; see [C1](#c1-one-code-for-an-operator-with-no-meaning) for the `missing-eq` code. | [`module.testing.exports`](../spec/10-modules.md#r-module.testing.exports) |

## Inventory

Each table gives the name, its anchor, one line on what it does, and why it
exists. "Why" cites the chapter's Why callout or the owner decision that
introduced the entry. Decision IDs refer to [Revision Notes](../spec/README.md#revision-notes)
entries unless a link says otherwise.

### Compiler Intrinsics

Things the compiler supplies, generates, or recognizes by a qualified name.

| # | Name | Anchor | What it does | Why |
| ---: | --- | --- | --- | --- |
| I1 | `@derive(...)` | [`trait.derive.intrinsic-decl`](../spec/09-traits.md#r-trait.derive.intrinsic-decl), [`annot.derive.opt-in`](../spec/14-annotations.md#r-annot.derive.opt-in) | The only form that creates a derived implementation; its arguments are trait names. | M18 P13: one opt-in; Decorators D6: stays intrinsic. |
| I2 | Comparison derivations | [`trait.derive.intrinsic-set`](../spec/09-traits.md#r-trait.derive.intrinsic-set) | `Eq`, `PartialOrd`, `Ord`, and `Hash` derive over every member, with no member lines. | M12, M18 R3 ("permanently"). |
| I3 | `@error` with `@from`, `@source`, `transparent` | [`annot.error.intrinsic`](../spec/14-annotations.md#r-annot.error.intrinsic) | Generates `Display`, `Error`, and one `From` per `@from` member. | Error Conversion 10: one intrinsic for three traits from the same markers. |
| I4 | Primitive operator bodies | [`expr.op.std.intrinsic`](../spec/05-expressions.md#r-expr.op.std.intrinsic) | `impl Add for i32` and the rest have compiler bodies that match the built-in operators. | OP2: generic code accepts primitives. |
| I5 | Built-in index bodies | [`expr.index.std.intrinsic`](../spec/05-expressions.md#r-expr.index.std.intrinsic) | `Index`/`IndexSet` bodies for `List`, `Map`, and `string`. | STR7-STR10. |
| I6 | Sealed `Num`, `Integer`, `Float` | [`trait.num.sealed`](../spec/09-traits.md#r-trait.num.sealed) | Generic numeric code over the primitive numbers only. | OP9: operators the compiler knows. |
| I7 | `Any`, `AnyVal`, `AnyRef` | [`types.sealed.decl`](../spec/04-type-system.md#r-types.sealed.decl), [`types.any`](../spec/04-type-system.md#r-types.any) | Compiler-implemented value categories; `is` needs `AnyRef`. | Value-category decision; VC-1 to VC-3. |
| I8 | `Inspectable` and `TypeId` | [`trait.inspect.supplied`](../spec/09-traits.md#r-trait.inspect.supplied) | Compiler-supplied runtime identity for inspectable types. | Inspectable decisions 1-16. |
| I9 | `Suspend[T]`, `Poll`, `PollContext`, `Waker` | [`req.protocol.sealed`](../spec/11-requirements-and-suspension.md#r-req.protocol.sealed) | Sealed protocol; only compiler frames and `std.task` implement it. | Why callout: one-shot checks are part of the protocol. |
| I10 | `std.task` combinators | [`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic) | `all!` and `race!` have compiler frames. | Sealed `Suspend` leaves users no way to write them. |
| I11 | `block_on` contexts | [`req.drive.block-on.forbidden-contexts`](../spec/11-requirements-and-suspension.md#r-req.drive.block-on.forbidden-contexts), [`req.drive.block-on.transitive`](../spec/11-requirements-and-suspension.md#r-req.drive.block-on.transitive) | Banned, transitively, in defaults, `defer`, facts, and non-entry initialization. | Those contexts cannot start suspension work. |
| I12 | `host_wait!` and `HostWait[T]` | [`req.host-wait.leaf`](../spec/11-requirements-and-suspension.md#r-req.host-wait.leaf) | The runtime leaf that maps a host wait to the component async ABI. | Host boundary. |
| I13 | `shape[T]()` and `shape_of(f)` | [Shape Intrinsics](../spec/14-annotations.md#shape-intrinsics) | Reflection with unnameable specialized shape types. | K1: no `shape` keyword. |
| I14 | `ShapeMetadata.metadata[M]()`, `TypeShape.is_optional()` | [Common Shape Representation](../spec/14-annotations.md#common-shape-representation) | The one narrow runtime type lookup for metadata. | Typed Derivation decision 10. |
| I15 | `std.structure` bodies | [`annot.structure.bodies`](../spec/14-annotations.md#r-annot.structure.bodies), [`annot.derive.supplied`](../spec/14-annotations.md#r-annot.derive.supplied) | `Structure`, handles, `Facts`, and generated `walk`, `describe`, `build`. | M1-M30: one typed traversal per type, no macros. |
| I16 | Marker names for literal sugar | [`expr.literal-fn.marker`](../spec/05-expressions.md#r-expr.literal-fn.marker) | `std.ops.NumSuffix`, `StrPrefix`, and `Template` recognized by name. | L11, L19-L22; one literal-function rule set since batch 31c. |
| I17 | `std.annotation.Annotate` | [`annot.target.recognized`](../spec/14-annotations.md#r-annot.target.recognized) | Limits a fact type's target kinds. | Decorators D1-D10. |
| I18 | `Display` for interpolation | [`expr.interp.display`](../spec/05-expressions.md#r-expr.interp.display) | `"$x"` calls `std.format.Display`. | No fallback to `Any` or debug text. |
| I19 | `From` for `?` | [`expr.try.convert.from`](../spec/05-expressions.md#r-expr.try.convert.from), [`trait.from.propagation`](../spec/09-traits.md#r-trait.from.propagation) | `?` calls `From[E]` once, with no import. | Error Conversion 1-11. |
| I20 | Erased `Error` in test bodies | [`expr.try.test.with-try`](../spec/05-expressions.md#r-expr.try.test.with-try) | A trailing block given to `it` gets `Result[void, Error]` when it uses `?`. | Testing T15. |
| I21 | `Termination` | [`module.entry.result-termination`](../spec/10-modules.md#r-module.entry.result-termination), [`module.testing.it.body`](../spec/10-modules.md#r-module.testing.it.body) | An ordinary bound on entry results and test bodies. | Testing T5, T8. |
| I22 | `Iterable` drives `for` | [`flow.for.accepts`](../spec/06-control-flow.md#r-flow.for.accepts) | `for` and comprehensions call `iter()` once, or advance a mutable iterator directly. | CS7, CS8, batch 24 IT2. |
| I23 | `collect` default | [`std-iter.collect.target-default`](../spec/std/iter.md#r-std-iter.collect.target-default) | Ordinary `FromIterator` bound with the default `List[T]`; no compiler rule. | CO1-CO6, TD. |
| I24 | `Option` and `Result` support | [`data.prelude.support`](../spec/08-data-and-enums.md#r-data.prelude.support) | `T?`, the one-layer wrap, `?`, and `.Ok()` for `void`. | O1-O3, Result variants decision. |
| I25 | Must-use types | [`flow.must-use.discard`](../spec/06-control-flow.md#r-flow.must-use.discard) | Discarding `Result`, `T?`, or `mut Suspend[T]` is an error. | Why callout: the discard is visible in review. |
| I26 | Test-case functions | [`module.testing.position-statements`](../spec/10-modules.md#r-module.testing.position-statements), [`module.testing.direct-call`](../spec/10-modules.md#r-module.testing.direct-call) | `it`, `it_each`, `it_prop`, `it_prop_with` are recognized in test position. | Testing T2-T50: tools list tests statically. |
| I27 | Function type constructors | [`fn.type.ctor.decl`](../spec/07-functions.md#r-fn.type.ctor.decl), `fn.type.rest` (retired in batch 31) | `fn(...)` is sugar for `Fn` or `SuspendFn`; `Rest[T]` marks a vararg; inputs are tuple-kinded. | FN_TYPE 1-8. |
| I28 | Delegation bodies | [`trait.by.generated`](../spec/09-traits.md#r-trait.by.generated) | `impl Tr for C by E` generates forwarding methods. | Trait delegation decision. |
| I29 | Newtype derivation | [`trait.derive.newtype`](../spec/09-traits.md#r-trait.derive.newtype) | Derives by rewrapping the base type's implementation. | TQ-11. |
| I30 | Doc comments in shapes | [`lex.doc.field`](../spec/01-lexical-structure.md#r-lex.doc.field) | `##` text becomes the shape's `doc` field. | Tools read documentation from shapes. |

### Rule Exceptions

A rule that applies to one type, one name, or one context. Grouped by area.

| # | Exception | Anchor | What it does | Why |
| ---: | --- | --- | --- | --- |
| R1 | `std` inherent methods on built-ins | [`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std), [`trait.own.module.inherent.std`](../spec/09-traits.md#r-trait.own.module.inherent.std) | Only `std` may add inherent methods to primitives, `List`, `Map`, `Option`, `Result`, in any std module. | STDLIB 8. |
| R2 | Tuples have no inherent members | [`trait.own.inherent.std.no-tuple`](../spec/09-traits.md#r-trait.own.inherent.std.no-tuple) | Not even from `std`. | STDLIB 8 detail. |
| R3 | `string` is not `Iterable` | [`flow.for.string-not-iterable`](../spec/06-control-flow.md#r-flow.for.string-not-iterable) | A loop names `chars()`, `char_indices()`, or `bytes()`. | STR1-STR6: bytes vs characters. |
| R4 | `string` has `Index` but no `IndexSet` | [`expr.index.std.string-no-store`](../spec/05-expressions.md#r-expr.index.std.string-no-store) | Strings are immutable. | STR7-STR10. |
| R5 | `mut` dropped on a primitive `Self` | [`types.prim.no-mut.self`](../spec/04-type-system.md#r-types.prim.no-mut.self), [`types.prim.no-mut.self-type`](../spec/04-type-system.md#r-types.prim.no-mut.self-type) | `mut self` is valid in an impl for `i32`, and `self` is plain `i32`. | LM-b, LM-c. |
| R6 | `let mut n = 0` reports `mut-on-primitive` | [`types.bind.let-mut-primitive`](../spec/04-type-system.md#r-types.bind.let-mut-primitive) | In place of `mutable-upgrade`. | `let mut` follow-ups. |
| R7 | Built-in types keep built-in indexing | [`expr.index.trait.builtin-direct`](../spec/05-expressions.md#r-expr.index.trait.builtin-direct) | `List`, `Map`, and `string` skip their own `Index` impls. | OP7, STR7-STR10. |
| R8 | `Map` index read differs by form | `expr.index.map.read`, `expr.assign.compound.map-present`, `expr.index.std.map-read` | `m[k]` is `V?`; `m[k] += v` and `Index::index` read `V` and panic. | Follow-up 5, Map 6. |
| R9 | Primitive operands skip traits | [`expr.op.primitive`](../spec/05-expressions.md#r-expr.op.primitive) | Built-in rules decide; no trait search. | OP2: no cross-type search cost. |
| R10 | Left literal takes its default type | [`expr.op.left-literal`](../spec/05-expressions.md#r-expr.op.left-literal) | `3 * price` needs `Mul[Money] for i32`. | OP4. |
| R11 | `-5s` is `s(-5)` | `expr.op.suffix-negation` (since retired) | The minus joins the suffixed literal; `Neg` is not called. | L4; removed in batch 31c (Q3): `-5s` is `-(5s)`. |
| R12 | Exponent literal is `u32` | [`expr.power.int.literal`](../spec/05-expressions.md#r-expr.power.int.literal) | An unsuffixed literal after `**` is not `i32`. | C2: a signed exponent is a type error. |
| R13 | Shifts do not unify operands | [`expr.shift.unification`](../spec/05-expressions.md#r-expr.shift.unification) | Count and value types are independent. | As in Rust and Go. |
| R14 | `string + string` | [`expr.arith.string-primitive`](../spec/05-expressions.md#r-expr.arith.string-primitive) | The only arithmetic on a non-numeric primitive. | Concatenation. |
| R15 | Floats break `Eq` reflexivity | [`trait.cmp.float-eq`](../spec/09-traits.md#r-trait.cmp.float-eq) | NaN is unequal to itself. | EQ-1. |
| R16 | Map keys | `types.map-key.bound` | `K < Eq & Hash`, no `mut K`, own code `invalid-map-key`. | NaN keys; ghost entries. |
| R17 | `is` on function types | [`expr.is.function`](../spec/05-expressions.md#r-expr.is.function), [`expr.is.function.generic`](../spec/05-expressions.md#r-expr.is.function.generic) | Direct use is an error; through `T < AnyRef` it compiles. | FN_TYPE 9. |
| R18 | `is` on tuples | [`expr.is.tuple`](../spec/05-expressions.md#r-expr.is.tuple) | An error even when the tuple holds references. | Tuples have no identity. |
| R19 | Readonly iterator in a loop | [`flow.for.iterator-mut`](../spec/06-control-flow.md#r-flow.for.iterator-mut), [`flow.for.iterator-direct`](../spec/06-control-flow.md#r-flow.for.iterator-direct) | An error; `for` takes only a mutable iterator directly, since `Iterator` is not `Iterable`. | Batch 24 IT1, IT2. |
| R20 | Adapter callback rows | [`std-iter.adapter.callback-row`](../spec/std/iter.md#r-std-iter.adapter.callback-row), [`std-iter.adapter.fold.row`](../spec/std/iter.md#r-std-iter.adapter.fold.row) | `filter` and `map` take the empty row; `fold` takes `R`. | STDLIB 14-22, PS3. |
| R21 | Comprehension restrictions | `expr.comp.no-suspension`, [`expr.comp.no-jumps`](../spec/05-expressions.md#r-expr.comp.no-jumps) | No bang calls, `return`, `break`, `continue`, or `let`; `?` is allowed. | Initial spec; CO1-CO4 added `?`. |
| R22 | Unread must-use binding | [`flow.unused.must-use`](../spec/06-control-flow.md#r-flow.unused.must-use) | An error, where other unread bindings warn. | The discard must be visible. |
| R23 | Recursive local closure | [`names.scope.recursive-closure`](../spec/03-names-and-scopes.md#r-names.scope.recursive-closure) | The one binding visible in its own initializer. | Local recursion without forward references. |
| R24 | Defaults before a final function parameter | [`fn.default.order-final-function`](../spec/07-functions.md#r-fn.default.order-final-function) | A final `fn` parameter may follow defaulted ones. | T40: `it` options before the body. |
| R25 | One-payload variant constructors | [`data.enum.fn-value`](../spec/08-data-and-enums.md#r-data.enum.fn-value), [`data.enum.fn-value.unsaturated`](../spec/08-data-and-enums.md#r-data.enum.fn-value.unsaturated) | Only these are function values. | Error Conversion 7. |
| R26 | GADT constructors | [`gadt.construct.no-explicit`](../spec/13-gadts.md#r-gadt.construct.no-explicit) | No explicit type arguments on a variant. | Inferred from payload and expected type. |
| R27 | `.Ok()` for `Result[void, E]` | [`types.result.void-ok`](../spec/04-type-system.md#r-types.result.void-ok) | A one-payload variant called with no argument. | `void` has no value to pass. |
| R28 | One-layer optional wrap | [`types.option.wrap.one-layer`](../spec/04-type-system.md#r-types.option.wrap.one-layer) | `T` to `T?`, never `T??`. | O3. |
| R29 | Test body result | [`expr.try.test.with-try`](../spec/05-expressions.md#r-expr.try.test.with-try), [`expr.try.test.without-try`](../spec/05-expressions.md#r-expr.try.test.without-try) | Chosen by whether the block uses `?`. | T15. |
| R30 | Literal test arguments | [`module.testing.it.name`](../spec/10-modules.md#r-module.testing.it.name), [`module.testing.snapshot.literal`](../spec/10-modules.md#r-module.testing.snapshot.literal) | Test names, `ignore`, `expect_panic`, and `expect` are literals. | Static test listing; in-place snapshot update. |
| R31 | Unit tests get no host providers | [`module.testing.unit-row`](../spec/10-modules.md#r-module.testing.unit-row) | Their row must be empty. | T18-T21: fakes only. |
| R32 | `println` under a driver | [`module.console.println-block-on.nested`](../spec/10-modules.md#r-module.console.println-block-on.nested) | Panics inside `main!` or a test body. | MHP-1: `println` is `block_on` of its write. |
| R33 | Redundant prelude `use` | [`module.prelude.no-reimport`](../spec/10-modules.md#r-module.prelude.no-reimport) | `use std.format.Display` is `prelude-name-shadow`. | One spelling per prelude name. |
| R34 | Suffix and prefix lookup | [`names.literal-fn.no-local`](../spec/03-names-and-scopes.md#r-names.literal-fn.no-local) | Module scope only; a local `s` never changes `5s`. | L8, L10; one rule since batch 31c. |
| R35 | `pack.map(` token sequence | `lex.contextual.pack.always` (since retired) | Wins over a local named `pack`. | GQ4; removed with packs in batch 31b. |
| R36 | `reified` position | [`lex.contextual.reified.modifier`](../spec/01-lexical-structure.md#r-lex.contextual.reified.modifier) | Always the modifier there; `[reified]` is an error. | B8. |
| R37 | `@error` and its markers | [`annot.error.name`](../spec/14-annotations.md#r-annot.error.name), [`annot.error.marker.outside`](../spec/14-annotations.md#r-annot.error.marker.outside) | Always the intrinsic; `@from` and `@source` are markers only inside an error type. | Error Conversion 10, batch 9. |
| R38 | Decorator bare call | [`annot.decorator.bare-call`](../spec/14-annotations.md#r-annot.decorator.bare-call) | `@hidden` means `@hidden()`, in decorators only. | D5. |
| R39 | Decorator placement | [`annot.decorator.no-locals`](../spec/14-annotations.md#r-annot.decorator.no-locals), [`grammar.fn.decorator-param-targets`](../spec/02-grammar.md#r-grammar.fn.decorator-param-targets) | No decorators on locals, closures, or local functions. | Module-level syntax only. |
| R40 | `unused-derivation-fact` cases | [`annot.fact.unused-non-std`](../spec/14-annotations.md#r-annot.fact.unused-non-std) to [`annot.fact.unused-block-decorator.any-type`](../spec/14-annotations.md#r-annot.fact.unused-block-decorator.any-type) | Non-std facts warn; std facts do not, except before a block. | M25, M28, M29, follow-up 8. |
| R41 | Walker bound | [`annot.walker.strengthen-member`](../spec/14-annotations.md#r-annot.walker.strengthen-member), [`trait.impl.generics.fixed-bounds`](../spec/09-traits.md#r-trait.impl.generics.fixed-bounds) | The one method whose impl may strengthen a bound. | Typed derivation M1-M22. |
| R42 | `Structure` positions | [`annot.structure.named-positions`](../spec/14-annotations.md#r-annot.structure.named-positions), [`trait.sealed.use-positions-structure`](../spec/09-traits.md#r-trait.sealed.use-positions-structure) | Named only in templates, its `use`, and after `by`. | M-series: no `Structure` outside derivations. |
| R43 | Sealed member names | [`trait.sealed.member-names.error`](../spec/09-traits.md#r-trait.sealed.member-names.error) | Reported as `sealed-trait-implementation`, not `duplicate-trait-member`. | Inspectable decisions 1-15. |
| R44 | Inspectable type arguments | [`trait.inspectable.argument-only`](../spec/09-traits.md#r-trait.inspectable.argument-only) | `void`, trait values, and `Any` count only as arguments. | Inspectable decisions. |
| R45 | `Error` needs an inspectable target | [`trait.error.not-inspectable`](../spec/09-traits.md#r-trait.error.not-inspectable) | A local error type cannot implement `Error`. | `Error < Inspectable`. |
| R46 | Newtype derivation positions | [`trait.derive.newtype.self-positions`](../spec/09-traits.md#r-trait.derive.newtype.self-positions) | `Self`, `Self?`, `Result[Self, E]`, `List[Self]` only. | Why callout: rewrap one value at a time. |
| R47 | Literal default among instantiations | [`trait.resolve.literal-default`](../spec/09-traits.md#r-trait.resolve.literal-default) | Picks the instantiation that fits `i32`/`f64` literals. | TQ-4 follow-ups. |
| R48 | Dynamic-safe method parameters | [`trait.dyn.safe.anyref-type-param`](../spec/09-traits.md#r-trait.dyn.safe.anyref-type-param) | Only `AnyRef`-bounded method type parameters. | One-copy rule. |
| R49 | Trait parameters are invariant | [`types.variance.trait-params`](../spec/04-type-system.md#r-types.variance.trait-params) | No variance markers on traits. | TQ-16. |
| R50 | Implementation modules | [`trait.own.module.inherent-target`](../spec/09-traits.md#r-trait.own.module.inherent-target), [`trait.own.module.trait`](../spec/09-traits.md#r-trait.own.module.trait) | An impl lives in the declaring module, `std` excepted. | TQ-17. |
| R51 | Local implementations | [`trait.impl.local.trait`](../spec/09-traits.md#r-trait.impl.local.trait) | Must involve a local type or trait. | Coherence. |
| R52 | Template and block modules | [`annot.template.module`](../spec/14-annotations.md#r-annot.template.module), [`annot.block.module`](../spec/14-annotations.md#r-annot.block.module) | A template in its trait's module; a block in its type's module. | One template per trait. |
| R53 | Trait-less block header | [`annot.traitless.generic`](../spec/14-annotations.md#r-annot.traitless.generic) | Only the type's own parameters, without bounds. | M28, M29. |
| R54 | Embedded fields | [`data.vis.embedded-public`](../spec/08-data-and-enums.md#r-data.vis.embedded-public), [`data.part.marker-required`](../spec/08-data-and-enums.md#r-data.part.marker-required) | Always public; filled only with a `...` copy. | VE1-VE4, VE-S. |
| R55 | Embedding limits | [`data.embed.width`](../spec/08-data-and-enums.md#r-data.embed.width), [`data.embed.depth`](../spec/08-data-and-enums.md#r-data.embed.depth) | At most three fields, three levels. | Embedding limits decision. |
| R56 | Promotion | [`names.promote.private`](../spec/03-names-and-scopes.md#r-names.promote.private), [`names.promote.no-trait`](../spec/03-names-and-scopes.md#r-names.promote.no-trait) | Only `pub` inherent members promote; trait methods never do. | Single view of members. |
| R57 | Closure suspension inferred once | [`req.suspend.closure.trailing-only`](../spec/11-requirements-and-suspension.md#r-req.suspend.closure.trailing-only) | Only a trailing block for an `fn!` parameter. | T14. |
| R58 | Row alias read by name | [`req.row.alias.bare.by-name`](../spec/11-requirements-and-suspension.md#r-req.row.alias.bare.by-name), [`req.row.alias.one-key`](../spec/11-requirements-and-suspension.md#r-req.row.alias.one-key) | A bare alias in a one-key slot is a row; a one-key alias is a type. | RU2, RU10. |
| R59 | No row parameters on types | [`req.row.param.no-data`](../spec/11-requirements-and-suspension.md#r-req.row.param.no-data), [`req.row.param.context`](../spec/11-requirements-and-suspension.md#r-req.row.param.context) | Data, enums, traits, newtypes, and `$.Context` take no row parameter. | RU3. |
| R60 | Inspectable traits as keys | [`req.key.inspectable`](../spec/11-requirements-and-suspension.md#r-req.key.inspectable) | Never a requirement key. | Provider views stay attenuated. |
| R61 | Closure rows and `$.with` | [`req.row.omitted.outer-scope`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.outer-scope), [`req.with.collision.closure`](../spec/11-requirements-and-suspension.md#r-req.with.collision.closure) | An outer `$.with` never satisfies or collides with a closure's keys. | PS1-PS3, PS3a. |
| R62 | Private entry point | [`module.entry.private-main`](../spec/10-modules.md#r-module.entry.private-main) | A non-`pub` `main` is an ordinary function. | `pub` marks the entry. |

The lexical and layout rules that the request named are special syntax
with their own exceptions. They are listed in [Special Syntax](#special-syntax)
(S38-S42) rather than repeated here.

### Special Syntax

Forms that exist for one feature.

| # | Form | Anchor | What it does | Why |
| ---: | --- | --- | --- | --- |
| S1 | `fn name!`, `f!(...)`, `fn!` types | [`req.suspend.marker`](../spec/11-requirements-and-suspension.md#r-req.suspend.marker), [`grammar.expr.bang-suffix`](../spec/02-grammar.md#r-grammar.expr.bang-suffix) | Marks and drives suspension; `s!()` drives a stored one. | Every suspension point is visible. |
| S2 | `$ A + B`, `$()` | [`grammar.type.row.plus-keys`](../spec/02-grammar.md#r-grammar.type.row.plus-keys) | Requirement rows. | Bound and row operators decision. |
| S3 | `$.use`, `$.with`, `$.context`, `$.Context` | [`req.use.namespace`](../spec/11-requirements-and-suspension.md#r-req.use.namespace) | Provider access, scopes, and reusable contexts; `$` is not a value. | Decomposed requirement model. |
| S4 | `Type::name`, `value::name`, `Trait::f(x)` | [`fn.ref.unbound`](../spec/07-functions.md#r-fn.ref.unbound), [`trait.qualified.form`](../spec/09-traits.md#r-trait.qualified.form) | Method references and qualified calls. | MR1-MR7. |
| S5 | `x \|> f`, `x \|> g(_, 1)` | [`expr.pipe.form`](../spec/05-expressions.md#r-expr.pipe.form) | Pipe with a bare or `_` step. | PL3-PL16, CS2. |
| S6 | `250ms` | [`lex.suffix.form`](../spec/01-lexical-structure.md#r-lex.suffix.form) | Suffixed literal, a call of an `@num_suffix` function. | L1-L22. |
| S7 | `sql"..."` | [`lex.prefix.form`](../spec/01-lexical-structure.md#r-lex.prefix.form) | Prefixed string, a call of an `@str_prefix` function; raw text. | L19-L22. |
| S8 | `"""..."""` | [`lex.multiline.form`](../spec/01-lexical-structure.md#r-lex.multiline.form) | Multiline string, kept verbatim. | |
| S9 | `$name`, `$self`, `${e}` | [`lex.interp.forms`](../spec/01-lexical-structure.md#r-lex.interp.forms) | Interpolation. | Kotlin style; GQ15 added `$self`. |
| S10 | `let (a, b) = p`, `let mut x` | `grammar.stmt.let-list` (now [`grammar.stmt.let-pattern.tuple`](../spec/02-grammar.md#r-grammar.stmt.let-pattern.tuple)), [`types.bind.let-mut-infer`](../spec/04-type-system.md#r-types.bind.let-mut-infer) | Multi-name `let` in parentheses; `let mut` infers `mut T`. | Let 1-7, local mutability decision. |
| S11 | `(a, b) := p`, `((a, b) := p)` | `grammar.stmt.bind-list` (now [`grammar.stmt.short-binding.one-name`](../spec/02-grammar.md#r-grammar.stmt.short-binding.one-name)), `grammar.expr.multi-binding.wrapped` | Multi-name binding; nested use in its own parentheses. | Q1, Q1b. |
| S12 | `x := e` as an expression | [`names.bind.expression`](../spec/03-names-and-scopes.md#r-names.bind.expression) | Binds in the enclosing scope. | Walrus-style guards. |
| S13 | `_ := e` | [`grammar.stmt.discard`](../spec/02-grammar.md#r-grammar.stmt.discard) | Explicit discard of a must-use value. | `_` is not an identifier. |
| S14 | Trailing block `f(a):` | [`fn.trailing.form`](../spec/07-functions.md#r-fn.trailing.form) | Last zero-argument callback as an indented block. | Kotlin and Swift. |
| S15 | `tests:` | [`grammar.tests.block`](../spec/02-grammar.md#r-grammar.tests.block) | One test-only block per file. | T2-T11. |
| S16 | `impl Tr for X by Structure`, `impl X by Structure` | [`annot.block.form`](../spec/14-annotations.md#r-annot.block.form), [`annot.traitless.form`](../spec/14-annotations.md#r-annot.traitless.form) | Derivation block and trait-less metadata block. | M26. |
| S17 | Member lines `f = [...]`, `f += [...]`, `f = pass`, `Self += [...]` | [`annot.line.extend`](../spec/14-annotations.md#r-annot.line.extend) | Edit facts or omit a member for one block. | M1-M30. |
| S18 | Template `impl[T] Tr for T by Structure` | [`annot.template.form`](../spec/14-annotations.md#r-annot.template.form) | A trait's one derived implementation. | M8, M12. |
| S19 | `impl Tr for C by E` | [`trait.by.form`](../spec/09-traits.md#r-trait.by.form) | Delegation to an embedded field. | Trait delegation decision. |
| S20 | `reified T` | [`types.reified.metadata`](../spec/04-type-system.md#r-types.reified.metadata) | Runtime type metadata for a parameter. | `shape[T]()`. |
| S21 | Packs `Ts...`, `pack.map`, `pack.map_list` | `pack.param.type-pack` (since retired), `pack.map.intrinsic` (since retired) | Heterogeneous packs for `all!`-style APIs. Removed in batch 31b; `all!` has one typing rule. | Why callout: no general metaprogramming. |
| S22 | Suffix `...` spread, prefix `...` copy, `...=` | [`grammar.primary.suffix-spreads`](../spec/02-grammar.md#r-grammar.primary.suffix-spreads), [`grammar.primary.prefix-copies`](../spec/02-grammar.md#r-grammar.primary.prefix-copies) | Spreads elements; copies members into a part. | VE-S; second embedding review. |
| S23 | Postfix `?` | [`expr.try.option`](../spec/05-expressions.md#r-expr.try.option) | Propagates `.None` or `.Err`. | Error Conversion. |
| S24 | `T?` | [`types.option.sugar`](../spec/04-type-system.md#r-types.option.sugar) | Sugar for `Option[T]`. | O1. |
| S25 | `.Variant` | [`expr.name.contextual`](../spec/05-expressions.md#r-expr.name.contextual) | Variant from the expected type. | No search over enums. |
| S26 | `enum E(i32): A -> E(200)` | [`data.shared.declare`](../spec/08-data-and-enums.md#r-data.shared.declare) | Per-variant shared constants. | Enum Semantics 1-4. |
| S27 | GADT `-> E[i64]` | [`gadt.result.declare`](../spec/13-gadts.md#r-gadt.result.declare) | Variant result refinement; shares `->` with S26. | |
| S28 | `+T`, `-T` | [`types.variance.markers`](../spec/04-type-system.md#r-types.variance.markers) | Declared variance. | |
| S29 | `[T = D]` | [`grammar.generic.default`](../spec/02-grammar.md#r-grammar.generic.default) | Type-argument defaults. | TD1-TD11. |
| S30 | `Tr[Item = T]` | [`grammar.generic.binding`](../spec/02-grammar.md#r-grammar.generic.binding) | Associated-type bindings. | AT1-AT7. |
| S31 | `type X(T)`, `i32(m)` | [`types.newtype.decl`](../spec/04-type-system.md#r-types.newtype.decl), [`types.newtype.construct`](../spec/04-type-system.md#r-types.newtype.construct) | Newtype; the base constructor unwraps it. | |
| S32 | `type R = A + B`, `type N = $()` | [`req.row.alias.decl`](../spec/11-requirements-and-suspension.md#r-req.row.alias.decl) | Row aliases. | RU2. |
| S33 | `defer:` | [`flow.defer.register`](../spec/06-control-flow.md#r-flow.defer.register) | Cleanup suite. | Owner principle: `defer` stays. |
| S34 | Loop `else` and `break value` | [`flow.loop.else.value`](../spec/06-control-flow.md#r-flow.loop.else.value) | Value-producing loops. | |
| S35 | `[for x in xs => e]`, `{for ... => k: v}` | [`expr.comp.list`](../spec/05-expressions.md#r-expr.comp.list), [`expr.comp.map`](../spec/05-expressions.md#r-expr.comp.map) | Comprehensions. | Initial spec. |
| S36 | `` `type` `` | [`lex.raw.form`](../spec/01-lexical-structure.md#r-lex.raw.form) | Raw identifiers for reserved words. | GQ6. |
| S37 | `@value`, `@derive(...)` | [`grammar.annot.item-targets`](../spec/02-grammar.md#r-grammar.annot.item-targets), [`grammar.annot.derive-traits`](../spec/02-grammar.md#r-grammar.annot.derive-traits) | Decorators; `@derive` has its own production. | D1-D10. |
| S38 | Leading `.` continuation | [`lex.dot.continue`](../spec/01-lexical-structure.md#r-lex.dot.continue) | 12 rules, with open-suite and `.Variant` exceptions. | GQ7. |
| S39 | Leading `\|>` continuation | [`lex.pipe.continue`](../spec/01-lexical-structure.md#r-lex.pipe.continue), [`lex.pipe.no-dot-line`](../spec/01-lexical-structure.md#r-lex.pipe.no-dot-line) | Continues a chain; no dot line after a `\|>`. | PL6, CS2, PL15. |
| S40 | Line starts: `(`, `[`, `{`, `!`, operators | [`lex.continue.suffix-line`](../spec/01-lexical-structure.md#r-lex.continue.suffix-line), [`lex.continue.paren-line`](../spec/01-lexical-structure.md#r-lex.continue.paren-line), [`lex.continue.no-other-operator`](../spec/01-lexical-structure.md#r-lex.continue.no-other-operator) | Such a line never continues the previous one. | GQ8, Q1, PL6. |
| S41 | Same-line suites | [`grammar.inline.no-comma`](../spec/02-grammar.md#r-grammar.inline.no-comma), [`grammar.inline.no-if`](../spec/02-grammar.md#r-grammar.inline.no-if), `grammar.inline.multi-name-for` | No comma, no same-line `if`, no multi-name `for` inside one. | GQ9, Let 7, Q1a. |
| S42 | Closure ends inside brackets | [`lex.closure.end`](../spec/01-lexical-structure.md#r-lex.closure.end), [`lex.colon.trailing-block`](../spec/01-lexical-structure.md#r-lex.colon.trailing-block) | Stricter end lines; trailing-block colons only at depth zero. | GQ2, GQ10, TB1. |

### Magic Names

Names that the compiler, the toolchain, or the prelude gives a meaning.

| # | Names | Anchor | Meaning |
| ---: | --- | --- | --- |
| N1 | Prelude table (56 names) | [Prelude](../spec/10-modules.md#prelude) | Implicit `use` of `std.core`, `std.format`, `std.cmp`, `std.hash`, `std.iter`, `std.console`, `std.testing`, `std.task`, `std.annotation` names. |
| N2 | `panic`, `println`, `debug`, `it`, `shape`, `shape_of` | [Prelude Functions](../spec/10-modules.md#prelude-functions) | Prelude functions; `shape` and `shape_of` are intrinsics. |
| N3 | `Some`, `None`, `Ok`, `Err` | [`types.option.prelude-name`](../spec/04-type-system.md#r-types.option.prelude-name), [`types.result.prelude-name`](../spec/04-type-system.md#r-types.result.prelude-name) | Variants, deliberately not prelude names. |
| N4 | `main`, `main!` | [`module.entry.definition`](../spec/10-modules.md#r-module.entry.definition) | Entry points when public and parameterless. |
| N5 | `self`, `Self` | [`names.self-type.reserved`](../spec/03-names-and-scopes.md#r-names.self-type.reserved) | Receiver and implementing type. |
| N6 | `_0`, `_1`, ... | [`names.tuple-member.underscore`](../spec/03-names-and-scopes.md#r-names.tuple-member.underscore) | Tuple members, unnamed shared data, positional payload members. |
| N7 | `_` | [`lex.ident.placeholder`](../spec/01-lexical-structure.md#r-lex.ident.placeholder) | Pipe placeholder, discard, catch-all, and inference slot. |
| N8 | Contextual words | [Contextual Words](../spec/01-lexical-structure.md#contextual-words) | `pkg`, `std`, `dep`, `super`, `as`, `use`, `reified`, `context`, `with`, `Context`, `pack`, `map`, `map_list`, `derive`, `by`. |
| N9 | `tests` | [`lex.keyword.tests`](../spec/01-lexical-structure.md#r-lex.keyword.tests), [`grammar.use.tests-root`](../spec/02-grammar.md#r-grammar.use.tests-root) | Reserved word, block keyword, and use root. |
| N10 | `mod.hd` | [`module.path.directory`](../spec/10-modules.md#r-module.path.directory) | Directory module. |
| N11 | `*_test.hd`, `tests/` | [`module.test.module`](../spec/10-modules.md#r-module.test.module), [`module.test.integration`](../spec/10-modules.md#r-module.test.integration) | Test modules and integration tests. |
| N12 | `src`, `hd.toml`, `hd.sum` | [`module.manifest.file`](../spec/10-modules.md#r-module.manifest.file), [`module.sum.file`](../spec/10-modules.md#r-module.sum.file) | Source root, manifest, integrity file. |
| N13 | `__snapshots__`, `__regressions__` | [`std-testing.snapshot-file.path`](../spec/std/testing.md#r-std-testing.snapshot-file.path), [`std-testing.prop.regression-file`](../spec/std/testing.md#r-std-testing.prop.regression-file) | Runner-owned folders. |
| N14 | `github.com` | [`module.repo.github`](../spec/10-modules.md#r-module.repo.github) | The one known host. |
| N15 | `std.ops.NumSuffix`, `StrPrefix`, `Template` | [`expr.literal-fn.marker`](../spec/05-expressions.md#r-expr.literal-fn.marker) | Literal-sugar markers. |
| N16 | `std.annotation.Annotate` | [`annot.target.recognized`](../spec/14-annotations.md#r-annot.target.recognized) | Target-kind limiter. |
| N17 | `std.structure.Structure` | [`annot.structure.sealed`](../spec/14-annotations.md#r-annot.structure.sealed) | Derivation view. |
| N18 | `std.convert.From`, `std.error.Error`, `std.process.Termination`, `std.format.Display` | [`trait.from.propagation`](../spec/09-traits.md#r-trait.from.propagation), [`module.entry.termination`](../spec/10-modules.md#r-module.entry.termination) | Traits the language calls without an import. |
| N19 | `std.task.block_on`, `all!`, `race!`, `host_wait!` | [`req.drive.block-on`](../spec/11-requirements-and-suspension.md#r-req.drive.block-on) | Drivers and combinators. |
| N20 | `std.function.Fn`, `SuspendFn`, `Rest` | [`fn.type.ctor.decl`](../spec/07-functions.md#r-fn.type.ctor.decl) | Function type constructors. |
| N21 | `std.time` `ms`, `s`, `min`, `h`; `std.text.r` | [`std-time.suffix.std.only-four`](../spec/std/time.md#r-std-time.suffix.std.only-four), [`std-text.prefix.std.only-r`](../spec/std/text.md#r-std-text.prefix.std.only-r) | The standard suffixes and prefix. |
| N22 | `Iterator::from_fn`, `iter`, `next` | [`flow.for.iterator-from-fn`](../spec/06-control-flow.md#r-flow.for.iterator-from-fn) | The iteration protocol. |
| N23 | `T::zero`, `one`, `from_i64` | [`trait.num.members`](../spec/09-traits.md#r-trait.num.members) | Numeric constants in generic code. |
| N24 | Panic categories | [`flow.panic.category-set`](../spec/06-control-flow.md#r-flow.panic.category-set) | 13 names, also the `expect_panic` vocabulary. |

### Special Diagnostics

Codes that serve one construct. General codes, such as `type-mismatch`,
`unsatisfied-trait-bound`, and `missing-requirement`, are not listed.

| Construct | Codes | Count |
| --- | --- | ---: |
| Test blocks and cases | `duplicate-tests-block`, `misplaced-tests-block`, `public-test-item`, `invalid-test-statement`, `misplaced-test-case`, `non-literal-test-argument`, `duplicate-test-name`, `unknown-panic-category`, `test-only-use`, `cyclic-test-dependency` | 10 |
| Pipes | `duplicate-pipe-placeholder`, `pipe-placeholder-in-closure`, `pipe-step-needs-placeholder`, `suspending-pipe-step`, `multi-line-pipe-step`, `placeholder-outside-pipe` | 6 |
| Typed derivation | `underivable-trait`, `misplaced-derivation`, `structure-outside-template`, `marker-template`, `invalid-member-line`, `omitted-member-without-default`, `member-not-derivable`, `generic-member-call`, `newtype-derivation-self`, `gadt-derivation`, `unknown-annotation-member`, `duplicate-fact`; warnings `derivation-line-drift`, `unused-derivation-fact` | 14 |
| Comparison derivations | `derive-field-missing-trait`, `missing-derived-bound`, `mixed-derived-law` | 3 |
| Error derivation | `invalid-error-marker` | 1 |
| Decorators | `decorator-not-annotator`, `decorator-not-top-level`, `decorator-target-kind` | 3 |
| Embedding | `embedded-copy-required`, `copy-into-ordinary-field`, `too-many-embedded-fields`, `embedding-too-deep`, `embedded-non-data`, `duplicate-embedded-field`, `mutable-embedded-field`, `invalid-delegation` | 8 |
| Literal sugar | `invalid-literal-suffix`, `invalid-string-prefix` | 2 |
| Shapes | `unknown-shape-target` | 1 |
| Requirements | `row-parameter-in-context`, `inspectable-requirement`, `ambiguous-row-pattern`, `generic-requirement-key-collision`, `nonhost-entry-requirement`, `requirement-in-default` | 6 |
| Suspension | `bang-call-outside-suspension`, `not-suspending`, `suspending-defer` | 3 |
| `defer` | `defer-control-flow`, `defer-outside-cleanup-scope` | 2 |
| Packs (removed in batch 31b) | `pack-length-mismatch`, `pack-map-mapper-mismatch`, `multiple-positional-value-packs`, `nonfinal-positional-value-pack` | 4 |
| Varargs and spreads | `nonfinal-vararg`, `nonfinal-positional-spread`, `positional-spread-needs-vararg` | 3 |
| `is` | `identity-needs-reference-bound`, `identity-requires-references`, `incompatible-identity-operands`, `unsupported-function-identity` | 4 |
| `==` and `<` | `missing-eq`, `missing-partial-ord`, `unsupported-equality` | 3 |
| Numeric operators | `unsigned-negation`, `nonnumeric-unary-plus`, `mixed-numeric-types`, `comparison-chaining` | 4 |
| Bindings | `multi-binding-needs-parentheses`, `mut-on-primitive`, `let-mut-readonly-type`; warning `redundant-let-mut` | 4 |
| Trailing blocks | `trailing-block-position` | 1 |
| Old spellings | `old-bound-operator`, `old-row-separator`, `old-import-declaration`, `old-export-declaration`, `old-struct-declaration` | 5 |
| Lexical | `reserved-semicolon`, `unexpected-bom`, `doc-comment-without-target`, `tab-whitespace` | 4 |
| Recursion | `recursive-function-needs-result-type`, `recursive-closure-needs-result-type` | 2 |
| Field modifiers | `mutable-field-modifier` | 1 |
| Map keys | `invalid-map-key` | 1 |
| GADTs | `variant-result-owner`, `impossible-gadt-pattern` | 2 |
| Variant constructors | `unsaturated-enum-constructor` | 1 |
| Loop values | `break-value-context` | 1 |
| Module graph | `folder-cycle`, `package-cycle`, `re-export-loop` | 3 |
| Patterns | `bare-variant-pattern`; warning `variant-binding-name-mismatch` | 2 |
| Implementation heads | `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target`, `nonlocal-impl`, `local-impl-nonlocal-pair` | 6 |
| Sealed traits | `sealed-trait-implementation` | 1 |
| Bound depth | `trait-resolution-depth` | 1 |
| Variance | `variance-representation-change` | 1 |

## Cut Candidates

Each cut names the existing rule that absorbs it and shows parsed before
and after examples. It states its cost, checks soundness, and accounts for
every rule, code, production, and fixture it touches. Parsing checks syntax only;
no block is claimed to type-check. A line that parses only after a cut ends
in `# hypothetical syntax`.

| Rank | Cut | Design cost order | Items removed | Changes valid source | Soundness |
| ---: | --- | --- | --- | --- | --- |
| 1 | [C1](#c1-one-code-for-an-operator-with-no-meaning) | diagnostic only | 6 codes | no | holds |
| 2 | [C2](#c2-diagnostic-twins) | diagnostic only | 4 codes | no | holds |
| 3 | [C3](#c3-value-packs-follow-the-vararg-rule) | rule exception removed | 2 codes, 2 rules | no | holds with condition |
| 4 | [C4](#c4-parenthesized-names-in-for) | syntax variant removed | 1 exception, 1 spelling | yes, with a fix-it | holds |
| 5 | [C5](#c5-one-dollar-rule-for-every-string) | rule exception removed | half of 1 rule | invalid becomes valid | holds |
| 6 | [C6](#c6-readonly-iterators-in-loops) | rule exception removed | 1 rule | invalid becomes valid | holds |
| 7 | [C7](#c7-suspension-in-comprehensions) | rule exception removed | 1 rule | invalid becomes valid | holds |
| 8 | [C8](#c8-map-keys-through-the-ordinary-bound) | intrinsic rule becomes a library bound | 1 code, 1 rule | variant A: invalid becomes valid | A opens a narrow hole; B holds |
| 9 | [C9](#c9-one-meaning-for-map-indexing) | rule exceptions removed | 4 rules, 1 note | yes: `m[k]` changes type | holds |

Cuts C1, C2, and C3 are independent. C4 to C9 are independent of each
other and of the first three.

### C1. One Code For An Operator With No Meaning

Six codes report "this operator has no meaning for these operands". Every
other operator reports the same failure as `type-mismatch`, whose message
names the missing trait ([`expr.op.no-impl`](../spec/05-expressions.md#r-expr.op.no-impl)).

**Before.**

```text
data Point:
    x: i32

fn same(a: Point, b: Point) -> bool:
    a == b  # error: missing-eq

fn less(a: Point, b: Point) -> bool:
    a < b  # error: missing-partial-ord

fn flip(n: u32) -> u32:
    -n  # error: unsigned-negation

fn lift(text: string) -> string:
    +text  # error: nonnumeric-unary-plus

fn grow(base: i32, rate: f64) -> f64:
    base ** rate  # error: mixed-numeric-types

fn scale(a: Point, b: Point) -> Point:
    a * b  # error: type-mismatch
```

**After.** The same programs stay rejected, with the code that `a * b`
already gets. `assert_equal` reports its failed `Eq` bound as it reports a
failed `Debug` bound.

```text
use std.testing.assert_equal

data Point:
    x: i32

fn same(a: Point, b: Point) -> bool:
    a == b  # error: type-mismatch

fn less(a: Point, b: Point) -> bool:
    a < b  # error: type-mismatch

fn flip(n: u32) -> u32:
    -n  # error: type-mismatch

fn check(a: Point) -> void:
    assert_equal(a, a, reason="same point")  # error: unsatisfied-trait-bound
```

**Absorbed by.** [`expr.op.no-impl`](../spec/05-expressions.md#r-expr.op.no-impl)
and [`expr.arith.non-numeric-no-impl`](../spec/05-expressions.md#r-expr.arith.non-numeric-no-impl)
for operators; [`trait.bound.unsatisfied`](../spec/09-traits.md#r-trait.bound.unsatisfied)
for `assert_equal`'s `T < Eq & Debug`, as
[`module.testing.assert-equal-debug`](../spec/10-modules.md#r-module.testing.assert-equal-debug)
already does for `Debug`.

**Cost.** A tool that filters on the code loses the finer split. The
message keeps it: [`expr.op.no-impl`](../spec/05-expressions.md#r-expr.op.no-impl)
already requires the message to name the missing trait. SF3 renamed
`missing-partial-eq` to `missing-eq` on 2026-09-29 because `PartialEq` was
gone; merging would retire the renamed code.

**Soundness.** Each removed code guarded a program that stays rejected.
Only the code changes. No valid program changes meaning.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `missing-eq` | deleted; `==` reports `type-mismatch`, `assert_equal` reports `unsatisfied-trait-bound` |
| `missing-partial-ord` | deleted; `<` reports `type-mismatch`. No rule named it before. |
| `unsupported-equality` | deleted; `==` on a function value reports `type-mismatch` |
| `nonnumeric-unary-plus` | deleted; reports `type-mismatch`. No rule named it before. |
| `unsigned-negation` | deleted; reports `type-mismatch` |
| `mixed-numeric-types` | deleted; reports `type-mismatch`, as `i + f` already does |
| [`module.testing.no-implicit-eq`](../spec/10-modules.md#r-module.testing.no-implicit-eq) | reworded: `unsatisfied-trait-bound` |
| [`expr.eq.no-implicit`](../spec/05-expressions.md#r-expr.eq.no-implicit), [`expr.eq.functions`](../spec/05-expressions.md#r-expr.eq.functions) | reworded: name `type-mismatch` |
| [`expr.arith.unary-plus`](../spec/05-expressions.md#r-expr.arith.unary-plus), [`expr.arith.unary-minus`](../spec/05-expressions.md#r-expr.arith.unary-minus), [`expr.power.mixed`](../spec/05-expressions.md#r-expr.power.mixed) | reworded: name `type-mismatch` |
| [`expr.power.negated-literal.not-other`](../spec/05-expressions.md#r-expr.power.negated-literal.not-other) | reworded: drop the mention of `unsigned-negation` |
| [`expr.ord.partial-cmp`](../spec/05-expressions.md#r-expr.ord.partial-cmp) | unchanged; the error follows `expr.op.no-impl` |
| Fixtures `assert-equal-non-eq.hd`, `assert-equal-fieldless-data-without-partial-eq.hd` | marker becomes `unsatisfied-trait-bound`; 2 `cases.tsv` rows |
| Fixtures `implicit-data-equality.hd`, `payload-free-enum-equality.hd`, `implicit-data-ordering.hd`, `bool-ordering.hd`, `function-equality.hd`, `nonnumeric-unary-plus.hd`, `unsigned-negation.hd`, `mixed-numeric-power.hd`, `power-mixed-numeric-types.hd` | marker becomes `type-mismatch`; 9 `cases.tsv` rows |
| [Diagnostics](../spec/README.md#diagnostics) table | 6 codes leave the Error row |

**Other languages.** Rust reports every binary operator without an
implementation, `==` and `<` included, as one code, E0369
([Rust error index](https://doc.rust-lang.org/error_codes/E0369.html)). A
failed trait bound is the separate E0277
([Rust error index](https://doc.rust-lang.org/error_codes/E0277.html)).

**Recommendation.** Take all six. Question [Q3](#q3-operator-operand-codes).

### C2. Diagnostic Twins

Four pairs of codes each report one mistake in two places. The first code
of each pair can report both.

| Twin removed | Absorbing code | Same mistake |
| --- | --- | --- |
| `suspending-defer` | `suspension-forbidden-context` | suspending where the context forbids it; comprehensions, defaults, facts, and `block_on` in `defer` already use the absorbing code |
| `identity-needs-reference-bound` | `identity-requires-references` | an `is` operand whose type is not `AnyRef` |
| `recursive-closure-needs-result-type` | `recursive-function-needs-result-type` | recursion through a callable with no written result |
| `mutable-embedded-field` | `mutable-field-modifier` | `mut` written as a data-member modifier |

**Before.**

```text
data Base:
    id: i32

data Post:
    mut Base  # error: mutable-embedded-field

fn cleanup!() -> void:
    pass

fn close!() -> void:
    defer:
        cleanup!()  # error: suspending-defer
    pass

fn same[T](a: T, b: T) -> bool:
    a is b  # error: identity-needs-reference-bound

fn count(limit: i32) -> i32:
    down := fn(n: i32):  # error: recursive-closure-needs-result-type
        if n == 0: 0 else: down(n - 1)
    down(limit)
```

**After.**

```text
data Base:
    id: i32

data Post:
    mut Base  # error: mutable-field-modifier

fn cleanup!() -> void:
    pass

fn close!() -> void:
    defer:
        cleanup!()  # error: suspension-forbidden-context
    pass

fn same[T](a: T, b: T) -> bool:
    a is b  # error: identity-requires-references

fn count(limit: i32) -> i32:
    down := fn(n: i32):  # error: recursive-function-needs-result-type
        if n == 0: 0 else: down(n - 1)
    down(limit)
```

**Absorbed by.** [`flow.defer.block-on`](../spec/06-control-flow.md#r-flow.defer.block-on)
and `expr.comp.no-suspension`
(`suspension-forbidden-context`); [`expr.is.primitive`](../spec/05-expressions.md#r-expr.is.primitive)
(`identity-requires-references`); [`fn.decl.omitted-cycle`](../spec/07-functions.md#r-fn.decl.omitted-cycle)
(`recursive-function-needs-result-type`);
[`data.field.no-mut-modifier`](../spec/08-data-and-enums.md#r-data.field.no-mut-modifier)
(`mutable-field-modifier`).

**Cost.** Four codes lose their context word. Each message can keep it,
as in "a `defer` suite cannot suspend". The recursion code's name says
"function" where the callable is a closure.

**Soundness.** Every program the four codes rejected stays rejected.

**Rule accounting.**

| Item | Change |
| --- | --- |
| [`flow.defer.suspend`](../spec/06-control-flow.md#r-flow.defer.suspend) | reworded: `suspension-forbidden-context` |
| [`types.generic.identity`](../spec/04-type-system.md#r-types.generic.identity) | reworded: `identity-requires-references` |
| [`fn.closure.recursive-result`](../spec/07-functions.md#r-fn.closure.recursive-result) | reworded: `recursive-function-needs-result-type` |
| [`data.field.embedded-no-mut`](../spec/08-data-and-enums.md#r-data.field.embedded-no-mut) | reworded: `mutable-field-modifier` |
| `suspending-defer`, `identity-needs-reference-bound`, `recursive-closure-needs-result-type`, `mutable-embedded-field` | deleted from the Diagnostics table |
| Fixtures `defer-bang-call-cold-twin.hd`, `defer-suspends.hd`, `bang-call-in-defer.hd` | marker becomes `suspension-forbidden-context`; 3 rows |
| Fixture `unbounded-generic-identity.hd` | marker becomes `identity-requires-references`; 1 row |
| Fixture `recursive-closure-inferred-result.hd` | marker becomes `recursive-function-needs-result-type`; 1 row |
| Fixture `parse/invalid/mutable-embedded-field.hd` | marker becomes `mutable-field-modifier`; 1 row |

**Other languages.** Python reports `await` outside an async function as
one `SyntaxError`, in a comprehension or not
([PEP 530](https://peps.python.org/pep-0530/)).

**Recommendation.** Merge all four pairs. Question [Q2](#q2-diagnostic-twins).

### C3. Value Packs Follow The Vararg Rule

**Superseded.** Batch 31b removed packs and both codes on 2026-09-30, so
this cut no longer applies.

A value pack is the heterogeneous form of a vararg. Its own two codes
restate the vararg finality rule.

**Before.**

```text
fn split[As..., Bs...](left: As..., right: Bs...) -> void:  # error: multiple-positional-value-packs
    pass

fn tail[Ts...](values: Ts..., last: i32) -> void:  # error: nonfinal-positional-value-pack
    pass

fn scaled(values...: List[i32], factor: i32) -> i32:  # error: nonfinal-vararg
    factor
```

**After.** Two packs always leave the first one non-final, so the finality
rule alone rejects them.

```text
fn split[As..., Bs...](left: As..., right: Bs...) -> void:  # error: nonfinal-vararg
    pass

fn tail[Ts...](values: Ts..., last: i32) -> void:  # error: nonfinal-vararg
    pass

fn scaled(values...: List[i32], factor: i32) -> i32:  # error: nonfinal-vararg
    factor
```

**Absorbed by.** [`fn.vararg.final`](../spec/07-functions.md#r-fn.vararg.final),
which `grammar.fn.vararg.value-pack` (since retired)
already extends to value packs.

**Cost.** The two-pack message loses its own name; it can still say "only
one pack may be positional".

**Soundness.** Holds while hd has no named-only parameters.
`pack.value.at-most-one` (since retired)
says so itself. If named-only parameters arrive, a second pack could follow
a separator and be final, and the at-most-one rule would have to return.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `pack.value.positional` (since retired) | deleted; a Note states the consequence |
| `pack.value.at-most-one` (since retired) | deleted; merged into that Note |
| `pack.value.final` (since retired) | reworded: Error `nonfinal-vararg` |
| `grammar.fn.vararg.value-pack` (since retired) | reworded: Error `nonfinal-vararg` |
| `multiple-positional-value-packs` | deleted; the same program reports `nonfinal-vararg` |
| `nonfinal-positional-value-pack` | merged into `nonfinal-vararg` |
| Fixtures `multiple-positional-value-packs.hd`, `nonfinal-positional-value-pack.hd` | marker becomes `nonfinal-vararg`; 2 rows |

**Other languages.** None needed: the cut merges codes inside hd and
changes no rule's reach.

**Recommendation.** Take it. Question [Q1](#q1-value-pack-codes).

### C4. Parenthesized Names In `for`

hd writes a name list three ways: `let (a, b) = p`, `(a, b) := p`, and
`for a, b in m:`. Batches 7 and 13 put the first two in parentheses because
"one shape reads better than two spellings". The `for` form is the one left.

**Before.**

```text
fn total(scores: Map[string, i32], ready: bool) -> i32 $ Console:
    let (low, high) = (0, 0)
    (first, second) := (1, 2)
    let sum = low + high + first + second
    for name, score in scores:
        sum = sum + score
    if ready: for name, score in scores: println(name)  # error: syntax-error
    sum
```

**After.** The loop and comprehension take the same parenthesized list.
Its commas sit inside parentheses, so a same-line body works, as it does
for `if ok: (a, b) := p`.

```text
fn total(scores: Map[string, i32], ready: bool) -> i32 $ Console:
    let sum = 0
    for (name, score) in scores:  # hypothetical syntax
        sum = sum + score
    if ready: for (name, score) in scores: println(name)  # hypothetical syntax
    labels := [for (name, score) in scores => name]  # hypothetical syntax
    sum
```

**Absorbed by.** `grammar.stmt.bind-list` (now [`grammar.stmt.short-binding.one-name`](../spec/02-grammar.md#r-grammar.stmt.short-binding.one-name))
and `grammar.inline.bind-list`:
the `binding_list` production replaces `binding_pattern`.

**Cost.** Every multi-name loop gains two characters: 18 fixtures, 3 spec
examples, and the tour. The fix-it is mechanical, as for Q1. Python, a
common source of agent habits, writes the bare form.

**Soundness.** The change is syntactic. Destructuring, arity checks
(`flow.for.tuple-arity` (since retired)),
and scopes are unchanged.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `binding_pattern` production | deleted; `for_expression`, `statement_for_expression`, `indented_for_expression`, and `comprehension_for` take `binding_target` |
| `grammar.inline.multi-name-for` | deleted: absorbed by `grammar.inline.bind-list` |
| [`grammar.inline.loops`](../spec/02-grammar.md#r-grammar.inline.loops) | reworded: a same-line `for` takes one name or a list |
| `flow.for.tuple-binding` (since retired) | reworded: `for (key, value) in entries` |
| New rule, as `grammar.stmt.bind-list.bare` | added: `for a, b in m` is `syntax-error` with a fix-it |
| [`names.comp.for`](../spec/03-names-and-scopes.md#r-names.comp.for) | unchanged |
| 18 fixtures with a multi-name loop, including `for-tuple-binding-else.hd` and `bracketed-for-multi-name-binding.hd` | rewritten with parentheses; rows unchanged |
| One new invalid fixture for the bare form | added |

**Other languages.** Rust and Swift write `for (k, v) in m` as a tuple
pattern ([Rust reference, for loops](https://doc.rust-lang.org/reference/expressions/loop-expr.html#iterator-loops),
[Swift, for-in](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/controlflow/#For-In-Loops)).
Python writes the bare form.

**Recommendation.** Take it; it completes the Q1 decision. Question
[Q7](#q7-parentheses-in-for).

**Owner decision (batch 26, 2026-09-30): taken**, as Syntax And Semantics
Cost Q4.

### C5. One Dollar Rule For Every String

A `$` that begins no interpolation is an error in a plain string and text
in a prefixed string. The small follow-ups of 2026-09-29 fixed the prefixed
side; the plain side kept the older error.

**Before.**

```text
use std.text.r

fn prices() -> void:
    raw := r"costs $5"
    plain := "costs $5"  # error: syntax-error
    sign := "$"  # error: syntax-error
    pass
```

**After.** One rule for both kinds: a `$` followed by neither `{` nor an
identifier start is text. A `$` before a reserved word other than `self`
stays an error.

```text
fn prices() -> void:
    plain := "costs $5"  # hypothetical syntax
    sign := "$"  # hypothetical syntax
    flag := "$true"  # error: syntax-error
    pass
```

**Absorbed by.** `lex.prefix.plain-dollar-start`
and `lex.prefix.reserved-dollar`,
stated once for every string.

**Cost.** A plain string can no longer catch a stray `$` as a typo. `\$`
stays valid, so no valid string changes value.

**Soundness.** The removed half rejected text that has one reading. No
valid program changes meaning; two invalid ones become valid.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `lex.interp.stray-dollar` | reworded: keeps the reserved-word error, drops the non-identifier error |
| `lex.prefix.plain-dollar-start` | merged into one rule for every string |
| `string_character`, `multiline_string_character` classes | reworded: a `$` that begins no interpolation is text |
| Fixtures `parse/invalid/stray-dollar.hd`, `parse/invalid/stray-dollar-in-string.hd` | move to `parse/valid`; 2 rows become `accept` |
| A fixture for `"$true"` | unchanged or added |

**Other languages.** Kotlin, whose interpolation hd follows, keeps a `$`
that begins no template as text, as in `"costs $5"`
([Kotlin, string templates](https://kotlinlang.org/docs/strings.html#string-templates)).

**Recommendation.** Take it. Question [Q4](#q4-dollar-in-plain-strings).

**Owner decision (batch 26, 2026-09-30): taken**, as Syntax And Semantics
Cost Q3.

### C6. Readonly Iterators In Loops

A loop over a readonly `Iterator[T]` is an error, but CS10 kept a readonly
`iter()` that shares the traversal. The loop itself calls `iter()`
([`flow.for.iterable`](../spec/06-control-flow.md#r-flow.for.iterable)).

**Before.**

```text
fn drain(source: Iterator[i32]) -> List[i32]:
    [for value in source.iter() => value]

fn drain_direct(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]  # error: mutable-receiver-required
```

**After.** Both functions are valid and do the same thing.

```text
fn drain_direct(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]
```

**Absorbed by.** [`flow.for.iterable`](../spec/06-control-flow.md#r-flow.for.iterable)
and `flow.for.iterator-self`, retired by batch 24,
with the CS10 Note on shallow readonly access.

**Cost.** A readonly iterator parameter no longer signals "this function
does not advance me" at loops. It never did for `iter()` or adapters over
`iter()`.

**Soundness.** Readonly views are shallow ([`types.readonly.not-deep`](../spec/04-type-system.md#r-types.readonly.not-deep)),
and `step` is a closure, so the rule guaranteed nothing a readonly `iter()`
does not already break.

**Rule accounting.**

| Item | Change |
| --- | --- |
| [`flow.for.iterator-mut`](../spec/06-control-flow.md#r-flow.for.iterator-mut) | deleted |
| [`flow.for.comprehension`](../spec/06-control-flow.md#r-flow.for.comprehension) | unchanged |
| Fixture `typing/invalid/readonly-iterator-in-comprehension.hd` | moves to `typing/valid`; 1 row becomes `accept` |
| [`std-iter.adapter.mut-receiver`](../spec/std/iter.md#r-std-iter.adapter.mut-receiver) | unchanged: adapters still take `mut self` |

**Other languages.** Rust's `for` takes any `IntoIterator`. For an
iterator `I`, `&mut I` is an iterator too, so a loop may advance a
borrowed one ([std::iter::IntoIterator](https://doc.rust-lang.org/std/iter/trait.IntoIterator.html)).

**Recommendation.** Take it. Question [Q5](#q5-readonly-iterator-loops).

**Owner decision (batch 24, 2026-09-30): not taken.** The rule stays, and
`Iterator[T]` stops implementing `Iterable[T]` instead.

### C7. Suspension In Comprehensions

A comprehension is eager and runs where it is written. A `for` loop in a
suspending body may make bang calls; a comprehension there may not.

**Before.**

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]  # error: suspension-forbidden-context

fn names_loop!(ids: List[i32]) -> List[string]:
    let out: mut List[string] = []
    for id in ids:
        out.append(fetch!(id))
    out
```

**After.**

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]
```

**Absorbed by.** [`req.bang.driver-contexts`](../spec/11-requirements-and-suspension.md#r-req.bang.driver-contexts):
a bang call is valid in a suspending body, and a comprehension inside one
is part of it.

**Cost.** A comprehension may pause mid-way, with the partial list live in
the frame. The `!` shows it. Outside a suspending body the call stays
`bang-call-outside-suspension`.

**Soundness.** The frame already holds every local live across a
suspension point ([`req.lowering.frame`](../spec/11-requirements-and-suspension.md#r-req.lowering.frame)).
The comprehension's `return`, `break`, and `continue` ban stays.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `expr.comp.no-suspension` | deleted |
| [`expr.comp.eager`](../spec/05-expressions.md#r-expr.comp.eager) | unchanged |
| Fixture `typing/invalid/bang-call-in-comprehension.hd` | moves to `typing/valid`, or keeps its marker in a non-suspending body as `bang-call-outside-suspension`; 1 row |
| STDLIB `std.iter` text that cites the ban | reworded |

**Other languages.** Python allows `await` in a comprehension inside an
async function ([PEP 530](https://peps.python.org/pep-0530/)).

**Recommendation.** Take it. Question [Q6](#q6-bang-calls-in-comprehensions).

**Owner decision (batch 26, 2026-09-30): taken**, as Syntax And Semantics
Cost Q2; the `?` rules become a Note.

### C8. Map Keys Through The Ordinary Bound

The map-key check is a compiler rule with its own code. The standard
library already writes `Map`'s implementations with `K < Eq & Hash`
(Q-map). A declared bound on `Map` would report the same failures.

**Before.**

```text
data UserId:
    value: string

data Session:
    token: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: invalid-map-key
    let open: Map[mut Session, i32] = {}  # error: invalid-map-key
    pass
```

**After, variant A.** `Map` is declared `Map[K < Eq & Hash, V]`. A missing
trait is the ordinary bound failure, and a `mut` key type is valid when
its type implements both traits.

```text
data UserId:
    value: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: unsatisfied-trait-bound
    pass
```

**After, variant B.** As A, but a `mut` key type stays a separate rule
with the code `invalid-map-key`.

**Absorbed by.** [`trait.bound.unsatisfied`](../spec/09-traits.md#r-trait.bound.unsatisfied),
through the bound on `Map`'s declaration.

**Cost.** Variant B keeps the code for one case. Variant A adds a valid
form, `Map[mut K, V]`.

**Soundness.** Variant B holds. Variant A opens a narrow hole: code holding
a `mut` key may mutate it and leave a ghost entry. The spec already accepts
ghost entries made through another alias ([`types.map.ghost`](../spec/04-type-system.md#r-types.map.ghost)),
so A widens an accepted risk rather than adding a new one.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `types.map-key.bound` | reworded: `Map` declares `K < Eq & Hash`; A drops the `mut` clause, B keeps it |
| `invalid-map-key` | A: deleted, reports `unsatisfied-trait-bound`; B: kept for `mut` keys only |
| [`std-iter.collect.map-key`](../spec/std/iter.md#r-std-iter.collect.map-key) | unchanged |
| Fixtures `tuple-map-key.hd`, `float-map-key.hd`, `invalid-map-key.hd`, `nominal-map-key.hd`, `float-literal-map-key.hd` | marker becomes `unsatisfied-trait-bound`; 5 rows |

**Other languages.** Rust's `HashMap` states `K: Eq + Hash` on its methods
and reports a missing trait as E0277 ([HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html)).

**Recommendation.** Variant B. Question [Q8](#q8-map-key-bound).

**Owner decision (batch 26, 2026-09-30): variant B**, as Syntax And
Semantics Cost Q7.

### C9. One Meaning For Map Indexing

`m[k]` reads `V?`, while `m[k] += v` and `Index::index` read `V` and panic
on a missing key. Two of the three forms already agree.

**Before.**

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores[name]:
        .Some(value) => value
        .None => 0

fn bump(counts: mut Map[string, i32], word: string) -> void:
    counts[word] += 1
```

**After.** `m[k]` reads `V` and panics with `index-out-of-bounds`, as
`List` indexing does. The optional read is the existing `get`.

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores.get(name):
        .Some(value) => value
        .None => 0

fn strict(scores: Map[string, i32], name: string) -> i32:
    scores[name]

fn bump(counts: mut Map[string, i32], word: string) -> void:
    counts[word] += 1
```

**Absorbed by.** `expr.index.std.map-read`
becomes the only rule, and `get(self, key: K) -> V?` in
[Built-In Methods](../spec/10-modules.md#built-in-methods) serves the
optional read.

**Cost.** This changes the meaning of valid source: `m[k]` changes type
from `V?` to `V`. Code that matches on it fails to type-check, and three
accept fixtures change. The owner chose the current split on 2026-09-29
(follow-up 5, Map 6), after OP13 had left `counts[w] += 1` invalid.

**Soundness.** A missing key panics instead of producing `.None`; no bad
program becomes valid.

**Rule accounting.**

| Item | Change |
| --- | --- |
| `expr.index.map.read` | reworded: returns `V`, panics on a missing key |
| `expr.index.map.generic` | reworded: no unwrapping step |
| `expr.assign.compound.map-present` | deleted: the ordinary read is `V` |
| `expr.assign.compound.map-missing` | merged into `expr.index.map.read` |
| `expr.index.std.map-read` | reworded: no longer an exception |
| Note after Map Indexing | deleted |
| Fixtures `map-indexing.hd`, `readonly-map-mutable-values.hd`, `map-lookup-and-duplicate-keys.hd` | rewritten with `get`; rows unchanged |
| Fixtures `compound-assign-map-run.hd`, `compound-assign-map-missing-key.hd`, `index-trait-map-missing.hd` | unchanged; `cases.tsv` anchors move to `expr.index.map.read` |

**Other languages.** Rust and Python panic or raise on a missing key and
offer `get` for the optional read
([Rust HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html),
[Python dict](https://docs.python.org/3/library/stdtypes.html#dict)).
Swift returns an optional from `dict[k]`
([Swift Dictionary](https://developer.apple.com/documentation/swift/dictionary)).

**Recommendation.** Ask, but do not push: it reverses a day-old choice and
changes valid source. Question [Q9](#q9-map-indexing).

**Owner decision (batch 26, 2026-09-30): taken**, as Syntax And Semantics
Cost Q8. It reverses the 2026-09-29 split.

### Cuts Considered And Not Proposed

| Idea | Why not |
| --- | --- |
| Drop bare pipe steps, so every step has `_` | The owner chose bare steps twice, PL8 then CS2, after trying `_`-only in PL7. |
| Derive `Eq`, `Ord`, and `Hash` through templates | M18 R3 keeps them on the closed intrinsic list "permanently". |
| Multi-payload variant constructors as function values | Error Conversion 7: such a constructor "stays `unsaturated-enum-constructor`". |
| Replace comprehensions with iterator adapters | Several `for` clauses need a `flat_map` that no prelude adapter provides, so the cut needs a new mechanism. |
| Merge `void` into `()` | Removes `.Ok()`'s special spelling, but touches every signature and the `void` role the chapter separates on purpose ([`types.void.distinct`](../spec/04-type-system.md#r-types.void.distinct)). |
| Move the Built-In Methods table to STDLIB | The methods belong to prelude types, which the spec may name (AGENTS.md scope rule), and 15 or more fixtures call them. |
| One marker for `@num_suffix` and `@str_prefix` | Saves one name; the parameter type would carry the difference, and the declaration would say less. |
| Fold `requirement-in-default` into `missing-requirement` | The message would suggest adding a row, which a default can never use. |
| Fold `old-import-declaration`, `old-export-declaration`, `old-struct-declaration` into `syntax-error` | Agents write `import` and `struct` from other languages; the codes carry fix-its. |
| A `@must_use` marker in place of the must-use type list | Needs a new compiler-recognized name, so it is a design option, not a cut. |
| A signed exponent that panics when negative | C2 made `2 ** -1` a compile-time `type-mismatch`; the literal `u32` rule follows from it. |

## Keep

These entries stay. "Decided" rows cite the owner decision that settled
them; the rest give a one-line reason.

| Entry | Inventory | Decided or reason |
| --- | --- | --- |
| `std` inherent methods on built-ins | R1, R2 | Decided: STDLIB 8. |
| `string` not `Iterable` | R3 | Decided: STR1-STR6. |
| `string` `Index` without `IndexSet` | R4 | Decided: STR7-STR10. |
| `mut` dropped on a primitive `Self` | R5 | Decided: LM-b, LM-c. |
| `mut-on-primitive` for `let mut n = 0` | R6 | Decided: `let mut` follow-ups, Let 1-5. |
| Built-in indexing kept | R7 | Decided: OP7. |
| Primitive operands skip traits | R9 | Decided: OP2. |
| Left literal default | R10 | Decided: OP4. |
| `-5s` | R11 | Decided: L4. |
| Literal exponent `u32` | R12 | Decided: C2. |
| Float `Eq` exception | R15 | Decided: EQ-1. |
| `is` on functions and tuples | R17, R18 | Decided: FN_TYPE 9, Option follow-up A3. |
| Adapter callback rows | R20 | Decided: STDLIB 14-22, PS3. |
| Comprehension jump ban | R21 | Keep: no loop for `break` to leave; C7 lifts only the suspension ban. |
| Unread must-use binding is an error | R22 | Keep: the discard must be written as `_ :=`. |
| Recursive local closure | R23 | Keep: the only local recursion without forward references. |
| Defaults before a final function parameter | R24 | Decided: T40. |
| One-payload variant constructors | R25 | Decided: Error Conversion 7. |
| `.Ok()` for `void` | R27 | Keep: tied to `void`, which stays. |
| One-layer wrap | R28 | Decided: O3. |
| Test body result by `?` | R29 | Decided: T15. |
| Literal test arguments, test position, test codes | R30, I26 | Decided: T2-T54. |
| Unit tests without host providers | R31 | Decided: T18-T21. |
| `println` under a driver | R32 | Decided: MHP-1 follow-ups. |
| Prelude shadowing and redundant `use` | R33 | Decided: prelude rule; Standard-library decision 7. |
| Suffix and prefix module-scope lookup | R34 | Decided: L8, L10. |
| `pack.map(` tokens | R35 | Decided: GQ4. |
| `reified` modifier | R36 | Decided: B8. |
| `@error` and its markers | I3, R37 | Decided: Error Conversion 10, batches 9, 11, 13. |
| Decorator bare call; `@derive` stays intrinsic | R38, I1 | Decided: D5, D6. |
| `unused-derivation-fact` cases | R40 | Decided: M25, M28, M29, follow-up 8. |
| Walker strengthened bound; `Structure` positions | R41, R42 | Decided: typed derivation M1-M30. |
| Comparison derivations stay intrinsic | I2 | Decided: M12, M18 R3. |
| Sealed member-name code | R43 | Decided: Inspectable decisions 1-15. |
| Inspectable argument-only types; `Error` needs inspectable | R44, R45 | Decided: Inspectable decisions. |
| Newtype derivation positions | R46 | Decided: TQ-11; `newtype-derivation-self`. |
| Literal-default tie-break | R47 | Decided: TQ-4 follow-ups. |
| Implementation and derivation modules; no orphan exception | R50, R52 | Decided: TQ-17; owner decision of 2026-09-27. |
| Embedding markers, visibility, limits, promotion | R54-R56 | Decided: VE1-VE4, VE-S, embedding limits, single view. |
| Suspending trailing blocks | R57 | Decided: T14. |
| Row aliases and row parameters | R58, R59 | Decided: RU2, RU3, RU10. |
| Provider scoping in closures | R61 | Decided: PS1-PS3, PS3a. |
| Bare pipe steps and pipe codes | S5, S39 | Decided: PL7-PL9, CS2, PL14-PL16. |
| Leading-dot, suffix-line, paren-line, closure-end rules | S38, S40, S42 | Decided: GQ2, GQ7, GQ8, Q1. |
| Same-line suite rules | S41 | Decided: GQ9, Let 7, Q1a; C4 removes only the multi-name `for` case. |
| Trailing-block positions | S14, S42 | Decided: GQ10, TB1. |
| Parenthesized `let` and `:=` lists | S10, S11 | Decided: Let 1-7, Q1, Q1b. |
| `old-bound-operator`, `old-row-separator` | diagnostics | Decided: bound and row operators, 2026-09-28. |
| `AnyVal`, `Debug`, `debug`, `it` in the prelude | N1 | Decided: value categories; T33; exceptions to Standard-library decision 7. |
| Sealed `Num`, `Integer`, `Float` | I6 | Decided: OP9. |
| Shifts do not unify | R13 | Keep: count and value types are independent, as in Rust and Go. |
| `reserved-semicolon`, `comparison-chaining` | diagnostics | Keep: each names a habit from another language and suggests the fix. |

## Questions For The Owner

Smallest first. Each states the effect, the choices, a recommendation, and
hd code.

### Q1. Value-Pack Codes

Superseded: batch 31b removed packs and both codes.

Two codes restate the vararg finality rule for value packs
([C3](#c3-value-packs-follow-the-vararg-rule)).

- **A.** Report both as `nonfinal-vararg`, and delete the at-most-one
  rule.
- **B.** Keep both codes.

**Recommendation: A.** No program changes validity.

```text
fn split[As..., Bs...](left: As..., right: Bs...) -> void:  # error: nonfinal-vararg
    pass
```

### Q2. Diagnostic Twins

Four codes duplicate a partner code ([C2](#c2-diagnostic-twins)).

- **A.** Merge all four pairs.
- **B.** Merge only `suspending-defer` into `suspension-forbidden-context`.
- **C.** Keep all eight codes.

**Recommendation: A.** Messages keep the context word.

```text
fn cleanup!() -> void:
    pass

fn close!() -> void:
    defer:
        cleanup!()  # error: suspension-forbidden-context
    pass
```

### Q3. Operator-Operand Codes

Six codes report an operator with no meaning for its operands; `a * b`
reports the same failure as `type-mismatch`
([C1](#c1-one-code-for-an-operator-with-no-meaning)).

- **A.** All six become `type-mismatch`, and `assert_equal`'s missing `Eq`
  becomes `unsatisfied-trait-bound`.
- **B.** As A, but keep `missing-eq`, which SF3 renamed on 2026-09-29.
- **C.** Keep all six, and write rules for `missing-partial-ord` and
  `nonnumeric-unary-plus`, which have none today.

**Recommendation: A.** It closes the two-code gap and matches Rust's
single binary-operator code.

```text
data Point:
    x: i32

fn less(a: Point, b: Point) -> bool:
    a < b  # error: type-mismatch
```

### Q4. Dollar In Plain Strings

`"costs $5"` is an error, while `r"costs $5"` keeps `$5` as text
([C5](#c5-one-dollar-rule-for-every-string)).

- **A.** One rule for every string: such a `$` is text.
- **B.** Keep the error in plain strings.

**Recommendation: A.** It matches Kotlin, whose interpolation hd follows.

```text
fn label() -> string:
    "costs $5"  # hypothetical syntax
```

**Owner decision (batch 26, 2026-09-30): A**, as Syntax And Semantics
Cost Q3. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q5. Readonly Iterator Loops

A loop over a readonly iterator is an error, while a readonly `iter()`
advances the same traversal ([C6](#c6-readonly-iterators-in-loops)).

- **A.** Delete the loop error.
- **B.** Keep it.

**Recommendation: A.** The rule guards nothing after CS10.

```text
fn drain(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]
```

**Owner decision (batch 24, IT1-IT3, 2026-09-30): B, keep.** `Iterator[T]`
no longer implements `Iterable[T]`, which reverses CS10, so a readonly
iterator cannot be advanced by any path. See
[Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q6. Bang Calls In Comprehensions

A `for` loop in a suspending body may make bang calls; a comprehension may
not ([C7](#c7-suspension-in-comprehensions)).

- **A.** Allow them in a suspending body.
- **B.** Keep the ban.

**Recommendation: A.** The `!` stays visible, as in Python's PEP 530.

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]
```

**Owner decision (batch 26, 2026-09-30): A**, as Syntax And Semantics
Cost Q2. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q7. Parentheses In `for`

`let (a, b)` and `(a, b) :=` use parentheses; `for a, b in m:` does not
([C4](#c4-parenthesized-names-in-for)).

- **A.** `for (a, b) in m:`, with a fix-it for the bare form.
- **B.** Keep the bare form.

**Recommendation: A.** It completes Q1's "one shape" and removes the
same-line `for` exception.

```text
fn names(scores: Map[string, i32]) -> List[string]:
    [for (name, score) in scores => name]  # hypothetical syntax
```

**Owner decision (batch 26, 2026-09-30): A**, as Syntax And Semantics
Cost Q4. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q8. Map Key Bound

The map-key check is a compiler rule with its own code
([C8](#c8-map-keys-through-the-ordinary-bound)).

- **A.** `Map[K < Eq & Hash, V]`; `mut` keys become valid.
- **B.** As A, but a `mut` key stays `invalid-map-key`.
- **C.** Keep the rule as it is.

**Recommendation: B.** It moves the trait check into a library bound and
keeps the `mut` guard.

```text
data UserId:
    value: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: unsatisfied-trait-bound
    pass
```

**Owner decision (batch 26, 2026-09-30): B**, as Syntax And Semantics
Cost Q7. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q9. Map Indexing

`m[k]` reads `V?`, while `m[k] += v` and `Index::index` read `V`
([C9](#c9-one-meaning-for-map-indexing)). Changing it changes valid source.

- **A.** `m[k]` reads `V` and panics; `m.get(k)` reads `V?`.
- **B.** Keep the split decided on 2026-09-29.

**Recommendation: B** unless the three forms matter more than the churn.
A removes four rules.

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores.get(name):
        .Some(value) => value
        .None => 0
```

**Owner decision (batch 26, 2026-09-30): A**, as Syntax And Semantics
Cost Q8. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts) on
2026-09-30. Parsing checks syntax only. Codes such as `type-mismatch` in
the comments are checker results the parser does not report.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | C1 Before | parses |
| 2 | C1 After | parses |
| 3 | C2 Before | `mutable-embedded-field` at line 5, as marked; the parser owns this code |
| 4 | C2 After | `mutable-embedded-field` at line 5: today's code for the line the cut marks `mutable-field-modifier` |
| 5 | C3 Before | parses |
| 6 | C3 After | parses |
| 7 | C4 Before | `syntax-error` at line 7, as marked |
| 8 | C4 After | `syntax-error` at line 3, the first `# hypothetical syntax` line; the parser stops at the first error, and lines 5 and 6 are hypothetical too |
| 9 | C5 Before | `syntax-error` at lines 5 and 6, as marked |
| 10 | C5 After | `syntax-error` at lines 2 and 3, both `# hypothetical syntax`, and at line 4, as marked |
| 11 | C6 Before | parses |
| 12 | C6 After | parses |
| 13 | C7 Before | parses |
| 14 | C7 After | parses |
| 15 | C8 Before | parses |
| 16 | C8 After | parses |
| 17 | C9 Before | parses |
| 18 | C9 After | parses |
| 19 | Q1 | parses |
| 20 | Q2 | parses |
| 21 | Q3 | parses |
| 22 | Q4 | `syntax-error` at line 2, `# hypothetical syntax` |
| 23 | Q5 | parses |
| 24 | Q6 | parses |
| 25 | Q7 | `syntax-error` at line 2, `# hypothetical syntax` |
| 26 | Q8 | parses |
| 27 | Q9 | parses |

Blocks that parse may still carry checker errors in their comments, such as
`type-mismatch` or `nonfinal-vararg`; the parser does not report those.
