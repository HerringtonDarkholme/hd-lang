# Stage 3 Source Sets: Stress Test

**Status:** design review, 2026-10-29 (#1005); changes no decision, design
record, specification text or compiler code. It tests the open stage 3 choice
in [The Path To Returned Local References](RETURNED_REFERENCES_PATH.md#stage-3-lifetime-variables)
against [Local References](../../spec/lang/13-values-and-references.md#local-references)
and [Dependent Results](../../spec/lang/13-values-and-references.md#dependent-results).

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison](#comparison)
- [Problems, Ranked](#problems-ranked)
- [Parse Log](#parse-log)

## Surface Being Tested

The common surface is stage 3: explicit dependencies only where elision cannot
choose, erased before monomorphization, with whole-source conflict enforcement
deferred to stage 7. Stored structures exercise whether the same binder model
can extend to stage 6; they are not stage 3 implementation scope.

| Variant | Surface |
| --- | --- |
| A | Rust-style nominal lifetime variables, such as `&'a T` |
| B | public path sets and lifetime variables, such as `&{user.name} T` and `&{^a} T` |
| C | A publicly, with field paths proved only inside a body |
| D | root-only source sets on results plus lifetime variables elsewhere |
| E | root-only named sources, with no lifetime variables |

Variant B permits a root fallback when an exact path is private, abstract or
unstable. The report also records where exact-path B would break. Variant C has
the same public annotation counts and verdicts as A.

## Method

The 18 cases approximate shapes already present in hd's std and compiler. They
cover getters, collections, lexer/parser data, green-tree wrappers, multiple
inputs, traits, `&dyn`, a counted mutable crossing and function types.

One written lifetime/source binder, qualifier or set counts as one annotation;
`{a, b}` counts as one. Bare references and elision count as zero. Source text
counts once, without macro expansion. `works` preserves the shape directly;
`friction` needs a root fallback, broader future freeze or extra annotations;
`breaks` cannot express the shape. Parsing checks syntax only, not types or the
proposed semantics.

For every translation, generated stage 3 behavior is dependency metadata in
the callable or function type and the same erased run-time signature. Stage 7
would later use the dependency to enforce conflicts. Each case calls out any
different generated behavior.

## Summary

| # | Shape | A | B | D | Element at fault |
| ---: | --- | --- | --- | --- | --- |
| 1 | private-field getter | friction | friction | friction | private path or whole-root freeze |
| 2 | `List` element getters | friction | friction | friction | opaque collection interior |
| 3 | `Deque` element getters | friction | friction | friction | computed wrapped slot |
| 4 | `Heap.peek` | friction | friction | friction | opaque collection interior |
| 5 | lexer stores source and bytes | works | works | works | lifetime variables do the work |
| 6 | parser stores source, tokens, cursor | works | works | works | lifetime variables do the work |
| 7 | green-tree wrappers and iterator | works | works | works | lifetime variables do the work |
| 8 | token text chooses source | friction | works | works | multi-input disambiguation |
| 9 | JSON field chooses value | friction | works | works | multi-input disambiguation |
| 10 | choose either input | friction | works | works | multi-input disambiguation |
| 11 | parser fallback result | friction | works | works | multi-input disambiguation |
| 12 | `Error.source` through `&dyn` | works | friction | works | implementation path erased by trait |
| 13 | `Inspectable` downcast on `&dyn` | works | works | works | receiver root is sufficient |
| 14 | store ref in caller collection | works | works | works | lifetime variables do the work |
| 15 | precision across counted mutable ref | friction | friction | friction | aliases erase static path identity |
| 16 | `Map.get` interior | friction | works | works | exact key path is not stable |
| 17 | higher-ranked stored function type | works | friction | friction | unnamed function parameters need variables |
| 18 | single-input dependent function type | works | works | works | elision is sufficient |

| Measure | A | B | C | D | E |
| --- | ---: | ---: | ---: | ---: | ---: |
| required annotations | 59 | 49 | 59 | 49 | incomplete |
| optional precision paths | 0 | 10 | internal only | 0 | 0 |
| works / friction / breaks | 8 / 10 / 0 | 10 / 8 / 0 | 8 / 10 / 0 | 11 / 7 / 0 | 13 / 0 / 5 |

Exact-path B, without root fallback, breaks cases 1–4, 12 and 15–17. E
breaks 5–7, 14 and 17 because those shapes need a lifetime not named by a
current parameter.

The bulk is invariant: cases 5–7, 14 and 17 contribute 45 of B and D's 49
required annotations. Paths move ten annotations from A's ambiguous
multi-input signatures into optional precision paths; maximal B and A both
total 59.

## Cases

### 1. Private-Field Getter

Source: `TypeId`'s private fields in `lib/std/inspect.hd`. The translation
approximates a getter absent from the current API.

```text
fn name_path(&self) -> &{self.name} string: &self.name  # hypothetical syntax
fn name_life['a](&'a self) -> &'a string: &self.name   # hypothetical syntax
fn name_root(&self) -> &{self} string: &self.name      # hypothetical syntax
```

Generated behavior is one receiver dependency. B must publish a private field
or widen to `self`; A and D later freeze more of the value. Verdict: friction
for all three.

### 2. List Element Getters

Source: `List.get`, `first` and `last` in `lib/std/collections.hd`.

```text
fn first_path[T](xs: &List[T]) -> &{xs[_]} T?  # hypothetical syntax
fn first_life['a, T](xs: &'a List[T]) -> &'a T?  # hypothetical syntax
fn first_root[T](xs: &List[T]) -> &{xs} T?     # hypothetical syntax
```

The result points into a compiler-owned copy-on-write buffer, not an ordinary
field. B needs an opaque interior identity or root fallback. A and D preserve
the result but later freeze the whole list. Verdict: friction.

### 3. Deque Element Getters

Source: `Deque.get`, `front` and `back` in `lib/std/collections.hd`.

```text
fn front_path[T](xs: &Deque[T]) -> &{xs.slots[_]} T?  # hypothetical syntax
fn front_life['a, T](xs: &'a Deque[T]) -> &'a T?      # hypothetical syntax
fn front_root[T](xs: &Deque[T]) -> &{xs} T?           # hypothetical syntax
```

The actual slot is computed through ring-buffer wraparound. B cannot expose a
stable source path without an element wildcard and the private `slots` field.
Root fallback works. Verdict: friction.

### 4. Heap Peek

Source: `Heap.peek` in `lib/std/collections.hd`.

```text
fn peek_path[T < Ord](heap: &Heap[T]) -> &{heap.items[_]} T?  # hypothetical syntax
fn peek_life['a, T < Ord](heap: &'a Heap[T]) -> &'a T?        # hypothetical syntax
fn peek_root[T < Ord](heap: &Heap[T]) -> &{heap} T?           # hypothetical syntax
```

B again exposes a private collection interior. All variants preserve the
operation with root fallback; the future freeze is broader. Verdict: friction.

### 5. Lexer Holding Source And Bytes

Source: Rust `Lexer<'s>` in `crates/hd_syntax/src/lexer.rs`.

```text
struct LexerPath[^s]:                 # hypothetical syntax
    source: &{^s} string              # hypothetical syntax
    bytes: &{^s} Bytes                # hypothetical syntax
struct LexerLife['s]:                 # hypothetical syntax
    source: &'s string                # hypothetical syntax
    bytes: &'s Bytes                  # hypothetical syntax
```

Both complete designs generate two fields sharing one erased binder. D also
uses the lifetime form because no current parameter names the stored source. E
cannot express the structure. Verdict: works for A–D; E breaks.

### 6. Parser Holding Three References

Source: Rust `Parser<'t>` in `crates/hd_syntax/src/parser/mod.rs`.

```text
struct ParserPath[^t]:                # hypothetical syntax
    src: &{^t} string                 # hypothetical syntax
    toks: &{^t} TokenBuf              # hypothetical syntax
    cur: LayoutCursor[^t]             # hypothetical syntax
struct ParserLife['t]:                # hypothetical syntax
    src: &'t string                   # hypothetical syntax
    toks: &'t TokenBuf                # hypothetical syntax
    cur: LayoutCursor['t]             # hypothetical syntax
```

The binder must flow through a nested stored type. Paths offer no reduction.
E cannot express it. Verdict: works for A–D; E breaks.

### 7. Green-Tree Views And Iterator

Source: `NodeRef`, `ChildIter`, `AstNode` and wrappers in
`crates/hd_syntax/src/green.rs`.

```text
struct NodeRef['tree]:                # hypothetical syntax
    tree: &'tree GreenTree            # hypothetical syntax
struct ChildIter['tree]:              # hypothetical syntax
    parent: NodeRef['tree]            # hypothetical syntax
fn children['tree](node: NodeRef['tree]) -> ChildIter['tree]  # hypothetical syntax
```

The same lifetime crosses several ADTs and an iterator. Every complete design
uses variables; E cannot. Verdict: works for A–D; E breaks.

### 8. Token Text Chooses The Source Parameter

Source: `TokenBuf::text<'s>` in `crates/hd_syntax/src/lexer.rs`.

```text
fn text_path(&self, token: TokenIdx, source: &string) -> &{source} string  # hypothetical syntax
fn text_life['s](&self, token: TokenIdx, source: &'s string) -> &'s string  # hypothetical syntax
fn text_root(&self, token: TokenIdx, source: &string) -> &{source} string  # hypothetical syntax
```

B and D name `source` once. A needs one binder and two qualifiers because
`self` is another reference position. Verdict: B and D work; A has annotation
friction.

### 9. JSON Field Chooses Value, Not Name

Source: the field shape in `crates/hd_cli/src/install_cmd.rs`.

```text
fn field_path(name: &string, value: &string) -> &{value} string  # hypothetical syntax
fn field_life['v](name: &string, value: &'v string) -> &'v string  # hypothetical syntax
fn field_root(name: &string, value: &string) -> &{value} string  # hypothetical syntax
```

The operation is another two-input disambiguation. Generated behavior and
result are otherwise identical. Verdict: B and D work; A has annotation
friction.

### 10. Choose Either Input

Source: the stage 3 motivating union operation.

```text
fn choose_path(a: &User, b: &User, first: bool) -> &{a, b} User  # hypothetical syntax
fn choose_life['x](a: &'x User, b: &'x User, first: bool) -> &'x User  # hypothetical syntax
fn choose_root(a: &User, b: &User, first: bool) -> &{a, b} User  # hypothetical syntax
```

All three keep both arguments alive. A writes the same variable three times;
B and D use one union. Verdict: B and D work; A has annotation friction.

### 11. Parser Peek Or Fallback

Source: the parser shape around `peek` in `examples/dogfood/calc.hd`.

```text
fn peek_path(&self, fallback: &Token) -> &{self, fallback} Token  # hypothetical syntax
fn peek_life['x](&'x self, fallback: &'x Token) -> &'x Token      # hypothetical syntax
fn peek_root(&self, fallback: &Token) -> &{self, fallback} Token  # hypothetical syntax
```

The result chooses parser storage or the fallback. B and D express the union
once; A repeats a variable. Verdict: B and D work; A has annotation friction.

### 12. Error Source Through A Trait Value

Source: `Error.source` and its `&dyn Error` chain in `lib/std/error.hd`.

```text
trait ErrorPath:                                                       # hypothetical syntax
    fn source(&self) -> &{self} dyn Error?                              # hypothetical syntax
impl ErrorPath for WrappedError:                                       # hypothetical syntax
    fn source(&self) -> &{self.inner} dyn Error?: &self.inner           # hypothetical syntax
```

The trait cannot name an implementation field. Exact B breaks; root fallback
widens the implementation to the trait's `self`. A and D already use that
abstract root. Verdict: B has friction; A and D work.

### 13. Downcast On A Trait Value

Source: `Inspectable.downcast_ref` in `lib/std/inspect.hd`.

```text
impl dyn Inspectable:                                                  # hypothetical syntax
    fn downcast_ref_path[T < Inspectable](&self) -> &{self} T?          # hypothetical syntax
    fn downcast_ref_life['a, T < Inspectable](&'a self) -> &'a T?       # hypothetical syntax
```

The recovered reference depends on the trait receiver itself, so no
implementation field is exposed. Elision can remove the explicit marks in all
complete variants. Verdict: works.

### 14. Store A Reference In The Caller's Collection

Source: a frame collection shape in `crates/hd_run/src/frames.rs`.

```text
fn remember_path[^a, T](seen: &mut List[&{^a} T], value: &{^a} T) -> void  # hypothetical syntax
fn remember_life['a, T](seen: &mut List[&'a T], value: &'a T) -> void       # hypothetical syntax
```

The source is a lifetime inside a caller-owned element type, not a parameter
path. B and D still need variables. E cannot express the store. Verdict: works
for A–D; E breaks.

### 15. Precision Past A Counted Mutable Reference

Source: mutable state shapes in `lib/std/http.hd` and `lib/std/format.hd`.

```text
fn field_path(state: *mut State) -> &{state.current} Item  # hypothetical syntax
fn field_life['a](state: *'a mut State) -> &'a Item        # hypothetical syntax
fn field_root(state: *mut State) -> &{state} Item          # hypothetical syntax
```

Two counted mutable aliases can reach the same object, so a static field path
does not preserve identity. B collapses at `*mut` or needs future run-time path
state. A and D are already whole-root. Verdict: friction for all three.

### 16. Map Interior Getter

Source: `Map.get` shapes in the expressions and modules chapters.

```text
fn get_path[K, V](map: &Map[K, V], key: K) -> &{map[_]} V?  # hypothetical syntax
fn get_life['a, K, V](map: &'a Map[K, V], key: K) -> &'a V?  # hypothetical syntax
fn get_root[K, V](map: &Map[K, V], key: K) -> &{map} V?     # hypothetical syntax
```

The key is dynamic and copy-on-write storage is compiler-owned. B can use an
element wildcard only as optional precision, so its required contract falls
back to the root. Verdict: B and D work; A has future freeze friction.

### 17. Stored Higher-Ranked Function Type

Source: a stored checker function type in `crates/hd_lint/src/lib.rs`.

```text
type CheckPath[T] = for[^a] fn(&{^a} T, &{^a} Context) -> void  # hypothetical syntax
type CheckLife[T] = for['a] fn(&'a T, &'a Context) -> void      # hypothetical syntax
```

Function-type parameters have no source variable names available outside the
type. B and D still need lifetime variables; exact path syntax alone cannot
express the relation. E breaks. Verdict: A works; B and D have dual-mechanism
friction.

### 18. Single-Input Dependent Function Type

Source: stage 3's `fn(&User) -> &string` shape.

```text
type GetName = fn(&User) -> &string
```

Elision supplies the sole input lifetime. All complete variants generate the
same dependency signature without a written annotation. Verdict: works.

## Comparison

| Reference | What it demonstrates | Difference from the recommendation |
| --- | --- | --- |
| [Rust lifetime elision](https://doc.rust-lang.org/reference/lifetime-elision.html) | nominal binders, receiver/sole-input defaults, higher-ranked function types | no public source paths |
| [Mojo origins](https://mojolang.org/docs/manual/values/lifetimes/) | origin parameters, arbitrary expression origins and unions | origins are compile-time values and include mutability |
| [Swift lifetime annotations](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md) | dependencies on named inputs, defaults and interface subtyping | stored refs use non-Escapable types rather than public field paths |

Rust is the closest complete precedent for cases 5–7, 14, 17 and 18. Mojo is
the closest precedent for B's cases 8–11. Swift shows that named-source
dependencies can remain at function boundaries, but does not remove the need
for a stored-reference model.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| ---: | --- | --- | --- | --- |
| 1 | `S3-PATH-ABSTRACTION` | public paths cross private or abstract interiors | high | 1–4, 12, 16 |
| 2 | `S3-VARIABLE-BULK` | lifetime variables remain 45 of 49 required B/D marks | high | 5–7, 14, 17 |
| 3 | `S3-COLLECTION-INTERIOR` | collection storage is not an ordinary field path | medium | 2–4, 11, 16 |
| 4 | `S3-TRAIT-ERASURE` | an implementation field cannot appear in a trait contract | medium | 12 |
| 5 | `S3-COUNTED-CROSSING` | counted aliases erase static target identity | medium | 15 |
| 6 | `S3-FUNCTION-TYPE` | unnamed function-type parameters need binders | medium | 17 |

### `S3-PATH-ABSTRACTION`

**Effect.** Exact B cannot state six public APIs without exposing private
layout or inventing an abstract interior identity. Root fallback makes the APIs
work but gives up the precision B was meant to add.

**Candidates.** Should public paths forbid private segments and widen to the
root, use a new opaque interior identity, or defer public paths until stage 9?

### `S3-VARIABLE-BULK`

**Effect.** Stored data and function types contribute 45 required annotations
where paths provide no alternative. A path design is therefore additive, not a
replacement for lifetime variables.

**Candidates.** Should stage 3 expose only lifetime variables, expose root sets
plus variables, or accept the full path-plus-variable surface?

### `S3-COLLECTION-INTERIOR`

**Effect.** A list element, deque slot and map entry depend on compiler-owned
copy-on-write buffers, dynamic indices or keys, and pin rules. Field syntax
does not describe those identities.

**Candidates.** Should interiors collapse to their collection root, receive an
opaque compiler identity, or wait for the stage 4/7 element model?

### `S3-TRAIT-ERASURE`

**Effect.** A trait can promise dependence on `self`, but cannot promise an
implementation's `self.inner`. Dynamic and bounded calls need one contract.

**Candidates.** Should implementations always widen to the trait root, or
should a later design add an associated interior identity?

### `S3-COUNTED-CROSSING`

**Effect.** Replacing or aliasing a counted mutable reference destroys a static
field identity. Preserving it through future stage 7 conflicts needs run-time
path state.

**Candidates.** Should a path collapse at the first counted dereference, or
should stage 7 price an opaque run-time identity before public paths ship?

### `S3-FUNCTION-TYPE`

**Effect.** A function type has positions, not source parameter names in an
enclosing signature. Named-source-only E cannot relate them, and B/D need a
second variable mechanism.

**Candidates.** Should higher-ranked lifetime binders be the one general
mechanism, or should function types get a separate positional source syntax?

## Parse Log

The 2026-10-29 parser run checked all 18 `text` blocks. Parsing is syntax
only. Block 18 is current syntax and parsed cleanly. Every other block contains
marked hypothetical syntax and was rejected at that syntax as expected.

| Block / case | First parser result |
| ---: | --- |
| 1 | `expected a type, found '{'` |
| 2 | `expected a type, found '{'` |
| 3 | `expected a type, found '{'` |
| 4 | `expected a type, found '{'` |
| 5 | `expected a generic parameter name, found '^'` |
| 6 | `expected a generic parameter name, found '^'` |
| 7 | `unclosed '[' delimiter` at the lifetime binder |
| 8 | `expected a type, found '{'` |
| 9 | `expected a type, found '{'` |
| 10 | `expected a type, found '{'` |
| 11 | `expected a type, found '{'` |
| 12 | `expected a type, found '{'` |
| 13 | `expected a type, found '{'` |
| 14 | `expected a generic parameter name, found '^'` |
| 15 | `expected a type, found '{'` |
| 16 | `expected a type, found '{'` |
| 17 | `expected a type, found 'for'` |
| 18 | parse clean |
