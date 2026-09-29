# Chaining Study: Pipe, Function Shorthand, And Iterator Adapters

Status: design exploration and stress test, 2026-09-29. Owner decisions
CS2, CS7, and CS8 are applied (2026-09-29), and the specification is
authoritative for them:
[Pipe Expressions](../spec/05-expressions.md#pipe-expressions),
[Leading-Pipe Continuation](../spec/01-lexical-structure.md#leading-pipe-continuation),
[Iteration Protocols](../spec/06-control-flow.md#iteration-protocols), and
[Iterator Adapters](../spec/06-control-flow.md#iterator-adapters). CS1 and
CS3 changed nothing in the specification, and CS5 became
[Method References](METHOD_REFERENCES.md). The rest of the record is the
study behind the decisions.

The owner asked for one joint study of three linked questions, because each
answer changes the others:

1. the pipe, as decided in [Pipe Operator PL1-PL10](PIPE_OPERATOR.md#owner-decisions);
2. a function shorthand, deferred by
   [PL10](PIPE_OPERATOR.md#owner-decisions);
3. iterator adapters as methods or as free functions,
   [STDLIB question 14](STDLIB.md#14-where-do-the-iterator-adapters-live)
   and [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters).

The study combines the [brainstorm](../.agents/skills/brainstorm/SKILL.md)
and [stress-test](../.agents/skills/stress-test/SKILL.md) methods. It
builds a matrix of combinations, prunes it to six, translates one corpus of
real code into each, parses every block, and measures the results.

## Owner Decisions

Decided 2026-09-29.

1. **CS1: iterator adapters stay methods** (`xs.iter().map(f)`). PL1 and
   PL2 stand: `map` and `fold` are static-only default methods and can't
   be overridden.
2. **CS2: pipe steps are `_` steps or bare names.** This amends PL8.
   - A step with `_` substitutes the value (PL7).
   - A **bare name**, a plain function name or path with no call
     parentheses, means a one-argument call: `x |> f` means `f(x)`. It must
     not suspend; write `x |> f!(_)`.
   - A call step without `_`, such as `x |> f(y)`, is an **error**. That
     removes the `f(y)(x)` reading and the "`make()` runs first" hazard.
   - **No mixing `|>` with leading-dot continuation lines.** Inside a pipe
     chain, a line starting with `.` is an error. Write the method call as
     a step (`|> _.map(fn(x): x * 2)`) or bind a name first. That removes
     the "`.map` attaches to the bare function" hazard.
   - **No indexing `[...]` on a bare step either** (owner, the same day).
     `x |> f[0]` and `x |> parse[i32]` are errors, because it's unclear
     whether that indexes the function or gives it type arguments. Write
     the `_` form: `x |> f(_)[0]` or `x |> parse[i32](_)`. So a bare step
     is only a name or a dotted path, never followed by `.`, `[` or `(`.
3. **CS3: no member-path shorthand** (`.name` as a function). It would
   look like enum shorthand such as `.None`. With PL10, closures stay
   `fn(u): u.name`.

4. **CS5: method and field references (`User::name` as a function) get
   designed now,** as a separate research task. Their spelling must not look
   like enum shorthand (see CS3).
5. **CS6: std adds key-function helpers** such as
   `sorted_by_key(fn(p): p.age)` and `sum_by(fn(o): o.total)`, as in Rust
   `sort_by_key` and Kotlin `sumOf`. This is std-only, recorded in
   STDLIB.md.

6. **CS7 (2026-09-29): `Iterator[T]` becomes a concrete, closure-backed
   `data` type, not a trait.** This reverses the `Iterator` trait of
   STDLIB 14 and makes PL1 and PL2 (static-only methods, no override) moot.
   - `data Iterator[T]` holds a `step: fn() -> T?` closure. All adapters
     (`filter`, `take`, `enumerate`, `map[U]`, `fold[A, R]`, `collect`,
     and so on) are ordinary methods, generic ones included, with no
     dynamic-safety question.
   - `next(mut self) -> T?` keeps advancing visible. Lazy adapters return
     `mut Iterator[...]`. Their callbacks have the empty row, while `fold`
     carries `R`.
   - A user source is built from a closure (Gleam's
     `Iterator`/`Yielder` and Go 1.23's `iter.Seq` are the precedents).
     The cost is one indirect call per step.
   - **`for x in coll` uses a new prelude trait `Iterable[T]`:
     `fn iter(self) -> mut Iterator[T]`.** It has no method-level generics,
     so it is dynamically safe. `List`, `Map` and `Iterator` itself
     implement it.

7. **CS8 (2026-09-29): CS7 is final, decided from the stage-1 analysis
   (future-work/ITERATOR_PERF.md).** The closure-backed data `Iterator[T]`
   is the one public iterator type. The owner's flat composed-stage `Iter`
   (design C) stays a later option, worth revisiting once the compiler
   specializes and inlines closures: without fusion, it makes no fewer
   indirect calls than the closure design. Stage 2's benchmarks measured
   nothing (every program fell back to plain loops). What it did produce is
   the writing-log evidence that the prototype can't yet capture a `mut`
   value in a closure or infer tuple element types across closure
   boundaries. Those are tracked as prototype gaps. The per-element costs
   that hurt every design (boxed `T?`, per-call dictionaries: F-502, F-504)
   should be fixed first.

## Still Open

Points the apply pass for CS7 and CS8 met (2026-09-29). Each waits for the
owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | How does user code build an `Iterator` from its own closure: a public `step` and the literal `Iterator { step: f }`, or a private `step` and an associated function? | The specification gives the field's type only ([`flow.for.iterator-type`](../spec/06-control-flow.md#r-flow.for.iterator-type)); no fixture builds a user iterator | A private `step` and `Iterator::from_fn(step)`, as Rust's `iter::from_fn`, so only `next(mut self)` advances an iterator. |
| 2 | `Iterator`'s `iter(self)` has a readonly receiver but returns the same traversal, so code with readonly access can advance an iterator through `iter()` or an `Iterable[T]` bound | Stated as [`flow.for.iterator-self`](../spec/06-control-flow.md#r-flow.for.iterator-self) and [`flow.for.iterator-bound`](../spec/06-control-flow.md#r-flow.for.iterator-bound); a loop directly over a readonly iterator stays `mutable-receiver-required` ([`flow.for.iterator-mut`](../spec/06-control-flow.md#r-flow.for.iterator-mut)) | Keep, and say in the guide that `iter()` on an iterator shares its progress. Rejecting it needs a rule exception for one implementation. |
| 3 | [`flow.for.iterator-progress`](../spec/06-control-flow.md#r-flow.for.iterator-progress) says `next` returns `.None` after exhaustion, but a user `step` closure may yield again | Kept as written | Keep; the constructor of point 1 wraps the closure so that `.None` is final. |

## Contents

1. [Decisions Under Review](#decisions-under-review)
2. [Why The Three Questions Are One](#why-the-three-questions-are-one)
3. [Survey: Languages With More Than One Of These](#survey-languages-with-more-than-one-of-these)
4. [The Matrix](#the-matrix)
5. [Surfaces Assumed](#surfaces-assumed)
6. [Corpus](#corpus)
7. [Translations](#translations)
8. [Measurements](#measurements)
9. [Hazards Found](#hazards-found)
10. [Ranking](#ranking)
11. [Recommendation](#recommendation)
12. [Questions For The Owner](#questions-for-the-owner)
13. [Sources](#sources)
14. [Parse Log](#parse-log)

## Decisions Under Review

| Decision | Content today | Source |
| --- | --- | --- |
| PL1, PL2 | `map` and `fold` are static-only default methods of `Iterator`; overriding one is `static-only-override` | [PL1, PL2](PIPE_OPERATOR.md#owner-decisions) |
| PL3, PL4 | Gleam-style first-argument pipe with bare names; superseded by PL7 and PL8 | [PL3, PL4](PIPE_OPERATOR.md#owner-decisions) |
| PL5, PL6 | no trailing blocks on steps; the piped value is evaluated first; `\|>` sits between comparison and `\|`; a line may start with `\|>` | [PL5, PL6](PIPE_OPERATOR.md#owner-decisions) |
| PL7 | Hack style: one `_` per step marks the slot; any expression; no `_` inside a nested closure | [PL7](PIPE_OPERATOR.md#owner-decisions) |
| PL8 | a step without `_` is a function value applied to the piped value (F# style); such an **application step** must not suspend | [PL8](PIPE_OPERATOR.md#owner-decisions) |
| PL9 | steps are single-line | [PL9](PIPE_OPERATOR.md#owner-decisions) |
| PL10 | no `_` lambda shorthand for now; revisit with the hd writing log | [PL10](PIPE_OPERATOR.md#owner-decisions) |
| STDLIB 14 | iterator adapters are default methods of the prelude `Iterator` | [STDLIB Owner Decisions](STDLIB.md#owner-decisions) |
| Closures | `fn(x): ...` is the only closure form | [`fn.closure.no-other-syntax`](../spec/07-functions.md#r-fn.closure.no-other-syntax) |
| Method values | `Type::name` and `x::name` are reserved and rejected | [Method Values](../spec/07-functions.md#method-values) |
| `.Variant` | the leading-dot shorthand needs an expected enum type and is never a function value | [`data.enum.fn-value.shorthand`](../spec/08-data-and-enums.md#r-data.enum.fn-value.shorthand) |

Two terms used below. A **substitution step** is a pipe step with `_`
(PL7). An **application step** is a pipe step without `_` (PL8).

## Why The Three Questions Are One

Each answer changes what the other two cost:

| Coupling | Effect |
| --- | --- |
| Adapter style decides which pipe steps are needed | Curried adapters (`xs \|> map(f)`) need application steps. Data-first adapters (`xs \|> map(_, f)`) need only substitution steps. Methods need neither for iterators. |
| The pipe owns `_` | A `_`-based shorthand (`fn: _ * 2`, Scala's `_ * 2`, F#'s `_.name`) competes with the pipe topic on the same line. |
| Shorthand only helps one-parameter callbacks | `filter`, `map`, `group_by`, `and_then` gain; `fold` and `sorted_by` take two parameters and gain nothing. |
| Adapter style decides consistency | `T?.map` is a normative built-in method ([Built-In Methods](../spec/10-modules.md#built-in-methods)); `Result.map_err` and `List.sorted_by` are `std` methods. Free-function adapters split the call style between iterators and every other container. |
| Adapter style decides dynamic safety | Methods need PL1's static-only rule; free functions need none. |

## Survey: Languages With More Than One Of These

| Language | Adapters | Pipe | Closure form | Shorthand | How they coexist | Sources |
| --- | --- | --- | --- | --- | --- | --- |
| Elixir | data-first free functions (`Enum.map(xs, f)`), lazy twins in `Stream` | `\|>` inserts into the first argument | `fn x -> x * 2 end` | capture: `&(&1 * 2)`, `&String.trim/1`, `& &1.name` | Captures number their arguments (`&1`), so no token is shared with the pipe, which has no placeholder; `then/2` covers other slots | [Kernel.SpecialForms `&`](https://hexdocs.pm/elixir/Kernel.SpecialForms.html#&/1), [Kernel `\|>`](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2), [Enum](https://hexdocs.pm/elixir/Enum.html) |
| Gleam | data-first (`list.map(xs, f)`) | `\|>` inserts first; if that fails, calls the step's result with the value | `fn(x) { x * 2 }` | capture: `add(1, _)`, one `_`, call only | `_` has one meaning: a capture. `x \|> f(a, _)` is the capture applied to `x`, so the pipe needs no topic of its own | [Tour: pipelines](https://tour.gleam.run/functions/pipelines/), [Tour: captures](https://tour.gleam.run/functions/function-captures/), [gleam/list](https://hexdocs.pm/gleam_stdlib/gleam/list.html) |
| F# | curried, data-last modules (`List.map f xs`); .NET methods also exist | library `\|>`; `x \|> f` is `f x` | `fun x -> x * 2` | `_.Name` and `_.Method()` since F# 8 | The pipe has no placeholder, so `_.` is free for lambdas: `xs \|> List.map _.Name` | [F# 8 what's new](https://learn.microsoft.com/en-us/dotnet/fsharp/whats-new/fsharp-8), [List module](https://fsharp.github.io/fsharp-core-docs/reference/fsharp-collections-listmodule.html) |
| Kotlin | extension functions on `Iterable` and `Sequence` | none; `let` and `run` scope functions | `{ x -> x * 2 }` | implicit `it`; references `User::name`, `String::trim` | No pipe, so `it` and `::` references have no competitor | [Lambdas: `it`](https://kotlinlang.org/docs/lambdas.html#it-implicit-name-of-a-single-parameter), [Callable references](https://kotlinlang.org/docs/reflection.html#callable-references), [Extensions](https://kotlinlang.org/docs/extensions.html) |
| Swift | protocol extensions on `Sequence` | none | `{ x in x * 2 }` | `$0`; key paths as functions, `\.name` | Key paths need `\` because `.name` already means an implicit member such as `.red` | [Closures](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/), [SE-0249](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0249-key-path-literal-function-expressions.md) |
| Scala 3 | collection methods; extension methods | `pipe` method from `scala.util.chaining` | `x => x * 2` | placeholder `_ * 2`; each `_` is a new parameter | Scala 3 moved the type wildcard from `_` to `?` to cut `_` overloading | [Anonymous functions](https://docs.scala-lang.org/scala3/book/fun-anonymous-functions.html), [Wildcard arguments](https://docs.scala-lang.org/scala3/reference/changed-features/wildcards.html), [Placeholder syntax](https://scala-lang.org/files/archive/spec/2.13/06-expressions.html#placeholder-syntax-for-anonymous-functions), [`ChainingOps`](https://www.scala-lang.org/api/current/scala/util/ChainingOps.html) |
| Rust | `Iterator` methods; `where Self: Sized` keeps `map` off the vtable | none | `\|x\| x * 2` | none; a method path such as `str::trim` is a function value | Methods only; no competition | [`Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html), [Closures](https://doc.rust-lang.org/book/ch13-01-closures.html), [Path expressions](https://doc.rust-lang.org/reference/expressions/path-expr.html) |
| JavaScript | `Array.prototype` methods | TC39 Hack pipe, `%` topic, Stage 2 | `x => x * 2` | none | The topic is `%`, a token arrows do not use; the F# pipe lost partly because each step needed an arrow | [Proposal README](https://github.com/tc39/proposal-pipeline-operator), [HISTORY.md](https://github.com/tc39/proposal-pipeline-operator/blob/main/HISTORY.md) |
| Elm | curried, data-last modules | `\|>` | `\x -> x * 2` | record accessor `.name` is a function | `.name` is free because Elm has no leading-dot member syntax | [Records](https://elm-lang.org/docs/records) |
| Ruby | `Enumerable` methods | none | `{ \|x\| x * 2 }` | `&:name`; `_1` (2.7); `it` (3.4) | No pipe | [`Symbol#to_proc`](https://docs.ruby-lang.org/en/3.3/Symbol.html#method-i-to_proc), [Ruby 3.4](https://www.ruby-lang.org/en/news/2024/12/25/ruby-3-4-0-released/) |
| Hack | methods and `Vec\map`-style free functions | `\|>` with topic `$$` | `$x ==> $x * 2` | none | The topic `$$` is used by nothing else | [Hack pipe](https://docs.hhvm.com/hack/expressions-and-operators/pipe) |

### Takeaways

1. **No surveyed language gives one token both jobs: pipe topic and
   lambda placeholder.** Topic pipes use a token nothing else uses: `$$`
   in Hack, `%` in TC39, and `_` in R, which has no lambda shorthand.
   Languages with a `_` lambda (Scala, F#, Gleam) have no topic pipe.
2. **Gleam is the one unified design.** Its `_` always makes a capture,
   and the pipe applies the capture. hd cannot copy it: `fetch!(_)?.body`
   would apply `?` and `.body` to the capture, not to the call's result.
3. **The library follows the chaining style.** Data-first libraries pair
   with a first-argument pipe (Elixir, Gleam). Curried libraries pair with
   an application pipe (F#, Elm). Method libraries ship no pipe (Kotlin,
   Swift, Rust, Scala, Ruby). JavaScript, with methods plus a proposed topic
   pipe, is closest to hd's decided design.
4. **Member shorthands are the most common kind.** Kotlin `User::name`,
   Swift `\.name`, Ruby `&:name`, Elm `.name`, F# `_.Name`, and Elixir
   `& &1.name` all target "call a member of the one argument".

## The Matrix

The axes:

| Axis | Values |
| --- | --- |
| Adapters | **M**: methods with PL1 static-only rule; **D**: data-first `std.iter` free functions; **C**: curried `std.iter` free functions |
| Shorthand | **N**: none; **F**: `fn: _ * 2` with an explicit boundary; **K**: a member-path function `.name` |
| Pipe | **P1**: as decided (substitution and application steps); **P2**: simplified, substitution steps only (PL7 without PL8) |

Why these values:

- `it` is out: it is the prelude test function, and
  [`module.prelude.no-shadow`](../spec/10-modules.md#r-module.prelude.no-shadow)
  forbids shadowing it.
- `$0` is out: `$` already starts rows, `$.use`, and interpolation.
- Scala's bare `_ * 2` is out: without a boundary, `x |> f(_ * 2)` has two
  readings.
- For the accessor, three spellings were checked with the reference parser:

| Spelling | Parses today | Conflict |
| --- | --- | --- |
| `.name` (Elm, Swift's implicit member) | yes | `.Variant` also starts with a dot; told apart by the expected type (enum or function) |
| `User::name` (Kotlin) | no: `deferred-method-value` | reserved for unbound method values; a field and a method may share a name ([`names.member.shared-name`](../spec/03-names-and-scopes.md#r-names.member.shared-name)) |
| `\.name` (Swift) | no: `invalid-token` | a new token |

The matrix uses `.name`, the only spelling that needs no syntax change.
Two simplified pipes were rejected. First-argument insertion reopens what
PL7 reversed. An application-only pipe cannot mark `!` or `?`, which PL8
sends to the `_` form.

### Cells

Eighteen cells; twelve are pruned.

| Adapters | Shorthand | P1 (as decided) | P2 (substitution only) |
| --- | --- | --- | --- |
| M | N | **S1** (status quo) | **S2** |
| M | F | pruned: `fn:` `_` sits beside application steps that have no `_`; dominated by S3 | **S3** |
| M | K | pruned: differs from S4 only on the pipe axis, which S1 versus S2 already measures | **S4** |
| D | N | pruned: identical to D-N-P2, since data-first steps all use `_`; P1 only adds unused rules | **S6** |
| D | F | pruned: `xs \|> iter.map(_, fn: _ * 2)` puts two `_` meanings in one step | pruned: same |
| D | K | pruned: differs from S6 only by accessor savings, which S4 measures | pruned: same |
| C | N | **S5** | pruned: curried adapters need application steps; `xs \|> iter.map(f)(_)` |
| C | F | pruned: `xs \|> iter.map(fn: _ * 2)` shows a `_` in an application step | pruned: needs application steps |
| C | K | pruned: differs from S5 only by accessor savings | pruned: needs application steps |

Hybrids were also pruned. Methods plus curried twins in `std.iter` give two
spellings of every adapter. Methods for `List` and `T?` with free functions
for iterators are what S5 and S6 already are, since `List.map` and `T?.map`
are normative methods.

### Survivors

| ID | Package | Nearest real model |
| --- | --- | --- |
| S1 | methods, no shorthand, pipe as decided | JavaScript with a Hack/F# hybrid pipe |
| S2 | methods, no shorthand, substitution-only pipe | JavaScript with the TC39 Hack pipe |
| S3 | methods, `fn: _` shorthand, substitution-only pipe | Scala with an explicit boundary |
| S4 | methods, `.name` member-path functions, substitution-only pipe | Kotlin `it.name`, Swift `\.name`, Elm `.name` |
| S5 | curried adapters, no shorthand, pipe as decided | F#, Elm |
| S6 | data-first adapters, no shorthand, substitution-only pipe | Go helpers plus a Hack pipe; the radical simplification |

S6 is the radical simplification: no static-only rule, no application
steps, no shorthand.

## Surfaces Assumed

### Shared Declarations

Every translation uses these helpers. `Sale`, `Team`, `User`, and the
error types are placeholders; parsing checks syntax only.

```text
data Order:
    customer: string
    total: i64
    paid: bool

data CustomerTotal:
    customer: string
    total: i64

fn parse_sale(line: string) -> Sale?:
    pass

fn ascii_fold(value: string) -> string:
    pass

fn strip_symbols(value: string) -> string:
    pass

fn collapse_dashes(value: string) -> string:
    pass

fn strip_edges(value: string, chars: string) -> string:
    pass

fn team_by_slug!(slug: string) -> Result[Team?, DbError] $ Db:
    pass

fn members!(team: i64) -> Result[List[Member], DbError] $ Db:
    pass

fn render_team(team: Team, names: List[string]) -> Result[string, TemplateError]:
    pass

fn user_url(id: i64) -> string:
    pass

fn fetch!(url: string) -> Result[Response, HttpError] $ Http:
    pass

fn decode_user(body: string) -> Result[User, DecodeError]:
    pass

fn parse_port(text: string) -> i64?:
    pass

fn read_file!(path: string) -> Result[string, FsError] $ Fs:
    pass

fn parse_config(body: string) -> Result[Config, SyntaxError]:
    pass
```

Every package also assumes one `std` addition on `List`, needed by the
report case. `List` is concrete, so it is an inherent method in every
package:

```text
impl[T] List[T]:
    pub fn group_by[K < Eq & Hash](self, key: fn(T) -> K) -> Map[K, List[T]]:
        pass
```

### Adapters As Methods (S1-S4)

PL1's static-only default methods, plus `filter_map`, which the ETL case
needs and every package gets in its own style:

```text
pub trait Iterator[T]:
    fn next(mut self) -> T?

    fn map[U](mut self, transform: fn(T) -> U) -> mut Iterator[U]:
        pass

    fn filter_map[U](mut self, transform: fn(T) -> U?) -> mut Iterator[U]:
        pass

    fn fold[A, R](mut self, initial: A, step: fn(A, T) -> A $ R) -> A $ R:
        pass
```

`filter`, `take`, `enumerate`, and `collect` stay as specified.

### Adapters As Data-First Functions (S6)

All six adapters move to `std.iter`. The four specified ones leave the
trait:

```text
pub fn map[T, U](source: mut Iterator[T], transform: fn(T) -> U) -> mut Iterator[U]:
    pass

pub fn filter[T](source: mut Iterator[T], keep: fn(T) -> bool) -> mut Iterator[T]:
    pass

pub fn fold[T, A, R](source: mut Iterator[T], initial: A, step: fn(A, T) -> A $ R) -> A $ R:
    pass

pub fn collect[T](source: mut Iterator[T]) -> List[T]:
    pass
```

### Adapters As Curried Functions (S5)

Each adapter takes its settings and returns a function of the iterator. A
row on the returned function needs parentheses, by
[`fn.closure.clause-grouped`](../spec/07-functions.md#r-fn.closure.clause-grouped):

```text
pub fn map[T, U](transform: fn(T) -> U) -> fn(mut Iterator[T]) -> mut Iterator[U]:
    pass

pub fn filter[T](keep: fn(T) -> bool) -> fn(mut Iterator[T]) -> mut Iterator[T]:
    pass

pub fn fold[T, A, R](initial: A, step: fn(A, T) -> A $ R) -> (fn(mut Iterator[T]) -> A $ R):
    pass

pub fn collect[T](source: mut Iterator[T]) -> List[T]:
    pass
```

S5 also needs a new inference rule. In `it |> iter.map(fn(v): v * 2)`,
`map`'s `T` is known only from the piped value. The pipe must give the
step the expected type `fn(V) -> _`, where `V` is the value's type.
Without it, the closure is `closure-parameter-needs-annotation`
([`fn.closure.needs-annotation`](../spec/07-functions.md#r-fn.closure.needs-annotation)).

### The Substitution-Only Pipe (P2)

P2 is PL7 as the owner first wrote it, keeping PL5, PL6, and PL9:

| Rule | Text |
| --- | --- |
| One topic | Every step contains exactly one `_` outside nested closures. |
| Missing topic | A step without `_` is an error, with the fix-it `f(_)`. Error: `pipe-missing-topic`. |
| No capture | A `_` inside a nested closure is an error. |

PL8's application steps and their non-suspending rule are removed.

### The `fn: _` Shorthand (S3)

| Rule | Text |
| --- | --- |
| Form | `fn: expr` is a closure with one parameter, named `_` in `expr`. |
| One use | `expr` contains exactly one `_`, as a pipe step does. `fn: _ * _` is an error. |
| Binding | A `_` belongs to the innermost enclosing `fn:`; otherwise to the enclosing pipe step. |
| Types | The parameter type comes from the expected function type, as for any unannotated closure. |

The "one use" rule matches PL7. The alternative, many uses of one value as
Kotlin's `it` allows, would make `fn: _ * _` mean a square; Scala reads it
as two parameters.

### The Member-Path Function (S4)

| Rule | Text |
| --- | --- |
| Form | Where the expected type is a function type with one parameter `T`, a leading-dot postfix chain `.a.b(args)` means `fn(v: T): v.a.b(args)`. |
| Chain end | The chain ends at the first token that is not `.name`, an argument clause, or an index. `.price * 2` is `(.price) * 2`, a type error. |
| No effects | `?` and `!` may not appear in the chain. |
| Arguments | Argument expressions are evaluated at each call, like a closure body, and may name locals. |
| Variants | With an expected enum type, `.Name` stays the variant shorthand. The two expected types never overlap. |

No syntax changes: every S4 line with a member path parses with today's
reference parser.

## Corpus

Six cases from real code. Each approximates the shape of its source; none
is copied at length.

| Case | Shape | Source |
| --- | --- | --- |
| C1 | line-based ETL: trim, drop blanks and comments, parse, filter, cap | Elixir [Enumerables and Streams](https://hexdocs.pm/elixir/enumerable-and-streams.html) and [`File.stream!/3`](https://hexdocs.pm/elixir/File.html#stream!/3); Rust Cookbook [CSV processing](https://rust-lang-nursery.github.io/rust-cookbook/encoding/csv.html) |
| C2 | web handler with `!` and `?`, and a fetch chain | [axum extractors](https://docs.rs/axum/latest/axum/extract/index.html); [reqwest README](https://docs.rs/reqwest/latest/reqwest/) |
| C3 | slugify, and header normalization | Django [`slugify`](https://github.com/django/django/blob/main/django/utils/text.py); Scala 3 book, [anonymous functions](https://docs.scala-lang.org/scala3/book/fun-anonymous-functions.html) |
| C4 | report: filter, group, sum, sort, top three | Kotlin [Grouping](https://kotlinlang.org/docs/collection-grouping.html) and [Aggregate operations](https://kotlinlang.org/docs/collection-aggregate.html) |
| C5 | Option and Result chains | Rust [`Option::and_then`](https://doc.rust-lang.org/std/option/enum.Option.html#method.and_then); Rust by Example, [`map` for Result](https://doc.rust-lang.org/rust-by-example/error/result/result_map.html); Swift [Optional Chaining](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/optionalchaining/) |
| C6 | builder with helpers from another package | [`reqwest::ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html); [reqwest-middleware](https://docs.rs/reqwest-middleware/latest/reqwest_middleware/) |

## Translations

Each case shows the source shape, then one hd block per distinct
translation. The label names every package that shares the block.

### C1: ETL Pipeline

```elixir
path
|> File.stream!()
|> Stream.map(&String.trim/1)
|> Stream.reject(&(&1 == "" or String.starts_with?(&1, "#")))
|> Stream.map(&parse_sale/1)
|> Stream.reject(&is_nil/1)
|> Stream.filter(&(&1.amount > 0))
|> Enum.take(1000)
```

```rust
input.lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && !line.starts_with('#'))
    .filter_map(parse_sale)
    .filter(|sale| sale.amount > 0)
    .take(1000)
    .collect::<Vec<_>>()
```

**S1, S2.**

```text
fn load_sales(input: string) -> List[Sale]:
    input.lines().iter()
        .map(fn(line): line.trim())
        .filter(fn(line): line != "" && !line.starts_with("#"))
        .filter_map(parse_sale)
        .filter(fn(sale): sale.amount > 0)
        .take(1000)
        .collect()
```

**S3.**

```text
fn load_sales(input: string) -> List[Sale]:
    input.lines().iter()
        .map(fn: _.trim())  # hypothetical syntax
        .filter(fn(line): line != "" && !line.starts_with("#"))
        .filter_map(parse_sale)
        .filter(fn: _.amount > 0)  # hypothetical syntax
        .take(1000)
        .collect()
```

**S4.**

```text
fn load_sales(input: string) -> List[Sale]:
    input.lines().iter()
        .map(.trim())
        .filter(fn(line): line != "" && !line.starts_with("#"))
        .filter_map(parse_sale)
        .filter(fn(sale): sale.amount > 0)
        .take(1000)
        .collect()
```

**S5.**

```text
use std.iter

fn load_sales(input: string) -> List[Sale]:
    input.lines().iter()
        |> iter.map(fn(line): line.trim())  # hypothetical syntax
        |> iter.filter(fn(line): line != "" && !line.starts_with("#"))  # hypothetical syntax
        |> iter.filter_map(parse_sale)  # hypothetical syntax
        |> iter.filter(fn(sale): sale.amount > 0)  # hypothetical syntax
        |> iter.take(1000)  # hypothetical syntax
        |> iter.collect  # hypothetical syntax
```

**S6.**

```text
use std.iter

fn load_sales(input: string) -> List[Sale]:
    input.lines().iter()
        |> iter.map(_, fn(line): line.trim())  # hypothetical syntax
        |> iter.filter(_, fn(line): line != "" && !line.starts_with("#"))  # hypothetical syntax
        |> iter.filter_map(_, parse_sale)  # hypothetical syntax
        |> iter.filter(_, fn(sale): sale.amount > 0)  # hypothetical syntax
        |> iter.take(_, 1000)  # hypothetical syntax
        |> iter.collect(_)  # hypothetical syntax
```

Every package keeps the two-condition `filter` as a full closure: `fn:`
allows one `_`, and a member path cannot hold `&&`.

### C2: Web Handler With `!` And `?`

```rust
async fn team_page(Path(slug): Path<String>, State(db): State<Db>) -> Result<Html<String>, AppError> {
    let team = db.team_by_slug(&slug).await?.ok_or(AppError::NotFound)?;
    let names: Vec<String> = db.members(team.id).await?
        .into_iter().filter(|m| m.active).map(|m| m.name).collect();
    let body = render_team(&team, &names).map_err(AppError::Template)?;
    Ok(Html(body))
}

async fn user_name(id: u64) -> Result<String, AppError> {
    let user: User = reqwest::get(user_url(id)).await?.json().await?;
    Ok(user.name)
}
```

**S1, S5.**

```text
fn team_page!(request: Request) -> Result[Html, AppError] $ Db:
    slug := request.param("team").ok_or(AppError.BadRequest)?
    team := team_by_slug!(slug)?.ok_or(AppError.NotFound)?
    names := members!(team.id)?.filter(fn(member): member.active).map(fn(member): member.name)
    body := render_team(team, names).map_err(AppError.Template)?
    .Ok(html(body))

fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url |> fetch!(_)?.body |> decode_user(_)?.name  # hypothetical syntax
    .Ok(name)
```

**S2, S6.**

```text
fn team_page!(request: Request) -> Result[Html, AppError] $ Db:
    slug := request.param("team").ok_or(AppError.BadRequest)?
    team := team_by_slug!(slug)?.ok_or(AppError.NotFound)?
    names := members!(team.id)?.filter(fn(member): member.active).map(fn(member): member.name)
    body := render_team(team, names).map_err(AppError.Template)?
    .Ok(html(body))

fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url(_) |> fetch!(_)?.body |> decode_user(_)?.name  # hypothetical syntax
    .Ok(name)
```

**S3.**

```text
fn team_page!(request: Request) -> Result[Html, AppError] $ Db:
    slug := request.param("team").ok_or(AppError.BadRequest)?
    team := team_by_slug!(slug)?.ok_or(AppError.NotFound)?
    names := members!(team.id)?.filter(fn: _.active).map(fn: _.name)  # hypothetical syntax
    body := render_team(team, names).map_err(AppError.Template)?
    .Ok(html(body))

fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url(_) |> fetch!(_)?.body |> decode_user(_)?.name  # hypothetical syntax
    .Ok(name)
```

**S4.**

```text
fn team_page!(request: Request) -> Result[Html, AppError] $ Db:
    slug := request.param("team").ok_or(AppError.BadRequest)?
    team := team_by_slug!(slug)?.ok_or(AppError.NotFound)?
    names := members!(team.id)?.filter(.active).map(.name)
    body := render_team(team, names).map_err(AppError.Template)?
    .Ok(html(body))

fn user_name!(id: i64) -> Result[string, AppError] $ Http:
    name := id |> user_url(_) |> fetch!(_)?.body |> decode_user(_)?.name  # hypothetical syntax
    .Ok(name)
```

The handler is the same in every package except for its two callbacks,
and `user_name` differs only in `user_url` against `user_url(_)`.
`members!` returns a `List`, whose `filter` and `map` are methods even in
S5 and S6. `!` and `?` sit at the same places everywhere.

### C3: String Processing

```python
value = unicodedata.normalize("NFKD", value).encode("ascii", "ignore").decode("ascii")
value = re.sub(r"[^\w\s-]", "", value.lower())
return re.sub(r"[-\s]+", "-", value).strip("-_")
```

```scala
line.split(",").map(_.trim.toLowerCase)
```

**S1, S5.**

```text
fn slugify(value: string) -> string:
    value
        |> ascii_fold  # hypothetical syntax
        |> strip_symbols(_.lower())  # hypothetical syntax
        |> collapse_dashes  # hypothetical syntax
        |> strip_edges(_, "-_")  # hypothetical syntax

fn headers(line: string) -> List[string]:
    line.split(",").map(fn(cell): cell.trim().lower())
```

**S2, S6.**

```text
fn slugify(value: string) -> string:
    value
        |> ascii_fold(_)  # hypothetical syntax
        |> strip_symbols(_.lower())  # hypothetical syntax
        |> collapse_dashes(_)  # hypothetical syntax
        |> strip_edges(_, "-_")  # hypothetical syntax

fn headers(line: string) -> List[string]:
    line.split(",").map(fn(cell): cell.trim().lower())
```

**S3.**

```text
fn slugify(value: string) -> string:
    value
        |> ascii_fold(_)  # hypothetical syntax
        |> strip_symbols(_.lower())  # hypothetical syntax
        |> collapse_dashes(_)  # hypothetical syntax
        |> strip_edges(_, "-_")  # hypothetical syntax

fn headers(line: string) -> List[string]:
    line.split(",").map(fn: _.trim().lower())  # hypothetical syntax
```

**S4.**

```text
fn slugify(value: string) -> string:
    value
        |> ascii_fold(_)  # hypothetical syntax
        |> strip_symbols(_.lower())  # hypothetical syntax
        |> collapse_dashes(_)  # hypothetical syntax
        |> strip_edges(_, "-_")  # hypothetical syntax

fn headers(line: string) -> List[string]:
    line.split(",").map(.trim().lower())
```

Without a pipe the slug is
`strip_edges(collapse_dashes(strip_symbols(ascii_fold(value).lower())), "-_")`.

### C4: Collection Report

```kotlin
val top = orders.filter { it.paid }
    .groupBy { it.customer }
    .map { (customer, group) -> CustomerTotal(customer, group.sumOf { it.total }) }
    .sortedByDescending { it.total }
    .take(3)
```

**S1, S2.**

```text
fn top_customers(orders: List[Order]) -> List[CustomerTotal]:
    groups := orders.filter(fn(order): order.paid).group_by(fn(order): order.customer)
    totals := [for customer, group in groups => CustomerTotal { customer: customer, total: sum_totals(group) }]
    totals.sorted_by(fn(a, b): b.total.cmp(a.total)).iter().take(3).collect()

fn sum_totals(orders: List[Order]) -> i64:
    orders.iter().fold(0, fn(sum, order): sum + order.total)
```

**S3.**

```text
fn top_customers(orders: List[Order]) -> List[CustomerTotal]:
    groups := orders.filter(fn: _.paid).group_by(fn: _.customer)  # hypothetical syntax
    totals := [for customer, group in groups => CustomerTotal { customer: customer, total: sum_totals(group) }]
    totals.sorted_by(fn(a, b): b.total.cmp(a.total)).iter().take(3).collect()

fn sum_totals(orders: List[Order]) -> i64:
    orders.iter().fold(0, fn(sum, order): sum + order.total)
```

**S4.**

```text
fn top_customers(orders: List[Order]) -> List[CustomerTotal]:
    groups := orders.filter(.paid).group_by(.customer)
    totals := [for customer, group in groups => CustomerTotal { customer: customer, total: sum_totals(group) }]
    totals.sorted_by(fn(a, b): b.total.cmp(a.total)).iter().take(3).collect()

fn sum_totals(orders: List[Order]) -> i64:
    orders.iter().fold(0, fn(sum, order): sum + order.total)
```

**S5.**

```text
use std.iter

fn top_customers(orders: List[Order]) -> List[CustomerTotal]:
    groups := orders.filter(fn(order): order.paid).group_by(fn(order): order.customer)
    totals := [for customer, group in groups => CustomerTotal { customer: customer, total: sum_totals(group) }]
    totals.sorted_by(fn(a, b): b.total.cmp(a.total)).iter() |> iter.take(3) |> iter.collect  # hypothetical syntax

fn sum_totals(orders: List[Order]) -> i64:
    orders.iter() |> iter.fold(0, fn(sum, order): sum + order.total)  # hypothetical syntax
```

**S6.**

```text
use std.iter

fn top_customers(orders: List[Order]) -> List[CustomerTotal]:
    groups := orders.filter(fn(order): order.paid).group_by(fn(order): order.customer)
    totals := [for customer, group in groups => CustomerTotal { customer: customer, total: sum_totals(group) }]
    totals.sorted_by(fn(a, b): b.total.cmp(a.total)).iter() |> iter.take(_, 3) |> iter.collect(_)  # hypothetical syntax

fn sum_totals(orders: List[Order]) -> i64:
    iter.fold(orders.iter(), 0, fn(sum, order): sum + order.total)
```

`sorted_by` and `fold` take two parameters, so no shorthand shortens them.
A `std` with key-function variants such as Kotlin's `sortedByDescending`
and `sumOf` would let S3 and S4 shorten those too.

### C5: Option And Result Chains

```rust
let port = env::var("PORT").ok().map(|s| s.trim().to_string())
    .and_then(|s| s.parse().ok()).unwrap_or(8080);
let body = fs::read_to_string(path).map_err(ConfigError::Io)?;
```

```swift
let city = user.address?.city ?? "unknown"
```

**S1, S2, S5, S6.**

```text
fn port(env: Map[string, string]) -> i64:
    env.get("PORT").map(fn(text): text.trim()).and_then(parse_port).unwrap_or(8080)

fn city(user: User) -> string:
    user.address.and_then(fn(address): address.city).unwrap_or("unknown")

fn load_config!(path: string) -> Result[Config, ConfigError] $ Fs:
    body := read_file!(path).map_err(ConfigError.Io)?
    parse_config(body).map_err(ConfigError.Syntax)
```

**S3.**

```text
fn port(env: Map[string, string]) -> i64:
    env.get("PORT").map(fn: _.trim()).and_then(parse_port).unwrap_or(8080)  # hypothetical syntax

fn city(user: User) -> string:
    user.address.and_then(fn: _.city).unwrap_or("unknown")  # hypothetical syntax

fn load_config!(path: string) -> Result[Config, ConfigError] $ Fs:
    body := read_file!(path).map_err(ConfigError.Io)?
    parse_config(body).map_err(ConfigError.Syntax)
```

**S4.**

```text
fn port(env: Map[string, string]) -> i64:
    env.get("PORT").map(.trim()).and_then(parse_port).unwrap_or(8080)

fn city(user: User) -> string:
    user.address.and_then(.city).unwrap_or("unknown")

fn load_config!(path: string) -> Result[Config, ConfigError] $ Fs:
    body := read_file!(path).map_err(ConfigError.Io)?
    parse_config(body).map_err(ConfigError.Syntax)
```

S5 and S6 keep `T?` and `Result` as methods, since `T?.map` is normative.
Their iterator code and their option code then use two call styles.

### C6: Builder

```rust
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(10))
    .user_agent("hd/1.0")
    .build()?;
let client = ClientBuilder::new(client).with(TracingMiddleware::default()).build();
```

**S1, S5.**

```text
use std.time.s
use dep.acme.{auth, tracing}

fn client(token: string) -> Result[Client, BuildError]:
    ClientBuilder::new()
        .timeout(10s)
        .user_agent("hd/1.0")
        |> tracing.install  # hypothetical syntax
        |> auth.with_bearer(_, token)  # hypothetical syntax
        .header("Accept", "application/json")
        .build()
```

**S2, S3, S4, S6.**

```text
use std.time.s
use dep.acme.{auth, tracing}

fn client(token: string) -> Result[Client, BuildError]:
    ClientBuilder::new()
        .timeout(10s)
        .user_agent("hd/1.0")
        |> tracing.install(_)  # hypothetical syntax
        |> auth.with_bearer(_, token)  # hypothetical syntax
        .header("Accept", "application/json")
        .build()
```

Under P1, `|> tracing.install` is valid only because the next line starts
with `|>`. Had it been followed by `.header(...)`, the leading-dot line
would join the step's function expression; see
[Hazards Found](#hazards-found).

## Measurements

### Counting Rules

- **Characters**: non-whitespace characters of each block, without the
  `# hypothetical syntax` comments.
- **Tokens**: identifiers, literals, and operators, with `|>`, `::`, `:=`,
  `->`, `=>`, `==`, `!=`, `<=`, `>=`, `&&`, and `||` as one token each.
- Each package's total sums its six blocks, including `use` lines and
  function headers, which are equal across packages except for
  `use std.iter`.
- **Closures** counts `fn(`; **`_`** counts every `_` token.

### Size

Characters / tokens per case:

| Case | S1 | S2 | S3 | S4 | S5 | S6 |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | 210 / 84 | 210 / 84 | 192 / 78 | 197 / 78 | 255 / 98 | 268 / 111 |
| C2 | 438 / 153 | 441 / 156 | 415 / 150 | 407 / 144 | 438 / 153 | 441 / 156 |
| C3 | 199 / 68 | 205 / 74 | 196 / 71 | 192 / 68 | 199 / 68 | 205 / 74 |
| C4 | 388 / 138 | 388 / 138 | 366 / 132 | 358 / 126 | 415 / 146 | 419 / 151 |
| C5 | 360 / 119 | 360 / 119 | 336 / 113 | 328 / 107 | 360 / 119 | 360 / 119 |
| C6 | 231 / 70 | 234 / 73 | 234 / 73 | 234 / 73 | 231 / 70 | 234 / 73 |
| **Total** | **1826 / 632** | **1838 / 644** | **1739 / 617** | **1716 / 596** | **1898 / 654** | **1927 / 684** |
| Characters against S1 | 0% | +0.7% | -4.8% | -6.0% | +3.9% | +5.5% |
| `\|>` steps | 9 | 9 | 9 | 9 | 18 | 17 |
| Closures (`fn(`) | 12 | 12 | 3 | 4 | 12 | 12 |
| `_` tokens | 5 | 9 | 18 | 9 | 5 | 17 |

The spread is small: S4 is 6% shorter than S1, and S6 is 5.5% longer.
S2 costs S1 twelve characters, three per unary step. The shorthand saves
characters only where a callback reads one member: eight sites in S4,
nine in S3.

### Everything Else

| Measure | S1 | S2 | S3 | S4 | S5 | S6 |
| --- | --- | --- | --- | --- | --- | --- |
| Concepts beyond closures, methods, `!`, `?` | 5: substitution step, application step, `f(y)` in a step means `f(y)(x)`, application steps can't suspend, `_` not in closures | 2: substitution step, `_` not in closures | 5: S2's, plus `fn:`, one `_` per `fn:`, `_` binds to the innermost `fn:` | 4: S2's, plus member-path functions, where the chain ends | 8: S1's, plus curried adapters, `use std.iter`, iterators piped while `List` and `T?` use dots | 5: S2's, plus data-first adapters, `use std.iter`, the style split |
| Ambiguity points (see [Hazards Found](#hazards-found)) | 5: H1-H5 | 2: H4, H5 | 4: H4-H7 | 4: H4, H5, H8, H9 | 7: H1-H5, H10, H11 | 3: H4, H5, H11 |
| `!` and `?` visibility | kept, by PL8's non-suspending rule | kept by construction: every step is an ordinary expression | as S2 | as S2 | as S1 | as S2 |
| Requirement rows | `fold[A, R]` method; lazy callbacks empty | as S1 | as S1; a `fn:` closure's row is inferred like any closure's | as S1; a member path calling a method with a row gets that row | `fold` returns `(fn(...) -> A $ R)`; `map(f)` returns a closure that captures `f`, then the iterator captures it again | `fold[T, A, R]` function; as S1 otherwise |
| Dynamic-safety rules | 2 (PL1, PL2) | 2 | 2 | 2 | 0 | 0 |
| IDE discoverability | high: `.` lists adapters | high | high: `_.` completes from the expected type | high: `(.` completes the members of `T`, as Swift does for key paths | low: adapters appear only after `iter.` | low: as S5 |
| Costliest change (Design Cost Order) | syntax | syntax | syntax (two forms) | syntax (the pipe only) | syntax | syntax |
| Changes: syntax / rule exceptions / library | 3 / 5 / 3 | 3 / 3 / 3 | 4 / 4 / 3 | 3 / 4 / 3 | 3 / 4 / 7 | 3 / 1 / 7 |
| Decisions reversed | none | PL8 | PL8, PL10, `fn.closure.no-other-syntax` | PL8; extends the `.Variant` rule | STDLIB 14, PL1, PL2, `flow.adapter.*` | STDLIB 14, PL1, PL2, PL8, `flow.adapter.*` |
| Predicted cheap-model error rate | medium | low | medium | low | high | high |

Change counts. Syntax: the `|>` token, `_` as an expression, and the
leading-`|>` line (three in every package); S3 adds `fn:`. Rule exceptions:
static-only, no override, `_` placement, and PL8's two step rules (S1).
S2 drops the two PL8 rules, and S3 and S4 add one rule each. S5 swaps the
two static-only rules for the pipe's expected type, and S6 keeps only `_`
placement. Library: `map`, `fold`, and `filter_map` (three); S5 and S6
also move `filter`, `take`, `enumerate`, and `collect` (seven).

### Predicted Cheap-Model Errors

No model was run; this is a prediction from what writers of other languages
type by reflex. The corpus has these reflex sites:

| Reflex | Who writes it | Sites in the corpus | Fails in | Fix the compiler can offer |
| --- | --- | --- | --- | --- |
| `xs.iter().map(f)` and friends | Rust, Kotlin, Swift, JS, Scala | 9 (C1, C4) | S5, S6 | "`map` is a function in `std.iter`": exact |
| `x \|> f(y)` meaning `f(x, y)` | Elixir, Gleam, R | 2 (`strip_edges`, `with_bearer`) | S1, S5: arity error on `f(y)`; S2-S4, S6: `pipe-missing-topic` | P2: fix-it `f(_, y)`; P1: a pipe-specific hint must be added to arity errors |
| `x \|> f` for a unary call | F#, Elm, Julia | 4 (`user_url`, `ascii_fold`, `collapse_dashes`, `install`) | S2, S3, S4, S6 | fix-it `f(_)` |
| `x \|> fetch!` | F# | 1 | all | fix-it `fetch!(_)` |
| `{ it.name }`, `$0.name`, `_.name` | Kotlin, Swift, Scala | 8 (member callbacks) | all: syntax or type errors | closure fix-it; S4 can suggest `.name` |
| `.age > 18` as a member path | writers who learned `.name` | 0 in the corpus; likely in filters | S4 | "a member path ends at `>`; write `fn(u): u.age > 18`" |
| `fn: _ + _` or `fn: _ != "" && _...` | Scala | 1 (C1's filter) | S3 | "one `_` per `fn:`" |

The iterator reflex dominates: nine sites, the most common code in the
corpus, fail only in S5 and S6. The pipe reflexes are rarer, and every one
has an exact fix-it under P2. Under P1 the Elixir reflex reaches an arity
error on `f(y)`, which does not mention the pipe unless a hint is added.
Shorthand reflexes fail equally everywhere, since no package accepts
`it`, `$0`, or bare `_`.

## Hazards Found

Every hazard is a place where a reader or writer can take a line to mean
something it does not.

| ID | Hazard | Packages | Silent? |
| --- | --- | --- | --- |
| H1 | `x \|> f(y)` means `f(y)(x)` | S1, S5 | no: an arity or type error, unless `f(y)` returns a fitting function |
| H2 | A leading-dot line after an application step joins the function expression | S1, S5 | no: a member error on a function value |
| H3 | `x \|> make()` calls `make()` and applies its result | S1, S5 | no, unless `make()` returns a fitting function |
| H4 | `_` also means a type-argument slot and a wildcard pattern | all | no |
| H5 | A step's `_` must not sit in a nested closure | all | no |
| H6 | Two `_` meanings on one line: `xs \|> helper(_, fn: _.id)` | S3 | no, but hard to read |
| H7 | A Scala reader expects `fn: _ + _` to take two parameters | S3 | no: an error |
| H8 | `.x` is a variant or a member path, by expected type | S4 | no |
| H9 | `.price * 2` and `.status == .Active` group as `(.price) * 2` | S4 | no: a type error |
| H10 | An adapter closure outside a pipe needs annotations | S5 | no |
| H11 | Iterators use free functions while `T?`, `Result`, and `List` use methods | S5, S6 | no |

No hazard is silent in the corpus. H1 and H3 can be silent when a function
returns a function, which no `std` API in S1-S4 does. In S5, every adapter
does.

H2, the application step before a leading dot:

```text
fn client() -> Client:
    ClientBuilder::new()
        |> tracing.install  # hypothetical syntax
        .build()
```

This means `tracing.install.build()` applied to the builder. The fix is
`|> tracing.install(_)`, which is the P2 form.

H6, two `_` meanings in one S3 step:

```text
fn by_customer(orders: List[Order]) -> Map[string, List[Order]]:
    orders |> report.index(_, fn: _.customer)  # hypothetical syntax
```

H9, a member path ending at an operator:

```text
fn active(orders: List[Order]) -> List[Order]:
    orders.filter(.status == .Active)
```

This parses today. Under S4 it compares a function with a variant, a type
error. The fix is `fn(order): order.status == .Active`.

H10, a curried adapter outside a pipe:

```text
use std.iter

fn doubler() -> fn(mut Iterator[i32]) -> mut Iterator[i32]:
    iter.map(fn(v: i32): v * 2)
```

The annotation `v: i32` is required; in a pipe step the expected type
would supply it.

## Ranking

Ranked by the measurements above, then the Design Cost Order as a
tie-breaker. The pipe is syntax in every package, so the order separates
packages only by counts of changes.

| Rank | Package | Why |
| --- | --- | --- |
| 1 | S4 | shortest (-6%); no new syntax beyond the pipe; the shorthand needs no `_`, so it never meets the pipe; low predicted error rate |
| 2 | S2 | fewest concepts (2) with methods; every pipe reflex has an exact fix-it; reverses only PL8 |
| 3 | S1 | the status quo; unary steps are 3 characters shorter, but H1-H3 and 3 more concepts |
| 4 | S3 | saves as much as S4 but adds syntax and a second `_` meaning (H6) |
| 5 | S6 | fewest rule exceptions (1), but the iterator reflex fails at 9 sites and the style splits (H11) |
| 6 | S5 | the most concepts (8); needs a new inference rule; every adapter returns a function, so H1 and H3 can be silent |

## Recommendation

**Recommendation.** Package S4: adapters stay methods, the pipe keeps
substitution steps only, and a leading-dot member path is the one function
shorthand.

1. **Methods (keep PL1, PL2, STDLIB 14).** The iterator reflex is the most
   common code in the corpus, and methods keep it working. They keep one
   call style across `Iterator`, `List`, `T?`, and `Result`, and `.`
   completion finds every adapter.
2. **Substitution steps only (reverse PL8).** Application steps save three
   characters per unary step: twelve in the whole corpus. They cost three
   concepts and hazards H1-H3. No `std` function in a methods package
   returns a function, so they serve only user code. P2 also restores
   PL7's own goal: the value's slot is always shown.
3. **`.name` member paths (new; PL10 untouched).** The shorthand needs no
   syntax and no `_`, so it never collides with the pipe, and it serves the
   most common callback shape: one member of the argument. It reuses the
   `.Variant` idea of "look the name up in the expected type".

It gives up the F# reflex `x |> f`, bare unary steps, and a shorthand for
arithmetic callbacks such as `fn(v): v * 2`. The member path adds no
syntax, so it can wait for [hd writing log](../audit/hd-writing-log.md)
evidence, as PL10 does for `fn: _`, without changing the rest.

**Runner-up: S2**, the same package without the member path. It is the
smallest design with methods, and S4 can be added to it later without
changing any valid program: `.name` with an expected function type is an
error today.

Decisions each package would reverse:

| Package | Reverses | Keeps |
| --- | --- | --- |
| S4 (recommended) | PL8 | PL1, PL2, PL5, PL6, PL7, PL9, PL10, STDLIB 14 |
| S2 (runner-up) | PL8 | as S4 |
| S1 | none | all |
| S3 | PL8, PL10, `fn.closure.no-other-syntax` | PL1, PL2, STDLIB 14 |
| S5 | STDLIB 14, PL1, PL2, `flow.adapter.*` | PL7, PL8 |
| S6 | STDLIB 14, PL1, PL2, PL8, `flow.adapter.*` | PL7 |

## Questions For The Owner

### CS1. Do iterator adapters stay methods?

Effect: moving them to `std.iter` removes PL1's rule, but the most common
reflex, `xs.iter().map(f)`, fails at nine corpus sites.

- **A.** Keep methods (PL1, PL2, STDLIB 14).
- **B.** Data-first `std.iter` functions (S6).
- **C.** Curried `std.iter` functions (S5).

**Recommendation:** A.

```text
fn names(users: List[User]) -> List[string]:
    users.iter().filter(fn(user): user.active).map(fn(user): user.name).collect()
```

### CS2. Does the pipe keep application steps?

Effect: `x |> f(y)` means `f(y)(x)` (H1), and a leading-dot line after
`|> f` attaches to `f` (H2). Dropping them costs `(_)` on unary steps.

- **A.** Drop them: every step has one `_` (PL7 without PL8).
- **B.** Keep PL8 as decided.
- **C.** Keep bare names only (`x |> f`), and make a call without `_`
  (`x |> f(y)`) an error.

**Recommendation:** A. C is the fallback if bare unary steps matter.

```text
fn slugify(value: string) -> string:
    value |> ascii_fold(_) |> collapse_dashes(_)  # hypothetical syntax
```

### CS3. Is `.name` with an expected function type a member-path function?

Effect: callbacks that read one member, such as `filter`, `map`,
`group_by`, and `and_then`, drop the `fn(x): x.` prefix: 8 of 12 corpus
closures.

- **A.** Yes, now.
- **B.** Yes, when the hd writing log shows demand, as PL10 does for
  `fn: _`.
- **C.** No.

**Recommendation:** A. B loses nothing if the owner prefers evidence
first.

```text
fn active_names(users: List[User]) -> List[string]:
    users.filter(.active).map(.name)
```

### CS4. How long is a member path?

Effect: a longer path covers more callbacks, but a reader must find where
it ends.

- **A.** The whole postfix chain: `.name.trim().lower()`, `.address.city`.
- **B.** Fields only, dotted: `.address.city`.
- **C.** One field: `.name`.

**Recommendation:** A, without `?` or `!`.

```text
fn headers(line: string) -> List[string]:
    line.split(",").map(.trim().lower())
```

### CS5. Do method values stay deferred?

Effect: `User::name` is Kotlin's spelling, but a field and a method may
share a name, and the member path covers the same use.

- **A.** Keep them deferred.
- **B.** Allow `Type::method` as a function value now.

**Recommendation:** A.

```text
fn domains(users: List[User]) -> List[string]:
    users.map(.domain())
```

### CS6. Does `std` add key-function variants?

Effect: `sorted_by` and `fold` take two parameters, so no shorthand
shortens a sort or a sum. Kotlin's `sortedByDescending` and `sumOf` take
one.

- **A.** Add `sorted_by_key` and a summing helper to `std` (library only).
- **B.** Keep two-parameter forms only.

**Recommendation:** A, after CS3.

```text
fn richest(totals: List[CustomerTotal]) -> List[CustomerTotal]:
    totals.sorted_by(fn(a, b): b.total.cmp(a.total))
```

## Sources

Languages:

- Elixir: [`&` capture](https://hexdocs.pm/elixir/Kernel.SpecialForms.html#&/1), [`|>`](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2), [Enum](https://hexdocs.pm/elixir/Enum.html), [Enumerables and Streams](https://hexdocs.pm/elixir/enumerable-and-streams.html), [`File.stream!/3`](https://hexdocs.pm/elixir/File.html#stream!/3)
- Gleam: [pipelines](https://tour.gleam.run/functions/pipelines/), [function captures](https://tour.gleam.run/functions/function-captures/), [gleam/list](https://hexdocs.pm/gleam_stdlib/gleam/list.html)
- F#: [What's new in F# 8](https://learn.microsoft.com/en-us/dotnet/fsharp/whats-new/fsharp-8), [List module](https://fsharp.github.io/fsharp-core-docs/reference/fsharp-collections-listmodule.html)
- Kotlin: [lambdas and `it`](https://kotlinlang.org/docs/lambdas.html#it-implicit-name-of-a-single-parameter), [callable references](https://kotlinlang.org/docs/reflection.html#callable-references), [extensions](https://kotlinlang.org/docs/extensions.html), [grouping](https://kotlinlang.org/docs/collection-grouping.html), [aggregate operations](https://kotlinlang.org/docs/collection-aggregate.html)
- Swift: [closures](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/), [SE-0249](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0249-key-path-literal-function-expressions.md), [optional chaining](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/optionalchaining/)
- Scala: [anonymous functions](https://docs.scala-lang.org/scala3/book/fun-anonymous-functions.html), [wildcard arguments in types](https://docs.scala-lang.org/scala3/reference/changed-features/wildcards.html), [placeholder syntax](https://scala-lang.org/files/archive/spec/2.13/06-expressions.html#placeholder-syntax-for-anonymous-functions), [`ChainingOps`](https://www.scala-lang.org/api/current/scala/util/ChainingOps.html)
- Rust: [`Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html), [closures](https://doc.rust-lang.org/book/ch13-01-closures.html), [path expressions](https://doc.rust-lang.org/reference/expressions/path-expr.html), [`Option::and_then`](https://doc.rust-lang.org/std/option/enum.Option.html#method.and_then), [`map` for Result](https://doc.rust-lang.org/rust-by-example/error/result/result_map.html), [Rust Cookbook CSV](https://rust-lang-nursery.github.io/rust-cookbook/encoding/csv.html)
- JavaScript: [TC39 pipeline proposal](https://github.com/tc39/proposal-pipeline-operator), [HISTORY.md](https://github.com/tc39/proposal-pipeline-operator/blob/main/HISTORY.md)
- Elm: [records](https://elm-lang.org/docs/records)
- Ruby: [`Symbol#to_proc`](https://docs.ruby-lang.org/en/3.3/Symbol.html#method-i-to_proc), [Ruby 3.4 release](https://www.ruby-lang.org/en/news/2024/12/25/ruby-3-4-0-released/)
- Hack: [pipe operator](https://docs.hhvm.com/hack/expressions-and-operators/pipe)

Corpus:

- axum: [extractors](https://docs.rs/axum/latest/axum/extract/index.html)
- reqwest: [crate docs](https://docs.rs/reqwest/latest/reqwest/), [`ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html); [reqwest-middleware](https://docs.rs/reqwest-middleware/latest/reqwest_middleware/)
- Django: [`django/utils/text.py`](https://github.com/django/django/blob/main/django/utils/text.py)

## Parse Log

Every `text` block was parsed with the reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)) on
2026-09-29. Parsing checks syntax only; no block is claimed to type-check.

The parser stops at the first error, and it knows neither `|>` nor `fn:`.
So each failing block was parsed a second time, **desugared**: every line
starting with `|>` joined to the line before, `|>` replaced by `|`, `fn: `
by `fn(p_): `, and every expression `_` by a name. Every desugared block
parses, so the rest of each block is valid syntax. Every first error falls
on a line marked `# hypothetical syntax`.

The S4 member paths parse today; they need the new rule to type-check.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Shared Declarations | parses |
| 2 | Shared Declarations | parses |
| 3 | Adapters As Methods (S1-S4) | parses |
| 4 | Adapters As Data-First Functions (S6) | parses |
| 5 | Adapters As Curried Functions (S5) | parses |
| 6 | C1: ETL Pipeline, S1, S2 | parses |
| 7 | C1: ETL Pipeline, S3 | `syntax-error` at line 3, marked; desugared: parses |
| 8 | C1: ETL Pipeline, S4 | parses |
| 9 | C1: ETL Pipeline, S5 | `syntax-error` at line 5, marked; desugared: parses |
| 10 | C1: ETL Pipeline, S6 | `syntax-error` at line 5, marked; desugared: parses |
| 11 | C2: Web Handler With `!` And `?`, S1, S5 | `syntax-error` at line 9, marked; desugared: parses |
| 12 | C2: Web Handler With `!` And `?`, S2, S6 | `syntax-error` at line 9, marked; desugared: parses |
| 13 | C2: Web Handler With `!` And `?`, S3 | `syntax-error` at line 4, marked; desugared: parses |
| 14 | C2: Web Handler With `!` And `?`, S4 | `syntax-error` at line 9, marked; desugared: parses |
| 15 | C3: String Processing, S1, S5 | `syntax-error` at line 3, marked; desugared: parses |
| 16 | C3: String Processing, S2, S6 | `syntax-error` at line 3, marked; desugared: parses |
| 17 | C3: String Processing, S3 | `syntax-error` at line 3, marked; desugared: parses |
| 18 | C3: String Processing, S4 | `syntax-error` at line 3, marked; desugared: parses |
| 19 | C4: Collection Report, S1, S2 | parses |
| 20 | C4: Collection Report, S3 | `syntax-error` at line 2, marked; desugared: parses |
| 21 | C4: Collection Report, S4 | parses |
| 22 | C4: Collection Report, S5 | `syntax-error` at line 6, marked; desugared: parses |
| 23 | C4: Collection Report, S6 | `syntax-error` at line 6, marked; desugared: parses |
| 24 | C5: Option And Result Chains, S1, S2, S5, S6 | parses |
| 25 | C5: Option And Result Chains, S3 | `syntax-error` at line 2, marked; desugared: parses |
| 26 | C5: Option And Result Chains, S4 | parses |
| 27 | C6: Builder, S1, S5 | `syntax-error` at line 8, marked; desugared: parses |
| 28 | C6: Builder, S2, S3, S4, S6 | `syntax-error` at line 8, marked; desugared: parses |
| 29 | Hazards Found, H2 | `syntax-error` at line 3, marked; desugared: parses |
| 30 | Hazards Found, H6 | `syntax-error` at line 2, marked; desugared: parses |
| 31 | Hazards Found, H9 | parses |
| 32 | Hazards Found, H10 | parses |
| 33 | CS1. Do iterator adapters stay methods? | parses |
| 34 | CS2. Does the pipe keep application steps? | `syntax-error` at line 2, marked; desugared: parses |
| 35 | CS3. Is `.name` with an expected function type a member-path function? | parses |
| 36 | CS4. How long is a member path? | parses |
| 37 | CS5. Do method values stay deferred? | parses |
| 38 | CS6. Does `std` add key-function variants? | parses |
