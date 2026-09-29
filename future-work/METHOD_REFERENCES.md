# Method And Field References: Survey And Design Options

Status: design exploration, 2026-09-29. Owner decisions MR1-MR4, MR6,
and MR7 are applied (2026-09-29), and the specification is authoritative for them:
[Method References](../spec/07-functions.md#method-references),
[Member References](../spec/02-grammar.md#member-references), and
[Bare Steps](../spec/05-expressions.md#bare-steps). The rest of the record
is the survey behind the decisions.

The owner asked for method and field references to be designed now
([Chaining Study CS5](CHAINING_STUDY.md#owner-decisions)), with a spelling
that does not look like enum shorthand such as `.None` (CS3). This record
follows the [brainstorm](../.agents/skills/brainstorm/SKILL.md) method. It
reviews:

- [Method Values](../spec/07-functions.md#method-values), which reserves
  `Type::name` and `x::name` and rejects both as `deferred-method-value`;
- [Methods And Receivers](../spec/07-functions.md#methods-and-receivers)
  and [Generic Methods And Qualified Calls](../spec/07-functions.md#generic-methods-and-qualified-calls);
- [Function Types And Values](../spec/07-functions.md#function-types-and-values),
  including [Generic Function Values](../spec/07-functions.md#generic-function-values)
  and [Suspending Function Values](../spec/07-functions.md#suspending-function-values);
- [Member Namespaces](../spec/03-names-and-scopes.md#member-namespaces)
  (`names.member.*`, `names.lookup.*`);
- [Trait-Qualified Calls](../spec/09-traits.md#trait-qualified-calls) and
  [Associated Function Calls](../spec/09-traits.md#associated-function-calls);
- [Variant Constructors As Function Values](../spec/08-data-and-enums.md#variant-constructors-as-function-values);
- [Pipe Operator PL1-PL10](PIPE_OPERATOR.md#owner-decisions) and
  [Chaining Study CS1-CS7](CHAINING_STUDY.md#owner-decisions).

## Owner Decisions

Decided 2026-09-29.

1. **MR1: `::` method references, as in Kotlin.**
   - **Unbound** `Type::method` (and `Trait::method`, `T::method`) is a
     function value with the receiver first, like the qualified call
     without its arguments: `Counter::reset: fn(mut Counter) -> void`.
   - **Bound** `value::method` captures the receiver object **when the
     reference is created**, like Kotlin, Go method values and C# method
     groups. After `r := counter::reset; counter = other`, calling `r()`
     resets the old object, while a closure `fn(): counter.reset()` reads
     the variable at call time. A bound reference to a `mut self` method
     needs a mutable view of the receiver at creation.
2. **MR2: fields never get a reference form.** There is no `User::email`
   and no key paths. Fields stay closures (`fn(u): u.email`). This also
   removes any field/method name clash.
3. **MR3: a suspending method is referenced as `Store::load`, without
   `!`,** like free function values. The `fn!` type keeps the suspension
   visible.
4. **MR4: a callee path is a valid bare pipe step:** `raw |> Config::parse`.
   The pipe rule that a bare step must not suspend still applies, so a
   suspending method needs `raw |> Store::load!(_)`.
5. **Open for the apply pass:**
   - generics on references (`Json::decode[User]`, while the pipe's `[...]`
     ban for bare steps stays);
   - inherent members before trait members;
   - inferring `Self` for trait references from the expected type.

   The record's recommendations apply unless the owner says otherwise.

6. **MR6 (2026-09-29): `value::name` where `name` is an associated
   function is `unknown-method`.** This answers Still Open 4. An
   associated function has no receiver to bind, so no method of that name
   takes one.
7. **MR7 (2026-09-29): `Identity::echo[i32]!(42)` stays a
   `syntax-error`.** This answers Still Open 5. A qualified bang call has
   one spelling, `Identity::echo![i32](42)`.

## Still Open

Points the apply pass met (2026-09-29). They are the open points of
decision 5, applied as the record recommends; each can change without
breaking a decision. MR6 and MR7 answered points 4 and 5: MR6 is
[`fn.ref.bound.associated`](../spec/07-functions.md#r-fn.ref.bound.associated),
and MR7 keeps
[`grammar.primary.method-reference.no-bang`](../spec/02-grammar.md#r-grammar.primary.method-reference.no-bang).

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | Generics on references, `Json::decode[User]` | Type arguments follow the name and owner arguments precede `::` ([`fn.ref.generic`](../spec/07-functions.md#r-fn.ref.generic)); every parameter is instantiated as for a generic function value. A bare pipe step still takes no `[...]` | Keep. |
| 2 | Inherent members before trait members | A reference resolves as the qualified call does: inherent first, then available traits, and two trait candidates are `ambiguous-method` ([`fn.ref.lookup`](../spec/07-functions.md#r-fn.ref.lookup)) | Keep. |
| 3 | `Self` for a trait reference | Inferred from the expected function type; unsolved is `unresolved-generic-placeholder` ([`fn.ref.trait-self`](../spec/07-functions.md#r-fn.ref.trait-self)) | Keep. |

## Contents

1. [Problem](#problem)
2. [What hd Has Today](#what-hd-has-today)
3. [Use Cases](#use-cases)
4. [Survey](#survey)
5. [Option 1: Closures Only](#option-1-closures-only)
6. [Option 2: Callee Paths](#option-2-callee-paths)
7. [Option 3: Double-Colon For Every Member](#option-3-double-colon-for-every-member)
8. [Option 4: Type-Dot Members](#option-4-type-dot-members)
9. [Option 5: Key Paths For Fields](#option-5-key-paths-for-fields)
10. [Cross-Cutting Details](#cross-cutting-details)
11. [Comparison](#comparison)
12. [Ranking By Design Cost Order](#ranking-by-design-cost-order)
13. [Recommendation](#recommendation)
14. [Questions For The Owner](#questions-for-the-owner)
15. [Sources](#sources)
16. [Parse Log](#parse-log)

## Problem

Should an hd program name a method, an associated function, or a field as a
function value, and how is it spelled? Today every such use needs a closure,
such as `fn(user): user.domain()`.

Recorded decisions that bind the answer:

| Decision | Effect here | Source |
| --- | --- | --- |
| CS1 | iterator adapters are methods, so callbacks are the main use | [CS1](CHAINING_STUDY.md#owner-decisions) |
| CS2 | a pipe step is a `_` step or a bare name or path; `x \|> f` means `f(x)`; a bare step must not suspend and takes no `.`, `[` or `(` after it | [CS2](CHAINING_STUDY.md#owner-decisions) |
| CS3 | no `.name` member-path shorthand, because it looks like `.None` | [CS3](CHAINING_STUDY.md#owner-decisions) |
| CS5 | references get designed now; the spelling must not look like enum shorthand | [CS5](CHAINING_STUDY.md#owner-decisions) |
| CS7 | `Iterator[T]` becomes a closure-backed `data` type; adapters are ordinary generic methods | [CS7](CHAINING_STUDY.md#owner-decisions) |
| PL10 | no `_` lambda shorthand for now | [PL10](PIPE_OPERATOR.md#owner-decisions) |
| Declarations only | a declaration's generics and signature are written; only use sites infer | [Generic Function Values](../spec/07-functions.md#generic-function-values) |

## What hd Has Today

| Area | Today | Source |
| --- | --- | --- |
| Named function values | `check` passes where `fn(i32, string) -> bool` is expected | [`fn.type.named-value`](../spec/07-functions.md#r-fn.type.named-value) |
| Suspending function values | `fn load!(...)` is referenced as `load`, with type `fn!(...)`; calling it without `!` builds a cold suspension | [Function Type Constructors](../spec/07-functions.md#function-type-constructors), [`req.suspend.type.plain-call`](../spec/11-requirements-and-suspension.md#r-req.suspend.type.plain-call) |
| Variant constructors | `SyncError.Fs`, with a `.` and no arguments, is a `fn(FsError) -> SyncError` | [`data.enum.fn-value`](../spec/08-data-and-enums.md#r-data.enum.fn-value) |
| `.Variant` shorthand | needs an expected enum type, never a function value | [`data.enum.fn-value.shorthand`](../spec/08-data-and-enums.md#r-data.enum.fn-value.shorthand) |
| Generic function values | every type parameter is instantiated from the expected type, an explicit list, or the enclosing call | [`fn.type.generic.instantiate-from`](../spec/07-functions.md#r-fn.type.generic.instantiate-from) |
| Method values | `Type::name`, `Trait::name` (unbound) and `x::name` (bound) are reserved and rejected: `deferred-method-value` | [`fn.unsupported.method-value`](../spec/07-functions.md#method-values) |
| Generic bound methods | "the explicitly instantiated member must be called" | [`fn.generic.bound-method-values`](../spec/07-functions.md#r-fn.generic.dot-member-value) |
| `::` today | associated calls `Duration::seconds(2)`, `User::guest()`, `T::create()`; trait-qualified calls `Display::to_string(v)`, `Add[Money]::add(a, b)` | [`fn.method.associated`](../spec/07-functions.md#r-fn.method.associated), [`trait.qualified.form`](../spec/09-traits.md#r-trait.qualified.form) |
| Type arguments on `::` | owner arguments before `::` (`Add[Money]::add`), member arguments after the name (`Type::name[T](...)`), `!` stays on the name (`Store::load![User](key)`) | [`fn.generic.qualified.member-list`](../spec/07-functions.md#r-fn.generic.qualified.member-list), [`fn.generic.bang.examples`](../spec/07-functions.md#r-fn.generic.bang.examples) |
| Member namespaces | fields and methods are separate namespaces and may share a name | [`names.member.shared-name`](../spec/03-names-and-scopes.md#r-names.member.shared-name) |
| Member lookup | `x.name` uses field lookup only; `x.name(args)` uses method lookup only | [`names.lookup.field-form`](../spec/03-names-and-scopes.md#r-names.lookup.field-form), [`names.lookup.method-form`](../spec/03-names-and-scopes.md#r-names.lookup.method-form) |
| Receivers | a method call equals calling the method with the receiver first; `mut self` is `self: mut Self` | [`fn.method.equivalence`](../spec/07-functions.md#r-fn.method.equivalence), [`fn.method.mut-self`](../spec/07-functions.md#r-fn.method.mut-self) |
| Overloading | none: one name resolves to one declaration | [`fn.name.no-overloading`](../spec/07-functions.md#r-fn.name.no-overloading), [`trait.inherent.no-overloading`](../spec/09-traits.md#r-trait.inherent.no-overloading) |
| Function values | positional only, no defaults; row and suspension are part of the type; no access permission | [`fn.type.positional`](../spec/07-functions.md#r-fn.type.positional), [`fn.type.signature-parts`](../spec/07-functions.md#r-fn.type.signature-parts), [`fn.type.no-permission`](../spec/07-functions.md#r-fn.type.no-permission) |
| Closure captures | captured `let` storage is shared, so a closure reads a variable's current value when it runs | [`fn.capture.storage.shared`](../spec/07-functions.md#r-fn.capture.storage.shared) |
| Adapter callbacks | lazy adapter callbacks have the empty row | [`flow.adapter.callback-row`](../spec/06-control-flow.md#r-flow.adapter.callback-row) |

The spec already names the workaround: an explicit closure adapts a method
where a function value is needed
([`fn.unsupported.closure-adapter`](../spec/07-functions.md#r-fn.unsupported.closure-adapter)).

## Use Cases

Every option shows the same six cases.

| # | Case | Closure today |
| --- | --- | --- |
| R1 | Field projection | `users.map(fn(user): user.email)` |
| R2 | Inherent method, receiver as the argument | `users.map(fn(user): user.domain())` |
| R3 | Associated function | `counts.map(fn(n): Duration::seconds(n))` |
| R4 | Trait method | `values.map(fn(value): Display::to_string(value))` |
| R5 | Bound receiver, `mut self` | `every(3, fn(): counter.reset())` |
| R6 | Pipe bare step | `raw \|> Config::parse(_)` |

Shared declarations. They parse; `Config` and `ParseError` are left
undeclared, since parsing checks syntax only:

```text
data User:
    name: string
    email: string

impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

    fn name(self) -> string:
        self.name.trim()

data Counter:
    value: i32

impl Counter:
    fn reset(mut self) -> void:
        self.value = 0

fn every(ticks: i32, action: fn() -> void) -> void:
    pass

impl Config:
    fn parse(text: string) -> Result[Config, ParseError]:
        pass
```

`User` has a field `name` and a method `name`, which
[`names.member.shared-name`](../spec/03-names-and-scopes.md#r-names.member.shared-name)
allows. Every option must say what a reference to `name` means.

## Survey

| Language | Unbound method | Bound method | Field | Type arguments | Mutating receiver | Sources |
| --- | --- | --- | --- | --- | --- | --- |
| Kotlin | `Regex::matches` is `(Regex, CharSequence) -> Boolean` | `numberRegex::matches` has "its receiver attached" | `String::length`, `::x` as a property object | an overload is picked by the expected type | no receiver marker | [Callable references](https://kotlinlang.org/docs/reflection.html#callable-references), [Bound references](https://kotlinlang.org/docs/reflection.html#bound-function-and-property-references) |
| Java | `String::compareToIgnoreCase`, receiver first; `Type::staticMethod`; `HashSet::new` | `containingObject::instanceMethodName` | none; a getter `User::getName` | `Type::<T>name` | no marker | [Method references tutorial](https://docs.oracle.com/javase/tutorial/java/javaOO/methodreferences.html), [JLS 15.13](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.13) |
| C# | no instance form; a static `Math.Abs` converts to a delegate | a method group `user.Greet` converts to a delegate | none; a lambda | overload chosen by the delegate type | n/a | [Method group conversions](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/conversions#108-method-group-conversions) |
| Swift | `User.greet` is curried: `(User) -> () -> String` | `user.greet` | key path `\User.name`, usable as a function `(User) -> String` | inferred | the curried form "is incompatible with `mutating` methods"; flattening was rejected; method key paths were returned for revision | [SE-0042](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0042-flatten-method-types.md), [SE-0161](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0161-key-paths.md), [SE-0249](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0249-key-path-literal-function-expressions.md), [SE-0479](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0479-method-and-initializer-keypaths.md) |
| Rust | a path such as `str::len` or `User::name` names the method or associated function, receiver first | none: `x.method` without a call is error E0615 | none; closures | turbofish on the path: `str::parse::<i32>` | `&mut self` becomes an `&mut T` first parameter | [Path expressions](https://doc.rust-lang.org/reference/expressions/path-expr.html), [E0615](https://doc.rust-lang.org/error_codes/E0615.html) |
| Scala 3 | `_.greet` placeholder | automatic eta-expansion: `user.greet` becomes a function where a function type is expected; not for nullary methods | `_.name` | inferred | n/a | [Automatic eta expansion](https://docs.scala-lang.org/scala3/reference/changed-features/eta-expansion.html) |
| Go | method expression `User.Greet`, receiver first; `(*User).SetName` for pointer receivers | method value `u.Greet`; "the receiver is evaluated and saved at the time the method value is obtained" | none; closures | methods can't have type parameters | the pointer type is named in the expression | [Method values](https://go.dev/ref/spec#Method_values), [Method expressions](https://go.dev/ref/spec#Method_expressions) |
| F# | `_.Greet()` shorthand (F# 8) | method as a first-class value | `_.Name` for `fun x -> x.Name` | inferred | n/a | [Announcing F# 8](https://devblogs.microsoft.com/dotnet/announcing-fsharp-8/) |

### Takeaways

1. **A method reference is the callee without its argument list.** Rust
   `str::len`, Java and Kotlin `Type::m`, and Go `T.M` all reuse the
   spelling of the call. hd's qualified call `User::domain(user)` already
   has that form.
2. **Field references need their own mechanism.** Rust, Go, Java and C#
   have none and use closures. Kotlin folds fields into properties, Swift
   adds the `\` token, and Scala and F# use a `_` placeholder, which PL10
   ruled out for hd.
3. **A bound reference evaluates its receiver once, when it's created**
   (Go's spec, Kotlin's "attached" receiver). A closure instead reads its
   captured variable when it runs.
4. **Mutating receivers are the hard case.** Swift's curried unapplied
   methods broke on `mutating` (SE-0042). Rust and Go put the mutable
   receiver in the first parameter's type.

## Option 1: Closures Only

**Idea.** The radical simplification. No reference syntax; `fn(x): ...` is
the only way to turn a member into a function value. `Type::name` and
`x::name` stay reserved, and `deferred-method-value` gains a fix-it that
writes the closure.

```text
fn report(users: List[User], counts: List[i64], values: List[i32], counter: mut Counter) -> void:
    emails := users.map(fn(user): user.email)
    domains := users.map(fn(user): user.domain())
    delays := counts.map(fn(n): Duration::seconds(n))
    labels := values.map(fn(value): Display::to_string(value))
    every(3, fn(): counter.reset())
```

R6 keeps the `_` form:

```text
fn load(raw: string) -> Result[Config, ParseError]:
    raw |> Config::parse(_)  # hypothetical syntax
```

- **Rules added:** none. The fix-it is diagnostic text.
- **Soundness:** unchanged.
- **Interactions:** none. A closure already carries rows, `mut`, `!`, and
  type arguments explicitly.
- **Cost:** every case names a parameter twice, as in `fn(user): user.` or
  `fn(n): Duration::seconds(n)`.

## Option 2: Callee Paths

**Idea.** Rust's paths. `Type::name`, `Trait::name` or `T::name`, with no
argument clause, is a function value. It names a method (receiver first)
or an associated function. Fields and bound receivers keep closures.
This lifts only the unbound half of the reservation.

```text
fn report(users: List[User], counts: List[i64], values: List[i32], counter: mut Counter) -> void:
    emails := users.map(fn(user): user.email)
    domains := users.map(User::domain)  # hypothetical syntax
    delays := counts.map(Duration::seconds)  # hypothetical syntax
    labels := values.map(Display::to_string)  # hypothetical syntax
    every(3, fn(): counter.reset())
```

```text
fn load(raw: string) -> Result[Config, ParseError]:
    raw |> Config::parse  # hypothetical syntax
```

Rules added:

| Rule | Text |
| --- | --- |
| Form | `Type::name`, `Trait::name`, or `T::name`, with an optional member-level type argument list and no argument clause, is a **callee path**. |
| Resolution | A callee path resolves exactly as the qualified call with the same path does ([Associated Function Calls](../spec/09-traits.md#associated-function-calls), [Trait-Qualified Calls](../spec/09-traits.md#trait-qualified-calls)). |
| Method type | For a method, the value's first parameter is the receiver: `fn domain(self)` gives `fn(User) -> string`, and `fn reset(mut self)` gives `fn(mut Counter) -> void`. |
| Associated function type | For an associated function, the value has the function's own type. |
| Generics | A generic member follows [Generic Function Values](../spec/07-functions.md#generic-function-values). |
| No fields | A callee path never names a field. |
| Bound form | `x::name` stays reserved: `deferred-method-value`. |

- **Soundness.** A callee path is the named function value that
  [`fn.method.equivalence`](../spec/07-functions.md#r-fn.method.equivalence)
  already describes. No new type rule is needed.
- **Shared names.** `User::name` is the method `name`, because `::` never
  reaches the field namespace. No ambiguity rule is needed.
- **Wasm GC.** A non-generic callee path is a static function reference,
  like a named function; no closure is allocated.
- **Cost kinds:** a grammar form (already reserved; the parser already
  recognizes it and reports `deferred-method-value`) and one semantic rule
  set that reuses existing ones.

## Option 3: Double-Colon For Every Member

**Idea.** Kotlin's model. Option 2, plus `Type::field` for fields and
`x::name` for bound methods, using both reserved spellings.

```text
fn report(users: List[User], counts: List[i64], values: List[i32], counter: mut Counter) -> void:
    emails := users.map(User::email)  # hypothetical syntax
    domains := users.map(User::domain)  # hypothetical syntax
    delays := counts.map(Duration::seconds)  # hypothetical syntax
    labels := values.map(Display::to_string)  # hypothetical syntax
    every(3, counter::reset)  # hypothetical syntax
```

Rules added, beyond Option 2:

| Rule | Text |
| --- | --- |
| Field reference | `Type::field` is a `fn(Type) -> F`, where `F` is the field's type as read through a readonly receiver. |
| Shared name | When a field and a method share the name, `Type::name` must pick one: the method, the field, or an error (question 3). |
| Bound reference | `x::name` evaluates `x` once, when the value is made, and captures it; the result drops the receiver parameter. |
| Bound `mut` | For a `mut self` method, `x` needs mutable access when the value is made; the result is a plain `fn` ([`fn.type.no-permission`](../spec/07-functions.md#r-fn.type.no-permission)). |

- **Soundness.** A field reference is a closure the compiler writes. A
  bound reference captures a `mut` receiver as a closure capture does
  ([`fn.capture.mutate.forms`](../spec/07-functions.md#r-fn.capture.mutate.forms)).
- **Behavior change.** `counter::reset` fixes the receiver when it is made.
  `fn(): counter.reset()` reads a `let counter` when it runs. After
  `counter = other`, the two call different objects.
- **Shared names.** Makes `names.member.no-hiding` depend on the use: a
  `::` use would look in both namespaces.
- **Cost kinds:** two grammar forms (both reserved), field and bound rules,
  and one shared-name rule.

## Option 4: Type-Dot Members

**Idea.** Go's method expressions and Swift's unapplied methods. Extend the
existing variant rule
([`data.enum.fn-value`](../spec/08-data-and-enums.md#r-data.enum.fn-value)):
`Type.member` with no argument clause is a function value, for fields,
methods and associated functions. It parses today, like `SyncError.Fs`.

```text
fn report(users: List[User], counts: List[i64], values: List[i32], counter: mut Counter) -> void:
    emails := users.map(User.email)
    domains := users.map(User.domain)
    delays := counts.map(Duration.seconds)
    labels := values.map(Display.to_string)
    every(3, fn(): counter.reset())
```

Rules added:

| Rule | Text |
| --- | --- |
| Type-dot value | `Type.name`, where `Type` names a type or trait and `name` is not a variant, is a function value, receiver first. |
| Shared name | The same shared-name choice as Option 3. |
| Enum types | A method and a variant with one name on an enum need a rule. |

- **No syntax change.** Every line above parses today; each needs the new
  semantic rule to type-check.
- **Two spellings.** Associated functions are called `Duration::seconds(2)`
  but referenced `Duration.seconds`. The value is no longer "the call
  without its arguments".
- **Reading.** `Shape.area` (a method) and `Shape.Circle` (a variant) look
  alike and differ only by case convention.
- **Pipe.** `raw |> Config.parse` fits CS2's "dotted path" bare step as
  written, so no pipe rule changes.
- **Cost kinds:** semantic rules only (the costliest is a rule exception
  on an existing rule).

## Option 5: Key Paths For Fields

**Idea.** Swift's split. Methods and associated functions use callee paths
(Option 2). Fields use a new key-path token, `\Type.field`, which may
chain through fields: `\User.address.city`.

```text
fn report(users: List[User], counts: List[i64], values: List[i32], counter: mut Counter) -> void:
    emails := users.map(\User.email)  # hypothetical syntax
    domains := users.map(User::domain)  # hypothetical syntax
    delays := counts.map(Duration::seconds)  # hypothetical syntax
    labels := values.map(Display::to_string)  # hypothetical syntax
    every(3, fn(): counter.reset())
```

- **Rules added:** Option 2's, a `\` token, a key-path grammar form, and
  its typing rule (`fn(Type) -> F`, readonly).
- **Shared names.** None: `\` reaches fields only, `::` methods only.
- **Evolution.** A key path could later become a typed value, for lenses
  or typed derivation, as Swift's `KeyPath` is.
- **Cost kinds:** a new token and a grammar form, plus Option 2's.

## Cross-Cutting Details

Each point below applies to every option with a reference form (2 to 5).

| Topic | Behavior | Source it follows |
| --- | --- | --- |
| `mut self` | The reference has type `fn(mut Counter) -> void`. Passing it where `fn(Counter) -> void` is expected fails, because parameters are contravariant and `Counter` isn't assignable to `mut Counter`. | [`fn.type.declared-variance`](../spec/07-functions.md#r-fn.type.declared-variance) |
| `!` methods | `fn load!(self, key: Key)` is referenced as `Store::load`, without `!`, as a free `fn load!` is referenced as `load`. Its type `fn!(Store, Key) -> ...` keeps suspension visible: a later call writes `f!(store, key)`. | [Suspending Function Values](../spec/07-functions.md#suspending-function-values) |
| Pipe and `!` | A suspending reference as a bare pipe step is an error under CS2; write `x \|> Store::load!(_, key)`. | [CS2](CHAINING_STUDY.md#owner-decisions) |
| Rows | A method with `$ Db` gives `fn(Repo, Id) -> User $ Db`. A lazy adapter rejects it, as it rejects any named function with a row. | [`flow.adapter.callback-row`](../spec/06-control-flow.md#r-flow.adapter.callback-row) |
| Trait methods | `Display::to_string` infers `Self` from the expected type, as a trait-qualified call does; if nothing fixes it, it's an error. `T::describe` resolves through the bound of `T`. | [`trait.assoc-call.trait`](../spec/09-traits.md#r-trait.assoc-call.trait), [`trait.assoc-call.parameter`](../spec/09-traits.md#r-trait.assoc-call.parameter) |
| Inherent versus trait | `User::to_string` looks for an inherent member first, then available traits; two trait candidates are `ambiguous-method`. | [`trait.assoc-call.type`](../spec/09-traits.md#r-trait.assoc-call.type) |
| Overloads | hd has none, so a reference never needs the expected type to pick a declaration (Kotlin and C# do). | [`fn.name.no-overloading`](../spec/07-functions.md#r-fn.name.no-overloading) |
| Explicit type arguments | `Json::decode[User]` is a reference with every parameter given; owner arguments stay before `::`, as in `Box[i32]::get`. | [`fn.generic.qualified.member-list`](../spec/07-functions.md#r-fn.generic.qualified.member-list) |
| Pipe and `[...]` | CS2 bans `[` after a bare step, so `x \|> Json::decode[User]` is an error; write `x \|> Json::decode[User](_)`. | [CS2](CHAINING_STUDY.md#owner-decisions) |
| Defaults and names | A reference drops parameter names and defaults, as every function value does. | [`fn.type.positional`](../spec/07-functions.md#r-fn.type.positional) |
| `.None` | No option starts with a bare `.`. Options 2, 3 and 5 start with `::` or `\`. Option 4 looks like `SyncError.Fs`, a variant function value, not like `.None`. | [CS3](CHAINING_STUDY.md#owner-decisions) |

Two examples of these details. The first is valid under Options 2 to 5;
the second is an error (`type-mismatch`) because `map` passes a readonly
`Counter`:

```text
fn load_one!(store: Store, key: Key) -> Blob:
    loader := Store::load  # hypothetical syntax
    loader!(store, key)

fn clear(counters: List[Counter]) -> void:
    counters.map(Counter::reset)  # hypothetical syntax
```

In the first, `loader(store, key)` without `!` would build a cold
suspension instead.

## Comparison

| | 1 Closures only | 2 Callee paths | 3 `::` for all | 4 Type-dot | 5 Key paths |
| --- | --- | --- | --- | --- | --- |
| R1 field | closure | closure | `User::email` | `User.email` | `\User.email` |
| R2 method | closure | `User::domain` | `User::domain` | `User.domain` | `User::domain` |
| R3 associated | closure | `Duration::seconds` | `Duration::seconds` | `Duration.seconds` | `Duration::seconds` |
| R4 trait method | closure | `Display::to_string` | `Display::to_string` | `Display.to_string` | `Display::to_string` |
| R5 bound | closure | closure | `counter::reset` | closure | closure |
| R6 pipe | `Config::parse(_)` | `Config::parse` | `Config::parse` | `Config.parse` | `Config::parse` |
| Rules added | 0 | 1 set, reusing qualified calls | 3 sets | 2 sets | 2 sets and a token |
| Shared-name rule | not needed | not needed | needed | needed | not needed |
| Reads as the call | n/a | yes | yes, except fields | no for associated functions | yes; fields look different |
| Agent-writability | one form to learn | Rust habit carries over | Kotlin habit carries over | Go habit carries over; may confuse with variants | two forms |
| Implementation | none | small: lift a parse error, reuse resolution | medium: field thunks, receiver capture | small parse, new checker rule | lexer, grammar, checker |
| Evolution | any option later | Option 3's `x::name` or Option 5's fields can be added | hard to remove | hard to move to `::` later | key paths can grow into typed values |

## Ranking By Design Cost Order

[AGENTS.md](../AGENTS.md#design-cost-order) ranks by the costliest change,
where a syntax change is costliest.

| Rank | Option | Costliest change | Changes |
| --- | --- | --- | --- |
| 1 | Closures only | none | 0 |
| 2 | Type-dot members | semantic rule exception (2) | 2 |
| 3 | Callee paths | syntax (1): a reserved grammar form | 2 |
| 4 | `::` for all | syntax (1): two reserved grammar forms | 5 |
| 5 | Key paths | syntax (1): a new token and a form | 4 |

Callee paths rank below type-dot by the order. The syntax change is the
smallest one possible: the parser already recognizes `Type::name` and
reports `deferred-method-value`, so admitting it adds no token.

## Recommendation

**Recommendation:** Option 2, callee paths, with fields and bound receivers
kept as closures.

- It reuses the spelling of the call hd already has, so a reference is "the
  qualified call without its arguments", as in Rust, Java and Go.
- `::` never reaches fields, so the shared field and method name needs no
  new rule, and `names.member.no-hiding` holds.
- It covers R2, R3, R4 and R6, the cases where no field-style shorthand
  helps. R3 and R4 are the longest closures today.
- It keeps the one real behavior hazard (bound receivers fixed at
  creation, Option 3) out.
- It gives up R1 and R5 shorthands. CS3 and PL10 already chose closures for
  fields; key paths (Option 5) can add them later without a conflict.

Next best: Option 4, which is cheaper by the cost order but gives
associated functions two spellings and looks like a variant.

## Questions For The Owner

### 1. Which reference mechanism does hd adopt?

Effect: decides how R2-R4 and R6 are written and which reserved spellings
are lifted.

- **A.** Closures only (Option 1).
- **B.** Callee paths `Type::name` for methods and associated functions
  (Option 2).
- **C.** `::` for fields, methods and bound receivers (Option 3).
- **D.** Type-dot members (Option 4).

**Recommendation:** B.

```text
fn domains(users: List[User]) -> List[string]:
    users.map(User::domain)  # hypothetical syntax
```

### 2. Do fields get a reference form?

Effect: callbacks that read one member were 8 of 12 closures in the
[Chaining Study corpus](CHAINING_STUDY.md#questions-for-the-owner);
field reads such as R1 are the ones Option 2 leaves as closures.

- **A.** No; closures, as CS3 already decided for `.name`.
- **B.** `Type::field`, sharing `::` with methods (Option 3).
- **C.** A key path `\Type.field` (Option 5).

**Recommendation:** A now; C if the hd writing log shows demand.

```text
fn emails(users: List[User]) -> List[string]:
    users.map(fn(user): user.email)
```

### 3. If fields use `::`, what does `User::name` mean when both exist?

Effect: only asked if question 2 is B. `User` has a field and a method
called `name`.

- **A.** An error; write a closure.
- **B.** The method wins, as in a qualified call today.
- **C.** The field wins.

**Recommendation:** A, so neither namespace hides the other.

```text
fn names(users: List[User]) -> List[string]:
    users.map(User::name)  # hypothetical syntax
```

### 4. Do bound references `x::name` exist?

Effect: `counter::reset` fixes `counter` when it's made; the closure reads
it when it runs.

- **A.** No; keep `x::name` reserved and write a closure.
- **B.** Yes, capturing the receiver at creation (Kotlin, Go).

**Recommendation:** A.

```text
fn schedule(counter: mut Counter) -> void:
    every(3, fn(): counter.reset())
```

### 5. How is a suspending method referenced?

Effect: `fn load!(self, key: Key)` as a value.

- **A.** `Store::load`, as a free `fn load!` is referenced as `load`.
- **B.** `Store::load!`, keeping the marker on the name.

**Recommendation:** A. The type `fn!(...)` and the later `f!(...)` call
show the suspension.

```text
fn loader() -> fn!(Store, Key) -> Blob:
    Store::load  # hypothetical syntax
```

### 6. Is a callee path a bare pipe step?

Effect: CS2 allows "a name or path"; this confirms `Type::name` counts.

- **A.** Yes: `raw |> Config::parse` means `Config::parse(raw)`.
- **B.** No: only free function names and dotted module paths.

**Recommendation:** A. It falls out of question 1 B with no pipe rule.

```text
fn load(raw: string) -> Result[Config, ParseError]:
    raw |> Config::parse  # hypothetical syntax
```

## Sources

- Kotlin: [Callable references](https://kotlinlang.org/docs/reflection.html#callable-references), [Bound function and property references](https://kotlinlang.org/docs/reflection.html#bound-function-and-property-references)
- Java: [Method references tutorial](https://docs.oracle.com/javase/tutorial/java/javaOO/methodreferences.html), [JLS 15.13](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.13)
- C#: [Method group conversions](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/conversions#108-method-group-conversions)
- Swift: [SE-0042](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0042-flatten-method-types.md) (rejected), [SE-0161](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0161-key-paths.md), [SE-0249](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0249-key-path-literal-function-expressions.md), [SE-0479](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0479-method-and-initializer-keypaths.md) (returned for revision)
- Rust: [Path expressions](https://doc.rust-lang.org/reference/expressions/path-expr.html), [E0615](https://doc.rust-lang.org/error_codes/E0615.html)
- Scala 3: [Automatic eta expansion](https://docs.scala-lang.org/scala3/reference/changed-features/eta-expansion.html)
- Go: [Method values](https://go.dev/ref/spec#Method_values), [Method expressions](https://go.dev/ref/spec#Method_expressions)
- F#: [Announcing F# 8](https://devblogs.microsoft.com/dotnet/announcing-fsharp-8/)

## Parse Log

Every `text` block was parsed with the reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)) on
2026-09-29. Parsing checks syntax only; no block is claimed to type-check.

The parser stops at the first error and does not know `|>`. Each failing
block was parsed a second time, **desugared**: `|>` replaced by `|`, `_`
by a name, `\` removed, and `A::b` without a call replaced by `A_b`. Every
desugared block parses, and every first error falls on a line marked
`# hypothetical syntax`.

The Option 4 block parses today; it needs the new rule to type-check.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Use Cases | parses |
| 2 | Option 1: Closures Only | parses |
| 3 | Option 1: Closures Only | `syntax-error` on line 2; desugared: parses |
| 4 | Option 2: Callee Paths | `deferred-method-value` on line 3; desugared: parses |
| 5 | Option 2: Callee Paths | `deferred-method-value` on line 2; desugared: parses |
| 6 | Option 3: Double-Colon For Every Member | `deferred-method-value` on line 2; desugared: parses |
| 7 | Option 4: Type-Dot Members | parses |
| 8 | Option 5: Key Paths For Fields | `invalid-token` on line 2; desugared: parses |
| 9 | Cross-Cutting Details | `deferred-method-value` on line 2; desugared: parses |
| 10 | 1. Which reference mechanism does hd adopt? | `deferred-method-value` on line 2; desugared: parses |
| 11 | 2. Do fields get a reference form? | parses |
| 12 | 3. If fields use `::`, what does `User::name` mean when both exist? | `deferred-method-value` on line 2; desugared: parses |
| 13 | 4. Do bound references `x::name` exist? | parses |
| 14 | 5. How is a suspending method referenced? | `deferred-method-value` on line 2; desugared: parses |
| 15 | 6. Is a callee path a bare pipe step? | `deferred-method-value` on line 2; desugared: parses |
