# Member Lookup: Go, Rust, And hd

How `x.name` and `x.name(...)` find a field or method when a type has fields,
inherent methods, trait (interface) methods, and composition. Written to
settle the embedding, trait-promotion, and member-namespace questions
together (E1 to E5, M1, TQ-31, TQ-32).

## The Three Designs

### Go: embedding, one namespace, structural interfaces

```go
type Base struct{ ID int }
func (b Base) Describe() string { return "base" }
type Page struct { Base; Title string }

p.ID          // promoted field
p.Describe()  // promoted method
```

- A struct's fields and methods share one namespace; a field and a method
  with the same name on one type is a compile error.
- Methods are declared only in the type's own package.
- `p.x` searches the embedding tree breadth-first; the shallowest match wins;
  two matches at the same depth are an error only when `x` is used.
- Interfaces are structural, and promoted methods count, so `Page` satisfies
  every interface `Base` satisfies.
- No overriding: inside `Base`'s methods, `b.Describe()` is always
  `Base.Describe`.

### Rust: no embedding, `Deref`, two namespaces, nominal traits

```rust
struct Page { base: Base }
impl Deref for Page { type Target = Base; fn deref(&self) -> &Base { &self.base } }
```

- Fields and methods are separate namespaces, chosen by syntax: `x.len` is a
  field, `x.len()` a method. A type may have both. A function-typed field is
  called as `(x.callback)()`.
- Inherent methods are declared in the type's crate; trait impls follow the
  orphan rule. A trait method is a candidate only when the trait is in scope.
- `Deref` gives one target, so composition lookup is a chain, not a tree. For
  a method call, each step tries inherent methods, then in-scope trait
  methods; the first step with a match wins. Field access walks the same
  chain.
- `Deref` never grants trait conformance, but method-call syntax works through
  it. `&Page` coerces to `&Base` at call sites.
- Using `Deref` for composition is considered an anti-pattern; it is meant
  for smart pointers.

### hd today (after E1 to E5, M2, P2, private embedded members, Rust-style trait lookup, Cut 2 depth semantics, ignored part traits, and the single view)

- Two namespaces, chosen by syntax (M2): `x.name` is field lookup,
  `x.name(args)` is method lookup, and a function-typed field is called as
  `(x.callback)(args)`. A field and a method may share a name.
- Embedding is a tree of at most three levels, and a data type declares at
  most three embedded fields (`embedding-too-deep`,
  `too-many-embedded-fields`). Own members are at depth 0, and the `pub`
  fields and `pub` inherent methods of every part are promoted at the part's
  depth. For each name the shallowest member hides deeper ones, so every
  embedded type decides its own names (Cut 2). Two members with one name at
  the same smallest depth are `ambiguous-promoted-member` at the outer
  type's declaration, on the later of the two embedded fields involved,
  never at a use; a type embedded twice at one depth always conflicts in its
  embedded field name, and at different depths the shallower copy wins. A
  dependency gaining a shallower member still switches a use silently (P3).
- Method lookup selects a visible own inherent method first, whatever its
  signature (E1, E2). Otherwise the candidates are the shallowest promoted
  inherent method and the receiver's trait methods whose trait is available
  at the call, wherever the impl is declared, as in Rust. More than one
  candidate is `ambiguous-method`, so neither a trait method nor a promoted
  method silently wins (Rust-style trait lookup, TQ-36). When the only
  candidates are instantiations of one generic trait, the argument and
  expected types choose among them (TQ-4). A forwarding impl is called as
  `Trait::m(x)`.
- A trait method whose trait is not available is invisible, so a promoted
  method of that name is selected, and a call that finds nothing is
  `unknown-method` with a message suggesting the import.
  `trait-not-in-scope` is gone.
- A type has a single view of its members: every name resolves to the same
  member for every caller, and one declaration check covers every use. A
  private member of a part is never promoted, even in the module that
  declares it; it is reached through the explicit path, `x.Part.secret`.
  Only a `pub` own member hides promoted members; a private own member with
  the name of a promoted member is `ambiguous-promoted-member` at the
  private member's declaration. `private-member` is reported for an own
  member that the caller cannot see, when nothing visible matches (an
  available trait method still can). Embedded fields are always public, so a
  promoted member's path never affects its visibility. This replaces the
  earlier two views (the declaring module's, and the one from other modules
  of a public type), which followed Rust's privacy-aware lookup.
- Embedded types offer fields and inherent methods only. Their trait
  methods neither promote nor block (E4, superseding TQ-31 revised): a call
  reaches a deeper inherent method as if they did not exist, and they are
  called through the part, as in `x.Part.m()`.
  `embedded-trait-method-not-promoted` is gone.
- Embedding never grants trait conformance, and promoted methods never fill
  trait methods (E5). Conformance through a part is explicit:
  `impl Trait for C by E` forwards every trait method to the embedded field
  `E` (trait delegation).
- No overriding: inside `Base`, `self.m()` is always `Base`'s `m`.
- Embedding is value embedding (VE1 to VE4, VE-S): filling an embedded
  field copies the value and is written with `...` (`Label: ...value`,
  `x.Label ...= value`), and access through an embedded field follows its
  container, so a promoted `mut self` method works on a `mut` receiver.
  Through a readonly receiver such a method is still selected and then
  rejected with `mutable-receiver-required`; lookup never skips it. Rule 7
  below (readonly edge) no longer holds.

## Comparison

| Dimension | Go | Rust | hd with M1 | hd proposed | hd today |
| --- | --- | --- | --- | --- | --- |
| Field and method namespaces | one | two, by syntax | one (M1) | **two, by syntax** | two, by syntax |
| Same-named field and method on one type | error | allowed | error | **allowed** | allowed |
| Calling a function-typed field | `x.f()` | `(x.f)()` | `x.f()` | **`(x.f)()`** | `(x.f)()` |
| Who may add methods to a type | its package | inherent: its crate; traits: orphan rule | same as Rust | same as Rust | same as Rust |
| Composition shape | tree | chain | tree | tree | tree |
| Lookup order within one type | one set | inherent, then in-scope traits | inherent, then traits | inherent, then traits | inherent, then in-scope traits pooled with the embedded search |
| Deeper lookup | shallowest wins | next step | shallowest wins | shallowest wins | shallowest wins |
| Same-depth matches | error at use | impossible (chain) | error at use | error at use | error at the outer declaration |
| Trait methods of composed types | promoted | found at their step | not promoted | **promoted** (not adopted; E4 kept) | not promoted; ignored entirely |
| Trait method of the type beside a promoted method | n/a | the type's step wins | trait wins | trait wins | `ambiguous-method` |
| Composition satisfies traits | yes | no | no | no | no |
| Promoted method fills a trait method | yes (structural) | no | no | no | no |
| Overriding | no | no | no | no (stated) | no |
| Converts to the composed type | no | yes (deref coercion) | no | no | no |
| Un-imported trait method | n/a | not a candidate | error, never fallthrough | error, never fallthrough | not a candidate; the not-found message suggests the import |

## Evolution Hazards

Who can break or silently change a working call, and how:

| Change | Go | Rust | hd proposed | hd today |
| --- | --- | --- | --- | --- |
| A trait adds or renames a method | n/a | ambiguity errors only | ambiguity errors only; never collides with a field | ambiguity errors only; never collides with a field |
| The type's package adds a method | local | inherent silently shadows a trait method | same as Rust, local to the type's owner | same as Rust, local to the type's owner |
| An embedded type adds a shallower member | silent switch | n/a | silent switch (accepted in E3, P3) | silent switch (E3, P3) |
| A type adds a private member | none | none (privacy-aware lookup) | none (skipped, P2) | a part's: none, never promoted; the type's own: an error at its declaration when a promoted member has the name, in its own package |
| An embedded type adds a member at the depth of another | error at use | n/a | error at use | error at the outer type's declaration, in its own package |
| A module imports a trait | n/a | can silently switch across `Deref` steps | no switch: presence counts known impls | no switch: a new candidate makes the call an error |
| A package adds a trait impl for the type or an embedded type | n/a | can silently switch across `Deref` steps | the type's impl hides embedded members (P4) | for the type: ambiguity errors only, where the trait is in scope; for an embedded type: no effect |

With one namespace, a trait author who adds or renames a method to a name
that some implementing type uses for a field breaks that type's package,
which the trait author cannot see. Splitting the namespaces removes that
hazard: fields and methods never collide.

## Proposed hd Rule

For a receiver of nominal type `S`:

1. **Syntax chooses the namespace.** `x.name` without a call looks up fields
   only. `x.name(args)` looks up methods only. A function-typed field is
   called as `(x.callback)(args)`. Bound-method values stay deferred, so
   `x.name` never yields a method.
2. **Field lookup.** `S`'s own fields; otherwise embedded fields
   breadth-first. Shortest path wins; two matches at the same depth are
   `ambiguous-promoted-member`.
3. **Method lookup.** At each depth, starting with `S` at depth 0, each type
   offers its inherent methods, then methods of traits it implements. Within
   one type, inherent beats trait and two trait candidates are
   `ambiguous-method`. The shallowest depth with a match wins; matches in
   two types at the same depth are `ambiguous-promoted-member`.
4. **Presence stops the search, in both namespaces.** A name that exists at a
   depth but is not usable there (private, or reachable only through a trait
   not imported in the calling module) is an error, never a fallthrough.
   Presence of a trait method counts every impl in the program's dependency
   graph.
5. **Embedding is not conformance.** A promoted method never fills a trait
   method, and an outer type never satisfies a bound through embedding. A
   bound failure on a type that embeds an implementing type suggests a
   forwarding impl.
6. **No overriding.** A promoted method runs as the embedded type's method:
   inside `Base`, `self.m(...)` is always `Base`'s `m`.
7. **Readonly edge.** A promoted `mut self` method is found and then rejected
   with `mutable-receiver-required`.

Outcome: the owner adopted rules 1, 2, and 4 to 7 as M2. Rule 3 was not
adopted for embedded depths: they offer inherent methods only, and E4 stays.
P2 later revised rule 4: an invisible member is skipped rather than
reported, and only an unavailable trait still stops the search. A later
decision ignores an invisible member of an embedded type entirely: only an
invisible own member of `S` is reported as `private-member`. Rust-style
trait lookup then removed the remaining stop: a trait method is a candidate
only where its trait is in scope, and a trait candidate beside a promoted
method is `ambiguous-method`. Cut 2 moved same-depth conflicts to the outer
declaration, and parts' trait methods were then ignored entirely, replacing
TQ-31's stop rule.

Changes from the applied rules proposed at the time: M1 becomes two namespaces (a field and a
method may share a name; `x.callback()` becomes `(x.callback)()`); E4 is
reversed (trait methods of embedded types are promoted); E2's "an outer field
hides an embedded method" no longer applies, because field and method lookups
are separate. TQ-32 disappears.
