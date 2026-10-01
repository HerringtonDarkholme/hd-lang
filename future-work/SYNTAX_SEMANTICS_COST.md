# Syntax And Semantics Cost Review

Status: simplification review, 2026-09-30. It changes no decision, spec
text, or prototype code, and nothing in it is accepted behavior. It covers
the special syntax forms and semantic rule exceptions of the language tier,
chapters [01](../spec/01-lexical-structure.md) to
[14](../spec/14-annotations.md). It builds on the inventory in
[Special Cases](SPECIAL_CASES.md) and the open map-read question in
[Call Indexing](CALL_INDEXING.md#q4-map-read-type).

The owner wrote: "i don't think error code is the most important
complexity. i wonder if syntax/semantics are more important". So this
record ignores diagnostic-code cuts. It asks what a user must learn: each
form's rules, how often real code uses it, and what users would write
without it. The stdlib tier in [spec/std/](../spec/std/README.md) is not
counted, following the [tier split](../AGENTS.md#spec-scope-for-the-standard-library).

## Contents

- [Summary](#summary)
- [Method](#method)
- [Cost Table](#cost-table)
- [Cut And Merge Candidates](#cut-and-merge-candidates)
- [Reopen Candidates](#reopen-candidates)
- [Keep](#keep)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Summary

The forms below take about 940 of the language tier's 3,687 numbered rule
items. Most of them earn their cost: bindings, same-line suites, trailing blocks,
`::`, `$` rows, and bang calls appear dozens to hundreds of times in real
code. The cheap wins are small exceptions around name lists and `let mut`.
The expensive forms with almost no real use are embedding, literal sugar,
and packs.

"Real code" here means the guide, `lib/std`, `examples/`, and the
playground examples. Fixtures and spec examples are counted apart. Note
that the guide and the playground mostly demonstrate features, so one
teaching block can be a form's whole real usage. `lib/std`, the only
production-like hd code, is the harder test.

### Top 5 Cuts Or Merges

| Rank | Cut | Absorbed by | Rules removed | Changes valid source | Soundness |
| ---: | --- | --- | ---: | --- | --- |
| 1 | [K1](#k1-one-name-list-shape-in-for) `for (k, v) in m`, as in `let (a, b)` and `(a, b) :=` (SPECIAL_CASES C4) | `grammar.stmt.bind-list` (now [`grammar.stmt.short-binding.one-name`](../spec/02-grammar.md#r-grammar.stmt.short-binding.one-name)), `grammar.inline.bind-list` | 1 exception, 1 of 3 list spellings | yes, with a fix-it | holds |
| 2 | [K2](#k2-let-mut-takes-no-annotation) `let mut` takes no type annotation | [`types.bind.let-mut`](../spec/04-type-system.md#r-types.bind.let-mut): the annotation states access | 7 rules become 2; 1 warning | yes: a warned form becomes an error | holds |
| 3 | [K3](#k3-a-multi-name-binding-is-a-statement) A multi-name `:=` is a statement only | `grammar.expr.multi-binding` | 1 rule, 1 nested form | yes: `((a, b) := p)` becomes an error | holds |
| 4 | [K4](#k4-a-comprehension-follows-its-loop) A comprehension follows the loop it abbreviates (SPECIAL_CASES C7) | [`expr.comp.shape`](../spec/05-expressions.md#r-expr.comp.shape), [`req.bang.driver-contexts`](../spec/11-requirements-and-suspension.md#r-req.bang.driver-contexts) | 3 rules | invalid becomes valid | holds |
| 5 | [K5](#k5-one-dollar-rule-for-every-string) One `$` rule for every string (SPECIAL_CASES C5) | `lex.prefix.plain-dollar-start` | half of 1 rule | invalid becomes valid | holds |

A sixth, [K6](#k6-readonly-iterators-in-loops) (SPECIAL_CASES C6),
deletes the readonly-iterator loop error; the owner kept the error
(batch 24). All six are independent. Batch 26 took K1, K3, K4, and K5,
and kept K2's rules.

### Top 3 Reopen Candidates

These are owner decisions whose cost-to-usage ratio is poor. The record
shows the evidence and does not recommend a reversal.

| Rank | Form | Rules | Spec lines | Real uses | `lib/std` uses | Decided by |
| ---: | --- | ---: | ---: | ---: | ---: | --- |
| 1 | [Embedding, promotion, and `by` delegation](#r1-embedding-promotion-and-delegation) | 137 | 534 | 16 lines, all in the guide | 0 | VE1-VE4, VE-S, embedding limits, trait delegation |
| 2 | [Literal suffixes and string prefixes](#r2-literal-suffixes-and-string-prefixes) | 81 | 461 | about 11 suffixes, 5 prefixes | 5 declarations, 0 uses | L1-L22 |
| 3 | [Type and value packs](#r3-type-and-value-packs) | 72 | 221 | 2 signatures, both in the guide | 0 | GQ4, B8, `all!` motivation |

Four more follow in [Reopen Candidates](#reopen-candidates): the pipe,
row aliases, leading-dot continuation, and derivation member lines.

## Method

**Rule cost.** A rule is one numbered `r[...]` item in chapters 01-14. Each
form is a set of rule-ID prefixes, such as `expr.pipe.*` and `lex.pipe.*`.
Spec lines are the lines of the sections that define the form, subsections
included. Both counts come from a script over the chapters on 2026-09-30;
they are exact for the chosen prefixes, and the prefixes are a judgment.

**Usage.** Counts are regular-expression matches over hd code: fenced
`text` and `hd` blocks in `guide/*.md`, and the `.hd` files in `lib/std`,
`examples`, and `website/playground/examples`. Fixtures are the 1,646
`.hd` files under `spec/conformance`. Counts marked "about" come from a
looser pattern and may be off by a few.

| Corpus | Lines of hd |
| --- | ---: |
| Guide code blocks | 1,705 |
| `lib/std` | 3,131 |
| `examples/` and playground | 283 |
| Fixtures | 25,323 |
| Spec examples, chapters 01-14 | 4,472 |

**Verdicts.** *keep* means the form earns its cost. *cut* and *merge* name
an existing rule that absorbs the form. *reopen?* marks an owner decision
with a poor cost-to-usage ratio; the record shows evidence only.

## Cost Table

"Real" is guide + `lib/std` + examples. "Fixtures" counts conformance
files' occurrences. Precedent lists which of Go, Rust, Swift, Kotlin,
Python, and MoonBit have the same form; details and sources are in each
section.

| # | Form or exception | Rules | Spec lines | Real uses | Fixture uses | Without it | Precedent | Verdict |
| ---: | --- | ---: | ---: | --- | ---: | --- | --- | --- |
| 1 | `:=` and `let`, two binding forms | 26 | 116 | `:=` 209, `let` 211 | 1,023 / 741 | one keyword with a reassign marker | Go `:=`/`var`; Rust `let`/`let mut`; Swift `let`/`var`; Kotlin `val`/`var`; MoonBit `let`/`let mut`; Python one form | keep |
| 2 | `let mut`, and its annotation rules | 20 | 108 | 19 | 153 | `let x: mut T = e` | Rust and MoonBit `let mut`; Swift, Kotlin `var` | merge: [K2](#k2-let-mut-takes-no-annotation) |
| 3 | Parenthesized binding lists | 22 | 84 | 3 `:=` lists, 5 `let` lists, 1 bare `for` list | 14 / 18 / 20 | a tuple binding then field reads | Rust, Swift, Kotlin `(a, b)`; Go and Python bare | merge: [K1](#k1-one-name-list-shape-in-for), cut: [K3](#k3-a-multi-name-binding-is-a-statement) |
| 4 | Same-line suites and their limits | 20 | 82 | 87 suites, 280 one-line `fn` bodies | 675 | indented bodies | Python allows them and bans a nested compound statement | keep |
| 5 | Trailing blocks and their positions | 21 | 106 | 36 | 893 | an inline `fn():` argument | Kotlin trailing lambdas, Swift trailing closures | keep |
| 6 | Closures inside delimiters, several inline closures | 24 | 105 | every inline closure | many | none: layout needs them | Kotlin, Swift braces; Python has no multi-line lambda | keep |
| 7 | Pipe `\|>`, `_`, bare steps, restrictions | 37 | 179 | 4, one guide block | 49 | nested calls or `:=` steps | MoonBit `\|>`; none of Go, Rust, Swift, Kotlin, Python | reopen?: [R4](#r4-the-pipe) |
| 8 | Comprehensions, beside adapters | 20 | 93 | 11 (adapters 16) | 30 | a `for` loop or `map`/`filter` | Python only | keep the form; merge limits: [K4](#k4-a-comprehension-follows-its-loop) |
| 9 | `@num_suffix` and `@str_prefix` literal sugar | 81 | 461 | about 11 suffixes, 5 prefixes | about 54 / 34 | `ms(250)`, `sql(...)`, a raw-string form | C++ user literals; Scala, JS prefixes; none of the six | merged (batch 31c, 2026-09-30): one rule set for literal functions, 89 counted rules to 38, `-5s` is `-(5s)`; see [R2](#r2-literal-suffixes-and-string-prefixes) |
| 10 | `"""` strings and `$` interpolation | 21 | 62 | 2 multiline, 49 interpolated | about 5 / 140 | concatenation | Kotlin `"""` and `$`; Swift `"""`, `\(x)`; Python f-strings; MoonBit `\{x}` | keep; cut half a rule: [K5](#k5-one-dollar-rule-for-every-string) |
| 11 | `!` bang calls, cold calls | 33 | 122 | 58 calls, 31 `fn name!` | 500 | `await`-style keyword | Rust `.await`, Swift and Python `await`; Kotlin `suspend` unmarked at calls | keep |
| 12 | `$` requirement rows | 100 | 591 | 69 rows, 33 `$.use`/`$.with` | 417 / 292 | parameters or globals | Koka effect rows; none of the six | keep |
| 13 | Row aliases | 28 | 118 | 1 | 24 | write the keys out | Koka effect aliases | reopen?: [R5](#r5-row-aliases) |
| 14 | `::` references and qualified calls | 38 | 141 | 66 | 209 | a closure; no static call form | Rust, MoonBit `T::f`; Kotlin `::f`; Go `T.M` | keep |
| 15 | Derivation blocks and member lines `=`, `+=`, `= pass` | 55 | 268 | 6 blocks, 5 member lines | 70 / 50 | a hand-written impl | Rust derive attributes, serde field attributes | reopen?: [R7](#r7-derivation-member-lines) |
| 16 | `reified` | 9 | 11 | 5 | 15 | pass a `TypeShape` argument | Kotlin `reified` | keep |
| 17 | Packs `Ts...`, `pack.map` | 72 | 221 | 2 | 15 / 6 | fixed arities; an intrinsic signature for `all!` | Swift parameter packs, Python PEP 646; not Go, Rust, Kotlin, MoonBit | removed (batch 31b, 2026-09-30): `Tuple`-bounded varargs, tuple spread, and an `all!` typing rule; see [R3](#r3-type-and-value-packs) |
| 18 | Varargs and suffix spread `xs...` | 22 | 70 | 13 | 97 | a list argument | Go `xs...`, Kotlin `*xs`, Python `*xs`, Swift variadics | keep |
| 19 | `?` propagation | 38 | 190 | about 11 | about 55 | `match` on every call | Rust `?`; Swift `try`; MoonBit checked errors | keep |
| 20 | Embedding, promotion, `...` copies | 115 | 427 | 12 embedded lines, 9 copies | 127 / 79 | a named field | Go embedding; not Rust, Swift, Kotlin, Python | kept and simplified (batch 32, 2026-09-30): each fact stated once; a private own member may not share a promoted name (batch 33); see [R1](#r1-embedding-promotion-and-delegation) |
| 21 | `impl Tr for C by E` delegation | 22 | 107 | 4 | 11 | written forwarding methods | Kotlin `by` | kept as written forwarding (batch 32, 2026-09-30), with row 20 |
| 22 | `tests:` blocks | 26 | 128 | 7 | 475 | test modules only | Rust `mod tests`, MoonBit `test` blocks; Go `_test.go` | keep |
| 23 | Decorators and facts | 48 | 410 | 32 | 57 | none: metadata has no other home | Python decorators, Rust attributes, Kotlin annotations | keep |
| 24 | Newtypes `type X(T)` | 21 | 85 | 4 | 33 | one-field `data` | Rust tuple structs, Go defined types, Kotlin value classes | keep |
| 25 | Leading-dot continuation | 12 | 46 | 0 | 13 | one line, or parentheses | Swift, Kotlin allow; Python needs parentheses; Go needs a trailing dot | reopen?: [R6](#r6-leading-dot-continuation) |
| 26 | Paren-line and other line-start rules | 8 | 37 | 12 paren-start lines | 52 | none: `(a, b) :=` needs it | Swift, Kotlin have similar newline rules | keep |
| 27 | C4: bare `for k, v` beside parenthesized lists | 3 | in row 3 | 1 | 20 | `for (k, v)` | Rust, Swift `for (k, v)`; Python bare | merge: K1 |
| 28 | C5: `$` in a plain string is an error, in a prefixed one text | 3 | in row 10 | 0 | 2 | `\$` | Kotlin keeps a stray `$` as text | cut: K5 |
| 29 | C6: readonly iterator loop error | 1 | in row 8 | 0 | 1 | `source.iter()` | Rust loops over `&mut I` | cut: K6 |
| 30 | C7: no bang call in a comprehension | 1 | in row 8 | 0 | 1 | a `for` loop | Python PEP 530 allows `await` | cut: K4 |
| 31 | Q8: map keys, a compiler rule with its own code | 8 | 31 | every `Map` type | 5 | a bound on `Map` | Rust `K: Eq + Hash` bound | owner question [Q7](#q7-map-key-bound) |
| 32 | Q9: `m[k]` reads `V?`; `m[k] += v` reads `V` | 9 | 13 | every map read | 6 | `get` for the optional read | Rust, Python panic or raise; Swift optional | owner question [Q8](#q8-map-read-type) |

Row 20 counts rules from `data.embed`, `data.part`, `data.promote`,
`data.edge`, `names.promote*`, `trait.embed`, and the copy productions.
Rows 20 and 21 together give the 137 rules of R1.

## Cut And Merge Candidates

Each cut names the existing rule that absorbs it and shows parsed before
and after examples. Parsing checks syntax only; no block is claimed to
type-check. A line that parses only after a cut ends in
`# hypothetical syntax`.

### K1. One Name-List Shape In `for`

hd writes a name list three ways: `let (a, b) = p`, `(a, b) := p`, and
`for a, b in m:`. Batches 7 and 13 put the first two in parentheses
because "one shape reads better than two spellings". The loop is the one
left, and it alone forbids a same-line body.

**Before.**

```text
fn names(scores: Map[string, i32]) -> List[string] $ Console:
    let (low, high) = (0, 0)
    (first, second) := (1, 2)
    for name, score in scores:
        println(name)
    [for name, score in scores => name]
```

**After.**

```text
fn names(scores: Map[string, i32], ready: bool) -> List[string] $ Console:
    for (name, score) in scores:  # hypothetical syntax
        println(name)
    if ready: for (name, score) in scores: println(name)  # hypothetical syntax
    [for (name, score) in scores => name]  # hypothetical syntax
```

**Absorbed by.** The `binding_list` production of
`grammar.stmt.bind-list` (now [`grammar.stmt.short-binding.one-name`](../spec/02-grammar.md#r-grammar.stmt.short-binding.one-name)),
and `grammar.inline.bind-list`
for the same-line body.

**Soundness.** The change is syntactic. Destructuring, the arity check
`flow.for.tuple-arity` (since retired),
and scopes do not change. No program changes meaning.

**Cost.** Each multi-name loop gains two characters: 1 guide loop and
about 23 fixture files. Python writes the bare form, so agents will write
it; the fix-it is mechanical.

| Item | Change |
| --- | --- |
| `binding_pattern` production | deleted; loops and comprehension `for` clauses take `binding_target` |
| `grammar.inline.multi-name-for` | deleted: absorbed by `grammar.inline.bind-list` |
| [`grammar.inline.loops`](../spec/02-grammar.md#r-grammar.inline.loops) | reworded: a same-line `for` takes one name or a list |
| `flow.for.tuple-binding` (since retired) | reworded: `for (key, value) in entries` |
| New rule beside `grammar.stmt.bind-list.bare` | added: `for a, b in m` is `syntax-error` with a fix-it |
| About 23 fixtures with a multi-name loop | rewritten; rows unchanged |

**Other languages.** Rust and Swift write `for (k, v) in m` as a tuple
pattern ([Rust][rust-for], [Swift][swift-for]). Kotlin writes
`for ((k, v) in m)` ([Kotlin][kotlin-destructure]). Python and Go write
the bare form.

**Owner decision (batch 26, 2026-09-30): taken** ([Q4](#q4-parentheses-in-for)).

### K2. `let mut` Takes No Annotation

`let mut` exists to avoid repeating a type: `let mut user = User { ... }`.
With an annotation it has nothing to add, yet seven rules and two
diagnostics cover how `mut` and the annotation agree or clash. The guide
already teaches the annotated spelling as `let names: mut List[string]`.

**Before.**

```text
fn build() -> List[string]:
    let names: mut List[string] = []
    let mut ids: mut List[i64] = []  # warning: redundant-let-mut
    let mut tags: List[string] = []  # error: let-mut-readonly-type
    let mut draft = ["a"]
    names.append("Ada")
    names
```

**After.** An annotated binding states its access in the type. `let mut`
is for the unannotated form only.

```text
fn build() -> List[string]:
    let names: mut List[string] = []
    let mut ids: mut List[i64] = []  # error after the cut: syntax-error
    let mut draft = ["a"]
    names.append("Ada")
    names
```

**Absorbed by.** [`types.bind.let-mut`](../spec/04-type-system.md#r-types.bind.let-mut):
"a `let` annotation may state mutable access explicitly". A tuple writes
`let (first, second): (mut User, User) = pair()`.

**Soundness.** The removed rules protected against a `mut` that
contradicts the annotation. That program is still rejected, now as a
syntax error with one fix-it: move `mut` into the type. The warned form
`let mut x: mut T` becomes an error; real code has no such line.

**Cost.** One more spelling to forbid. A reader coming from Rust will
write `let mut x: T` and get the fix-it.

| Item | Change |
| --- | --- |
| `grammar.stmt.let-mut-single`, `grammar.stmt.let-mut-list` (now [`grammar.stmt.let-pattern.mut`](../spec/02-grammar.md#r-grammar.stmt.let-pattern.mut)) | reworded: `mut` before a name only when the `let` has no annotation |
| [`types.bind.let-mut-annotated`](../spec/04-type-system.md#r-types.bind.let-mut-annotated) | reworded: `let mut` with an annotation is `syntax-error` |
| [`types.bind.let-mut-annotated.fix`](../spec/04-type-system.md#r-types.bind.let-mut-annotated.fix) | reworded: the fix-it moves `mut` into the type |
| [`types.bind.let-mut-annotated.warning`](../spec/04-type-system.md#r-types.bind.let-mut-annotated.warning) | deleted; `redundant-let-mut` retires |
| [`types.bind.let-mut-annotation`](../spec/04-type-system.md#r-types.bind.let-mut-annotation), [`.fix`](../spec/04-type-system.md#r-types.bind.let-mut-annotation.fix) | merged into `let-mut-annotated`; `let-mut-readonly-type` retires |
| [`types.bind.let-mut-pattern.annotated`](../spec/04-type-system.md#r-types.bind.let-mut-pattern.annotated), [`.redundant`](../spec/04-type-system.md#r-types.bind.let-mut-pattern.redundant) | merged into `let-mut-annotated` |
| [`types.bind.let-mut-primitive`](../spec/04-type-system.md#r-types.bind.let-mut-primitive) and the other `let-mut` rules | unchanged |
| 5 fixtures with `redundant-let-mut` or `let-mut-readonly-type` | marker becomes `syntax-error` |

**Other languages.** Rust's `let mut x: T` exists because Rust has no
`mut` in types ([Rust][rust-let]). Swift and Kotlin pick `var` over `let`
or `val`, and the type never repeats it ([Swift][swift-let],
[Kotlin][kotlin-var]).

**Owner decision (batch 26, 2026-09-30): not taken.** Today's warning
and error stay ([Q6](#q6-let-mut-with-an-annotation)).

### K3. A Multi-Name Binding Is A Statement

`(a, b) := p` may also appear nested, wrapped as `((a, b) := p)`. Four
rules and a code police the nested spelling. No real code nests one; four
fixtures do.

**Before.**

```text
fn pair() -> (i32, i32): (1, 2)

fn sum() -> i32:
    whole := ((low, high) := pair())
    (a, b) := pair()
    low + high + a + b + whole._0
```

**After.**

```text
fn pair() -> (i32, i32): (1, 2)

fn sum() -> i32:
    whole := pair()
    (low, high) := whole
    (a, b) := pair()
    low + high + a + b
```

**Absorbed by.** `grammar.expr.multi-binding`:
"a multi-name short binding such as `(a, b) := value` is a statement".
The nested case becomes a plain syntax error.

**Soundness.** The wrapped form's rules existed so a nested list never
read as a tuple. Without a nested form, `[(a, b) := p]` and `((a, b) := p)`
are both errors, so nothing new is ambiguous.

**Cost.** A guard that tests a destructured value binds first, on its own
line. The single-name walrus, `if (n := f()) > 0:`, is unchanged.

| Item | Change |
| --- | --- |
| `grammar.expr.multi-binding.wrapped` | deleted; a nested multi-name binding is `syntax-error` |
| `grammar.expr.multi-binding.no-grouped` (since retired), `.no-grouped.fix` | reworded: the fix-it hoists the binding to a statement |
| `grammar.expr.multi-binding.tuple-element` (since retired) | unchanged |
| Fixtures `grammar-disambiguation.hd`, `binding-expression-tuple-value.hd`, `binding-expressions.hd` | rewritten or moved to invalid |
| Fixture `grouped-binding-expression.hd` | comment reworded |

**Other languages.** Python's `:=` takes one name only; PEP 572 leaves
out unpacking ([PEP 572][pep-572]). Go's `:=` is a statement
([Go][go-short-var]).

**Owner decision (batch 26, 2026-09-30): taken** ([Q5](#q5-nested-multi-name-bindings)).

### K4. A Comprehension Follows Its Loop

A comprehension "is equivalent in iteration shape to nested loops"
([`expr.comp.shape`](../spec/05-expressions.md#r-expr.comp.shape)), but
three more rules restate or break that. A `for` loop in a suspending body
may make bang calls; a comprehension there may not.

**Before.**

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]  # error: suspension-forbidden-context
```

**After.**

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]
```

**Absorbed by.** [`expr.comp.shape`](../spec/05-expressions.md#r-expr.comp.shape)
for evaluation, and [`req.bang.driver-contexts`](../spec/11-requirements-and-suspension.md#r-req.bang.driver-contexts)
for where a bang call is valid. `?` then needs no rule of its own: it
returns as it would from the loop.

**Soundness.** The frame already holds every local live across a
suspension point ([`req.lowering.frame`](../spec/11-requirements-and-suspension.md#r-req.lowering.frame)),
so a partial list is safe. Outside a suspending body the call is still
`bang-call-outside-suspension`. The jump and `let` bans stay.

**Cost.** A comprehension may pause mid-way. The `!` shows where.

| Item | Change |
| --- | --- |
| `expr.comp.no-suspension` | deleted |
| `expr.comp.propagation`, `.propagation.stop` | merged into `expr.comp.shape` as a Note |
| [`expr.comp.eager`](../spec/05-expressions.md#r-expr.comp.eager), [`expr.comp.no-jumps`](../spec/05-expressions.md#r-expr.comp.no-jumps), [`expr.comp.no-let`](../spec/05-expressions.md#r-expr.comp.no-let) | unchanged |
| Fixture `typing/invalid/bang-call-in-comprehension.hd` | moves to valid, or keeps an error in a non-suspending body |

**Other languages.** Python allows `await` in a comprehension inside an
async function ([PEP 530][pep-530]). Rust, Swift, Kotlin, and MoonBit
have no comprehensions; a loop there may `await` or suspend.

**Owner decision (batch 26, 2026-09-30): taken** ([Q2](#q2-bang-calls-in-comprehensions)).

### K5. One Dollar Rule For Every String

A `$` that begins no interpolation is an error in a plain string and text
in a prefixed string. Two rules say opposite things about the same
character.

**Before.**

```text
use std.text.r

fn prices() -> void:
    raw := r"costs $5"
    plain := "costs $5"  # error: syntax-error
    pass
```

**After.**

```text
fn prices() -> void:
    plain := "costs $5"  # hypothetical syntax
    flag := "$true"  # error: syntax-error
    pass
```

**Absorbed by.** `lex.prefix.plain-dollar-start`
and `lex.prefix.reserved-dollar`,
stated once for every string.

**Soundness.** The removed half rejected text with one reading. `\$` stays
valid, so no valid string changes value.

**Cost.** A plain string no longer flags a stray `$` as a possible typo.

| Item | Change |
| --- | --- |
| `lex.interp.stray-dollar` | reworded: keeps the reserved-word error only |
| `lex.prefix.plain-dollar-start` | merged into one rule for every string |
| Fixtures `stray-dollar.hd`, `stray-dollar-in-string.hd` | move to valid |

**Other languages.** Kotlin, whose interpolation hd follows, keeps such a
`$` as text ([Kotlin][kotlin-templates]).

**Owner decision (batch 26, 2026-09-30): taken** ([Q3](#q3-dollar-as-text)).

### K6. Readonly Iterators In Loops

A loop over a readonly `Iterator[T]` is an error. But the loop calls
`iter()`, and a readonly `iter()` already shares the traversal (CS10).

**Before.**

```text
fn drain(source: Iterator[i32]) -> List[i32]:
    [for value in source.iter() => value]

fn drain_direct(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]  # error: mutable-receiver-required
```

**After.**

```text
fn drain_direct(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]
```

**Absorbed by.** [`flow.for.iterable`](../spec/06-control-flow.md#r-flow.for.iterable)
and `flow.for.iterator-self`, retired by batch 24.

**Soundness.** Readonly views are shallow
([`types.readonly.not-deep`](../spec/04-type-system.md#r-types.readonly.not-deep)),
so the rule guarded nothing that `source.iter()` does not already allow.

| Item | Change |
| --- | --- |
| [`flow.for.iterator-mut`](../spec/06-control-flow.md#r-flow.for.iterator-mut) | deleted |
| Fixture `typing/invalid/readonly-iterator-in-comprehension.hd` | moves to valid |

**Other languages.** Rust's `for` accepts `&mut I` for any iterator `I`
([IntoIterator][rust-intoiter]).

**Owner decision (batch 24, 2026-09-30): not taken.** The rule stays, and
`Iterator[T]` stops implementing `Iterable[T]` instead.

### Cuts Considered And Not Proposed

| Idea | Why not |
| --- | --- |
| Merge `:=` into `let` | Both forms are used about 210 times each in real code, and each carries one permission. Swift and Kotlin also spend two keywords. |
| Allow `if` inside a same-line suite | Needs a new dangling-`else` rule, so it is a design option, not a cut. Python also bans it. |
| Drop the test-body result rule chosen by `?` | Needs an implicit `.Ok()`, a new rule. Decided in T15. |
| Replace comprehensions with adapters | Nested `for` clauses need a `flat_map` the prelude lacks, as [Special Cases](SPECIAL_CASES.md#cuts-considered-and-not-proposed) found. |
| Drop `*_test.hd` test modules for `tests:` blocks | One tree fixture uses them, and a long suite needs its own file. |

## Reopen Candidates

Each item below is an owner decision. The evidence is its rule cost beside
its real use; none of these sections recommends a reversal. Rank follows
rules that would leave the language, weighted by how little production
code uses the form.

| Rank | Form | Rules | Real uses | Uses in `lib/std` | Cost order of the form |
| ---: | --- | ---: | --- | ---: | --- |
| 1 | Embedding, promotion, `by` | 137 | 16 guide lines | 0 | syntax and semantic exceptions |
| 2 | Literal suffixes, string prefixes | 81 | about 16 | declarations only | syntax, name lookup, intrinsic markers |
| 3 | Packs | 72 | 2 | 0 | syntax and an intrinsic |
| 4 | Pipe | 37 | 4 | 0 | syntax |
| 5 | Row aliases | 28 | 1 | 0 | syntax and semantic exceptions |
| 6 | Leading-dot continuation | 12 | 0 | 0 | lexical syntax |
| 7 | Derivation member lines | 55 | 5 | 1 | syntax |

### R1. Embedding, Promotion, And Delegation

**Status.** Decided and applied: batch 32 kept embedding, value parts, and
both limits, and merged the rules
([Simplify Embedding](SIMPLIFY_EMBEDDING.md#owner-decisions)). Passes 32a
and 32b applied it on 2026-09-30 and 2026-10-01. The record counted 219
rules, including the restatements outside row 20's prefixes, and 57
remained. Batch 33 restored the private-shadow error as one rule, so 58
remain. The evidence below shows the language before batch 32.

**Evidence.** 115 rules and 427 spec lines for embedding, parts, `...`
copies, mutable edges, and promotion; 22 more for `by` delegation. Real
code has 12 embedded-field lines and 4 delegations, all in guide teaching
blocks. `lib/std` and the examples use none. Eight diagnostics serve it.

**Without it.** A named field and explicit access, as in Rust, Swift,
Kotlin, and Python:

```text
data Timestamps:
    pub created_at: i64

data Post:
    Timestamps
    id: string

fn show(post: Post) -> void $ Console:
    println(post.created_at)
```

```text
data Timestamps:
    pub created_at: i64

data Post:
    stamps: Timestamps
    id: string

fn show(post: Post) -> void $ Console:
    println(post.stamps.created_at)
```

**Precedent.** Go embeds structs and promotes their fields and methods
([Go][go-embed]). Kotlin delegates an interface with `by`
([Kotlin][kotlin-delegation]). Rust, Swift, and MoonBit have neither;
Rust's `Deref` is the closest, and its docs discourage it for this use
([Rust][rust-deref]).

**Decided by.** VE1-VE4, VE-S, the embedding limits, and the trait
delegation decision; see [Special Cases R54-R56](SPECIAL_CASES.md#rule-exceptions).

### R2. Literal Suffixes And String Prefixes

**Status.** Decided and applied: batch 31 kept the feature and merged its
rules (Q1 to Q4), and pass 31c applied them on 2026-09-30
([B1 As Applied In 31c](REOPEN_PACKS_LITERALS.md#b1-as-applied-in-31c)).
The record counted 89 rules, including cross-references this review left
out, and 38 remain. The evidence below shows the language before that
change.

**Evidence.** 81 rules and 461 spec lines across five chapters. Real code
writes about 11 suffixed literals (`5s`, `250ms`, and a playground `px`)
and 5 prefixed strings: four `r"..."` and one playground `total"..."`.
`lib/std` declares the four time suffixes and `r`, and uses none.

**Without it.** Suffixes become calls; prefixes become calls on a
template. A raw string then needs its own lexical form, because `"""`
processes escapes ([`lex.multiline.escapes`](../spec/01-lexical-structure.md#r-lex.multiline.escapes)).

```text
use std.time.ms

fn wait() -> void:
    timeout := 250ms
    pattern := r"\d+"
    pass
```

```text
use std.time.ms

fn wait() -> void:
    timeout := ms(250)
    pass
```

**Precedent.** C++ has user-defined literal suffixes
([cppreference][cpp-udl]). Scala string interpolators and JavaScript
tagged templates match `sql"..."` ([Scala][scala-interp],
[MDN][mdn-tagged]). Go writes `250 * time.Millisecond`, Rust
`Duration::from_millis(250)`, Kotlin `250.milliseconds`
([Go][go-duration], [Rust][rust-duration], [Kotlin][kotlin-duration]).
Rust and Python have a built-in raw string `r"..."`
([Rust][rust-raw], [Python][python-raw]).

**Decided by.** L1-L22. Keeping a raw-string form costs about 6 rules, so
the net saving without user prefixes would be about 75 rules.

### R3. Type And Value Packs

**Status.** Decided and applied: batch 31 (Q9) removed packs, and pass
31b took them out of the specification on 2026-09-30
([Reopen: Packs And Literal Sugar](REOPEN_PACKS_LITERALS.md)). The
examples below show the language before that change.

**Evidence.** 72 rules and 221 spec lines, most of chapter 12. Real code
has two pack signatures, `call_with` and `all!`, both in one guide
section; `pack.map` has no real use. `std.task.all!` is already a compiler
intrinsic (`pack.all.intrinsic` (since retired)).

**Without it.** The compiler types `all!` and `race!` as intrinsics, and
user code forwards a fixed arity:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

```text
fn call_with[A, R](f: fn(A) -> R, arg: A) -> R:
    f(arg)
```

**Precedent.** Swift added parameter packs in 5.9
([SE-0393][swift-packs]); Python types them with `TypeVarTuple`
([PEP 646][pep-646]). Rust joins futures with a macro, `tokio::join!`
([Tokio][tokio-join]); Kotlin awaits a homogeneous list
([Kotlin][kotlin-awaitall]). Go and MoonBit have no variadic generics.

**Design cost order.** An intrinsic signature for two combinators ranks
cheaper than a syntax change ([AGENTS.md](../AGENTS.md#design-cost-order)).
Whether tuple-kinded function inputs (FN_TYPE) could carry `call_with`
needs a brainstorm, not a cut.

### R4. The Pipe

**Evidence.** 37 rules, 179 spec lines, and six diagnostics. Real code has
4 pipes, all in the guide's one pipe example. `lib/std` and the examples
use none. The owner chose bare steps twice, PL8 then CS2.

**Without it.** Nested calls or named steps:

```text
fn label(raw: string) -> string:
    raw
        |> clean
        |> tag(_, 2)
        |> _.len() |> tag("size", _)
```

```text
fn label(raw: string) -> string:
    tagged := tag(clean(raw), 2)
    tag("size", tagged.len())
```

**Precedent.** MoonBit pipes into the first argument
([MoonBit][moonbit-pipe]); Hack uses a `$$` placeholder
([Hack][hack-pipe]). Go, Rust, Swift, Kotlin, and Python have no pipe;
Kotlin's `let` covers the same need ([Kotlin][kotlin-scope]).

### R5. Row Aliases

**Evidence.** 28 rules and 118 spec lines, including the one-key rules
that read a bare alias as a row or a type by name. Real code has one alias,
`type Stack = Database + Cache + Logger`.

**Without it.** The keys are written out, or grouped in a reusable
`$.Context`:

```text
type Stack = Database + Cache + Logger

fn serve() -> void $ Stack:
    pass
```

```text
fn serve() -> void $ Database + Cache + Logger:
    pass
```

**Precedent.** Koka aliases effect rows ([Koka][koka-alias]). None of the
six languages has effect rows.

### R6. Leading-Dot Continuation

**Evidence.** 12 rules and 46 spec lines, with open-suite and `.Variant`
exceptions. Real code has no leading-dot line. `lib/std` writes adapter
chains on one line.

**Without it.** One line, or parentheses, inside which every line already
continues ([`lex.dot.where`](../spec/01-lexical-structure.md#r-lex.dot.where)):

```text
fn short(line: string) -> string:
    line
        .slice(0, 8)
        .slice(2, 5)
```

```text
fn short(line: string) -> string:
    (line
        .slice(0, 8)
        .slice(2, 5))
```

**Precedent.** Swift and Kotlin continue a line that starts with `.`.
Python needs parentheses ([Python][python-join]), and Go needs the dot at
the end of the line ([Go][go-semicolons]).

### R7. Derivation Member Lines

**Evidence.** 55 rules and 268 spec lines for templates, blocks, and the
member lines `f = [...]`, `f += [...]`, and `f = pass`. Real code has six
`by Structure` lines and five member lines; one of each is in `lib/std`.

**Without it.** A derivation block keeps only whole-type facts, and a
member that needs other facts gets a hand-written implementation.

**Precedent.** Rust's serde puts per-field choices in attributes on the
field, such as `#[serde(skip)]` ([serde][serde-attrs]).

**Constraint.** The owner principle "one derivation block per concern"
stays; only the three line operators would be in question.

## Keep

| Form | Reason |
| --- | --- |
| `:=` and `let` | About 210 real uses each; `:=` is fixed, `let` is reassignable, `mut` is access. |
| `let mut` without an annotation | Saves repeating the type; K2 trims only the annotated half. |
| Same-line suites | 367 real uses; the no-comma and no-`if` limits follow Python's layout. |
| Trailing blocks | Every test case and `$.with` scope uses one; Kotlin and Swift have them. |
| Closures inside delimiters | Any multi-line closure argument needs these layout rules. |
| Comprehensions | 11 real uses beside 16 adapter calls; K4 trims only the limits. |
| `"""` and `$` interpolation | 49 interpolated strings in real code; Kotlin's model. |
| Bang calls and cold calls | Every suspension point is visible; 58 real calls. |
| `$` requirement rows | hd's effect model: 69 real rows and 33 provider scopes. |
| `::` references and qualified calls | 66 real uses, mostly `Type::f()` associated calls. |
| `reified` | 9 rules for runtime type metadata; Kotlin has the same word. |
| Varargs and suffix spread | 13 real uses, and `$.with(ctx...)` shares the form. |
| `?` propagation | Rust's operator; the alternative is a `match` per call. |
| `tests:` blocks | The only test home that sees private items; Rust and MoonBit keep tests in the file. |
| Decorators and facts | 32 real uses; metadata has no other home. |
| Newtypes | 21 rules; Rust, Go, and Kotlin each have one. |
| Paren-line rule | 8 rules that `(a, b) :=` statements need. |
| Loop `else` and `break value`, `defer` | Decided; `defer` is an owner principle. |

## Questions For The Owner

Smallest first. Each states the effect, the choices, a recommendation, and
hd code. Q1-Q6 are cuts; Q7 and Q8 carry Special Cases Q8 and Q9; Q9-Q11
ask whether to reopen, with no recommendation.

### Q1. Readonly Iterator Loops

A loop over a readonly iterator is an error, while `source.iter()` loops
over the same traversal ([K6](#k6-readonly-iterators-in-loops), Special
Cases C6).

- **A.** Delete the loop error.
- **B.** Keep it.

**Recommendation: A.** The rule guards nothing after CS10.

```text
fn drain(source: Iterator[i32]) -> List[i32]:
    [for value in source => value]
```

**Owner decision (batch 24, IT1-IT3, 2026-09-30): B, keep.** `Iterator[T]`
no longer implements `Iterable[T]`, which reverses CS10, so a readonly
iterator cannot be advanced by any path. See
[Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q2. Bang Calls In Comprehensions

A `for` loop in a suspending body may make bang calls; a comprehension may
not ([K4](#k4-a-comprehension-follows-its-loop), Special Cases C7).

- **A.** Allow them, and state `?` as a consequence of the loop shape.
- **B.** Allow them, and keep the `?` rules as they are.
- **C.** Keep the ban.

**Recommendation: A.** One rule, "a comprehension is its loop", replaces
three.

```text
fn fetch!(id: i32) -> string:
    "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]
```

**Owner decision (batch 26, 2026-09-30): A.** A bang call is valid in a
comprehension in a suspending body, and the calls run in order; the `?`
rules become a Note. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q3. Dollar As Text

`"costs $5"` is an error, while `r"costs $5"` keeps `$5` as text
([K5](#k5-one-dollar-rule-for-every-string), Special Cases C5).

- **A.** One rule for every string: such a `$` is text.
- **B.** Keep the error in plain strings.

**Recommendation: A.** It matches Kotlin.

```text
fn label() -> string:
    "costs $5"  # hypothetical syntax
```

**Owner decision (batch 26, 2026-09-30): A.** A `$` that starts no
interpolation is text in every string. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q4. Parentheses In `for`

`let (a, b)` and `(a, b) :=` use parentheses; `for a, b in m:` does not,
and cannot take a same-line body ([K1](#k1-one-name-list-shape-in-for),
Special Cases C4).

- **A.** `for (a, b) in m:`, with a fix-it for the bare form.
- **B.** Keep the bare form.

**Recommendation: A.** It completes the "one shape" decision of batches 7
and 13, and deletes a same-line exception.

```text
fn names(scores: Map[string, i32]) -> List[string]:
    [for (name, score) in scores => name]  # hypothetical syntax
```

**Owner decision (batch 26, 2026-09-30): A.** `for (k, v) in m`, in
loops and comprehension clauses; the bare form is `syntax-error` with a
fix-it. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q5. Nested Multi-Name Bindings

`((a, b) := p)` is valid inside an expression, with four rules for its
wrapping; no real code nests one ([K3](#k3-a-multi-name-binding-is-a-statement)).

- **A.** A multi-name `:=` is a statement only.
- **B.** Keep the nested form.

**Recommendation: A.** Python's `:=` also takes one name.

```text
fn pair() -> (i32, i32): (1, 2)

fn sum() -> i32:
    (low, high) := pair()
    low + high
```

**Owner decision (batch 26, 2026-09-30): A.** A multi-name `:=` is a
statement only; this supersedes Q1b's nested form. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q6. `let mut` With An Annotation

`let mut x: mut T` warns and `let mut x: T` is an error; seven rules cover
the pair ([K2](#k2-let-mut-takes-no-annotation)).

- **A.** `let mut` takes no annotation; an annotated binding writes `mut`
  in the type.
- **B.** Keep today's warning and error.

**Recommendation: A.** Seven rules become two, and the type keeps one
meaning for `mut`.

```text
fn build() -> List[string]:
    let names: mut List[string] = []
    names.append("Ada")
    names
```

**Owner decision (batch 26, 2026-09-30): B, keep.** The warning and the
error stay as they are. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q7. Map Key Bound

The map-key check is a compiler rule with its own code
(`types.map-key.bound`,
Special Cases C8 and Q8). It is 8 rules and 31 spec lines.

- **A.** `Map[K < Eq & Hash, V]`; `mut` keys become valid.
- **B.** As A, but a `mut` key stays an error.
- **C.** Keep the rule as it is.

**Recommendation: B.** The trait check becomes an ordinary bound, and the
ghost-entry guard stays.

```text
data UserId:
    value: string

fn setup() -> void:
    let users: Map[UserId, string] = {}  # error: unsatisfied-trait-bound
    pass
```

**Owner decision (batch 26, 2026-09-30): B.** `Map[K < Eq & Hash, V]`
reports `unsatisfied-trait-bound`; a `mut` key type stays an error. See
[Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q8. Map Read Type

`m[k]` reads `V?`, while `m[k] += v` and `Index::index` read `V` and panic
(Special Cases C9 and Q9). This is the same question as
[Call Indexing Q4](CALL_INDEXING.md#q4-map-read-type), and its answer
should follow the call-indexing choice.

- **A.** `m[k]` reads `V` and panics; `m.get(k)` reads `V?`.
- **B.** Keep the split decided on 2026-09-29.

**Recommendation: decide with Call Indexing.** A removes 4 of the 9 map
index rules but changes valid source.

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores.get(name):
        .Some(value) => value
        .None => 0
```

**Owner decision (batch 26, 2026-09-30): A.** `m[k]` reads `V` and
panics with `index-out-of-bounds`; `m.get(k)` reads `V?`. This reverses
the 2026-09-29 split. See [Open Issues](OPEN_ISSUES.md#language-design-decisions).

### Q9. Reopen Embedding?

Embedding, promotion, and `by` delegation cost 137 rules; real code uses
them in 16 guide lines and nowhere in `lib/std`
([R1](#r1-embedding-promotion-and-delegation)).

- **A.** Reopen: brainstorm named-field composition against today's design.
- **B.** Keep the decision.

**No recommendation.** This record shows the evidence only.

```text
data Post:
    stamps: Timestamps
    id: string
```

### Q10. Reopen Literal Sugar?

Suffixes and prefixes cost 81 rules; real code writes about 16 of them,
and `lib/std` none ([R2](#r2-literal-suffixes-and-string-prefixes)).

- **A.** Reopen: compare calls plus a built-in raw string.
- **B.** Keep the decision.

**No recommendation.**

```text
fn wait() -> void:
    timeout := ms(250)
    pass
```

### Q11. Reopen Packs?

Packs cost 72 rules; real code has two signatures, and `all!` is already
an intrinsic ([R3](#r3-type-and-value-packs)).

- **A.** Reopen: type `all!` and `race!` as intrinsics without user packs.
- **B.** Keep the decision.

**No recommendation.**

```text
fn call_with[A, R](f: fn(A) -> R, arg: A) -> R:
    f(arg)
```

## Sources

[rust-for]: https://doc.rust-lang.org/reference/expressions/loop-expr.html#iterator-loops
[swift-for]: https://docs.swift.org/swift-book/documentation/the-swift-programming-language/controlflow/#For-In-Loops
[kotlin-destructure]: https://kotlinlang.org/docs/destructuring-declarations.html
[rust-let]: https://doc.rust-lang.org/reference/statements.html#let-statements
[swift-let]: https://docs.swift.org/swift-book/documentation/the-swift-programming-language/thebasics/#Constants-and-Variables
[kotlin-var]: https://kotlinlang.org/docs/basic-syntax.html#variables
[pep-572]: https://peps.python.org/pep-0572/
[go-short-var]: https://go.dev/ref/spec#Short_variable_declarations
[pep-530]: https://peps.python.org/pep-0530/
[kotlin-templates]: https://kotlinlang.org/docs/strings.html#string-templates
[rust-intoiter]: https://doc.rust-lang.org/std/iter/trait.IntoIterator.html
[go-embed]: https://go.dev/ref/spec#Struct_types
[kotlin-delegation]: https://kotlinlang.org/docs/delegation.html
[rust-deref]: https://doc.rust-lang.org/std/ops/trait.Deref.html
[cpp-udl]: https://en.cppreference.com/w/cpp/language/user_literal
[scala-interp]: https://docs.scala-lang.org/overviews/core/string-interpolation.html
[mdn-tagged]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Template_literals#tagged_templates
[go-duration]: https://pkg.go.dev/time#Duration
[rust-duration]: https://doc.rust-lang.org/std/time/struct.Duration.html
[kotlin-duration]: https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/-duration/
[rust-raw]: https://doc.rust-lang.org/reference/tokens.html#raw-string-literals
[python-raw]: https://docs.python.org/3/reference/lexical_analysis.html#string-and-bytes-literals
[swift-packs]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0393-parameter-packs.md
[pep-646]: https://peps.python.org/pep-0646/
[tokio-join]: https://docs.rs/tokio/latest/tokio/macro.join.html
[kotlin-awaitall]: https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/await-all.html
[moonbit-pipe]: https://docs.moonbitlang.com/en/latest/language/fundamentals.html
[hack-pipe]: https://docs.hhvm.com/hack/expressions-and-operators/pipe
[kotlin-scope]: https://kotlinlang.org/docs/scope-functions.html
[koka-alias]: https://koka-lang.github.io/koka/doc/book.html
[python-join]: https://docs.python.org/3/reference/lexical_analysis.html#implicit-line-joining
[go-semicolons]: https://go.dev/ref/spec#Semicolons
[serde-attrs]: https://serde.rs/field-attrs.html

| Topic | Source |
| --- | --- |
| Rust `for` patterns | [Rust reference, iterator loops][rust-for] |
| Swift `for`-`in` | [Swift book, For-In Loops][swift-for] |
| Kotlin destructuring | [Kotlin docs, destructuring declarations][kotlin-destructure] |
| Rust `let` | [Rust reference, let statements][rust-let] |
| Swift `let` and `var` | [Swift book, constants and variables][swift-let] |
| Kotlin `val` and `var` | [Kotlin docs, variables][kotlin-var] |
| Python `:=` | [PEP 572][pep-572] |
| Go `:=` | [Go spec, short variable declarations][go-short-var] |
| Python async comprehensions | [PEP 530][pep-530] |
| Kotlin string templates | [Kotlin docs, string templates][kotlin-templates] |
| Rust `IntoIterator` | [Rust std][rust-intoiter] |
| Go embedding | [Go spec, struct types][go-embed] |
| Kotlin delegation | [Kotlin docs, delegation][kotlin-delegation] |
| Rust `Deref` | [Rust std][rust-deref] |
| C++ user literals | [cppreference][cpp-udl] |
| Scala interpolators | [Scala docs][scala-interp] |
| JavaScript tagged templates | [MDN][mdn-tagged] |
| Durations | [Go][go-duration], [Rust][rust-duration], [Kotlin][kotlin-duration] |
| Raw strings | [Rust][rust-raw], [Python][python-raw] |
| Swift parameter packs | [SE-0393][swift-packs] |
| Python `TypeVarTuple` | [PEP 646][pep-646] |
| `tokio::join!` | [Tokio docs][tokio-join] |
| Kotlin `awaitAll` | [kotlinx.coroutines][kotlin-awaitall] |
| MoonBit pipe | [MoonBit docs, fundamentals][moonbit-pipe] |
| Hack pipe | [HHVM docs][hack-pipe] |
| Kotlin scope functions | [Kotlin docs][kotlin-scope] |
| Koka effect rows | [Koka book][koka-alias] |
| Python line joining | [Python reference][python-join] |
| Go semicolons | [Go spec][go-semicolons] |
| serde field attributes | [serde docs][serde-attrs] |

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts) on
2026-09-30. Parsing checks syntax only. Codes such as `type-mismatch` in
comments are checker results the parser does not report.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | K1 Before | parses |
| 2 | K1 After | `syntax-error` at line 2, the first `# hypothetical syntax` line; the parser stops there, and lines 4 and 5 are hypothetical too |
| 3 | K2 Before | parses; the warning and error are checker results |
| 4 | K2 After | parses; line 3 parses today and becomes an error only after the cut |
| 5 | K3 Before | parses |
| 6 | K3 After | parses |
| 7 | K4 Before | parses |
| 8 | K4 After | parses |
| 9 | K5 Before | `syntax-error` at line 5, as marked |
| 10 | K5 After | `syntax-error` at line 2, `# hypothetical syntax`, and at line 3, as marked |
| 11 | K6 Before | parses |
| 12 | K6 After | parses |
| 13 | R1 today | parses |
| 14 | R1 without embedding | parses |
| 15 | R2 today | parses |
| 16 | R2 without literal sugar | parses |
| 17 | R3 today | parses |
| 18 | R3 without packs | parses |
| 19 | R4 today | parses |
| 20 | R4 without the pipe | parses |
| 21 | R5 today | parses |
| 22 | R5 without aliases | parses |
| 23 | R6 today | parses |
| 24 | R6 in parentheses | parses |
| 25 | Q1 | parses |
| 26 | Q2 | parses |
| 27 | Q3 | `syntax-error` at line 2, `# hypothetical syntax` |
| 28 | Q4 | `syntax-error` at line 2, `# hypothetical syntax` |
| 29 | Q5 | parses |
| 30 | Q6 | parses |
| 31 | Q7 | parses |
| 32 | Q8 | parses |
| 33 | Q9 | parses |
| 34 | Q10 | parses |
| 35 | Q11 | parses |
