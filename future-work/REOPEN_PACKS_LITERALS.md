# Reopen: Packs And Literal Sugar

Status: design exploration, 2026-09-30. Nothing here is decided, accepted
behavior, or in the specification. It changes no spec text, fixture, or
prototype code.

> **Note.** The owner answered the questions as batch 31 in
> [Open Issues](OPEN_ISSUES.md#language-design-decisions). Pass 31a is
> applied: varargs as in VARARG-SPELL, the `Tuple` bound (Q5), tuple
> spread (Q8, close to A3), and `race!` (Q7). Pass 31b is applied: packs
> are gone (Q9), `all!` has one typing rule (ALL-INTRINSIC), and tuples
> derive their comparison traits (Q6). Pass 31c is applied: one rule set
> for literal functions (Q1, with C3), both markers kept (Q2), `-5s` as
> `-(5s)` (Q3), and a suffix parameter's default kept (Q4, reversing C2);
> see [B1 As Applied In 31c](#b1-as-applied-in-31c). Examples below that
> use packs show the language before 31b, and the B1 corners show it
> before 31c.

The owner reopened two features on 2026-09-30, after the
[Syntax And Semantics Cost Review](SYNTAX_SEMANTICS_COST.md) marked them
as possible reopens ([Q10](SYNTAX_SEMANTICS_COST.md#q10-reopen-literal-sugar),
[Q11](SYNTAX_SEMANTICS_COST.md#q11-reopen-packs)). Embedding stays as is.

- **A. Packs.** "i kind of hate the idea of pack." The aim is to remove
  type packs, value packs, `pack.map`, `pack.map_list`, and pack spreads.
  Heterogeneous `all!` and `race!`, and impls over any function arity,
  must still work.
- **B. Literal suffixes and prefixes.** Duration suffixes, `r"..."`,
  `sql"..."` with `Template[T]`, and user unit suffixes such as `5kg` all
  stay. The aim is to trim the mechanism.

Under review: [Variadic Generics](../spec/12-variadic-generics.md),
[Function Type Constructors](../spec/07-functions.md#function-type-constructors),
[Implementation Targets](../spec/09-traits.md#implementation-targets),
[Standard Combinators](../spec/11-requirements-and-suspension.md#standard-combinators),
[Literal Suffixes](../spec/01-lexical-structure.md#literal-suffixes) and
[Prefixed Strings](../spec/01-lexical-structure.md#prefixed-strings) in
chapter 01, [Literal Suffix Names](../spec/03-names-and-scopes.md#literal-suffix-names),
[Suffixed Literals](../spec/04-type-system.md#suffixed-literals),
[Literal Suffixes](../spec/05-expressions.md#literal-suffixes) and
[Prefixed Strings](../spec/05-expressions.md#prefixed-strings) in
chapter 05, and the stdlib pages [Time](../spec/std/time.md) and
[Text](../spec/std/text.md#raw-text-prefix).

## Contents

- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Options For A: Packs](#options-for-a-packs)
- [Options For B: Literal Sugar](#options-for-b-literal-sugar)
- [Before And After](#before-and-after)
- [Rule Accounting](#rule-accounting)
- [What Users Lose](#what-users-lose)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

Packs cost 75 language rules and four diagnostics. Real code has two
pack signatures, both in one guide section, and `lib/std` has none. The
two real needs are a typed `all!` and arity-generic adapters. `all!` is
already a compiler intrinsic
([`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic)),
and `Fn` already takes its inputs as one tuple
([`fn.type.ctor.inputs`](../spec/07-functions.md#r-fn.type.ctor.inputs)).

Literal sugar costs 89 language rules over seven chapters. Every use must
stay, so the question is how few rules can carry it. Suffixes and prefixes
have parallel rule sets that differ in a handful of places.

## What hd Has Today

Counts are numbered `r[...]` items in chapters 01-14 on 2026-09-30. The
cost review counted 72 and 81 with narrower prefix sets; this record
counts every rule a removal or trim would touch.

### Packs: 75 Rules

| Where | Rules | Count |
| --- | --- | ---: |
| [Chapter 12](../spec/12-variadic-generics.md) | `pack.kind`, `pack.param`, `pack.expand`, `pack.value`, `pack.ellipsis`, `pack.lockstep`, `pack.map` (24), `pack.tuple`, `pack.all` (8), `pack.infer`, `pack.runtime`, `pack.limit`, `pack.validate` | 63 |
| [Contextual Words](../spec/01-lexical-structure.md#contextual-words) | `lex.contextual.pack` (since retired), `.always`, `.ordinary` | 3 |
| [Grammar](../spec/02-grammar.md) | `grammar.pack.*` (since retired) (4), `grammar.fn.vararg.value-pack`, `grammar.primary.pack-map` | 6 |
| [Type System](../spec/04-type-system.md) | `types.pack.declare` (since retired), `types.pack.compile-time` | 2 |
| [Varargs](../spec/07-functions.md#varargs) | `fn.vararg.ellipsis` (since retired) | 1 |
| Rules that name packs beside other things | `grammar.generic.reified-and-packs`, `grammar.generic.default.positions`, `grammar.primary.function-type-argument`, `grammar.primary.suffix-spreads`, `lex.raw.not-reserved`, `types.generic.specialized`, `types.generic.interfaces`, `types.trait.safe.no-reified-or-pack`, `trait.dyn.safe.reified-or-pack`, `trait.impl.generics.markers`, `trait.target.function-type`, `fn.type.ctor.inputs`, `fn.generic.bang.examples`, `grammar.expr.method-type-arguments.bang` | 14, reworded only |

Diagnostics: `multiple-positional-value-packs`,
`nonfinal-positional-value-pack`, `pack-length-mismatch`, and
`pack-map-mapper-mismatch`. Fixtures: 14 rows of
[cases.tsv](../spec/conformance/cases.tsv) cite chapter 12 or a pack rule.

What already works without packs:

| Fact | Rule |
| --- | --- |
| A type parameter used as `Fn`'s inputs is tuple-kinded | `fn.type.ctor.input-kind` (since retired) |
| A non-tuple there is `generic-kind-mismatch` | `fn.type.ctor.kind-mismatch` (since retired) |
| A function type is an ordinary impl target | [`trait.target.function-type.valid`](../spec/09-traits.md#r-trait.target.function-type.valid) |
| A row parameter may stand in an impl head | [`trait.target.row-argument`](../spec/09-traits.md#r-trait.target.row-argument) |
| `all!` and `race!` are intrinsics with compiler-supplied frames | [`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic), `req.combinator.ordinary-call` (since retired) |
| Their concrete signatures are still library design | `req.combinator.library` (since retired) |
| Tuples are reference-shaped, so one generic body serves every tuple | [Implementation Model](../spec/04-type-system.md#implementation-model-non-normative) |

So `impl[Args, O, R] Describe for Fn[Args, O, R]` is valid today. What is
missing is a way to call `f` with an `Args` value.

Two promises in the spec already cover every tuple arity, with no stated
mechanism: tuple equality
([`expr.eq.std`](../spec/05-expressions.md#r-expr.eq.std)) and hashable
tuple map keys
([`types.map-key.builtin-types`](../spec/04-type-system.md#r-types.map-key.builtin-types)).
`lib/std` implements `Debug` only for pairs. Packs do not fill this gap
either: tuples of each arity are a separate constructor
([`trait.target.tuple.arity`](../spec/09-traits.md#r-trait.target.tuple.arity)).

### Literal Sugar: 89 Rules

| Chapter | Rules | Count |
| --- | --- | ---: |
| [01 Literal Suffixes](../spec/01-lexical-structure.md#literal-suffixes) | `lex.suffix.*` | 13 |
| [01 Raw Strings](../spec/01-lexical-structure.md#raw-strings), [Prefixed Strings](../spec/01-lexical-structure.md#prefixed-strings) | `lex.raw-string.prefix`, `lex.prefix.*` | 14 |
| [02 Grammar](../spec/02-grammar.md#suffixed-literals) | `grammar.primary.suffixed-literal`, `.prefixed-string`, `grammar.pattern.no-suffixed-literal`, `.no-prefixed-string` | 4 |
| [03 Names](../spec/03-names-and-scopes.md#literal-suffix-names) | `names.suffix.*`, `names.prefix.*` | 10 |
| [04 Types](../spec/04-type-system.md#suffixed-literals) | `types.literal.suffixed*`, `types.literal.prefixed*` | 9 |
| [05 Expressions](../spec/05-expressions.md#literal-suffixes) | `expr.literal.suffixed`, `.prefixed`, `expr.interp.prefixed`, `expr.suffix.*` (15), `expr.prefix.*` (15), `expr.op.suffix-negation` | 34 |
| [09 Traits](../spec/09-traits.md#numeric-traits) | `trait.num.suffix` | 1 |
| [10 Prelude](../spec/10-modules.md#standard-names-outside-the-prelude) | `module.prelude.ops-num-suffix`, `.no-suffix`, `.ops-str-prefix-markers`, `.no-prefix` | 4 |

By [Design Cost Order](../AGENTS.md#design-cost-order) kind: 31 syntax
rules (chapters 01 and 02), 20 intrinsic rules (the markers, function
shapes, and `std.ops` names the compiler knows), and 38 semantic rules
(lookup, typing, and call meaning). Diagnostics: `invalid-literal-suffix`
and `invalid-string-prefix`. About 54 fixtures cover the area. The stdlib
tier adds `std-time.suffix.*` and `std-text.prefix.*`, which no option
here changes.

## Use Cases

Every option is shown against the same six uses.

| # | Use | Today |
| --- | --- | --- |
| U1 | `let (user, orders) = all!(load_user(id), load_orders(id))` | pack signature, intrinsic body |
| U2 | `race!` over several children | intrinsic; no signature is written anywhere |
| U3 | A tool adapter over any function arity: decode the arguments, call, encode the result | `impl[Is..., O, R] Tool for SuspendFn[(Is...), O, R]`, and the body cannot call `f` without a value pack |
| U4 | `memoize` and `retry` over any arity, with `Eq + Hash` or `Debug` on the arguments | needs bounds on every pack element and a call |
| U5 | `500ms`, `5kg`, `r"\d+"`, `sql"... $id"` | 89 rules |
| U6 | Literal corners: `-5s`, a suffix with a default parameter, `5else` | 7 rules |

Note on U1: the children are cold calls, `load_user(id)`, with no `!`
(`pack.all.cold-call` (since retired)).
`all!(load_user!(id), ...)` would await each child in turn, then pass
plain values.

## Survey

| Language | Arity-generic functions | Heterogeneous join | Literal suffix | String prefix and raw text |
| --- | --- | --- | --- | --- |
| Rust | No variadics. `FnOnce<Args>` takes a tuple, but implementing or calling it with a tuple is unstable ([fn_traits][rust-fn-traits], [FnOnce][rust-fnonce]). Libraries implement per arity with macros, as axum's `Handler` does ([axum][axum-handler]). | `tokio::join!` macro ([Tokio][tokio-join]) | built-in numeric type suffixes only | built-in `r"..."` ([Rust][rust-raw]) |
| Swift | Parameter packs, `each T` and `repeat` ([SE-0393][swift-packs]) | `async let` ([Swift book][swift-async-let]) | none | `#"..."#` ([SE-0200][swift-raw]) |
| C++ | Variadic templates ([cppreference][cpp-pack]); `std::apply(f, tuple)` calls with a tuple ([cppreference][cpp-apply]) | futures, by hand | user-defined literals, `operator""` ([cppreference][cpp-udl]); `500ms` in `std::chrono_literals` ([cppreference][cpp-chrono-ms]) | built-in `R"(...)"` |
| Zig | No variadics. Tuples, `anytype`, `@call(modifier, f, args_tuple)`, and `std.meta.ArgsTuple(F)` ([Zig][zig-call], [ArgsTuple][zig-argstuple]) | async removed in 0.11 | none | `\\` multiline lines are raw ([Zig][zig-multiline]) |
| TypeScript | Tuple-typed rest: `(...args: A) => R` with `A extends unknown[]` ([TS 3.0][ts-rest-tuples]) | `Promise.all` typed with a mapped tuple type ([lib.es2015.promise][ts-promise-all]) | none | tagged templates; `String.raw` is a tag ([MDN][mdn-tagged], [MDN][mdn-raw]) |
| Python | `TypeVarTuple` ([PEP 646][pep-646]); `ParamSpec` for decorators ([PEP 612][pep-612]) | `asyncio.gather`, typed with a fixed set of overloads ([typeshed][typeshed-gather]) | none | built-in `r"..."`; `t"..."` builds a `Template` of strings and values ([PEP 750][pep-750]) |
| Scala 3 | Generic tuples with a `Tuple` upper bound ([Scala][scala-tuple]) | `Future.sequence` on homogeneous lists | none built in | string interpolators, including `raw"..."` ([Scala][scala-interp]) |
| Kotlin | none | `awaitAll` on a homogeneous list ([kotlinx][kotlin-awaitall]) | extension properties, `250.milliseconds` ([Kotlin][kotlin-duration]) | `"""` raw strings |
| Go | none | `errgroup`, homogeneous | `250 * time.Millisecond` ([Go][go-duration]) | backquoted raw strings |

Rust's standard library implements tuple traits only up to arity 12
([Rust][rust-tuple]).

Takeaways:

1. Without packs, the common shape is one tuple of arguments plus one
   call operation: C++ `std::apply`, Zig `@call`, TypeScript tuple rest,
   and Rust's `Fn<Args>`. hd's `Fn[(A, B), O, R]` is already this shape.
2. A heterogeneous join without packs is a macro (Rust), a language form
   (Swift `async let`), or a fixed set of overloads (Python). A
   compiler-typed intrinsic is hd's analog of the macro.
3. No mainstream language without variadics implements traits for every
   tuple arity in library code. They stop at a fixed arity.
4. User literal suffixes are rare; only C++ has them. A string prefix as
   a library function is common: Scala interpolators, JavaScript tags, and
   Python 3.14 templates.

## Options For A: Packs

All four options type `all!` the same way, so they differ only in how
arity-generic code calls a function. The tuple-impl question is the same
in all four; see [Q6](#q6-tuple-trait-impls).

### A1. Packs Gone: Tuple Inputs And One Call Intrinsic

Delete chapter 12's mechanism. Arity-generic code names the inputs tuple
as a type parameter, as it may today, and calls through one intrinsic.

| Added rule | Kind | Tier |
| --- | --- | --- |
| `all!(e1, ..., en)`, each `ei: mut Suspend[Ti]`, has type `(T1, ..., Tn)`. Any other argument is `type-mismatch`. | intrinsic | language |
| `all` is valid only as the callee of a direct call with positional arguments; a spread or a function-value use is `type-mismatch`. | intrinsic | language |
| `race!` is the ordinary vararg signature `fn race![T](tasks: mut Suspend[T]...) -> T`, with an intrinsic body. | none new: `req.combinator.intrinsic` | stdlib signature |
| `call_with(f, args)`, with `f: Fn[I, O, R]` or `SuspendFn[I, O, R]` and `args: I`, is the call of `f` with the elements of `args`. Written `call_with!`, it is that bang call. | intrinsic | language |
| A final `Rest[T]` element of `I` holds a `List[T]`, which `call_with` passes as a spread. | intrinsic | language |
| A tuple-kinded parameter used as a value type is a tuple of unknown arity: it has no `_0` members, and is stored, passed, bounded, or given to `call_with`. | semantic | language |
| Every tuple type implements `Eq`, `PartialOrd`, `Ord`, and `Hash` when its elements do, as an intrinsic derivation. | intrinsic | language |

**Suspension and rows.** `call_with(f, args)` follows `f`'s calling
convention: plain on a `SuspendFn`, it builds the cold `mut Suspend[O]`
([`req.suspend.type.plain-call`](../spec/11-requirements-and-suspension.md#r-req.suspend.type.plain-call)).
`call_with!(f, args)` is the bang call and needs a driver context. Its row
`R` joins the caller's row as any call's does.

**`Fn` rules in chapter 07.** `fn.type.ctor.input-kind` and
`fn.type.ctor.kind-mismatch` are unchanged; they are the mechanism. The
example `(Is...)` in `fn.type.ctor.inputs` becomes `Args`.
`fn.vararg.ellipsis` is deleted. The new value-type rule and the two
`call_with` rules join [Function Type Constructors](../spec/07-functions.md#function-type-constructors).

**Sugar.** `fn(Args) -> O` keeps meaning one parameter of type `Args`,
by [`fn.type.ctor.no-flatten`](../spec/07-functions.md#r-fn.type.ctor.no-flatten).
Arity-generic code therefore spells `Fn[Args, O, R]` or
`SuspendFn[Args, O, R]`.

**Name.** `apply` is taken: `std.ops.Apply` declares a method `apply`
([Callable Values](../spec/05-expressions.md#callable-values)). This record
uses `std.function.call_with`, the name chapter 12's example uses today.

**Representation.** Tuples are reference-shaped, so a generic body over
`Args` is shared. `call_with` then calls a function value of unknown
arity, so each function value needs a tupled entry, or the caller is
specialized. This is an implementation choice, not observable
([`types.generic.unobservable`](../spec/04-type-system.md#r-types.generic.unobservable)).
A shared body also keeps a `Tool[R]` trait value over function types
dynamically safe, which a pack method is not.

**Soundness.** The kind rule already rejects `Fn[i32, O, R]`. A
`call_with` whose `args` type is not `f`'s inputs is `type-mismatch`.
`all!` children keep the `mut Suspend[T]` requirement. No pack-length
error remains, because no two packs can disagree.

### A2. Packs Kept But Trimmed

Keep type packs, one value-pack parameter, and expansion in types,
parameters, and arguments. Delete `pack.map`, `pack.map_list`, the
contextual word `pack`, lockstep expansion, and the `all!` driver sketch.

| Kept | Removed |
| --- | --- |
| `Ts...` declarations, `(Ts...)`, `mut Suspend[Ts]...`, `f(args...)`, inference, runtime rules: about 40 rules | `pack.map*` (24), `lex.contextual.pack` (3), `grammar.primary.pack-map`, lockstep (3), five `pack.all` rules: about 36 rules, two diagnostics |

`call_with` stays user-writable as today, and `all!` keeps a written pack
signature. The owner's dislike of user-facing packs is not addressed, and
the tuple-impl gap stays.

### A3. Packs Gone: Spread A Tuple

As A1, but `f(args...)` spreads a tuple into fixed parameters instead of
calling `call_with`. The token exists; its meaning is new. It reverses
`fn.vararg.spread-fixed` (since retired)
and `fn.vararg.spread-needs-vararg` (since retired)
for tuple operands. That is a semantic rule exception, which ranks below an
intrinsic in the Design Cost Order. It also gives `...` two meanings again,
chosen by the operand's type.

### A4. Packs Gone, No Call Intrinsic (Radical)

As A1, without `call_with`. Arity-generic impls still type-check, so an
adapter can describe any function, but it can only call one-input
functions. Tools take one input record, memoize takes one key, and retry
takes a zero-argument closure, as Go and most Rust retry crates do. This
fails the stated need of U3 and U4 at full arity, and is shown as the
floor.

### Comparison For A

| | A1 tuple inputs + `call_with` | A2 trimmed packs | A3 tuple spread | A4 no call |
| --- | --- | --- | --- | --- |
| U1 `all!` | written intrinsic rule | pack signature | written intrinsic rule | written intrinsic rule |
| U2 `race!` | plain vararg signature | plain vararg signature | plain vararg signature | plain vararg signature |
| U3 tool adapter | yes | yes | yes | one-input functions only |
| U4 memoize, retry | yes; memoize returns a `Memo` value | yes | yes | closures and one key |
| User-facing pack syntax | none | `Ts...`, `x...` on packs | none | none |
| Language rules removed / added | 75 / 6 | about 36 / 0 | 75 / 5 and 2 reversed | 75 / 4 |
| Costliest change | intrinsic | syntax kept | semantic exception | intrinsic |
| Diagnostics removed | 4 | 2 | 4 | 4 |

### Future Option: A Type-Level Tuple Map

A type-level map over a tuple, such as `Each[Args, F]` or a TypeScript-style
mapped tuple type, would make `all!`, `zip`, `map_n`, and parser `seq`
writable: `fn all![Args < Tuple](sus...: Each[Args, Suspend]) -> Args`. The
owner rejected it for now (ALL-INTRINSIC, 2026-09-30). `F` is a
higher-kinded argument, which hd does not have, and inference would run
backwards from `Suspend[X_i]` to `X_i`. The body of `all!` would still be an
intrinsic. Revisit it if hd gains higher-kinded type parameters.

## Options For B: Literal Sugar

### B1. One Rule Set, Two Markers

Write one set of rules for a **literal function**: a function marked
`@num_suffix` or `@str_prefix`. Only the lexing, the parameter type, and
the template rules stay separate. The two marker names stay, so no
declaration changes. Three corners change: C1 and C2 change valid
programs, and C3 changes one error code.

| Cut | Before | After | Absorbed by |
| --- | --- | --- | --- |
| C1 negation | `-5s` is `s(-5)`; `-128b` fits an `i8` parameter | `-5s` is `-(5s)`, which needs `Neg` on the result | the ordinary unary `-` ([Operators](../spec/05-expressions.md#unary-and-binary-operators)); `std.time` adds `Neg for Duration` |
| C2 default parameter | `@num_suffix fn unit(count: i64 = 1)` is valid | the one parameter takes no default: `type-mismatch` at the definition | the shape rule |
| C3 reserved word | `5else` is `invalid-token`; `return"x"` is two tokens | both are two tokens: `5else` is `syntax-error` | one shared rule: a reserved word is never a suffix or prefix |

Pure merges and deletions, with no program changing meaning:

| Cut | Rules | Why it holds |
| --- | --- | --- |
| Shared lookup | `names.suffix.*` and `names.prefix.*`, 10 to 3 | identical text with "suffix" for "prefix" |
| Shared marker, shape, and call rules | 30 `expr.suffix.*`/`expr.prefix.*` to 14 | identical text; the shape rule names the parameter type per form |
| Ordinary call everywhere | `call-errors`, `ordinary-rules` (2), `exact-call` (2), `position-rules` to 1 | facts and shared enum data already apply call rules to every call |
| Lexing consequences | `lex.suffix.separator`, `.no-string`, `.no-quote`, `lex.prefix.no-hash`, `lex.raw-string.prefix` deleted | `lex.sep.placement`, the grammar, and the character-literal rules already reject them with the same codes |
| Typing consequences | `types.literal.suffixed.kind`, `.prefixed.value-errors`, `.prefixed.display` deleted | each restates argument checking |
| Restatements | `trait.num.suffix`, `module.prelude.*` 4 to 2, grammar 4 to 2 | cross-references and pairs |

Result: 89 rules become 38. Both diagnostic codes stay.

### B2. One Marker

As B1, with one marker `@literal`. The parameter type decides the form: a
numeric parameter makes a suffix, a `Template[T]` parameter a prefix. It
saves one name to learn and about one rule, but every existing
declaration changes, including the five in `lib/std`.

### B3. No Marker (Radical)

Any module-level function with one numeric parameter is a suffix, and any
with one `Template[T]` parameter is a prefix. It deletes the four marker
rules and the two `std.ops` names. But `5abs` becomes valid, and a shape
error moves from the definition to every literal. It reverses the reason
L1-L22 gave for a marker, with no new evidence.

### Comparison For B

| | B1 one rule set | B2 one marker | B3 no marker |
| --- | --- | --- | --- |
| Rules after (from 89) | 38 | about 37 | about 32 |
| Source changes | 3 corners | 3 corners and every marker | 3 corners and every marker |
| Accidental literals | no | no | yes, `5abs` |
| Error site for a bad shape | definition | definition | each literal |
| Costliest change | intrinsic, as today | intrinsic | semantic |

## Before And After

### A: Packs

Before, with packs, as chapter 12 writes it:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)

fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):
    panic("intrinsic")
```

After A1, U1: `all!` needs no written signature.

```text
use std.task.all

data User:
    name: string

fn load_user!(id: i64) -> User:
    User { name: "Ada" }

fn load_orders!(id: i64) -> List[i64]:
    [id]

fn page!(id: i64) -> string:
    let (user, orders) = all!(load_user(id), load_orders(id))
    "${user.name}: ${orders.len()}"
```

After A1, U2: `race!` is a plain vararg signature over one type.

```text
fn race![T](tasks: mut Suspend[T]...) -> T:
    panic("intrinsic")
```

After A1, U3: a tool adapter over any arity. `Decode` and `Encode` stand
for library traits.

```text
use std.function.{SuspendFn, call_with}

trait Tool[R]:
    fn run!(self, input: string) -> string $ R

impl[Args < Decode, O < Encode, R] Tool[R] for SuspendFn[Args, O, R]:
    fn run!(self, input: string) -> string $ R:
        args := Args::decode(input)
        call_with!(self, args).encode()
```

After A1, U4: memoize keeps a `Memo` value, and retry calls again.

```text
use std.function.{Fn, call_with}

data Memo[Args, O, R]:
    f: Fn[Args, O, R]
    cache: mut Map[Args, O]

impl[Args < Eq & Hash, O, R] Memo[Args, O, R]:
    pub fn call(mut self, args: Args) -> O $ R:
        match self.cache.get(args):
            .Some(hit) => hit
            .None => self.fill(args)

    fn fill(mut self, args: Args) -> O $ R:
        out := call_with(self.f, args)
        self.cache[args] = out
        out
```

```text
use std.function.{SuspendFn, call_with}

fn retry![Args, O, E, R](f: SuspendFn[Args, Result[O, E], R], args: Args, times: i32) -> Result[O, E] $ R:
    let mut result = call_with!(f, args)
    let mut left = times - 1
    while result.is_err() && left > 0:
        result = call_with!(f, args)
        left -= 1
    result
```

A3 instead writes the call as a tuple spread. It parses today, as a list
spread; the meaning is new.

```text
use std.function.SuspendFn

fn retry_once![Args, O, R](f: SuspendFn[Args, O, R], args: Args) -> O $ R:
    f!(args...)
```

### B: Literal Sugar

The four uses read the same before and after B1:

```text
use std.ops.{Template, num_suffix, str_prefix}
use std.text.r
use std.time.ms

data Mass:
    grams: f64

data Query:
    text: List[string]
    params: List[i64]

@num_suffix
fn kg(count: f64) -> Mass:
    Mass { grams: count * 1000.0 }

@str_prefix
fn sql(t: Template[i64]) -> Query:
    Query { text: t.raw_parts, params: t.values }

fn demo(id: i64) -> void:
    timeout := 500ms
    pattern := r"\d+"
    query := sql"select * from users where id = $id"
    load := 5.0kg
    pass
```

The three corners, before B1:

```text
use std.ops.num_suffix

@num_suffix
fn b(count: i8) -> i8: count

@num_suffix
fn unit(count: i64 = 1) -> i64: count

fn corners() -> void:
    low := -128b  # b(-128)
    five := 5unit  # unit(5)
    pass
```

After B1, checker results:

```text
use std.ops.num_suffix

@num_suffix
fn b(count: i8) -> i8: count

@num_suffix
fn unit(count: i64 = 1) -> i64: count  # error: type-mismatch

fn corners() -> void:
    low := -128b  # error: integer-literal-range, since it is -(b(128))
    pass
```

## Rule Accounting

### A1, By Design Cost Order Kind

| Kind | Removed | Added |
| --- | ---: | ---: |
| 1. Syntax: `generic_parameter` and `type_argument` `...`, `type_element` `...`, `pack_map_expression`, contextual `pack`/`map`/`map_list`, value-pack parameters, tuple expansion | 26 | 0 |
| 2. Semantic: pack kind, lockstep, inference, runtime, limits, validation, the `all!` motivation | 25 | 1 (tuple-kinded value type) |
| 3. Intrinsic: `pack.map`, `pack.map_list` | 24 | 5 (`all!` typing 2, `call_with` 2, tuple derivations 1) |
| 4. Core library | 0 | `race!` signature; `Debug` for tuples to a fixed arity, stdlib tier |
| **Total, language tier** | **75** | **6** |

Reworded, not removed: the 14 rules in the last row of
[Packs: 75 Rules](#packs-75-rules), each to drop the word pack or an
`(Is...)` example. The four pack diagnostics are deleted; nothing reports
in their place, because no program can reach them. Fixtures: the 14 rows
citing chapter 12 or a pack rule are deleted or rewritten. Chapter 12
keeps its heading so links survive, as a short Note that hd has no packs.

### A1 As Applied In 31b

Pass 31b removed 79 rule IDs and added 11, a net change of 68. Five of
the added IDs replace a rule that named packs beside other things, so 74
rules left outright and 6 are new. The record's estimate was 75 out and 6
in; `fn.vararg.ellipsis`, the 75th, was already retired by 31a.

| Chapter | Before | After | Retired | Added |
| --- | ---: | ---: | --- | --- |
| 01 Lexical Structure | 235 | 232 | `lex.contextual.pack`, `.pack.always`, `.pack.ordinary` | none |
| 02 Grammar | 262 | 256 | `grammar.pack.*` (4), `grammar.fn.vararg.value-pack`, `grammar.primary.pack-map`, `grammar.generic.reified-and-packs` | `grammar.generic.reified-positions` |
| 04 Type System | 466 | 464 | `types.pack.declare`, `.compile-time`, `types.trait.safe.no-reified-or-pack` | `types.trait.safe.no-reified` |
| 09 Traits | 547 | 549 | `trait.dyn.safe.reified-or-pack`, `trait.debug.std` | `trait.dyn.safe.reified`, `trait.debug.std-types`, `trait.target.tuple.derived`, `.derived.elementwise` |
| 11 Requirements | 301 | 305 | `req.combinator.ordinary-call` | `req.combinator.bang-called`, `.all-typing`, `.all-argument`, `.all-direct`, `.all-bang-child` |
| 12 Variadic Generics | 63 | 0 | every `pack.*` rule | none |
| **Language tier** | **3,907** | **3,839** | **79** | **11** |

The new rules differ from the A1 table in two ways. `call_with` and the
tuple-kinded value type are gone, since 31a's tuple spread and `Tuple`
bound took their place. `all!` gains a fourth rule for a bang-call child,
and the tuple derivation takes two rules, one for the impls and one for
their element-wise meaning. Other rules reworded in place:
`lex.raw.not-reserved`, `lex.contextual.elsewhere`,
`grammar.generic.default.positions`, `grammar.primary.function-type-argument`,
`grammar.primary.suffix-spreads`, `grammar.expr.method-type-arguments.bang`,
`types.generic.specialized`, `types.generic.interfaces`,
`fn.type.ctor.inputs`, `fn.type.no-ellipsis`, `fn.generic.bang.examples`,
`trait.impl.generics.markers`, and `trait.target.function-type`.

### B1, By Design Cost Order Kind

| Kind | Before | After | Change |
| --- | ---: | ---: | --- |
| 1. Syntax (chapters 01, 02) | 31 | 14 | 13 merged, 4 deleted as consequences |
| 2. Semantic (lookup, typing, call meaning) | 38 | 15 | lookup 10 to 3, typing 9 to 3, call 19 to 9 |
| 3. Intrinsic (markers, shapes, `std.ops` names) | 20 | 9 | shared marker and shape rules; `trait.num.suffix` deleted |
| **Total** | **89** | **38** | **51 fewer** |

B1, rule by rule:

| Today | After B1 |
| --- | --- |
| `lex.suffix.form` | kept |
| `lex.suffix.name`, `.longest-match` | merged into `lex.suffix.name` |
| `lex.suffix.decimal`, `.no-radix`, `.radix-letters` | merged into `lex.suffix.decimal` |
| `lex.suffix.exponent`, `.exponent.examples` | merged into `lex.suffix.exponent` |
| `lex.suffix.reserved`, `lex.prefix.reserved` | merged into one shared rule (C3) |
| `lex.suffix.meaning`, `lex.prefix.meaning` | merged into one shared rule |
| `lex.suffix.separator`, `.no-string`, `.no-quote` | deleted; same codes from `lex.sep.placement`, the grammar, and character literals |
| `lex.raw-string.prefix` | deleted; becomes a Note |
| `lex.prefix.form`, `.double-quote`, `.separate` | merged into `lex.prefix.form` |
| `lex.prefix.name`, `.raw-text`, `.interpolation` | kept |
| `lex.prefix.backslash`, `.odd-backslashes` | merged into `lex.prefix.backslash` |
| `lex.prefix.single-line`, `.multiline` | merged: the line rules of the unprefixed form |
| `lex.prefix.no-hash` | deleted |
| `grammar.primary.suffixed-literal`, `.prefixed-string` | merged into one |
| `grammar.pattern.no-suffixed-literal`, `.no-prefixed-string` | merged into one |
| `names.suffix.module-name`, `.ordinary`, and the `names.prefix` pair | merged into one |
| `names.suffix.no-local`, `names.prefix.no-local` | merged into one |
| `names.suffix.unknown-name`, `names.prefix.unknown-name` | merged into one |
| `names.suffix.function`, `names.prefix.function` | merged into the shared not-marked rule |
| `types.literal.suffixed`, `types.literal.prefixed` | merged: result type |
| `types.literal.suffixed.in`, `types.literal.prefixed.values` | kept |
| `types.literal.suffixed.kind`, `.prefixed.value-errors`, `.prefixed.display` | deleted; argument checking covers them |
| `types.literal.suffixed.negation`, `.negation.check`, `expr.op.suffix-negation` | deleted (C1) |
| `expr.literal.suffixed`, `expr.literal.prefixed` | merged into one |
| `expr.interp.prefixed` | kept |
| `expr.suffix.marker`, `expr.prefix.marker` | merged |
| `expr.suffix.marker.fn-only`, `expr.prefix.marker.fn-only` | merged |
| `expr.suffix.not-marked`, `expr.prefix.not-marked` | merged; both codes kept |
| `expr.suffix.no-marker-import`, `expr.prefix.no-marker-import` | merged |
| `expr.suffix.fn-shape-num`, `expr.prefix.fn-shape-one` | merged: one parameter, no default, never suspends |
| `expr.suffix.fn-shape.default` | deleted (C2) |
| `expr.suffix.fn-shape.definition`, `expr.prefix.fn-shape.definition` | merged |
| `expr.suffix.call-errors`, `.ordinary-rules`, `.exact-call`, `.position-rules`, `expr.prefix.ordinary-rules`, `.exact-call` | merged: an ordinary call in every respect and position |
| `expr.suffix.fn-call`, `.fn-call.example` | merged |
| `expr.suffix.fn-shape-param`, `.generic-num` | merged |
| `expr.prefix.fn-call`, `.no-join` | merged |
| `expr.prefix.template`, `.raw-parts`, `.order` | kept |
| `expr.prefix.parts`, `.parts.example` | merged |
| `trait.num.suffix` | deleted; restates `expr.suffix.fn-shape-param` |
| `module.prelude.ops-num-suffix`, `.ops-str-prefix-markers` | merged |
| `module.prelude.no-suffix`, `.no-prefix` | merged |

Fixtures that change under B1: `literal-suffix-negative-minimum` and
`literal-suffix-negative-range` (C1), `literal-suffix-default-parameter`
(C2), and `reserved-word-suffix` (C3, code `syntax-error`). The rest only
change their specification column.

### B1 As Applied In 31c

Pass 31c applied B1 with the owner's two changes. Q4 keeps a suffix
parameter's default, so cut C2 does not apply. `expr.suffix.fn-shape.default`
is still deleted, as a restatement of the ordinary parameter rule, so the
count matches the record: 89 rules became 38, with 70 retired and 19 added.

| Chapter | Before | After | Retired | Added |
| --- | ---: | ---: | --- | --- |
| 01 Lexical Structure | 27 | 12 | 18: `lex.suffix.*` (9), `lex.prefix.*` (8), `lex.raw-string.prefix` | `lex.literal-fn.reserved`, `.meaning`, `lex.prefix.lines` |
| 02 Grammar | 4 | 2 | 4 | `grammar.primary.literal-call`, `grammar.pattern.no-literal-call` |
| 03 Names | 10 | 3 | 10 | `names.literal-fn.module-name`, `.no-local`, `.unknown-name` |
| 04 Types | 9 | 3 | 7 | `types.literal.call-result` |
| 05 Expressions | 34 | 16 | 26 | `expr.literal.call`, `expr.literal-fn.*` (7) |
| 09 Traits | 1 | 0 | `trait.num.suffix` | none |
| 10 Prelude | 4 | 2 | 4 | `module.prelude.ops-literal-markers`, `.no-literal-fn` |
| **Literal rules** | **89** | **38** | **70** | **19** |

The language tier went from 3,810 to 3,759 rule IDs. The stdlib tier adds
one rule, [`std-time.suffix.std.duration-neg`](../spec/std/time.md#r-std-time.suffix.std.duration-neg).
The record's table holds with three differences of naming: the merged
rules take new `literal-fn` IDs, the line rules become `lex.prefix.lines`,
and the deleted consequences survive as Notes. Fixtures: one deleted
(`literal-suffix-negative-minimum`), four rewritten, three new, and
`reserved-word-suffix` now expects `syntax-error`. One consequence of C3
is new: `if flag: 5else: 3` is now a valid `if` expression.

## What Users Lose

| Option | Loss |
| --- | --- |
| A1 | `call_with(f, a, b)` with separate arguments: callers build the tuple, `call_with(f, (a, b))`. |
| A1 | `pack.map` over a tuple: per-element code is written out. |
| A1 | A wrapper that returns the same arity, `memoize(f) -> fn(A, B) -> O`: it returns a `Memo` value with `call(args)`. Closing this gap needs a second intrinsic. |
| A1 | `fn(Args...) -> O` sugar in arity-generic code: it is `Fn[Args, O, R]`. |
| A1 | Library traits on tuples, such as `Decode`, stop at the arity a library writes, as in Rust. |
| A1 | A mixed-type `race!`: each child is wrapped in a local `fn!` returning one enum. |
| B1 | `-5s` as `s(-5)`, and so `-128b` for an `i8` suffix. |
| B1 | A suffix function whose parameter has a default. |
| B1 | Nothing else: every other cut restates a rule that already holds. |

## Recommendation

**Recommendation for A: A1, packs gone.**

- It removes 75 language rules and four diagnostics, and adds 6.
- Its costliest change is an intrinsic, which ranks above A3's semantic
  exception and A2's kept syntax.
- It builds on rules the owner already applied: tuple-kinded `Fn` inputs
  and intrinsic combinators.
- It matches the mainstream non-variadic answer: C++ `std::apply`, Zig
  `@call`, TypeScript tuple rest.
- It gives up user-written variadic functions and same-arity wrappers.
  The next best is A2 if the owner wants `call_with(f, a, b)` spelling.

**Recommendation for B: B1, one rule set with both markers, 89 rules to 38.**

- 47 of the 51 removed rules are merges or restatements, and no valid
  program changes meaning through them.
- The two markers stay, so no declaration changes, and a reader still sees
  which form a function serves.
- C1 and C2 drop corners with no use in real code; C3 makes suffixes and
  prefixes agree.
- The next best is B2, which saves about one more rule at the cost of
  editing every declaration.

## Questions For The Owner

Fewest and smallest first. Each has a recommendation.

### Q1. Merge The Literal Rule Sets

**Decided: A, applied in 31c.** See [B1 As Applied In 31c](#b1-as-applied-in-31c).

Suffix and prefix rules are written twice. Merging them removes 47 rules.
No valid program changes; `5else` reports `syntax-error` instead of
`invalid-token` (C3).

- **A.** Merge, as in B1's pure merges. *Recommended.*
- **B.** Keep the two rule sets.

```text
@num_suffix
fn kg(count: f64) -> Mass:
    Mass { grams: count * 1000.0 }
```

### Q2. One Marker Or Two

**Decided: A, applied in 31c.**

With merged rules, the markers could also become one name.

- **A.** Keep `@num_suffix` and `@str_prefix`. *Recommended:* no source
  changes, and the declaration says which form it serves.
- **B.** One marker, `@literal`; the parameter type picks the form.

```text
@str_prefix
fn sql(t: Template[i64]) -> Query:
    Query { text: t.raw_parts, params: t.values }
```

### Q3. Negated Suffixed Literals

**Decided: A, applied in 31c.**

`-5s` means `s(-5)` through three special rules. Without them it means
`-(5s)`, and `-128b` no longer fits an `i8` suffix.

- **A.** Ordinary negation; `std.time` implements `Neg` for `Duration`.
  *Recommended.*
- **B.** Keep the special case.

```text
fn back() -> Duration:
    -5s
```

### Q4. Default On A Suffix Parameter

**Decided: B, applied in 31c.** The default stays; `expr.suffix.fn-shape.default` is deleted as a restatement.

A suffix function's one parameter may have a default today, and no real
code uses it.

- **A.** No default; the shape rule rejects it. *Recommended.*
- **B.** Keep it.

```text
@num_suffix
fn unit(count: i64 = 1) -> i64: count
```

### Q5. How `Args` Becomes Tuple-Kinded

An arity-generic impl needs `Args` to be a tuple. Today its use as `Fn`'s
inputs makes it one.

- **A.** Keep the rule by use, `fn.type.ctor.input-kind`. *Recommended:*
  no new rule, and a misuse is already `generic-kind-mismatch`.
- **B.** Require a written bound, `Args < Tuple`, with a new marker trait.

```text
impl[Args < Debug, O, R] Describe for Fn[Args, O, R]:
    fn describe(self) -> string:
        "function"
```

### Q6. Tuple Trait Impls

Memoize needs `Args < Eq & Hash`; logging needs `Args < Debug`. The spec
promises tuple equality and hashing at every arity but names no mechanism.

- **A.** The compiler derives `Eq`, `PartialOrd`, `Ord`, and `Hash` for
  every tuple whose elements have them; `lib/std` writes `Debug` and other
  traits up to arity 12. *Recommended.*
- **B.** `lib/std` writes every tuple impl up to arity 12, as Rust does.
- **C.** Let typed derivation templates target tuples, so libraries derive
  for every arity. This is a new mechanism and waits for a brainstorm.

```text
impl[A < Debug, B < Debug, C < Debug] Debug for (A, B, C):
    fn debug(self) -> string:
        "(${self._0.debug()}, ${self._1.debug()}, ${self._2.debug()})"
```

### Q7. `race!` Over Mixed Types

`all!` returns a tuple, but `race!` returns one value, so mixed children
need one result type.

- **A.** `race!` is `fn race![T](tasks: mut Suspend[T]...) -> T`, a plain
  signature; mixed children are wrapped in an enum. *Recommended.*
- **B.** A written intrinsic rule joins the child types to an expected
  type, as list literals do.

```text
fn race![T](tasks: mut Suspend[T]...) -> T:
    panic("intrinsic")
```

### Q8. Calling With An Inputs Tuple

Arity-generic code holds `f: Fn[Args, O, R]` and `args: Args`, and must
call one with the other.

- **A.** An intrinsic, `std.function.call_with(f, args)`, and
  `call_with!` for suspending functions. *Recommended.*
- **B.** A tuple spread, `f(args...)`, which reverses two vararg rules.

```text
use std.function.{Fn, call_with}

fn run[Args, O, R](f: Fn[Args, O, R], args: Args) -> O $ R:
    call_with(f, args)
```

### Q9. Remove Packs

- **A.** A1: packs gone, `all!` typed by a written rule, `call_with`, 75
  rules out and 6 in. *Recommended.*
- **B.** A2: keep type packs and one value pack, drop `pack.map` and
  lockstep, about 36 rules out.
- **C.** Keep packs.

```text
let (user, orders) = all!(load_user(id), load_orders(id))
```

## Sources

[rust-fn-traits]: https://doc.rust-lang.org/unstable-book/library-features/fn-traits.html
[rust-fnonce]: https://doc.rust-lang.org/std/ops/trait.FnOnce.html
[axum-handler]: https://docs.rs/axum/latest/axum/handler/trait.Handler.html
[tokio-join]: https://docs.rs/tokio/latest/tokio/macro.join.html
[rust-raw]: https://doc.rust-lang.org/reference/tokens.html#raw-string-literals
[rust-tuple]: https://doc.rust-lang.org/std/primitive.tuple.html
[swift-packs]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0393-parameter-packs.md
[swift-async-let]: https://docs.swift.org/swift-book/documentation/the-swift-programming-language/concurrency/#Calling-Asynchronous-Functions-in-Parallel
[swift-raw]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0200-raw-string-escaping.md
[cpp-pack]: https://en.cppreference.com/w/cpp/language/parameter_pack
[cpp-apply]: https://en.cppreference.com/w/cpp/utility/apply
[cpp-udl]: https://en.cppreference.com/w/cpp/language/user_literal
[cpp-chrono-ms]: https://en.cppreference.com/w/cpp/chrono/operator%22%22ms
[zig-call]: https://ziglang.org/documentation/master/#call
[zig-argstuple]: https://ziglang.org/documentation/master/std/#std.meta.ArgsTuple
[zig-multiline]: https://ziglang.org/documentation/master/#Multiline-String-Literals
[ts-rest-tuples]: https://www.typescriptlang.org/docs/handbook/release-notes/typescript-3-0.html#rest-parameters-with-tuple-types
[ts-promise-all]: https://github.com/microsoft/TypeScript/blob/main/src/lib/es2015.promise.d.ts
[mdn-tagged]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Template_literals#tagged_templates
[mdn-raw]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String/raw
[pep-646]: https://peps.python.org/pep-0646/
[pep-612]: https://peps.python.org/pep-0612/
[pep-750]: https://peps.python.org/pep-0750/
[typeshed-gather]: https://github.com/python/typeshed/blob/main/stdlib/asyncio/tasks.pyi
[scala-tuple]: https://www.scala-lang.org/api/3.x/scala/Tuple.html
[scala-interp]: https://docs.scala-lang.org/overviews/core/string-interpolation.html
[kotlin-awaitall]: https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-core/kotlinx.coroutines/await-all.html
[kotlin-duration]: https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.time/-duration/
[go-duration]: https://pkg.go.dev/time#Duration

| Topic | Source |
| --- | --- |
| Rust `Fn` traits over tuple arguments | [FnOnce][rust-fnonce], [fn_traits][rust-fn-traits] |
| Rust per-arity impls by macro | [axum `Handler`][axum-handler] |
| Rust tuple trait arity limit | [Rust std, tuple][rust-tuple] |
| Rust join and raw strings | [Tokio][tokio-join], [Rust reference][rust-raw] |
| Swift packs, `async let`, raw strings | [SE-0393][swift-packs], [Swift book][swift-async-let], [SE-0200][swift-raw] |
| C++ packs, `std::apply`, literals | [packs][cpp-pack], [apply][cpp-apply], [user literals][cpp-udl], [chrono `ms`][cpp-chrono-ms] |
| Zig tuple calls and raw lines | [`@call`][zig-call], [ArgsTuple][zig-argstuple], [multiline strings][zig-multiline] |
| TypeScript tuple rest and `Promise.all` | [TS 3.0][ts-rest-tuples], [lib.es2015.promise][ts-promise-all] |
| JavaScript tags | [tagged templates][mdn-tagged], [`String.raw`][mdn-raw] |
| Python variadics, decorators, templates, gather | [PEP 646][pep-646], [PEP 612][pep-612], [PEP 750][pep-750], [typeshed][typeshed-gather] |
| Scala tuples and interpolators | [Tuple][scala-tuple], [interpolation][scala-interp] |
| Kotlin and Go | [awaitAll][kotlin-awaitall], [Duration][kotlin-duration], [Go][go-duration] |

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts) on
2026-09-30. Parsing checks syntax only; no block is claimed to
type-check. Codes in comments are checker results the parser does not
report. `call_with`, `Decode`, `Encode`, and the `race!` signature are
proposals; they parse because they use existing syntax.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | A before: packs | parses |
| 2 | A1, U1 `all!` | parses |
| 3 | A1, U2 `race!` | parses |
| 4 | A1, U3 tool adapter | parses |
| 5 | A1, U4 memoize | parses |
| 6 | A1, U4 retry | parses |
| 7 | A3 tuple spread | parses |
| 8 | B four uses | parses |
| 9 | B corners before | parses |
| 10 | B corners after | parses; the errors are checker results |
| 11 | Q1 | parses |
| 12 | Q2 | parses |
| 13 | Q3 | parses |
| 14 | Q4 | parses |
| 15 | Q5 | parses |
| 16 | Q6 | parses |
| 17 | Q7 | parses |
| 18 | Q8 | parses |
| 19 | Q9 | parses |
