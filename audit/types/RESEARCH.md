# Type-Checking Rules: Research

How Rust, MoonBit, Swift, and Kotlin specify trait behavior, compared with hd
at commit 158a430. Sources: Rust Reference, RFC 1023, RFC 2451, rustc-dev-guide
(traits/resolution, solve/candidate-preference); MoonBit docs
(moonbit-docs `next/language/{packages,methods,derive}.md`, September 2026);
Swift Evolution SE-0143, SE-0185, SE-0266, SE-0302, SE-0309, SE-0335, SE-0364;
Kotlin docs (interfaces, extensions, delegation, data classes) and spec.
Items marked (unverified) were not confirmed in official text.
The sections on coherence and on method resolution were removed: the
decisions they informed (TQ-1 to TQ-4, the member-lookup decisions) are
applied and recorded in the spec's Revision Notes.

## 1. Impl Selection

| | Rule |
| --- | --- |
| Rust | Candidates: impls (head unify), where-clauses, builtins; winnow by nested obligations; preference builtin trivial > non-global where-bounds > alias bounds > impls. Where-bounds also shadow impls for normalization. Unconstrained impl params rejected (E0207). |
| Swift | Witness = most specialized implementation whose constraints the conformance implies. |
| MoonBit | Unique per pair by placement; no documented solver. |
| hd today | "At most one implementation" plus overlap; no stated candidate order, determinism, or termination (TY-04, TY-24). |

## 2. Supertraits

| | Rule |
| --- | --- |
| Rust | Supertraits are bounds on `Self`; implementing the subtrait requires them (E0277); supertrait items usable under the subtrait bound (implied bounds). |
| Swift | Inherited requirements must be met; with conditional conformance the inherited conformance must be stated explicitly. |
| MoonBit | `trait Sub: A + B`; supertrait methods on a type parameter must be called qualified (`Position::pos(x)`). |
| Kotlin | Interfaces extend interfaces. |
| hd today | `missing-supertrait-implementation` for concrete impls; generic-impl obligations and implied bounds unstated (TY-07, TY-08). |

## 3. Default Methods And Conflicts

| | Rule |
| --- | --- |
| Rust | Same-name methods from two traits: E0034 ambiguity; fully qualified syntax. Defaults are generic bodies. |
| Kotlin | Inheriting two implementations of one member forces an override; `super<A>.foo()` selects. |
| Swift | Extension-only methods are statically dispatched: through `any P` or `T: P` the extension version runs even if the type has its own. Only requirements are customization points. |
| MoonBit | Defaults written `impl Trait with m(...)`; a regular method beats an attached trait method. Implicit dot-callability of trait methods is deprecated because a new upstream default method can make an existing dot call ambiguous. |
| hd today | Conflicts are ambiguous at the dot call; inherent method or `Trait::m` resolves. Default-body dispatch, supertrait defaults, and name reuse unstated (TY-13). |

## 4. Dynamic Safety

| | Rule |
| --- | --- |
| Rust | dyn-compatible: supertraits dyn-compatible, no `Sized` supertrait, no associated consts/GATs; dispatchable methods have no type parameters, `Self` only as receiver, no `async fn`/`impl Trait` return; `where Self: Sized` excludes a method from dyn. |
| Swift | Any protocol usable as `any P` (SE-0309); members referencing `Self` in non-covariant positions are unavailable on the existential. |
| MoonBit | `&Trait` objects; `Self` must be the first parameter and occur once. |
| Kotlin | Every interface is a type. |
| hd today | No associated types/functions, `Self` only as receiver, method generics bounded by `AnyRef`. Row/pack/`reified`/suspending methods unstated; requirement keys not required to be safe (TY-14, TY-15). |

hd's `AnyRef` rule is more permissive than Rust (generic methods allowed
when they share one representation). Rust's per-method exclusion
(`where Self: Sized`) is an option hd lacks; nothing here recommends adding it.

## 5. Derivation

| | Rule |
| --- | --- |
| Rust | Built-in derives Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd. Bound `T: Trait` on every type parameter (the "perfect derive" problem). Derive plus manual impl conflicts (E0119). Enum ordering by discriminant. |
| MoonBit | Eq, Compare, Debug, Default, Hash, Arbitrary, Shrink, FromJson, ToJson (Show status unverified). "All fields used must implement T." Enum cases ordered by declaration. Derived methods are not dot-callable without `extend`. |
| Swift | Equatable/Hashable synthesized when declared in the type or a same-file extension and all stored properties conform (SE-0185); user-written members win. Comparable for enums without raw type (SE-0266). |
| Kotlin | Data classes generate equals/hashCode/toString/copy/componentN from primary-constructor properties; explicit equals/hashCode/toString suppress generation. |
| hd today | Eq, PartialOrd, Ord, Hash on data, enums, and newtypes (EQ-1, TQ-11); bound per type parameter occurring in a compared field (between Rust and per-field); no field exclusion; law partners derived in one list (TQ-12). GADT existentials, field obligations, and placement unstated (TY-17, TY-18). |

Locality note: Swift and Kotlin forbid mixing generated and hand-written halves
only partly; Kotlin's suppression (a hand-written `equals` suppresses generation)
is implicit. hd's explicit `@derive` list is closer to Rust.

## 6. Automatic Traits

| | Rule |
| --- | --- |
| Rust | Auto traits (Send, Sync, Unpin, ...) implemented structurally: aggregates if all fields, closures if all captures; explicit generic impls replace the automatic one; negative impls std-only. |
| Swift | Sendable inferred for non-public (or frozen public) structs/enums whose stored members are Sendable; public non-frozen types never inferred "for API resilience"; no inferred conditional conformances. |
| MoonBit | Empty traits implemented automatically (flagged). |
| hd today | `Any` universal, `AnyRef` sealed and closed; `Inspectable` and `NonEscapable` are proposed. |

Swift's resilience argument applies to hd's public-signature rule: a
structurally inferred property of a public type changes when a private field
changes. That argues for declared-and-checked propagation (TQ-24).
