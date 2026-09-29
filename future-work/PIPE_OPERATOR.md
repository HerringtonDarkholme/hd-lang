# Pipe Operator And Iterator `map`/`fold`: Survey And Design Options

Status: design exploration, 2026-09-29. Owner decisions PL3-PL13, as
amended by [Chaining Study CS2](CHAINING_STUDY.md#owner-decisions), are
applied (2026-09-29), and the specification is authoritative for them:
[Pipe Expressions](../spec/05-expressions.md#pipe-expressions) and
[Leading-Pipe Continuation](../spec/01-lexical-structure.md#leading-pipe-continuation).
PL1 and PL2 are superseded by CS7 and CS8 and were not applied: `map`
and `fold` are ordinary methods of the data type `Iterator[T]`
([Iterator Adapters](../spec/06-control-flow.md#iterator-adapters)). The rest of
the record is the survey behind the decisions.

The owner wants a pipe operator and expects it to be hard. The same record
settles how `map[U]` and `fold[A]` reach iterators, which
[STDLIB Still Open](STDLIB.md#still-open) 1 and its
[Owner Decisions](STDLIB.md#owner-decisions) send here. It reviews
[Precedence](../spec/05-expressions.md#precedence),
[Method Calls](../spec/05-expressions.md#method-calls),
[Propagation](../spec/05-expressions.md#propagation),
[Leading-Dot Continuation](../spec/01-lexical-structure.md#leading-dot-continuation),
[Parameters](../spec/07-functions.md#parameters),
[Trailing Callback Blocks](../spec/07-functions.md#trailing-callback-blocks),
[Dynamic Safety](../spec/09-traits.md#dynamic-safety),
[Method Lookup](../spec/03-names-and-scopes.md#method-lookup),
[Bang Calls And Driver Contexts](../spec/11-requirements-and-suspension.md#bang-calls-and-driver-contexts),
and [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters).

## Owner Decisions

Decided 2026-09-29.

1. **PL1: `map` and `fold` are static-only default methods on `Iterator`.**
   A default method whose own signature isn't dynamically dispatchable
   stays out of the dispatch table. A call through a trait value runs the
   default body. It is Rust's `where Self: Sized` without a marker, and
   `xs.map(f)` works.
2. **PL2: static-only methods can't be overridden.** Doing so is the new
   error `static-only-override`, so a call means the same thing directly
   and through a trait value.
3. **PL3: hd gets a Gleam-style pipe `|>`.** The value goes into the first
   argument by default, and a single top-level `_` argument of the step
   picks another slot (`title |> format("Title: {}", _)`). Only the step's
   own direct arguments count, so a `_` inside a nested call or closure
   isn't the pipe slot. Two `_` in one step is an error.
4. **PL4: bare names are allowed** (the owner reversed the parentheses
   recommendation): `x |> f` means `f(x)`, and `x |> fetch!` means
   `fetch!(x)`. A step written as a call (`x |> f(a)`) always means
   "insert the value" (`f(x, a)`), never `f(a)(x)`.
5. **PL5: no trailing blocks on pipe steps for now.** Use closures or
   methods; this can be added later.
6. **PL6: accepted as recommended:**
   - the piped value is evaluated before the step's callee;
   - `|>` sits between comparison and `|` in precedence;
   - a line may start with `|>` to continue the previous expression, like
     leading-dot lines.

7. **PL7 (2026-09-29): Hack-style, `_` required.** This supersedes the
   Gleam-style default of PL3 and the bare names of PL4, because the owner
   wants the value's slot always shown.
   - Every pipe step must contain exactly one `_`, which marks where the
     piped value goes. A step without `_`, or with two, is an error.
   - A step may be any expression, not just a call: `user |> render(_,
     theme)`, `order |> _.total`, `price |> _ * 1.2`,
     `x |> Point { x: _, y: 0 }`.
   - `_` may appear anywhere in the step except inside a nested closure
     (`fn(...): ...`) within it. There it is an error, because it would be
     unclear which value is meant, and the closure may run later.
   - Bare names (`x |> f`) are gone; write `x |> f(_)`.
   - PL5 (no trailing blocks) and PL6 (evaluation order, precedence,
     leading `|>` lines) still stand.

8. **PL8 (2026-09-29): two kinds of step.** This refines PL7, whose
   "`_` required" no longer holds.
   - **A step with `_`** substitutes the value there, under PL7's rules:
     exactly one `_`, any expression, not inside a nested closure.
     `user |> render(_, theme)` means `render(user, theme)`.
   - **A step without `_`** is an expression that evaluates to a function,
     which is then called with the value (F# style). `x |> trim(xxx)`
     means `trim(xxx)(x)`, and `x |> f` means `f(x)`. A step never inserts
     the value silently. The guide must say plainly that `x |> f(y)` means
     `f(y)(x)`, not `f(x, y)`. TC39's 2018 "smart pipeline" proposal made
     that form an error for exactly this confusion; hd gives it a
     definite meaning instead.
   - **An application step must not suspend.** If the function value is
     suspending, that is an error, because the call would suspend with no
     visible `!`. Write the `_` form instead: `x |> handler!(_)`. `?`
     likewise needs the `_` form: `x |> f(_)?`.

9. **PL9 (2026-09-29): pipe steps are single-line.** A step may not
   contain an indented block: no multi-line `match`, `if` or closure body.
   The error hints to bind a name first or extract a function. Leading
   `|>` continuation lines stay allowed. One-line closures
   (`fn(o): o.paid`) and inline `if ... else ...` are fine. Precedent:
   Elixir allows `|> case do ... end`, but its style tool, Credo,
   discourages it.

10. **PL10 (2026-09-29): no `_` lambda shorthand for now.** Closures stay
    `fn(v): v * 2`. The pipe already owns `_` in expression position.
    - Scala's bare `_ * 2` can't be parsed next to the pipe.
    - `it` is taken by the prelude test function.
    - `$0` collides with requirements and string interpolation.
    - `fn: _ * 2` would work but needs a "not inside a pipe step"
      exception.

    Revisit using evidence from `audit/hd-writing-log.md`: if cheap-model
    agents show demand, add `fn: _` then, which breaks no code.

11. **PL11 (2026-09-29): no partial application `f(_, a)` outside pipes.**
    The owner declined Gleam-style function capture. Outside a pipe step,
    `_` has no expression meaning, and closures (`fn(u): format_user(u,
    style)`) or method references (MR1) cover the need. Inside a pipe step,
    `_` stays the pipe slot (PL7). This settles the capture form that PL10
    left for the writing log.

12. **PL12 (2026-09-29): capture only in callback arguments.** This
    replaces PL11.
    - `f(_, a)` (one `_`, a direct argument of a call) creates a
      one-parameter function, and it is allowed **only as a direct argument
      of a function or method call whose parameter type is a function
      type**, as in `xs.iter().map(format_user(_, style))` or
      `filter(greater_than(_, 0))`.
    - It is not allowed anywhere else a function is merely expected: not in
      annotated `let` bindings, return values or fields. The parser always
      reads `f(_, a)` as a capture, and the checker requires a callback
      argument position, so the rule restricts where a capture may appear
      and never changes its meaning.
    - **No captures inside pipe steps:** there every `_` is the pipe slot
      (PL7). Without this rule a step such as `x |> apply(f(_, 1))` would be
      ambiguous (the slot, or a capture with no slot).
      `x |> map(_, format_user(_, style))` is an error: two slots.
    - Exactly one `_`. The callee must not suspend, and no `?` is allowed.
      A method capture (`user.greet(_, "hi")`) fixes the receiver at
      creation, as MR1 bound references do.

13. **PL13 (2026-09-29): no function placeholder at all.** The owner
    reversed PL12 the same day. `f(_, a)` capture doesn't exist anywhere,
    so PL11 stands again: outside a pipe step `_` has no expression
    meaning. Callbacks are closures (`fn(u): format_user(u, style)`) or
    method references (MR1). Inside a pipe step, `_` is only the pipe slot.

## Still Open

Points the apply pass met (2026-09-29). Each waits for the owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | Which pipe owns a `_` in a nested pipe, as in `x \|> f(_, y \|> g(_))`? | No rule: [`expr.pipe.slot.one`](../spec/05-expressions.md#r-expr.pipe.slot.one) counts the `_` of "the step", and no fixture nests pipes | The innermost step that contains the `_` owns it, so the outer step above has one `_`. |
| 2 | Is a leading-dot line before the first `\|>` of a chain part of the chain, as in `xs` then `.iter()` then `\|> f`? | Valid: [`lex.pipe.no-dot-line`](../spec/01-lexical-structure.md#r-lex.pipe.no-dot-line) rejects only a dot line after a `\|>` on its logical line | Keep. The CS2 hazard, `.name` attaching to a step, needs a step before the dot line. |
| 3 | A dotted bare step whose prefix is a value, `x \|> user.greet`, reads as `user.greet(x)`, a method call with receiver `user` | Read as that call by [`expr.pipe.bare.call`](../spec/05-expressions.md#r-expr.pipe.bare.call) | Keep; the alternative is to allow only module paths and variant constructors as dotted steps. |

## Contents

1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Use Cases](#use-cases)
4. [Survey](#survey)
5. [Why A Pipe Cannot Be A Library Function In hd](#why-a-pipe-cannot-be-a-library-function-in-hd)
6. [Option 1: No Pipe, Static-Only Default Methods](#option-1-no-pipe-static-only-default-methods)
7. [Option 2: First-Argument Pipe](#option-2-first-argument-pipe)
8. [Option 3: Placeholder Pipe](#option-3-placeholder-pipe)
9. [Option 4: UFCS Instead Of A Pipe](#option-4-ufcs-instead-of-a-pipe)
10. [Option 5: Free Functions Only](#option-5-free-functions-only)
11. [Hard Parts Of Any Pipe](#hard-parts-of-any-pipe)
12. [Considered And Set Aside](#considered-and-set-aside)
13. [Comparison](#comparison)
14. [Ranking By Design Cost Order](#ranking-by-design-cost-order)
15. [Recommendation](#recommendation)
16. [Questions For The Owner](#questions-for-the-owner)
17. [Sources](#sources)
18. [Parse Log](#parse-log)

## Problem

There are two questions, and they constrain each other.

1. **Pipe.** Should hd have an operator that feeds a value into a call, as
   `x |> f(y)` does in Elixir, F#, R, and Gleam? If so, which argument does
   the value fill, and how does it meet `!`, `?`, rows, trailing blocks,
   named arguments, and the operator table?
2. **`map` and `fold`.** STDLIB question 14 made iterator adapters default
   methods of the prelude `Iterator`, as in Rust. `map[U]` and `fold[A]`
   have method type parameters without an `AnyRef` bound. That makes
   `Iterator` not dynamically safe
   ([`trait.dyn.safe.anyref-type-param`](../spec/09-traits.md#r-trait.dyn.safe.anyref-type-param)),
   yet `iter()` returns the trait value `mut Iterator[T]`.

A pipe could answer question 2 by making `map` and `fold` free functions.
A method rule could answer question 2 with no pipe at all. So the options
below each say how they handle both.

Recorded decisions that bind this record:

- iterator adapters are default methods of the prelude `Iterator`, with no
  `IteratorExt` (STDLIB decision 14);
- lazy adapter callbacks have the empty row and capture their providers;
  the eager `fold` carries a row parameter,
  `fold[A, R](init: A, step: fn(A, T) -> A $ R) -> A $ R`
  ([STDLIB Owner Decisions](STDLIB.md#owner-decisions));
- the prelude does not grow (STDLIB decision 7);
- more methods on built-in types come from `std` inherent methods (STDLIB
  decision 8);
- no shorthand-argument closures
  ([`fn.closure.no-other-syntax`](../spec/07-functions.md#r-fn.closure.no-other-syntax));
- method values stay deferred
  ([Method Values](../spec/07-functions.md#method-values)).

## What hd Has Today

| Area | Today | Source |
| --- | --- | --- |
| `\|>` token | Not a token. `\|` then `>` never appear together in a valid program, since `>` cannot start an operand. A new `\|>` token changes no existing program | [Lexical grammar](../spec/01-lexical-structure.md#lexical-token-grammar), [Precedence](../spec/05-expressions.md#precedence) |
| `\|` and `\|\|` | Bitwise OR (overloadable through `BitOr`) and logical OR | [Operator Traits](../spec/05-expressions.md#operator-traits) |
| Multi-line chains | A line starting with `.name` continues the previous line; a line starting with a binary operator never does | [`lex.dot.continue`](../spec/01-lexical-structure.md#r-lex.dot.continue), [`lex.continue.no-other-operator`](../spec/01-lexical-structure.md#r-lex.continue.no-other-operator) |
| `_` | A distinct placeholder token: discards, wildcard patterns, and inferred slots such as `convert[_, User]` | [`lex.ident.placeholder`](../spec/01-lexical-structure.md#r-lex.ident.placeholder), [Explicit Type Arguments](../spec/07-functions.md#explicit-type-arguments) |
| `!` | A postfix call suffix: `fetch!(url)`. The plain call `fetch(url)` builds a cold suspension | [`req.suspend.cold`](../spec/11-requirements-and-suspension.md#r-req.suspend.cold) |
| `?` | Postfix, returns early from the nearest function | [Propagation](../spec/05-expressions.md#propagation) |
| Method calls | Receiver first, then arguments; the same as calling the method with the receiver first | [`fn.method.eval-order`](../spec/07-functions.md#r-fn.method.eval-order), [`fn.method.equivalence`](../spec/07-functions.md#r-fn.method.equivalence) |
| Extending types | Inherent methods only in the owning package; `std` owns built-ins. A local trait can extend a concrete foreign type, but never a trait value or a bare parameter | [Implementation Ownership](../spec/09-traits.md#implementation-ownership), [`trait.target.trait-value`](../spec/09-traits.md#r-trait.target.trait-value) |
| Iterator adapters | `filter`, `take`, `enumerate`, `collect` are default methods; `map` and `fold` are not specified | [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters) |
| Dynamic safety | Every method compiles to one copy; a row parameter is fine, a plain method type parameter is not | [Dynamic Safety](../spec/09-traits.md#dynamic-safety) |
| Trait values as bounds | `mut Iterator[T]` satisfies `S < mut Iterator[T]` | [`trait.dyn.bound`](../spec/09-traits.md#r-trait.dyn.bound) |
| Closures | `fn(x): ...`, parameter types from the expected type | [Closure Annotations](../spec/07-functions.md#closure-annotations) |
| Comprehensions | Eager `[for x in xs if p => e]`, no suspension | [Comprehensions](../spec/05-expressions.md#comprehensions) |

Where a pipe would help today: calling a free function in chain position.
Examples are `text.join(parts, "-")`, a user helper such as
`collapse_dashes(s)`, a trait-qualified call such as
`Display::to_string(x)`, and a helper from another package. A method chain
has to break into a temporary or nest inside-out there.

## Use Cases

Every option shows the same four cases:

| # | Case | What it stresses |
| --- | --- | --- |
| U1 | Iterator pipeline: `filter`, then `map`, then `fold` | the `map`/`fold` question, closures, rows |
| U2 | String chain: trim, lower, split, `text.join`, a user helper | free functions in chain position |
| U3 | Suspending HTTP chain with `!` and `?` | suspension marking, early return |
| U4 | Builder chain with one helper from another package | methods mixed with free functions |

The helpers the examples call. They parse; `Response`, `User`, and the
error types are left undeclared, since parsing checks syntax only:

```text
use std.text
use std.time.s

fn collapse_dashes(value: string) -> string:
    pass

fn user_url(id: i64) -> string:
    pass

fn fetch!(url: string) -> Result[Response, HttpError] $ Http:
    pass

fn decode_user(body: string) -> Result[User, DecodeError]:
    pass

fn with_auth(builder: mut ClientBuilder, token: string) -> mut ClientBuilder:
    pass
```

## Survey

| Language | Mechanism | Slot the value fills | Notes | Source |
| --- | --- | --- | --- | --- |
| F# | `\|>` is a library operator, `x \|> f` is `f x`; functions are curried | last, through currying | Syme credits `\|>` with left-to-right type inference: the value's type is known before the lambda is checked | [F# functions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/functions/#pipelines), [Syme, HOPL 2020](https://dl.acm.org/doi/10.1145/3386325) |
| OCaml | `Stdlib.(\|>)`, "reverse-application", since 4.01 | last, through currying | a compiler primitive, so no closure is built | [OCaml Stdlib](https://ocaml.org/manual/5.2/api/Stdlib.html) |
| Elm | `Basics.(\|>)`; the core library puts data last | last, through currying | no methods at all | [elm/core Basics](https://package.elm-lang.org/packages/elm/core/latest/Basics) |
| Elixir | `\|>` macro inserts the value as the first argument of the call on the right | first | anonymous functions go through `then/2` | [Kernel](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2) |
| JavaScript (TC39) | Hack-style `x \|> f(%)`: the right side is any expression with a topic token | placeholder | Stage 1 in 2017, Stage 2 on 2021-08-31, still Stage 2. F# style lost over memory-performance concerns from engine implementers and `await`. The topic token is still not final | [proposal README](https://github.com/tc39/proposal-pipeline-operator), [HISTORY.md](https://github.com/tc39/proposal-pipeline-operator/blob/main/HISTORY.md) |
| R | native `\|>` (4.1.0) is a parse-time rewrite, `x \|> f(y)` is `f(x, y)`; `\(x)` lambdas came with it. The `_` placeholder (4.2.0) must be a named argument | first, or `_` by name | the right side must be a call, and not a special form such as `+` or `if` | [pipeOp](https://stat.ethz.ch/R-manual/R-devel/library/base/html/pipeOp.html), [R NEWS](https://cran.r-project.org/doc/manuals/r-release/NEWS.html) |
| Gleam | `\|>` passes the value as the first argument of a call; a function capture `f(a, _)` picks another slot | first, or capture | Gleam has no methods, so the pipe is its only chaining | [Gleam tour: pipelines](https://tour.gleam.run/functions/pipelines/), [function captures](https://tour.gleam.run/functions/function-captures/) |
| Julia | `x \|> f` is `f(x)`; more arguments need an anonymous function in parentheses | only argument | `.\|>` broadcasts | [Julia manual](https://docs.julialang.org/en/v1/manual/functions/#Function-composition-and-piping) |
| Unison | `\|>` is an ordinary function, `a -> (a ->{e} b) ->{e} b` | only argument | the ability variable `{e}` carries effects through the pipe, like an hd row parameter | [Unison: function application operators](https://www.unison-lang.org/docs/fundamentals/values-and-functions/function-application-operators/) |
| Kotlin | extension functions, resolved statically; a member always wins | receiver | no pipe; `x.let { f(it) }` covers the rest | [Extensions](https://kotlinlang.org/docs/extensions.html), [Scope functions](https://kotlinlang.org/docs/scope-functions.html) |
| Swift | extensions, including protocol extensions; SE-0352 opens an existential `any P` when it is passed to a generic function, so generic code runs on it | receiver | no pipe | [Extensions](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/extensions/), [SE-0352](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0352-implicit-open-existentials.md) |
| C# | extension methods; LINQ's `Select` over the interface `IEnumerable<T>` is one, enabled by `using System.Linq` | receiver | no pipe | [Extension methods](https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/classes-and-structs/extension-methods) |
| Rust | method chaining; a `where Self: Sized` method is "explicitly non-dispatchable", and `Iterator::map` is one. `&mut I` and `Box<I>` implement `Iterator`, so `boxed.map(f)` still works | receiver | no pipe; the `tap` crate adds `.pipe(f)` | [Reference: dyn compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility), [`Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html), [`tap::Pipe`](https://docs.rs/tap/latest/tap/pipe/trait.Pipe.html) |
| Go | methods may not have type parameters, because interfaces could not dispatch them; iterator helpers are free functions over `iter.Seq` | n/a | no pipe, no UFCS | [Type parameters proposal: no parameterized methods](https://go.googlesource.com/proposal/+/refs/heads/master/design/43651-type-parameters.md#no-parameterized-methods), [`iter`](https://pkg.go.dev/iter) |
| D | UFCS: if no member fits, `x.f(a)` is tried as `f(x, a)`; members win; only module-level functions take part | first | | [D spec: UFCS](https://dlang.org/spec/function.html#pseudo-member), [D tour](https://tour.dlang.org/tour/en/gems/uniform-function-call-syntax-ufcs) |
| Nim | method call syntax: `obj.f(args)` is `f(obj, args)` | first | | [Nim manual](https://nim-lang.org/docs/manual.html#procedures-method-call-syntax) |
| Scala 3 | extension methods, found by simple name, in given instances, or in the implicit scope of the receiver's type; `scala.util.chaining` adds `.pipe(f)` | receiver | | [Extension methods](https://docs.scala-lang.org/scala3/reference/contextual/extension-methods.html), [`ChainingOps`](https://www.scala-lang.org/api/current/scala/util/ChainingOps.html) |

### Takeaways

1. **The slot follows the library's argument order.** Curried, data-last
   libraries (F#, OCaml, Elm) pipe into the last slot. Data-first
   libraries without currying (Elixir, R, Gleam) pipe into the first. hd
   has no currying, and its `std` free functions take the data first
   (`text.join(parts, separator)`).
2. **Languages with methods and extensions do not ship a pipe.** Kotlin,
   Swift, C#, Scala, and Rust chain with dots and offer a library `.pipe`
   or `let`. Pipes appear where there are no methods (Elixir, Gleam, Elm,
   R, Unison) or where methods cannot be extended (JavaScript).
3. **Generic methods on dynamic interfaces have four known answers.** Rust
   excludes them from the vtable, Swift opens the existential for generic
   code, C# uses extension methods, and Go forbids them. Only Go gives up the `xs.map(f)`
   spelling.
4. **Placeholder pipes stall on the placeholder.** TC39 picked Hack style
   in 2021 and still has not fixed the topic token. R limits `_` to a named
   argument to avoid ambiguity.

## Why A Pipe Cannot Be A Library Function In hd

F#, OCaml, and Unison define `|>` as an ordinary function. hd could write
one with a row parameter, as Unison does with an ability variable:

```text
pub fn pipe[A, B, R](value: A, step: fn(A) -> B $ R) -> B $ R:
    step(value)
```

It fails the use cases for three reasons:

- **`!` is syntax.** A suspending step needs `fn!`, so a second
  `pipe!` would be needed, and the caller writes a closure per step.
- **`?` returns from the nearest function.** Inside the step's closure, `?`
  returns from the closure, not from the enclosing function
  ([`expr.try.convert`](../spec/05-expressions.md#r-expr.try.convert)).
- **No method on every type.** hd has no blanket implementations
  ([`trait.target.no-blanket`](../spec/09-traits.md#r-trait.target.no-blanket)),
  so `x.pipe(f)` in the style of Rust's `tap` or Scala's `chaining` cannot
  be written in `std`.

So a pipe in hd is syntax or a compiler intrinsic, never a library function.

## Option 1: No Pipe, Static-Only Default Methods

**Idea.** Keep method chaining as the one chaining style. Make `map` and
`fold` default methods, and let a default method whose own signature breaks
the one-copy rule stay out of the dispatch table. This is Rust's
`where Self: Sized` on `Iterator::map`, applied without a marker.

Rules added (a semantic rule exception to
[`trait.dyn.safe.one-copy`](../spec/09-traits.md#r-trait.dyn.safe.one-copy)):

| Rule | Text |
| --- | --- |
| Static-only method | A default method whose own signature breaks the one-copy rule is **static-only**. It is left out of the dispatch table and does not make its trait not dynamically safe. |
| Call through a trait value | Calling a static-only method on a dynamic trait value runs the default body with `Self` as the trait value type, which satisfies the bound by [`trait.dyn.bound`](../spec/09-traits.md#r-trait.dyn.bound). |
| No override (question PL2) | An implementation that supplies a static-only method is an error. Error: `static-only-override`. |
| Required methods | A required method (no body) that breaks the rule still makes the trait not dynamically safe. |

The `std` declarations, with the row parameter the owner decided for
`fold`:

```text
pub trait Iterator[T]:
    fn next(mut self) -> T?

    fn map[U](mut self, transform: fn(T) -> U) -> mut Iterator[U]:
        pass

    fn fold[A, R](mut self, initial: A, step: fn(A, T) -> A $ R) -> A $ R:
        pass
```

U1, iterator pipeline:

```text
fn even_square_sum(values: List[i32]) -> i32:
    values.iter()
        .filter(fn(value): value % 2 == 0)
        .map(fn(value): value * value)
        .fold(0, fn(total, value): total + value)
```

U2, string chain. The free functions break the chain:

```text
fn slug(title: string) -> string:
    words := title.trim().lower().split(" ")
    collapse_dashes(text.join(words, "-"))
```

U3, HTTP chain. Methods and `?` already chain; the free functions nest:

```text
fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    response := fetch!(user_url(id))?
    .Ok(decode_user(response.body)?.name)
```

U4, builder chain. The package helper splits the chain in two:

```text
fn client(token: string) -> Client:
    builder := ClientBuilder::new()
        .base_url("https://api.example.com")
        .timeout(5s)
    with_auth(builder, token)
        .header("Accept", "application/json")
        .build()
```

Interactions:

- **Soundness.** The default body is checked once with `Self` bounded by
  the trait ([`trait.default.checked-once`](../spec/09-traits.md#r-trait.default.checked-once)).
  Instantiating it at the trait value type is already legal for any
  generic function bounded by the trait. No new type rule is needed.
- **Overrides.** With overrides allowed, `counter.fold(...)` and the same
  call through `mut Iterator[i32]` could run different bodies, as in Rust.
  Forbidding them makes the two calls identical. Rust uses overrides of
  `fold` for speed; hd loses that.
- **Rows.** `fold`'s row parameter is already dynamically safe; only `A`
  makes it static-only. `map`'s callback keeps the empty row.
- **Wasm GC.** A static-only method compiles per instantiation, like any
  generic function; the trait's table keeps only `next` and the one-copy
  adapters.
- **Explicit type arguments.** `it.map[string](f)` lists only `U`.

What stays hard: every free function still breaks a chain (U2, U4).

## Option 2: First-Argument Pipe

**Idea.** Add `x |> f(args)`, meaning `f(x, args)`: the value fills the
first positional parameter, as in Elixir, R, and Gleam. `map` and `fold`
can then be `std.iter` free functions, or stay methods under Option 1.

Changes: one token (`|>`), one precedence level, one continuation rule, and
these semantic rules:

| Rule | Text |
| --- | --- |
| Leading call | The right side of `\|>` is a postfix chain that contains an argument clause. The first argument clause in it is the **leading call**. |
| Insertion | `a \|> s` inserts the value of `a` as the first positional argument of the leading call. |
| Order | `a` is evaluated first, then `s` as usual. So `x \|> obj.m(y)` evaluates `x`, `obj`, `y`. |
| Call required | A right side without an argument clause, as in `x \|> f`, is an error with the fix-it `f()`. Error: `pipe-target-not-call`. |
| Postfix after | Suffixes after the leading call, including `?` and `.name`, apply to its result. |
| Continuation | A line starting with `\|>` continues the previous logical line under the conditions of [`lex.dot.continue`](../spec/01-lexical-structure.md#r-lex.dot.continue). |

U1, with `map` and `fold` as `std.iter` free functions:

```text
fn even_square_sum(values: List[i32]) -> i32:
    values.iter()
        .filter(fn(value): value % 2 == 0)
        |> iter.map(fn(value): value * value)  # hypothetical syntax
        |> iter.fold(0, fn(total, value): total + value)  # hypothetical syntax
```

The chain mixes `.filter` and `|> iter.map`, because `filter` is a decided
default method. Under Option 1, U1 stays all dots, and the pipe serves only
the other cases.

U2, string chain:

```text
fn slug(title: string) -> string:
    title.trim().lower().split(" ") |> text.join("-") |> collapse_dashes()  # hypothetical syntax
```

U3, HTTP chain. `!` stays on the callee and `?` follows the call it
propagates:

```text
fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url() |> fetch!()?.body |> decode_user()?.name  # hypothetical syntax
    .Ok(name)
```

U4, builder chain. The step's leading call is `with_auth(token)`, and the
`.header` and `.build` lines continue its result:

```text
fn client(token: string) -> Client:
    ClientBuilder::new()
        .base_url("https://api.example.com")
        .timeout(5s)
        |> with_auth(token)  # hypothetical syntax
        .header("Accept", "application/json")
        .build()
```

## Option 3: Placeholder Pipe

**Idea.** Hack style: the right side is any expression, and `_` marks where
the value goes. `x |> f(a, _)`, `x |> _ + 1`, and `x |> _.name` all work.
A narrower variant (R, Gleam) allows `_` only as a direct argument of the
leading call and defaults to the first slot without it.

Changes: the Option 2 token, precedence level, and continuation rule, plus:

| Rule | Text |
| --- | --- |
| Topic | In the right side of `\|>`, `_` in expression position names the piped value. |
| Required | A right side with no topic `_` is an error. Error: `pipe-missing-topic`. |
| Once | The piped value is evaluated once, before the right side, and every `_` reads that value. |
| No capture | A topic `_` inside a closure in the right side is an error, so a closure never captures a hidden value. |
| Other `_` | `_` keeps its meaning in patterns, `_ :=`, and type-argument slots. |

U1:

```text
fn even_square_sum(values: List[i32]) -> i32:
    values.iter()
        .filter(fn(value): value % 2 == 0)
        |> iter.map(_, fn(value): value * value)  # hypothetical syntax
        |> iter.fold(_, 0, fn(total, value): total + value)  # hypothetical syntax
```

U2:

```text
fn slug(title: string) -> string:
    title.trim().lower().split(" ") |> text.join(_, "-") |> collapse_dashes(_)  # hypothetical syntax
```

U3:

```text
fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url(_) |> fetch!(_)?.body |> decode_user(_)?.name  # hypothetical syntax
    .Ok(name)
```

U4:

```text
fn client(token: string) -> Client:
    ClientBuilder::new()
        .base_url("https://api.example.com")
        .timeout(5s)
        |> with_auth(_, token)  # hypothetical syntax
        .header("Accept", "application/json")
        .build()
```

The `_` meanings collide in one step. Here the first `_` is an inferred
type argument, the second the piped value, and the third a wildcard
pattern:

```text
fn load(payload: Json, code: i32) -> string:
    user := payload |> convert[_, User](_)  # hypothetical syntax
    code |> match _:  # hypothetical syntax
        200 => "ok"
        _ => "error"
```

## Option 4: UFCS Instead Of A Pipe

**Idea.** No new syntax. When method lookup finds nothing, `x.f(args)`
calls a visible free function `f(x, args)`, as in D and Nim. `map` and
`fold` become `std.iter` free functions reached with dots.

Rules added (a change to [Method Lookup](../spec/03-names-and-scopes.md#method-lookup)
and [`expr.member.method-call`](../spec/05-expressions.md#r-expr.member.method-call)):

| Rule | Text |
| --- | --- |
| Fallback | When method lookup finds no inherent method and no candidate, `x.f(args)` calls the free function `f` as `f(x, args)`. |
| Which functions | Top-level functions visible under their simple name: declared in the module or imported by `use`. Local functions and closures bound to names do not take part, as in D. |
| Home module (question PL1, C) | Also the `pub` functions of the module that declares the receiver's type or trait, as Scala 3 searches the receiver's implicit scope. Then `xs.iter().map(f)` needs no `use`. |
| Methods win | A method always wins. A free function it hides gets a warning at the call. Warning: `ufcs-shadowed`. |
| Type arguments | `x.f[Ts](args)` lists the free function's full parameter list, receiver's parameter included: `it.map[i32, string](f)`. |

U1 (with `use std.iter.{map, fold}` unless the home-module rule applies):

```text
use std.iter.{map, fold}

fn even_square_sum(values: List[i32]) -> i32:
    values.iter()
        .filter(fn(value): value % 2 == 0)
        .map(fn(value): value * value)
        .fold(0, fn(total, value): total + value)
```

U2:

```text
use std.text.join

fn slug(title: string) -> string:
    title.trim().lower().split(" ").join("-").collapse_dashes()
```

U3. UFCS invites `id.user_url()`, which reads as if `user_url` were a
method of `i64`; the natural form keeps one call nested:

```text
fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    .Ok(fetch!(user_url(id))?.body.decode_user()?.name)
```

U4:

```text
fn client(token: string) -> Client:
    ClientBuilder::new()
        .base_url("https://api.example.com")
        .timeout(5s)
        .with_auth(token)
        .header("Accept", "application/json")
        .build()
```

Every block parses today: UFCS changes name resolution, not syntax.

Interactions:

- **Action at a distance.** A dependency that adds an inherent `join` to
  its type silently redirects `x.join(...)` from the user's free function.
  hd's [Inherent Methods Win](../spec/09-traits.md#inherent-methods-win)
  is safe because only the owner adds methods. UFCS makes a `use` line
  change what a dot call reaches.
- **Reading.** `x.f()` no longer tells the reader whether `f` belongs to
  the type. Tools can answer; plain text cannot.
- **Diagnostics.** `unknown-method` must also report the free functions
  it tried, and a first-parameter mismatch reads as a receiver error.
- **`pack.map(`.** The token sequence `pack.map(` always forms the pack
  operation ([`lex.contextual.pack`](../spec/01-lexical-structure.md#r-lex.contextual.pack)),
  so a value named `pack` cannot reach a free `map` by UFCS.
- **`map` and `fold`.** Free functions are statically dispatched and never
  overridden, so dynamic safety is untouched, as with Swift protocol
  extensions and C# LINQ.

## Option 5: Free Functions Only

**Radical simplification.** No pipe, no UFCS, and no rule exception. `map`
and `fold` are `std.iter` free functions; `filter`, `take`, `enumerate`,
and `collect` stay default methods. Eager map-and-filter already has
comprehensions. The only change is a core library addition.

```text
pub fn map[T, U](source: mut Iterator[T], transform: fn(T) -> U) -> mut Iterator[U]:
    pass

pub fn fold[T, A, R](source: mut Iterator[T], initial: A, step: fn(A, T) -> A $ R) -> A $ R:
    pass
```

U1, with temporaries, or with a comprehension and a loop:

```text
fn even_square_sum(values: List[i32]) -> i32:
    evens := values.iter().filter(fn(value): value % 2 == 0)
    squares := iter.map(evens, fn(value): value * value)
    iter.fold(squares, 0, fn(total, value): total + value)

fn even_square_sum_eager(values: List[i32]) -> i32:
    let total = 0
    for square in [for value in values if value % 2 == 0 => value * value]:
        total += square
    total
```

U2, U3, and U4 are the Option 1 blocks unchanged.

This is Go's answer. It gives up the chain for the most common pipeline,
which STDLIB decision 14 chose against. It also leaves `map` and `fold`
in a different call style from their sibling adapters.

## Hard Parts Of Any Pipe

These apply to Options 2 and 3. Each row gives the Option 2 reading and
where Option 3 differs.

### Which argument

hd `std` takes data first, so first-slot insertion fits most calls. It
misfits a function whose data is not first, such as a callback-first
helper. Option 3 reaches any slot; Option 2 needs a closure or a
temporary.

```text
fn retry_all(jobs: List[Job]) -> void:
    jobs |> each(fn(job): retry(3, job))  # hypothetical syntax
    jobs |> retry_each(3, _)  # hypothetical syntax
```

The second line is Option 3 only.

### `!` suspension marking

`!` stays on the callee's name, as in every hd call. `url |> fetch!()` is
`fetch!(url)`. Without the `!`, `url |> fetch()` is `fetch(url)`, a cold
suspension of type `mut Suspend[...]`, exactly as today. The bare
`url |> fetch!` has no argument clause; it is `pipe-target-not-call` with
the fix-it `fetch!()`.

```text
fn page!(url: string) -> Result[Response, HttpError] $ Http:
    url |> fetch!()  # hypothetical syntax
```

A pipe step inside a lazy adapter callback cannot suspend, since the
callback has the empty row and is not a suspending body
([`flow.adapter.callback-row`](../spec/06-control-flow.md#r-flow.adapter.callback-row)).

### `?` mid-pipeline

`?` is postfix, so it follows the leading call and returns from the
enclosing function, as it does in a method chain. `a |> f()? |> g()` is
`g(f(a)?)`. Option 2 needs the "Postfix after" rule for this. In Option 3
the step is an ordinary expression, so `f(_)?` needs no rule.

### Requirement rows

A pipe is a rewrite of a call, so rows flow as in the rewritten call. A
step that calls `fold` with a logging callback gives `fold`'s `R` the
`Log` key, and the enclosing function lists it:

```text
fn logged_total(values: List[i32]) -> i32 $ Log:
    values.iter() |> iter.fold(0, fn(total, value): total + noted(value))  # hypothetical syntax
```

No row rule is added. Unison needs the ability variable in `|>`'s type
only because its pipe is a function.

### Trailing blocks and closures

A closure argument works inside the step's parentheses, as in U1. A
trailing block raises a question: `job |> retry(3):` would be
`retry(job, 3):` with the block. Today a trailing-block call must be a
complete statement or the whole right side of a binding
([`fn.trailing.position`](../spec/07-functions.md#r-fn.trailing.position)).
The simplest rule forbids a trailing block on a pipe step; question PL9.

```text
fn save(job: Job) -> void:
    job |> retry(3):  # hypothetical syntax
        store(job)
```

A multiline closure inside a step follows
[Multiple Inline Closures](../spec/07-functions.md#multiple-inline-closures):
the closing `)` sits on its own line, and the next line may start with
`|>` under the continuation rule.

### Named arguments, defaults, and varargs

- The piped value is positional. `host |> connect(port=8080)` is
  `connect(host, port=8080)`.
- `host |> connect(host="x")` supplies `host` twice:
  `duplicate-argument`, and the message should say the pipe filled it.
- Defaults behave as in the rewritten call.
- For a vararg first parameter, `1 |> sum(2, 3)` is `sum(1, 2, 3)`.
  `values |> sum()` passes the list as one element, a `type-mismatch`.
  Option 3 writes `values |> sum(_...)`.
- Option 3 can fill a named slot: `host |> connect(host=_, port=80)`,
  as R 4.2 requires.

### Generic inference order

The piped value is evaluated and typed first, so a closure in the step
gets its parameter types from it, as with a method receiver. A free
function's explicit list includes the source's parameter:
`it |> iter.map[i32, string](f)`, or `iter.map[_, string]`. The method
form of Option 1 writes `it.map[string](f)`. A last-slot pipe would place
the value after the closure in argument order, so the checker would have
to type the value first anyway; F# relies on this.

### Precedence

Three placements, with what each does to the same lines:

| Expression | A: between comparison and `\|` (Elixir) | B: lowest, above `:=` (JavaScript) | C: postfix level, like `.` |
| --- | --- | --- | --- |
| `a + b \|> f()` | `f(a + b)` | `f(a + b)` | `a + f(b)` |
| `x \| mask \|> f()` | `f(x \| mask)` | `f(x \| mask)` | `x \| f(mask)` |
| `n \|> f() == 3` | `f(n) == 3` | `n \|> (f() == 3)`: `pipe-target-not-call` | `f(n) == 3` |
| `ok \|\| x \|> f()` | `ok \|\| f(x)` | `f(ok \|\| x)` | `ok \|\| f(x)` |

The operator traits never change precedence
([`expr.op.fixed`](../spec/05-expressions.md#r-expr.op.fixed)), so a
`BitOr` implementation does not affect the choice.

### Method calls on the right

- `x |> obj.m(y)` is `obj.m(x, y)`: the value fills the first argument
  after the receiver. It is never `x.m(y)`.
- `x |> iter.map(f)` is `iter.map(x, f)`: a module-qualified call reads
  the same way.
- `x |> Display::to_string()` is `Display::to_string(x)`: the receiver is
  the first argument of a trait-qualified call
  ([`trait.qualified.receiver`](../spec/09-traits.md#r-trait.qualified.receiver)).
- `x |> user.name.trim()` is `user.name.trim(x)`, an arity error. The
  message should show the rewritten call.
- `x |> (handler.callback)()` calls a stored function with `x`.

```text
fn label(value: Money) -> string:
    value |> Display::to_string()  # hypothetical syntax
```

### Tooling, formatting, and errors

- **Formatter.** One step per line once a pipe wraps, with `|>` leading,
  as leading-dot chains do today.
- **Continuation.** Without the leading-`|>` rule, a multi-line pipe needs
  parentheses, since a line starting with a binary operator never
  continues ([`lex.continue.no-other-operator`](../spec/01-lexical-structure.md#r-lex.continue.no-other-operator)).
  The parenthesized form parses today up to the first `|>`:

```text
fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := (id
        |> user_url()  # hypothetical syntax
        |> fetch!()?.body  # hypothetical syntax
        |> decode_user()?.name)  # hypothetical syntax
    .Ok(name)
```

- **Errors.** Every argument-count and type error in a step names the
  piped slot, as in "argument 1 (the piped value)". Hover shows the
  rewritten call.
- **Agents.** Coding agents know Elixir and F# pipes. The likely mistakes
  are `x |> f` without parentheses and last-slot habits; both need precise
  diagnostics. Option 3 adds `_` misuse.

## Considered And Set Aside

| Idea | Why set aside |
| --- | --- |
| Last-argument pipe (F#, Elm) | hd has no currying, and `std` puts data first, so the last slot is rarely the data. |
| A `pipe` compiler intrinsic method on every value | Cheaper than syntax (level 3), but each step needs a closure, `?` inside it returns from the closure, and a suspending step needs a second `pipe!`. |
| A library pipe through `BitOr` | Needs `impl[T] BitOr[fn(T) -> U] for T`, a bare-parameter target ([`trait.target.bare-parameter`](../spec/09-traits.md#r-trait.target.bare-parameter)). |
| Owner-only `impl Iterator[T]:` blocks on a trait (Swift protocol extensions) | The same mechanism as Option 1 with overrides forbidden, written outside the trait. It needs a new inherent target kind. |
| Inherent methods on the trait value type `Iterator[T]` (STDLIB 14C) | A concrete iterator such as `Counter` would not get `map` without first converting to the trait value. |
| Gleam's fallback (call the step's result when first-slot insertion fails) | Makes the meaning depend on types; hd prefers one reading per spelling. |

## Comparison

| | 1: static-only methods | 2: first-argument pipe | 3: placeholder pipe | 4: UFCS | 5: free functions only |
| --- | --- | --- | --- | --- | --- |
| U1 iterator | all dots | dots, then `\|>` (or all dots with Option 1) | dots, then `\|> f(_, ...)` | all dots | temporaries or a loop |
| U2 strings | nesting and a temporary | one line | one line with `_` | one dot chain | nesting and a temporary |
| U3 `!` and `?` | methods chain; free functions nest | one line; `!` and `?` as today | one line | mostly chained | as Option 1 |
| U4 builder | chain split in two | one chain | one chain | one chain | as Option 1 |
| Syntax added | none | `\|>` token, precedence level, leading-`\|>` continuation | the same, plus `_` in expressions | none | none |
| Semantic rules | static-only exception, no override | leading call, insertion, order, call required, postfix after | topic, required, once, no capture | lookup fallback, visible set, home module, shadow warning, type arguments | none |
| New diagnostics | `static-only-override` | `pipe-target-not-call` | `pipe-missing-topic`, topic in closure | `ufcs-shadowed` | none |
| Soundness risk | low: reuses `trait.dyn.bound` | none: a rewrite | none: a rewrite | none; resolution changes with imports | none |
| No action at a distance | kept | kept | kept | weakened: a `use` or a dependency method changes `x.f()` | kept |
| Agent-writability | high: Rust habits | high: Elixir habits; `x \|> f` mistakes | medium: `_` has four meanings | high to write, harder to review | high, verbose |
| Human readability | good for methods only | good | good; `_` dense lines | good; origin of `f` hidden | inside-out |
| Implementation cost | small checker change | parser, lexer, formatter, diagnostics | the same, plus topic scoping | resolver, completion, diagnostics | library only |
| Evolution | pipe can still be added | `_` placement can be added later | hard to narrow | hard to remove | any of 1-4 can be added later |

## Ranking By Design Cost Order

[AGENTS.md](../AGENTS.md#design-cost-order) ranks by the costliest kind of
change, then by the number of changes.

| Rank | Option | Costliest change | Changes |
| --- | --- | --- | --- |
| 1 | 5: free functions only | core library addition (4) | 1: `std.iter.map`, `std.iter.fold` |
| 2 | 1: static-only default methods | semantic rule exception (2) | 2 rules (static-only, no override) plus the two default methods |
| 3 | 4: UFCS | semantic rule exception (2) | 5 rules plus two free functions |
| 4 | 2: first-argument pipe | syntax change (1) | 1 token, 1 precedence level, 1 continuation rule, 5 rules |
| 5 | 3: placeholder pipe | syntax change (1) | Option 2's syntax plus `_` as an expression, 4 rules |

The two questions separate. For `map` and `fold`, the order is 5, 1, 4.
For the pipe, it is none, then 2, then 3. Option 1 and Option 2 combine.
Then iterators chain with dots, and the pipe serves free functions only.

## Recommendation

**Recommendation.**

1. **`map` and `fold`: Option 1**, static-only default methods with
   overrides forbidden. Why:
   - it keeps STDLIB decision 14 whole: every adapter is a method, and U1
     chains with dots;
   - Rust, Swift, and C# all keep `xs.map(f)` on a dynamic interface;
     hd's `trait.dyn.bound` already makes the default body valid at the
     trait value type;
   - with overrides forbidden, the exclusion is invisible except in
     speed, so no marker is needed.

   It gives up Rust-style fast overrides of `fold`. The next best is
   Option 5, which is cheaper but gives up the chain decision 14 chose.
2. **Pipe: Option 2**, since the owner wants one. Required parentheses,
   insertion into the leading call, the piped value evaluated first,
   precedence placement A, a leading-`|>` continuation rule, and no
   trailing block on a step. Why:
   - first-slot insertion matches `std`'s data-first order and three
     shipped languages (Elixir, R, Gleam);
   - `!` and `?` keep their existing postfix meaning, and rows need no rule;
   - `_` placement is a compatible extension later, since `_` in a step is
     invalid under Option 2.

   It gives up arbitrary right sides (`x |> _ + 1`) and non-first slots.
   If the Design Cost Order decides alone, the answer is no pipe: Option 1
   covers iterators, and free functions nest.
3. **Not Option 4.** UFCS avoids new syntax, but a `use` line or a
   dependency's new method can change what `x.f()` reaches. That is the
   action at a distance the owner principles rule out.

## Questions For The Owner

### PL1. How do `map` and `fold` reach iterators?

Effect: `values.iter().map(f)` is not valid today, because a generic
default method would make `mut Iterator[T]` unusable as a value type.

- **A.** Static-only default methods (Option 1).
- **B.** `std.iter` free functions (Option 5), reached by plain calls or by
  a pipe.
- **C.** `std.iter` free functions reached with dots by UFCS (Option 4).

**Recommendation:** A.

```text
fn squares(values: List[i32]) -> List[i32]:
    values.iter().map(fn(value): value * value).collect()
```

### PL2. May an implementation override a static-only method?

Effect: with overrides, `counter.fold(...)` and the same call through
`mut Iterator[i32]` can run different bodies.

- **A.** No: `static-only-override`, one behavior everywhere.
- **B.** Yes, as in Rust; a call through a trait value always runs the
  default.

**Recommendation:** A.

```text
data Counter:
    current: i32

impl Iterator[i32] for Counter:
    fn next(mut self) -> i32?:
        .None

    fn fold[A, R](mut self, initial: A, step: fn(A, i32) -> A $ R) -> A $ R:
        initial
```

Under A, the second method is `static-only-override`.

### PL3. Does hd get a pipe operator?

Effect: free functions such as `text.join` and package helpers break a
method chain into temporaries or nested calls.

- **A.** No pipe; method chains only.
- **B.** A first-argument pipe (Option 2).
- **C.** A placeholder pipe (Option 3).

**Recommendation:** B.

```text
fn slug(title: string) -> string:
    title.trim().lower().split(" ") |> text.join("-") |> collapse_dashes()  # hypothetical syntax
```

### PL4. May a step choose its slot with `_`?

Effect: a function whose data parameter is not first needs a closure or a
temporary under a first-only pipe.

- **A.** First slot only; `_` in a step is an error for now.
- **B.** First slot by default, `_` as a direct argument picks another
  (Gleam, R).

**Recommendation:** A. B can be added later without changing valid code.

```text
fn run(jobs: List[Job]) -> void:
    jobs |> retry_each(3, _)  # hypothetical syntax
```

### PL5. Where does `|>` sit in the precedence table?

Effect: `a + b |> f()` and `n |> f() == 3` read differently under each
placement (see [Precedence](#precedence)).

- **A.** Between comparison and bitwise OR, as in Elixir.
- **B.** Lowest, just above `:=`, as in JavaScript.
- **C.** Postfix level, like `.`.

**Recommendation:** A: arithmetic feeds a pipe, and a pipe's result can be
compared.

```text
fn check(a: i32, b: i32) -> bool:
    a + b |> double() == 10  # hypothetical syntax
```

### PL6. Can a line start with `|>`?

Effect: without a rule, a multi-line pipe needs parentheses, since a line
starting with a binary operator never continues.

- **A.** Yes: a leading-`|>` continuation rule, parallel to leading dots.
- **B.** No: wrap multi-line pipes in parentheses.

**Recommendation:** A.

```text
fn slug(title: string) -> string:
    title.trim().lower().split(" ")
        |> text.join("-")  # hypothetical syntax
        |> collapse_dashes()  # hypothetical syntax
```

### PL7. Must a step have parentheses?

Effect: `url |> fetch!` and `x |> f` look natural to F# and Julia users,
but hd has no `!` without an argument clause.

- **A.** Parentheses required; `x |> f` is `pipe-target-not-call` with a
  fix-it.
- **B.** A bare name means a call with the value as its only argument.

**Recommendation:** A. It keeps one rule for `!` and leaves bare names
for deferred method values.

```text
fn page!(url: string) -> Result[Response, HttpError] $ Http:
    url |> fetch!()  # hypothetical syntax
```

### PL8. Which is evaluated first, the piped value or the step's callee?

Effect: `x |> obj.m(y)` evaluates `obj` first if read as `obj.m(x, y)`,
but `x` first in reading order.

- **A.** The piped value first, then the step as usual.
- **B.** Exactly the rewritten call's order.

**Recommendation:** A.

```text
fn run(items: List[i32], sink: Sink) -> void:
    load(items) |> sink.write()  # hypothetical syntax
```

### PL9. May a pipe step take a trailing block?

Effect: `job |> retry(3):` with a block would need a new trailing-block
position.

- **A.** No, for now; write `retry(job, 3):`.
- **B.** Yes, on the last step of a pipe in trailing-block position.

**Recommendation:** A. B can be added later.

```text
fn save(job: Job) -> void:
    retry(job, 3):
        store(job)
```

## Sources

- F# functions, pipelines: <https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/functions/#pipelines>
- Don Syme, "The Early History of F#", HOPL IV, 2020: <https://dl.acm.org/doi/10.1145/3386325>
- OCaml `Stdlib`: <https://ocaml.org/manual/5.2/api/Stdlib.html>
- Elm `Basics.(|>)`: <https://package.elm-lang.org/packages/elm/core/latest/Basics>
- Elixir `Kernel.|>/2` and `then/2`: <https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2>
- TC39 pipeline proposal: <https://github.com/tc39/proposal-pipeline-operator>; history: <https://github.com/tc39/proposal-pipeline-operator/blob/main/HISTORY.md>
- R forward pipe: <https://stat.ethz.ch/R-manual/R-devel/library/base/html/pipeOp.html>; R NEWS: <https://cran.r-project.org/doc/manuals/r-release/NEWS.html>
- Gleam tour, pipelines: <https://tour.gleam.run/functions/pipelines/>; function captures: <https://tour.gleam.run/functions/function-captures/>
- Julia manual, function composition and piping: <https://docs.julialang.org/en/v1/manual/functions/#Function-composition-and-piping>
- Unison, function application operators: <https://www.unison-lang.org/docs/fundamentals/values-and-functions/function-application-operators/>
- Kotlin extensions: <https://kotlinlang.org/docs/extensions.html>; scope functions: <https://kotlinlang.org/docs/scope-functions.html>
- Swift extensions: <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/extensions/>; SE-0352: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0352-implicit-open-existentials.md>
- C# extension methods: <https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/classes-and-structs/extension-methods>
- Rust Reference, dyn compatibility: <https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility>; `Iterator`: <https://doc.rust-lang.org/std/iter/trait.Iterator.html>; `tap::Pipe`: <https://docs.rs/tap/latest/tap/pipe/trait.Pipe.html>
- Go type parameters proposal, no parameterized methods: <https://go.googlesource.com/proposal/+/refs/heads/master/design/43651-type-parameters.md#no-parameterized-methods>; `iter`: <https://pkg.go.dev/iter>
- D, UFCS: <https://dlang.org/spec/function.html#pseudo-member>; D tour: <https://tour.dlang.org/tour/en/gems/uniform-function-call-syntax-ufcs>
- Nim manual, method call syntax: <https://nim-lang.org/docs/manual.html#procedures-method-call-syntax>
- Scala 3 extension methods: <https://docs.scala-lang.org/scala3/reference/contextual/extension-methods.html>; `ChainingOps`: <https://www.scala-lang.org/api/current/scala/util/ChainingOps.html>

## Parse Log

Every `text` block was parsed with the reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)) on
2026-09-29. Parsing checks syntax only; no block is claimed to type-check.
Names such as `Response`, `Client`, `ClientBuilder`, `Job`, and the error
types are placeholders.

The parser stops at the first error, so a hypothetical line hides the rest
of its block. Each such block was parsed a second time with every `|>` step
rewritten as a leading-dot step and every topic `_` as a name. All of them
then parse, except block 16 line 3, where `match` cannot follow a dot.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Use Cases, helpers | parses |
| 2 | Why A Pipe Cannot Be A Library Function | parses |
| 3 | Option 1, `std` declarations | parses |
| 4 | Option 1, U1 | parses; needs Option 1 to type-check |
| 5 | Option 1, U2 | parses |
| 6 | Option 1, U3 | parses |
| 7 | Option 1, U4 | parses |
| 8 | Option 2, U1 | `syntax-error` at line 4, marked `# hypothetical syntax` |
| 9 | Option 2, U2 | `syntax-error` at line 2, marked |
| 10 | Option 2, U3 | `syntax-error` at line 2, marked |
| 11 | Option 2, U4 | `syntax-error` at line 5, marked |
| 12 | Option 3, U1 | `syntax-error` at line 4, marked |
| 13 | Option 3, U2 | `syntax-error` at line 2, marked |
| 14 | Option 3, U3 | `syntax-error` at line 2, marked |
| 15 | Option 3, U4 | `syntax-error` at line 5, marked |
| 16 | Option 3, `_` collisions | `syntax-error` at line 2, marked |
| 17 | Option 4, U1 | parses; needs UFCS to type-check |
| 18 | Option 4, U2 | parses; needs UFCS to type-check |
| 19 | Option 4, U3 | parses; needs UFCS to type-check |
| 20 | Option 4, U4 | parses; needs UFCS to type-check |
| 21 | Option 5, `std.iter` | parses |
| 22 | Option 5, U1 | parses |
| 23 | Hard Parts, which argument | `syntax-error` at line 2, marked |
| 24 | Hard Parts, `!` | `syntax-error` at line 2, marked |
| 25 | Hard Parts, rows | `syntax-error` at line 2, marked |
| 26 | Hard Parts, trailing blocks | `syntax-error` at line 2, marked |
| 27 | Hard Parts, method calls on the right | `syntax-error` at line 2, marked |
| 28 | Hard Parts, parenthesized multi-line pipe | `syntax-error` at line 3, marked |
| 29 | PL1 | parses; needs Option 1 to type-check |
| 30 | PL2 | parses |
| 31 | PL3 | `syntax-error` at line 2, marked |
| 32 | PL4 | `syntax-error` at line 2, marked |
| 33 | PL5 | `syntax-error` at line 2, marked |
| 34 | PL6 | `syntax-error` at line 3, marked |
| 35 | PL7 | `syntax-error` at line 2, marked |
| 36 | PL8 | `syntax-error` at line 2, marked |
| 37 | PL9 | parses |

