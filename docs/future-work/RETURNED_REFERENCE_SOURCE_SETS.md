# Returned References, Stage 3: Source Sets Or Lifetimes

**Status:** decided 2026-10-29 (#1005): option A, single lifetime variables spelled `'name`, root-only provenance until stage 9. Nothing else in this
document is decided. It studies the open stage 3 syntax in
[The Path To Returned Local References](RETURNED_REFERENCES_PATH.md#stage-3-lifetime-variables).
It reviews [Local References](../../spec/lang/13-values-and-references.md#local-references),
[Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results)
and the parked [tier-2 study](OBJECT_MODEL_TIER2.md).
It changes no specification or compiler behavior.

## Recommendation

Use plain lifetime variables in public types, spelled like Rust's `'src`.
Keep the checker's existing origin sets as an implementation detail. A public
signature records only which reference positions share a lifetime; it does not
record field paths.

```text
fn choose['src](a: &'src User, b: &'src User, first: bool) -> &'src User:  # hypothetical syntax
    if first: a else: b                                                   # hypothetical syntax

struct Parser['src]:                                                      # hypothetical syntax
    text: &'src string                                                    # hypothetical syntax
```

This is the accepted end state's mechanism, makes stored references and
function types possible, and keeps private layout out of public interfaces.
For direct fields of a value-owned source, `{user.name}` and `{user}` die with
the same owner, so paths buy conflict precision rather than lifetime safety.
Paths through independently owned or counted storage need a separate owner
model; that unresolved boundary is one reason not to expose them in stage 3.

The additional spelling cost of `'src` over `^src` is a modest lexer change.
hd currently sends every apostrophe to the character-literal scanner. Rust
demonstrates a deterministic split between a quoted character and an
apostrophe followed by an identifier. Familiarity and one notation across
functions, structs, traits and function types outweigh the small mechanical
delta from reusing `^`.

## Decisions This Study Asks For

1. Does stage 3 expose plain lifetime variables, leaving source paths to stage
   9 only if whole-source freezing proves too strict?
2. If yes, is the spelling `'src`, despite the lexer work, or the novel but
   mechanically cheaper `^src`?
3. Should the checker retain field paths internally now, then widen them to the
   public lifetime at calls, or should it stay root-only until stage 9?

## Fixed Requirements

The stage 3 design must express two inputs sharing one result lifetime. It must
also cover function values, traits and `&dyn`. Its binder model must later
extend to references stored in stage 6 structs and enums. Stored values are a
stress test here, not stage 3 build scope. Elision remains the common case.
Lifetime arguments erase before monomorphization and add no run-time
representation.

The study compares five mechanisms, not five spellings:

| Option | Public mechanism | Changes needed | What it postpones |
| --- | --- | --- | --- |
| A. Plain lifetimes | nominal lifetime binders and equality between reference slots | lifetime syntax, semantics and checker/interface representation | field-level conflict precision |
| B. Public path sets | normalized parameter, field, element and lifetime paths | source-set syntax, path semantics and checker/interface representation | exact precision at private fields, abstract `Self` and counted dereferences without more mechanisms |
| C. Public lifetimes, private path proofs | lifetime binders publicly; field paths tracked only within a body | lifetime syntax and semantics plus internal path analysis | caller-side field precision |
| D. Root sets plus lifetimes | parameter sets on results; lifetime variables elsewhere | two syntaxes, two relation rules and checker/interface representation | field-level conflict precision |
| E. Named sources only | results name parameters; no lifetime binders | source-set syntax and source semantics | stored references and higher-order lifetime abstraction |

Every complete option spends a syntax change; none needs a compiler intrinsic
or core-library addition. Option E is the radical simplification. It ships
multi-source results but does
not satisfy all fixed requirements, so it is a useful stage boundary rather
than the stage 3 end state.

## Cost

The estimate is bottom-up from the current checker and interface code. It is an
engineering range, not measured completed code. Tests are shown separately in
the subtotal. It prices stage 3 declarations and function types, not stage 6
stored ADTs. Both options erase before the backend.

| Component | Plain lifetimes | Public path sets |
| --- | ---: | ---: |
| syntax, green tree, lowering, formatting | 120–200 | 220–350 |
| dependency model, namespace, function-type wire/substitution/unification/display | 300–500 | 430–700 |
| function signature and interface encoding/hash | 90–160 | 150–260 |
| per-reference-slot provenance and call instantiation | 500–800 | 800–1,250 |
| function values, closures, traits, `&dyn`, conformance | 200–350 | 320–520 |
| implementation subtotal | **1,210–2,010** | **1,920–3,080** |
| focused Rust and conformance coverage | 300–500 | 450–750 |
| **total** | **1,510–2,510** | **2,370–3,830** |

The dependency model row includes erasing function-type dependency signatures
in `Types::lower_refs` before monomorphization keys form. Pricing stage 6 now
would add ADT lifetime arguments, field and store escape rules, variance,
substitution and interface work that this table excludes.

Stage 7's later conflict checker is additional: about 500–800 lines for
whole-source liveness and overlap, against 900–1,500 for field prefixes,
element wildcards and invalidation. Stage 3 path provenance remains erased and
safe through current counts. Stage 7 field-precise conflict enforcement through
a counted `*mut` needs run-time path state. Two counted mutable aliases may
reach the same object. Collapsing there needs only a whole-object flag.

Plain lifetimes preserve the existing fixed-point shape. Public paths multiply
states by path depth and replace equality with prefix and wildcard containment.
Interned paths can control allocation, but path-heavy bodies still have more
states to compare. No credible percentage is available before a prototype and
benchmark.

Interface and check/type-wire caches must hash either representation. Plain
metadata is a small lifetime ID per reference slot plus binder tables. A path
adds a formal root and a word for every field, dereference or `[_]` segment.
Public signature metadata affects the interface API hash; higher-order local
types affect the module check artifact. A changed public path must invalidate
dependents because it changes the callable contract.

## Stress-Test Result

Eighteen cases produced no expressibility error under complete designs A, B
and D. B permits a public path to fall back to its root. Public paths need 49
required annotations and 10 optional precision paths. Plain lifetimes need 59;
root-only sets plus variables need 49. Maximal path precision and plain
lifetimes therefore both total 59.

The difference is where the marks go. Lexer, parser and green-tree reference
holders contribute 39 identical marks under every design. Stored collection
references and a dependent function field raise that to 45 of the path
design's 49 required marks, all places where paths buy nothing. Public paths
move ten marks from plain lifetimes' multi-input disambiguation into optional
field precision.

Public paths were clean in 10 cases and friction in 8. Plain lifetimes were
clean in 8 and friction in 10. Root-only sets were clean in 11 and friction in
7. Exact paths without root fallback break cases 1–4, 12 and 15–17. The
boundaries are private or abstract interiors, one trait result, a counted
mutable crossing and unnamed function-type parameters.

Root sets win this count, but require two public relationship forms. Parameter
names serve multi-input results; lifetime variables serve stored and
higher-order references. Plain lifetimes spend ten more marks in four
deliberately ambiguous cases in exchange for one mechanism everywhere.

## Open Points

**Private fields.** A public `{user.private_field}` makes private layout part of
the semantic API, regardless of what internal interface data already knows.
Forbid it or widen it to `{user}`. Widening is the cheaper rule and preserves
encapsulation; plain lifetimes need no exception.

**Traits and `&dyn`.** A trait cannot name an implementation's field. The trait
signature must remain authoritative. A path design therefore permits only
paths visible in the trait and widens an implementation's private path to
`self`. Per-implementation refinement would make bound and dynamic calls depend
on a new associated-path mechanism.

**Past `*mut`.** Counted mutable references may alias. A static path cannot
prove that `a.field` and `b.field` are separate. The practical boundary is to
collapse a path to the whole referent at its first counted dereference. Keeping
field precision would require run-time object/path borrow state, element
wildcards, pointer-replacement invalidation and an ABI cost.

## Implementation Shape If Plain Lifetimes Win

1. Add an erased lifetime binder and a dependency signature mapping flattened
   input and result reference slots to lifetime IDs.
2. Put that dependency signature on callable interfaces and function types, but
   not on monomorphization keys or run-time references.
3. Upgrade dependent-result checking from one origin set per value to one set
   per reference slot. The current aggregate representation cannot distinguish
   `(&'a A, &'b B)` from the two slots swapped.
4. Instantiate the slot map at each named, bound, dynamic or function-value
   call. Alpha-remap binders when checking a trait implementation.
5. Defer path overlap. The body may retain its current root provenance, or
   option C may preserve paths internally and widen them at every interface.

`Types::lower_refs` must remove a function type's dependency signature before
mono keys form; dependency identity remains in interface and check/type-wire
artifacts only.

This order keeps lifetime arguments erased, uses the existing call identity,
and makes stage 6 stored references an extension of the same binder model.

## Appendix A: Stress Test

The complete translations, generated-behavior notes, verdicts, ranked problems
and parser results are in
[Stage 3 Source Sets: Stress Test](RETURNED_REFERENCE_SOURCE_SETS_STRESS_TEST.md).

One written lifetime/source binder, qualifier or set counts as one annotation;
`{a, b}` counts as one. Bare references and elision count as zero. Public paths
are option B, plain lifetimes option A, and root sets option D. The path column
is required plus optional precision paths. Option C has A's public counts and
verdicts. Incomplete option E cannot express cases 5–7, 14 and 17.

| Case and source | Public paths | Plain lifetimes | Root sets |
| --- | ---: | ---: | ---: |
| `TypeId` private-field getter, `lib/std/inspect.hd` | 0+1 | 0 | 0 |
| `List.get`, `first`, `last`, `lib/std/collections.hd` | 0+3 | 0 | 0 |
| `Deque.get`, `front`, `back`, `lib/std/collections.hd` | 0+3 | 0 | 0 |
| `Heap.peek`, `lib/std/collections.hd` | 0+1 | 0 | 0 |
| lexer holding source and bytes, `crates/hd_syntax/src/lexer.rs` | 6 | 6 | 6 |
| parser holding source, tokens and cursor, `crates/hd_syntax/src/parser/mod.rs` | 8 | 8 | 8 |
| `NodeRef`, `ChildIter`, `AstNode`, wrapper, `crates/hd_syntax/src/green.rs` | 25 | 25 | 25 |
| token text chooses source, not `self`, `crates/hd_syntax/src/lexer.rs` | 1 | 3 | 1 |
| JSON field chooses value, not name, `crates/hd_cli/src/install_cmd.rs` | 1 | 3 | 1 |
| two-input `choose` | 1 | 4 | 1 |
| calculator parser `peek_or(fallback)`, `examples/dogfood/calc.hd` | 1 | 4 | 1 |
| `Error.source` trait and `&dyn` chain, `lib/std/error.hd` | 0 | 0 | 0 |
| `Inspectable` downcast on `&dyn`, `lib/std/inspect.hd` | 0 | 0 | 0 |
| store a reference in caller collection, `crates/hd_run/src/frames.rs` | 3 | 3 | 3 |
| precision past `*mut`, `lib/std/http.hd`, `lib/std/format.hd` | 0+1 | 0 | 0 |
| `Map.get` interior, language spec examples | 0+1 | 0 | 0 |
| stored function type sharing one lifetime, `crates/hd_lint/src/lib.rs` | 3 | 3 | 3 |
| single-input dependent function type | 0 | 0 | 0 |
| **Total** | **49+10** | **59** | **49** |

The matrix translates shapes from real code; it does not claim those Rust
types or current hd APIs already hold local references. The counts measure
surface obligations under each proposed mechanism, not implementation lines.
Friction cases are B: 1–4, 12 and 15–17; A: 1–4, 8–11 and 15–16; D: 1–4
and 15–17. Their clean-case totals are therefore 10, 8 and 11.

## Appendix B: Why The Current Checker Is Not Almost There

The current dependent-result checker has reusable least-fixed-point origin
sets, but the public contract is still result-wide:

- source inference is `Owned`, one parameter, missing or ambiguous;
- calls record one source parameter;
- values and held places have one aggregate origin set;
- function-value calls deliberately yield no origins;
- field, dereference and element projections are collapsed;
- callable interfaces and function types carry no dependency identity.

The relevant seams are `crates/hd_check/src/dep_results.rs` for origin solving
and call sources, `crates/hd_resolve/src/iface.rs` for callable interface
encoding, and `crates/hd_types/src/pool.rs` for function and reference type
identity. Existing TIR places have field, dereference, element and variant
projections, but the dependent-result solver currently collapses them.

The first stage-3 correctness step is therefore per-reference-position
provenance, not parsing a binder. Without it, a tuple with independent
lifetimes can swap its reference slots and still pass because the aggregate
origin set is unchanged.

## Appendix C: Sigils And Prior Art

| Spelling | hd cost | Reading | Precedent |
| --- | --- | --- | --- |
| `'src` | add a lifetime token and disambiguate character literals | familiar and compact | Rust |
| `^src` | reuse the existing caret token contextually | clear inside a mixed set; novel and also XOR | none for lifetimes |
| `src: Origin` | add a new generic kind; bare names can collide with values | self-describing | Mojo's typed origin parameters |

[Rust's token grammar](https://doc.rust-lang.org/reference/tokens.html#lifetimes-and-loop-labels)
defines character literals and lifetime tokens separately. Its
[elision rules](https://doc.rust-lang.org/reference/lifetime-elision.html)
give each input its own lifetime, use a sole input lifetime for an elided
result, and prefer the receiver for method results. That is already the model
accepted for hd's ordinary signatures.

[Mojo origins](https://mojolang.org/docs/manual/values/lifetimes/) are
parameter values. `origin_of(expression)` accepts a field expression, multiple
expressions form a union, and a `ref[a, b]` result keeps both inputs alive. This
shows that one source-set language can work, but it also brings origin values,
mutability and path expressions into the type interface.

[Swift's experimental lifetime annotations](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md)
name source parameters with `borrow`, `&` or `copy` and provide single-parameter
and receiver defaults. Its lifetime subtyping permits an interface source
superset while requiring dependency kinds to match. [Swift `Ref` and `MutableRef`](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0519-ref-mutableref-types.md)
show stored first-class references while keeping dependencies on inputs rather
than exposing field paths.

## Appendix D: Alternatives

### A. Plain nominal lifetimes

One lifetime ID can label several inputs and results. Interfaces contain
binders and equality relationships. Stage 7 later freezes whole roots named by
calls and results.

### B. Unified public source sets

```text
fn name(user: &User) -> &{user.name} string             # hypothetical syntax
fn choose(a: &User, b: &User) -> &{a, b} User           # hypothetical syntax
struct Parser[^src]:                                    # hypothetical syntax
    text: &{^src} string                                # hypothetical syntax
```

Interfaces contain normalized paths and unions. This preserves field precision
across calls, but makes privacy, path substitution, traits, wildcards and
counted dereferences part of stage 3.

### C. Public lifetimes, private path proofs

```text
fn name['src](user: &'src User) -> &'src string:  # hypothetical syntax
    &user.name                                     # hypothetical syntax
```

The body can prove the precise path, but the exported interface widens it to
the lifetime. This preserves encapsulation and compact caches. It helps only
inside a body until a later interface mechanism exposes field precision.

### D. Root-only source sets plus lifetime variables

```text
fn choose(a: &User, b: &User) -> &{a, b} User           # hypothetical syntax
struct Parser[^src]:                                    # hypothetical syntax
    text: &{^src} string                                # hypothetical syntax
```

Multi-input results use compact root sets; stored and function types still use
lifetime variables. This scored best on annotations and friction, but makes
programmers learn two overlapping relationship mechanisms.

### E. Named sources without lifetime variables

```text
fn choose(a: &User, b: &User) -> &{a, b} User           # hypothetical syntax
```

This is sufficient for multi-source function results. It cannot name a lifetime
inside `Parser`, a collection element type or a higher-order function type, so
stage 6 would need a second mechanism.

## Appendix E: Parse Log

Every block uses syntax no current chapter specifies; parsing checks syntax
only. The 2026-10-29 parser run rejected them as expected:

| Block | Content | First parser result |
| ---: | --- | --- |
| 1 | recommended `'src` functions and stored type | `unclosed-delimiter` at the lifetime binder, followed by character-literal errors |
| 2 | public path sets | `expected a type, found '{'` |
| 3 | public lifetimes with private path proof | `invalid-token: a character literal must contain one Unicode scalar value` |
| 4 | root sets plus lifetime variables | `expected a type, found '{'` |
| 5 | named sources only | `expected a type, found '{'` |

## Questions For The Owner

1. **Public mechanism.** This decides whether stage 3 has one relationship
   mechanism or exposes paths too. Choose plain lifetimes (recommended),
   unified path sets, or root sets plus lifetime variables. Examples are
   `fn choose['a](x: &'a T, y: &'a T) -> &'a T` and `&{x, y} T`.
2. **Lifetime sigil.** This trades a familiar syntax and a lexer change against
   a novel contextual token. Choose Rust-style `'src` (recommended), `^src`,
   or a typed parameter such as `src: Origin`.
3. **Private precision.** This decides whether stage 3 pays for field paths that
   callers cannot see. Keep provenance root-only until measured stage 7
   conflicts justify stage 9 (recommended), or track `user.name` privately and
   widen it to `user` at every interface.
