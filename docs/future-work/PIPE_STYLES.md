# Pipe Styles For Shell Work

Design review, 2026-10-25; nothing here is accepted behavior.
This report changes no decision, specification, conformance fixture, standard library, or compiler code.
It evaluates [#822](https://github.com/HerringtonDarkholme/hd-lang/issues/822) and its [four rule sheets](pipe-styles/haiku-probe/README.md), extending [#818](PIPE_STEPS.md).

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Candidate Rule Text](#candidate-rule-text)
- [Cases](#cases)
- [Grammar And Inference](#grammar-and-inference)
- [Standard Library Cost](#standard-library-cost)
- [Measured Performance](#measured-performance)
- [Comparison With Other Languages](#comparison-with-other-languages)
- [Problems, Ranked](#problems-ranked)
- [Parse Log](#parse-log)
- [Rankings And Owner Questions](#rankings-and-owner-questions)

## Surface Being Tested

The four candidates are Elixir first-argument insertion, F# unary function values, Hack explicit slots, and bare-or-slot pipes with general partial application.
UFCS, function composition, and removing pipes are excluded by the owner.
There is no compatibility requirement, but silent misreading remains a cost.
The relevant current rules are [Pipe Expressions](../../spec/lang/05-expressions.md#pipe-expressions), [functions](../../spec/lang/07-functions.md), and [requirements and suspension](../../spec/lang/11-requirements-and-suspension.md).

The library evidence uses [iterators](../../spec/std/iter.md), [text](../../spec/std/text.md), [tasks](../../spec/std/task.md), and the [policy design](POLICY_COMPOSITION.md).
The hypothetical shell facade below supplies subject-first direct functions, or corresponding curried factories under F#.
Names such as `fields`, `group_count`, and `render` describe small adapters, not existing std exports.
No candidate creates automatic currying of today's ordinary hd functions.

Two omissions in the sheets need explicit assumptions before examples have exact meanings.
First, placeholder ownership does not fully specify how an inner partial differs from the pipe's slot.
Second, calling a suspending function without `!` can construct a cold suspension through ordinary function-value weakening.
The rule completions below expose both assumptions for approval rather than installing them.

## Method

Sixteen cases cover the requested source families and difficult call forms.
Each case shows the same operation in all four styles; originals are short, independently written approximations.
The ports preserve operation order unless their case explicitly describes a difference.
They do not claim to implement Elixir's protocols, jq's stream algebra, or a shell's concurrent byte streams.

**Works** means the candidate expresses the operation under its stated facade.
**Friction** requires a wrapper, annotation, explicit closure, or material behavior explanation.
**Breaks** means the displayed form has ambiguous ownership or cannot express its intended operation under the literal candidate rules.
Parsing establishes syntax only; executable desugarings and inference controls establish the narrower claims identified below.

All 16 case blocks are parsed as complete function bodies, with unresolved facade names intentionally left to future implementations.
Comments identify the four candidates; function names prevent accidentally running all alternative interpretations together.
The separate benchmark uses real, checked hd and never measures hypothetical pipe syntax.
Negative controls deliberately test rejected forms and are not repaired into successes.

Counts of the earlier Haiku trial are reused only as reading evidence.
Its 48 programs had zero recorded slot errors and rule violations; they were not compiled.
Its pipe-character totals were 1,367 Elixir, 1,227 F#, 1,722 Hack, and 1,371 placeholder.
Those counts neither establish correctness nor predict this report's performance.

## Summary

| Case | Source or operation | Elixir | F# | Hack | Placeholder | Main pressure |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | Elixir Enum: error count | works | works | works | works | Common subject-first flow |
| C2 | Elixir Stream: lazy first matches | works | works | works | works | Laziness belongs to adapters |
| C3 | F# List: parse, keep, sum | works | works | works | works | Returned generic input type |
| C4 | F# Seq: map, take, join | works | works | works | works | Same lazy adapter across styles |
| C5 | grep / awk / sort / uniq | works | works | works | works | Adjacent unique follows sort |
| C6 | jq: active users' names | friction | friction | friction | friction | Explicit materialized list port |
| C7 | TC39: arbitrary expression topic | friction | friction | works | works | Insertion/value styles need lambdas |
| C8 | TC39: subject in second slot | friction | works | works | works | Silent same-type wrong-slot risk |
| C9 | Scala partial inside map | friction | works | friction | breaks | Two `_` owners in one step |
| C10 | Generic calls and method references | works | works | works | works | Syntax parses; value typing differs |
| C11 | Named/default/varargs calls | works | friction | works | works | Defaults are absent from function types |
| C12 | Trailing blocks and multiline | friction | friction | friction | friction | Existing layout limits remain |
| C13 | Fallible steps and lookup | works | friction | works | works | `?` acts on factory versus result |
| C14 | Suspension and cold values | works | friction | works | works | No implicit execution of `fn!` |
| C15 | #783: policy configuration chain | works | works | works | works | Policy data need not be closures |
| C16 | Effects and reuse of partials | works | works | works | friction | Configuration timing differs |

All four express ordinary shell transformations.
The strongest differences are slot ownership, function-value typing, std shape, and when configuration effects happen.
Direct desugarings add no pipe-specific closure; curried factories retain four captured environments in the measured pipeline.
F# is compact, but today's compiler does not make its idiomatic form zero-cost.

## Candidate Rule Text

These are proposed language-tier rules in [spec style](../../spec/STYLE.md), with study-only IDs.
They are not additions to the accepted rule inventory.
If adopted, every replaced current meaning needs a new production ID and migrated conformance cases.
The std facade conventions belong to the stdlib tier, except any intrinsic the compiler must recognize.

### Shared Rules

1. r[study.pipe.common.input] A pipe evaluates its input exactly once before evaluating any part of its step.
2. r[study.pipe.common.chain] Pipe chains associate left and retain the current pipe precedence.
3. r[study.pipe.common.result] A pipe's result is the result of its selected step operation.
4. r[study.pipe.common.access] The saved input retains its ordinary argument type and access.
5. r[study.pipe.common.layout] Steps retain the current leading-pipe continuation and prohibition on indented suites and trailing blocks.
6. r[study.pipe.common.effects] Calls, requirements, defaults, propagation, and explicit suspension inside a step obey their ordinary rules.
7. r[study.pipe.common.no-implicit-bang] A pipe never adds an unwritten bang invocation.

Rule 1 requires a saved input, rather than textual rewriting that could evaluate a factory before that input.
That temporary is an evaluation-order device, not necessarily a heap allocation.
All examples use existing syntax; the differences are proposed meanings and typing rules.

### Elixir

1. r[study.pipe.elixir.bare] A bare callable path invokes its declaration with the saved input as the first positional argument.
2. r[study.pipe.elixir.call] A call step inserts the saved input before its written positional arguments.
3. r[study.pipe.elixir.callee] The step may use an ordinary function path, method reference, or explicitly instantiated generic callee.
4. r[study.pipe.elixir.arguments] Written named arguments and spreads follow the inserted argument under ordinary call checking.
5. r[study.pipe.elixir.suffix] A call's `!` and following `?` apply to the inserted call.
6. r[study.pipe.elixir.slots] An expression placeholder is invalid under this candidate.
7. r[study.pipe.elixir.expression] A non-call, non-path step is invalid unless written as an explicit unary closure.
8. r[study.pipe.elixir.closure] An explicit ordinary unary closure step is called with the saved input.

The closure allowance is the ordinary escape hatch for arithmetic or a non-first subject.
A deliberately minimal first-insertion-only variant could require a separately named unary helper instead.
Neither variant inserts into a call nested inside a callback.
The library convention makes the transformed subject the first parameter of direct facade functions.

### F#

1. r[study.pipe.fsharp.expected] A step is checked against an ordinary unary function type with the input's type as its parameter.
2. r[study.pipe.fsharp.result-context] The expected pipe result type, when present, supplies the unary function's result context.
3. r[study.pipe.fsharp.expression] After saving the input, a pipe evaluates its step expression once.
4. r[study.pipe.fsharp.invoke] The resulting ordinary function is called once with the saved input.
5. r[study.pipe.fsharp.no-insertion] A pipe inserts no argument into any call within its step expression.
6. r[study.pipe.fsharp.arity] A step value whose function type is not unary is invalid.
7. r[study.pipe.fsharp.placeholder] An expression placeholder is invalid under this candidate.
8. r[study.pipe.fsharp.suspend-guard] A final suspending function value is rejected before conversion to an ordinary cold-suspension constructor.

Rule 8 is an explicit safety completion carried from #818, not a consequence of the sheet's simple equation.
Without it, C14's ordinary `feed` control accepts a `fn!` value and returns a cold job.
An ordinary function explicitly returning a cold job remains valid.
Configured facade functions return unary functions; hd's tupled argument syntax does not acquire F#'s automatic partial application.

### Hack

1. r[study.pipe.hack.one] A step must contain exactly one own expression placeholder.
2. r[study.pipe.hack.substitute] That placeholder denotes the saved input at its written expression position.
3. r[study.pipe.hack.any] A step may be any expression permitted by the pipe's precedence and layout.
4. r[study.pipe.hack.nested] A nested pipe owns its own placeholders independently of its enclosing pipe.
5. r[study.pipe.hack.closure] A placeholder inside an explicit closure cannot refer to the enclosing pipe's input.
6. r[study.pipe.hack.outside] A placeholder outside a pipe creates no function and is invalid.
7. r[study.pipe.hack.no-bare] A step without a placeholder is invalid, including a bare unary function.

This is the issue's restricted Hack candidate, not an exact copy of TC39's draft.
In particular, this study retains hd's exactly-once marker rule.
Ordinary parameter order is unrestricted; first-subject examples are a facade choice, not a language requirement.

### Placeholder

1. r[study.pipe.placeholder.bare] A bare callable path invokes its declaration with the saved input as its positional argument.
2. r[study.pipe.placeholder.slot] A non-bare step must contain exactly one pipe-owned expression placeholder.
3. r[study.pipe.placeholder.substitute] A pipe-owned placeholder denotes the saved input at its written expression position.
4. r[study.pipe.placeholder.nested-pipe] A nested pipe owns its own placeholders independently of the enclosing pipe.
5. r[study.pipe.placeholder.partial] Outside pipes, a call containing one direct placeholder denotes a unary closure over that call.
6. r[study.pipe.placeholder.inner] A proper nested call containing a placeholder forms a partial before its enclosing call is interpreted.
7. r[study.pipe.placeholder.root] Within a pipe, a placeholder directly owned by the outer step expression denotes the pipe input.
8. r[study.pipe.placeholder.partial-body] Non-placeholder expressions in a partial execute when its resulting closure is called.
9. r[study.pipe.placeholder.partial-arity] A partial call containing multiple direct placeholders is invalid under this unary completion.
10. r[study.pipe.placeholder.closure] A placeholder cannot cross an explicit closure boundary to capture a pipe input.

Rules 6–8 are **completion Pα**, used only to make the disputed examples precise.
They select the outer `map` slot and the inner `pad` partial in C9.
An alternative Pβ preserves today's innermost-pipe ownership and requires explicit closures inside a pipe.
The sheet does not choose between them; the uncompleted candidate therefore receives C9's breaks verdict.

Pα also changes a current nested-call idiom: `x |> render(normalize(_))` creates an inner partial instead of forwarding `x` to `normalize`.
There is then no outer slot, so the step is rejected.
Different syntax or an explicit lambda can resolve this, but that is an owner choice.
Rule 8 follows the sheet's literal lambda expansion; capture-once semantics would be a different rule.

## Cases

### C1. Elixir Enum: count matching lines

Original shape (an approximation):

```elixir
text |> String.split("\n") |> Enum.filter(&String.contains?(&1, "ERROR")) |> Enum.count()
```

```text
fn elixir_case() -> void:
    result := text |> lines |> filter(fn(line): line.contains("ERROR")) |> count
fn fsharp_case() -> void:
    result := text |> lines |> filter(fn(line): line.contains("ERROR")) |> count
fn hack_case() -> void:
    result := text |> lines(_) |> filter(_, fn(line): line.contains("ERROR")) |> count(_)
fn placeholder_case() -> void:
    result := text |> lines |> filter(_, fn(line): line.contains("ERROR")) |> count
```

All work with eager facade operations. The visually identical Elixir/F# call to `filter` has a different declaration and lowering: `filter(xs, keep)` versus `filter(keep)(xs)`. The benchmark measures both rather than treating identical spelling as identical code.

### C2. Elixir Stream: stop after three matches

Original shape (an approximation):

```elixir
lines |> Stream.filter(&String.contains?(&1, "ERROR")) |> Stream.take(3) |> Enum.to_list()
```

```text
fn elixir_case() -> void:
    result := source |> lazy_filter(keep) |> take(3) |> collect
fn fsharp_case() -> void:
    result := source |> lazy_filter(keep) |> take(3) |> collect
fn hack_case() -> void:
    result := source |> lazy_filter(_, keep) |> take(_, 3) |> collect(_)
fn placeholder_case() -> void:
    result := source |> lazy_filter(_, keep) |> take(_, 3) |> collect
```

All work when `lazy_filter` and `take` return lazy iterators. The facade must not replace them with eager list transformations: that changes effects and may never terminate on an infinite source. Pipe syntax alone supplies neither laziness nor fusion.

### C3. F# List: keep parsed numbers

Original shape (an approximation):

```fsharp
words |> List.choose tryNumber |> List.sum
```

```text
fn elixir_case() -> void:
    result := words |> filter_map(parse_number) |> sum
fn fsharp_case() -> void:
    result := words |> filter_map(parse_number) |> sum
fn hack_case() -> void:
    result := words |> filter_map(_, parse_number) |> sum(_)
fn placeholder_case() -> void:
    result := words |> filter_map(_, parse_number) |> sum
```

All work under an optional-number parser facade. `filter_map` removes failed parses rather than propagating the first failure. F# needs expected input typing through a factory; fresh scalar/list controls show this direction of inference works today. The facade does not rely on implicit conversion of Result to Option.

### C4. F# Seq: format a bounded prefix

Original shape (an approximation):

```fsharp
numbers |> Seq.map string |> Seq.take 20 |> String.concat ","
```

```text
fn elixir_case() -> void:
    result := numbers |> lazy_map(number_text) |> take(20) |> join(",")
fn fsharp_case() -> void:
    result := numbers |> lazy_map(number_text) |> take(20) |> join(",")
fn hack_case() -> void:
    result := numbers |> lazy_map(_, number_text) |> take(_, 20) |> join(_, ",")
fn placeholder_case() -> void:
    result := numbers |> lazy_map(_, number_text) |> take(_, 20) |> join(_, ",")
```

All work with lazy map/take and a consuming join. This is a prefix operation, unlike the million-element eager benchmark. F# creates configured step values; direct variants still need the element callback but no outer map/take/join factories.

### C5. Shell: grep, awk, sort, uniq counts

Original shape (an approximation):

```bash
grep ERROR app.log | awk '{print $2}' | sort | uniq -c
```

```text
fn elixir_case() -> void:
    result := text |> lines |> filter(is_error) |> map(second_word) |> sort |> group_count |> render
fn fsharp_case() -> void:
    result := text |> lines |> filter(is_error) |> map(second_word) |> sort |> group_count |> render
fn hack_case() -> void:
    result := text |> lines(_) |> filter(_, is_error) |> map(_, second_word) |> sort(_) |> group_count(_) |> render(_)
fn placeholder_case() -> void:
    result := text |> lines |> filter(_, is_error) |> map(_, second_word) |> sort |> group_count |> render
```

All express the value transformation. `second_word` performs whitespace field selection; `group_count` counts adjacent equal sorted keys, and `render` writes count plus key. Without sort, adjacent uniq differs from global deduplication. This port does not reproduce concurrent processes, locale-dependent sort, or arbitrary byte input.

### C6. jq: active users, sorted unique names

Original shape (an approximation):

```jq
[.users[] | select(.active) | .name] | sort | unique
```

```text
fn elixir_case() -> void:
    result := document |> users |> filter(is_active) |> map(user_name) |> sort |> unique
fn fsharp_case() -> void:
    result := document |> users |> filter(is_active) |> map(user_name) |> sort |> unique
fn hack_case() -> void:
    result := document |> users(_) |> filter(_, is_active) |> map(_, user_name) |> sort(_) |> unique(_)
fn placeholder_case() -> void:
    result := document |> users |> filter(_, is_active) |> map(_, user_name) |> sort |> unique
```

All incur the same semantic friction: `users` explicitly materializes a typed list, while jq filters can emit multiple results. `unique` here globally deduplicates sorted names; it is not C5's adjacent run counter. No candidate automatically converts a value pipe into jq's stream algebra.

### C7. TC39: expression-shaped topic

Original shape (an approximation):

```javascript
message |> (% + "!") |> console.log(%)
```

```text
fn elixir_case() -> void:
    result := message |> (fn(value): value + "!") |> emit
fn fsharp_case() -> void:
    result := message |> (fn(value): value + "!") |> emit
fn hack_case() -> void:
    result := message |> (_ + "!") |> emit(_)
fn placeholder_case() -> void:
    result := message |> (_ + "!") |> emit
```

Hack and placeholder substitute into the arithmetic expression directly. Elixir and F# need an explicit unary lambda or named helper. The JS spelling follows the proposal's illustrative topic token; this is not runnable shipped JavaScript. `emit` is a facade function requiring Console, not a fifth dot lookup.

### C8. TC39: non-first subject in a foreign API

Original shape (an approximation):

```javascript
body |> replace(pattern, %, replacement)
```

```text
fn elixir_case() -> void:
    result := body |> (fn(value): replace(pattern, value, replacement))
fn fsharp_case() -> void:
    result := body |> replace(pattern, replacement)
fn hack_case() -> void:
    result := body |> replace(pattern, _, replacement)
fn placeholder_case() -> void:
    result := body |> replace(pattern, _, replacement)
```

The foreign direct signature is `replace(pattern, body, replacement)`, all strings. Pure Elixir needs the explicit lambda shown; naively writing `body |> replace(pattern, replacement)` swaps pattern and body and may silently type-check. F#'s displayed form requires a curried facade with those two configuration arguments; it is not the unchanged foreign declaration. Hack and placeholder call the original signature.

### C9. Scala: a partial nested inside map

Original shape (an approximation):

```scala
names.map(pad(_, 10))
```

```text
fn elixir_case() -> void:
    result := names |> map(fn(value): pad(value, 10))
fn fsharp_case() -> void:
    result := names |> map(pad(10))
fn hack_case() -> void:
    result := names |> map(_, fn(value): pad(value, 10))
fn placeholder_case() -> void:
    result := names |> map(_, pad(_, 10))
```

Elixir and Hack use explicit callbacks; F# uses a curried `pad(width)` factory. Placeholder's two markers need different owners. Pα makes this work; today's checker reports duplicate placeholders, and the sheet alone does not specify the Pα boundary. Under Pα, `names |> map(pad(_, 10))` still lacks an outer pipe slot and is invalid. This is the central unresolved candidate rule, not a parser failure.

### C10. Generic parse calls and method references

Original shape (an approximation):

```fsharp
text |> System.Int64.Parse
```

```text
fn elixir_case() -> void:
    result := text |> string::parse::[i64]()
fn fsharp_case() -> void:
    result := text |> string::parse::[i64]
fn hack_case() -> void:
    result := text |> string::parse::[i64](_)
fn placeholder_case() -> void:
    result := text |> string::parse::[i64](_)
```

The hd result is `Result[i64, ParseNumberError]`, rather than the .NET throwing parse operation. Elixir inserts into an explicitly instantiated declaration call. F# evaluates the instantiated unary method value. Hack and placeholder substitute directly; an instantiated method path need not be added to placeholder's bare-path exception. Fresh ordinary function-value controls check both static and receiver-bound references.

### C11. Defaults, named configuration, and varargs

Original shape (an approximation):

```python
decorate(items, suffix="!")
append(items, "tail", *extras)
```

```text
fn elixir_case() -> void:
    result := items |> decorate(suffix="!") |> append("tail", extras...)
fn fsharp_case() -> void:
    result := items |> decorate(suffix="!") |> append("tail", extras...)
fn hack_case() -> void:
    result := items |> decorate(_, suffix="!") |> append(_, "tail", extras...)
fn placeholder_case() -> void:
    result := items |> decorate(_, suffix="!") |> append(_, "tail", extras...)
```

Elixir, Hack, and placeholder retain declaration defaults and direct varargs binding. F# needs separate factory signatures: configure suffix and collect extra values before accepting the final unary subject. A bare defaulted direct declaration is not a unary function value. A defaulted factory must be called as `decorate()`; `items |> decorate` may instead pass items as its configuration argument or fail arity checking.

### C12. Multiline shell pipeline, callback bound first

Original shape (an approximation):

```bash
cat names.txt | sed 's/^ *//;s/ *$//' | grep -v '^$' | paste -sd, -
```

```text
fn elixir_case() -> void:
    result := text
        |> lines
        |> map(trim_line)
        |> filter(nonempty)
        |> join(",")
fn fsharp_case() -> void:
    result := text
        |> lines
        |> map(trim_line)
        |> filter(nonempty)
        |> join(",")
fn hack_case() -> void:
    result := text
        |> lines(_)
        |> map(_, trim_line)
        |> filter(_, nonempty)
        |> join(_, ",")
fn placeholder_case() -> void:
    result := text
        |> lines
        |> map(_, trim_line)
        |> filter(_, nonempty)
        |> join(_, ",")
```

All leading-pipe continuations parse. Whitespace normalization is deliberately represented by a named callback. An indented callback suite inside the step or a trailing block after the pipe remains invalid for all candidates. Bind that callback outside the chain, then pass its name; changing layout is a separate question.

### C13. Fallible transformation versus fallible function lookup

Original shape (an approximation):

```rust
let number = parse_number(text)?;
let result = functions.get(key).ok_or(Missing)?(number);
```

```text
fn elixir_case() -> void:
    result := text |> parse_number()? |> apply_lookup(key)?
fn fsharp_case() -> void:
    result := (text |> parse_number())? |> (lookup(key)?)
fn hack_case() -> void:
    result := text |> parse_number(_)? |> lookup(key)?(_)
fn placeholder_case() -> void:
    result := text |> parse_number(_)? |> lookup(key)?(_)
```

Elixir's `parse_number()` is an inserted call and F#'s is a factory returning a parser. The F# parentheses deliberately propagate the parser's result, while `lookup(key)?` propagates before invoking the retrieved function. Hack/placeholder place the input in that invocation explicitly. The lookup facade produces a Result-wrapped unary function; no candidate invents implicit Result chaining.

### C14. Suspending step and explicit cold work

Original shape (an approximation):

```rust
let response = load(request).await?;
```

```text
fn elixir_case() -> void:
    result := request |> load!()?
fn fsharp_case() -> void:
    result := request |> (fn(value): load!(value)?)
fn hack_case() -> void:
    result := request |> load!(_)?
fn placeholder_case() -> void:
    result := request |> load!(_)?
```

The direct bang calls visibly run work. The F# lambda spelling cannot hide suspension inside an ordinary closure: a suspending callback needs `fn!`, which the safety completion rejects as a final implicit unary step. It therefore needs an ordinary factory returning a cold job followed by an explicit bound `job!()` outside the pipe, or explicit direct application outside this strict F# pipe. Its displayed lambda parses but fails the intended effect typing; this is friction, not a claimed working program. A cold constructor `load(value)` is distinct from execution in every candidate.

### C15. Policy chain from #783

Original shape (the hd policy design):

```hd
retry(backoff) |> with_timeout(ms(50), _) |> fallback(backup, _)
```

```text
fn elixir_case() -> void:
    result := retry(backoff) |> with_timeout(ms(50)) |> fallback(backup)
fn fsharp_case() -> void:
    result := retry(backoff) |> with_timeout(ms(50)) |> fallback(backup)
fn hack_case() -> void:
    result := retry(backoff) |> with_timeout(ms(50), _) |> fallback(backup, _)
fn placeholder_case() -> void:
    result := retry(backoff) |> with_timeout(ms(50), _) |> fallback(backup, _)
```

For Elixir the reviewed facade takes `(inner, limit)` and `(inner, backup)`; F# factories take configuration then return unary wrappers. Hack/placeholder use the existing work-last illustrative shape directly. All build policy data, not running work; `apply!(policy)` with its block stays outside the pipe. This preserves timeout outside retry, followed by fallback. A timeout per attempt instead puts timeout before retry in the tuple; changing order changes behavior. Existing policy runtime evidence lives in PIPE_STEPS and POLICY_COMPOSITION, not in these facade-only parse cases.

### C16. Effectful configuration and reusable partial

Original shape (an approximation):

```scala
names.map(pad(_, nextWidth()))
```

```text
fn elixir_case() -> void:
    result := names |> map(fn(value): pad(value, next_width()))
fn fsharp_case() -> void:
    result := names |> map(pad(next_width()))
fn hack_case() -> void:
    result := names |> map(_, fn(value): pad(value, next_width()))
fn placeholder_case() -> void:
    result := names |> map(_, pad(_, next_width()))
```

The F# factory evaluates `next_width()` once while configuring pad. The literal callback expansions in Elixir/Hack and Pα evaluate it for each element. All first evaluate the pipe input before step configuration. To make effects match, prebind width for all four, or explicitly put next_width inside an element callback in all four.

Fresh `effects.hd` runs both real desugarings on both engines.
It prints widths `1/1`, `2/1`, `3/1`, then `calls 3 1`, confirming repeated versus creation-time evaluation.
Reusing a curried factory or capture-once partial can also retain shared mutable state; duplication is not automatically safe.


## Grammar And Inference

The parser does not distinguish the four meanings of a call-shaped pipe.
Consequently a clean parse of `xs |> map(f)` proves nothing about first insertion or a curried factory.
Current checking still rejects zero-slot calls with `pipe-step-needs-placeholder`.
General `_` partials remain unimplemented and conflict with accepted `expr.pipe.no-partial`.

| Interaction | Elixir | F# | Hack / placeholder |
| --- | --- | --- | --- |
| `Type::m` | Direct declaration call with inserted first argument | Ordinary function value; bound receiver remains bound | Explicit slot works; placeholder bare declaration follows current call behavior |
| `f::[T](...)` | Insert into explicitly instantiated call | Instantiate factory, then call its returned unary function | Instantiate direct callee and substitute slot |
| Named arguments | Keep labels after inserted argument | Configure factory with labels; returned function type has no labels | Place slot in positional or named argument under ordinary binding |
| Defaults | Declaration calls may omit parameters | Factory call has defaults; bare function value carries full arity | Direct declaration calls retain defaults |
| Varargs | Insert before written arguments/spread | Factory must package configuration and return one input slot | Written spread remains after explicit slot |
| `?` | Propagates inserted call result | Inside step, propagates lookup/factory result before invoking its function | Inside step, propagates substituted call result |
| `!` | Marks inserted call | Marks an explicit factory call, not the later implicit unary call | Marks the substituted call |
| Trailing block | Existing prohibition | Existing prohibition | Existing prohibition |
| Multiline chain | Leading `|>` works | Leading `|>` works | Leading `|>` works |

Fresh controls in [the probe directory](../../bench/study/pipe-styles/probes/) retest #818's claims with the frozen compiler described below.
`inference.hd` infers a builder's input from `feed(+7, with_timeout(50))` and prints `7/50` on both engines.
`inference-result-context.hd` also infers an otherwise hidden result element from an annotated `List[string]` result.
`parser-builder-model.hd` checks explicit generic method values and a returned generic parse function.

The nested nonempty requirement-row counterexample still fails.
`returned-row-gap.hd` reports `type-mismatch`, including “permissions inside a type convert only by declared variance”.
The empty-row control succeeds; explicitly passing `keep::[$ Console]()` succeeds and prints `hello` on both engines.
Thus F# input inference works for these scalar/list builders, but generic returned capability-bearing callbacks need an annotation today.

`defaults-current.hd` prints `hd!`; `defaults-value.hd` rejects the same declaration as a unary function value.
Its message is `in argument: expected fn(string) -> ?, found fn(string, string) -> string`.
`forms-model.hd` executes stored/indexed functions, bound/static method references, explicit generic values, and a lambda on both engines.
The outputs are `7`, `8`, `6`, `9`, `6`, `hi hd`, and `4`.

`suspending-factory.hd` executes an explicit `factory!()` before invoking its returned ordinary function, then an explicit substituted bang call.
Both print `4` twice.
`suspending-value.hd` checks successfully through ordinary constructor weakening; that is evidence for the guard question, not proof that bare suspending pipes are safe.
Today's `suspending-bare.hd` rejects the bare form with `suspending-pipe-step`.

The returned suspending-callback variant `fn-input-transform.hd` also fails the nested nonempty-row inference control.
It reports the same variance limitation; the ordinary `factory!()` control succeeds because the explicit factory obtains its requirements directly.
`multiline-rejected.hd` reports `multi-line-pipe-step` in checking.
`trailing-block-rejected.hd` is rejected earlier by parsing at its colon with `syntax-error`.

### Diagnostic And Fix-it Requirements

These are proposed diagnostics, not messages added to the compiler.
Current negative-control messages and empty fix lists are retained in the evidence.
Fixes must preserve source expressions and evaluation order, rather than substitute an example's literals.

| Mistake | Useful diagnostic | Source-preserving fix |
| --- | --- | --- |
| Elixir `body |> replace(pattern, replacement)` against pattern-first API | Identify the actual first parameter and inserted subject | Change to explicit unary lambda with `body` in the declared slot |
| F# `n |> scale(3)` where `scale` returns a number | Step produces `i64`; expected a unary function | Use a unary lambda, or the actual configured factory |
| F# bare defaulted multi-argument declaration | Show its full function arity | Write `fn(x): greet(x)` so declaration defaults still apply |
| F# `text |> parse_as()?` when factory returns plain function | `?` applies to factory, not mapped result | Parenthesize whole pipe before `?`, or bind result then propagate |
| Hack bare `count` | Step needs one input marker | Replace that path with `count(_)` |
| Duplicate Hack marker | Two uses of the pipe input | Bind input once and name it twice outside the step |
| Placeholder nested ownership collision | Highlight both owners, not just second underscore | Replace only inner partial with `fn(s): pad(s, 10)` |
| Bare suspending value | Step would construct a cold job instead of running it | Spell explicit bang call with a visible input slot or explicit lambda |
| Multiline or trailing-block step | Point to suite, with outer pipe context | Bind callback before chain and pass that name |

An Elixir same-type slot mistake may type-check, so no general type-error fix can catch it.
A convention lint or signature documentation can warn; changing overload resolution to guess the subject would add a new mechanism.
For effectful expressions, fixes that move configuration outside a callback must disclose changed timing.

## Standard Library Cost

The census scans balanced parameter lists of explicit `pub fn` declarations, including indented inherent methods.
It is a declaration inventory, not a count of unique operations or every implicit-public trait method.
Overloads and generated impls are not deduplicated; constructors and unrelated functions remain visible for honest upper bounds.
The saved inventory names every path, line, parameter list, and receiver classification.

| Inventory at root source `22ad8e0` | Including runtime | Excluding `lib/std/rt*` |
| --- | ---: | ---: |
| Explicit public declarations | 558 | 494 |
| Top-level public functions | 148 | 92 |
| Top-level functions with at least two parameters | 32 | 23 |
| Public declarations with `self` receiver | 364 | 357 |
| Receiver plus at least one other parameter | 193 | 189 |
| Declarations with defaults | 10 | 10 |
| Declarations with varargs | 3 | 3 |

No evidence supports claiming all 494 APIs must change.
The concrete exposure is 23 non-runtime multi-argument free functions and 189 configured receiver operations, before selecting a useful shell facade.
Subject classification is semantic: parameter count alone cannot tell whether a parser's schema or its text is the subject.
Private helpers, callers, docs, rules, and fixtures add migration work beyond this census.

| Style | Required convention | Practical change |
| --- | --- | --- |
| Elixir | Subject-first direct facade | Review 23 multi-argument free functions; expose selected methods as free adapters or method-reference calls |
| F# | Configuration-first factory returning unary function | Curried replacements/wrappers for selected free functions and up to 189 configured receiver operations; update direct callers to two calls |
| Hack | None | Existing signatures can be called with an explicit slot; add shell adapters only for useful operations |
| Placeholder | None | Same direct facade freedom as Hack; general partials add language semantics instead of std wrappers |

Text's `starts_with(self, prefix)`, `split(self, separator)`, and `replace(self, old, replacement)` already put their receiver first.
They are not automatically free functions just because a pipe exists.
An unbound `string::split` provides the receiver parameter; a bound `value::split` already closes over its receiver.
None of the candidates needs a fifth dot lookup.

`task.timeout!(limit, cleanup=..., work=...)` and `retry!(policy, when=..., attempt=...)` illustrate operations whose block/work subject is last.
Elixir needs subject-first wrappers to pipe the work; F# needs ordinary factories around configuration, rather than executing the suspending operation while configuring it.
Hack and placeholder can name the existing work slot directly.
Named arguments and trailing-block ergonomics make a blanket reorder especially costly here.

Core-library wrappers are cheaper mechanisms than automatic currying or subject-guessing exceptions.
Nevertheless, a broad F#-only facade multiplies exported shapes and captured environments under today's compiler.
There is no compatibility barrier, so replacing APIs is possible; the question is whether two-call direct use is worth the resulting convention.

## Measured Performance

### Reproduction And Provenance

Sources and scripts live in [bench/study/pipe-styles](../../bench/study/pipe-styles/README.md).
The six real packages use the same `common.hd`, package name, datasets, eager algorithms, verification checksum, and output.
Direct models Elixir, Hack, and an explicit-slot placeholder pipe; curried models F# factories; partial models an outside-pipe unary lambda.
No compiler recognizes any proposed syntax in this measurement.

The host was arm64 Apple M3 Max, Node `v24.19.0`, Python `3.14.7`.
The **dev-profile compiler** is frozen from source `16fd4789bf3dda1ada8cab1710c8f5ff5fc21880`.
Its SHA-256 is `d6dd4dc62db537d98558d3bb0cc1eaf8439c46a4a5ce1b49c4f5eda61fc9c625`.
Programs are optimized with `hd build --release`; the compiler itself was not built with Cargo's release profile.

The study branch started at `22ad8e0653b61e86f48f2ff845ecf57edd3cffb0`.
A root build unexpectedly reused the shared target's approved #827 follow-up CLI; a hash audit caught the source mismatch before reporting.
The producer confirmed its one-way freeze from that follow-up tree, so measurements are attributed to `16fd4789`, not falsely to root main.
Backend, ownership, inliner, parser, and std executable sources are identical between those two trees; driver entry metadata differs.

There are 21 fresh-process runs per dataset/form/engine: **252 checked timings**.
Forms rotate, engines alternate, and dataset order reverses on alternate rounds.
A discarded full-input preflight verifies each artifact; measured processes receive no warmed module or persisted JIT state.
This is the V8-only workload policy of study `--quick`, explicitly extended to 21 runs and a matching Wasmtime pass.

Wall time includes process startup, loading/compilation, guest work, checksum, and output.
CPU is child user plus system time, not CPU inferred from wall time; V8 worker threads can make CPU exceed wall time.
Each run verifies status, exact output, checksum, and empty stderr against independent Python computation.
Other agents held heavy tests during timing; sampled one-minute load ranged from 5.58 to 7.16.

Numeric input is one million `i64` values, filtered by remainder, mapped to decimal text with offset seven, then joined.
String input is one million padded rows, filtered by substring, trimmed/lowercased with a suffix, then joined.
Both print kept/mapped counts, joined byte length, and a rolling checksum.
These workloads compare pipe lowering cost, not lazy fusion, streaming memory bounds, or foreign-language runtimes.

### Timing And Size

All times are milliseconds; wall IQR describes dispersion, not a confidence interval.
Function count is defined Wasm functions, excluding imports; code bytes are the entire code-section payload.
Node and Wasmtime use separately built target artifacts, so cross-engine module sizes need not match.

| Input | Form | Engine | Wall median | CPU median | Wall IQR | Wasm bytes | Functions | Code bytes |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| numeric | direct | V8 | 128.76 | 138.55 | 7.28 | 16,728 | 110 | 11,175 |
| numeric | curried | V8 | 134.10 | 143.29 | 7.39 | 21,906 | 152 | 14,201 |
| numeric | partial | V8 | 129.07 | 137.32 | 9.63 | 16,744 | 110 | 11,191 |
| numeric | direct | Wasmtime | 126.03 | 123.65 | 7.77 | 17,864 | 108 | 12,356 |
| numeric | curried | Wasmtime | 133.28 | 129.58 | 10.92 | 24,551 | 150 | 16,903 |
| numeric | partial | Wasmtime | 125.68 | 122.97 | 5.64 | 17,880 | 108 | 12,372 |
| string | direct | V8 | 558.21 | 633.96 | 18.42 | 20,681 | 119 | 14,219 |
| string | curried | V8 | 575.29 | 652.13 | 26.61 | 26,012 | 161 | 17,384 |
| string | partial | V8 | 565.35 | 642.36 | 18.29 | 20,697 | 119 | 14,235 |
| string | direct | Wasmtime | 553.58 | 548.09 | 20.19 | 21,720 | 117 | 15,303 |
| string | curried | Wasmtime | 581.66 | 574.83 | 27.12 | 28,491 | 159 | 19,921 |
| string | partial | Wasmtime | 550.57 | 542.81 | 26.44 | 21,736 | 117 | 15,319 |

Curried wall medians are 4.15%/5.76% higher for numeric V8/Wasmtime and 3.06%/5.07% higher for strings.
Corresponding CPU medians rise 3.42%/4.79% and 2.87%/4.88%.
The partial/direct differences range from −0.54% to +1.28% wall, within the observed dispersion; there is no demonstrated useful speed difference.
Curried modules add 42 functions and 5,178–6,771 bytes; partial adds 16 code bytes, so it is not byte-identical to direct.

These are observed medians, not a claim that every curried pipeline is 5% slower.
Extra compiled functions affect startup; closure environment reads and indirect calls affect execution; string work and checksum dilute both.
The experiment does not isolate those contributions.
It does rule out assuming today's inliner erases this idiomatic curried implementation.

### Allocations And Captures

The allocator's existing `GIVEN` counter counts allocation requests, including reused slots and specialized class fast paths.
Scratch-only Wasm wrappers read its four bytes after successful original `hd.poll`; production compiler/runtime code is untouched.
The wrapper refuses an unrecognized allocator layout, and every instrumented execution must preserve exact output.
Four fresh executions reconstruct each 32-bit count at both 100 and one million inputs, on both engines.

| Input | Form | Requests at 100 | Requests at 1M | Requests per input at 1M |
| --- | --- | ---: | ---: | ---: |
| numeric | direct | 68 | 333,420 | 0.333420 |
| numeric | curried | 72 | 333,424 | 0.333424 |
| numeric | partial | 68 | 333,420 | 0.333420 |
| string | direct | 327 | 2,941,257 | 2.941257 |
| string | curried | 331 | 2,941,261 | 2.941261 |
| string | partial | 327 | 2,941,257 | 2.941257 |

Both engines return these same counts.
The four extra requests correspond to filter, outer map, configured element mapper, and join environments created once per pipeline.
They are **not four allocations per element**.
A program building such a curried chain inside its element loop would instead pay those creations repeatedly.

Literal partial callbacks here have no captures, as do the direct form's corresponding explicit lambdas.
They therefore add no allocation requests; this does not prove a partial capturing dynamic configuration allocates nothing.
`emit/closure.rs` emits a zero environment for no captures and a counted object for captured values.
Its shared mutable captures use cells; capture handling can also prevent the ownership borrow optimization.

These are guest allocator requests, not live-object counts, retained bytes, JavaScript allocations, or Wasmtime's native allocations.
They include input construction and common text-building work.
The denominator is input elements, not filtered survivors.
The 100-element control distinguishes fixed setup from input-proportional overhead.

### What Today's Inliner Removes

The planner in `crates/hd_driver/src/own.rs` only considers small nonintrinsic std callees outside `std.rt`.
The limit is eight TIR instructions, not eight source lines.
`hd_tir/src/inline.rs` rejects closure creation, captures, dynamic calls, suspension, providers, ownership statements, and multiple sub-bodies.
It also skips omitted-argument/provider calls, and declines splicing into callers containing dynamic calls.

These benchmark helpers are package functions, so the planner does not inline them in the first place.
Moving the same factories into std would not suffice: their `Closure` and capture bodies still fail eligibility.
All four factory names and returned closure bodies remain in emitted `hd.names`, and allocation counts confirm materialized environments.
There is no measured factory/closure erasure to credit to the inliner.

| Style | Pipe-specific runtime work | What is needed for a zero-cost lowering |
| --- | --- | --- |
| Elixir | Saved input plus direct call | Correct call insertion; ordinary lowering can keep temporary in locals |
| Hack | Saved input plus substituted expression | Correct single evaluation and normal call lowering |
| Placeholder inside pipe | Same direct substitution or bare call | Same as Hack; no partial factory is needed for its pipe slot |
| F# configured step | Factory call, captured environment, unary indirect call | Inline factory, propagate known target/captures, remove environment and its retains/releases; preserve order |
| Placeholder outside pipe | Unary callback, possibly captured environment | Stateless lambda needs no heap environment; captured lambda needs specialization/escape-safe environment elimination |

Devirtualization and scalar replacement are plausible future optimizations, not existing guarantees.
Increasing the eight-instruction limit alone cannot remove rejected closures or bypass package exclusion.
Ownership must preserve captures across calls and suspension, so silently borrowing a captured temporary is not a valid optimization.
Source hoisting can avoid repeated factories, but moving effectful configuration or changing mutable-capture identity is observable.

## Comparison With Other Languages

| Source | Borrowed property | Difference in this study |
| --- | --- | --- |
| Elixir Enum | Subject-first eager functions and input insertion | hd has static signatures and explicit suspension; the facade is illustrative |
| Elixir Stream | Lazy recipe, consumed later | Laziness comes from iterator adapters, independent of pipe selection |
| F# List / Seq | Unary step produced by partial application | hd uses explicit factory declarations and tupled calls, not F# automatic currying |
| TC39 Hack draft | Topic may occur in an arbitrary expression | This issue additionally requires one `_` and rejects bare steps |
| Scala placeholders | Anonymous callback shorthand | Innermost-call ownership does not automatically coexist with hd pipe ownership |
| shell / jq | Left-to-right transformation notation | hd pipes pass values; they do not launch concurrent commands or implicitly flatten result streams |

Elixir distinguishes eager Enum traversal from lazy Stream recipes in its [Enum documentation](https://elixir.hexdocs.pm/Enum.html) and [Stream documentation](https://elixir.hexdocs.pm/Stream.html).
F# documents curried functions and partial application in [Functions](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/functions/), and lazy element production in [Sequences](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/sequences).
Those explain C1–C4's API shapes; they do not establish hd performance.

The [TC39 explainer](https://github.com/tc39/proposal-pipeline-operator) motivates an explicit topic across varied expression forms; its [draft specification](https://tc39.es/proposal-pipeline-operator/) is a proposal, not shipped JavaScript semantics.
Scala's [placeholder-expression specification](https://www.scala-lang.org/files/archive/spec/3.4/06-expressions.html) supplies the comparison for C9.
The [jq manual](https://jqlang.org/manual/) explicitly describes filters producing multiple results, which this report's materialized list port does not reproduce implicitly.

## Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | PS1 | Pipe slot versus partial owner | Silent alternative meaning or rejection | C9, C16 |
| 2 | PS2 | Cold work and configuration timing | Deferred or repeated effects | C13, C14, C16 |
| 3 | PS3 | Currying is not erased today | Measured size/runtime cost; inference limit | C3, C4, C10, C11, C15, benchmarks |
| 4 | PS4 | First-subject convention meets arbitrary APIs | Same-type wrong-slot call can succeed | C7, C8, C11, C15 |
| 5 | PS5 | Pipe layout is not callback layout | Workaround needed | C12 |
| 6 | PS6 | Uniform explicit marker adds noise | Reading/writing burden | All common unary chains |

### PS1. One Underscore Has Two Owners

**Effect.** C9's outer list slot and inner string partial look identical.
The rule sheet promises both features but does not establish their boundary.
Pα handles the motivating example while changing existing nested-call substitution; Pβ preserves current ownership while requiring explicit inner lambdas.
C16 adds the separate question of when a partial evaluates its fixed expressions.

**Candidates.** Should general partials be deferred while bare and explicit-slot pipes remain?
Should Pα's structural boundary be accepted, with a diagnostic for nested-call substitution?
Or should inner partials require a distinct explicit marker, after the owner evaluates the extra syntax cost?

### PS2. A Function Step Can Hide When Work Runs

**Effect.** C13's `?` may propagate a factory error rather than the transformed value's error.
C14 can return a cold suspension under ordinary function-value weakening.
C16's literal lambda expansion repeats configuration effects at each callback call, while a curried factory usually configures once.
These differences survive successful parsing and can survive successful typing.

**Candidates.** Should final `fn!` step values be rejected before constructor weakening, as the explicit F# completion proposes?
Should general partials follow literal lambda timing, or capture fixed arguments at partial creation?
The latter is a separate choice from accepting either pipe style.

### PS3. Curried Factories Have A Current Cost

**Effect.** F#'s idiomatic form adds four captured environments, 42 defined functions, and 5.2–6.8 KB in the measured programs.
Its wall medians rise 3.1–5.8%; the four allocations alone do not explain that timing.
Scalar expected-type inference succeeds, but returned nonempty capability rows still require an explicit argument.
Defaults and direct calls need different API treatment.

**Candidates.** Is the compact syntax worth today's measured cost and broad std facade change?
Should a narrow curried facade remain ordinary library code while direct slot pipes are the default?
Or should factory inlining and closure elimination be a measured prerequisite before adopting F# as the universal convention?

### PS4. First Insertion Can Be Quietly Wrong

**Effect.** In C8, two strings can swap subject and configuration without a type error.
C7's expression and policy work-last functions need adapters under first insertion.
A subject-first std reduces this risk but cannot impose its convention on every imported API.
Named arguments expose the conflict rather than determining a subject automatically.

**Candidates.** Should Elixir require a documented subject-first std facade and explicit closures for foreign shapes?
Should explicit slots remain available for non-first subjects?
The latter would be a hybrid requiring its own reviewed rule sheet, not the pure Elixir candidate tested here.

### PS5. Layout Still Bounds Expressiveness

**Effect.** C12's multiline closure and trailing block remain invalid in all four styles.
Compact pipe spelling does not remove that restriction.
Binding a callback first works and keeps the suite outside the step.

**Candidates.** Should current layout stay, with a source-preserving bind-callback fix?
Or should a separately scoped layout proposal be investigated after choosing the core pipe rule?

### PS6. Mandatory Markers Cost Readability

**Effect.** Hack repeats `_` even for `lines`, `count`, and `sum`.
The Haiku sample's pipe characters are about 25% above Elixir and placeholder; that is length evidence, not an error-rate finding.
The uniform rule still helps readers locate non-first subjects and arbitrary expressions.

**Candidates.** Does uniform input visibility outweigh the extra markers?
Or should bare unary declaration calls remain as the one shortcut, without general partial application?

## Parse Log

All 17 report blocks parse with the source-compatible `hd_syntax` parser: one four-style block for each case, plus C15's original hd expression.
The exact log is [results/parse.json](../../bench/study/pipe-styles/results/parse.json); `parse.py` extracts both `text` and `hd` fences.
No new grammar spelling is assumed, so there are no untested hypothetical syntax lines.
Hypothetical facade declarations and candidate semantics remain explicitly unimplemented despite clean parsing.

| Blocks | Case | Parser result | Checking status |
| --- | --- | --- | --- |
| 1–9 | C1–C9 | 9/9 clean | Facade ports only; C9 placeholder ownership unresolved |
| 10–12 | C10–C12 | 3/3 clean | Facade ports only; method/default/layout controls checked separately |
| 13–14 | C13–C14 | 2/2 clean | Facade ports only; C14's F# ordinary lambda intentionally has invalid effect typing |
| 15 | C15 original | 1/1 clean | Existing policy spelling; no new runtime claim |
| 16–17 | C15 variants, C16 | 2/2 clean | Facade ports only; configuration timing described explicitly |

The six generated benchmark packages check successfully and execute the 252 measured runs with exact results.
Fourteen reused #818 controls produce ten successful checked programs and four expected checker errors.
Nine successful controls execute on both engines; the cold-function control is check-only.
Three additional controls preserve the expected multiline, trailing-block, and nonempty-row errors.

`partial-outside.hd` and `double-placeholder.hd` reproduce `placeholder-outside-pipe` and `duplicate-pipe-placeholder` respectively.
They also report intentional unused-binding warnings; the original messages and empty fix lists are preserved.
`effects.hd` checks cleanly and runs on both engines, confirming C16's different configuration timing.
Its receipt is [effects-probes.json](../../bench/study/pipe-styles/results/effects-probes.json).

Parsing the standalone negative controls also distinguishes stages: the trailing-block file fails parsing; the suite and type-negative files parse.
Negative controls are not conformance exceptions and do not change KNOWN_FAILURES.tsv.
Their full current messages are in [probes.json](../../bench/study/pipe-styles/results/probes.json) and [additional-probes.json](../../bench/study/pipe-styles/results/additional-probes.json).

## Rankings And Owner Questions

**Recommendation for agents:** Hack first, Elixir second, placeholder third, F# fourth under today's compiler and literal candidate scope.
Hack makes placement explicit, supports arbitrary APIs, and needs no currying facade or partial-ownership inference.
Elixir is shorter and strong for a deliberately subject-first shell facade; its wrong-slot risk keeps it second.
Placeholder could move first if general partial ownership is resolved or deferred; the uncompleted four-feature sheet is not yet exact enough.

**Recommendation for human readers:** Elixir first, Hack second, placeholder third, F# fourth for mixed shell and effectful hd programs today.
Elixir's common left-to-right subject flow is compact; Hack's extra markers buy explicitness at foreign calls.
F# remains attractive for readers already fluent in curried APIs, but this ranking includes its factory typing, cold-work guard, and measured current cost.
These rankings are judgments over the stated cases, not results of a new human study or an observed Haiku failure rate.

The cheapest alternative worth owner review is bare unary declaration calls plus existing explicit-slot calls, **without** general partials.
It adds no automatic currying mechanism, keeps current direct lowering, and avoids PS1.
That is a narrowed fourth candidate, not an owner decision or a silently substituted implementation.

| Rank | Owner question | Concrete effect | Candidate answers |
| --- | --- | --- | --- |
| 1 | Must general `_` partials ship with pipe selection? | C9's `map(_, pad(_, 10))` needs two ownership scopes | Defer partials; Pα structural ownership; separate explicit partial syntax |
| 2 | Is first-subject convention or explicit input placement more valuable? | C8's pattern-first imported function can silently accept swapped strings | Pure Elixir facade; pure Hack; separately specify bare-plus-slot |
| 3 | Should pipe steps reject final suspending function values before weakening? | C14 can produce a cold job without running it | Early guard; explicitly permit construction with strong diagnostics |
| 4 | If partials exist, when do their other arguments evaluate? | `pad(_, next_width())` can configure once or once per item | Literal lambda timing; capture-once timing |
| 5 | Is F# worth its present cost, or conditional on optimization? | Four environments, +42 functions, +5.2–6.8 KB, modest measured slowdown | Adopt measured cost; narrow library facade; require proven elimination |
| 6 | Should pipe layout change in this decision? | C12 requires prebinding a multiline callback | Keep existing layout; investigate separately after core choice |

No implementation or spec rewrite follows until the owner answers the core questions.
