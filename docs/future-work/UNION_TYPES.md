# Positional Union Types

Status: fully specified and built 2026-10-31. Conformance, both-engine parity,
compile time, release Wasm, and web size meet the acceptance bars in Appendix F.
This record turns the decisions in [#804](https://github.com/HerringtonDarkholme/hd-lang/issues/804)
into an implementation-sized design. It changes no specification or compiler
behavior beyond the slices named in the status line. A prior [twelve-case stress test](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/UNION_TYPES.md)
supplies the evidence summarized in Appendix A.

## Proposal

`A | B` is an ordered anonymous enum, as a tuple is an ordered anonymous
struct. Its payload variants are `_0(A)` and `_1(B)`. `A | B | C` has three
variants; `(A | B) | C` has two outer variants and a nested union in `_0`.
Order matters, duplicates remain, and construction is always explicit.

```text
type LoadError = HttpError | JsonError
fn load!() -> Result[Document, LoadError] $ Http:
    response := http.get!(url).map_err(Union._0)?
    response.json::[Document]().map_err(Union._1)

fn message(error: LoadError) -> string:
    match error:
        ._0(http) => "http: ${http}"
        ._1(json) => "json: ${json}"
```

There is no member-to-union coercion, type-directed injection, commutative
equality, normalization or type-test pattern. A union is available for all
types, though errors motivate it. Aliases name the anonymous type; they do not
create a nominal wrapper.

This is chiefly shorthand for an enum that a programmer can write today:

```text
enum Union[A, B]:
    _0(A)
    _1(B)

type LoadError = Union[HttpError, JsonError]
```

## Problems It Solves

Named enums remain the right public boundary when member names, messages,
serialization or stable evolution matter. A positional union removes the
wrapper ceremony for small, local sums:

| Use | Named enum today | Positional union |
| --- | --- | --- |
| HTTP plus JSON | enum, two variants, conversion or three maps | one alias, three explicit maps |
| Generic timeout | wrapper `enum Timed[E]` | `E | TimedOut`, including when `E = TimedOut` |
| Local parser stages | a new enum for each short pipeline | an ordered local alias |
| Nested policy errors | multiple wrapper enums | explicit nested sums; no flattening surprise |

The explicit maps are deliberate. They show which positional case a boundary
assigns, work when two members have the same type, and add no new conversion
priority to `?`.

## How It Works

- The parser treats `|` in a type as an ordered separator. An unparenthesized
  chain is flat. Parentheses preserve nesting. The formatter writes one space
  on each side.
- Type identity includes arity, order, duplicates and nesting. Aliases expand
  normally, so aliases of the same union are the same type.
- Construction uses `._k(value)` where an expected union is already known.
  `Union._k(value)` uses a compiler-known constructor namespace, resolved from
  the expected anonymous-union type; it is not a declaration, prelude item or
  source-level variadic generic. Without an expected union type it reports
  `cannot-infer-type` and asks for an annotation. Matching uses the same
  positional variant names and ordinary enum exhaustiveness.
- `?` is unchanged. A programmer first uses `map_err(Union._k)`, then `?`
  propagates the resulting union because its type already matches.
- Representation, drop behavior, copying, borrowing and pattern lowering are
  exactly those of an equivalent enum. `(A | B) | C` therefore has an outer
  and inner discriminant; flat `A | B | C` has one.
- Conditional `Eq`, `Debug`, `Hash`, `Serialize`, and `Deserialize`
  implementations treat the union as a positional enum. Display and Error
  follow the member types too. Like thiserror's transparent wrapper, Display
  and `Error.source` forward to the active payload; the union adds no source
  chain layer.
- No public `Structure`, automatic `Ord`, or public field names are introduced.
  Use a named enum when those semantics are needed.

## Cost And Performance Budget

These are engineering estimates from the earlier compiler inventory, not
measurements. Each phase should be measured before the next one lands.

| Part | Production Rust | Compile cost | Runtime / Wasm | Web compiler gzip |
| --- | ---: | --- | --- | ---: |
| Type grammar and formatting | 60-110 | one type-parser branch | none | 1-2 KB |
| Canonical type identity and aliases | 120-220 | O(members) per union | none | 1-3 KB |
| Constructors, patterns, exhaustiveness | 80-160 | ordinary enum work | none | 1-3 KB |
| Lowering, layout and ownership | 50-110 | ordinary enum work | same as named enum | 1-2 KB |
| Conditional trait templates | 150-260 | instantiated on demand | only used methods | 3-5 KB |
| **Total** | **460-860** | target: <1% on compiler corpus | target: byte-equivalent to named control | **7-15 KB** |

The acceptance bar is no measurable runtime regression and no release-Wasm
size difference from the equivalent named enum after names are stripped. A
web-compiler increase over 15 KB gzip, or a compiler-corpus increase over 1%,
requires a smaller follow-up design before implementation continues.

## Appendix A: Stress Test

The historical report translated twelve cases and ran eight complete named
enum controls on both Node and Wasmtime: 14 clean parse cases, one intentional
syntax failure, and 16 successful executions. The explicit design added seven
positional constructions and five `map_err` calls across the independent
HTTP/JSON and full policy ports.

| Case | Real operation | Result for this design |
| --- | --- | --- |
| thiserror HTTP + JSON | distinct transport and decode failures | works; three explicit maps replace conversions |
| anyhow context | add operation and path context | union alone does not add context; keep a wrapper |
| Go wrapped errors | inspect concrete causes through a chain | transparent forwarding preserves the active payload's existing chain |
| Effect selected recovery | remove one error from a residual set | requires explicit remapping; no set subtraction |
| generic retry | preserve arbitrary `E` | works without constructing a union |
| generic timeout | return `E` or `TimedOut` | works; two explicit constructors; `E = TimedOut` is unambiguous by position |
| timeout/retry/fallback policy | compose several mechanisms | works; fallback still needs an adapter |
| nested and duplicate cases | preserve exact shape | works; order, duplicates and parentheses stay visible |

The most important control uses only present-day enum behavior:

```text
enum Timed[E]:
    _0(E)
    _1(TimedOut)

fn timed_error(error: HttpError) -> Timed[HttpError]:
    Timed._0(error)
```

That control established that a one-payload enum variant is already a
function value, expected result types solve its generic arguments, and an
unsolved constructor already reports `cannot-infer-type`. The proposed union
reuses all three facts.

The stress test also found thirteen generic `@error` declarations in the
repository and none in `lib/std`. Restricting union members to nongeneric
errors would therefore reject real project practice without simplifying the
ordered representation.

## Appendix B: Prior Art

### Rust: nominal, explicit sums

Rust enum variants are named under the enum and may carry different payloads.
That is the closest semantic model; hd removes the declaration and substitutes
positional names. [Source](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html)

```rust
enum LoadError {
    Http(HttpError),
    Json(JsonError),
}

let error = LoadError::Http(http_error);
```

Rust's `Result` chapter also shows the tradeoff: `?` returns errors through
`From`, so a named application error owns conversion policy.
[Source](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)

### Scala 3: structural set union

Scala's `A | B` accepts an `A` or a `B` directly and widens inferred union
types. It is commutative set-like typing, not a tagged positional sum.
[Source](https://docs.scala-lang.org/scala3/reference/new-types/union-types.html)

```scala
def size(value: String | Int): Int = value match
  case text: String => text.length
  case number: Int  => number.toString.length
```

hd rejects this model: it needs normalization, implicit injection and runtime
type tests, and cannot distinguish two same-typed cases.

### Kotlin rich errors: flattened error sets

Kotlin's proposal gives functions explicit error sets and combines aliases as
flat unions. It deliberately treats them as sets rather than values with
ordered tags. [Source](https://github.com/Kotlin/KEEP/blob/main/proposals/KEEP-0441-rich-errors-motivation.md)

```kotlin
typealias LoadErrors = NetworkError | ParseError

fun load(): Document raises LoadErrors =
    parse(fetch())
```

That better supports selective propagation, but adds a second error-specific
type system. hd's candidate is an ordinary sum usable outside errors.

### Go: concrete errors in a source chain

Go's `errors.As` traverses wrapped errors to find a concrete member. The Go
team also warns that wrapping exposes the wrapped error as API. hd's union is
transparent instead: its `source` is the active payload's own source, so the
anonymous wrapper adds no extra chain layer.
[Source](https://go.dev/blog/go1.13-errors)

```go
var pathError *os.PathError
if errors.As(err, &pathError) {
    log.Print(pathError.Path)
}
```

### Swift: typed throws remains nominal

Swift typed throws writes one concrete error type in a function signature;
the proposal recommends an enum when several failures must be represented.
[Source](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0413-typed-throws.md)

```swift
enum LoadError: Error {
  case network(NetworkError)
  case parse(ParseError)
}

func load() throws(LoadError) -> Document
```

## Appendix C: Alternatives

| Alternative | Changes needed | Benefit | Why not recommend it |
| --- | --- | --- | --- |
| Keep named enums only | none | semantic names and stable APIs | local two-case wrappers remain repetitive |
| Standard `Either[A, B]` | core library addition (preferred cost class) | no parser work | cannot express flat arbitrary arity; exposes nesting and library name everywhere |
| Generate a named wrapper | compiler intrinsic or macro | stable nominal identity | generated names, navigation and diagnostics are worse than the declaration it replaces |
| Scala/Kotlin set union | syntax, semantic exceptions, checker mechanisms | implicit injection and set algebra | erases order and duplicates; new conversion and type-test rules |
| Ordered positional union | syntax plus reuse of enum machinery | exact local sum with explicit boundaries | still costs one type syntax and a compiler-known constructor namespace |

The radical simplification considered was to do nothing and retain named
enums. It has zero implementation cost and remains the fallback if the
measured wrapper burden does not justify the budget above. The decided ordered
design is cheaper semantically than a set union because it adds no subtyping,
injection or error-specific algebra.

## Appendix D: Today In hd

The specification admits [union syntax](../../spec/lang/04-type-system.md#union-type-syntax),
[ordered identity](../../spec/lang/04-type-system.md#union-type-identity), and
[positional construction and matching](../../spec/lang/04-type-system.md#union-construction-and-matching).
The checker lowers constructors and patterns through ordinary enum operations,
and unions reuse enum layout and active-payload ownership. Conditional `Eq`,
`Debug`, `Hash`, serde, `Display`, and `Error` implementations follow the
members as specified in [Union Trait Templates](../../spec/lang/09-traits.md#union-trait-templates).

`Union._0` works when an expected constructor result supplies its union type.
Without that context it reports `cannot-infer-type`. Contextual `._0` is not a
function value and reports `missing-contextual-enum-type`. Those diagnostics
remain deliberate constraints on inference.

## Appendix E: Language Checklist And Implementation Slices

| Domain | Built behavior or check |
| --- | --- |
| Grammar | `|` is allowed only in type grammar; it binds looser than type application; chains flatten unless parenthesized |
| Formatting | one space around `|`; parentheses retained when they preserve nesting |
| Typing | exact ordered identity; no assignability from a member; aliases expand normally |
| Generics | a type parameter may be a member; duplicates after substitution remain distinct positions |
| Construction | `._k` needs an expected union; `Union._k` is resolved from an expected constructor result |
| Matching | ordinary enum arms, positional names and exhaustiveness; no type-test arms |
| Closures | expected closure results can solve `Union._k`; otherwise inference fails normally |
| Ownership | copy, move, borrow and drop the active payload exactly as an enum does |
| Requirements | member requirements are unchanged; unions add no row operation |
| Lowering | reuse enum tag selection and payload storage; nested sums stay nested |
| Optimization | inlining and dead-code elimination see ordinary constructor and match operations |
| Soundness | no untagged reinterpretation, implicit coercion or post-monomorphization case selection |
| Diagnostics | reuse `cannot-infer-type`, `type-mismatch`, `unknown-variant` and `invalid-result-propagation` |
| Specification | no unsupported-union rule remains; syntax, identity, construction, matching, runtime and trait rules replace it |
| Conformance | flat, nested, duplicate, generic, exhaustive, trait and both-engine runtime fixtures |

Implementation was split into hour-sized, revertible slices:

1. Parsed, printed and stage-tested union type trees.
2. Added canonical ordered identity, alias expansion and diagnostic rendering.
3. Resolved positional constructors and patterns through ordinary enum checks.
4. Lowered flat and nested unions through enum layout, ownership and matches.
5. Added the decided conditional trait templates, including transparent Display
   and `Error.source` forwarding.
6. Added conformance, both-engine parity, and named-control measurements.

No slice exceeded its budget or changed the decided representation. Appendix F
records the final specification and implementation checks.

## Appendix F: Build Report

All six slices are built. No language rule describes union types as
unsupported, and no known-failure row is needed.

| Domain | Conformance evidence | Result |
| --- | --- | --- |
| Grammar and formatting | `parse/valid/union-type-syntax.hd` plus parser and formatter fixed-point stages | all five flat and parenthesized shapes preserved |
| Identity and generics | `typing/valid/union-type-identity-and-aliases.hd` and four negative identity cases | order, duplicates, nesting, aliases and substitution covered |
| Construction and matching | `typing/valid/union-construction-and-match.hd` plus four negative constructor or exhaustiveness cases | contextual cases and exhaustive matching covered |
| Runtime and ownership | `runtime/valid/union-runtime-layout-and-ownership.hd` | flat, nested, duplicate, generic and borrowed values pass five tests on each engine |
| Traits and errors | the forwarding, hash and serde runtime fixtures plus the missing-member negative case | five tests pass on each engine; a missing member implementation rejects |

The compiler-corpus comparison ran the same 3,422 parse and type cases with
four workers. Five cycles alternated the pre-union and completed compilers.
The named controls used 21 alternating cold release builds.

| Measure | Base or named control | Union feature | Delta | Bar |
| --- | ---: | ---: | ---: | --- |
| Production Rust | — | 601 net lines | — | 460-860: pass |
| Compiler corpus median | 3.51 s | 3.47 s | -1.1% | less than 1% increase: pass |
| Layout-control compile median | 179.5 ms | 182.2 ms | +1.5% | ranges overlap: no measurable regression |
| Trait-control compile median | 235.8 ms | 232.8 ms | -1.3% | no regression |
| Layout-control release Wasm | 3,707 B | 3,707 B | 0 B | byte-identical: pass |
| Trait-control release Wasm | 40,586 B | 39,582 B | -1,004 B | no increase: pass |
| Web compiler gzip | 1,918,876 B | 1,924,129 B | +5,253 B | at most 15 KB: pass |

The layout modules have the same stripped SHA-256,
`fab6fb88859c4353bf8def232d52f291a697bf6b5a8ac0d0678a1ab2a5a7a5cb`.
The trait control is smaller because its required debug case name differs from
the named enum, but it adds no code-size regression.

An audit found no std or example enum whose only purpose is combining two
error types. No wrapper conversion was therefore applicable.

## Parse Log

Each `text` block was copied exactly to a scratch `.hd` file and run through
`scripts/hd-conformance.sh parse` on 2026-10-31.

| Block | Result |
| --- | --- |
| proposed union alias and use | `1/1 clean` after the completed implementation |
| present-day generic `Union[A, B]` control | `1/1 clean` |
| present-day generic `Timed[E]` control | `1/1 clean` |
