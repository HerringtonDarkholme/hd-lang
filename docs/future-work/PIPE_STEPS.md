# Pipe Steps Without Placeholders

Design review, 2026-10-24; nothing here is accepted behavior.
This report changes no specification, fixture, standard library, or compiler code.
It stress-tests [#818](https://github.com/HerringtonDarkholme/hd-lang/issues/818), before the [#783 policy rewrite](https://github.com/HerringtonDarkholme/hd-lang/issues/783).

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Proposed Rule Text](#proposed-rule-text)
- [Cases](#cases)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Soundness And Implementation](#soundness-and-implementation)
- [Problems, Ranked](#problems-ranked)
- [Executable Policy Control](#executable-policy-control)
- [Parse Log](#parse-log)
- [Questions For The Owner](#questions-for-the-owner)

## Surface Being Tested

The candidate evaluates a zero-placeholder step as an expression, then calls its value with the piped value.
Thus `a |> other(ss)` calls `other(ss)(a)`, never `other(ss, a)`.
The input still evaluates first, even though that parenthesized spelling ordinarily evaluates the callee first.
A configuration factory must actually return a function; the pipe adds no automatic currying.

The relevant language rules are [Pipe Expressions](../../spec/lang/05-expressions.md#pipe-expressions), [function values](../../spec/lang/07-functions.md#function-types-and-values), and [method references](../../spec/lang/07-functions.md#method-references).
Also relevant are [generic function values](../../spec/lang/07-functions.md#generic-function-values), [suspending function values](../../spec/lang/07-functions.md#suspending-function-values), and [requirements](../../spec/lang/11-requirements-and-suspension.md).
The library cases use [iterators](../../spec/std/iter.md), [text parsing](../../spec/std/text.md), and [tasks](../../spec/std/task.md).

| Variant | Meaning | Most expensive change |
| --- | --- | --- |
| V0, current rules | A bare name/path calls its declaration; other steps need `_`. | None. Curried factories can already be bound to names. |
| V1, standalone values | Every zero-slot step is a unary ordinary function expression, including existing names and dotted paths. | Language semantic changes; no grammar or intrinsic addition. |
| V2, compatible extension | Preserve existing bare-path calls; evaluate other zero-slot expressions as V1 values. | Language semantic changes plus one path-versus-expression exception. |

V1 is the issue's uniform proposal, tested literally.
V2 is a comparison, not a quiet reinterpretation of V1.
Neither variant adds general partial application, a second trailing-block form, or implicit bang execution.
The existing one-slot substitution rules and layout limits remain.

## Method

Eighteen cases cover the requested suffixes, effects, inference, foreign pipelines, and three fixed policy programs.
Verdicts describe expressiveness and observable behavior under each candidate.
**Works** means the candidate expresses the case; **friction** needs a workaround or visible API change; **breaks** changes an existing successful program.
A proposed program parsing does not establish that today's checker accepts it.

The source baseline is `424c34c444c7325483c14a3520be3f2f118ff99d`.
A fresh dev CLI was built there with native-engine support; the source-matched parser has unchanged parser dependencies from `aafe3fe75`.
Both Node and Wasmtime execute the ordinary controls.
No implementation of the proposed pipe rule is hidden in these measurements.

An ordinary helper `feed(value, step)` models the expected unary type and evaluation order.
It checks and calls `step: fn(A) -> B $ R` after evaluating `value`.
It deliberately does **not** implement the proposed ban on implicitly weakening a final `fn!` value.
Case C10 demonstrates why that extra guard matters.

Line counts include nonempty, noncomment source lines in each fixed port body.
Markers count standalone expression `_` tokens, excluding identifier underscores, patterns outside those bodies, and type names.
API implementation lines are counted separately, including signatures and closure bodies.
No runtime wall time or allocation claim follows from a line count.

## Summary

| Case | Library or operation | V1 | V2 | Main finding |
| --- | --- | --- | --- | --- |
| C1 | Defaulted bare function | breaks | works | Function values carry full arity, without declaration defaults. |
| C2 | Curried policy factory | works | works | Inner type infers from the expected unary input. |
| C3 | Functions as pipeline data | works | works | `compose(double)` receives the function, not an extra source argument. |
| C4 | Uncurried `scale(3)` | friction | friction | Remains an arity error; suggest a source-specific placeholder. |
| C5 | Method path and stored function | breaks | works | A dotted path can silently switch from method to field. |
| C6 | Indexed function | works | works | Index once, then invoke its function value. |
| C7 | Fallible function lookup | works | works | `?` propagates lookup failure before invocation. |
| C8 | Generic values and nested rows | friction | friction | Ordinary type inference works; nonempty returned rows have a current gap. |
| C9 | Closures and nested placeholders | works | works | Ordinary closure typing; existing placeholder ownership stays. |
| C10 | Final suspending function | friction | friction | Reject before automatic constructor weakening. |
| C11 | Explicit suspending factory | works | works | The explicit `!` computes an ordinary function. |
| C12 | Multiline chains | friction | friction | Leading pipes work; indented step suites still fail. |
| C13 | Ordering, references, cleanup | works | works | Preserve first input evaluation and its access. |
| C14 | F# list pipeline | friction | friction | Explicit curried wrappers are required around current hd adapters. |
| C15 | OCaml map and reduction | friction | friction | Function application transfers; automatic currying does not. |
| C16 | Elixir and Gleam pipelines | friction | friction | Their call steps insert the input first; this proposal does not. |
| C17 | Three fixed policy ports | works | works | Unary models and explicit-slot controls run identically. |
| C18 | Parsers and config builders | works | works | Input and result context can solve separate generic parameters. |

The proposal supports useful factories without a new grammar form.
Its chief risk is semantic migration, especially a successful dotted step that calls different code.
Scalar policy inference works today; the separate nested-row counterexample prevents claiming universal inference.
Removing placeholders saves markers in these ports, not source lines.

## Proposed Rule Text

These are **candidate language-tier rules**, written in [spec style](../../spec/STYLE.md).
They specify V1 completely enough to review, but are not installed rules.
The IDs below are new; existing meanings do not retain their IDs.

1. r[expr.pipe.value-step.kinds] A step with one own `_` is a substitution step, and a step with none is a value step.
2. r[expr.pipe.value-step.result] A pipe expression has the type and value produced by evaluating its substitution step or invoking its value step.
3. r[expr.pipe.value-step.form] A value step may be any expression permitted at the step's precedence and by the pipe layout rules.
4. r[expr.pipe.value-step.expected] A value step is checked against an ordinary unary function type whose input is the piped value's type.
5. r[expr.pipe.value-step.context] The expected pipe result type, when available, supplies the expected result type of that unary function.
6. r[expr.pipe.value-step.evaluate] After evaluating the piped value, a value step's expression is evaluated exactly once.
7. r[expr.pipe.value-step.invoke] The resulting function is called once with the saved piped value as its positional argument.
8. r[expr.pipe.value-step.arity] A value step's function must accept exactly one argument through its function type. Error: `argument-count`.
9. r[expr.pipe.value-step.no-insertion] A value step never inserts the piped value into a call inside its expression.
10. r[expr.pipe.value-step.member] Member access in a value step has its ordinary expression meaning, including a dotted field read and a `::` method reference.
11. r[expr.pipe.value-step.no-suspend] A value step whose inferred value is a suspending function is an error before constructor weakening. Error: `suspending-pipe-step`.
12. r[expr.pipe.value-step.effects] Calls and propagation inside the step expression execute in the enclosing scope under their ordinary requirement and suspension rules.
13. r[expr.pipe.value-step.not-callable] A value step whose expression produces no function value is an error. Error: `not-callable`.
14. r[expr.pipe.value-step.input-access] The saved piped value retains its type and access for argument passing under the ordinary call rules.

> **Why.** The zero-slot step computes the operation; the one-slot step writes the operation with its input position visible.
> Neither form guesses a missing argument or creates a reusable partial application.

`expr.pipe.order` continues to require first, exactly-once input evaluation.
The existing duplicate, closure, and innermost-placeholder rules still select or reject substitution steps.
The existing single-line, continuation, and no-trailing-block rules still constrain both kinds.
`expr.pipe.no-partial` remains: writing `_` outside a pipe does not create a callback.

Defaults belong to declarations, not function types, under [Function Types](../../spec/lang/07-functions.md#function-types-and-values).
Consequently V1 deliberately breaks C1 rather than silently inventing default-arity adapters.
The same distinction makes `user.greet` a field expression under V1; `user::greet` is the bound method value.
An explicit ordinary function returning a cold suspension is still a valid unary step.

### Rule Inventory

| Action | IDs | Count |
| --- | --- | --- |
| Retire | `expr.pipe.step-kinds`, `expr.pipe.value` | 2 |
| Retire | `expr.pipe.bare.form`, `.call`, `.method`, `.no-suspend`, `.needs-placeholder`, `.needs-placeholder.forms`, `.method-reference` | 7 |
| Add | The fourteen fully written `expr.pipe.value-step.*` rules above | 14 |
| Preserve | Form, association, precedence, order, seven slot rules, four layout rules, three outside-placeholder rules | 18 |

The proposed net change is five rule IDs; it is not a rewrite reusing changed IDs.
Both V1 and V2 retire the only current pipe rejection using `pipe-step-needs-placeholder`.
A later implementation needs an explicit diagnostic retirement/migration decision, fixture migration, and an inventory check.
This report changes no diagnostic registration or conformance expectation.

V2's exact semantic delta is one dispatch exception: preserve current bare-path declaration calls, including defaults and method lookup.
Only other zero-slot expressions follow rules 3–14 above.
Its inventory must use distinct new IDs for the mixed classification, result rule, and preserved-path exception.
V2 retains the seven old bare rules only where their meanings truly remain; it retires both obsolete needs-placeholder rules.
This costs a permanent difference between `user.greet` and a parenthesized field expression.

## Cases

### C1. A Defaulted Bare Function

The current declaration call prints `hd!` on both engines.
Turning `greet` into an ordinary unary value rejects it: its function type has two parameters.
That is a V1 compatibility break, even without any new suffix syntax.
V2 preserves the existing direct declaration call.

```text
fn greet(name: string, suffix: string = "!") -> string: name + suffix
pub fn main() -> void $ Console:
    println("hd" |> greet)
```

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn greet(name: string, suffix: string = "!") -> string: name + suffix
pub fn main() -> void $ Console:
    println(feed("hd", greet))
```

The second program fails with `type-mismatch`: `in argument: expected fn(string) -> ?, found fn(string, string) -> string`.
An explicit `greet(_, suffix="!")` or a unary closure preserves the behavior.
Automatically manufacturing a defaulted adapter would add a separate function-value rule.

### C2. A Curried Builder Gets Its Input Type

The helper supplies the proposed `fn(typeof lhs) -> _` context without adding a `typeof` language operator.
This complete control checks cleanly and prints `7/50` on both engines.
No explicit `Inner` appears at the use site.

```text
struct Limit:
    millis: usize

fn with_timeout[Inner](millis: usize) -> (fn(Inner) -> (Inner, Limit)):
    fn(inner: Inner) -> (Inner, Limit):
        (inner, Limit { millis: millis })

fn feed[A, B](value: A, step: fn(A) -> B) -> B: step(value)

pub fn main() -> void $ Console:
    paired := feed(+7, with_timeout(50))
    println("${paired._0}/${paired._1.millis}")
```

`with_timeout(50)` is one ordinary call returning another function.
The inner type occurs in that returned function's input, so expected-type inference can solve it.
The factory's parameter default, when present, is used by this ordinary declaration call.
It does not become a default on the returned function.

V0 also works after binding the factory result: the existing `+7 |> step` control prints `7/50`.
That smallest option adds one binding per configured operation and changes no pipe semantics.

### C3. The Piped Value Can Itself Be A Function

The following executable control also covers C5, C6, C8, and C9's simple positive forms.
Its output is `7`, `8`, `6`, `9`, `6`, `hi hd`, and `4` on both engines.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn inc(value: i32) -> i32: value + 1
fn double(value: i32) -> i32: value * 2
fn add(by: i32) -> (fn(i32) -> i32):
    fn(value: i32) -> i32: value + by
fn identity[T](value: T) -> T: value
fn compose(outer: fn(i32) -> i32) -> (fn(fn(i32) -> i32) -> (fn(i32) -> i32)):
    fn(inner: fn(i32) -> i32) -> (fn(i32) -> i32):
        fn(value: i32) -> i32: outer(inner(value))
struct Config:
    prefix: string
impl Config:
    fn greet(&self, name: string) -> string: self.prefix + name
    fn parse(text: string) -> usize: text.len()
pub fn main() -> void $ Console:
    println(feed(+4, add(+3)))
    composed := feed(inc, compose(double))
    println(composed(+3))
    functions := [double, inc]
    println(feed(+3, functions[0]))
    println(feed(+9, identity::[i32]))
    println(feed(+5, fn(value): value + 1))
    user := Config { prefix: "hi " }
    println(feed("hd", user::greet))
    println(feed("word", Config::parse))
```

The proposed `inc |> compose(double)` calls the unary result of `compose(double)` with `inc`.
It creates a composed function; it never calls `compose(double, inc)`.
Generic references remain monomorphic after use-site instantiation.
This introduces neither higher-rank function values nor signature inference from a function body.

The corresponding candidate spellings below parse, but today reject the new zero-slot suffix steps.
The dotted method line retains current behavior and is assessed separately in C5.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn inc(value: i32) -> i32: value + 1
fn double(value: i32) -> i32: value * 2
fn add(by: i32) -> (fn(i32) -> i32):
    fn(value: i32) -> i32: value + by
fn identity[T](value: T) -> T: value
fn compose(outer: fn(i32) -> i32) -> (fn(fn(i32) -> i32) -> (fn(i32) -> i32)):
    fn(inner: fn(i32) -> i32) -> (fn(i32) -> i32):
        fn(value: i32) -> i32: outer(inner(value))
struct Config:
    prefix: string
impl Config:
    fn greet(&self, name: string) -> string: self.prefix + name
    fn parse(text: string) -> usize: text.len()
pub fn main() -> void $ Console:
    println((+4 |> add(+3)))
    composed := (inc |> compose(double))
    println(composed(+3))
    functions := [double, inc]
    println((+3 |> functions[0]))
    println((+9 |> identity::[i32]))
    println((+5 |> (fn(value): value + 1)))
    user := Config { prefix: "hi " }
    println(("hd" |> user.greet))
    println(feed("word", Config::parse))
```

### C4. The Uncurried Mistake Needs A Targeted Diagnostic

For `fn scale(by: i32, value: i32) -> i32`, `x |> scale(3)` still lacks an argument to `scale`.
Current pipes reject the suffix with `pipe-step-needs-placeholder` before inspecting its call.
The executable unary model instead reports `argument-count`, followed by a less useful function-type mismatch.

| Situation | Current or modeled diagnostic | Proposed diagnostic behavior |
| --- | --- | --- |
| `x |> scale(3)` | Current: `write '_' where the piped value goes` with backticked `_`. | Report `scale`'s arity once; suggest `scale(3, _)`. |
| `x |> scale(3)` through `feed` | `argument-count`: `` `scale` takes 2 arguments ``; then `expected fn(?) -> ?, found i32`. | Suppress the secondary noncallable/type cascade. |
| A call returns a number | Existing `not-callable` or `type-mismatch`. | Explain that a zero-slot step must produce a unary function. |
| A final `fn!` value | `suspending-pipe-step`. | Keep the explicit `!`/`_` advice. |
| Unsolved factory generic | `cannot-infer-type`. | Point at the factory/type argument, not a nonexistent pipe slot. |

The suggested replacement must be assembled from the actual callee, argument list, and missing parameter position.
Offer it only for one missing final required positional argument, with no spread or named/default ambiguity.
For `gain(17)`, the suggestion is `gain(17, _)`, never the issue's literal `scale(3, _)`.
The edit is a suggestion about intent, not an automatically safe semantic fix.

Appending `_` is wrong when a valid factory already returns a function.
No fallback should reinterpret a successful factory call according to arity.
The following whole-file probe parses and receives the current pipe rejection.

```text
fn scale(by: i32, value: i32) -> i32: value * by
pub fn main() -> void $ Console:
    println(+4 |> scale(3))
```

### C5. Method Lookup Can Silently Become A Field Read

The same spelling selects different code, even when both forms check.
This complete program prints `method hd`, `field hd`, `method hd` on both engines.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

struct User:
    greet: fn(string) -> string
impl User:
    fn greet(&self, value: string) -> string: "method " + value
pub fn main() -> void $ Console:
    user := User { greet: fn(value: string) -> string: "field " + value }
    println("hd" |> user.greet)
    println(feed("hd", user.greet))
    println(feed("hd", user::greet))
```

The first line uses current pipe method-call lowering.
The second models V1's ordinary dotted field expression.
The third explicitly chooses the bound method reference, preserving the first result.
V1 therefore breaks behavior silently; V2 keeps the first line's existing meaning.

Without a callable field, the standalone dotted expression fails `unknown-field`: `` no field `greet` on Config ``.
That diagnostic is honest but does not explain migration from a formerly valid pipe method.
A migration hint should offer the actual `user::greet` source spelling when that method exists.
`Config::parse` remains an unbound associated function reference, demonstrated in C3.

### C6. Indexed Functions Are Evaluated Once

C3's `functions[0]` model prints `6`.
The proposed spelling is `+3 |> functions[0]`.
Ordinary bounds checks happen after evaluating the input and before calling the selected function.
No rule interprets the index as an argument position.

A factory returning an `Apply` object does not qualify as a unary function.
Current [callable values](../../spec/lang/05-expressions.md#callable-values) accept zero arguments only; the proposal does not extend that trait.

### C7. Propagation Belongs To Lookup Or To Invocation, As Written

The complete model prints `lhs`, `Result.Ok(4)`, `lhs`, and `Result.Err("missing")`.
The candidate `+3 |> candidate?` first obtains a function or propagates the lookup error.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn inc(value: i32) -> i32: value + 1
fn apply_optional!(present: bool) -> Result[i32, string] $ Console:
    println("lhs")
    let candidate: Result[fn(i32) -> i32, string] = if present: .Ok(inc) else: .Err("missing")
    .Ok(feed(+3, candidate?))
pub fn main!() -> void $ Console:
    println(debug(apply_optional!(true)))
    println(debug(apply_optional!(false)))
```

By contrast, `x |> parse(_)?` propagates the result of calling `parse`.
The proposal must not move `?` into a synthetic closure with a different propagation target.
Result conversion, cleanup, and permission checks remain the enclosing function's ordinary rules.
The current `propagate-proposed` repro parses, then fails the existing placeholder requirement.

### C8. Generic Inference Works For Types, With A Nested-Row Gap

Explicit type arguments such as `identity::[i32]` work in C3's expected unary model.
An inline generic reference can also instantiate from that expected input and result type.
A factory's type parameter absent from both remains unconstrained, as expected.

The `manufacture[Inner, Hidden]` probe returns `fn(Inner) -> List[Hidden]`.
Without result context it fails `cannot-infer-type`: `annotate this value's type`.
An outer `List[string]` annotation solves `Hidden`, while the input solves `Inner`.

A separate, smaller example fails with a nonempty row inside the returned function type.
It needs no suspension, policy trait, associated type, or new pipe syntax.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn keep[$R]() -> (fn(fn() -> void $ R) -> (fn() -> void $ R)):
    fn(work: fn() -> void $ R) -> (fn() -> void $ R): work
fn hello() -> void $ Console: println("hello")
pub fn main() -> void $ Console:
    wrapped := feed(hello, keep())
    wrapped()
```

Exact current diagnostic:

```console
type-mismatch: in argument: expected fn(fn() -> () $ Console) -> fn() -> () $ ?, found fn(fn() -> ()) -> fn() -> (); permissions inside a type convert only by declared variance, and a function type inside a type keeps its row
```

The same program with `keep::[$ Console]()` checks and prints `hello` on both engines.
An empty-row `hello` control also checks and runs.
A suspending-work wrapper reproduces the same distinction; explicit `delayed::[usize, $ Console]` works and preserves `Clock` in the result.

This is a current inference gap, not evidence that the proposed pipe inherently needs row erasure.
Inspection suggests that a factory's unresolved row becomes empty before outer nested-function context solves it.
That is a source-based hypothesis; this docs-only job neither diagnoses every inference path nor fixes one.
Do not weaken the nested-row invariance rule to make this example pass.

### C9. Closures And Placeholder Ownership

C3's same-line closure uses its expected input type and prints `6`.
A parenthesized same-line closure can therefore be a value step without giving `_` a callback meaning.
The existing innermost-pipe ownership rule remains observable in this control, which prints `8` twice.

```text
fn add(a: i32, b: i32) -> i32: a + b
fn inc(value: i32) -> i32: value + 1
pub fn main() -> void $ Console:
    println(+3 |> add(_, +4 |> inc))
    println(+3 |> add(_, +4 |> inc(_)))
```

A `_` captured inside a nested closure still fails `pipe-placeholder-in-closure`.
Two own placeholders still fail `duplicate-pipe-placeholder`.
An inner pipe's slot never changes whether an outer step is a zero-slot value step.
Neither variant permits `format(_, 8)` as a free-standing partial application.

### C10. A Final Bang Function Must Not Weaken Silently

Today `+3 |> load` fails `suspending-pipe-step` when `load` is suspending.
But ordinary function typing can weaken `fn!` into a cold-constructor function.
This clean control prints `4` on both engines:

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn load!(value: i32) -> i32: value + 1
pub fn main!() -> void $ Console:
    let job: mut Suspend[i32] = feed(+3, load)
    println(job!())
```

Therefore checking only against `fn(A) -> B` would accept more than the intended explicit-bang rule.
The candidate rule rejects a final inferred `fn!` value before that weakening.
Use `x |> load!(_)` to execute work now.
An explicitly supplied ordinary constructor function may still return `mut Suspend[T]`; constructing it does not run the work.

For `x |> retry(b)`, inspect the actual return type of `retry(b)`.
If it is `fn!(A) -> T`, reject the step under this candidate.
If it is ordinary `fn(Policy) -> Stacked`, compose policy configuration normally.
If it is a nonfunction policy value, report that it cannot be called with the input.

### C11. An Explicit Bang May Compute The Callable

The step expression may itself suspend visibly, then produce an ordinary callable.
This control prints `4` twice, comparing that factory with today's explicit substitution call.

```text
use std.time.{Clock, ms, sleep}

fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn factory!() -> (fn(i32) -> i32) $ Clock:
    sleep!(ms(1))
    fn(value: i32) -> i32: value + 1
pub fn main!() -> void $ Console + Clock:
    println(feed(+3, factory!()))
    println(+3 |> load!(_))
fn load!(value: i32) -> i32: value + 1
```

The proposed `+3 |> factory!()` requires a suspending enclosing body and `Clock`.
It first evaluates `+3`, then awaits `factory!()`, then calls the returned ordinary function.
That differs from silently bang-calling the function returned by a plain factory.
Rows must include both factory evaluation and final invocation.

### C12. Multiline Chains Do Not Mean Multiline Steps

The candidate `retry(backoff)` chain can put each `|>` on a separate leading-pipe line.
Its two suffix steps parse today but fail `pipe-step-needs-placeholder`.
No additional indentation grammar is necessary.

```text
fn add(by: i32) -> (fn(i32) -> i32):
    fn(value: i32) -> i32: value + by
pub fn main() -> void $ Console:
    value := +3
        |> add(+1)
        |> add(+2)
    println(value)
```

The existing `multi-line-pipe-step` control still rejects an indented closure suite.
The exact diagnostic is `a pipe step must not contain an indented suite`.
For such a callback, bind a name or extract a function before constructing the chain.
Trailing blocks remain complete statements or binding right-hand sides, not pipe-step suffixes.

### C13. Evaluation Order And Reference Access

This ordinary model prints `4` then `lhs,factory,call` on both engines.
It exposes a wrong textual lowering that would evaluate the factory before the input.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn left(log: &mut List[string]) -> i32:
    log.push("lhs")
    3
fn factory(log: &mut List[string]) -> (fn(i32) -> i32):
    log.push("factory")
    fn(value: i32) -> i32:
        log.push("call")
        value + 1
pub fn main() -> void $ Console:
    let mut log: List[string] = []
    println(feed(left(&mut log), factory(&mut log)))
    println(log.join(","))
```

Lowering needs an evaluated input and a saved callable, then an ordinary call.
The input's original access must survive; a synthetic immutable value binding must not remove writable access.
Explicit `&mut` input works in both today's pipe and the ordinary value model.
Both controls increment the original state and print `4` on both engines.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

struct State:
    count: i32
fn advance(state: &mut State) -> void: state.count += 1
pub fn main() -> void $ Console:
    let mut state = State { count: +3 }
    feed(&mut state, advance)
    println(state.count)
```

Neither current pipe nor unary model automatically borrows a list for an ordinary `fn(&List[i32])` parameter.
Both negative controls reject passing `List[i32]` without `&`.
Method receiver auto-addressing remains separate, through `receiver::method` under V1.

The saved input must remain live across a factory's suspension or propagation.
Existing ownership and unwind paths must release it if the factory fails before invocation.
The policy port checks cancellation cleanup, but physical resource-release behavior for every new lowering still needs implementation regressions.
This report does not claim that ordinary helper calls prove an unimplemented lowering's cleanup correctness.

### C14. F# Filtering And Mapping

Microsoft's [F# functions guide](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/functions/) supplies the odd-value square-and-add pipeline.
The abbreviated source shape below names its two callbacks.
F# currying makes each configured list operation a function.

```fsharp
values |> List.filter odd |> List.map squareAndAddOne
```

The hd translation uses lazy iterator adapters and an explicit collection boundary.
The complete expected-type control prints `[2, 10, 26]` on both engines.

```text
use std.iter.{Iterable, Iterator, Filter, MapIter}

fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn filter[T, I < Iterator[T]](predicate: fn(T) -> bool) -> (fn(I) -> Filter[I, T]):
    fn(source: I) -> Filter[I, T]: source.filter(predicate)
fn map[T, U, I < Iterator[T]](transform: fn(T) -> U) -> (fn(I) -> MapIter[I, T, U]):
    fn(source: I) -> MapIter[I, T, U]: source.map(transform)
fn odd(value: i32) -> bool: value % 2 != 0
fn sq_plus_one(value: i32) -> i32: value * value + 1
pub fn main() -> void $ Console:
    xs := [+1, +2, +3, +4, +5]
    let result: List[i32] = feed(feed(xs.iter(), filter(odd)), map(sq_plus_one)).collect()
    println(debug(result))
```

Its candidate surface is `xs.iter() |> filter(odd) |> map(sq_plus_one)` followed by collection.
Today's equivalent calls the methods with `_`, or binds the curried functions first.
The pipe proposal alone does not change std's receiver-first adapter APIs.

The hd control delays callback execution until `collect`; F#'s `List` operations eagerly construct intermediate lists.
That timing difference matters for side effects and early termination.
An eager hd list-wrapper implementation can preserve that behavior, at the cost of intermediate collections.
The example's pure callbacks make its values comparable without claiming identical allocation behavior.

### C15. OCaml Mapping And Reduction

The [CS3110 textbook's own pipeline](https://cs3110.github.io/textbook/chapters/hop/pipelining.html) maps squares, then reduces their sum.
The abbreviated original omits its range-construction helper:

```ocaml
let sum_sq n = 0 -- n |> List.map square |> sum
```

The eager hd translation uses the same numeric input and operation order.
Its complete executable control prints `14` on both engines.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn flatten(values: List[List[i32]]) -> List[i32]:
    let mut result: List[i32] = []
    for group in values:
        for value in group: result.push(value)
    result
fn map_list[T, U](transform: fn(T) -> U) -> (fn(List[T]) -> List[U]):
    fn(values: List[T]) -> List[U]:
        let mut result: List[U] = []
        for value in values: result.push(transform(value))
        result
fn twice(value: i32) -> i32: value * 2
fn result_map[A, B, E](transform: fn(A) -> B) -> (fn(Result[A, E]) -> Result[B, E]):
    fn(result: Result[A, E]) -> Result[B, E]: result.map(transform)
fn magnitude(value: i32) -> i32:
    if value < 0: -value else: value
fn square(value: i32) -> i32: value * value
fn sum(values: List[i32]) -> i32:
    let result = +0
    for value in values: result += value
    result
pub fn main() -> void $ Console:
    println(feed([+0, +1, +2, +3], map_list(square)) |> sum)
```

The proposal changes its modeled `feed([+0, +1, +2, +3], map_list(square))` into a value step.
The final bare `sum` is unchanged.
The [OCaml guide](https://ocaml.org/docs/higher-order-functions) explains that ordinary currying supplies the remaining argument.
hd instead needs the written factory return type and closure body shown here.

### C16. Elixir And Gleam Read Calls Differently

The [Elixir Kernel documentation](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2) shows flattening followed by mapping:

```elixir
[1, [2], 3] |> List.flatten() |> Enum.map(fn x -> x * 2 end)
```

Its pipe inserts the input as the first argument of each call.
Under V1, `flatten()` must first return a function; the existing unary `flatten` declaration does not do that.
A direct translation keeps explicit `_`, or changes the API to an actual factory.
The hd control uses homogeneous nested lists because hd cannot express that heterogeneous Elixir list type.

The [Gleam tour's executable example](https://tour.gleam.run/everything/) includes:

```gleam
int.parse("-1234") |> result.map(int.absolute_value)
```

Current [Gleam pipeline documentation](https://tour.gleam.run/functions/pipelines/) also specifies first-argument insertion for call steps.
The hd equivalent needs a curried result-map factory or an explicit receiver/placeholder call.
This control prints `[2, 4, 6]` and `Result.Ok(1234)` on both engines.

```text
fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn flatten(values: List[List[i32]]) -> List[i32]:
    let mut result: List[i32] = []
    for group in values:
        for value in group: result.push(value)
    result
fn map_list[T, U](transform: fn(T) -> U) -> (fn(List[T]) -> List[U]):
    fn(values: List[T]) -> List[U]:
        let mut result: List[U] = []
        for value in values: result.push(transform(value))
        result
fn twice(value: i32) -> i32: value * 2
fn result_map[A, B, E](transform: fn(A) -> B) -> (fn(Result[A, E]) -> Result[B, E]):
    fn(result: Result[A, E]) -> Result[B, E]: result.map(transform)
fn magnitude(value: i32) -> i32:
    if value < 0: -value else: value
pub fn main() -> void $ Console:
    xs := [[+1], [+2], [+3]]
    println(debug(feed(xs |> flatten, map_list(twice))))
    println(debug(feed("-1234".parse::[i32](), result_map(magnitude))))
```

The longer Gleam example uses `int.remainder(_, 42)` as a callback capture.
That is not hd's `_`: hd's placeholder only substitutes the nearest enclosing pipe input.
A direct textual port would target the wrong value or fail, so write an ordinary hd closure.
Use source-specific hints for missing arguments; do not import Elixir insertion as an error-recovery fallback.

### C17. The Three Fixed #783 Programs

The control uses #783's current policy-values direction: ordinary configuration values, tuple composition, one `apply!` block, and a generic `Layer` implementation.
It preserves typed `From[TimedOut]` conversion and adds `Clock` only to timed work.
The breaker keeps shared state across four applications; hedging retains cancellation cleanup.
No new std API or policy design is accepted here.

Three versions ran on both engines: original uncurried factories with `_`, curried factories invoked with `(_)`, and curried factories through expected-type `feed`.
All six runs have identical complete stdout and pass the ports' assertions.
The assertions check two cancelled primary replies, the breaker's stopped calls, one hedge completion, and both hedge scopes closing.
The additional total-budget case verifies timeout outside all retries rather than per-attempt timing.

| Fixed program body | Original `_`: lines / slots | Curried `(_)`: lines / slots | Proposed: lines / slots |
| --- | --- | --- | --- |
| HTTP timeout/retry/fallback | 8 / 1 | 8 / 1 | 8 / 0 |
| Stateful breaker | 9 / 0 | 9 / 0 | 9 / 0 |
| Hedged request | 6 / 0 | 6 / 0 | 6 / 0 |
| Additional total-budget binding | 1 / 2 | 1 / 2 | 1 / 0 |

The proposal saves no lines on these fixed bodies.
It removes three slot markers across the HTTP body and total-budget binding.
The breaker's and hedge's ports do not need a configured composition step, so there is no artificial benefit to report.
The total-budget binding shrinks from 78 to 72 characters, excluding indentation.

| Factory implementation | Original | Curried |
| --- | --- | --- |
| `with_timeout` | 2 lines | 3 lines |
| `fallback` | 2 lines | 3 lines |

Currying adds two implementation lines and duplicates the input/output types in the closure signatures.
Those factories remain normal generic functions; no `Policy`, `Stack`, automatic closure polymorphism, or new inference rule is required for these configuration values.
Calling the curried factories currently adds an ordinary closure call in the source model.
Inlining and allocation removal are compiler questions; these probes measure neither generated allocation counts nor performance.

The appended executable uses `feed` only for those configured steps.
To obtain the proposed chain, replace the two nested calls in `exact` with `retry(backoff) |> with_timeout(ms(5)) |> fallback(backup_again)`.
The proposed source parses today, then fails the existing pipe placeholder checks.
Do not misclassify its follow-on `cannot-infer-type` messages as failures of the separately successful expected-type experiment.

Representative assertions and output are:

```console
http Result.Ok("200") requests=3
breaker calls=2 failures=2
hedge Result.Ok("replica 2") started=2 finished=1 closed=2
reuse 11 reused
total Result.Ok("201") requests=2
```

The full sources and existing error messages are retained in the evidence directory listed in the parse log.
This study changes the factory signatures; it does not obtain the shorter chain merely by deleting `_` from an uncurried API.

### C18. Parsers And Ordinary Builders

Explicit generic references and result context both work in this control.
The input solves a parser factory's input type, while the annotated result solves its output type.
A concrete config factory needs no generic inference at all.
Both engines print `Result.Ok(42)`, `Result.Ok(43)`, and `local/3`.

```text
use std.text.Parse
use std.num.ParseNumberError

fn feed[A, B, $R](value: A, step: fn(A) -> B $ R) -> B $ R: step(value)

fn parse_as[T < Parse]() -> (fn(string) -> Result[T, T::Error]):
    fn(text: string) -> Result[T, T::Error]: text.parse::[T]()
struct Config:
    address: string
    attempts: usize
fn attempts(count: usize) -> (fn(Config) -> Config):
    fn(config: Config) -> Config: Config { address: config.address, attempts: count }
pub fn main() -> void $ Console:
    println(debug(feed("42", string::parse::[i32])))
    let parsed: Result[i32, ParseNumberError] = feed("43", parse_as())
    println(debug(parsed))
    config := feed(Config { address: "local", attempts: 1 }, attempts(3))
    println("${config.address}/${config.attempts}")
```

The candidate spelling is `"43" |> parse_as()` under the same annotated result.
The current explicit form is `"43" |> parse_as::[i32]()(_)`.
A factory may use a defaulted parameter during construction without making its returned unary function variadic.
These are ordinary library additions; changing pipe acceptance remains a language-tier decision.

## Comparison With Other Languages

| Source | Meaning of a configured pipe step | What transfers to hd | What does not |
| --- | --- | --- | --- |
| [F# functions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/functions/) | Apply the function resulting from configuration to the input. | V1's value application and function-data composition. | F#'s implicit currying, declaration inference, and eager List costs. |
| [OCaml functions](https://ocaml.org/docs/higher-order-functions) | Unary application, commonly after partial application. | Expected input type and a final unary call. | General partial application or callback capture through hd `_`. |
| [Elixir Kernel](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2) | Insert the input as the first argument of a call. | Left-to-right reading. | The meaning of `x |> f(y)`; it is a different operation. |
| [Gleam pipelines](https://tour.gleam.run/functions/pipelines/) | Call steps receive input first; other steps are called with it. | Explicitly configured callbacks and typed pipelines. | Call insertion and function-capture `_`. |

In hd, the API's written type determines whether a configured step is usable.
The pipe does not determine which missing parameter the programmer intended.
The foreign examples are small public documentation programs, adapted explicitly rather than asserted to be library implementations.

## Soundness And Implementation

### Type And Effect Preservation

A V1 implementation can reuse ordinary unary-call typing after checking the step expression with the expected input and result.
It must keep the existing invariant checks for function rows inside containers and other function inputs.
The C8 gap requires a focused inference correction or an explicit argument, not an unsound row conversion.
The final-call row joins the input and factory evaluation rows under existing permission rules.

An explicit `!` inside the factory expression may suspend.
The final implicit application remains ordinary, and must reject an inferred suspending function before constructor weakening.
`?`, early exits, and cancellation keep their enclosing targets and cleanup paths.
No helper closure should be inserted around the step merely to make the lowering convenient.

### Evaluation And Ownership

Conceptually evaluate `lhs`, retain its value and access, evaluate `step`, then invoke the resulting function with the retained input.
This is ordered lowering, not a source rewrite into `step(lhs)` that might evaluate the callee first.
It creates no additional user-visible copy or mutable alias beyond ordinary argument passing.
If step evaluation fails or suspends, ownership must keep or release the retained input through existing paths.

Bound `receiver::method` references use the existing address-capture rules.
Dotted fields have ordinary field access semantics under V1, including when a method shares that name.
Generic function values instantiate at their use site and remain monomorphic values.
`Apply` objects remain zero-argument callable objects, not a new unary protocol.

### Checker Work And Diagnostics

Current `crates/hd_check/src/expr.rs` checks a bare path through direct call-argument checking.
Other steps count placeholders and reject zero slots before checking the expression.
V1 replaces those two paths with expected unary expression checking and ordinary invocation, retaining slot ownership and layout guards.
V2 retains the old path branch and adds that behavior for formerly rejected expressions only.

Error recovery should check the step once and retain its first useful diagnostic.
It should not repeatedly guess currying, first-argument insertion, last-argument insertion, or a defaulted adapter.
The candidate needs no parser production, runtime intrinsic, or new representation.
A trustworthy implementation estimate waits for the chosen compatibility and final-`fn!` rules; no line-count speed claim is made here.

Focused implementation regressions would cover C1/C5 migration, C4 source-specific hints, C7 propagation, C8 inference, C10 weakening, and C13 cleanup/access.
Those are requirements for a later implementation, not tests added by this docs-only job.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | PIPE-1 | Uniformity changes existing method/default semantics. | Silent wrong-call risk plus rejection. | C1, C5 |
| 2 | PIPE-2 | Expected ordinary typing can weaken `fn!` to a cold constructor. | Silent execution-shape change. | C10, C11 |
| 3 | PIPE-3 | Nested nonempty rows do not always infer through returned function types. | Current compiler rejection with explicit workaround. | C8, future work-valued builders |
| 4 | PIPE-4 | Foreign call insertion and automatic currying are tempting misreadings. | Usually diagnosed; possible wrong API design. | C2–C4, C14–C18 |
| 5 | PIPE-5 | Fewer call-site markers require longer factory APIs. | Ergonomics and teaching cost. | C2, C14, C17, C18 |

### PIPE-1: Existing Successful Programs Change

**Effect.** C5 can call a field instead of a method without any error.
C1 rejects a declaration that currently supplies its default.
This is a language migration, even if most new suffix examples are merely newly accepted.

**Candidates.** Choose uniform V1 with explicit migration, preserve bare-path behavior as V2, or keep V0 and bind factories first?
The final questions give concrete examples.

### PIPE-2: Expected Types Can Hide The Bang Boundary

**Effect.** C10 proves that a plain expected `fn` permits a cold constructor adaptation.
C11 shows that explicit suspension to obtain an ordinary callable is a separate, useful operation.
An implementation must distinguish both while preserving the rows.

**Candidates.** Reject inferred final `fn!` values before weakening, or explicitly allow cold construction as a documented change?
An ordinary explicitly written constructor wrapper remains available either way.

### PIPE-3: Nested-Row Inference Has A Counterexample

**Effect.** C8 rejects an ordinary curried identity factory with `$ Console`.
Explicit row arguments and the empty-row control pass.
The configuration-only factories in C17 do not encounter that failure.

**Candidates.** Fix this existing inference gap before pipe implementation, or ship with documented explicit row arguments pending a separate correction?
Erasing rows or loosening nested invariance is not a sound candidate.

### PIPE-4: Pipes Do Not Curry Existing APIs

**Effect.** C4 remains an incomplete ordinary call.
F# and OCaml examples transfer only after explicitly returning unary functions.
Elixir and Gleam call steps have a different argument-placement rule.

**Candidates.** Keep one value-application rule with source-specific suggestions, or keep the current rejection until wrapper ergonomics justify the change?
Do not silently add a second insertion interpretation to recover mistakes.

### PIPE-5: The Fixed Ports Save Markers, Not Lines

**Effect.** C17 removes three placeholders but adds two factory implementation lines.
C14 likewise needs explicit wrappers over today's adapter methods.
This does not establish that the feature improves every pipe or justifies a std-wide API migration.

**Candidates.** Limit the first library use to genuinely function-producing builders, or retain explicit substitution for the current std APIs?
Both can coexist under one pipe rule without providing duplicate APIs for each operation.

### Recommendation

Evaluate V1 as a deliberate semantic change, with C1 and C5 migration called out before approval.
The configuration-builder inference result supports its usefulness, while the fixed-port counts bound the claimed benefit.
V2 is the next-best compatibility option, but permanently keeps two meanings for zero-slot steps.
V0 remains the smallest design: bind a returned function, then use today's bare pipe.

Retain explicit suspension at the final call boundary and source-specific diagnostics.
Keep the row gap separate from the policy composition choice.
No variant is selected on the owner's behalf.

## Executable Policy Control

This complete program is the curried expected-type control for all three fixed ports.
Its `feed` helper models the proposed expected type; it does not implement relaxed pipe checking.
The corresponding original and explicit-slot versions differ only in factory signatures and configured-step application.

```text
use std.convert.From
use std.http.{Http, HttpError, Request, Response, ScriptedHttp, get}
use std.task.{Backoff, TimedOut, race}
use std.task
use std.time.{Clock, Duration, ms, sleep}

fn feed[A, B](value: A, step: fn(A) -> B) -> B: step(value)

# Lean shape 1, owner direction 2026-10-24 03:30 UTC.
# A policy is configuration. Tuples compose it, and one apply! runs work.
# No new language semantics and no production std/compiler changes.

# ---- 1. proposed std.task surface ----

# Ordinary tuples compose from the work outward; no Policy/Stack/then.
trait Layer[W]:
    type Wrapped
    fn wrap(self, work: W) -> Self::Wrapped

impl[W] Layer[W] for ():
    type Wrapped = W
    fn wrap(self, work: W) -> W: work

impl[W, I < Layer[W], O < Layer[I::Wrapped]] Layer[W] for (I, O):
    type Wrapped = O::Wrapped
    fn wrap(self, work: W) -> O::Wrapped:
        self._1.wrap(self._0.wrap(work))

# Wraps the work in the policy and runs the result once.
fn apply![T, U, $R, $S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](policy: P, work: fn!() -> T $ R) -> U $ S:
    wrapped := policy.wrap(work)
    wrapped!()

# A deadline for each run of the work; a timeout becomes the work's error.
struct TimeLimit:
    limit: Duration
    cleanup: Duration? = .None


impl[T, E < From[TimedOut], $R] Layer[fn!() -> Result[T, E] $ R] for TimeLimit:
    type Wrapped = fn!() -> Result[T, E] $ R + Clock
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
        fn!() -> Result[T, E] $ R + Clock:
            task.with_timeout!(self.limit, cleanup=self.cleanup, work=work)

# Runs the work again after a failure that `when` accepts.
struct Retry:
    backoff: Backoff


impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Retry:
    type Wrapped = fn!() -> Result[T, E] $ R + Clock
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
        fn!() -> Result[T, E] $ R + Clock:
            task.retry!(self.backoff, attempt=work)

# A typed predicate is optional configuration, fixed before apply!.
struct SelectiveRetry[E]:
    backoff: Backoff
    when: fn(E) -> bool

impl Retry:
    fn when[E](self, predicate: fn(E) -> bool) -> SelectiveRetry[E]:
        SelectiveRetry { backoff: self.backoff, when: predicate }

impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for SelectiveRetry[E]:
    type Wrapped = fn!() -> Result[T, E] $ R + Clock
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
        fn!() -> Result[T, E] $ R + Clock:
            task.retry!(self.backoff, when=self.when, attempt=work)

# Runs `other` when the work fails.
struct Fallback[F]:
    other: F


impl[T, E, $R, $S] Layer[fn!() -> Result[T, E] $ R] for Fallback[fn!() -> Result[T, E] $ S]:
    type Wrapped = fn!() -> Result[T, E] $ R + S
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + S):
        fn!() -> Result[T, E] $ R + S:
            match work!():
                .Ok(value) => .Ok(value)
                .Err(_) => (self.other)!()

# A breaker's error, and its state, which outlives each application.
struct CircuitOpen: pass

struct Breaker:
    failures: usize
    limit: usize
    calls: usize

# Refuses the work once `limit` failures in a row have happened.
struct Guard:
    state: &mut Breaker


impl[T, E < From[CircuitOpen], $R] Layer[fn!() -> Result[T, E] $ R] for Guard:
    type Wrapped = fn!() -> Result[T, E] $ R
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R):
        fn!() -> Result[T, E] $ R:
            if self.state.failures >= self.state.limit:
                return .Err(E::from(CircuitOpen {}))
            self.state.calls += 1
            result := work!()
            self.state.failures = if result.is_ok(): 0 else: self.state.failures + 1
            result

# Starts a second copy after `delay`; the first to finish wins.
struct Hedge:
    delay: Duration


impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Hedge:
    type Wrapped = fn!() -> Result[T, E] $ R + Clock
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Clock):
        fn!() -> Result[T, E] $ R + Clock:
            race!(work(), late_copy(work, self.delay))

fn late_copy![T, E, $R](work: fn!() -> Result[T, E] $ R, delay: Duration) -> Result[T, E] $ R + Clock:
    sleep!(delay)
    work!()

# Plain functions build configuration values without obtaining providers.
fn retry(backoff: Backoff) -> Retry: Retry { backoff: backoff }
fn timeout(limit: Duration, cleanup: Duration? = .None) -> TimeLimit:
    TimeLimit { limit: limit, cleanup: cleanup }
fn with_timeout[I](limit: Duration, cleanup: Duration? = .None) -> (fn(I) -> (I, TimeLimit)):
    fn(inner: I) -> (I, TimeLimit):
        (inner, timeout(limit, cleanup=cleanup))
fn fallback[F, I](other: F) -> (fn(I) -> (I, Fallback[F])):
    fn(inner: I) -> (I, Fallback[F]):
        (inner, Fallback { other: other })
fn guarded(state: &mut Breaker) -> Guard: Guard { state: state }
fn hedged(delay: Duration) -> Hedge: Hedge { delay: delay }

# ---- 2. a user policy: one struct and one Layer impl ----

struct Trace:
    label: string


impl[T, E, $R] Layer[fn!() -> Result[T, E] $ R] for Trace:
    type Wrapped = fn!() -> Result[T, E] $ R + Console
    fn wrap(self, work: fn!() -> Result[T, E] $ R) -> (fn!() -> Result[T, E] $ R + Console):
        fn!() -> Result[T, E] $ R + Console:
            result := work!()
            println("trace ${self.label}: ${if result.is_ok(): "ok" else: "failed"}")
            result

# ---- 3. the programs ----

@derive(Debug)
enum Failure:
    Http(HttpError)
    Timeout
    Open

impl From[HttpError] for Failure:
    fn from(error: HttpError) -> Failure: .Http(error)

impl From[TimedOut] for Failure:
    fn from(error: TimedOut) -> Failure: .Timeout

impl From[CircuitOpen] for Failure:
    fn from(error: CircuitOpen) -> Failure: .Open

fn transient(error: Failure) -> bool:
    match error:
        .Http(inner) => inner.is_transient()
        .Timeout => true
        .Open => false

# A fake provider that records each request, then holds primary replies
# for 500 ms, so the caller's deadline cancels them.
struct DelayedHttp:
    inner: &mut ScriptedHttp
    clock: &mut dyn Clock
    finished_primary: usize

impl Http for DelayedHttp:
    fn send!(&mut self, request: Request) -> Result[Response, HttpError]:
        slow := request.url == "http://primary/item"
        result := self.inner.send!(request)
        if slow:
            self.clock.sleep!(ms(500))
            self.finished_primary += 1
        result

fn fetch!(url: string) -> Result[string, Failure] $ Http:
    response := get!(url)?
    .Ok("${response.status}")

struct Replicas:
    started: usize
    finished: usize
    closed: usize

fn replica!(state: &mut Replicas) -> Result[string, Failure] $ Http + Clock:
    state.started += 1
    number := state.started
    defer: state.closed += 1
    sleep!(if number == 1: ms(500) else: ms(10))
    _ := get!("http://replica/$number")?
    state.finished += 1
    .Ok("replica $number")

fn count!() -> Result[usize, string]: .Ok(9)

# Only Console: Trace adds Console and nothing else, so no Clock appears.
fn traced!() -> Result[usize, string] $ Console:
    apply!(Trace { label: "pure" }):
        count!()

pub fn main!() -> void $ Console + Clock:
    println("pure ${debug(traced!())}")
    http := ScriptedHttp::new()
    http.reply("http://primary/item", .Err(.Connect("first")))
    http.reply("http://primary/item", .Err(.Connect("second")))
    http.reply("http://backup/item", .Ok(Response { status: 200 }))
    http.reply("http://down/item", .Err(.Connect("down")))
    http.reply("http://replica/2", .Ok(Response { status: 200 }))
    let mut delayed_http = DelayedHttp { inner: http, clock: $.use(Clock), finished_primary: 0 }
    $.with(Http=&mut delayed_http):
        # 1. a deadline per attempt, two attempts, then a fallback source.
        backup := fn!() -> Result[string, Failure] $ Http: fetch!("http://backup/item")
        policy := feed((timeout(ms(50)), retry(Backoff { attempts: 2, initial: ms(1) }).when(transient)), fallback(backup))
        response := apply!(policy):
            fetch!("http://primary/item")
        println("http ${debug(response)} requests=${http.received().len()}")
        if http.received().len() != 3 || delayed_http.finished_primary != 0:
            panic("expected two cancelled primary replies and one fallback request")

        # 2. a breaker whose state outlives each application.
        let mut breaker = Breaker { failures: 0, limit: 2, calls: 0 }
        guard := guarded(&mut breaker)
        for i in 0..4:
            outcome := apply!(guard):
                fetch!("http://down/item")
            println("breaker $i ${debug(outcome)}")
        println("breaker calls=${breaker.calls} failures=${breaker.failures}")
        if breaker.calls != 2 || breaker.failures != 2:
            panic("the breaker did not stop repeated calls")

        # 3. a hedged request: a second copy after 10 ms.
        let mut state = Replicas { started: 0, finished: 0, closed: 0 }
        result := apply!(hedged(ms(10))):
            replica!(&mut state)
        println("hedge ${debug(result)} started=${state.started} finished=${state.finished} closed=${state.closed}")
        if state.started != 2 || state.finished != 1 || state.closed != 2:
            panic("the hedge did not complete one request and close both scopes")

        # The owner's exact pipe spelling: timeout is outside all retries.
        single := timeout(ms(5))
        let number: Result[usize, Failure] = apply!(single):
            let value: Result[usize, Failure] = .Ok(11)
            value
        match number:
            .Ok(value) => if value != 11: panic("wrong first value")
            .Err(_) => panic("unexpected timeout")
        let word: Result[string, Failure] = apply!(single):
            let value: Result[string, Failure] = .Ok("reused")
            value
        match word:
            .Ok(value) => if value != "reused": panic("wrong second value")
            .Err(_) => panic("unexpected timeout")
        println("reuse 11 reused")

        backoff := Backoff { attempts: 5, initial: ms(1) }
        backup_again := fn!() -> Result[string, Failure] $ Http: fetch!("http://second-backup/item")
        exact := feed(feed(retry(backoff), with_timeout(ms(5))), fallback(backup_again))
        http.reply("http://primary/item", .Ok(Response { status: 200 }))
        http.reply("http://second-backup/item", .Ok(Response { status: 201 }))
        before := http.received().len()
        budget := apply!(exact):
            fetch!("http://primary/item")
        if http.received().len() - before != 2 || delayed_http.finished_primary != 0:
            panic("the total budget allowed another primary attempt")
        println("total ${debug(budget)} requests=${http.received().len() - before}")
```

## Parse Log

All hd fences are whole source files, parsed with the source-matched `hd_syntax` parser.
The table records current checking separately from candidate verdicts.
The complete scratch source set also contains the candidate spellings and negative controls.
Parsing alone proves no type or runtime claim.

| Block | Source/control | Parse | Current check |
| --- | --- | --- | --- |
| 1 | `defaults-current` | clean | clean |
| 2 | `defaults-value` | clean | expected rejection: type-mismatch |
| 3 | `inference` | clean | clean |
| 4 | `forms-model` | clean | clean |
| 5 | `forms-proposed` | clean | expected rejection: pipe-step-needs-placeholder, not-callable |
| 6 | `wrong-scale-proposed` | clean | expected rejection: pipe-step-needs-placeholder |
| 7 | `field-method-collision` | clean | clean |
| 8 | `propagate-model` | clean | clean |
| 9 | `returned-row-gap` | clean | expected rejection: type-mismatch |
| 10 | `nested-placeholders` | clean | clean |
| 11 | `suspending-value` | clean | clean |
| 12 | `suspending-factory` | clean | clean |
| 13 | `multiline-proposed` | clean | expected rejection: pipe-step-needs-placeholder |
| 14 | `order-model` | clean | clean |
| 15 | `mutable-value` | clean | clean |
| 16 | `iterators-model` | clean | clean |
| 17 | `ocaml-model` | clean | clean |
| 18 | `elixir-gleam-model` | clean | clean |
| 19 | `parser-builder-model` | clean | clean |
| 20 | `policies-curried-expected` | clean | clean |

The complete scratch matrix has 39 parsed/formatted programs: 23 clean checks and 16 intentional rejections.
The 23 positive programs produced 46 successful engine runs, with matching stdout for each Node/native pair.

The evidence directory is `/private/tmp/muse-818-pipe-steps-1792817126850624000`.
It contains frozen compiler/parser binaries, provenance, checked source hashes, complete diagnostics, engine output, and count calculations.
Every positive source in the final probe manifest ran on both engines.
All negative cases are deliberate controls; no compiler rejection is silently reported as successful execution.
No full suite ran.

## Questions For The Owner

1. **PIPE-1:** V1 can change a successful method call into a field call and reject defaults; choose uniform V1 with migration, V2 compatibility, or V0?
   `"hd" |> user.greet` currently selects the method; V1 selects the field, while `user::greet` explicitly preserves the method.
   Recommendation: evaluate V1 as a deliberate migration, never describe it as acceptance-only relaxation.
2. **PIPE-2:** Plain expected function typing admits cold construction; retain rejection of a final inferred `fn!`, or explicitly permit that behavior?
   `+3 |> load` currently fails; `+3 |> load!(_)` executes visibly.
   Recommendation: retain the rejection before weakening.
3. **PIPE-3:** Nested nonempty rows currently need help; fix inference first, or permit an explicit-row workaround initially?
   `feed(hello, keep::[$ Console]())` works where `feed(hello, keep())` fails.
   Recommendation: track the ordinary inference correction separately and preserve invariant row checking.
4. **PIPE-4:** Missing arguments need intent-sensitive advice; approve source-specific suggestions, or retain a generic arity message only?
   For `x |> scale(3)`, suggest `scale(3, _)` only when exactly one unambiguous final argument is missing.
   Recommendation: use a suggestion, suppress the cascade, and never automatically reinterpret a successful factory.
5. **PIPE-5:** The fixed ports save three markers and no body lines; should #783 use curried builders, or keep its current explicit input slot?
   `retry(backoff) |> with_timeout(ms(50)) |> fallback(backup)` requires actual unary-returning factories.
   Recommendation: allow the curried shape as evidence for #783, with its extra closure/type signatures and the chosen pipe semantics made explicit.

