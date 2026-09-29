# Type-Argument Defaults: Survey And Design

Status: design exploration, 2026-09-29. Nothing here is accepted
behavior. The owner decided to add general type-argument defaults; this
record designs how they work and asks the open points. The specification
stays authoritative until an apply pass. Under review:

- [`flow.collect.default`](../spec/06-control-flow.md#r-flow.collect.default),
  which these defaults replace, and the rest of
  [Collect Targets](../spec/06-control-flow.md#collect-targets);
- [Explicit Type Arguments](../spec/07-functions.md#explicit-type-arguments),
  the `_` placeholder rules `fn.generic.placeholder.*`, and
  [Generic Function Values](../spec/07-functions.md#generic-function-values);
- [Default Values](../spec/07-functions.md#default-values) for value
  parameters;
- [Generics](../spec/04-type-system.md#generics) and
  [Type Inference Boundaries](../spec/04-type-system.md#type-inference-boundaries);
- [Associated Type Bindings](../spec/09-traits.md#associated-type-bindings),
  [Method Generic Parameters](../spec/09-traits.md#method-generic-parameters),
  and [Dynamic Safety](../spec/09-traits.md#dynamic-safety);
- [Generic Parameters And Bounds](../spec/02-grammar.md#generic-parameters-and-bounds)
  in the grammar;
- [Collect](COLLECT.md), whose CO1 open point led to `flow.collect.default`.

Owner constraints, not reopened here: declarations are never inferred, and
the simplest proven model wins, Rust and Go first. Trivial details stay out
of the specification.

## Contents

1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Use Cases](#use-cases)
4. [Survey](#survey)
5. [Options Per Question](#options-per-question)
6. [Recommendation](#recommendation)
7. [Questions For The Owner](#questions-for-the-owner)
8. [Sources](#sources)
9. [Parse Log](#parse-log)

## Problem

[CO1](COLLECT.md#owner-decisions) made `collect` generic in its target `C`.
`xs := it.collect()` has no expected type, so the apply pass added a
one-off rule: when nothing determines `C`, it is `List[T]`. The owner chose
to replace that rule with a general feature, spelled at the declaration:

```text
impl[T] Iterator[T]:
    pub fn collect[C < FromIterator[T] = List[T]](mut self) -> C:  # hypothetical syntax
        C::from_iter(self)
```

This record designs the feature: where a default may appear, when it
applies, and what it may name. It also covers explicit lists, `_`, function
values, traits, API evolution, and diagnostics.

## What hd Has Today

| Area | Today | Source |
| --- | --- | --- |
| Collect fallback | `C` is `List[T]` when nothing determines it; a special case of one method | [`flow.collect.default`](../spec/06-control-flow.md#r-flow.collect.default) |
| Explicit lists | complete or absent; a partial prefix list is an error even when inference could finish it | [`fn.generic.explicit.complete`](../spec/07-functions.md#r-fn.generic.explicit.complete), [`fn.generic.explicit.no-partial`](../spec/07-functions.md#r-fn.generic.explicit.no-partial) |
| `_` | a whole call-site slot meaning "infer"; never inside a type such as `List[_]` | [`fn.generic.placeholder`](../spec/07-functions.md#r-fn.generic.placeholder), [`fn.generic.placeholder.not-type`](../spec/07-functions.md#r-fn.generic.placeholder.not-type) |
| Solving a call | from the call's arguments, its expected result type, and the function's constraints | [`fn.generic.placeholder.solve`](../spec/07-functions.md#r-fn.generic.placeholder.solve) |
| Unsolved parameter | an error | [`fn.generic.placeholder.ambiguous`](../spec/07-functions.md#r-fn.generic.placeholder.ambiguous), [`fn.type.generic.unsolved`](../spec/07-functions.md#r-fn.type.generic.unsolved) |
| Generic function values | fully instantiated from an expected type, a complete list, or the enclosing call | [`fn.type.generic.instantiate-sources`](../spec/07-functions.md#r-fn.type.generic.instantiate-sources) |
| Value defaults | trailing, may name earlier parameters only, checked at the declaration | [`fn.default.order-final-function`](../spec/07-functions.md#r-fn.default.order-final-function), [`fn.default.scope`](../spec/07-functions.md#r-fn.default.scope) |
| Trait method generics | an implementation repeats count, markers and bounds exactly | [`trait.impl.generics.bounds`](../spec/09-traits.md#r-trait.impl.generics.bounds) |
| Associated type bindings | `Name = Type` inside a bound trait's brackets: `I < Supplier[Item = T]` | [`grammar.generic.binding`](../spec/02-grammar.md#r-grammar.generic.binding), [`trait.binding.form`](../spec/09-traits.md#r-trait.binding.form) |
| Row parameters | only on functions, methods, implementations and row aliases; data, enums and traits have none | [`req.row.param.callables`](../spec/11-requirements-and-suspension.md#r-req.row.param.callables), [`req.row.param.no-data`](../spec/11-requirements-and-suspension.md#r-req.row.param.no-data) |
| Type packs | always inferred; no explicit pack arguments exist | [`pack.infer.no-explicit`](../spec/12-variadic-generics.md#r-pack.infer.no-explicit) |
| Literals | an integer literal with no expected type is `i32` | [`types.literal.int-default`](../spec/04-type-system.md#r-types.literal.int-default) |
| Operator traits | `Add[Rhs]` has no default, so every impl and bound writes `Rhs` | [`expr.op.trait.shape`](../spec/05-expressions.md#r-expr.op.trait.shape), [Supertrait Bindings](../spec/09-traits.md#supertrait-bindings) |

## Use Cases

Five cases, each checked against every question below.

| # | Case | Declaration kind |
| --- | --- | --- |
| U1 | `collect` with no expected type | method |
| U2 | `Add` whose right operand is usually `Self` | trait |
| U3 | A result alias with a usual error type | type alias |
| U4 | Adding a parameter to a published type without breaking users | data |
| U5 | A decoder whose format is usually JSON | function |

**U1: collect.** The default replaces `flow.collect.default`; the other
targets keep working through the expected type or an explicit list.

```text
impl[T] Iterator[T]:
    pub fn collect[C < FromIterator[T] = List[T]](mut self) -> C:  # hypothetical syntax
        C::from_iter(self)

fn report(names: List[string]) -> Map[string, i32]:
    copied := names.iter().collect()
    size := names.iter().collect().len() + copied.len()
    let by_name: Map[string, i32] = names.iter().map(fn(name): (name, size)).collect()
    by_name
```

**U2: operators.** Rust declares `Add<Rhs = Self>`, so `impl Add for Money`
and `T: Add<Output = T>` need no `Rhs`. In hd both lines below would lose
their repeated `Money` or `Self`.

```text
pub trait Add[Rhs = Self]:  # hypothetical syntax
    type Out
    fn add(self, rhs: Rhs) -> Self::Out

data Money:
    cents: i64

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { cents: self.cents + rhs.cents }

trait Summable < Add[Out = Self]

fn double[T < Add[Out = T]](value: T) -> T:
    value + value
```

**U3: a result alias.** As Rust crates write `type Result<T, E = Error>`.

```text
data AppError:
    message: string

type Outcome[T, E = AppError] = Result[T, E]  # hypothetical syntax

fn load(path: string) -> Outcome[string]:
    .Ok(path)

fn port(text: string) -> Outcome[i32, string]:
    .Err(text)
```

**U4: growing a type.** Rust added a hasher to `HashMap` and an allocator to
`Vec` this way. Every existing `Cache[string, i32]` keeps its meaning.

```text
trait Evict:
    fn pick(self, sizes: List[i32]) -> i32

data Lru: pass

impl Evict for Lru:
    fn pick(self, sizes: List[i32]) -> i32: 0

data Cache[K < Eq & Hash, V, P < Evict = Lru]:  # hypothetical syntax
    entries: Map[K, V]
    policy: P

fn size(cache: Cache[string, i32]) -> i32:
    cache.entries.len()
```

**U5: a function default.** The type argument `T` is always written or
expected; the format usually is not.

```text
trait Decode

trait Format

data Json: pass

data Yaml: pass

impl Format for Json

impl Format for Yaml

data User: pass

impl Decode for User

fn decode[T < Decode, F < Format = Json](text: string) -> T?:  # hypothetical syntax
    pass

fn load(text: string) -> void:
    first := decode[User](text)
    second := decode[User, Yaml](text)
    let third: User? = decode(text)
```

## Survey

Where each language allows defaults, and when a default applies.

| Language | Allowed on | When the default applies | Order | Bound checked | Sources |
| --- | --- | --- | --- | --- | --- |
| Rust | `struct`, `enum`, `type`, `trait` | only when a written type omits the slot; never as an inference fallback | trailing | at the declaration | [Reference](https://doc.rust-lang.org/reference/items/generics.html), [#36887](https://github.com/rust-lang/rust/issues/36887) |
| C++ | class, alias and function templates | after deduction, for each parameter left undeduced | trailing for class and alias templates; free for function templates | at use | [cppreference](https://en.cppreference.com/w/cpp/language/template_parameters), [\[temp.deduct.general\]/5](https://eel.is/c++draft/temp.deduct.general) |
| TypeScript | functions, classes, interfaces, type aliases | when inference has no candidate; an omitted explicit slot takes the default | trailing | at the declaration | [TS 2.3 notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-2-3.html) |
| Swift | none; a value default may fix a generic type (SE-0347) | the manifesto proposes "when type inference could not determine the type argument" | n/a | n/a | [Generics Manifesto](https://github.com/swiftlang/swift/blob/main/docs/GenericsManifesto.md), [SE-0347](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0347-type-inference-from-default-exprs.md) |
| C# | none; generic types and methods overload by arity instead | n/a | n/a | n/a | [csharplang #278](https://github.com/dotnet/csharplang/discussions/278), [Signatures and overloading](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/basic-concepts) |
| Go | none | n/a | n/a | n/a | [Type parameter declarations](https://go.dev/ref/spec#Type_parameter_declarations) |

Checked locally on 2026-09-29 with rustc 1.97.1, clang 23.1 (`-std=c++20`),
tsc 5.9.3 and Swift 6.4:

| Probe | Result |
| --- | --- |
| Rust `fn f<T = i32>()` | hard error: "defaults for generic parameters are not allowed here" |
| Rust `struct Lead<A = i32, B>` | error: "generic parameters with a default must be trailing" |
| Rust `struct Bad<T: Copy = String>` | error at the declaration: `String: Copy` is not satisfied |
| Rust `let q = Pair::default()` for `Pair<A, B = A>` | E0283 "type annotations needed": the default does not drive inference |
| Rust `&dyn Add<Output = i32>` | E0393: `Rhs` "must be explicitly specified", because its default names `Self` |
| C++ `f(3)` for `template<class T = long> T f(T)` | `T` is `int`: the argument wins over the default |
| C++ `auto r = &h;` for `template<class T = int> void h(T*)` | error; `&h<>` and a typed target both work |
| C++ `template<std::integral T = double>` | accepted until a call uses the default |
| TypeScript `make()` for `make<C = string[]>(): C` | `C` is `string[]`; with an expected `number[]`, `C` is `number[]` |
| TypeScript `p<string>("x")` for `p<A, B = number>(b: B)` | error: the omitted `B` is `number`, not inferred |
| TypeScript `f(3)` for `f<T = bigint>(x: T)` | `T` is `number`: the argument wins |
| Swift `struct Promise<Value, Reason = Error>` | syntax error |

### Why Rust Has No Function Defaults

- **Accepted, then parked.** [RFC 213](https://github.com/rust-lang/rfcs/blob/master/text/0213-defaulted-type-params.md)
  allowed defaults on functions and impls, used as inference *fallback*. It
  gave defaults "precedence over integer/float literal fallback".
- **The conflict.** Niko Matsakis's
  [2015 internals post](https://internals.rust-lang.org/t/interaction-of-user-defined-and-integral-fallbacks-with-inference/2496)
  asked whether `foo(22)` with `T = u64` gives `u64` or `i32`. The worry: a
  changed fallback lets "existing programs silently change behavior".
- **No agreement.** The tracking issue
  [#27336](https://github.com/rust-lang/rust/issues/27336) never stabilized.
  An attempt to remove the feature, [PR #127655](https://github.com/rust-lang/rust/pull/127655),
  hit crater regressions, and the issue is "slated for removal".
- **Function defaults became an error.** Rust 1.0 accepted them by accident;
  lint [#36887](https://github.com/rust-lang/rust/issues/36887) denied them,
  and rustc 1.97 rejects them outright.

Rust's fallback had to fit whole-function inference, where a type variable
can stay open across many statements. hd solves each call on its own, from
its arguments, its expected type and its constraints. Most of the Rust
conflict therefore has no hd counterpart; the literal case remains, and
question 2 settles it.

### Takeaways

1. **Fallback after inference is the proven function model.** C++ and
   TypeScript both use it, and Swift's manifesto proposes it.
2. **Written types use the default for an omitted slot.** Rust, C++ and
   TypeScript agree.
3. **Trailing order is the norm.** Only C++ function templates relax it.
4. **Arguments beat defaults.** In C++ and TypeScript a literal argument
   decides the parameter; the default is never consulted.

## Options Per Question

Every option below needs the decided syntax change (kind 1 in the
[Design Cost Order](../AGENTS.md#design-cost-order)). The Cost column lists
what each adds on top of it.

### S. Spelling

The motivating spelling puts the default after the bound:
`[C < FromIterator[T] = List[T]]`. hd already writes `Name = Type` inside a
bound trait's brackets as an associated type binding.

| # | Spelling | Cost | Notes |
| --- | --- | --- | --- |
| S1 | `C < FromIterator[T] = List[T]` | none extra | matches value defaults `port: i32 = 443`, Rust, C++, TypeScript |
| S2 | `C = List[T] < FromIterator[T]` | none extra | default before the bound; no precedent; hides the bound |
| S3 | `C < FromIterator[T] default List[T]` | a new keyword (kind 1) | no `=` at all; longer |

**Parser check.** The reference parser was rerun with one change to
`type_parameter` and `generic_parameter`: an optional `[ "=", type_argument ]`
after the bound. Every S1 block in this record parses with it, including a
binding and a default on one parameter:

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

data Constant: pass

fn pick[T, I < Supplier[Item = T] = Constant](source: I) -> T:  # hypothetical syntax
    source.get()
```

A binding sits inside the bound trait's brackets; a default follows the
closing bracket at the parameter list's own level. The parser never
confuses them, because a binding exists only inside `bound_type_arguments`.

**Human misreading.** A misplaced bracket turns a default into a binding:
`[C < FromIterator[T = List[T]]]` parses today as a binding named `T`. It
fails with `unknown-associated-type`, because `FromIterator` declares no
associated type `T`. It could compile silently only if the trait had an
associated type spelled like the intended argument; no such case was found.

**Recommendation:** S1.

### 1. Where Are Defaults Allowed?

| # | Option | Cost | Covers |
| --- | --- | --- | --- |
| 1A | Functions and methods only | none extra | U1, U5 |
| 1B | Data, enum, trait and type declarations only (Rust) | keeps `flow.collect.default` (kind 2) | U2, U3, U4 |
| 1C | Both: every declaration a use site instantiates | none extra | all |

Under 1C these take no default:

| Place | Why not |
| --- | --- |
| `impl[...]` header | no use site names its parameters; matching solves them (Rust rejects them too) |
| Variant-local parameters of a GADT variant | solved from the payload or existential in a match; no use case |
| Type packs `Ts...` | never written explicitly ([`pack.infer.no-explicit`](../spec/12-variadic-generics.md#r-pack.infer.no-explicit)) |

The grammar can express all three exclusions. `type_parameter` and the
non-pack function generic parameter gain `[ "=", type_argument ]`; the
`impl` and variant lists keep a list without it.

**Row parameters.** They exist only on callables, implementations and row
aliases. A callable's row parameter appears in a callback type, so the
callback argument solves it, and `$ R + K` already infers the least row. No
use case was found. With `type_argument` as the default's grammar,
`R = $()` needs no row-specific rule. A default of the wrong kind is the
existing `generic-kind-mismatch`.

**Recommendation:** 1C, with rows allowed by the same rule rather than
banned by a special case.

### 2. When Does A Default Apply?

| # | Option | Precedent | Cost |
| --- | --- | --- | --- |
| 2A | Only when a written list omits the slot | Rust | keeps `flow.collect.default` (kind 2) |
| 2B | After the use site's own inference, for each parameter it leaves unsolved | C++, TypeScript, Swift manifesto | none extra; removes `flow.collect.default` |
| 2C | As 2B, but a default also beats a literal's default type | RFC 213 | an ordering exception (kind 2) |
| 2D | Before inference, whenever no list is written | none | breaks `let m: Map[...] = it.collect()` |

Under 2B a use site solves its parameters as today. Then each unsolved
parameter that has a default takes it, in declaration order, with earlier
arguments substituted. Last, the bounds are checked. An unsolved parameter
without a default stays `unresolved-generic-placeholder`.

The "use site" is the one call or reference, as
[`fn.generic.placeholder.solve`](../spec/07-functions.md#r-fn.generic.placeholder.solve)
already bounds it. A later statement never changes the choice:

```text
fn later(names: List[string]) -> List[string]:
    copied := names.iter().collect()
    let again: List[string] = copied
    again
```

**Literals.** In `fn widen[T = i64](value: T) -> T`, the call `widen(3)`
has two candidates: `i32` from the literal, `i64` from the default. Under 2B
the literal is an argument, so it solves `T` as `i32`, as in C++ and
TypeScript. 2C picks `i64`, as RFC 213 did.

2B keeps one property that 2C loses: **adding a default never changes a
program that compiled without it**. It only fills parameters that used to
be errors. That property is exactly what Rust could not agree on.

**Recommendation:** 2B.

### 3. What May A Default Name, And Is It Checked?

| # | Option | Precedent | Cost |
| --- | --- | --- | --- |
| 3A | Earlier parameters, outer parameters and `Self` where in scope; checked against its bound at the declaration | Rust, TypeScript | none extra |
| 3B | Closed types only | none | U1, U2 impossible |
| 3C | As 3A, but checked at each use | C++ | errors land at callers |

Under 3A:

- `List[T]` in U1 names the impl's `T`; `Self` in U2 is the implementing
  type, as in Rust's `Rhs = Self`.
- A default that names a later parameter is an error, as for value
  defaults ([`fn.default.later-parameter`](../spec/07-functions.md#r-fn.default.later-parameter)).
- The default must satisfy its bound for every instantiation the earlier
  parameters allow. It is checked once, at the declaration, like a trait's
  default body ([`trait.default.checked-once`](../spec/09-traits.md#r-trait.default.checked-once)).
- Callers then never see a bound error that the author caused.

```text
fn count[C < FromIterator[i32] = i32](items: List[i32]) -> C:  # error: unsatisfied-trait-bound  # hypothetical syntax
    pass

fn swap[A = B, B = i32](value: B) -> A:  # error: binding-not-yet-visible  # hypothetical syntax
    pass
```

**Recommendation:** 3A.

### 4. Ordering, Omitted Slots, And `_`

Order:

| # | Option | Precedent | Cost |
| --- | --- | --- | --- |
| 4A | Defaulted parameters come last | Rust, TypeScript, C++ class templates, hd value defaults | none extra |
| 4B | Anywhere on functions | C++ function templates | a function-only exception (kind 2) |

Omitted trailing slots in a written list:

| # | Option | Precedent | Effect |
| --- | --- | --- | --- |
| 4C | An omitted slot takes its default, in types and calls alike | Rust types, TypeScript | `decode[User]` means `decode[User, Json]` |
| 4D | An omitted slot is inferred, then defaulted | C++ | reintroduces partial inference, which [`fn.generic.explicit.no-partial`](../spec/07-functions.md#r-fn.generic.explicit.no-partial) bans |

With 4A and 4C, three spellings give one consistent picture:

| Written | Meaning of a defaulted slot |
| --- | --- |
| no list, in an expression | inferred; the default if unsolved |
| `_` in the slot | inferred; the default if unsolved |
| slot omitted from a written list, or no list in a type | the default |

`_` still never appears in a type. A type written without any list, such as
`impl Add for Money`, is valid only when every parameter has a default.
Omitting a slot that has no default stays `partial-generic-arguments`.

**Empty lists.** `collect[]()` would mean `collect[List[T]]()`, which
`collect()` already gives when nothing else determines `C`. The grammar
requires at least one type argument, so no change is needed.

```text
fn load(text: string) -> void:
    first := decode[User](text)
    let second: User? = decode[_, Yaml](text)

fn bad[F < Format = Json, T](text: string) -> T?:  # error: default-order  # hypothetical syntax
    pass
```

**Recommendation:** 4A and 4C; no empty list.

### 5. Generic Function Values And Method References

| # | Option | Precedent | Cost |
| --- | --- | --- | --- |
| 5A | A default fills a parameter the value leaves unsolved, as at a call | TypeScript at calls | none extra: one rule for every use site |
| 5B | Only a written list applies defaults | C++ `&h<>` | a value-only exception (kind 2) |

Under 5A the value is still monomorphic
([`fn.type.generic.monomorphic`](../spec/07-functions.md#r-fn.type.generic.monomorphic)).
The default is chosen when the reference is made; a later call cannot
change it. A reference already drops value-parameter defaults
([`fn.ref.value`](../spec/07-functions.md#r-fn.ref.value)); a type default
is gone for the same reason, since the reference is already instantiated.

```text
fn drainer(names: List[string]) -> fn() -> List[string]:
    items := names.iter()
    items::collect
```

With no expected type, `drain := items::collect` would be
`fn() -> List[string]` under 5A and `unresolved-generic-placeholder` under
5B. C++ rejects `auto r = &h;` only because it has overload sets, which hd
lacks.

**Recommendation:** 5A.

### 6. Traits, Dynamic Safety, And API Compatibility

**Trait method overrides.**

| # | Option | Cost |
| --- | --- | --- |
| 6A | The default is part of the exact signature, so an implementation repeats it | none extra: [`trait.impl.generics.bounds`](../spec/09-traits.md#r-trait.impl.generics.bounds) already demands the same bounds |
| 6B | An implementation must omit it and inherits the trait's | an exception to the exact-signature rule (kind 2) |
| 6C | Either, but a written one must match | an exception (kind 2) and two spellings |

A call through the concrete type and one through a bound must pick the
same default. 6A guarantees that with no new rule, and the implementation
header reads like the trait's.

```text
trait Source[T]:
    fn drain[C < FromIterator[T] = List[T]](mut self) -> C  # hypothetical syntax

data Numbers:
    values: List[i32]

impl Source[i32] for Numbers:
    fn drain[C < FromIterator[i32] = List[i32]](mut self) -> C:  # hypothetical syntax
        self.values.iter().collect()
```

**Dynamic safety.** No change. The caller's compiler fills a default before
dispatch, so a method still has one body. A method-level parameter still
needs its `AnyRef` bound, and the bound check makes the default satisfy it.

One gap: a trait value type that omits a parameter whose default names
`Self`. No `Self` is known there, as Rust's E0393 says. Recommended error:
`partial-generic-arguments`, whose fix is to write the argument.

```text
trait Same[Other = Self]:  # hypothetical syntax
    fn same(self, other: Other) -> bool

fn check(value: Same) -> void:  # error: partial-generic-arguments
    pass
```

**API compatibility.** Under 2B, 4A and 4C:

| Change | Compatible? | Why |
| --- | --- | --- |
| Add a default to an existing parameter | yes | it fills only parameters that used to be errors |
| Add a trailing defaulted parameter to a type, trait or alias | yes | every written type still means the same (U4) |
| Add a trailing defaulted parameter to a function | yes | complete lists stay complete; calls solve or default it |
| Add a trailing defaulted parameter to a trait method | no | under 6A every implementation must repeat it |
| Change or remove a default | no | callers that relied on it change type or stop compiling |

**Recommendation:** 6A, the `partial-generic-arguments` reuse, and the
table above as the compatibility statement.

### 7. Diagnostics

Every new error fits an existing code:

| Error | Code | Existing use |
| --- | --- | --- |
| A parameter without a default after one with a default | `default-order` | value defaults |
| A default that fails its bound | `unsatisfied-trait-bound` | bounds everywhere |
| A default that names a later parameter | `binding-not-yet-visible` | value defaults |
| An omitted slot without a default, or a `Self` default in a trait value type | `partial-generic-arguments` | partial lists |
| An implementation default that differs from the trait's | `trait-method-signature` | exact signatures |
| A default of the wrong kind | `generic-kind-mismatch` | row and type kinds |
| A default on an `impl` header, a variant or a pack | `syntax-error` | the grammar has no slot there |

**Recommendation:** no new code.

## Recommendation

**Recommendation:** S1, 1C, 2B, 3A, 4A with 4C, 5A, 6A, and no new
diagnostic code. In short: a default is written with `=` after the bound
and checked once at the declaration. It fills a parameter that its use
site leaves unsolved or its written list omits.

- It is the C++ and TypeScript function model, and Rust's type model.
- It replaces `flow.collect.default`, a rule exception, with one general
  rule, and adds no diagnostic code.
- Declarations stay fully written: a default is spelled at the
  declaration and only use sites apply it.
- Adding a default never changes a program that compiled without it, which
  is the property Rust's fallback could not keep.

Ranking by the Design Cost Order, counting what each choice adds beyond the
decided syntax change:

| Rank | Choices | Costliest extra change | Extra changes |
| --- | --- | --- | --- |
| 1 | the recommendation | none | 0; one rule exception removed |
| 2 | 2C, 4B, 5B, 6B or 6C instead | rule exception (2) | 1 each |
| 3 | 1B or 2A instead | rule exception (2): `flow.collect.default` stays | 1 |
| 4 | S3 instead | a keyword (1) | 1 |

The apply pass would touch these areas:

| Area | Change |
| --- | --- |
| [Grammar](../spec/02-grammar.md#generic-parameters-and-bounds) | `[ "=", type_argument ]` on `type_parameter` and the non-pack function generic parameter |
| [Generics](../spec/04-type-system.md#generic-arguments) | where defaults apply; omitted slots; bare names |
| [Explicit Type Arguments](../spec/07-functions.md#explicit-type-arguments) | omitted trailing slots; `_` falls back to the default |
| [Generic Function Values](../spec/07-functions.md#generic-function-values) | the default as a last source |
| [Collect Targets](../spec/06-control-flow.md#collect-targets) | retire `flow.collect.default`; declare the default |
| [Method Generic Parameters](../spec/09-traits.md#method-generic-parameters) | the exact signature includes the default |
| [Operator traits](../spec/05-expressions.md#r-expr.op.trait.shape) | `Rhs = Self`, only if the owner asks for it separately |

## Questions For The Owner

Smallest first. Each has a recommendation.

### 1. Is the spelling `=` after the bound?

Effect: `[C < FromIterator[T] = List[T]]`. The parser never confuses it with
a binding such as `Supplier[Item = T]`.

- **A.** `=` after the bound, like value defaults.
- **B.** A `default` keyword.

**Recommendation:** A.

```text
fn pick[T, I < Supplier[Item = T] = Constant](source: I) -> T:  # hypothetical syntax
    source.get()
```

### 2. Does an implementation repeat a trait method's default?

Effect: decides whether the `impl` header matches the trait's exactly.

- **A.** Yes; the default is part of the exact signature.
- **B.** No; the implementation omits it.

**Recommendation:** A. The existing bound rule already works this way.

```text
impl Source[i32] for Numbers:
    fn drain[C < FromIterator[i32] = List[i32]](mut self) -> C:  # hypothetical syntax
        self.values.iter().collect()
```

### 3. Are existing diagnostic codes enough?

Effect: no new code; section 7 maps each error to one.

- **A.** Reuse the seven codes in section 7.
- **B.** Add a `type-default` code for all default errors.

**Recommendation:** A.

```text
fn bad[F < Format = Json, T](text: string) -> T?:  # error: default-order  # hypothetical syntax
    pass
```

### 4. What does an omitted slot in a written list mean?

Effect: `decode[User](text)` with `F < Format = Json`.

- **A.** The default, in calls and types alike.
- **B.** Inferred, then the default.

**Recommendation:** A. B reopens partial inference, which the explicit-list
rules ban.

```text
fn load(text: string) -> void:
    first := decode[User](text)
```

### 5. Does an argument beat a default?

Effect: `widen(3)` for `fn widen[T = i64](value: T) -> T`.

- **A.** Yes: `T` is `i32`, as in C++ and TypeScript.
- **B.** No: `T` is `i64`, as Rust's RFC 213 said.

**Recommendation:** A. Adding a default then never changes a program that
compiled before.

```text
fn widen[T = i64](value: T) -> T:  # hypothetical syntax
    value
```

### 6. Does a generic function value use defaults?

Effect: `items::collect` with no expected type.

- **A.** Yes, like a call: `fn() -> List[string]`.
- **B.** No: `unresolved-generic-placeholder`.

**Recommendation:** A.

```text
fn drainer(names: List[string]) -> fn() -> List[string]:
    items := names.iter()
    items::collect
```

### 7. Where may defaults appear?

Effect: which of U1 to U5 work.

- **A.** Every declaration a use site instantiates: functions, methods,
  data, enums, traits and type declarations.
- **B.** Functions and methods only.
- **C.** Types, traits and aliases only, keeping `flow.collect.default`.

**Recommendation:** A.

```text
type Outcome[T, E = AppError] = Result[T, E]  # hypothetical syntax
```

## Sources

- Rust: [Reference: generic parameters](https://doc.rust-lang.org/reference/items/generics.html), [RFC 213](https://github.com/rust-lang/rfcs/blob/master/text/0213-defaulted-type-params.md), [tracking issue #27336](https://github.com/rust-lang/rust/issues/27336), [PR #127655](https://github.com/rust-lang/rust/pull/127655), [lint issue #36887](https://github.com/rust-lang/rust/issues/36887), [internals: user-defined and integral fallbacks](https://internals.rust-lang.org/t/interaction-of-user-defined-and-integral-fallbacks-with-inference/2496), [E0393](https://doc.rust-lang.org/error_codes/E0393.html), [`std::ops::Add`](https://doc.rust-lang.org/std/ops/trait.Add.html), [`Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html), [`HashMap`](https://doc.rust-lang.org/std/collections/struct.HashMap.html)
- C++: [cppreference: template parameters](https://en.cppreference.com/w/cpp/language/template_parameters), [\[temp.deduct.general\]](https://eel.is/c++draft/temp.deduct.general)
- TypeScript: [TypeScript 2.3 release notes](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-2-3.html), [partial type argument inference, #26242](https://github.com/microsoft/TypeScript/issues/26242)
- Swift: [Generics Manifesto](https://github.com/swiftlang/swift/blob/main/docs/GenericsManifesto.md), [SE-0347](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0347-type-inference-from-default-exprs.md), [Default Generic Arguments discussion](https://forums.swift.org/t/default-generic-arguments/4960)
- C#: [csharplang discussion #278](https://github.com/dotnet/csharplang/discussions/278), [Basic concepts: signatures and overloading](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/basic-concepts)
- Go: [Type parameter declarations](https://go.dev/ref/spec#Type_parameter_declarations)

## Parse Log

Every `text` block was parsed twice on 2026-09-29. The first run used the
reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)). The
second used a scratch copy whose grammar adds `[ "=", type_argument ]` to
`type_parameter` and `generic_parameter`. Parsing checks syntax only; no
block is claimed to type-check. Lines with a default end in
`# hypothetical syntax`.

| Block | Section | Today | Extended grammar |
| --- | --- | --- | --- |
| 1 | Problem | `syntax-error` on line 2 (hypothetical) | parses |
| 2 | Use Cases, U1 | `syntax-error` on line 2 (hypothetical) | parses |
| 3 | Use Cases, U2 | `syntax-error` on line 1 (hypothetical) | parses |
| 4 | Use Cases, U3 | `syntax-error` on line 4 (hypothetical) | parses |
| 5 | Use Cases, U4 | `syntax-error` on line 9 (hypothetical) | parses |
| 6 | Use Cases, U5 | `syntax-error` on line 17 (hypothetical) | parses |
| 7 | S. Spelling | `syntax-error` on line 7 (hypothetical) | parses |
| 8 | 2. When Does A Default Apply? | parses | parses |
| 9 | 3. What May A Default Name | `syntax-error` on line 1 (hypothetical) | parses |
| 10 | 4. Ordering, Omitted Slots, And `_` | `syntax-error` on line 5 (hypothetical) | parses |
| 11 | 5. Generic Function Values | parses | parses |
| 12 | 6. Traits, overrides | `syntax-error` on line 2 (hypothetical) | parses |
| 13 | 6. Traits, dynamic safety | `syntax-error` on line 1 (hypothetical) | parses |
| 14 | Question 1 | `syntax-error` on line 1 (hypothetical) | parses |
| 15 | Question 2 | `syntax-error` on line 2 (hypothetical) | parses |
| 16 | Question 3 | `syntax-error` on line 1 (hypothetical) | parses |
| 17 | Question 4 | parses | parses |
| 18 | Question 5 | `syntax-error` on line 1 (hypothetical) | parses |
| 19 | Question 6 | parses | parses |
| 20 | Question 7 | `syntax-error` on line 1 (hypothetical) | parses |

The reference parser reports only the first error of a block, so blocks 9
and 12 show one of their two hypothetical lines. Block 7's form, a binding
and a default on one parameter, is the spelling check of section S.
