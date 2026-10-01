# Open Issues

The language specification is implementable for the behavior it accepts, but
the decisions below remain deliberately open and some block the broader product
claims. Implementations must not guess an extension: unsupported forms remain
errors until an issue is resolved in the specification. Runtime, ABI, library,
and tooling work is listed separately at the end. Resolved entries are
removed; the specification holds what they decided, and git history holds
their text.

## Language Design Decisions

### Follow-Ups Decided 2026-09-29 (Evening)

**Applied.** The ten evening follow-ups of 2026-09-29, the apply-pass
answers Let 1-5 and Map 6, batch 7 (Let 7), and batch 13 (Q1) are in the
specification; the [Revision Notes](../spec/README.md#revision-notes) list
each. The two that batch 15 below builds on:
- **Let 7**: a parenthesized `let` list may be a same-line suite body, as
  in `if ok: let (a, b) = pair`
  (`grammar.inline.let-list` (now [`grammar.inline.let-pattern`](../spec/02-grammar.md#r-grammar.inline.let-pattern))).
- **Q1**: a multi-name `:=` binding always uses parentheses, as in
  `(dt, key) := case`, and a line that starts with `(` begins a new
  statement ([Short Binding Lists](../spec/02-grammar.md#short-binding-lists)).

**Batch 15 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list each. The owner
answered the two points left open by applying Q1, settled two `let mut`
points (LM), and stated three clarifications:

| # | Decision | Where |
| --- | --- | --- |
| Q1a | A parenthesized `:=` list may be a same-line suite body, as in `if ok: (a, b) := pair`. | `grammar.inline.bind-list` |
| Q1b | The grouped form `(a, b := value)` is dropped; a nested use writes `((a, b) := value)`. Batch 26 (SSC Q5) removed the nested form: a multi-name `:=` is a statement only. | [Multi-Name Bindings](../spec/02-grammar.md#multi-name-bindings) |
| LM-a | A list name written `mut` whose annotated element is already `mut` warns `redundant-let-mut`. The fix-it removes the name-level `mut`, never the annotation. | [`types.bind.let-mut-pattern.redundant`](../spec/04-type-system.md#r-types.bind.let-mut-pattern.redundant), [`types.bind.let-mut-annotated.fix`](../spec/04-type-system.md#r-types.bind.let-mut-annotated.fix) |
| LM-b | `mut self` in an impl whose `Self` is primitive is not `mut-on-primitive`. | [`types.prim.no-mut.self`](../spec/04-type-system.md#r-types.prim.no-mut.self) |
| LM-c | Batch 16: in such a method the `mut` is dropped, so `self` has the plain type `Self`, as in `i32`, and `self + 1` type-checks. | [`types.prim.no-mut.self-type`](../spec/04-type-system.md#r-types.prim.no-mut.self-type) |
| CLO1 | A closure is monomorphic, and `fn[T](x: T): x` is a `syntax-error`. | [`fn.closure.monomorphic`](../spec/07-functions.md#r-fn.closure.monomorphic) |
| Q-? | The operand of `x?` gets an expected type as an inference hint, never a coercion. | [`expr.try.expected`](../spec/05-expressions.md#r-expr.try.expected) |
| Q-map | std writes the `Iterable` and `FromIterator` impls for `Map` with `K < Eq & Hash`; no new rule. | [Collect Targets](../spec/std/iter.md#collect-targets) |

**Batch 16 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list TB1 and LM-c. LM-c
answers the point left open by applying batch 15 and is in the table
above.

| # | Decision | Where |
| --- | --- | --- |
| TB1 | Editorial: the next-line rule names its code, `syntax-error`. `if close: trailing(): xxx` and `if close: trailing:` with an indented body are both errors; the valid forms indent the `if` body or write `trailing(fn(): xxx)`. | [`grammar.call.trailing-block.next-line`](../spec/02-grammar.md#r-grammar.call.trailing-block.next-line) |
| AUD | Delete four audit inputs that back no open finding: `audit/scripts/runtime/provider-config.ts`, `audit/probes/runtime/config/gate.hd`, `audit/probes/runtime/truncated/`, and `audit/scripts/runtime/suspension-fixtures.txt`. | [audit/README.md](../audit/README.md) |

**Batch 17 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list SR1 and INF-mut.
SR1 is recorded in [TESTING](TESTING.md#owner-decisions) and
[TYPED_DERIVATION](TYPED_DERIVATION.md#owner-decision-sr1-2026-09-30), and
applied in [Self References](../spec/14-annotations.md#self-references)
and [Derived Arbitrary](../spec/std/testing.md#derived-arbitrary).

| # | Decision | Where |
| --- | --- | --- |
| INF-mut | Revised by the owner ("no number widening"). When generic call inference solves one type parameter from several arguments, the only conversion is permission weakening: `mut X` and `X` meet at `X`. No numeric widening: `max(x_i32, y_i64)` is `type-mismatch`; write `max(i64(x_i32), y_i64)`, as Rust and Go do. Never a trait-value conversion (`types.lct.no-trait-value`): `cmp(user, display_value)` is `no-common-type` unless written `cmp[Display](...)`, where the explicit argument acts as an expected type, as in `types.lct.expected-trait`. It is its own rule in chapter 04's generic inference, cross-linked to the least-common-type rules but not a row of their table, since it is a narrower join. The prototype's `assert_equal` special case goes. | [Inference From Several Arguments](../spec/04-type-system.md#inference-from-several-arguments), from [`types.generic.infer.join`](../spec/04-type-system.md#r-types.generic.infer.join) |
| LM-c | Confirmed: calling a `mut self` method on a primitive needs no mutable access; the receiver is a copy, so `n.next()` leaves `n` unchanged. Applied in batch 16. | [`types.prim.no-mut.self-call`](../spec/04-type-system.md#r-types.prim.no-mut.self-call) |

**Still open from applying batch 17.** The specification states only what
the decisions say; each point below is unchanged there. SIMPLE (batch 20,
[Typed Derivation](TYPED_DERIVATION.md#owner-decision-simple-2026-09-30))
answers SR-enum and SR-args.

| # | Question | **Recommendation** |
| --- | --- | --- |
| INF-lit | Does an integer literal argument take the type solved from the other arguments in any position? Without that, `pick(1, large)` with an `i64` `large` is a `type-mismatch`, since the literal alone is `i32`, while `pick(large, 1)` checks. | Yes: a literal is not a conversion, so it takes the solved type as its expected type in any position, as Rust's integer literals do. |
| INF-code | Which code does any other conflict get, such as a `List[mut User]` and a `List[User]` (variance), a `T` and a `T?`, or two child-trait values? The decision names `type-mismatch` for numbers and `no-common-type` for trait values. | `no-common-type` where the least common type also fails (trait values, supertrait widening); `type-mismatch` otherwise, as `choose(1, true)` already is. |
| SR-omit | Does a variant's `self_ref` count a member that the derivation block omits (`cache = pass`)? | No: count only the members the derivation sees, since an omitted member takes its default and is never walked or built. |

**Batch 21 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list it. The owner
answered three points left open by applying batch 20, each as recommended:

| # | Decision | Where |
| --- | --- | --- |
| AT-any | Confirmed reading: `with[T < Inspectable](gen)` returns an opaque `Generator` holding `fn(mut Choices) -> Inspectable`. Each drawn value is erased to `Inspectable`, and the template downcasts it to the member's type; a mismatch is `explicit-panic`, naming the member. It is not a raw `Any`, which has no type test. | [`std-testing.arbitrary.with.wrap`](../spec/std/testing.md#r-std-testing.arbitrary.with.wrap) and the Note after [Derived Arbitrary](../spec/std/testing.md#derived-arbitrary)'s examples |
| ST8-newtype | Accepted: a newtype gets no `Structure` today, so a newtype that derives `Arbitrary` through its base panics with the base's name. [`annot.structure.name.newtype`](../spec/14-annotations.md#r-annot.structure.name.newtype) stays for when newtypes gain a `Structure`. | Notes in [The Structure Trait](../spec/14-annotations.md#the-structure-trait) and [Derived Arbitrary](../spec/std/testing.md#derived-arbitrary) |
| ST8-clash | The Note on generated names in Templates covers `facts` and `name`: inside a template, a clash with the derived trait's own receiverless member is resolved by qualifying, `Structure::name` vs `MyTrait::name`, as M30 does for `Structure::walk`. | [Templates](../spec/14-annotations.md#templates) |

**Still open from applying batch 20.** Spec Tiers migration step 5 applied
Testing AT-with, SIMPLE, and ST8, stating only what the decisions say
([Derived Arbitrary](../spec/std/testing.md#derived-arbitrary),
[The Structure Trait](../spec/14-annotations.md#the-structure-trait),
[Self References](../spec/14-annotations.md#self-references)). AT-code
waits for the error-code revamp (#101). ST8-self is answered by batch 25
below.

| # | Question | **Recommendation** |
| --- | --- | --- |
| AT-code | [`annot.walker.obligation.error`](../spec/14-annotations.md#r-annot.walker.obligation.error) gives `member-not-derivable` for a member that fails a source's bound. AT-with names `unsatisfied-trait-bound`, which [`std-testing.arbitrary.derive.not-derivable`](../spec/std/testing.md#r-std-testing.arbitrary.derive.not-derivable) states. So two rules name different codes for one check. | `member-not-derivable`, the code every other template reports at the opt-in, naming the member. |

**Batch 24 (owner decision, 2026-09-30).** Iterator consumption. Applied;
the [Revision Notes](../spec/README.md#revision-notes) list it. The owner
wrote: "Iterator[i32] is immutable. consume it twice in
for should be an error, unless via something like clone_mut". The owner
then chose "Iterator is not Iterable" and "no clone; iterate the source".
IT1 answers [Special Cases Q5](SPECIAL_CASES.md#q5-readonly-iterator-loops)
and [Syntax And Semantics Cost Q1](SYNTAX_SEMANTICS_COST.md#q1-readonly-iterator-loops)
with B, keep.

| # | Decision | Where |
| --- | --- | --- |
| IT1 | Keep [`flow.for.iterator-mut`](../spec/06-control-flow.md#r-flow.for.iterator-mut): a loop over a readonly iterator stays `mutable-receiver-required`. | [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols) |
| IT2 | `Iterator[T]` does not implement `Iterable[T]`, so a readonly iterator cannot be advanced by any path. `for`, and each comprehension `for` clause, accepts a value implementing `Iterable`, or a `mut Iterator[T]` directly: a rule exception in `for`. An `I < Iterable` bound no longer accepts iterators; callers `collect()` first. This reverses Chaining Study CS10 ("Iterator 2: keep, and document that iter() on an iterator shares progress"). `flow.for.iterator-self`, `flow.for.iterator-bound`, and the CS10 guide note are retired; only `List` and `Map` implement `Iterable`. `lib/std/iter.hd` drops `impl Iterable for Iterator`. | [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols) |
| IT3 | No `clone_mut` or `tee`. An iterator is single-pass: iterate twice by calling `.iter()` on the collection again, or `collect()` first, as in Rust and Go. | [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols), [Iterators](../spec/std/iter.md) |

**Batch 25 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list each. The
owner answered ST8-self, from batch 20 above, and a duplicate found while
checking the stdlib tier, each as recommended. The Call Indexing
follow-ups of the same batch, BF and BFF, are in
[Call Indexing](CALL_INDEXING.md#owner-decisions).

| # | Decision | Where |
| --- | --- | --- |
| ST8-self | Inside a template, a `Structure::` call has the template's `T` as its `Self`, so `Structure::name()` is `T`'s declared name, and `Structure::facts()` is `T`'s facts. Language tier. | `annot.template.structure-self`, since replaced by [`annot.template.qualified-self`](../spec/14-annotations.md#r-annot.template.qualified-self) (ST8-own) |
| r-merge | `std-text.prefix.std.import-text` and `std-text.prelude.text-r` both say that `std.text` declares `r` and code imports it. Merge them into one rule and retire the duplicate. | [`std-text.prefix.std.import-text`](../spec/std/text.md#r-std-text.prefix.std.import-text) |

**Still open from applying batch 25.** The owner answered ST8-own with
batch 28 below.

| # | Question | **Recommendation** |
| --- | --- | --- |
| ST8-own | The Templates clash Note says that, when `Encode` declares its own receiverless `name`, `Encode::name()` is `Encode`'s. That call has no argument either, so [`trait.assoc-call.trait.undetermined`](../spec/09-traits.md#r-trait.assoc-call.trait.undetermined) rejects it, as it rejected `Structure::name()`. | Inside a template, a call qualified by the derived trait also has `T` as its `Self`, since the template implements that trait for `T` alone. |

**Batch 26 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list each. The owner
answered AT-gen and [Syntax And Semantics Cost](SYNTAX_SEMANTICS_COST.md#questions-for-the-owner)
Q2-Q8. Each follows the recommendation, except Q6, where the owner kept
today's rules. Q5 supersedes Q1b's nested form, and Q8 reverses the
2026-09-29 split (follow-up 5, Map 6).

| # | Decision | Where |
| --- | --- | --- |
| AT-gen | Derived `Arbitrary` generates `T < Arbitrary & Inspectable` for each type parameter a member uses, so `@derive(Arbitrary)` on `data Box[T]: value: T` works without a manual block. Stdlib tier. | [Derived Arbitrary](../spec/std/testing.md#derived-arbitrary) |
| SSC Q2 (K4) | A comprehension follows the loop it abbreviates. A bang call is valid inside a comprehension in a suspending body, and the calls run one at a time, in order. The two `?` rules become a Note. | [Comprehension Restrictions](../spec/05-expressions.md#comprehension-restrictions) |
| SSC Q3 (K5) | One `$` rule for every string: a `$` that starts no interpolation is text, so `"costs $5"` is valid, as `r"costs $5"` is. A `$` before a reserved word other than `self` stays an error. | [Interpolation](../spec/01-lexical-structure.md#interpolation), [Prefixed Strings](../spec/01-lexical-structure.md#prefixed-strings) |
| SSC Q4 (K1) | A multi-name loop writes `for (k, v) in m`, in loops and in comprehension clauses, matching `let (a, b)` and `(a, b) :=`. Bare `for k, v in m` is `syntax-error` with a fix-it. The same-line multi-name `for` exception, `grammar.inline.multi-name-for`, is retired. | [Same-Line Suite Bodies](../spec/02-grammar.md#same-line-suite-bodies), [For Loops](../spec/06-control-flow.md#for-loops) |
| SSC Q5 (K3) | A multi-name `:=` is a statement only. The nested `((a, b) := p)` form and its wrapping rules are removed. | [Multi-Name Bindings](../spec/02-grammar.md#multi-name-bindings) |
| SSC Q6 (K2) | Keep today's rules: `let mut x: mut T` warns `redundant-let-mut`, and `let mut x: T` is `let-mut-readonly-type`. | unchanged |
| SSC Q7 (C8) | Map keys use an ordinary bound, `Map[K < Eq & Hash, V]`, reporting `unsatisfied-trait-bound`. A `mut` key type stays an error. | [Map Key Types](../spec/04-type-system.md#map-key-types) |
| SSC Q8 (C9) | `m[k]` reads `V` and panics with `index-out-of-bounds` on a missing key; `m.get(k)` reads `V?`. This reverses the 2026-09-29 split. | [Map Indexing](../spec/05-expressions.md#map-indexing) |

**Still open from applying batch 26.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.
The owner answered Q5-list with LP1 below.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| Q7-code | The decision says a `mut` key "stays an error" but names no code. | It keeps `invalid-map-key` ([`types.map-key.no-mut`](../spec/04-type-system.md#r-types.map-key.no-mut)), as variant B of [Special Cases C8](SPECIAL_CASES.md#c8-map-keys-through-the-ordinary-bound) proposed. **Recommendation:** keep it; the code names a rule no bound states. |

**Batch 26, LP1: patterns in `let` (owner decision, 2026-09-30).**
Applied; the [Revision Notes](../spec/README.md#revision-notes) list each.
The owner asked "can i do pattern matching in let? let Point
{x , y} = point", then chose "also refutable, with else". The LP1 details
that followed are final, each as recommended except the `:=` scope, which
the owner narrowed. Later the same day the owner narrowed it again:
`:=` binds exactly one name, and every destructuring goes through `let`.
That revision, LP1-one, replaces LP1-scope, LP1-unify, and LP1-cover
where they differ.

| # | Decision | Where |
| --- | --- | --- |
| LP1 | `let` accepts the `match` pattern grammar: data patterns (`Point { x, y }`, `Point { x: px }`), tuple patterns, nested patterns, `_`, and variant patterns. An irrefutable pattern needs no `else`, as in `let Point { x, y } = point`. | [Let Statements](../spec/02-grammar.md#let-statements) |
| LP1-else | A refutable pattern, such as a literal field, an enum variant, or `.Some(v)`, requires `else:` with a block that must diverge: `return`, `break`, `continue`, or a panic. The block never falls through, as in Rust's let-else and Swift's `guard let`. A refutable pattern without `else` is an error. | [Control Flow](../spec/06-control-flow.md) |
| LP1-scope | Patterns, let-else included, come only after `let`. `:=` keeps a name or a tuple of names, so `Point { x, y } := p` is an error: write `let`. Owner: "let's first use this, we can add in future". Revised by LP1-one. | [Short Binding Lists](../spec/02-grammar.md#short-binding-lists) |
| LP1-mut | `mut` may precede each name in a `let` pattern, as in `let Point { mut tags, name } = user`, following the `let mut` rules. | [Binding Forms](../spec/04-type-system.md#binding-forms) |
| LP1-unify | Today's `let (a, b)` and `(a, b) :=` lists become ordinary tuple patterns, with the same meaning. Revised by LP1-one: only `let (a, b)` remains. | [Let Statements](../spec/02-grammar.md#let-statements) |
| LP1-inline | A let-else body may be a same-line suite: `let .Some(user) = find(id) else: return .None`. | [Same-Line Suite Bodies](../spec/02-grammar.md#same-line-suite-bodies) |
| LP1-cover | The owner noted that `(a, b) := pair` is also ambiguous. It is the cover-grammar case: a short lookahead finds `:=`. It is kept. Superseded by LP1-one, which removes the form. | [Short Binding Lists](../spec/02-grammar.md#short-binding-lists) |
| LP1-one | `:=` binds exactly one name. Multi-name `(a, b) := value` is removed; every destructuring, tuple patterns included, uses `let`, as in `let (a, b) = pair`. So `(a, b) := pair` is an error that says to use `let`, as `Point { x, y } := p` is. The multi-name `:=` rules are retired, and every `(a, b) :=` site is respelled `let (a, b) =`. `for (k, v) in m` is not a `:=` binding and stays. | [Short Binding Lists](../spec/02-grammar.md#short-binding-lists), [Multi-Name Bindings](../spec/02-grammar.md#multi-name-bindings) |
| Q5-list | Answers the question left open from applying batch 26. `[a, b := value]` is a list whose last element is the single-name binding expression `b := value`, as `[a, (b := value)]` is. A list item already takes a binding expression. | [Multi-Name Bindings](../spec/02-grammar.md#multi-name-bindings) |

**Noted for the future, not decided.** A cover grammar for `:=`, as in
JavaScript, Python, Rust's destructuring assignment, and Elixir, would let
`:=` take patterns too. With it would come data-literal field shorthand,
`Point { x, y }` for `Point { x: x, y: y }`, so a pattern and a literal
read the same. The owner deferred both ("we can add in future").

**Still open from applying LP1.** The owner answered every point with
batch 30 below; LP-codes was not asked and waits for the code revamp
(#101).

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| LP-codes | The decisions name no codes for a refutable pattern without `else`, an `else` block that falls through, or a pattern before `:=`. | Two new codes, `refutable-let-pattern` and `let-else-falls-through` ([Let Patterns](../spec/06-control-flow.md#let-patterns)); a pattern before `:=` reuses `missing-let`. **Recommendation:** keep them until the planned code revamp. |
| LP-irrefutable-else | An `else` after an irrefutable pattern, as in `let (a, b) = pair else: return`, can never run. | It is valid and never runs (`flow.let.else.irrefutable`, since retired). **Recommendation:** add the `unreachable-code` warning on it, as Rust's `irrefutable_let_patterns` lint does. |
| LP-inline | Layout ends a same-line suite before `else`, so a let-else is never a same-line suite body. `if ok: let .Some(v) = f() else: return` is an `if` with an `else`, and its `let` is `refutable-let-pattern`. | Stated as [`grammar.stmt.let-else.not-inline`](../spec/02-grammar.md#r-grammar.stmt.let-else.not-inline). **Recommendation:** keep it; an indented body holds a let-else. |
| LP-discard | `let _ = save()` binds no name. `flow.unused.discard-form`, since retired, made `_ := expression` the only discard of a must-use value. | Nothing new: `let _ = save()` on a must-use value is `discarded-must-use-value`. **Recommendation:** keep `_ :=` as the one discard spelling. |
| LP-bare | `a, b := pair` was `syntax-error` with a fix-it that added parentheses, which would now give `missing-let`. | It stays `syntax-error`, and the fix-it writes `let (a, b) = pair` ([`grammar.stmt.short-binding.bare-list`](../spec/02-grammar.md#r-grammar.stmt.short-binding.bare-list)). **Recommendation:** keep it. |
| LP-for | `for` still takes a name or a name list, not a pattern, so `for Point { x, y } in points` is a `syntax-error`. | Unchanged; the decision covers `let` only. **Recommendation:** leave `for` as it is until patterns in `let` have been used for a while. |
| Q5-tuple | `[a, b := value]` is now a list ending in a binding, but `(a, b := value)` is still `syntax-error` (`grammar.expr.multi-binding.no-grouped`, since retired), since a tuple element takes no binding. | Unchanged. **Recommendation:** keep it; `(a, b := value)` reads as the removed multi-name binding. |

**Batches 27 and 28 (owner decisions, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list each. The owner
wrote of tuples: "change tuple to be value type, immutable", and then
"tuple is impl, spec does not know". So the spec states only tuple
semantics; the representation (TU3) is implementation work, task #122.
Batch 29 is in [Spec Tiers](SPEC_TIERS.md#still-open).

| # | Decision | Where |
| --- | --- | --- |
| TU1 | Confirmed: tuples are `AnyVal`, with no identity, and immutable. No rule changed. | [Tuple Types](../spec/04-type-system.md#tuple-types) |
| TU2 | `mut (A, B)` is an error, as `mut` on a primitive is: a tuple has no `mut` form. Tuple expressions leave `types.fresh.mutable`. An element keeps its permission, so `(mut User, i32)` is valid. | [`types.tuple.no-mut`](../spec/04-type-system.md#r-types.tuple.no-mut), [`types.fresh.mutable-outer`](../spec/04-type-system.md#r-types.fresh.mutable-outer) |
| TU-spec | No representation, layout, or flattening note about tuples anywhere in `spec/`. The shape table of the non-normative Implementation Model leaves tuples to the implementation. | [Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code) |
| ST8-own | Inside a template, a call qualified by the derived trait, such as `Encode::name()`, also has the template's `T` as its `Self`: one rule with `Structure::`. | [`annot.template.qualified-self`](../spec/14-annotations.md#r-annot.template.qualified-self) |

**Still open from applying batches 27 and 28.** The specification
applies the reading in the middle column; each point asks the owner to
confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| TU2-code | TU2 names no code for `mut (A, B)`. | A new code, `mut-on-tuple`; `mut-on-primitive` would misname a tuple. **Recommendation:** keep it until the code revamp (#101), which may merge the two. |
| TU2-let | `let mut pair = (1, 2)` was valid, since a fresh tuple had mutable access. With no `mut` form, it needs a code. | `mut-on-tuple`, in place of `mutable-upgrade`, as `let mut n = 0` is `mut-on-primitive` ([`types.bind.let-mut-tuple`](../spec/04-type-system.md#r-types.bind.let-mut-tuple)). **Recommendation:** keep it. |
| ST8-own-args | The one rule also fixes `Self` for a derived-trait call with an argument. In `Encode`'s template, `Encode::encode(item)` for an `item` that is not `T` was valid and is now `type-mismatch`. | Applied, since ST8-self already did this for `Structure::`. **Recommendation:** keep it; write `item.encode()` for another type. |

**Batch 30 (owner decision, 2026-09-30).** Applied; the
[Revision Notes](../spec/README.md#revision-notes) list each. The owner
answered the points left open by applying LP1. LP-inline and LP-bare
follow the recommendation; the other three do not.

| # | Decision | Where |
| --- | --- | --- |
| LP-irrefutable-else | An `else` after an irrefutable pattern, as in `let (a, b) = pair else: return`, is an error, not a warning. It reuses `unreachable-match-arm`, since the `else` is an arm that can never run. | [`flow.let.else.unreachable`](../spec/06-control-flow.md#r-flow.let.else.unreachable) |
| LP-inline | Keep: a let-else is never a same-line suite body. | [`grammar.stmt.let-else.not-inline`](../spec/02-grammar.md#r-grammar.stmt.let-else.not-inline), unchanged |
| LP-discard | `let _ = save()` discards a must-use value, as `_ := save()` does. Both spellings are discard forms. | [`flow.unused.discard-forms`](../spec/06-control-flow.md#r-flow.unused.discard-forms) |
| LP-bare | Keep: `a, b := pair` is `syntax-error`, with a fix-it to `let (a, b) = pair`. | [`grammar.stmt.short-binding.bare-list`](../spec/02-grammar.md#r-grammar.stmt.short-binding.bare-list), unchanged |
| LP-for | `for` takes an irrefutable pattern now, as in `for Point { x, y } in points`. A refutable one is `refutable-let-pattern`. `for (k, v) in m` is the tuple pattern, and the parenthesized-list rule is retired. | [`grammar.flow.for-pattern`](../spec/02-grammar.md#r-grammar.flow.for-pattern), [`flow.for.pattern`](../spec/06-control-flow.md#r-flow.for.pattern) |
| Q5-tuple | Owner: "keep rule simple, if this does not need a new rule". Allowing needs fewer rules: a tuple element takes an expression, as a list item does, so `(a, b := v)` is a tuple whose last element is a binding. One rule replaces four, leaving two in the section where there were five. | [`grammar.expr.multi-binding.element`](../spec/02-grammar.md#r-grammar.expr.multi-binding.element) |

**Batch 31, packs and literals (owner decisions, 2026-09-30).** Answers
the questions of [Reopen: Packs And Literal Sugar](REOPEN_PACKS_LITERALS.md#questions-for-the-owner).
It is applied in three passes: 31a (varargs, `Tuple`, and tuple spread),
31b (removing packs, Q9), and 31c (the literal rules, Q1 to Q4). All
three passes are applied; the [Revision Notes](../spec/README.md#revision-notes)
list each. Later entries supersede earlier ones: VARARG-SPELL over
VARARG-TYPE over SPREAD-SITE over Q5 and Q8.

| # | Decision | Where |
| --- | --- | --- |
| Q5 | Not as recommended: an arity-generic `Args` is written `Args < Tuple`, with a new sealed marker trait `Tuple` in `std.function`. It replaces the by-use rule `fn.type.ctor.input-kind`. | [`fn.type.ctor.tuple-trait`](../spec/07-functions.md#r-fn.type.ctor.tuple-trait), [`fn.type.ctor.inputs-tuple`](../spec/07-functions.md#r-fn.type.ctor.inputs-tuple) |
| Q7 | As recommended: `race!` is the plain signature `fn race![T](tasks...: List[mut Suspend[T]]) -> T`, with no intrinsic typing rule. | [`req.combinator.race-signature`](../spec/11-requirements-and-suspension.md#r-req.combinator.race-signature) |
| Q8, SPREAD-SITE | Not as recommended: no `call_with`; a call spreads a tuple, `f(args...)`, into fixed parameters, and `f!(args...)` for a suspending callee. `...` never marks a tuple in a type. | [Positional Spreads](../spec/05-expressions.md#positional-spreads) |
| VARARG-TYPE | Following TypeScript, the type of a vararg is the type the body sees: `List[T]`, a tuple type, or a `Tuple`-bounded parameter, solved as the tuple of the argument types. A tuple is never spread automatically. The chapter 05 and 07 spread rules merge. | [Varargs](../spec/07-functions.md#varargs) |
| VARARG-SPELL | `...` goes after the parameter name, `args...: Args`, and never in a type. Being a vararg belongs to the declaration: `f := sum` has type `fn(List[i32]) -> i32`. Every existing vararg is respelled. | [`fn.vararg.form`](../spec/07-functions.md#r-fn.vararg.form), [`fn.vararg.value`](../spec/07-functions.md#r-fn.vararg.value) |
| Q9 | As recommended (A1): packs are gone. Type and value packs, pack expansion, `pack.map`, lockstep, and the four pack diagnostics are removed; chapter 12 keeps its number with no rules. Applied in 31b. | [Variadic Generics](../spec/12-variadic-generics.md) |
| ALL-INTRINSIC | `all!` stays an intrinsic with one written typing rule: children `mut Suspend[X_i]` give `(X_1, ..., X_n)`. A type-level tuple map is rejected for now. `call` is an ordinary hd function. Applied in 31b. | [`req.combinator.all-typing`](../spec/11-requirements-and-suspension.md#r-req.combinator.all-typing) |
| Q6 | As recommended: the compiler derives `Eq`, `PartialOrd`, `Ord`, and `Hash` for every tuple arity; `lib/std` writes `Debug` up to 12 elements. Applied in 31b. | [`trait.target.tuple.derived`](../spec/09-traits.md#r-trait.target.tuple.derived), [`trait.debug.std-types`](../spec/09-traits.md#r-trait.debug.std-types) |
| Q1 | As recommended (B1's merges): one rule set for a **literal function**, marked `@num_suffix` or `@str_prefix`. Only the lexing, the parameter type per form, and the template stay separate. Cut C3: a reserved word is never a suffix or prefix, so `5else` is `5` then `else`. Applied in 31c. | [Literal Suffixes](../spec/05-expressions.md#literal-suffixes), [`lex.literal-fn.reserved`](../spec/01-lexical-structure.md#r-lex.literal-fn.reserved) |
| Q2 | As recommended: both markers stay; no declaration changes. Applied in 31c. | [`expr.literal-fn.marker`](../spec/05-expressions.md#r-expr.literal-fn.marker) |
| Q3 | As recommended: `-5s` is the ordinary negation `-(5s)`, and `std.time` implements `Neg` for `Duration`. `-128b` no longer fits an `i8` suffix parameter. Applied in 31c. | [Suffixed Literals](../spec/04-type-system.md#suffixed-literals), [`std-time.suffix.std.duration-neg`](../spec/std/time.md#r-std-time.suffix.std.duration-neg) |
| Q4 | Not as recommended: a suffix parameter's default stays allowed, reversing cut C2. `expr.suffix.fn-shape.default` is deleted as a restatement of the ordinary rule; a second parameter, with or without a default, is still rejected. Applied in 31c. | [`expr.literal-fn.shape`](../spec/05-expressions.md#r-expr.literal-fn.shape) |

**Still open from applying batch 31a.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| VA-type-code | The decisions name no code for a vararg of another type, as in `values...: i32`. | `type-mismatch` ([`fn.vararg.type.kinds`](../spec/07-functions.md#r-fn.vararg.type.kinds)). **Recommendation:** keep it until the code revamp (#101). |
| VA-ellipsis-code | `...` in a type, as in `fn(i32...) -> i32` or the old `values: i32...`, needs a code. | `syntax-error` ([`fn.type.no-ellipsis`](../spec/07-functions.md#r-fn.type.no-ellipsis)). Since 31b removed packs it is a plain grammar error. **Recommendation:** keep it. |
| VA-unbounded-code | An unbounded `Args` used as `Fn`'s inputs needs a code. | `generic-kind-mismatch`, as a non-tuple there already is, rather than `unsatisfied-trait-bound`. **Recommendation:** keep it; one rule covers both. |
| VA-tuple-then-vararg | A tuple spread before a vararg, as in `g(t...)` for `fn g(a: i32, xs...: List[i32])`, fills the vararg with one element as its collected value, so `t` must be `(i32, List[i32])`. | Applied, by [`expr.call.spread.tuple`](../spec/05-expressions.md#r-expr.call.spread.tuple): the remaining parameters, the vararg included, form one tuple. **Recommendation:** keep it; it matches the function value's type. |

**Still open from applying batch 31b.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| ALL-type-args | `all!::[i32, string](a, b)` was valid with the pack signature. `all!` now has no type parameters to fill. | Not stated; an explicit list on a callee with no generic parameters is already invalid. **Recommendation:** add it to [`req.combinator.all-direct`](../spec/11-requirements-and-suspension.md#r-req.combinator.all-direct) as `type-mismatch`. |
| ALL-plain | A plain `all(a, b)` without `!`, by the ordinary `fn!` rule, builds a cold `mut Suspend[(A, B)]`. | Applied, since [`req.combinator.all-direct`](../spec/11-requirements-and-suspension.md#r-req.combinator.all-direct) allows any direct call. **Recommendation:** keep it; `race(a, b)` behaves the same. |
| Q6-tier | The decision puts tuple `Debug` in `lib/std`, and the tier test says stdlib. But `assert_equal`, a language-tier harness item, needs `Debug` on tuples. | Kept in the language tier as [`trait.debug.std-types`](../spec/09-traits.md#r-trait.debug.std-types), capped at 12 elements; `lib/std/format.hd` implements 0 to 12. **Recommendation:** keep it until the Debug section moves to `spec/std/`. |
| Q6-others | "Debug and the other traits up to size 12" names no other trait. | Only `Debug` is promised. **Recommendation:** add `Display` or `Default` only when a use needs one. |

**Still open from applying batch 31c.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| C3-valid | With `5else` split into `5` and `else`, the line `if flag: 5else: 3` is now a valid `if` expression, not an error. | Applied, as [`lex.literal-fn.reserved`](../spec/01-lexical-structure.md#r-lex.literal-fn.reserved) gives it; `return"done"` already worked this way. **Recommendation:** keep it, and let a formatter insert the space. |
| Q3-neg-std | `Neg for Duration` negates the milliseconds, so negating the minimum `i64` duration overflows. | Not stated; checked `i64` negation panics `integer-overflow`, as the suffix overflow rule already says for scaling. **Recommendation:** keep it implicit. |

**Batch 32, simplify embedding (owner decisions, 2026-09-30).** Answers
the questions of [Simplify Embedding](SIMPLIFY_EMBEDDING.md#questions-for-the-owner).
It is applied in two passes: 32a (Q1, Q2, Q5, Q6) and 32b (Q3, Q4). Pass
32a is applied; the [Revision Notes](../spec/README.md#revision-notes) list
each.

| # | Decision | Where |
| --- | --- | --- |
| Q1 | As recommended (O1b): each delegated method is the forwarding method `Trait::m(self.E, ...)`, checked as if written. Applied in 32a. | [`trait.by.generated`](../spec/09-traits.md#r-trait.by.generated) |
| Q2 | As recommended (O1a): `E: ...e` and `x.E ...= e` store the copy-update `E { ...e }`; the mutable-edge rules and the chapter 04 and 05 copy rules merge into it. Applied in 32a. | [`data.part.construct`](../spec/08-data-and-enums.md#r-data.part.construct), [Mutable Edges](../spec/08-data-and-enums.md#mutable-edges) |
| Q3 | As recommended (O1c and the rest of O1): each fact is stated once; chapter 08 keeps a short linking summary. Pass 32b. | pending |
| Q4 | As recommended (O2b): a private own member hides a promoted one; another module gets `private-member`. Pass 32b. | pending |
| Q5 | Not as recommended (option B): both limits stay, as width and depth, and a self-embedding type gets its own code, `embedding-cycle`, whose message shows the cycle path. `depth.generic` and `names.part.depth.levels` are deleted. Applied in 32a. | [Embedding Limits](../spec/08-data-and-enums.md#embedding-limits) |
| Q6 | As recommended: value parts stay (VE1-VE4, VE-S); no O3. Nothing to apply. | [Parts And Copies](../spec/08-data-and-enums.md#parts-and-copies), unchanged |

**Still open from applying batch 32a.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| Q5-cycle-site | "Not just `embedding-too-deep`" leaves open whether a cycle also reports the depth code, and on which types. | `embedding-cycle` replaces `embedding-too-deep` for every type in the cycle, reported once per cycle on its first declared type, as `alias-cycle` is. A type outside the cycle that embeds into it still gets `embedding-too-deep`. **Recommendation:** keep it. |
| Q5-self-id | STYLE says a rule whose meaning changes gets a new ID, but the owner said to keep `data.embed.depth.self` while its code changes. | The ID is kept, as directed. **Recommendation:** keep it; the Revision Note records the code change. |
| Q1-variadic | O1b merges `trait.by.generated.variadic` into `trait.by.generated`, but varargs are under discussion. | Kept as its own rule for now. **Recommendation:** merge it once the vararg spelling settles; a written forwarding method already passes a vararg on. |
| Q1-dot-call | The O1b table merges `trait.by.dot-call` into `trait.by.ordinary`; the rule-by-rule table merges it into `names.method-lookup.ambiguous`. | Retired into `trait.by.ordinary`, with a Note keeping the example. **Recommendation:** let pass 32b drop the Note once `names.method-lookup.ambiguous` states the case once. |

### Bound And Row Operators

The owner's decisions (2026-09-28) are applied: bounds join with `&`, rows
join with `+`, and the old spellings are `old-row-separator` and
`old-bound-operator` ([Multiple Bounds](../spec/02-grammar.md#multiple-bounds),
[Requirement Clauses](../spec/02-grammar.md#requirement-clauses),
[Row Operators](../spec/02-grammar.md#row-operators),
[Least Row Solutions](../spec/11-requirements-and-suspension.md#least-row-solutions)).

**Questions from applying them.** Each needs an owner answer; the spec
states the current behavior.

| Question | Applied now | Recommendation |
| --- | --- | --- |
| Does `$ A + B` inside `[...]` or a parameter list need precedence rules? | A row ends at the first `,`, `)`, or `]`; nested function types keep the innermost-owner rule | None needed. No ambiguity was found in type arguments, parameters, `$.Context[...]`, or closure headers. |
| Codes for other old row spellings | `$(A + B)`, `$(A)`, a `-` between keys, and the pre-2026-09-27 `Job[A + B]` and `$.Context[A + B]` are `syntax-error` | Keep `syntax-error`. Only the two decided codes carry fix-its. |
| `$.Context[$ A + B]` keeps its inner `$`, while one key is `$.Context[A]` | Kept: the context type takes a key or a row type argument | Keep it. It matches row type arguments such as `Job[$ A + B]`. |

### Mutable Host Providers

The owner's decisions (2026-09-27 and 2026-09-28) are applied: a
requirement trait with a `mut self` method is a mutable requirement trait
([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)),
`Console.write_line!` takes `mut self`, and `println` is an ordinary std
function that drives `write_line!` with `block_on`
([`module.console.println-write`](../spec/10-modules.md#r-module.console.println-write)
through
[`module.console.println-script`](../spec/10-modules.md#r-module.console.println-script)).
Console calls stay out of replay recording.

**Still open (raised 2026-09-28).** Nothing here is decided:

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

**Decided.** Owner decisions M1-M30 are applied in
[Typed Derivation](../spec/14-annotations.md#typed-derivation), and M30
confirms the readings of the M26 apply pass. Error derivation is the
separate `@error` intrinsic, applied in
[Error Derivation](../spec/14-annotations.md#error-derivation) and listed
below.

**Waiting on other areas.** The spec lists these as
[undecided parts](../spec/14-annotations.md#undecided-parts); each waits
for the owner, and [Typed Derivation](TYPED_DERIVATION.md#remaining-open)
gives their background:

| Question | What is undecided |
| --- | --- |
| Non-escaping handles (M18 R5) | Whether the parked NonEscapable design (TQ-24 to TQ-26) makes handles non-escaping. |
| `Clone`'s module (M24) | Which standard module declares `Clone`; chosen with the standard library ([STDLIB](STDLIB.md#clone)). |
| Derived-function cache (M24) | The cache's API and module; chosen with the standard library ([STDLIB](STDLIB.md#derived-function-cache)). |
| Function targets | Deriving for functions, as tool adapters need ([FN_TYPE](FN_TYPE.md) questions 9 and 10). A decorator before a function attaches a plain value that `shape_of(f).metadata[M]()` reads ([Prefix Decorators](../spec/14-annotations.md#prefix-decorators)). |

M30 deferred template constants, typed shared constants, and composing
templates until a real template needs them; they are not in the spec.

**Member-typed facts (future option, not decided).** Testing AT-with
(batch 20, 2026-09-30) chose option B: derived `Arbitrary` requires every
member to satisfy `Arbitrary & Inspectable`, and a type that fails is not
derivable ([Testing](TESTING.md#owner-decisions)). The owner said "let's
first go with B", so option D stays open for later.

| Option | What it adds | What it would change |
| --- | --- | --- |
| D, member-typed facts | A fact type generic in its member's type, such as `MemberFact[F]`, checked against the member at compile time | A tuned member would need no `Arbitrary`, and a mismatched generator would be a compile-time error, not a panic |

Templates have no per-member bound that a fact discharges today
([`annot.walker.obligation`](../spec/14-annotations.md#r-annot.walker.obligation)),
and M30 gave facts no compile-time check hook, so D needs both.

**Secret values (removed for now).** `Secret[T]` and `Redact` were removed
from the standard-library design as too early
([STDLIB decision 12](STDLIB.md#owner-decisions), 2026-09-26). Revisit them
together with typed derivation. Options already discussed: whether standard
capability traits may take `Secret[T]` parameters so the host receives the
real value without an `expose()` in hd code; whether exported functions may
take `Secret[T]` inputs; and that a secret never encodes or appears in
outputs.

### Provider Scope Overlap

**Owner direction (batch 13, Q4, 2026-09-30).**
[`req.with.nearest.forced`](../spec/11-requirements-and-suspension.md#r-req.with.nearest.forced)
lets a callee's provider serve a callback's lookup of a key that the caller
fixed in `R`. A research pass compared lexical row keys, as Effekt's
tunneling does, with making that overlap an error, on hd's own requirement
examples; then the owner was asked.

**Batch 14 (owner decision, 2026-09-30).** Applied in
[Lexical And Dynamic Providers](../spec/11-requirements-and-suspension.md#lexical-and-dynamic-providers)
and the rules it links. PS1: row keys stay dynamically scoped. PS2:
lexical scoping is explicit, by capturing `$.use(K)` in the closure. PS3:
no closure captures a `$.with`-bound key implicitly; its row resolves at
each call.

**Batch 15 (owner decision, 2026-09-30).** Applied in
[`req.with.collision.closure`](../spec/11-requirements-and-suspension.md#r-req.with.collision.closure)
and
[`req.with.collision.closure.outer`](../spec/11-requirements-and-suspension.md#r-req.with.collision.closure.outer).
PS3a: inside a closure,
[`req.with.collision.compared`](../spec/11-requirements-and-suspension.md#r-req.with.collision.compared)
counts only keys that a lookup in the closure body can select. Those are
the closure's declared or inferred row and the `$.with` blocks inside the
closure. A `$.with` block outside the closure does not count: since batch
14 removed implicit capture, no lookup in the closure reaches it.

### Serializable Closures And Incremental Computation

**Problem.** Closures have unspecified identity
([Identity](../spec/05-expressions.md#identity)) and no stable code
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
([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only)).

**Unblocks.** Persisted callbacks, safe incremental caches, distributed work,
and bounded graph lifetimes.

**Decided 2026-09-27, not yet applied: option 1 now.** A serializable
closure's code identity is a content hash. Its captures must be
boundary-safe values (Durable Replay decision 11, in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules), replaces the `Durable` bound), and capturing a provider or mutable state is
rejected. The design still needs a record: the hash input, how a closure
opts in, and graph lifetimes.

### Observability Hooks

**Problem.** There is no task-local carrier for trace context and no
specified point where suspension/provider activity can be instrumented without
rewriting user code.

**Decided.** Observability and replay use separate hooks, and both derive
their IDs from the execution ID and the event index
(Durable Replay decision 14, in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)).

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
([Method References](../spec/07-functions.md#method-references), MR1). Runtime type tests beyond exact-type recovery from
`Inspectable` values stay unsupported
([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)).

**Direction.** Keep weakening with variance deferred, and design it only
with a motivating requirement; it must preserve representation.
Negative implementations and additional pack operations are likewise confirmed
future work rather than implicit extensions.

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

### Closure Shorthand

**Deferred (Pipe Operator PL10, 2026-09-29).** Closures stay
`fn(v): v * 2`; there is no `_` lambda shorthand, and no `f(_, a)` capture
(PL13). The pipe owns `_` inside a step
([Pipe Expressions](../spec/05-expressions.md#pipe-expressions)), `it` is
the prelude test function, and `$0` collides with requirements and
interpolation. `fn: _ * 2` would parse but needs a "not inside a pipe
step" exception. Revisit if [the hd writing log](../audit/hd-writing-log.md)
shows demand from cheap-model agents; adding `fn: _` then breaks no code.

### Iterator Performance

**Deferred.** Chaining Study CS8 (2026-09-29) keeps the closure-backed
data `Iterator[T]` as the one public iterator type. The flat
composed-stage design C is a later option, once the compiler specializes
and inlines closures. [Iterator Performance Study](ITERATOR_PERF.md) keeps
the analysis, the stage 2 benchmarks to rerun, and C's two open questions.

## Runtime, Library, ABI, And Tooling Work

These items remain required but do not currently require new core syntax:

- weak-reference runtime representation inside the standard runtime; weak
  references and finalizers are never user-visible
  ([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only));
- the prototype's replay experiments in the
  [Wasm GC compiler plan](../src/MVP_IMPLEMENTATION_PLAN.md) predate the
  decided [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules): their
  identity is per function rather than per program, their site IDs contain
  byte offsets, and they stop at the end of a history instead of resuming;
- the mandatory default algorithm, canonical field encoding, and evolution
  rules for `std.fingerprint`, whose digests always carry an algorithm/version
  identifier;
- the final `hd.toml` schema and the concrete host binding for capabilities
  such as `Console`. Executable-main selection is drafted in
  [Packages](PACKAGES.md#26-entry-points);
- dependencies through version control hosts, with no registry:
  Dependencies decisions DEP1-DEP7
  are applied in [Package Manifest](../spec/10-modules.md#package-manifest)
  (version tags, minimal version selection, `hd.sum`, workspaces,
  pseudo-versions), with DEP8-DEP19 after them. The manifest diagnostics
  wait for the manifest schema (DEP14,
  [`module.tooling.package-schema`](../spec/10-modules.md#r-module.tooling.package-schema)),
  and the tooling work is in
  [Package Tooling](RUNTIME_AND_LIBRARY.md#package-tooling);
- a `package-cycle` conformance fixture, which waits until the manifest
  schema exists (Dependency Cycles DC12,
  [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package));
- the Wasm component ABI, exact export registration API, adapter wire format,
  and runtime-profile panic status codes (histories record a panic by its
  diagnostic name, as [Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)
  state);
- stateful property testing in `std.testing`, which waits for the event
  log. The property-test API is decided and applied
  ([Testing PT1-PT9](TESTING.md#owner-decisions),
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
