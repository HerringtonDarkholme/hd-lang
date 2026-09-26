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

### hd today (after E1 to E5, M2, and P2)

- Two namespaces, chosen by syntax (M2): `x.name` is field lookup,
  `x.name(args)` is method lookup, and a function-typed field is called as
  `(x.callback)(args)`. A field and a method may share a name.
- Embedding is a tree, searched breadth-first; shortest path wins; a
  same-depth clash is an error (E3).
- Each lookup checks the receiver's own members first: its fields, or its
  inherent and trait methods (E1). A visible member, or any own trait
  method, stops the search whatever its signature (E2).
- Members not visible from the calling module are skipped, at every depth,
  as in Rust's privacy-aware lookup; `private-member` is reported only when
  nothing visible matches. Inside the defining module the private member
  wins. An own trait method whose trait is not imported still stops the
  search with `trait-not-in-scope` (P2).
- Embedded types offer fields and inherent methods only. Trait methods count
  only on the receiver's own type; the search skips an embedded type's trait
  methods and continues below it (E4, TQ-31).
- Embedding never grants trait conformance, and promoted methods never fill
  trait methods (E5).
- No overriding: inside `Base`, `self.m()` is always `Base`'s `m`.

## Comparison

| Dimension | Go | Rust | hd with M1 | hd proposed |
| --- | --- | --- | --- | --- |
| Field and method namespaces | one | two, by syntax | one (M1) | **two, by syntax** |
| Same-named field and method on one type | error | allowed | error | **allowed** |
| Calling a function-typed field | `x.f()` | `(x.f)()` | `x.f()` | **`(x.f)()`** |
| Who may add methods to a type | its package | inherent: its crate; traits: orphan rule | same as Rust | same as Rust |
| Composition shape | tree | chain | tree | tree |
| Lookup order within one type | one set | inherent, then in-scope traits | inherent, then traits | inherent, then traits |
| Deeper lookup | shallowest wins | next step | shallowest wins | shallowest wins |
| Same-depth matches | error at use | impossible (chain) | error at use | error at use |
| Trait methods of composed types | promoted | found at their step | not promoted | **promoted** (not adopted; E4 kept) |
| Composition satisfies traits | yes | no | no | no |
| Promoted method fills a trait method | yes (structural) | no | no | no |
| Overriding | no | no | no | no (stated) |
| Converts to the composed type | no | yes (deref coercion) | no | no |
| Un-imported trait method | n/a | not a candidate | error, never fallthrough | error, never fallthrough |

## Evolution Hazards

Who can break or silently change a working call, and how:

| Change | Go | Rust | hd proposed |
| --- | --- | --- | --- |
| A trait adds or renames a method | n/a | ambiguity errors only | ambiguity errors only; never collides with a field |
| The type's package adds a method | local | inherent silently shadows a trait method | same as Rust, local to the type's owner |
| An embedded type adds a shallower member | silent switch | n/a | silent switch (accepted in E3, P3) |
| A type adds a private member | none | none (privacy-aware lookup) | none (skipped, P2) |
| A module imports a trait | n/a | can silently switch across `Deref` steps | no switch: presence counts known impls |

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
reported, and only an unavailable trait still stops the search.

Changes from the applied rules proposed at the time: M1 becomes two namespaces (a field and a
method may share a name; `x.callback()` becomes `(x.callback)()`); E4 is
reversed (trait methods of embedded types are promoted); E2's "an outer field
hides an embedded method" no longer applies, because field and method lookups
are separate. TQ-32 disappears.
