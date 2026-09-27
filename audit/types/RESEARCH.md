# Type-Checking Rules: Research

How Rust, MoonBit, Swift, and Kotlin specify trait behavior, compared with hd
at commit 158a430. Sources: Rust Reference, RFC 1023, RFC 2451, rustc-dev-guide
(traits/resolution, solve/candidate-preference); MoonBit docs
(moonbit-docs `next/language/{packages,methods,derive}.md`, September 2026);
Swift Evolution SE-0143, SE-0185, SE-0266, SE-0302, SE-0309, SE-0335, SE-0364;
Kotlin docs (interfaces, extensions, delegation, data classes) and spec.
Items marked (unverified) were not confirmed in official text.

## 1. Coherence, Orphan Rule, Overlap

| | Rule |
| --- | --- |
| Rust | Orphan (RFC 2451): `impl<P..> Trait<T1..Tn> for T0` is allowed if the trait is local, or some `Ti` is local and no uncovered type parameter appears before it. Fundamental types (`&`, `&mut`, `Box`, `Pin`) do not cover. Overlap: every impl pair is checked; also rejected if a conflicting impl *could* be added upstream ("upstream crates may add a new impl"). Negative reasoning only for traits/types the crate owns (RFC 1023). |
| MoonBit | Only the package of the trait or of the type may write `impl T for Ty`, giving a globally unique impl per pair. Trait visibility controls implementability: `pub` traits are read-only (sealed) outside, `pub(open)` are implementable anywhere. Impl visibility matters. Overlap of generic impls: not documented (unverified). |
| Swift | A type conforms to a protocol at most once, even if conditional constraints look disjoint (SE-0143). Retroactive conformance of a foreign type to a foreign protocol warns unless marked `@retroactive` (SE-0364). Runtime uniqueness is assumed; duplicates are undefined. |
| Kotlin | No impl concept; classes declare interfaces. Extensions are static and imported. |
| hd today | Trait-or-target package owns the impl; trait arguments never confer ownership. Overlap text both uses and ignores bounds (TY-01). Link-time check over interface files. Annotation facets rely on a different, undocumented ownership rule (TY-02). |

Lesson: Swift's "conform once" is the simplest and fully local; Rust's rule
needs global knowledge of who may add impls. hd's link-time check already
accepts some global failure; ignoring bounds removes the rest.

## 2. Impl Selection

| | Rule |
| --- | --- |
| Rust | Candidates: impls (head unify), where-clauses, builtins; winnow by nested obligations; preference builtin trivial > non-global where-bounds > alias bounds > impls. Where-bounds also shadow impls for normalization. Unconstrained impl params rejected (E0207). |
| Swift | Witness = most specialized implementation whose constraints the conformance implies. |
| MoonBit | Unique per pair by placement; no documented solver. |
| hd today | "At most one implementation" plus overlap; no stated candidate order, determinism, or termination (TY-04, TY-24). |

## 3. Supertraits

| | Rule |
| --- | --- |
| Rust | Supertraits are bounds on `Self`; implementing the subtrait requires them (E0277); supertrait items usable under the subtrait bound (implied bounds). |
| Swift | Inherited requirements must be met; with conditional conformance the inherited conformance must be stated explicitly. |
| MoonBit | `trait Sub: A + B`; supertrait methods on a type parameter must be called qualified (`Position::pos(x)`). |
| Kotlin | Interfaces extend interfaces. |
| hd today | `missing-supertrait-implementation` for concrete impls; generic-impl obligations and implied bounds unstated (TY-07, TY-08). |

## 4. Default Methods And Conflicts

| | Rule |
| --- | --- |
| Rust | Same-name methods from two traits: E0034 ambiguity; fully qualified syntax. Defaults are generic bodies. |
| Kotlin | Inheriting two implementations of one member forces an override; `super<A>.foo()` selects. |
| Swift | Extension-only methods are statically dispatched: through `any P` or `T: P` the extension version runs even if the type has its own. Only requirements are customization points. |
| MoonBit | Defaults written `impl Trait with m(...)`; a regular method beats an attached trait method. Implicit dot-callability of trait methods is deprecated because a new upstream default method can make an existing dot call ambiguous. |
| hd today | Conflicts are ambiguous at the dot call; inherent method or `Trait::m` resolves. Default-body dispatch, supertrait defaults, and name reuse unstated (TY-13). |

## 5. Dynamic Safety

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

## 6. Derivation

| | Rule |
| --- | --- |
| Rust | Built-in derives Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd. Bound `T: Trait` on every type parameter (the "perfect derive" problem). Derive plus manual impl conflicts (E0119). Enum ordering by discriminant. |
| MoonBit | Eq, Compare, Debug, Default, Hash, Arbitrary, Shrink, FromJson, ToJson (Show status unverified). "All fields used must implement T." Enum cases ordered by declaration. Derived methods are not dot-callable without `extend`. |
| Swift | Equatable/Hashable synthesized when declared in the type or a same-file extension and all stored properties conform (SE-0185); user-written members win. Comparable for enums without raw type (SE-0266). |
| Kotlin | Data classes generate equals/hashCode/toString/copy/componentN from primary-constructor properties; explicit equals/hashCode/toString suppress generation. |
| hd today | PartialEq, Eq, PartialOrd, Ord, Hash on data and enums; bound per type parameter occurring in a compared field (between Rust and per-field); no field exclusion. Newtypes, GADT existentials, field obligations, placement, law partners unstated (TY-16..TY-19). |

Locality note: Swift and Kotlin forbid mixing generated and hand-written halves
only partly; Kotlin's suppression (a hand-written `equals` suppresses generation)
is implicit. hd's explicit `@derive` list is closer to Rust.

## 7. Method Resolution

| | Rule |
| --- | --- |
| Rust | Receiver steps (autoderef, then `&`, `&mut`); at each step inherent before in-scope trait methods; type-parameter bounds before other traits; several at one step is an error. |
| MoonBit | Methods live in the type's namespace; trait methods dot-callable only after `extend Type with Trait::{..}`; regular beats attached; type-parameter dot calls only for the single written constraint. |
| Swift | Concrete members beat protocol-extension members; more constrained extension wins. |
| Kotlin | Member always beats extension. Delegation `class C(b: B) : B by b`: body overrides beat delegated members; delegate's own calls do not see the overrides. Two delegates supplying one member require an explicit override (unverified). |
| hd today | Inherent > promoted > trait list conflicts with the pooling sentence (TY-03); no permission step; no multi-instantiation rule (TY-09). |

Kotlin delegation is the closest analog to hd embedding-with-bodyless-impl:
explicit opt-in per interface, local overrides win. It supports hd keeping
"embedding never grants conformance".

## 8. Automatic Traits

| | Rule |
| --- | --- |
| Rust | Auto traits (Send, Sync, Unpin, ...) implemented structurally: aggregates if all fields, closures if all captures; explicit generic impls replace the automatic one; negative impls std-only. |
| Swift | Sendable inferred for non-public (or frozen public) structs/enums whose stored members are Sendable; public non-frozen types never inferred "for API resilience"; no inferred conditional conformances. |
| MoonBit | Empty traits implemented automatically (flagged). |
| hd today | `Any` universal, `AnyRef` sealed and closed; `Inspectable` and `NonEscapable` are proposed. |

Swift's resilience argument applies to hd's public-signature rule: a
structurally inferred property of a public type changes when a private field
changes. That argues for declared-and-checked propagation (TQ-24).
