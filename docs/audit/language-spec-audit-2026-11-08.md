# Language specification correctness and consistency audit

Status: audit, 2026-11-08; findings and candidate resolutions are not accepted behavior. This report changes no decision, specification, fixture, or compiler rule.

GitHub tracking ticket: [#1454](https://github.com/HerringtonDarkholme/hd-lang/issues/1454).

Reviewed revision: `a8de22f95b367c61afd717e3152a370a594c9946`. The checkout was fast-forwarded from `108c40af4` to the fetched `origin/main` before checks, following repository instructions. The fetched commits did not change the language chapters.

The review covers all 14 [language chapters](../../spec/README.md#contents) and the normative README definitions: 4,969 language rules. Stdlib and CLI were consulted at their language boundaries; this is not a complete stdlib or CLI audit. The report follows [STYLE](../../spec/STYLE.md) and the repository’s [owner-decision rules](../../.agents/skills/shared-rules.md#who-decides).

**Result:** 24 findings: one P1, eighteen P2, and five P3. A01 demonstrates accepted escape of a local reference through generic recovery. The other findings identify contradictions, missing contracts, observable implementation freedom, incorrect examples, and validation gaps. They are not 24 demonstrated compiler bugs.

## Method and limits

Four reviewers read every language chapter, then cross-checked chapter boundaries. The review examined typing versus evaluation, references versus generic origin propagation, dynamic dispatch versus value copying, grammar versus feature rules, and diagnostics versus examples. Candidate issues were checked for narrower rules and existing exceptions before inclusion.

The parent ran 23 focused source probes through parsing and checking. Seven were also executed: raw byte indexing, generic bang calls, reference precedence, interpolation builder identity, borrowed downcast escape, mut-self dispatch, and delegated forwarding. Rejected controls were intentional evidence, not coding mistakes. The probe files and raw logs are in `/tmp/hd-spec-audit-2026-11-08/` for this session; durable reproductions and results are recorded below.

The specification remains the authority. Compiler acceptance does not settle contradictory rules. No new conformance suite run is claimed: `coverage --compiler` derives its results from the index and known-failure list. This is a structured semantic review, not a formal soundness proof or exhaustive enumeration of feature combinations.

Historical reconciliation was targeted: open issue titles, the open-question umbrella #833, the downcast decision #996, and the deferred function study #1436. It was not an audit of every historical owner comment. No new language mechanism or alternative design has been accepted here.

## Findings

| ID | Priority | Finding | Evidence class |
| --- | --- | --- | --- |
| [A01](#a01) | P1 | Borrowed origins disappear through generic downcast recovery | Soundness; reproduced checker acceptance |
| [A02](#a02) | P2 | Structure handle declarations omit their construction safeguards | Specification safety gap; implementation protected |
| [A03](#a03) | P2 | Interpolation builder reuse changes observable output | Observable implementation freedom; reproduced |
| [A04](#a04) | P2 | Dynamic and provider write-through promises conflict with mut self copies | Cross-chapter contradiction; runtime probe |
| [A05](#a05) | P2 | Generic shared enum initializers have no selected instantiation | Missing rejection rule; focused probe |
| [A06](#a06) | P2 | Plain function values both implement and fail Fn | Direct conformance contradiction; reproduced |
| [A07](#a07) | P2 | Inspectable includes types that its implicit Copy bound excludes | Copy consistency; focused probe |
| [A08](#a08) | P2 | Lifetime trait and alias arguments are accepted and forbidden | Direct contradiction; reproduced |
| [A09](#a09) | P2 | Raw string indexing conflicts with the scalar-boundary panic rule | Observable contradiction; runtime probe |
| [A10](#a10) | P2 | The numeric operator rule gives comparisons a numeric result | Result-type contradiction |
| [A11](#a11) | P2 | Suspending function types are definitionally equal and distinct | Type-identity contradiction |
| [A12](#a12) | P2 | Every trait is dynamically safe except the traits explicitly rejected | Universal rule conflicts with named exceptions; reproduced |
| [A13](#a13) | P2 | Race polling order is assumed but absent from language rules | Missing observable scheduling contract |
| [A14](#a14) | P2 | The consolidated grammar omits three supported syntax families | Grammar completeness; parser probes |
| [A15](#a15) | P2 | The lexical bang-call rule excludes generic bang calls | Direct syntax contradiction; runtime probe |
| [A16](#a16) | P2 | Reference precedence places power on both sides of ampersand | Direct precedence contradiction; runtime probe |
| [A17](#a17) | P2 | Copy rules contradict blanket ownership and feature exclusions | Two stale universal statements |
| [A18](#a18) | P2 | Delegation does not behave like its specified forwarding body | Generated-code equivalence; reproduced |
| [A19](#a19) | P3 | The folder-cycle diagnostic requires an impossible fix-it | Diagnostic-contract completeness |
| [A20](#a20) | P3 | The positive identity example violates exclusive borrowing | Incorrect example; reproduced |
| [A21](#a21) | P3 | The structure example passes and returns the wrong reference types | Incorrect example; reproduced |
| [A22](#a22) | P3 | Generic examples still claim i32 for unsuffixed positive literals | Incorrect consequence and example; reproduced |
| [A23](#a23) | P3 | A numeric runtime panic is tagged as a compile-time error | Adjacent stdlib diagnostic inconsistency; tool warning |
| [A24](#a24) | P2 | The example inventory ignores 230 hd code fences | Validation gap; measured from source |

<a id="a01"></a>

## A01 Borrowed origins disappear through generic downcast recovery

Priority: P1. Soundness; reproduced checker acceptance.

Rules: [`trait.inspectable.reference-type`](../../spec/lang/09-traits.md#r-trait.inspectable.reference-type), [`trait.downcast.shared-local-reference`](../../spec/lang/09-traits.md#r-trait.downcast.shared-local-reference), [`trait.downcast.copy-through-reference`](../../spec/lang/09-traits.md#r-trait.downcast.copy-through-reference), [`ref.generic.call`](../../spec/lang/13-values-and-references.md#r-ref.generic.call), [`ref.dep.return`](../../spec/lang/13-values-and-references.md#r-ref.dep.return).

Local reference types are inspectable, and recovery may copy their values out of an erased local reference. A generic result position added by `T` depends on nothing when no input parameter contains `T`. Together these rules lose the origin hidden behind `&dyn Inspectable`.

The following function returns a reference to its own `inner`, although its declared result may depend only on `outer`. Both parsing and checking succeed. Adding a caller prints `inner` after `escape` returns. Returning `&inner` directly instead is rejected with `local-reference-escapes`: “`inner` is a local of `escape`; the result depends only on `outer`”. The run demonstrates the escape, not a witnessed memory corruption or crash.

```text
use std.inspect.Inspectable

fn recover[T < Inspectable](value: &dyn Inspectable) -> T?:
    match value.downcast_ref::[T]():
        .Some(found) => .Some(*found)
        .None => .None

fn escape(outer: &string) -> &string:
    inner := "inner"
    r := &inner
    let erased: &dyn Inspectable = &r
    match recover::[&string](erased):
        .Some(found) => found
        .None => outer
```

The owner chose local dependent downcasts in [#996](https://github.com/HerringtonDarkholme/hd-lang/issues/996). This finding concerns nested origins lost during recovery; it does not reopen that API choice.

**Owner question.** Must recovery preserve every nested erased origin, or should recovery targets holding local references be rejected until those origins can be represented?

**Done check.** Add an invalid typing fixture for this escape, plus valid local and counted recovery controls. Exercise reference-valued targets and lifetime structs through generic helpers; preserve the accepted local API.

<a id="a02"></a>

## A02 Structure handle declarations omit their construction safeguards

Priority: P2. Specification safety gap; implementation protected.

Rules: [`annot.structure.decl`](../../spec/lang/14-annotations.md#r-annot.structure.decl), [`annot.handle.get-borrowed`](../../spec/lang/14-annotations.md#r-annot.handle.get-borrowed), [`annot.handle.get.variant`](../../spec/lang/14-annotations.md#r-annot.handle.get.variant).

The published `FieldRecord[S,F]` contains only public `info: Member`. The other handle records are similarly presented as ordinary structs with public fields. No normative opacity or construction restriction prevents manufacturing a handle with an unrelated `S` or `F`, but `get` promises a reference into a member of that type. The only specified failure concerns the wrong enum variant.

```text
use std.structure.{FieldRecord, Member}

struct Victim:
    value: i32

fn forge(info: Member, target: &Victim) -> &string:
    h := box(FieldRecord::[Victim, string] { info: info })
    h.get(target)
```

The implementation rejects this construction with `missing-required-field` and `private-member`: its record has private `hd_get`, `hd_has_default`, `hd_default`, and `hd_witness` fields. See [the actual record](../../lib/std/structure.hd#L124). This is an omitted specification safeguard, not a demonstrated compiler exploit.

**Owner question.** Are handle records opaque outside `std.structure`, or may users construct them with defined validation and failure behavior?

**Done check.** State the construction restriction or validation contract for all four record types, and cover attempts to forge `S`, `F`, variants, and member keys.

<a id="a03"></a>

## A03 Interpolation builder reuse changes observable output

Priority: P2. Observable implementation freedom; reproduced.

Rules: [`expr.interp.builder.push`](../../spec/lang/05-expressions.md#r-expr.interp.builder.push), [`expr.interp.write.measured`](../../spec/lang/05-expressions.md#r-expr.interp.write.measured), [`trait.display.write`](../../spec/lang/09-traits.md#r-trait.display.write).

Receiver reuse is permitted when interpolation values are names or field paths. A custom `Display.write` can nevertheless compare its `out` with a counted builder captured in the displayed value. No normative rule requires output to be independent of builder identity.

The following program checks and prints `same` followed by `other`. The direct push and explicitly materialized interpolation observe different builders. This is permitted by the present reuse rule, so it is an observable semantic choice requiring review, rather than proof that the implementation violates an unambiguous rule.

```text
use std.text.StringBuilder

struct Probe:
    target: *mut StringBuilder

impl Display for Probe:
    fn to_string(&self) -> string:
        "unused"
    fn write(&self, out: &mut StringBuilder) -> void:
        if out is self.target:
            out.push("same")
        else:
            out.push("other")

fn demo() -> string:
    b := StringBuilder::new()
    p := Probe { target: b }
    b.push("$p")
    b.build()

fn materialized() -> string:
    b := StringBuilder::new()
    p := Probe { target: b }
    text := "$p"
    b.push(text)
    b.build()

pub fn main() -> void $ Console:
    println(demo())
    println(materialized())
```

**Owner question.** Must reuse preserve the materialized expression’s behavior, or is builder-dependent output explicitly permitted?

**Done check.** State the observable contract and cover a custom writer that observes builder identity or existing content.

<a id="a04"></a>

## A04 Dynamic and provider write-through promises conflict with mut self copies

Priority: P2. Cross-chapter contradiction; runtime probe.

Rules: [`fn.method.mut-self-struct`](../../spec/lang/07-functions.md#r-fn.method.mut-self-struct), [`fn.method.equivalence`](../../spec/lang/07-functions.md#r-fn.method.equivalence), [`trait.dyn.ref.member`](../../spec/lang/09-traits.md#r-trait.dyn.ref.member), [`trait.dyn.ref.write-through`](../../spec/lang/09-traits.md#r-trait.dyn.ref.write-through), [`req.ref.share.write-through`](../../spec/lang/11-requirements-and-suspension.md#r-req.ref.share.write-through).

`mut self` changes a method’s own struct/list/map copy. Dynamic trait references explicitly expose those methods, yet say a receiver-changing method changes the referenced object. Providers repeat the write-through promise without restricting it to reference receivers.

A boxed `Counter { n: +0 }` implementing `bump(mut self)` as `self.n += 1` remains `0` after a concrete call, a `*mut dyn Bump` call, and a `$.use(Bump)` call. The probe prints three zeroes, following the copy rule.

**Owner question.** Does dynamic/provider dispatch preserve copying for `mut self`, or is write-through an explicit exception?

**Done check.** Reconcile the three chapters and contrast `mut self` with `&mut self` through concrete, local dynamic, counted dynamic, and provider calls.

<a id="a05"></a>

## A05 Generic shared enum initializers have no selected instantiation

Priority: P2. Missing rejection rule; focused probe.

Rules: [`names.type-param.local`](../../spec/lang/03-names-and-scopes.md#r-names.type-param.local), [`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init), [`data.shared.generic-copy`](../../spec/lang/08-data-and-enums.md#r-data.shared.generic-copy), [`data.shared.type-parameter`](../../spec/lang/08-data-and-enums.md#r-data.shared.type-parameter).

Shared data is evaluated once per declaration at module initialization. A shared parameter’s type cannot mention the enum’s type parameter, but no corresponding restriction covers its initializer or default expression. `enum Tagged[T < Label](name: string)` with `A -> Tagged(T::label())` uses a fixed `string` field yet needs a particular `T` to run its requirement-free associated function.

The parser accepts the probe; the checker rejects `T` with `unknown-name`. General declaration scoping and the shared-initialization rules do not explain that exclusion. Reading a const generic directly is not the counterexample: it is independently prohibited.

**Owner question.** Are enum generic parameters out of scope in these expressions, or should generic-dependent initialization have a specific rejection rule?

**Done check.** Cover constructor and default expressions that mention a type parameter through an associated call.

<a id="a06"></a>

## A06 Plain function values both implement and fail Fn

Priority: P2. Direct conformance contradiction; reproduced.

Rules: [`fn.trait.function-values`](../../spec/lang/07-functions.md#r-fn.trait.function-values), [`fn.trait.read-only.values`](../../spec/lang/07-functions.md#r-fn.trait.read-only.values), [`trait.sealed.fn`](../../spec/lang/09-traits.md#r-trait.sealed.fn).

The universal rules give every plain function value/type an exact `Fn` implementation. The read-only rules give ordinary concrete `fn(...)` values only `FnMut`, because their captures may change. Passing a parameter `test: fn() -> bool` to `F < Fn() -> bool` therefore has incompatible required outcomes.

The checker rejects the focused probe with `unsatisfied-trait-bound`: “a fn() -> bool value implements only `FnMut`, since it may change its captures, but this parameter's bound is `Fn`; pass a function by name or write the closure here”. The existing `fn-value-only-fn-mut.hd` fixture also expects rejection.

**Owner question.** Should the universal implementation statements be qualified by the existing read-only rules?

**Done check.** Make the implementation table and static-bound rules agree for names, closure literals, bound methods, and values read from parameters/fields. The deferred function-representation study [#1436](https://github.com/HerringtonDarkholme/hd-lang/issues/1436) is not an accepted replacement design.

<a id="a07"></a>

## A07 Inspectable includes types that its implicit Copy bound excludes

Priority: P2. Copy consistency; focused probe.

Rules: [`ref.copy.trait.default`](../../spec/lang/13-values-and-references.md#r-ref.copy.trait.default), [`ref.copy.trait.relaxed`](../../spec/lang/13-values-and-references.md#r-ref.copy.trait.relaxed), [`trait.rtti.reference-surface`](../../spec/lang/09-traits.md#r-trait.rtti.reference-surface), [`trait.inspectable.declared`](../../spec/lang/09-traits.md#r-trait.inspectable.declared), [`trait.inspectable.fields`](../../spec/lang/09-traits.md#r-trait.inspectable.fields).

The published `trait Inspectable:` has no `?Copy` relaxation. Ordinary traits therefore require `Self: Copy`. Its supplied implementation set nevertheless includes every module-level struct/enum independently of its fields, including `impl !Copy` types and types holding `&mut`.

A probe calling `runtime_type()` through `&Photo`, where `Photo` declares `impl !Copy`, checks successfully. This follows the supplied implementation set but needs an explicit exception or a relaxed trait declaration. `AnyVal`’s published declaration and all-struct implementation promise merit the same reconciliation.

**Owner question.** Should these universal marker traits explicitly relax `Copy`, or have a narrower implementation set?

**Done check.** Align each supplied marker’s declaration, supertraits, implementation table, and non-copyable fixtures.

<a id="a08"></a>

## A08 Lifetime trait and alias arguments are accepted and forbidden

Priority: P2. Direct contradiction; reproduced.

Rules: [`ref.lifetime.struct.argument.other`](../../spec/lang/13-values-and-references.md#r-ref.lifetime.struct.argument.other), [`ref.lifetime.trait`](../../spec/lang/13-values-and-references.md#r-ref.lifetime.trait), [`ref.lifetime.alias`](../../spec/lang/13-values-and-references.md#r-ref.lifetime.alias), [`trait.decl.lifetime`](../../spec/lang/09-traits.md#r-trait.decl.lifetime).

`ref.lifetime.struct.argument.other` rejects every lifetime argument of a trait or type alias with `references-not-yet-checked`. The Lifetime Traits and Lifetime Aliases sections explicitly allow those arguments and explain their behavior.

The combined `Source[T,'src]` and `Borrowed[T,'src]` probe parses and checks. Accepted conformance fixtures already cover lifetime traits and aliases.

**Owner question.** Should the blanket staged restriction be retired in favor of the specified supported cases?

**Done check.** Distinguish supported lifetime traits/aliases from the separately unsupported alias transformations, without changing an existing rule ID’s meaning silently.

<a id="a09"></a>

## A09 Raw string indexing conflicts with the scalar-boundary panic rule

Priority: P2. Observable contradiction; runtime probe.

Rules: [`types.string.split-scalar`](../../spec/lang/04-type-system.md#r-types.string.split-scalar), [`expr.index.string.byte`](../../spec/lang/05-expressions.md#r-expr.index.string.byte), [`expr.index.string.range`](../../spec/lang/05-expressions.md#r-expr.index.string.range).

Every operation given a non-scalar-boundary byte offset is required to panic. Raw string indexing instead reads any in-range byte. `"é"[1]` satisfies the indexing bounds but points inside the scalar’s encoding.

The probe checks and prints `169` (`0xA9`), following raw-byte indexing.

**Owner question.** Is the scalar-boundary requirement limited to operations that produce string ranges?

**Done check.** Explicitly cover raw continuation-byte indexing and rejection of slicing through that same byte offset.

<a id="a10"></a>

## A10 The numeric operator rule gives comparisons a numeric result

Priority: P2. Result-type contradiction.

Rules: [`types.num.binary.literal-join`](../../spec/lang/04-type-system.md#r-types.num.binary.literal-join), [`types.num.binary.same-type`](../../spec/lang/04-type-system.md#r-types.num.binary.same-type), [`expr.op.comparison`](../../spec/lang/05-expressions.md#r-expr.op.comparison).

The section covers binary arithmetic and comparison expressions, then says same-type operands produce that numeric type. Comparison semantics instead produce `bool`. With `a: f32` and `b: f32`, `a < b` cannot have both types.

A focused function returning that comparison as `bool` checks successfully.

**Owner question.** Should the numeric-result sentence apply only to arithmetic operators?

**Done check.** Separate operand unification from result typing, including comparisons with literals on either side.

<a id="a11"></a>

## A11 Suspending function types are definitionally equal and distinct

Priority: P2. Type-identity contradiction.

Rules: [`req.suspend.type.equivalent`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.equivalent), [`req.suspend.type.conversion`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.conversion), [`req.suspend.type.bang`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.bang), [`fn.type.suspend.ctor`](../../spec/lang/07-functions.md#r-fn.type.suspend.ctor), [`types.fn.same`](../../spec/lang/04-type-system.md#r-types.fn.same).

The first rule declares `fn!(A)->T $R` definitionally equivalent to `fn(A)->mut Suspend[T] $R`. The following rules allow only one-way conversion and give only the first the bang-call operation. Function identity requires the same constructor; the lowering explicitly changes `SuspendFn` to `Fn`.

**Owner question.** Are these distinct source types related by lowering, or actual identical types?

**Done check.** Agree on identity, conversion in both directions, generic matching, and permitted call operations.

<a id="a12"></a>

## A12 Every trait is dynamically safe except the traits explicitly rejected

Priority: P2. Universal rule conflicts with named exceptions; reproduced.

Rules: [`types.trait.dyn.any-trait`](../../spec/lang/04-type-system.md#r-types.trait.dyn.any-trait), [`ref.type.dyn.no-fn`](../../spec/lang/13-values-and-references.md#r-ref.type.dyn.no-fn), [`ref.type.dyn.no-fn-mut`](../../spec/lang/13-values-and-references.md#r-ref.type.dyn.no-fn-mut).

The type chapter says every trait may be `dyn`, with no per-trait safety check. The reference chapter explicitly rejects `dyn Fn` and `dyn FnMut` with `trait-not-dynamically-safe`.

The `&dyn Fn[(i32,), i32]` probe receives that diagnostic.

**Owner question.** Should the universal rule explicitly exclude the static function traits?

**Done check.** Make the permitted trait-value type set identical in the type, function, and reference chapters.

<a id="a13"></a>

## A13 Race polling order is assumed but absent from language rules

Priority: P2. Missing observable scheduling contract.

Rules: [`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished), [`req.combinator.race-child-cancelled`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-child-cancelled).

Argument-order initial and subsequent polling is specified only for `join!`. A `race!` child that completed earlier on the same poll wins over a cancelled sibling, making the order observable. No corresponding language-tier race ordering rule was found.

Meanwhile [std-task.timeout.tie](../../spec/std/task.md#r-std-task.timeout.tie) explicitly relies on `race!` polling its children in order. Existing runtime cases, including `race-wake-during-sibling-println.hd`, assume an order too. Determinism within one compiled program does not select one order across conforming implementations.

**Owner question.** Is argument-order polling guaranteed for `race!`, including after wakes, or intentionally unspecified?

**Done check.** Cover two immediately-ready children, completion versus cancellation on one poll, and a child waking during a sibling’s poll.

<a id="a14"></a>

## A14 The consolidated grammar omits three supported syntax families

Priority: P2. Grammar completeness; parser probes.

Rules: [`grammar.scope.consolidated`](../../spec/lang/02-grammar.md#r-grammar.scope.consolidated), [`ref.expr.deref.target`](../../spec/lang/13-values-and-references.md#r-ref.expr.deref.target), [`fn.trait.bound-sugar`](../../spec/lang/07-functions.md#r-fn.trait.bound-sugar), [`lex.interp.dollar-text`](../../spec/lang/01-lexical-structure.md#r-lex.interp.dollar-text).

Three accepted forms have no derivation in the consolidated EBNF:

| Form | Missing connection |
| --- | --- |
| `*r = 1` and `*r += 1` | Assignment requires `postfix_expression`; `dereference_expression` is separate. The inline and suite alternatives repeat that restriction. |
| `F < Fn(i32) -> i32` and corresponding `FnMut` bounds | `bound_trait_type` accepts only a qualified name and optional bracketed arguments. |
| `"$"` and `"costs $5"` | Both interpreted-string segment productions omit ordinary-dollar text while `string_text` excludes `$`. |

All three focused probes parse and check with the compiler. That confirms supported implementation syntax, not an EBNF derivation. Separate feature grammars are explicitly disallowed.

**Owner question.** Should the consolidated productions be completed to express these already specified forms?

**Done check.** Add derivation-oriented checks independent of the compiler parser; preserve the existing accepted and rejected boundary cases.

<a id="a15"></a>

## A15 The lexical bang-call rule excludes generic bang calls

Priority: P2. Direct syntax contradiction; runtime probe.

Rules: [`lex.op.bang-call`](../../spec/lang/01-lexical-structure.md#r-lex.op.bang-call), [`grammar.expr.method-type-arguments.bang`](../../spec/lang/02-grammar.md#r-grammar.expr.method-type-arguments.bang).

The lexical rule requires `!` immediately followed by `(`. The grammar prescribes `identity!::[i32](1)`, where `::` follows `!`, for an explicitly instantiated bang call.

That complete program parses, checks, and prints `1`.

**Owner question.** Should the lexical statement allow the specified intervening type-argument list?

**Done check.** Cover ordinary and explicitly generic bang calls, including the rejected misplaced-bang spelling.

<a id="a16"></a>

## A16 Reference precedence places power on both sides of ampersand

Priority: P2. Direct precedence contradiction; runtime probe.

Rules: [`ref.prec.unary-ref`](../../spec/lang/13-values-and-references.md#r-ref.prec.unary-ref), [`ref.prec.power-ref`](../../spec/lang/13-values-and-references.md#r-ref.prec.power-ref).

The first rule makes `&` and `&mut` bind more tightly than every binary operator. The second says their operand includes `**`, so `&x ** 2` means `&(x ** 2)`. Both cannot describe the same precedence.

A probe dereferencing the result of `&x ** 2` checks and returns `9` for `x = 3`, agreeing with the power-specific rule and EBNF.

**Owner question.** Should the universal precedence sentence exclude exponentiation?

**Done check.** Specify and cover both reference prefixes around power and contrast the tighter dereference operator.

<a id="a17"></a>

## A17 Copy rules contradict blanket ownership and feature exclusions

Priority: P2. Two stale universal statements.

Rules: [`flow.match.no-ownership`](../../spec/lang/06-control-flow.md#r-flow.match.no-ownership), [`ref.move.pattern`](../../spec/lang/13-values-and-references.md#r-ref.move.pattern), [`trait.unsupported.list`](../../spec/lang/09-traits.md#r-trait.unsupported.list), [`ref.copy.opt-out`](../../spec/lang/13-values-and-references.md#r-ref.copy.opt-out).

Matching is said never to transfer ownership, while matching a non-copyable payload expressly moves it. The unsupported-traits list says there are no negative implementations, while `impl !Copy` is explicitly defined as a negative implementation.

**Owner question.** Should these blanket statements be qualified by the specified non-copyable behavior and `!Copy` exception?

**Done check.** Retain rejection of other negative implementations and cover payload movement, reuse after a move, and matching only copied fields.

<a id="a18"></a>

## A18 Delegation does not behave like its specified forwarding body

Priority: P2. Generated-code equivalence; reproduced.

Rules: [`trait.by.generated`](../../spec/lang/09-traits.md#r-trait.by.generated), [`fn.method.equivalence`](../../spec/lang/07-functions.md#r-fn.method.equivalence).

Every generated receiver method is said to call `Trait::m(self.E, arguments...)`, checked as if written. For `m(&mut self)`, that expression supplies `E` where an explicit `&mut E` argument is required.

The delegated `impl Bump for Outer by Inner` checks and changes the embedded counter to `1`. Writing the specified expansion instead is rejected with `type-mismatch`: “in argument: expected &mut ?, found Inner”.

**Owner question.** Should the expansion explicitly adapt the embedded receiver according to each receiver form?

**Done check.** Compare delegation with its written expansion for value, mutable-value, local-reference, and counted-reference receivers.

<a id="a19"></a>

## A19 The folder-cycle diagnostic requires an impossible fix-it

Priority: P3. Diagnostic-contract completeness.

Rules: [`module.cycle.diagnostic.fix`](../../spec/lang/10-modules.md#r-module.cycle.diagnostic.fix).

Every folder-cycle diagnostic must move some `x.hd` to `x/mod.hd` while retaining module identity. A cycle consisting of `src/a/mod.hd` using `pkg.b` and `src/b/mod.hd` using `pkg.a` already has only directory-module roots. Neither can undergo that identity-preserving move.

**Owner question.** Is this fix-it conditional on a suitable flat module file existing?

**Done check.** Include both a cycle with a useful move and an all-directory-root cycle with no such move.

<a id="a20"></a>

## A20 The positive identity example violates exclusive borrowing

Priority: P3. Incorrect example; reproduced.

Rules: [`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut), [`ref.identity.same-place`](../../spec/lang/13-values-and-references.md#r-ref.identity.same-place).

The Identity section binds `r := &mut p`, then `s := &p`, and later uses `r is s`. `r` is still live at the second borrow. The unmarked positive example therefore violates exclusivity.

The exact complete example parses but checking reports `conflicting-access`: “this uses `p` while `r` refers to it as `&mut` and is used later; use `p` after `r`'s last use, or through `r`”.

**Owner question.** Should the example use two shared references or counted aliases to demonstrate identity?

**Done check.** Type-check the complete positive example and retain separate exclusivity rejection coverage.

<a id="a21"></a>

## A21 The structure example passes and returns the wrong reference types

Priority: P3. Incorrect example; reproduced.

Rules: [`annot.handle.get-borrowed`](../../spec/lang/14-annotations.md#r-annot.handle.get-borrowed).

The Handles section’s `missing[F](...) -> Result[F, never]` returns `.Ok(h.get(self.old))`. `get` requires `&S` and returns `&F`; the example supplies `S` and promises `F`.

A complete helper with that expression receives both errors: “in argument: expected &S, found S” and “in payload: expected F, found &F”. This remains a defect even though the surrounding example elides other protocol methods.

**Owner question.** Should the copied-field example explicitly borrow the old value and copy the result?

**Done check.** Type-check an executable equivalent of the complete example.

<a id="a22"></a>

## A22 Generic examples still claim i32 for unsuffixed positive literals

Priority: P3. Incorrect consequence and example; reproduced.

Rules: [`types.generic.default.argument-wins`](../../spec/lang/04-type-system.md#r-types.generic.default.argument-wins), [`types.literal.local.default`](../../spec/lang/04-type-system.md#r-types.literal.local.default), [`types.literal.local.statement`](../../spec/lang/04-type-system.md#r-types.literal.local.statement).

The generic-default rule explicitly says `widen(3)` in the preceding example solves `T = i32`. Its initializer has no expected signed type; the positive literal defaults to `usize`, and later statements cannot change it. The later return annotation cannot repair the binding.

The extracted `small() -> i32` example fails with `type-mismatch`: “in result: expected i32, found usize”. The generic function-value explanation in chapter 07 likewise calls `count := apply(3, identity)` an `i32` instantiation.

**Owner question.** Should those examples use signed literals or describe the `usize` instantiation?

**Done check.** Keep the default-argument example valid without implying backward inference across statements.

<a id="a23"></a>

## A23 A numeric runtime panic is tagged as a compile-time error

Priority: P3. Adjacent stdlib diagnostic inconsistency; tool warning.

Rules: [`trait.num.from-i64-checked`](../../spec/lang/09-traits.md#r-trait.num.from-i64-checked).

[std-num.u64.checked](../../spec/std/num.md#r-std-num.u64.checked) says negative `to_u64` and overflowing `from_u64` panic in every build, then tags the rule `Error: integer-overflow`. The code is a runtime panic category, absent from the stable compile-time error table.

`spec audit --list` reports the unknown error code. `spec/check.sh` still passes because this audit finding is a warning.

**Owner question.** Should the rule use the runtime `Panic:` classification its prose already describes?

**Done check.** Align the tag with runtime fixtures and keep diagnostic-phase auditing consistent.

<a id="a24"></a>

## A24 The example inventory ignores 230 hd code fences

Priority: P2. Validation gap; measured from source.

The language chapters contain 230 `hd` fences and 747 `text` fences. STYLE and `examples.tsv` inventory only `text` fences; `spec/check.sh` counts only those, and `check-example-overlap.ts` scans only those. Consequently the positive Identity example in A20 is outside the fixture-overlap gate.

Even inventoried examples are checked through fixture identifier overlap, not by type-checking the exact printed source. That explains how the mismatched structure example and stale literal example survive a passing structural check. Citation coverage also establishes only that a rule is mentioned.

**Owner question.** Should both hd fence spellings be inventoried, with complete positive examples type-checked and fragments explicitly classified?

**Done check.** A deliberately invalid positive example must fail the example check. Cover both fence spellings, error examples, and fragments without turning implementation behavior into the specification oracle.

## Chapter review ledger

Counts below come from `pnpm run spec counts`. “Reviewed” means the chapter’s text, productions, and examples were read; it does not certify soundness.

| Chapter | Rules | Review focus | Findings |
| --- | ---: | --- | --- |
| 01 Lexical Structure | 243 | Encoding, layout, token boundaries, interpolation | A14–A15 |
| 02 Grammar | 295 | Every production; calls, types, assignments, patterns | A14–A15 |
| 03 Names and Scopes | 207 | Categories, declaration scope, lookup, visibility, embedding | A05 cross-check; no independent confirmed defect |
| 04 Type System | 557 | Inference, conversions, variance, opaque types, strings, markers | A07, A09–A12, A22 |
| 05 Expressions | 488 | Evaluation, places, interpolation, operators, indexing | A03, A09–A10 |
| 06 Control Flow | 304 | Iteration, matching, completion, cleanup, panic | A17 |
| 07 Functions | 324 | Parameter copies, closures, callable bounds, function lifetimes | A04, A06, A11, A14, A18, A22 |
| 08 Data Types and Enums | 170 | Construction/defaults, sharing, embedding, recursion | A05 |
| 09 Traits | 602 | Conformance, resolution, dynamic calls, RTTI, delegation | A01, A03–A04, A06–A07, A12, A17–A18 |
| 10 Modules | 626 | Module identity, initialization, cycles, boundaries, testing | A19 |
| 11 Requirements and Suspension | 391 | Rows, providers, capture, polling, cancellation | A04, A11, A13 |
| 12 Variadic Generics | 0 | No-packs statement and tuple/function references | No confirmed defect |
| 13 Values and References | 366 | Every rule; origins, lifetime stores, loans, moves, identity | A01, A07–A08, A12, A16–A17, A20 |
| 14 Annotations | 396 | Typed facts, derivation, structure handles, generated behavior | A02, A21 |

High-risk interaction checks covered borrowed values through erasure and generic returns; handle authenticity versus public construction; mutable value receivers through dynamic dispatch/providers; generic declarations versus module initialization; custom formatting versus builder reuse; and pattern matching versus ownership. Variance and cancellation received textual review, but no general soundness theorem or randomized state-space exploration was performed.

## Additional observations requiring narrower follow-up

These observations are not counted among the 24 findings and are not demonstrated safety bugs:

| Area | Observation | Why kept separate |
| --- | --- | --- |
| Place terminology | `expr.place.value-forms` calls `:=` bindings non-places; `ref.place.forms` makes every binding form a place for references. | Could be an intended address-taking distinction; its terminology needs clarification. |
| Mutable element loops | `flow.for.ref.conflict` says “any other use” of the list is forbidden, but linked `ref.elem.access.mut` permits other element reads/stores; an accepted example does both. | The narrower linked rule may already be intended as the qualification. |
| Diverging loops | `flow.loop.infinite-never` gives no-break infinite loops `never`, while later `else`-join wording can suggest a value type from an unreachable `else`. | Priority between the rules needs closer examination before calling it a type-system defect. |
| Range explanations | Chapter 05 still describes range indexing as slicing and omits `..b` from a pattern note. | Stale explanatory text; explicit syntax rules already settle the intended behavior. |
| Capturing function values | `ref.generic.static.meets` says function types always meet `'static`, while explicit captured-origin rules prohibit boxing some values of them. | Value-origin exceptions prevent this alone from demonstrating an escape. |
| Runtime identity | The definition does not explicitly account for const/lifetime arguments or every structural constructor. | The precise issue needs separation from A01 and the existing exact-type rules. |
| Counted-reference polarity | The polarity table names `&mut` but omits `*mut`. | Representation-preserving conversion restrictions may prevent the suspected unsafe conversion. |

The apparent difference between the Map iterable protocol’s `(K,V)` and direct built-in loops’ `(&K,&V)` was checked and excluded: direct collection traversal has an explicit special path.

## Validation

| Check | Result |
| --- | --- |
| `pnpm run spec counts` | Language 4,969; stdlib 3,083; CLI 774; total 8,826 |
| `pnpm run spec audit --list` | 427 warnings: 287 long sentences, 4 long paragraphs, 89 sections without examples, 45 repeated sentences, 1 unknown error code, 1 unused code; 0 bad IDs |
| `pnpm run spec coverage` | 6,232 of 8,826 rules cited, 70.6%; citation coverage is not semantic coverage |
| `pnpm run spec coverage --compiler` | Metadata inventory only; not a fresh test execution |
| `pnpm run spec phase-audit` | Passed, 3,157 diagnostic rows |
| `pnpm run spec refs --dead` | 0 failing, 70 warnings, 9 historical allowances |
| `bash spec/check.sh` | Passed, including 431 pointer-loan matrix cases and example overlap |
| `cargo build -j 2 -p hd_cli` | Passed; one heavy command at a time, test threads limited to 2 |
| Focused source probes | All 23 parse; check outcomes below |
| `pnpm run website:build` | Passed, 80 pages; grammar 285 rules / 286 definitions |

The source checks printed a cache-write warning because the sandbox does not permit the normal user cache. Each command continued without that cache; reported exit statuses and diagnostics are unchanged.

### Parse and check log

Every `text` block in this report is a complete source file from a parsed probe. Parsing alone verifies syntax; the table separately records checking and execution.

| Probe | Parse | Check | Runtime or diagnostic |
| --- | --- | --- | --- |
| `downcast-reference-origin` | Pass | Accept | Returns a reference to its own local |
| `downcast-reference-origin-run` | Pass | Accept | Prints `inner` after return |
| `direct-reference-origin` | Pass | Reject | `local-reference-escapes` control |
| `forged-field-handle` | Pass | Reject | Private implementation fields protect construction |
| `interpolation-builder-identity` | Pass | Accept | Prints `same`, then `other` |
| `mut-self-dispatch` | Pass | Accept | Prints `0`, `0`, `0` |
| `generic-enum-shared` | Pass | Reject | `unknown-name` for `T` |
| `fn-conformance` | Pass | Reject | `unsatisfied-trait-bound` for `Fn` |
| `inspectable-noncopy` | Pass | Accept | Non-copyable referent has runtime type method |
| `lifetime-trait-alias` | Pass | Accept | Supported contrary to blanket restriction |
| `byte-index` | Pass | Accept | Prints `169` |
| `numeric-comparison` | Pass | Accept | `f32 < f32` returns `bool` |
| `dyn-fn` | Pass | Reject | `trait-not-dynamically-safe` |
| `deref-assignment` | Pass | Accept | Both dereference assignment forms |
| `fn-bound` | Pass | Accept | Callable bound sugar |
| `literal-dollar` | Pass | Accept | Both ordinary-dollar forms |
| `bang-generic` | Pass | Accept | Prints `1` |
| `ref-power` | Pass | Accept | Prints `9` |
| `forwarded-ref` | Pass | Accept | Delegated mutation prints `1` |
| `forwarding-expansion` | Pass | Reject | `type-mismatch` in specified expansion |
| `identity-example` | Pass | Reject | `conflicting-access` |
| `handle-example-types` | Pass | Reject | Both argument and payload `type-mismatch` |
| `literal-default-example` | Pass | Reject | Expected `i32`, found `usize` |

## Resolution constraints

Owner questions above identify competing contracts; they are not approved changes. Any chosen correction belongs in a subsequent `spec-update` pass, with rule-ID retirement where meaning changes and focused conformance cases. Preserve recorded decisions, especially local dependent recovery. No compiler, specification, or fixture was edited as part of this audit.
