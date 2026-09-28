# Literal Suffixes: Survey And Design Options

Status: design exploration, 2026-09-27; owner decisions L1-L18 (2026-09-28)
are below. L1-L9 were applied to the specification on 2026-09-28 (L5's
operator traits excepted, which are designed separately), and L12, L13,
L15-L17 and L18 the same day. L11 (`@suffix fn`) is not applied: the owner is
rethinking decorators, so the `LiteralSuffix` mechanism stays in the
specification for now. The survey and options before the decisions are the
exploration they came from. Questions raised while applying them are under
[Still Open](#still-open).

The owner asked for this while designing tests: `it("fetch", timeout=5s):`
instead of `timeout="5s"`, and the design should be general enough for
`12px` and `50GB`. This record surveys how other languages write unit
literals, gives four options, and ends with questions. It reviews
[Integer Literals](../spec/01-lexical-structure.md#integer-literals),
[Floating-Point Literals](../spec/01-lexical-structure.md#floating-point-literals),
[Literal Types](../spec/04-type-system.md#literal-types),
[Test Cases](../spec/10-modules.md#test-cases) and
[Testing T22](TESTING.md#owner-decisions).

## Owner Decisions

Decided 2026-09-28. Applied 2026-09-28 in
[Literal Suffixes](../spec/01-lexical-structure.md#literal-suffixes) (lexing),
[Literal Suffix Names](../spec/03-names-and-scopes.md#literal-suffix-names),
[Suffixed Literals](../spec/04-type-system.md#suffixed-literals) (typing),
[Literal Suffixes](../spec/05-expressions.md#literal-suffixes) (the call and
`std.time`), [Literal Suffix Trait](../spec/09-traits.md#literal-suffix-trait),
and the test [`timeout`](../spec/10-modules.md#r-module.testing.option.timeout-any-duration)
option. L5 is a direction only; no operator trait is specified. L12, L13 and
L15-L17 were applied on 2026-09-28 in the same sections, as
[`lex.suffix.no-radix`](../spec/01-lexical-structure.md#r-lex.suffix.no-radix),
[`lex.suffix.reserved`](../spec/01-lexical-structure.md#r-lex.suffix.reserved),
[`expr.suffix.exact-call`](../spec/05-expressions.md#r-expr.suffix.exact-call),
[`module.testing.option.timeout-at-run`](../spec/10-modules.md#r-module.testing.option.timeout-at-run)
and [`expr.suffix.std.duration`](../spec/05-expressions.md#r-expr.suffix.std.duration).
L10 and L14 confirm what was applied. L11 is not applied yet (see
[Still Open](#still-open)).

1. **L1: suffixes are imported library declarations** (option 3's
   resolution): `250ms` resolves the suffix `ms` through ordinary `use`
   (`use std.time.ms`), and libraries may declare their own (`12px`,
   `50Gb`).
2. **L2: a suffix is declared by implementing `std.ops.LiteralSuffix`** on
   a newtype whose name is the suffix, not by a keyword or a decorator.
   The newtype only holds the suffix's name; the literal's value has the
   trait's output type, so `5s` and `250ms` are both `Duration` and mix
   freely:

   ```text
   # std.ops
   pub trait LiteralSuffix[In, Out]:
       fn from_literal(n: In) -> Out

   # std.time
   pub type ms(i64)
   impl LiteralSuffix[i64, Duration] for ms:
       fn from_literal(n: i64) -> Duration: Duration.millis(n)

   # user code
   use std.time.{s, ms}
   delay := 250ms              # means ms::from_literal(250): Duration
   t := 1s.plus(250ms)
   ```

   Rejected on 2026-09-28: a literal whose type is the newtype itself
   (`-> Self`, units as distinct types), because `Duration` APIs would
   then need conversions; a zero-field `data ms()` carrier; the method
   names `apply` and `make`. Implementing the trait on the function's own
   item type waits on function item types (FN_TYPE questions 9 and 10); a
   `suffix fn` keyword was rejected in favour of the trait.
3. **L3: `In` is one numeric type.** `impl LiteralSuffix[i64, Duration]`
   makes `1.5s` a type error (write `1500ms`).
4. **L4: `-5s` means `s::from_literal(-5)`.** The `-` folds into the literal
   before the suffix applies, checked like `-128` for `i8`.
5. **L5: operator traits in `std.ops` are planned** (`Add`, `Sub`, `Neg`,
   `Mul`, and so on, Rust's model), so `5s + 3s` and `-d` can work on
   library types. They are designed separately
   ([Open Issues](OPEN_ISSUES.md#operator-traits)); `LiteralSuffix` is the
   first member of `std.ops`.
6. **L6: every numeric literal may carry a suffix.** Decimal integers and
   floats take it directly (`5s`, `1.5kb`); `1e3` stays an exponent. A
   radix literal (hex, binary, octal) takes a suffix only after a `'`
   separator, Nim's form (`0xff'B`, `0b1010'flags`), because `_` is already
   a digit separator and letters such as `B` are hex digits. (The owner
   chose "all numeric"; the `'` spelling is filled in here, open to owner
   correction.)
7. **L7: a suffixed literal is a plain call** to `from_literal`. It is evaluated at
   compile time only where the position already requires that (facts,
   shared enum data, test options), and there `from_literal` must need no
   providers and never suspend.
8. **L8: suffixes are imported normally,** with no special case for tests:
   `timeout=5s` needs `use std.time.s`.
9. **L9: std ships only duration suffixes at first,** `ns us ms s min h`
   for `Duration` (no `m`, no `d`). Byte sizes wait for a byte-size type.
   String suffixes are out of scope.
10. **L10 (2026-09-28): suffix lookup stays as decided in L8.** A suffix is
    an ordinary module name you import explicitly, as in
    `use std.time.{Duration, ms, s}`. The owner considered and rejected three
    alternatives: lookup on the expected type (like `.None`), expected type
    with an import fallback, and a separate suffix namespace (C++
    `chrono_literals`). The owner's reasons: clashes with other module names
    are not an important problem and can be renamed with `as`; the import
    should stay explicit; and resolving through a trait or the expected type
    is not wanted. Importing short names is a small cost the owner accepts.
11. **L11 (2026-09-28): back to option 3, `@suffix fn`. This replaces L2
    and L3.** A suffix is a function marked with the intrinsic decorator
    `@suffix`, such as `@suffix pub fn ms(count: i64) -> Duration`.
    `250ms` is the call `ms(250)`, and `-5s` is `s(-5)`.
    `std.ops.LiteralSuffix` and the newtype carriers are removed. The
    owner's reason: an impl-based suffix lets impls on one carrier return
    different types. `@suffix` is the one decorator allowed before a
    module-level function, an exception to M24 `annot.decorator.function`.
    A suffix function has exactly one parameter, of a primitive integer or
    float type, and no type parameters; it needs no providers and never
    suspends. There is no overloading, so `1.5s` is a type error when `s`
    takes `i64`. Lookup is unchanged from L8 and L10: the name is found at
    module scope, locals never take part, and it is imported explicitly.
12. **L12: only decimal and float literals take a suffix.** Radix literals
    take none, so the `'` form (`0xff'B`, `5'ms`) is gone.
    String, character and boolean literals never take a suffix. The owner
    confirmed this on 2026-09-28: the other languages' non-number uses are
    C++ `"abc"s` and `"abc"sv`, and prefix or tag forms such as Rust
    `b"..."`, Python `f"..."` and Scala `sql"..."`. None of them needs a
    suffix on an hd literal.
13. **L13: a reserved word straight after digits is a lexer error.**
    `5else` is `invalid-token`, not a suffix and not two tokens. (Revised
    the same day; the first answer was two tokens.)
14. **L14: kept as applied.** A suffixed literal in a `match` pattern is a
    `syntax-error` until constant patterns are designed. Diagnostics reuse
    existing codes, and the message text names the suffix; no
    `literal-suffix` code is added.
15. **L15: no special compile-time evaluation.** `5s` is exactly the call
    `s(5)` wherever it appears. This replaces the compile-time part of L7: a
    suffixed literal in a fact, shared enum data or other compile-time
    position follows the same rules as any other call there. A panic there
    behaves as it would for any other call; suffixes get no extra rule.
    (Revised the same day; the first answer was a compile error at the
    literal.)
16. **L16: `timeout=` takes any `Duration` value,** such as
    `Duration::seconds(5)` or `5s`, because the runner runs the test code
    anyway. This relaxes Testing T22's literal-only rule for `timeout`
    alone.
17. **L17: `Duration` is a whole number of milliseconds in an `i64`.**
    Nanosecond precision isn't needed for now. std ships only the suffixes
    `ms s min h`; `ns` and `us` are dropped from L9 until a finer
    representation exists.

18. **L18 (2026-09-28): answers to the apply-pass questions.**
    - `timeout=` is an ordinary named argument of type `Duration` in a
      plain call to `it` or `it_each`. It is evaluated when the call runs,
      under the ordinary provider and suspension rules. No timeout-specific
      evaluation rule exists. The runner only enforces the limit on the
      body. The applied rules `module.testing.option.timeout-at-run` and
      `timeout-import` already agree with this; they only restate the
      general argument and import rules.
    - A suffix call that overflows `i64` milliseconds panics at run time,
      like any checked arithmetic. A general compile-time lint for certain
      panics, similar to Rust's `unconditional_panic`, is nice to have but
      not required.
    - `Timestamp` switches to whole milliseconds, to match `Duration`.
    - The `std.time` API for `Duration` is the small set:
      `Duration::milliseconds(n)`, `Duration::seconds(n)` and
      `d.as_milliseconds()`. The nanosecond functions are dropped.
    - L13 stays reserved-words-only: `5true` and `5self` are
      `invalid-token`, and contextual words such as `as` can still be
      suffixes.

    Applied 2026-09-28 in
    [`expr.suffix.std.duration-api`](../spec/05-expressions.md#r-expr.suffix.std.duration-api)
    and [`expr.suffix.std.overflow`](../spec/05-expressions.md#r-expr.suffix.std.overflow),
    with the optional lint as a note. `Timestamp` is whole milliseconds
    (`unix_millis`) in the [STDLIB draft](STDLIB.md#stdtime); the spec
    names no `Timestamp`. The `timeout` and L13 points needed no change.
    The prototype already follows all of it.

19. **L19 (2026-09-28): string prefixes through `@str_prefix`.** Strings
    take user-defined prefixes; they never take suffixes (L12).
    - A prefix is a function marked `@str_prefix` (fact type
      `std.ops.StrPrefix`, with `@annotate(.Fn)`, as Decorators D9 does
      for `NumSuffix`). `name"..."` and `name"""..."""`, with no space
      before the quote, call `name(template)`. The prefix is found at
      module scope, as a number suffix is.
    - **Tagged parts:** the argument is a
      `std.ops.Template[T]` with `raw_parts: List[string]` (`n + 1`
      pieces) and `values: List[T]` (the `n` interpolated values). Each
      value converts to the `T` of the prefix function's parameter, like
      any argument. So `sql(t: Template[SqlParam])` type-checks what gets
      interpolated, and `Template[Display]` accepts any displayable value.
      The compiler never joins the parts.
    - **Raw text only,** as Scala's interpolators do: no escapes are
      processed in a prefixed string. std offers helpers such as
      joining with the values' `Display` text, and processing escapes, for
      prefixes that want them.
    - **`r` becomes an ordinary std prefix**
      (`@str_prefix pub fn r(t: Template[Display]) -> string`). The
      built-in raw string literal is removed, and every prefixed string is
      lexed the way raw strings are today, including `$name`
      interpolation.
    - Survey: Scala `StringContext` (raw parts, the interpolator decides),
      JavaScript tagged templates (both processed and raw parts), Python
      3.14 `t"..."` (a `Template`), and fixed prefixes in Rust, C# and
      Swift.
    - Left for the apply pass, each with a recommendation in Still Open:
      the exact interpolation forms inside a prefixed string (recommend
      the same as today's raw strings), the helper names, `b"..."` bytes
      (recommend waiting until a bytes type exists), and a prefix in a
      pattern (recommend `syntax-error`, as L14 does for suffixes).

## Contents

- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Lexing Rules Shared By Options 2-4](#lexing-rules-shared-by-options-2-4)
- [Option 1: No Suffixes, Constructor Calls](#option-1-no-suffixes-constructor-calls)
- [Option 2: A Fixed Standard Suffix Table](#option-2-a-fixed-standard-suffix-table)
- [Option 3: Imported Suffix Functions](#option-3-imported-suffix-functions)
- [Option 4: Suffixes Looked Up On The Expected Type](#option-4-suffixes-looked-up-on-the-expected-type)
- [Out Of Scope: Units Of Measure](#out-of-scope-units-of-measure)
- [String Suffixes](#string-suffixes)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Still Open](#still-open)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

Can a numeric literal carry a unit, as in `5s`, `250ms`, `12px`, or
`50GB`? If so, who declares a suffix, how does a use find it, and what does
the literal mean? The answer must also say whether a suffixed literal counts
as a literal in positions that accept only literals, such as test options.

## What hd Has Today

| Topic | Today | Rule |
| --- | --- | --- |
| `5s` | A number token, then an identifier token: a `syntax-error` | [Lexical Token Grammar](../spec/01-lexical-structure.md#lexical-token-grammar) |
| `1e3`, `1.5e-6` | Floating-point literals; the exponent needs a digit after `e` and an optional sign | [`lex.float.forms`](../spec/01-lexical-structure.md#r-lex.float.forms) |
| `0x1fs`, `0b1z` | A radix literal followed by a letter outside its radix is an error | [`lex.int.outside-radix`](../spec/01-lexical-structure.md#r-lex.int.outside-radix) |
| `5_000`, `5_` | A separator sits only between digits | [`lex.sep.placement`](../spec/01-lexical-structure.md#r-lex.sep.placement) |
| `-5` | `-` is an operator, not part of the literal | [`lex.int.sign`](../spec/01-lexical-structure.md#r-lex.int.sign) |
| `-128` as `i8` | Negation of a literal is range-checked as a unit | [`types.literal.negation`](../spec/04-type-system.md#r-types.literal.negation) |
| Literal type | From the expected type; else `i32` or `f64` | [`types.literal.int-default`](../spec/04-type-system.md#r-types.literal.int-default) |
| Operators on user types | None: no arithmetic operator overloading | [`expr.unsupported.overloading`](../spec/05-expressions.md#r-expr.unsupported.overloading) |
| Methods on `i32` | Only std declares inherent ones; any package may implement its own trait for `i32` | [`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std), [`trait.own.orphan`](../spec/09-traits.md#r-trait.own.orphan) |
| `5.seconds()` | Parses today: a method call on the literal `5` | [Member Access](../spec/05-expressions.md#member-access) |
| `.Queued` | Resolved on the expected enum type; an error without one | [`data.enum.shorthand`](../spec/08-data-and-enums.md#r-data.enum.shorthand) |
| Test options | String literals only, read statically by the runner (before L1-L9) | [`module.testing.it.options-strings`](../spec/10-modules.md#r-module.testing.it.options-strings) |
| Facts, shared enum data | Any requirement-free expression, evaluated once at compile time | [`annot.fact.eval`](../spec/14-annotations.md#r-annot.fact.eval), [`data.shared.compile-time`](../spec/08-data-and-enums.md#r-data.shared.compile-time) |
| `Duration` | Drafted as `data Duration: nanos: i64` with `Duration::seconds(5)` | [STDLIB `std.time`](STDLIB.md#stdtime) |

Two facts shape every option. Because hd has no operator overloading,
`5s + 3s` and `-(5s)` never work on a `Duration`; arithmetic is a method
such as `a.plus(b)`. Because facts already evaluate any requirement-free
call, `Duration::seconds(5)` is already valid in a fact or shared enum data.

## Use Cases

Every option is shown on the same five cases.

| # | Case | Written today |
| --- | --- | --- |
| U1 | A test timeout, read statically by the runner | `it("fetch", timeout="5s"):` |
| U2 | A duration in code | `$.use(Clock).sleep!(Duration::milliseconds(250))` |
| U3 | A byte size, where `GB` (bytes) and `Gb` (bits) differ | `ByteSize::gigabytes(50)` (no such type yet) |
| U4 | A user unit from a UI library, sometimes negative | `Px(12)`, `Px(-12)` |
| U5 | A compile-time position: shared enum data | `Fast -> Tier(limit=Duration::milliseconds(250))` |

## Survey

| Language | Unit literal | Who declares a suffix, and how a use finds it | Int or float source; range | Compile time | Source |
| --- | --- | --- | --- | --- | --- |
| C++ (11, 14) | `12_km`, `5s`, `250ms`, `"abc"s` | A literal operator `operator""_km`; found by ordinary lookup, so std's need `using namespace std::chrono_literals`. User suffixes must start with `_` | Separate operators for integer (`unsigned long long`) and floating (`long double`) sources, or a raw `const char*` form | Operators may be `constexpr`; `-5s` is unary minus on the result | [cppreference: user literals](https://en.cppreference.com/w/cpp/language/user_literal), [chrono literals](https://en.cppreference.com/w/cpp/chrono/operator%22%22s) |
| Nim | `0xff'u4`, `-4'big` | A proc named with a leading apostrophe, as in `` proc `'u4`(n: string) ``; the `'` is required for custom suffixes | The proc receives the literal's text as a string | A `const` or macro can evaluate it; the minus is part of the literal so `-128'i8` works | [Nim manual: custom numeric literals](https://nim-lang.org/docs/manual.html#lexical-analysis-custom-numeric-literals) |
| Rust | `5u8`, `1.0f32` only | Fixed set; any suffix lexes, so macros see it, but a literal expression accepts only the built-in ones | Suffix picks the primitive type | Constant | [Rust Reference: suffixes](https://doc.rust-lang.org/reference/tokens.html#suffixes) |
| Rust `time` crate | `5.seconds()`, `1.5.seconds()` | An extension trait `NumericalDuration` implemented for the numeric types; brought in with `use` | One impl per numeric type | Ordinary call | [docs.rs: NumericalDuration](https://docs.rs/time/latest/time/ext/trait.NumericalDuration.html) |
| Kotlin | `5.seconds`, `250.milliseconds` | Extension properties on `Int`, `Long`, `Double`; imported as `kotlin.time.Duration.Companion.seconds` | One property per numeric type | Ordinary call | [Kotlin: Duration](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/-duration/) |
| Scala | `5.seconds` | Implicit classes (Scala 2) or extension methods, imported from `scala.concurrent.duration` | `DurationInt`, `DurationLong`, `DurationDouble` | Ordinary call | [Scala: duration package](https://www.scala-lang.org/api/current/scala/concurrent/duration.html) |
| Swift | `.seconds(5)`, `.milliseconds(250)` | No suffixes; a static member resolved on the expected type (implicit member expression) | Generic over `BinaryInteger`, plus a `Double` form | Ordinary call | [Swift: Duration](https://developer.apple.com/documentation/swift/duration), [implicit member expression](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/expressions/#Implicit-Member-Expression) |
| Go | `5 * time.Second` | No suffixes; `Duration` is a named `int64` with typed constants | Untyped constants are exact, so `1.5 * time.Second` works | Constant expressions, overflow is a compile error | [Go: time.Duration](https://pkg.go.dev/time#Duration), [Go spec: constants](https://go.dev/ref/spec#Constant_expressions) |
| Zig, Odin | `5 * std.time.ns_per_s`, `5 * time.Second` | No suffixes; constants and a distinct integer type | Integer | `comptime` or constant | [Zig std.time](https://ziglang.org/documentation/master/std/#std.time), [Odin core:time](https://pkg.odin-lang.org/core/time/) |
| F# | `5.0<m>`, `9999I` | Units of measure are declared types checked at compile time and erased; custom numeric suffixes are limited to `Q R Z I N G` via `NumericLiteralX` modules | Float or integer | Checked at compile time | [F#: units of measure](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure), [F# spec 4.1 §6.3.1](https://fsharp.org/specs/language-spec/4.1/FSharpSpec-4.1-latest.pdf) |
| Julia | `2x`, `5u"s"`, `5s` with Unitful | Juxtaposition: a numeric literal before a name multiplies. `0x`, `e`, and `f` spellings win over it | Any | Runtime | [Julia: numeric literal coefficients](https://docs.julialang.org/en/v1/manual/integers-and-floating-point-numbers/#man-numeric-literal-coefficients), [Unitful.jl](https://painterqubits.github.io/Unitful.jl/stable/) |
| OCaml | `12z` | One letter `g`-`z` or `G`-`Z`: parsed, rejected by the type checker unless a ppx rewrites it | Int or float | ppx at compile time | [OCaml manual: extension syntax](https://ocaml.org/manual/5.2/extensionsyntax.html) |
| MoonBit | `1L`, `14U`, `1N` | Fixed set; otherwise the expected type picks the literal type | Suffix picks the type | Constant | [MoonBit fundamentals](https://docs.moonbitlang.com/en/latest/language/fundamentals.html) |
| CSS | `12px`, `5s`, `1em` | A fixed unit table; a number followed by an identifier is one dimension token | Any | n/a | [CSS Syntax 3: numeric token](https://www.w3.org/TR/css-syntax-3/#consume-numeric-token) |

### Takeaways

1. **Two mature languages have user suffixes.** C++ and Nim both treat
   `5s` as a call of an imported suffix function, found by name lookup. Both keep suffix names apart from ordinary names: C++ by
   `operator""`, Nim by the leading `'`.
2. **Most languages chose a method or constant instead.** Kotlin, Scala,
   and Rust's `time` crate write `5.seconds`. Go, Zig, and Odin write
   `5 * Second`, and Swift writes `.seconds(5)`. None of these needs lexer
   changes, but Go's form needs operator arithmetic, which hd lacks.
3. **Radix literals and exponents collide with suffixes.** Rust's reference
   warns that `0x01_f32` is the integer 7986, and Julia lists `0xff` and
   `1e10` as conflicts. Every language resolves them in favor of the
   number.
4. **Unit arithmetic is a separate feature.** Only F# checks dimensions, and
   it needs type-level unit algebra. Every suffix design above is a
   constructor call, not a unit system.

## Lexing Rules Shared By Options 2-4

Options 2, 3, and 4 share one lexical form. They differ only in how the
suffix is resolved. Today every form below is already an error, so none of
the options changes the meaning of accepted code.

```ebnf
suffixed_literal = ( decimal_integer_literal | float_literal ), suffix ;
suffix = LETTER, { identifier_continue } ;
```

| Input | Tokens | Why |
| --- | --- | --- |
| `5s`, `250ms`, `12px` | one suffixed literal | a letter directly after decimal digits |
| `1.5s` | float `1.5`, suffix `s` | a suffix may follow a float |
| `5_000ms` | integer `5000`, suffix `ms` | separators between digits, as today |
| `5_ms` | error `invalid-token` | a separator cannot end the digits; a suffix cannot start with `_` |
| `1e3` | float `1000.0` | `e` plus an optional sign plus a digit is an exponent, as today |
| `1e3ms` | float `1000.0`, suffix `ms` | the exponent is taken first |
| `5em`, `2eV` | integer, suffix `em` / `eV` | `e` not followed by a digit is not an exponent (the CSS rule) |
| `0x1fs`, `0xffB` | error `syntax-error` | radix literals take no suffix; `lex.int.outside-radix` stays |
| `5 s` | two tokens, error | a suffix must touch the digits |
| `5.s`, `5.seconds()` | member access | unchanged; `5.` never starts a float |
| `-5s` | operator `-`, suffixed literal `5s` | `lex.int.sign` stays; see the typing rule per option |

Radix literals are excluded because hex digits swallow suffixes: `0xffB`
would be `0xffb`. Allowing suffixes there needs a separator such as Nim's
`'`, and `'` already starts a `char` literal in hd.

## Option 1: No Suffixes, Constructor Calls

**Idea.** The radical simplification: add nothing. Values with units are
built by associated functions or newtype constructors. Test options stay
strings, and the compiler validates the `timeout` string's format.

```text
use std.time.{Clock, Duration}

pub type Px(i32)

enum Tier(limit: Duration):
    Fast -> Tier(limit=Duration::milliseconds(250))
    Slow -> Tier(limit=Duration::seconds(5))

fn pause!() -> void $ Clock:
    $.use(Clock).sleep!(Duration::milliseconds(250))

fn margin() -> Px:
    Px(-12)

tests:
    it("fetches the index", timeout="5s"):
        pass
```

A library that wants `12.px()` can already have it: it declares its own
trait and implements it for `i32`, which the orphan rule allows. A caller
brings the trait in with `use`, as with Rust's `NumericalDuration`.

```text
pub type Px(i32)

pub trait PxLiteral:
    fn px(self) -> Px

impl PxLiteral for i32:
    fn px(self) -> Px:
        Px(self)

fn margin() -> Px:
    12.px()
```

| Aspect | Effect |
| --- | --- |
| Rules added | One: the `timeout` string must match a duration format such as `5s`, `250ms`, or `1m30s` (Go's `ParseDuration` grammar is a model). |
| U1 | `timeout="5s"`; a typo is a compile error only if that rule is added |
| U2, U5 | `Duration::milliseconds(250)`: valid in facts and shared data today |
| U3 | `ByteSize::gigabytes(50)`; `GB` versus `Gb` is spelled out |
| U4 | `Px(12)` is one character longer than `12px`; `Px(-12)` has no sign question |
| Soundness | Nothing new |
| Cost | The format check only |

The trait form has one wrinkle: the receiver `5` defaults to `i32`, so a
trait implemented only for `i64` makes `5.seconds()` an error. A library
implements it for each integer type it wants.

## Option 2: A Fixed Standard Suffix Table

**Idea.** The lexing rules above, plus a closed table in the specification.
Each suffix has a fixed result type and meaning, as Rust's `u8` or
MoonBit's `L` do. No package can add a suffix.

| Suffix | Result | Meaning |
| --- | --- | --- |
| `ns`, `us`, `ms`, `s`, `min`, `h` | `std.time.Duration` | nanoseconds to hours |
| `B`, `KB`, `MB`, `GB`, `KiB`, `MiB`, `GiB` | a std byte-size type, if one is added | bytes; no bit units |

```text
use std.time.{Clock, Duration}

enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)  # hypothetical syntax
    Slow -> Tier(limit=5s)  # hypothetical syntax

fn pause!() -> void $ Clock:
    $.use(Clock).sleep!(250ms)  # hypothetical syntax

tests:
    it("fetches the index", timeout=5s):  # hypothetical syntax
        pass
```

| Aspect | Effect |
| --- | --- |
| Rules added | The token form; the table; a suffixed literal is a literal of the table's type; its value must fit, else a compile error |
| Resolution | None: a suffix is not a name, so `s := 1` and `5s` never interact and no `use` is needed |
| Int or float | Both allowed: `1.5s` is 1.5 seconds; a result that is not a whole number of nanoseconds is an error |
| `-5s` | Folded as one literal, like `types.literal.negation`, since `Duration` has no `-` operator |
| Constant | Yes, everywhere: it is a literal, so test options, facts, and shared data accept it |
| U4 | Not served: `Px(12)` as in option 1 |
| Tooling | Hover shows the table row; go-to opens `Duration` |

Soundness is simple: the compiler knows every suffix's meaning, so overflow
such as `10_000_000_000s` (beyond `i64` nanoseconds) is a compile error.
The cost is that std's `Duration` representation becomes part of the
language, as `string` is. Evolution: user suffixes can be added later only
if std's suffixes keep working without a `use`, or by a breaking change.

## Option 3: Imported Suffix Functions

**Idea.** The C++ and Nim model. A suffix is a function marked `@suffix`.
`250ms` means the call `ms(250)`, where `ms` is found at module scope,
declared in the module or brought in by `use`.

```text
# std.time
@suffix  # hypothetical syntax
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)

@suffix  # hypothetical syntax
pub fn s(count: i64) -> Duration:
    Duration::seconds(count)
```

```text
use std.time.{Clock, Duration, ms, s}
use dep.ui.units.{Px, px}

enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)  # hypothetical syntax
    Slow -> Tier(limit=5s)  # hypothetical syntax

fn pause!() -> void $ Clock:
    $.use(Clock).sleep!(250ms)  # hypothetical syntax

fn margin() -> Px:
    -12px  # hypothetical syntax

tests:
    it("fetches the index", timeout=5s):  # hypothetical syntax
        pass
```

Rules added:

1. `@suffix` is an intrinsic decorator, like `@error`, allowed before a
   module-level function.
2. A suffix function has exactly one parameter, of a primitive integer or
   float type, and no type parameters.
3. It must be requirement-free and must not suspend, checked from its
   signature as for [default values](../spec/07-functions.md#default-values).
4. `Nsuffix` is the call `suffix(N)`. The literal `N` gets the parameter's
   type as its expected type, so the existing range check applies.
5. `-Nsuffix` is the call `suffix(-N)`, range-checked as a unit.
6. Suffix lookup reads module scope only. A local binding named `s` never
   changes what `5s` calls.
7. A suffix that names no `@suffix` function in module scope is an error
   whose message suggests a `use`.

| Aspect | Effect |
| --- | --- |
| Clashes | `m` for minutes and `m` for meters are two module names: the second `use` is `names.use.no-collision`. Rename with `use ... as`, or call `meters(5)` |
| Case | `GB` and `Gb` are different names, as identifiers already are |
| Int or float | One parameter, no overloading: `1.5s` is a type error when `s` takes `i64`; write `1500ms` |
| Constant | A requirement-free call, so facts and shared data accept it today |
| Test option | `timeout` accepts an integer literal whose suffix resolves to a `std.time` suffix. The runner reads the digits and the suffix; it runs no code |
| Overflow | The literal is range-checked; overflow inside the function, such as `10_000_000_000s`, panics at run time unless the literal is in a compile-time position |
| Tooling | Hover shows `std.time.s(5) -> Duration`; go-to and rename follow the function |
| Wasm GC | An ordinary call; nothing new |

Soundness conditions: suffix functions are plain functions, so no new
typing rule is needed beyond rules 4 and 5. Rule 6 prevents action at a
distance from local names. Std's suffixes cannot be prelude names: that
would forbid every local named `s` or `h`
([`names.prelude.no-shadow`](../spec/03-names-and-scopes.md#r-names.prelude.no-shadow)).
So a test module writes `use std.time.s`.

## Option 4: Suffixes Looked Up On The Expected Type

**Idea.** Like `.Queued` and Swift's `.seconds(5)`: a suffix is resolved on
the expected type. The type's owner declares its suffixes as inherent
associated functions. No `use` is needed and suffixes never clash across
types.

```text
impl Duration:
    @suffix  # hypothetical syntax
    pub fn ms(count: i64) -> Duration:
        Duration::milliseconds(count)
```

```text
use std.time.{Clock, Duration}

enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)  # hypothetical syntax

fn pause!() -> void $ Clock:
    $.use(Clock).sleep!(250ms)  # hypothetical syntax

fn start() -> void:
    started := 5s  # hypothetical syntax, and an error in this option
    pass

tests:
    it("fetches the index", timeout=5s):  # hypothetical syntax
        pass
```

The `started := 5s` line is an error in this option: no expected type
exists, as with `status := .Queued`
([`data.match.no-inference`](../spec/08-data-and-enums.md#r-data.match.no-inference)).

| Aspect | Effect |
| --- | --- |
| Rules added | The token form; `@suffix` on inherent associated functions; lookup on the expected type, looking through `T?`; an error without an expected type |
| Clashes | None across types: `m` on `Duration` and `m` on `Length` coexist |
| Ownership | Only `Duration`'s package declares `Duration` suffixes, so no package can add `5d` to std's type (`trait.own.inherent`) |
| Generic calls | `assert_equal(elapsed, 5s)` works only if inference fixes `T` from `elapsed` first; order-dependent |
| Constant, test option | As option 3: requirement-free call; `timeout: Duration?` gives the expected type |
| Tooling | Hover needs the expected type; go-to works after type checking, not from names alone |

Soundness conditions: the lookup needs a known expected type, so the
failure cases are the same as for enum shorthand. The cost is that reading
`5m` needs the type context to know if it means minutes or meters.

## Out Of Scope: Units Of Measure

Every option makes `5s` a value of an ordinary type. None checks
dimensions, so `speed = 5m / 2s` is not typed as meters per second. hd has
no arithmetic operator overloading, so `5m / 2s` is not even an
expression. F#-style units would need type-level unit algebra and a new
kind; that is a separate feature and waits on operator design.

## String Suffixes

C++ allows `"abc"s`; Scala and JavaScript put a tag before the string
instead (`sql"..."`). None of the options adds string suffixes. `"abc"u`
stays a `syntax-error`, so a later design can add it without breaking code.
A string suffix mostly serves compile-time validation, such as a regex or a
date; that needs compile-time evaluation of user code in every position.

## Comparison

| | 1: No suffixes | 2: Fixed std table | 3: Imported functions | 4: Expected type |
| --- | --- | --- | --- | --- |
| U1 test timeout | `"5s"` string | `5s` | `5s` plus `use std.time.s` | `5s` |
| U2 duration | `Duration::milliseconds(250)` | `250ms` | `250ms` | `250ms`, when typed |
| U3 byte size | `ByteSize::gigabytes(50)` | `50GB` if std adds the type | `50GB` from a library | `50GB` |
| U4 user unit | `Px(12)`, `12.px()` | `Px(12)` | `12px`, `-12px` | `12px`, when typed |
| U5 compile time | yes | yes | yes | yes |
| `x := 5s` | n/a | `Duration` | `Duration` | error |
| Rules added | 0-1 | token, table, fold `-` | token, `@suffix`, 5 call rules | token, `@suffix`, expected-type lookup |
| Rules removed | none | none | none | none |
| Clash handling | n/a | none possible | existing `use` rules | none possible |
| Soundness risk | none | none | none beyond ordinary calls | order-dependent inference |
| Agent-writability | high: one spelling | high | high: the `use` says where `s` comes from | medium: meaning depends on context |
| Human readability | verbose | best for durations | good; imports explain it | good when the type is visible |
| Implementation cost | lowest | low | medium | medium-high |
| Evolution | 2, 3, or 4 later | 3 later breaks or keeps std special | 2's std set is a special case of it | hard to add imported suffixes later |

## Recommendation

**Recommendation:** option 3, imported suffix functions, with the shared
lexing rules; std ships only `Duration` suffixes at first.

- It is the model of C++ and Nim, the two mature languages with user
  suffixes. In Kotlin and Scala, too, an import brings the unit in.
- It adds no new resolution machinery: a suffix is a function found by
  existing `use` rules, and clashes use existing errors and `as`.
- It serves `12px` and `50GB` from libraries, which the owner asked for.
- Suffix functions are requirement-free, so `5s` already works in facts and
  shared enum data, and the runner reads `timeout=5s` without running code.

It gives up `1.5s` (no overloading, so write `1500ms`) and costs one `use`
line per module. The next best option is option 2, if the owner wants no
user suffixes yet. Its lexing is the same. Moving to option 3 later means
std suffixes either keep working without `use` or start needing one.

## Questions For The Owner

### 1. Which mechanism?

Effect: decides whether `12px` is possible and whether `5s` needs a `use`.
Options: (1) no suffixes; (2) a fixed std table; (3) imported `@suffix`
functions; (4) lookup on the expected type. **Recommended: 3.**

```text
use std.time.s

tests:
    it("fetch", timeout=5s):  # hypothetical syntax
        pass
```

### 2. Only decimal literals take a suffix?

Effect: `0xffB` would otherwise read as the hex number `0xffb`.
Options: (a) decimal integers and floats only; radix literals stay an error
when followed by a letter; (b) also radix literals, with a separator.
**Recommended: a.** The exponent keeps priority, so `1e3` stays a float and
`5em` is `5` with suffix `em`.

```text
fn size() -> i64:
    0x1f  # a radix literal takes no suffix
```

### 3. What does `-5s` mean?

Effect: `Duration` has no `-` operator, so without a rule `-12px` is an
error. Options: (a) fold: `-5s` is `s(-5)`, range-checked as a unit, like
`-128` for `i8`; (b) error: write `Px(-12)`. **Recommended: a.**

```text
fn margin() -> Px:
    -12px  # hypothetical syntax
```

### 4. Integer and float sources?

Effect: decides whether `1.5s` works. Options:

- (a) one parameter, so a suffix accepts only its parameter's literal kind;
- (b) a suffix may name two functions, one per kind;
- (c) the function takes the literal's text, as Nim does.

**Recommended: a.** `1.5s` is a type error; write `1500ms`.

```text
fn backoff() -> Duration:
    1500ms  # hypothetical syntax
```

### 5. Is a suffixed literal evaluated at compile time?

Effect: decides whether `10_000_000_000s` fails the build or panics at run
time. Options:

- (a) a plain call, evaluated at compile time only where the position
  already is: facts, shared data, and test options;
- (b) always evaluated at compile time, so a panic in the suffix function
  is a compile error, as with C++ `consteval`.

**Recommended: a.**

```text
enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)  # hypothetical syntax
```

### 6. Does `timeout=5s` need `use std.time.s`?

Effect: one import line in each test module. Options: (a) yes, like every
other suffix; (b) no, `it` knows std's duration suffixes. **Recommended:
a**; (b) adds a special case to the intrinsic.

```text
use std.time.s
```

### 7. Which suffixes does std ship?

Effect: fixes the spelling of durations and whether byte sizes exist.
Options:

- (a) `ns`, `us`, `ms`, `s`, `min`, `h` only, with no `m` and no `d`, since
  a day is not always 24 hours;
- (b) also byte sizes `B`, `KB`, `KiB`, `MB`, `MiB`, `GB`, `GiB`, with a new
  byte-size type.

**Recommended: a.** Byte sizes wait for a byte-size type in std.

```text
use std.time.{min, s}
```

### 8. String suffixes?

Effect: `"a+"re` or `"2024-01-01"date`. Options: (a) out of scope, still a
`syntax-error`; (b) add them with the same `@suffix` rule taking `string`.
**Recommended: a.**

```text
fn pattern() -> string:
    "a+"
```

## Still Open

These points came up while applying L1-L9 and then L12-L17 on 2026-09-28.
Each waits for the owner. The specification states the reading in the
Applied column, so each can change without breaking a decision.

The first pass's points 1-3 and 8-10 are answered: L12 removed the `'`
form (points 1 and 2), L13 made `5else` an `invalid-token` (point 3), L16
lets `timeout=` take any `Duration` (point 8), L15 makes a panic in a suffix
behave as in any other call (point 9), and L17 fixes `Duration` as whole
milliseconds (point 10). L14 kept points 4 and 5 as applied. Points 6 and 7
concern the `LiteralSuffix` mechanism, which L11 would replace. L18 answered
the second pass's points 4-9 below (applied 2026-09-28); only points 1-3
remain open.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | L11, `@suffix fn` in place of `LiteralSuffix` | Not applied: the owner is rethinking decorators. The spec keeps `std.ops.LiteralSuffix` and the newtype carriers ([Literal Suffix Trait](../spec/09-traits.md#literal-suffix-trait)), and `std.ops` stays | Apply L11 once the decorator design settles, with Typed Derivation M25's `@suffix` note. |
| 2 | One suffix type with two `LiteralSuffix` impls (first-pass point 6) | Chosen by the ordinary rule for instantiations of one generic trait ([`expr.suffix.instantiations`](../spec/05-expressions.md#r-expr.suffix.instantiations)) | Moot under L11, which has no overloading; keep until then. |
| 3 | A `LiteralSuffix` impl on a data type rather than a newtype (first-pass point 7) | No diagnostic | Moot under L11; keep until then. |
| 4 | When a `timeout` value is evaluated, relative to the body | "When the test case runs, in its program instance, as `it_each` rows are" ([`module.testing.option.timeout-at-run`](../spec/10-modules.md#r-module.testing.option.timeout-at-run)); the order against the body is unstated | Evaluate it before the body starts, outside the time limit. |
| 5 | Whether a `timeout` value may need providers or suspend | Not specified: it is an ordinary argument of `it`, whose row is `R` | Require it to be requirement-free and non-suspending, as a default value is. |
| 6 | A `Duration` suffix whose result overflows `i64` milliseconds, as in `10_000_000_000_000_000h` | Not specified; the prototype panics on its checked multiplication | A checked arithmetic panic, as for any `i64` overflow. |
| 7 | `std.time` constructors and accessors under L17 | STDLIB now drafts `Duration` with private `millis: i64`, `milliseconds`, `seconds` and `as_milliseconds`; `nanoseconds` and `as_nanoseconds` are dropped | Keep that set until `Duration` gets a finer representation. |
| 8 | `Timestamp` and `Instant` precision, now that `Duration` is milliseconds | Not changed: STDLIB drafts `Timestamp` with `unix_nanos: i64`, so `since` loses precision | Store `Timestamp` as milliseconds too, matching `Duration`. |
| 9 | Which words count as reserved for L13 | The reserved-word list of [Keywords And Reserved Words](../spec/01-lexical-structure.md#keywords-and-reserved-words), so `5true` and `5self` are `invalid-token`; contextual words such as `5as` stay suffixes | Keep: contextual words are ordinary names outside their positions. |

```text
use std.time.{Duration, h}

enum Budget(limit: Duration):
    Forever -> Budget(limit=10_000_000_000_000_000h)  # overflows i64 milliseconds
```

## Sources

- C++ user-defined literals: <https://en.cppreference.com/w/cpp/language/user_literal>
- C++ chrono literals: <https://en.cppreference.com/w/cpp/chrono/operator%22%22s>
- Nim custom numeric literals: <https://nim-lang.org/docs/manual.html#lexical-analysis-custom-numeric-literals>
- Rust literal suffixes: <https://doc.rust-lang.org/reference/tokens.html#suffixes>
- Rust `time` crate extension trait: <https://docs.rs/time/latest/time/ext/trait.NumericalDuration.html>
- Kotlin `Duration`: <https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/-duration/>
- Scala durations: <https://www.scala-lang.org/api/current/scala/concurrent/duration.html>
- Swift `Duration`: <https://developer.apple.com/documentation/swift/duration>
- Swift implicit member expressions: <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/expressions/#Implicit-Member-Expression>
- Go `time.Duration`: <https://pkg.go.dev/time#Duration>; constant expressions: <https://go.dev/ref/spec#Constant_expressions>
- Zig `std.time`: <https://ziglang.org/documentation/master/std/#std.time>
- Odin `core:time`: <https://pkg.odin-lang.org/core/time/>
- F# units of measure: <https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure>
- F# language specification 4.1, §6.3.1 (`NumericLiteralQ` to `NumericLiteralG`): <https://fsharp.org/specs/language-spec/4.1/FSharpSpec-4.1-latest.pdf>
- Julia numeric literal coefficients: <https://docs.julialang.org/en/v1/manual/integers-and-floating-point-numbers/#man-numeric-literal-coefficients>
- Unitful.jl: <https://painterqubits.github.io/Unitful.jl/stable/>
- OCaml extension-only literals: <https://ocaml.org/manual/5.2/extensionsyntax.html>
- MoonBit numeric literals: <https://docs.moonbitlang.com/en/latest/language/fundamentals.html>
- CSS numeric tokens: <https://www.w3.org/TR/css-syntax-3/#consume-numeric-token>

## Parse Log

Every `text` block was parsed with the reference parser on 2026-09-27.
Parsing checks syntax only; no block is claimed to type-check. The parser
stops at the first error, so each block with suffixed literals was parsed
twice. The second parse rewrites each marked `5s` to the call `s(5)`.
Every rewritten block parses except block 6.

`@suffix` lines parse at module level, since the grammar accepts a
decorator before a function. No chapter gives `@suffix` a meaning, so they
are marked. Inside an `impl`, the parser reports `decorator-not-top-level`
on the marked line.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Option 1 | parses |
| 2 | Option 1, trait form | parses |
| 3 | Option 2 | `syntax-error` at the first marked line (4); parses when rewritten |
| 4 | Option 3, std declarations | parses |
| 5 | Option 3, uses | `syntax-error` at the first marked line (5); parses when rewritten |
| 6 | Option 4, declaration | `decorator-not-top-level` at the marked line 2 |
| 7 | Option 4, uses | `syntax-error` at the first marked line (4); parses when rewritten |
| 8 | Question 1 | `syntax-error` at marked line 4; parses when rewritten |
| 9 | Question 2 | parses |
| 10 | Question 3 | `syntax-error` at marked line 2; parses when rewritten |
| 11 | Question 4 | `syntax-error` at marked line 2; parses when rewritten |
| 12 | Question 5 | `syntax-error` at marked line 2; parses when rewritten |
| 13 | Question 6 | parses |
| 14 | Question 7 | parses |
| 15 | Question 8 | parses |

Every block was parsed again on 2026-09-28, after the reference parser
learned suffixed literals. The table above numbers blocks as they stood on
2026-09-27; the L2 example and the Still Open example were added later, so
the file now has 16 blocks. All 16 parse except option 4's declaration,
now block 6, which still reports `decorator-not-top-level` at line 2.

After L12-L17 were applied on 2026-09-28, the reference parser rejects
`0xff'B` and `5else`. Every block was parsed again: all 16 parse except
block 6, as before. The rewritten Still Open example parses.
