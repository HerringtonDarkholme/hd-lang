# Typed Derivation: Stress Test Round 2 (M1-M15)

Status: design review, 2026-09-27. Nothing here is accepted language
behavior, and nothing here changes the design record, the specification, or
the prototype. Every design choice below is a question for the owner.

The design under test is the one recorded in
[Typed Derivation](TYPED_DERIVATION.md) after decision M14 (typed member
handles) and M15 (no impl families; error derivation is the intrinsic
`@derive(Error)`; facts may carry a compile-time check): the
[Current Design: Full Example](TYPED_DERIVATION.md#current-design-full-example-m1-m14)
and the [Current Rules](TYPED_DERIVATION.md#current-rules-m1-m14), plus
[Error Conversion decision 10](ERROR_CONVERSION.md#owner-decisions) for
`@derive(Error)`. [Round 1](DERIVATION_STRESS_TEST.md) tested the M1-M11
design; its problem ids P1-P20 are reused here. New problems are R1-R20.

The goal is to find where the design breaks, so each case pushes on the
parts that M14 and M15 claim to have solved.

## Contents

1. [Method](#method)
2. [Summary](#summary)
3. [The Surface Being Tested](#the-surface-being-tested)
4. [Use Cases](#use-cases) (1-12)
5. [Round-1 Problems Under M14](#round-1-problems-under-m14)
6. [Problems, Ranked](#problems-ranked)
7. [Parse Log](#parse-log)

## Method

As in round 1, each use case gives the library side, the user side with the
tiers that apply, a verdict (*works*, *works with friction*, or *breaks*),
and the design element at fault. The code follows specification syntax:
`=` for named arguments, brace data literals, `let x: T = ...` and
`x := ...`, `T::f()` inside templates, `Result[void, E]` with `.Ok()`, no
`mut` at argument sites, and `use dep.json` so annotations read
`@json.json(...)`. Library code reuses the names of the reference example
(`Encoder[S]`, `FieldSource[S]`, `key_for`, `apply_case`) without repeating
them. Lines that use syntax no chapter specifies yet end in
`# hypothetical syntax`; the [Parse Log](#parse-log) lists every block.

Two limits of this method. Parsing checks syntax only: nothing here was
type-checked, and the prototype implements none of the surface. And where
the design leaves an API open (the handle API, the fact check hook), a block
that needs it says what it assumes.

## Summary

| # | Use case | Verdict | Design element at fault |
| --- | --- | --- | --- |
| 1 | Error enums with `@derive(Error)` (ast-grep's `RuleCoreError`) | Works for the reference case; two shapes **break** | A generic `@from` payload overlaps a concrete one (R6). Spans per value cannot use `@from`, and its common-field rule contradicts 08 (R12). `?` silently picks the `@from` variant among variants that share a payload type (R11). `@transparent` hides the payload from `find` (R10). Messages see a new scope and may recurse through `$self` (R7). The two spellings of member `@source` disagree (R4). |
| 2 | Fact check hook | Works for single-fact checks; cross-member and type checks **break** | The hook sees one fact and its target, not the type (R8). Member types are visible only as exact `TypeId`s (R8). Facts on a type that no template reads are never checked (R8). |
| 3 | `Eq`, `Ord`, `Hash` | Works with friction | Each walker rebuilds "same variant" bookkeeping; variant choice is a linear scan of `holds` (R2). `= pass` lines are per block, so `Eq` and `Hash` can disagree silently (R3). |
| 4 | Clone, diff and patch, one-step shrinking | Clone works; with `mut` members **breaks**. Diff and patch: friction. Shrink: friction | `Field[S, F]` cannot type a `mut U` member for both `get` and `build` (R1). No patch type (P10). Shrinking recomputes each member's candidates per candidate (R2, P7). |
| 5 | JSON Schema, OpenAPI, MCP tool schemas, CLI help, DDL | Works with friction | No type name, docs, or enum flag without `std.inspect` (R9). `variant` cannot report an error (R2). Function targets still wait for FN_TYPE. |
| 6 | Handles under pressure (member model) | Mixed | Handles escape into closures and module storage (R5). Structure's `facts`, `walk`, `build` collide with trait methods (R13). Embedding, shared fields, newtypes, GADTs still unspecified (P11). The M12 bound names the trait, not the walker's bound (R14). |
| 7 | Performance under per-shape compilation | Holds per member; enums scale with variant count | Linear variant scan (R2). `default()` allocates for every type (R15). Handle projection cost is unstated (P19). |
| 8 | Validation with accumulated errors | Works with friction | Cross-member rules cannot extend the template (P16). Facts checked per fact only (R8). |
| 9 | Builders | **Breaks** for typed builders | Derivation declares no types or methods (P10). Defaults now reachable (P3 fixed). |
| 10 | Database rows with suspending I/O | Row mapping works; per-member I/O **breaks** | `walk` and `build` are pure (P8). A value-free plan plus prefetch is the workaround. |
| 11 | Versioned binary codecs | Works with friction | Cross-member tag checks need a type-level check (R8). Wire stability silent (P20). |
| 12 | Several libraries on one type | Works with friction; worse than round 1 | One skipped member now forces up to eight tier-2 blocks (P13, R3). Name collisions between libraries' template methods and Structure (R13). |

What M14 fixed is real: two-value, value-to-value, and value-free
traversals all exist as library code, and declared defaults reach sources.
What it did not fix, or made worse, sits in three places: enums (the walker
must reconstruct variant structure from `holds` answers, and `get` panics
when it gets that wrong), mutability (a handle has one member type, but
reading and constructing need different ones), and scope (handles are
ordinary values that can outlive the walk). M15 moved error enums out of
typed derivation. `@derive(Error)` with E1-E4 handles the ast-grep
reference case and most thiserror shapes; generic wrapper enums and located
errors are the ones it cannot spell.

## The Surface Being Tested

The surface is the `std.structure` block of the reference example:
`Structure` with `facts()`, `walk[W < Walker[Self]]`, and
`build[S < Source[Self]]`; the constant handles `Field[S, F]` (`info`,
`get`, `has_default`, `default`) and `Variant[S]` (`info`, `holds`); and
`Walker[S]` and `Source[S]`, whose `member[F]` bound an impl may
strengthen. This report assumes only what the example uses: `Member` has
`name` and `facts`, and a variant's `info` has `name`, `index`, and `facts`.
It does not assume a type name, member docs, a member count, an "is enum"
flag, or a variant back-reference on `Field`. Where a case needs one, it
says so.

For `@derive(Error)` the surface is Error Conversion decision 10 with its
refinements E1-E4 and the
[Current Design](ERROR_CONVERSION.md#current-design) section: markers
`@message("... $member ...")` (an ordinary interpolated string with the
payload members and common fields in scope, E1), `@from`, `@source` (on a
one-payload variant, or for one member of a multi-member variant, E4), and
`@transparent` (whose `cause()` returns the inner error's cause, E3). The
compiler always generates `impl Display` (E2), `impl Error` with `cause()`,
and one `impl From[P] for E` per `@from` variant. `std.error.Error` is as
in [STDLIB](STDLIB.md#stderror): `cause(self) -> Error?` with a default,
plus `find[T]`, `chain`, and `root_cause`.

For the fact check hook the surface is one sentence of M15: a fact type may
define a compile-time `check` against its member or variant, run at the
opt-in site. Its form is not designed, so case 2 states its assumption.

## Use Cases

### 1. Error Enums And `@derive(Error)`

#### 1.1 The reference case: ast-grep's `RuleCoreError`

Three variants carry `RuleSerializeError`. `Rule` is `@from`, so `?`
converts that type into `Rule`. `Utils` and `Constraints` are `@source`, so
they report the payload as their cause but produce no `From`.

```text
use std.error.Error
use dep.yaml
use dep.rules.{RuleSerializeError, TransformError}

@derive(Error)
pub enum RuleCoreError:
    @message("Fail to parse yaml as RuleConfig")
    @from
    Yaml(error: yaml.YamlError)
    @message("`utils` is not configured correctly.")
    @source
    Utils(error: RuleSerializeError)
    @message("`rule` is not configured correctly.")
    @from
    Rule(error: RuleSerializeError)
    @message("`constraints` is not configured correctly.")
    @source
    Constraints(error: RuleSerializeError)
    @message("`transform` is not configured correctly.")
    @from
    Transform(error: TransformError)
    @message("Undefined meta var `$var` used in `$context`.")
    UndefinedMetaVar(var: string, context: string)

fn try_new(config: SerializableRuleCore) -> Result[RuleCore, RuleCoreError]:
    utils := parse_utils(config.utils).map_err(RuleCoreError.Utils)?
    rule := parse_rule(config.rule)?                      # From[RuleSerializeError]: Rule
    constraints := parse_constraints(config.constraints).map_err(RuleCoreError.Constraints)?
    transform := parse_transform(config.transform)?       # From[TransformError]: Transform
    check_vars(rule, constraints, transform)?             # returns RuleCoreError directly
    .Ok(RuleCore { utils: utils, rule: rule, constraints: constraints, transform: transform })
```

What the intrinsic generates, per decision 10 (ordinary hd):

```text
impl Display for RuleCoreError:
    fn to_string(self) -> string:
        match self:
            RuleCoreError.Yaml(_) => "Fail to parse yaml as RuleConfig"
            RuleCoreError.Utils(_) => "`utils` is not configured correctly."
            RuleCoreError.Rule(_) => "`rule` is not configured correctly."
            RuleCoreError.Constraints(_) => "`constraints` is not configured correctly."
            RuleCoreError.Transform(_) => "`transform` is not configured correctly."
            RuleCoreError.UndefinedMetaVar(var, context) => "Undefined meta var `$var` used in `$context`."

impl Error for RuleCoreError:
    fn cause(self) -> Error?:
        match self:
            RuleCoreError.Yaml(error) => .Some(error)
            RuleCoreError.Utils(error) => .Some(error)
            RuleCoreError.Rule(error) => .Some(error)
            RuleCoreError.Constraints(error) => .Some(error)
            RuleCoreError.Transform(error) => .Some(error)
            RuleCoreError.UndefinedMetaVar(_, _) => .None

impl From[yaml.YamlError] for RuleCoreError:
    fn from(value: yaml.YamlError) -> Self:
        RuleCoreError.Yaml(value)

impl From[RuleSerializeError] for RuleCoreError:
    fn from(value: RuleSerializeError) -> Self:
        RuleCoreError.Rule(value)

impl From[TransformError] for RuleCoreError:
    fn from(value: TransformError) -> Self:
        RuleCoreError.Transform(value)
```

`.Some(error)` in `cause` is one conversion (the payload erased to `Error`
inside an explicit `.Some`), so it is valid under single-step
assignability. The whole reference case is expressible, and the messages
are type-checked. This part works.

What the reference case does not show: **`?` picks `Rule` silently.** In
`try_new`, writing `parse_utils(config.utils)?` without `.map_err`
compiles, and the error displays as "`rule` is not configured correctly."
Nothing warns that the payload type is shared with two `@source` variants
(R11).

#### 1.2 Messages

Under E1 a message is an ordinary interpolated string, type-checked with the
variant's payload members and the enum's common fields in scope. That
settles round 1's unchecked placeholders and makes computed messages
ordinary expressions:

```text
@derive(Error)
pub enum FetchError:
    @message("timed out after ${secs}s")        # error: unknown-name `secs` (checked, good)
    Timeout(seconds: i64)
    @message("expected `{` after $path")        # braces are ordinary string content
    Syntax(path: string)
    @message("missing ${keys.length()} keys: ${keys.join(", ")}")
    Missing(keys: List[string])
    @message("${plural(count, "file")} over the limit")
    TooMany(count: i64)
    @message("request failed: $self")           # accepted by type checking; recurses at run time
    Failed(status: i32)
    NotFound(string)                            # an unnamed payload: no name to interpolate
```

What remains:

1. **A new scope rule.** A decorator argument is otherwise an expression in
   the enclosing module's scope (14 Annotations). Here it sees names that
   the variant declares on the next line, and a payload member named like a
   module-level function (`format`, `path`) shadows it inside the message.
   Tooling and the reference parser treat the text as an ordinary string;
   only the intrinsic knows the scope (R7).
2. **`$self` recurses.** The message is the body of the generated
   `to_string`, so `$self` calls it again. Type checking accepts it; the
   intrinsic should reject `self` in a message (R7).
3. **Unnamed payloads.** `NotFound(string)` is legal (08 Variant Payloads),
   but nothing in scope names its payload, so the message cannot show it,
   and `@source` cannot name it in a multi-member variant (R18).
4. **Display is always generated** (E2). Any computed message fits in
   `${...}` with a helper function, so a hand-written `Display` is rarely
   needed. What remains is that a type cannot have a `Display` different
   from its error message.

#### 1.3 `From` through `@from`, and `?`

The ordinary case works: `@from Yaml(error: yaml.YamlError)` gives
`impl From[yaml.YamlError] for RuleCoreError`, and `?` uses it by 05
Propagation step 2. A repeated payload type is an error naming both
variants, as decided. Three edges:

```text
@derive(Error)
pub enum AppError[E]:
    @transparent
    @from
    Inner(error: E)
    @from
    Io(error: FsError)
# generated: impl[E] From[E] for AppError[E]
#            impl[E] From[FsError] for AppError[E]
# error: overlapping-impl: the heads unify at E = FsError (09 Overlap ignores bounds)

@derive(Error)
pub enum CliError:
    @transparent
    @from
    Other(error: Error)                      # generates impl From[Error] for CliError

fn run() -> Result[void, CliError]:
    text := read_config("a.toml")?          # error: invalid-result-propagation
    .Ok()
```

- **Generic plus concrete payloads overlap** (R6). Decision 10's duplicate
  check compares payload types, so `E` and `FsError` pass it, and the user
  gets `overlapping-impl` on two generated impls that do not appear in the
  source. The diagnostic should name both variants, as the same-type check
  does, or the rule should reject the pair.
- **A catch-all `Error` payload catches nothing by `?`.** `read_config`
  fails with `FsError`. Step 1 fails (`FsError` is not assignable to
  `CliError`), and step 2 finds `From[Error]`, not `From[FsError]`. This
  follows from "`?` converts at most once" (decision 5) and is not a new
  problem, but an `anyhow`-style catch-all variant is the first thing Rust
  users write. The error message for this case should suggest
  `.map_err(fn(e: FsError) -> CliError: CliError.Other(e))`.
- **Bounds of the generated impls are unstated** (R16). For `AppError[E]`,
  the `From[E]` impl needs no bound, `Display` needs `E < Display` only
  when a message or `@transparent` uses `E`, and `Error` needs
  `E < Error` for the cause. 09's rule for `@derive` ("`T < Trait` for
  every parameter in a compared field") does not say which fields count
  for which generated impl.

#### 1.4 Cause chains, `@source`, `@transparent`, and `find`

E4 lets `@source` mark one member of a multi-member variant, and the two
texts of the record spell it differently. Decision 10's E4 writes the
marker on the member; the Current Design section writes it on the variant
with the member's name:

```text
@derive(Error)
pub enum LoadError:
    @message("$path:$line: invalid rule")
    Parse(path: string, line: i64, @source error: SyntaxError)   # hypothetical syntax
```

```text
@derive(Error)
pub enum LoadError:
    @message("$path:$line: invalid rule")
    @source(error)
    Parse(path: string, line: i64, error: SyntaxError)
    @message("cannot read $path")
    @source(error)
    Read(path: string, error: FsError?)       # an absent optional cause gives .None
    @transparent
    @from
    Rules(error: RuleCoreError)
```

The first is a `syntax-error`: `variant_parameter_clause` holds
`data_parameter`s, which take no decorator (02 Enums). The second parses,
and `error` in `@source(error)` is resolved as a payload name, not as an
expression (R4).

`@transparent` returns the inner error's cause (E3), so the inner error is
never in the chain:

```text
fn is_rule_error(e: LoadError) -> bool:
    let erased: Error = e
    erased.find[RuleCoreError]().is_some()   # false for LoadError.Rules
```

For `LoadError.Rules(inner)`, `chain(erased)` visits the `LoadError` and
then `inner.cause()`, such as a `YamlError`. `find[RuleCoreError]`
downcasts neither to `RuleCoreError`. E3 chose this so that `chain` does not
repeat the inner message, as in thiserror. In hd, though, `find` is the
documented way to test an erased error (STDLIB `std.error`), so the
wrapped type disappears from the only test users have. The workaround is
`erased.downcast[LoadError]()` and a `match` (R10).

#### 1.5 Common enum fields: a span on every variant

hd's shared enum data looks like the right tool for "every parse error has
a span", but shared data comes from each variant's `->` clause, never from
the caller (08 Shared Enum Constructor Data: a named argument that names no
payload field is `unknown-data-field`). A span per value therefore has to be
a payload that the clause copies:

```text
@derive(Error)
pub enum ParseError(span: Span):
    @message("$span: unexpected $token")
    Unexpected(token: string, at: Span) -> ParseError(at)
    @from
    Io(error: FsError) -> ParseError(Span::none())
```

- Decision 10 says "common enum fields of a `@from` variant must have
  defaults". Here `Io`'s clause initializes `span` without a default, which
  08 accepts, so it is unclear why the rule is needed or what it rejects
  (R12).
- A variant that carries its own span has two payloads, so it can never be
  `@from`. Conversions through `?` produce `Span::none()`, which silently
  drops the location.
- The idiom that works is a wrapper data type,
  `data ParseError` with a `span` field and a `@source kind: ParseErrorKind`
  field, plus a hand-written `From[FsError]` that has no span to give
  either. E4 also forbids `@source` on a common field, so the span can
  never be the cause, which is right.

#### 1.6 Generic error types

`@derive(Error)` on `enum Retry[E]: Failed(attempts: i64, last: E)` works in
the checked parts (interpolating `$last` needs `E < Display`), but the
generated headers are unstated (R16), and a generic payload next to a
concrete one overlaps (1.3, R6). Recovering a generic error from an erased
`Error` needs the exact instantiation (`find[Retry[FsError]]`), because
09 Runtime Type Identity compares type arguments exactly. That is correct,
and worth a sentence in the error-handling guide.

#### 1.7 Errors as trait values and downcast

`Error < Display + Inspectable` stays dynamically safe: `@derive(Error)`
adds no associated function, and M13 removed the configuration hook. `?`
reaches `Result[T, Error]` by assignability, and `find[T]` works for every
`@from` and `@source` payload. An enum declared in a block suite is not
inspectable, so its generated `impl Error` is
`missing-supertrait-implementation`; the diagnostic should point at
`@derive(Error)`. **Works.**

#### 1.8 Error enums that other libraries also derive

An error enum sent over an LSP connection also opts in to json. The two
mechanisms meet in one place: decision 10 does not say whether `@message`,
`@from`, and `@source` are facts that templates can read. A json encoder
that wants the message calls `to_string()` and does not need them, so this
is minor. The markers are resolved by name only under `@derive(Error)`: a
module that also imports a function named `message` or `source` (a json
or tracing library's fact function) has two readings of `@message(...)`
(R17).

**Verdict for case 1: works for the reference case and for the Current
Design's `LoadError`; works with friction for message scope (R7), `?` among
shared payload types (R11), and transparent payloads in `find` (R10);
breaks for generic-plus-concrete conversions (R6) and for spans per value
with `@from` (R12).**

### 2. The Fact Check Hook

M15 accepts a compile-time `check` on a fact type, run at the opt-in site,
for other libraries' facts. The form is open. This case assumes the least a
useful check needs: the fact's value and a description of its target.

```text
# Assumed form, for this report only: FactTarget describes the member or
# variant the fact is attached to (its name, its type as a TypeId, and
# whether it has a declared default).
pub data Range:
    min: i64
    max: i64

impl Range:
    pub fn check(self, target: FactTarget) -> List[string]:
        let problems: mut List[string] = []
        if self.min > self.max:
            problems.append("range: min is greater than max")
        if !(target.member_type == TypeId::of[i64]() || target.member_type == TypeId::of[i32]()):
            problems.append("range applies to i32 and i64 members")
        problems
```

```text
use dep.validate
use dep.proto

type Age(i32)

@validate.validate()
pub data Signup:
    @validate.range(13, 130)
    pub age: Age                  # rejected: TypeId of Age is neither i32 nor i64
    @validate.range(1, 5)
    pub nickname: string          # rejected, as intended
    @validate.range(5, 1)
    pub level: i64                # rejected: min > max (a value check works)

pub data Person:                  # no @proto opt-in: nothing below is checked
    @proto.tag(1)
    pub id: i64
    @proto.tag(1)
    pub name: string              # a duplicate tag: each Tag sees only its own member
```

1. **Value checks work.** `range(5, 1)`, a malformed regex in
   `validate.pattern`, and a negative protobuf tag are caught at the fact.
   Round 1's P14 examples of that kind are fixed.
2. **Type checks are brittle.** hd has no type predicate beyond `TypeId`
   equality (09: "no trait tests"). A check can list exact types but cannot
   say "any integer", "any newtype over an integer", or "any type with
   `validate.Numeric`". Chapter 14's typed metadata (`FieldMetadata[T]`)
   answered this with an impl per accepted type; the hook does not (R8).
3. **Cross-member checks have no home.** Duplicate protobuf tags, two json
   renames to one key, a reserved tag number used by a member, and a
   `@wire.since(2)` member without a default all need every member's facts
   at once. A type-level fact's check could see them if its target were the
   whole type; M15 says "its member or variant" (R8).
4. **Unused facts are never checked.** "Run at the opt-in site" means a
   fact on a type that opts in to nothing (`Person` above) or only to other
   libraries is silently ignored. A misspelled or forgotten opt-in is the
   common mistake (R8).
5. **Where the check's body lives.** Like annotation functions, a check
   runs in the user's compile, so package interfaces must carry its body
   (P19).

**Verdict: works for single-fact value checks; breaks for type and
cross-member checks.**

### 3. Two-Value Traversals: `Eq`, `Ord`, `Hash`

The reference example's `EqWalker` holds two values and enters a variant
only when both hold it. `Ord` needs more: variants compare by declaration
order, members lexicographically, and the first unequal member should end
the walk. The error channel gives a real early exit:

```text
use std.structure.{Structure, Field, Variant, Walker}

pub trait PartialOrd < Eq:
    fn partial_cmp(self, other: Self) -> Ordering?

pub trait Ord < PartialOrd:
    fn cmp(self, other: Self) -> Ordering

impl[T] Ord for T by Structure:
    fn cmp(self, other: Self) -> Ordering:
        let w: mut OrdWalker[T] = OrdWalker { a: self, b: other, first: .None }
        match T::walk(w):
            .Err(order) => order                   # the first unequal member
            .Ok() => match w.first:
                .Some(order) => order              # different variants
                .None => Ordering.Equal

data OrdWalker[S]:
    a: S
    b: S
    first: Ordering?

impl[S] Walker[S] for OrdWalker[S]:
    type Error = Ordering                          # early exit, typed

    fn variant(mut self, v: Variant[S]) -> bool:
        in_a := v.holds(self.a)
        in_b := v.holds(self.b)
        if self.first.is_none() && in_a != in_b:   # the first held variant decides
            if in_a:
                self.first = .Some(Ordering.Less)
            else:
                self.first = .Some(Ordering.Greater)
        in_a && in_b

    fn member[F < Ord](mut self, h: Field[S, F]) -> Result[void, Ordering]:
        match h.get(self.a).cmp(h.get(self.b)):
            .Equal => .Ok()
            order => .Err(order)

impl[T] Hash for T by Structure:
    fn hash(self, state: mut Hasher) -> void:
        let w: mut HashWalker[T] = HashWalker { value: self, state: state }
        _ := T::walk(w)

data HashWalker[S]:
    value: S
    state: mut Hasher

impl[S] Walker[S] for HashWalker[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> bool:
        if !v.holds(self.value):
            return false
        v.info.index.hash(self.state)
        true

    fn member[F < Hash](mut self, h: Field[S, F]) -> Result[void, never]:
        h.get(self.value).hash(self.state)
        .Ok()
```

**What works.** All three are ordinary library code with typed members and
no boxing, which round 1 could not write at all. `PartialOrd` is the same
walker with `type Error = Ordering?`, so an unordered float member ends the
walk with `.None`.

**What does not.**

1. **Every walker rebuilds the variant structure.** `Eq` needs two flags to
   tell "same variant, all members equal" from "different variants"
   (reference example). `Ord` needs the first variant held by either
   value. `Hash` needs to answer `false` for every variant it does not
   hold. The generated walk knows which variant a value holds, but it
   exposes that only as a yes/no question per variant, asked in order
   (R2).
2. **Variant choice is linear.** Generated `walk` asks `w.variant(v)` for
   every variant. `Eq` on a 60-variant token enum makes up to 120 `holds`
   calls before it compares any payload, where today's derived equality
   compares two tags. Different variants cannot end the walk early: `variant`
   returns `bool`, with no error channel (R2).
3. **Law partners drift across blocks.** Member lines are local to their
   block (M10), and `= pass` is one:

```text
data Session:
    user: string
    token: string
    cache: Cache = Cache::empty()

impl Eq for Session by Structure:
    cache = pass                   # hypothetical syntax; equality ignores the cache

impl Hash for Session by Structure # no `cache = pass`: equal sessions can hash differently
```

   If `Cache` implements `Hash`, this compiles, and a `Map[Session, V]`
   loses entries. TQ-12's partner rule ("derived in the same list") has no
   meaning when each trait is its own block, and whether `= pass` removes
   `cache` from `walk` at all is still open (R3).
4. Tier 1 cannot select a subset of `std.cmp`'s templates (P13), so
   `impl Eq for T by Structure`, `impl Hash ...`, and `impl Ord ...` are
   three lines where `@derive(Eq, Hash, Ord)` was one.

**Verdict: works with friction** (R2, R3, P13).

### 4. Value-To-Value: Clone, Diff And Patch, Shrinking

**Clone** is in the reference example: a source that holds the old value
and answers each `member` with `h.get(old).clone()`. It works for data
types and enums. Three members break it:

```text
@json.json()
pub data Counter:
    pub name: string
    pub hits: mut Cell

impl Clone for Counter by Structure
# The handle for `hits` has one member type F. build needs F = mut Cell to
# construct a Counter; get(s: Counter) on a readonly s yields only Cell
# (04 Mutable Paths). Clone::clone also returns a readonly Self (P17).

pub enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    Retry -> HttpStatus(503, phrase="Service Unavailable")

impl Clone for HttpStatus by Structure
# build cannot set shared fields: each variant's `->` clause computes them.
# After `status.phrase = "Busy"` through a mut root, clone() returns
# phrase "Service Unavailable", and a derived Eq that walks shared fields
# reports the clone unequal to its original.
```

- A `mut U` member cannot be both read and constructed through one
  `Field[S, F]` (R1). Round 1's P17 was a convenience issue; with handles it
  decides whether the handle type is well formed.
- Shared enum fields: whether `walk` visits them, and where, is still
  unspecified (P11), and `build` cannot honor a reassigned value.
- An embedded part is copied by generated `build` (`Timestamps: ...value`),
  which works if the part is one member (case 6).

**Diff and patch.** Diff is a two-value walker, patch a source over the old
value. Both work untyped:

```text
use dep.json

pub trait Patch:
    fn patch(self, path: string, changes: Map[string, json.Value]) -> Result[Self, json.DecodeError]

data PatchSource[S]:
    old: S
    path: string
    changes: Map[string, json.Value]

impl[S] Source[S] for PatchSource[S]:
    type Error = json.DecodeError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], json.DecodeError]:
        for v in choices:
            if v.holds(self.old):
                return .Ok(v)
        panic("a value holds exactly one variant")

    fn member[F < Patch](mut self, h: Field[S, F]) -> Result[F, json.DecodeError]:
        h.get(self.old).patch(self.path + "." + h.info.name, self.changes)

impl[T < json.Decode] Patch for T by Structure:
    fn patch(self, path: string, changes: Map[string, json.Value]) -> Result[Self, json.DecodeError]:
        match changes.get(path):
            .Some(whole) => json.from_value(whole)       # replaced, including a variant change
            .None =>
                let s: mut PatchSource[T] = PatchSource { old: self, path: path, changes: changes }
                T::build(s)
```

The patch is a map from paths to json values, not a typed `UserPatch`
(P10). A variant change replaces the whole value, because `build` cannot
take members of the new variant from the old value. The template needs
`T < json.Decode` in its header, which the M12 bound rule does not produce
(R14). **Works with friction.**

**One-step shrinking.** `shrink(self) -> List[Self]` wants every value that
differs from `self` in one member, shrunk by that member's own `shrink`.
A source can replace one member, so the template builds once per candidate:

```text
impl[T] Shrink for T by Structure:
    fn shrink(self) -> List[Self]:
        let counter: mut CountWalker[T] = CountWalker { value: self, counts: [] }
        _ := T::walk(counter)
        let out: mut List[T] = []
        for position, count in counter.counts:
            let pick: mut i64 = 0
            while pick < count:
                let s: mut ShrinkSource[T] = ShrinkSource { old: self, target: position, pick: pick, seen: 0 }
                match T::build(s):
                    .Ok(smaller) => out.append(smaller)
                    .Err(e) => e
                pick = pick + 1
        out

# (CountWalker[S] records (position, candidate count) for each member of the
# held variant; ShrinkSource[S] holds old, target, pick, and seen.)
impl[S] Source[S] for ShrinkSource[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        for v in choices:
            if v.holds(self.old):
                return .Ok(v)
        panic("a value holds exactly one variant")

    fn member[F < Shrink](mut self, h: Field[S, F]) -> Result[F, never]:
        position := self.seen
        self.seen = self.seen + 1
        if position == self.target:
            return .Ok(h.get(self.old).shrink()[self.pick])   # recomputed for every candidate
        .Ok(h.get(self.old))
```

It works, but a member with `k` candidates recomputes its `shrink` list
`k` times, because nothing can hold a `List[F]` for a member between two
`build` calls without erasing it (P7). Shrinking toward a simpler variant
needs members of that variant from nowhere: a generator or `Default` bound
on every member. **Works with friction.**

**Verdict:** clone works except for `mut` members (R1) and shared fields
(P11); diff, patch, and shrinking work with friction (P10, R14, P7).

### 5. Value-Free: Schemas, CLI Help, DDL

The reference example's `json.Schema` is a walker without a value. The
same shape serves MCP tool inputs, OpenAPI components, CLI help, and DDL.

```text
use dep.json

@json.json()
pub data SearchArgs:
    ## Words to search for
    query: string
    limit: i64 = 20                        # schema: "default": 20; decode: 20 when absent
    filter: Filter? = .None

@json.json(tag="op")
pub enum Filter:
    Term(field: string, value: string)
    All(filters: List[Filter])             # recursive: a $defs reference
```

`json.schema_of[SearchArgs]()` reaches `Filter` through `F::schema`, and
`Filter` reaches itself through `List[Filter]`'s hand-written impl and
`defs.reserve`. Declared defaults appear in the schema, and the decoder
uses them. Round 1's P2 and P3 are fixed for schemas.

Four gaps:

1. **No type name.** The `$defs` key comes from
   `TypeId::of[T]().to_string()` under `T < Inspectable`, which spells the
   absolute qualified name (`acme.search.Filter`). Moving a type between
   modules renames its public schema component. Debug printing needs a
   short name too (R9).
2. **No docs.** `## Words to search for` should become the schema's
   `description` and the CLI help line. `Member` in the example has no
   doc, and the handle API is open (R9).
3. **`variant` cannot fail.** A DDL walker that meets an enum should report
   "an enum is not a row". `variant` returns `bool`, so the walker records
   a flag and the template returns the error after the walk (R2):

```text
data DdlWalker[S]:
    columns: mut List[string]
    saw_enum: bool

impl[S] Walker[S] for DdlWalker[S]:
    type Error = DbError

    fn variant(mut self, v: Variant[S]) -> bool:
        self.saw_enum = true                       # no error channel here
        false

    fn member[F < ToSql](mut self, h: Field[S, F]) -> Result[void, DbError]:
        column := column_name(h.info) + " " + F::sql_type()
        match h.default():
            .Some(d) => self.columns.append(column + " DEFAULT " + F::sql_literal(d))
            .None => self.columns.append(column + " NOT NULL")
        .Ok()
```

4. **Function targets** (MCP tools declared as functions) still wait for
   FN_TYPE; the argument-record workaround above now works end to end.

CLI help follows the same pattern: `F::metavar()` per member and
`[default: ...]` from `h.default()` rendered through the value trait. It
works apart from the missing docs.

**Verdict: works with friction** (R9, R2).

### 6. Handles Under Pressure

This case collects member-model questions and the new questions handles
raise.

**Enum payload `get` panics.** Correctness of every enum walker rests on its
own `variant` answer. A walker that answers `true` for a variant its value
does not hold compiles, and the first `h.get` panics. The failure is at run
time, in library code, for some values only (R2).

**Handles escape.** A handle is an ordinary value. A walker can capture it:

```text
data Readers[S]:
    readers: mut List[fn(S) -> string]

impl[S] Walker[S] for Readers[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> bool:
        true

    fn member[F < Display](mut self, h: Field[S, F]) -> Result[void, never]:
        self.readers.append(fn(s: S) -> string: h.get(s).to_string())   # outlives the walk
        .Ok()
```

The closures read every member, private ones included (M5 includes private
members), of any `S` value, forever, wherever the library passes them. A
library can also keep them in module storage (10 Module Initialization
allows top-level bindings). Round 1's `visit` exposed each private value for
one call. A handle is a read capability that outlives the call, and a
source can already construct values that bypass constructor invariants
(R5). The same closures are how a library could cache a per-type plan (P7),
so restricting escape has a cost.

**Name collisions.** Inside a template, `T` has `Structure`'s `facts`,
`walk`, and `build` next to the trait's own methods:

```text
pub trait Builder:
    fn build() -> Self

impl[T] Builder for T by Structure:
    fn build() -> Self:
        let s: mut DefaultSource[T] = DefaultSource {}
        match T::build(s):                 # Structure::build or Builder::build?
            .Ok(value) => value
            .Err(e) => e
```

Both are receiverless, so 09's `Trait::method(receiver, ...)` form cannot
choose one (R13). A trait with a method named `facts` or `walk` has the
same problem. Round 1's P5 had this for hooks; M13 removed the hooks, and
M14 brought the names back as `Structure`'s own members.

**The bound rule names the trait.** M12 gives a derived `Tree[T]` the bound
`T < Trait`. The obligation comes from the walker's `member` bound, which
may be stronger. The reference example had to declare `Schema < Encode` so
that `T < Schema` covers the schema walker's use of `default_node`. `Patch`
in case 4 needs `T < json.Decode` in the template header, which no derived
header contains (R14).

**`= pass` members** (open in M14). Three readings meet different needs:
absent from `walk` and filled from the default in `build` (DDL, `Eq`
ignoring a cache); present in `walk` with no obligation (Debug printing a
redacted label); or present in `build` only (decode with a default, but not
encode). The first is what M3 says. Payload members have no defaults
(08: "Variant payload parameters do not have defaults"), so `= pass` on a
payload member is an error in any block whose template builds, and legal in
a block whose template only walks (R3).

**Embedded parts** (08 Data Embedding). As one member named `Timestamps`,
json writes `{"Timestamps": {...}}`, and generated `build` fills it with a
copy (`Timestamps: ...value`). Flattened, each part member gets a handle
that projects through the part, but generated `build` must construct the
part from its members, which may be private to the part's module. Neither
reading is chosen (P11).

**Newtypes.** `type Mile(i32)` gets no `Structure`, and a decorator before
`type` is a `syntax-error` (unchanged from round 1; TQ-11 is not applied)
(P11).

**GADTs.** `walk` works for `enum Expr[T]` with refined variants (a handle's
`get` is typed by the variant's payload). `build` at an arbitrary `Expr[T]`
cannot construct `IntLit(value: i64) -> Expr[i64]`, so `Source::variant`'s
`choices` would have to depend on `T`, and a variant-local generic
parameter would appear in a handle type `Field[Expr[T], A]` with `A` in no
scope (P11).

**Generic types and recursion.** M12's rule and coinductive recursion work
with handles unchanged (`Tree[T]` in the reference example). `Set[T]`
members still need a tier-2 header (P6 status: fixed with a known gap).

**Verdict: mixed.** Handles fix nothing in the member model and add escape
(R5), collisions (R13), and the `mut` member problem (R1).

### 7. Performance Under Per-Shape Compilation

hd compiles a generic function once per shape and passes bounds as
dictionaries (04 Shapes and Generic Code). Per member, one generated `walk`
call now costs:

| Step | Round 1 (`visit`) | M14 (`walk`) |
| --- | --- | --- |
| Call into the walker's `member[F]` (compiled once per shape of `F`) | 1 | 1 |
| Read the member | inline in generated code | `h.get(s)`: an offset load or an indirect call, unstated |
| Call through `F`'s dictionary (`encode`, `eq`) | 1 | 1 |
| Allocation | none | none |

Per enum value, M14 adds a scan: `walk` asks `variant(v)` once per variant,
and each answer usually calls `holds` (twice for `Eq` and `Ord`). A tag
switch becomes `O(variants)` indirect calls (R2). Per decode, a missing key
calls `h.default()`, which evaluates the declared default and allocates a
`.Some` for every member type (R15: M14 claims "reference-shaped members"
only, but 04 Composite Representation gives every `.Some` its own identity).

Per-call recomputation (P7) is unchanged: `key_for` scans facts and
converts case per member per call, and `choices` lists are built per call
unless constants. A library can now build a per-type plan of closures over
handles and keep it in module storage keyed by `TypeId`, but that needs
`T < Inspectable`, a downcast per call, and mutable module state.

**Verdict:** no per-member allocation holds; enum cost grows with variant
count (R2), and the handle's projection cost is unstated (P19).

### 8. Validation With Accumulated Errors

```text
data Collector[S]:
    value: S
    path: string
    issues: mut List[Issue]

impl[S] Walker[S] for Collector[S]:
    type Error = never                     # never stops: every issue is collected

    fn variant(mut self, v: Variant[S]) -> bool:
        v.holds(self.value)

    fn member[F < Validate](mut self, h: Field[S, F]) -> Result[void, never]:
        h.get(self.value).check(h.info.facts, self.path + "." + h.info.name, self.issues)
        .Ok()
```

Accumulation works, nested paths come from member names, and with the fact
check hook `@validate.range(1, 5)` on a `string` is now a compile error
(case 2), as long as the check can recognize the member type. Two problems
from round 1 remain:

- **Cross-member rules** (`ends > starts`) need the structural checks plus
  one more. A tier-2 block may override `check`, but the override replaces
  the template body and cannot call it (P16).
- **Compiled patterns.** The fact check can reject a malformed regex at
  compile time, but the fact still holds the pattern text, and `check`
  compiles it on every call (P14-b, P7).

**Verdict: works with friction** (P16, P7).

### 9. Builders

A typed builder (`Request::builder().url("x").build()`) needs per-member
methods and a companion type, which derivation cannot declare (P10). The
dynamic builder from round 1 now honors declared defaults:

```text
use std.inspect.{Inspectable, downcast_val}

impl[S] Source[S] for BuilderSource[S]:
    type Error = BuildError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], BuildError]:
        .Err(BuildError.Missing("variant"))

    fn member[F < Inspectable](mut self, h: Field[S, F]) -> Result[F, BuildError]:
        match self.values.get(h.info.name):
            .Some(value) => match downcast_val[F](value):
                .Some(typed) => .Ok(typed)
                .None => .Err(BuildError.WrongType(h.info.name))
            .None => match h.default():
                .Some(fallback) => .Ok(fallback)   # P3 fixed
                .None => .Err(BuildError.Missing(h.info.name))
```

Member names are still strings and every value is boxed. hd's data literals
with defaults and copy-update remain the typed answer inside the owning
module.

**Verdict: breaks** for typed builders (P10).

### 10. Database Rows With Suspending I/O

Row encoding and decoding are the json pattern with db's facts and work. A
member loaded lazily (`tags: List[Tag]` from a join table) needs `$ Db`
and suspension inside `Source::member`, which has the empty row (P8). M14
adds a workaround that is often better anyway: a value-free walk plans the
relations, the I/O runs outside `build`, and `build` reads prefetched rows.

```text
data RelationPlanner[S]:
    queries: mut List[(string, Query)]

impl[S] Walker[S] for RelationPlanner[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> bool:
        false

    fn member[F < Column](mut self, h: Field[S, F]) -> Result[void, never]:
        match F::relation():                       # value-free: from F's own impl
            .Some(query) => self.queries.append((h.info.name, query))
            .None => pass
        .Ok()

pub fn load![T < Record](id: i64) -> Result[T?, DbError] $ Db:
    match $.use(Db).fetch_one!(T::table_name(), id)?:
        .None => .Ok(.None)
        .Some(row) =>
            related := $.use(Db).fetch_related!(T::relations(), id)?   # one batch, before build
            .Ok(.Some(T::from_row(row, related)?))
```

`T::relations()` is a `Record` method whose template runs the planner. The
batch avoids one query per row per relation. True per-member suspension
(streaming a large member from an async reader) stays impossible.

**Verdict:** rows work; per-member I/O **breaks** (P8), with a prefetch
workaround.

### 11. Versioned Binary Codecs

A member added in version 2 carries `@wire.since(2)` and must have a
declared default, so version-1 input still decodes:

```text
impl[S] Source[S] for WireReader[S]:
    type Error = WireError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], WireError]:
        index := self.input.read_varint()?
        if index >= choices.length():
            return .Err(WireError.UnknownVariant(index))
        .Ok(choices[index])

    fn member[F < Wire](mut self, h: Field[S, F]) -> Result[F, WireError]:
        if since_of(h.info) > self.version:
            match h.default():
                .Some(value) => return .Ok(value)
                .None => return .Err(WireError.NoDefault(h.info.name))
        F::read(self.input)
```

- Reading old data works because `h.default()` exists (P3 fixed).
- "A `since` member must have a default" is a check of one fact against
  its member, if the hook's target reports `has_default` (case 2 assumed
  it does). Duplicate tags and reserved numbers are cross-member (R8).
- Variants are written by index. Adding a variant in the middle renumbers
  the rest with no diagnostic (P20). A variant tag fact avoids it by
  convention only.

**Verdict: works with friction** (R8, P20).

### 12. Several Libraries On One Type

```text
use dep.json
use dep.db
use dep.validate
use dep.cli

@json.json(case=.Camel)
@db.table("users")
@validate.validate()
@cli.command("user")
pub data User:
    @db.primary_key()
    pub id: i64
    @json.rename("mail")
    @validate.pattern(".+@.+")
    @cli.short("e")
    pub email: string
    pub session: Session = Session::none()     # no json, db, validate, cli, Eq, or Hash impl
```

Facts still never collide. The skipped member costs more than in round 1,
because json now has three templates and `std.cmp` is template-based:

| Trait | Needs its own tier-2 block with `session = pass` |
| --- | --- |
| `json.Encode`, `json.Decode`, `json.Schema` | 3 blocks, each repeating `Self += [json.json(case=.Camel)]` |
| `db.Record` | 1 block, repeating `Self += [db.table("users")]` |
| `validate.Validate`, `cli.Command` | 2 blocks |
| `Eq`, `Hash`, when `User` derives them | 2 blocks, which must agree (R3) |

Up to eight blocks for one member, where round 1 counted four. Each block must
also drop the matching annotation, because tier 1 plus tier 2 for one trait
is `overlapping-impl` (rule 7). A library whose trait declares a method
named `build`, `walk`, or `facts` cannot be derived at all (R13).

**Verdict: works with friction; worse than round 1** (P13, R3, R13).

## Round-1 Problems Under M14

| Id | Round-1 problem | Status under M14 and M15 | Where |
| --- | --- | --- | --- |
| P1 | No two-value or value-to-value traversal | **Fixed** for `Eq`, `Ord`, `Hash`, diff, clone, patch, and shrinking, with friction. **Worse** for `mut` members (R1). `T -> U` mapping still open. | Cases 3, 4 |
| P2 | No value-free typed traversal | **Fixed** (`walk` without a value). Type name, docs, and enum-ness missing (R9). | Case 5 |
| P3 | Declared defaults unreachable | **Fixed** (`has_default`, `default`). `default()` allocates for every type (R15). | Cases 5, 9, 11 |
| P4 | Marker templates check nothing | **Fixed** by M12 (no marker templates). | — |
| P5 | Style hook is an associated function | **Fixed** by M13 (type-level facts). The name collision returned as Structure's own members (R13). | Case 6 |
| P6 | No bound rule for generic targets | **Fixed** by M12, with a gap: the rule names the trait, not the walker's bound (R14). | Cases 4, 6 |
| P7 | Declaration-order decode, per-call recomputation | **Remains.** Plans over handles are possible but need module storage and `Inspectable`. Enum walks add a linear scan (R2). | Cases 4, 7 |
| P8 | No requirement row or suspension in traversal | **Remains.** Value-free planning plus prefetch is a workaround. | Case 10 |
| P9 | A member is fully visited or absent | **Remains** for libraries. For errors, `@message` interpolation and `@source` replace the need (M15). | Cases 1, 6 |
| P10 | Derivation cannot declare types or methods | **Remains** (typed builders, typed patches). | Cases 4, 9 |
| P11 | Member model unspecified | **Remains**, and handles add questions: `F` for `mut` members, payload handle naming, shared fields in `walk`, embedded parts, newtypes, GADTs. | Cases 4, 6 |
| P12 | No impl family indexed by member types | **Closed by M15** (out of scope; `@derive(Error)` intrinsic). The intrinsic has its own problems (R4, R6, R7, R10-R12, R16-R18, R20). | Case 1 |
| P13 | Tier-1 meaning depends on package contents | **Remains open**, and **worse**: more templates per library, so one skipped member now means up to eight blocks. | Cases 3, 12 |
| P14 | Facts untyped against members, checked at run time | **Partly fixed** by the fact check hook (M15): value checks work. Type and cross-member checks do not (R8). | Case 2 |
| P15 | Foreign types cannot be derived | **Remains.** | — |
| P16 | Templates cannot be extended or composed | **Remains.** Also one template per trait program-wide (R19). | Case 8 |
| P17 | `build` returns readonly `Self` | **Remains, worse**: a `mut` member's handle type is ill-formed (R1). | Case 4 |
| P18 | Reference example does not parse | **Fixed**: the M14 example parses except its marked member lines. Q18-b (`by Structure` versus delegation) remains. | Parse log |
| P19 | Compilation and interface model not stated | **Remains.** Adds handle projection cost and fact-check bodies. | Cases 2, 7 |
| P20 | Wire stability silent | **Remains.** Schema names now derive from qualified type names (R9). | Cases 5, 11 |

## Problems, Ranked

New problems have ids R1-R20; round-1 problems that remain keep their P
ids. Each entry states the effect and one to three candidate fixes, with
trade-offs written as questions for the owner. None of the fixes is a
recommendation, and none reopens an alternative the record already
rejected (impl-family templates, delegation to a function, trait-kinded
parameters, associated type packs, trait values per member, the wrapper, or
the configuration hook).

| Rank | Id | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | R1 | A `mut` member's handle has no single member type | Critical | 4, 6 |
| 2 | R2 | The enum protocol: linear `holds` scan, per-walker bookkeeping, panicking `get`, no error from `variant` | High | 3, 5, 6, 7 |
| 3 | R3 | `= pass` in `walk` and `build` unspecified; law partners drift across blocks | High | 3, 6, 12 |
| 4 | R5 | Handles escape the walk | High | 6 |
| 5 | P13 | Tier-1 template selection still open, now with more templates per library | High | 3, 12 |
| 6 | R6 | A generic `@from` payload overlaps a concrete one | Medium | 1 |
| 7 | R12 | `@from`'s common-field rule contradicts 08; spans per value cannot use `@from` | Medium | 1 |
| 8 | R8 | The fact check hook sees one fact, exact types, and only opted-in types | Medium | 2, 8, 11 |
| 9 | R13 | `Structure`'s `facts`, `walk`, `build` collide with trait methods | Medium | 6, 12 |
| 10 | R11 | `?` silently picks the `@from` variant among variants sharing a payload type | Medium | 1 |
| 11 | R9 | No type-level information beyond `facts()` | Medium | 5 |
| 12 | R14 | The M12 bound names the trait, not the walker's bound | Medium | 4, 6 |
| 13 | R10 | `@transparent` hides the payload from `find` | Medium | 1 |
| 14 | R16 | Bounds of the impls `@derive(Error)` generates are unstated | Medium | 1 |
| 15 | P7 | Declaration-order decode and per-call recomputation | Medium | 4, 7 |
| 16 | P8 | No requirement row or suspension in traversal | Medium | 10 |
| 17 | P11 | Member model: shared fields, embedding, newtypes, GADTs, payload lines | Medium | 4, 6 |
| 18 | R7 | Message scope: a new scope rule, and `$self` recursion | Low | 1 |
| 19 | R4 | Two spellings of a member `@source`; E4's is not in the grammar | Low | 1 |
| 20 | R17 | Error markers resolve by bare name | Low | 1 |
| 21 | R20 | Opaque public errors: `@transparent` is variant-only | Low | 1 |
| 22 | R18 | Unnamed payloads cannot be named in messages or `@source` | Low | 1 |
| 23 | R19 | One template per trait, program-wide | Low | 8 |
| 24 | R15 | `default()` allocates for every member type | Low | 7 |
| 25 | P10 | Derivation cannot declare types or methods | Low | 4, 9 |
| 26 | P16 | Templates cannot be extended or composed | Low | 8 |
| 27 | P15 | Foreign types cannot be derived | Low | — |
| 28 | P19 | Compilation and interface model not stated | Low | 2, 7 |
| 29 | P20 | Wire stability is silent | Low | 5, 11 |
| 30 | P1 | `T -> U` mapping between two types (residue) | Low | — |

### R1. A `mut` Member's Handle Has No Single Member Type

**Effect.** For `hits: mut Cell`, `build` must receive a `mut Cell` from
`Source::member` to construct the value, and `get(s: S)` on a readonly `s`
can only yield `Cell` (04 Mutable Paths). `Field[S, F]` has one `F`, so
either `get` upgrades access or `build` cannot construct the value. Every
derived clone, decode, default, and generator of a type with a `mut` member
is affected.

**Candidate fixes.**

- **A. Two member types per handle.** `get` yields the readonly view, and
  `Source::member` produces the declared type. *Q:* is a handle such as
  `Field[S, R, F]` (read type, build type) acceptable, given that most
  members have `R = F`?
- **B. `get` needs mutable access for `mut` members.** A `get_mut(s: mut S)`
  exists only when the member is `mut`. *Q:* may walkers that hold readonly
  values then not read `mut` members at their declared type at all?
- **C. `mut` members are not derivable.** Walking or building one is an
  error at the opt-in, unless the block says `= pass`. *Q:* is it
  acceptable that such types never derive clone or decode?

Round 1's P17 (`build` returns readonly `Self`) is the same family:
*Q:* should `build` return `mut Self` together with whichever fix is chosen?

### R2. The Enum Protocol

**Effect.** Generated `walk` asks `variant(v)` about every variant in order,
and a walker answers with `holds`. So: every two-value walker rebuilds
"same variant" and "which variant first" from yes/no answers; enum walks
cost one or two indirect `holds` calls per variant; a wrong answer makes
`h.get` panic at run time; `variant` cannot report an error (DDL, a
row-only format); and nothing tells a walker that a variant's members have
ended (externally tagged encodings close their object lazily).

**Candidate fixes.**

- **A. A generated "which variant" operation.** `Structure` gains
  `variant_of(s: Self) -> Variant[Self]`, and `walk` for a one-value
  walker enters exactly that variant. *Q:* is a value-aware entry point
  acceptable next to the value-free `walk`, given that two-value walkers
  then compare two `variant_of` results directly?
- **B. A richer `variant` callback.** `variant` returns
  `Result[bool, Self::Error]`, and `walk` calls an `end_variant` after the
  entered variant's members. *Q:* are two protocol changes worth fixing DDL
  and tagged encodings, when the linear scan stays?
- **C. Keep M14 and ship helpers.** `std.structure` provides
  `SameVariant[S]` and `HeldVariant[S]` walkers that libraries compose by
  hand. *Q:* is `O(variants)` per enum walk acceptable for the comparison
  traits that TQ-13 wants on this protocol?

### R3. `= pass` And Law Partners Across Blocks

**Effect.** M14 leaves open how `= pass` members appear in `walk` and
`build`. Whatever it means, it is a member line, local to its block (M10).
`Eq` and `Hash` for one type are two blocks, so a line in one and not the
other compiles and breaks the map-key law silently. TQ-12's partner rule
("derived in the same list") has no list to check.

**Candidate fixes.**

- **A. M3's reading plus a partner check.** `= pass` means absent from
  `walk` and default-filled in `build`, and the compiler rejects derived
  `Eq` and `Hash` (and `PartialOrd`, `Ord`) blocks for one type whose
  member lines differ. *Q:* is a rule specific to `std.cmp`'s law partners
  acceptable?
- **B. One block for partner traits.** `impl Eq + Hash for Session by
  Structure:` shares its lines. *Q:* does that reopen M10 and M11's
  one-trait-per-block locality, or is it limited to traits declared as law
  partners?
- **C. A third meaning for walks.** `= pass` in a walk-only template means
  "present, no obligation" (a redacted label in Debug), and each walker
  decides what to do. *Q:* is per-template meaning acceptable for a
  compiler-interpreted line?

### R5. Handles Escape The Walk

**Effect.** A walker can capture a handle in a closure and store it, so a
library holds a read capability for every member, private ones included, of
every value of the type, after the walk returns. Round 1's `visit` exposed
each value for one call.

**Candidate fixes.**

- **A. Accept it.** Opting in is consent (M5), and the capability is what
  makes per-type plans possible (P7). *Q:* is that the documented contract
  for private members?
- **B. Handles do not escape.** A handle may not be captured or stored.
  *Q:* this needs the non-escaping direction the owner has parked
  (TQ-24 to TQ-26); should derivation wait for it, or take a narrow
  special case?
- **C. Private members get no `get` outside a walk.** A handle checks at
  run time that its walk is active. *Q:* is a per-`get` check acceptable?

### P13. Tier-1 Template Selection (Remaining, Worse)

**Effect.** Still open. json now has three templates and `std.cmp` four, so
one member without an impl costs up to eight tier-2 blocks in case 12, each
repeating configuration.

**Candidate fixes** (round 1's, restated). **A.** Tier 1 names a declared
group. **B.** Tier 1 supplies configuration and a tier-2 block adds member
lines instead of overlapping. **C.** Keep rule 5 with tooling. *Q:* which,
now that `Eq`, `Hash`, and `Ord` would each need a block per type?

### R6. A Generic `@from` Payload Overlaps A Concrete One

**Effect.** `@from Inner(error: E)` and `@from Io(error: FsError)` in
`AppError[E]` generate impls whose heads unify at `E = FsError`, so the
user gets `overlapping-impl` on code they did not write. A hand-written
pair overlaps the same way (09 Overlap ignores bounds), so the combination
is not expressible in hd at all.

**Candidate fixes.**

- **A. Extend the duplicate check.** A generic `@from` payload together with
  any other `@from` in the same enum is an error naming both variants.
  *Q:* is the better diagnostic enough, given that no spelling works?
- **B. Document the limit** in the error-handling guide, with `map_err` for
  the concrete variant. *Q:* is that sufficient for generic wrapper enums?

### R12. `@from`'s Common-Field Rule Contradicts 08; Spans

**Effect.** Decision 10 requires a `@from` variant's common enum fields to
have defaults. Under 08, a variant's `->` clause always initializes shared
data, so the rule rejects `Io(error: FsError) -> ParseError(Span::none())`,
which is otherwise valid. Separately, a span that varies per value must be
a payload, so every variant that carries one has two payloads and cannot be
`@from`.

**Candidate fixes.**

- **A. Drop or restate the rule.** *Q:* what case was it meant to reject?
- **B. Document the wrapper idiom** (`data ParseError` with `span` and a
  `@source kind`). *Q:* is that the intended design for located errors?

### R8. The Fact Check Hook's Scope

**Effect.** As far as M15 states it, a check sees one fact and its member or
variant. It cannot check duplicate tags, duplicate keys, or reserved numbers
(cross-member). Member types are visible only as exact `TypeId`s, so "any
integer" or a newtype over one cannot be recognized. A fact on a type with
no opt-in, or with only other libraries' opt-ins, is never checked.

**Candidate fixes.**

- **A. A whole-type target for type-level facts.** The check of a
  type-level fact sees every member, its facts, its `TypeId`, and
  `has_default`. *Q:* how much compile-time API is acceptable?
- **B. Typed placement next to the hook.** A fact type declares the member
  types it applies to by impls, as chapter 14's `FieldMetadata[T]` did.
  *Q:* are two mechanisms for facts acceptable?
- **C. Check at the declaration.** Every fact's check runs where it is
  written, opt-in or not. *Q:* is running a library's checks in builds that
  never use its derivations acceptable?

### R13. `Structure`'s Member Names Collide

**Effect.** Inside a template, `T::build(s)` means `Structure::build` or the
trait's own receiverless `build`. 09 has no form that selects one trait's
receiverless function, so a trait with a method named `build`, `walk`, or
`facts` cannot have a template that uses `Structure`.

**Candidate fixes.**

- **A. Reserve the names.** A trait with a template may not declare
  `facts`, `walk`, or `build`, as 09 already forbids reusing a sealed
  supertrait's member names. *Q:* is constraining library API names
  acceptable?
- **B. A qualified receiverless call.** A form that names the trait for a
  receiverless function (round-1 Q5-b). *Q:* is new call syntax
  acceptable?

### R11. `?` Picks The `@from` Variant Silently

**Effect.** In `RuleCoreError`, `?` on a `RuleSerializeError` always
produces `Rule`, including in code that meant `Utils` or `Constraints`.
Forgetting `.map_err` compiles and yields the wrong message.

**Candidate fixes.**

- **A. A warning** when `?` converts through `From[P]` and `P` is also the
  payload of a `@source` variant of the same enum. *Q:* is a warning on the
  correct `Rule` sites too noisy?
- **B. No `From` for shared payload types.** A payload type that appears in
  any `@source` variant cannot be `@from`. *Q:* is losing `?` for `Rule`
  acceptable?
- **C. Keep thiserror's behavior.** *Q:* documented only?

### R9. No Type-Level Information Beyond `facts()`

**Effect.** M14 replaced `describe() -> Shape` with `facts()`. Schemas need
a type name for `$defs`, Debug printing needs a short name, help text and
schema descriptions need member docs, and nothing says whether a type is an
enum before `walk` asks. The reference example takes the name from
`TypeId` under `T < Inspectable`, which spells the absolute qualified name
and excludes block-local types.

**Candidate fixes.**

- **A. Add a small type description** to `Structure`: short name, doc,
  enum-ness, and member docs on `Member`. *Q:* is a public name that
  changes with renames acceptable for schemas (P20)?
- **B. Keep the `TypeId` workaround.** *Q:* are qualified names in public
  schemas acceptable?
- **C. Library facts.** Each library asks for its own name fact
  (`@json.name("Filter")`). *Q:* is per-library naming acceptable?

### R14. The M12 Bound Names The Trait

**Effect.** A derived impl for `Box[T]` gets `T < Trait`, while the checked
obligation is the walker's `member` bound. The two differ whenever a walker
needs more (`F < Schema + Encode`), and a template that needs more from `T`
itself (`Patch` needs `T < json.Decode`) cannot say so.

**Candidate fixes.**

- **A. Derive the header from the strengthened bound.** *Q:* is it
  acceptable that a public impl head changes when a library changes which
  walker its template uses?
- **B. Keep M12** and have libraries fold extra needs into supertraits, as
  the example did with `Schema < Encode`. *Q:* is a supertrait per
  derivation need acceptable?

### R10. `@transparent` Hides The Payload From `find`

**Effect.** E3 decided that a transparent variant's `cause()` returns the
payload's own `cause()`, as thiserror's `source()` does, so `chain` does
not repeat the inner message. The payload is then never in
`chain(error)`, so `error.find[Payload]()` returns `.None`.

**Candidate fixes.**

- **A. Keep E3 and document it** next to `find`: test a transparent
  wrapper with `downcast` on the outer error first. *Q:* is that enough?
- **B. Let `find` see the payload.** `Error` gains a defaulted
  `transparent_inner(self) -> Error?` that `find` also visits, while
  `chain` stays as E3 wants. *Q:* is a second chain for searching worth one
  more `Error` member?

### R16. Bounds Of The Generated Error Impls

**Effect.** For `enum AppError[E]`, the generated `Display`, `Error`, and
`From` impls need different bounds, and decision 10 does not state them.

**Candidate fixes.**

- **A. Per impl, the 09 rule.** `E < Display` for parameters in interpolated
  or transparent members (`Display`), `E < Error` for parameters in cause
  members (`Error`), none for `From`. *Q:* are three headers per enum
  acceptable?
- **B. One header.** `E < Error` for every parameter in any payload. *Q:* is
  over-constraining `From` acceptable?

### P7, P8, P11 (Remaining)

- **P7.** Round 1's fixes stand (input-driven build, a memoized plan, static
  tables). M14 adds a fourth: plans as closures over handles in module
  storage keyed by `TypeId`. *Q:* is order-free decoding a protocol goal,
  and does a per-(trait, type) cache come back?
- **P8.** Round 1's fixes stand (suspending variants, pure traversal,
  function-target rows). Case 10's value-free planning makes B stronger.
  *Q:* is per-member I/O in scope?
- **P11.** Round 1's questions Q11-a to Q11-g stand. Handles add: which
  variant a payload handle belongs to (`Field` has no back-reference),
  whether shared fields get handles and where `walk` reports them, and
  whether an embedded part is one handle or one per part member. *Q:* can
  these be decided with R1 and R2, since all three shape the handle API?

### R7. Message Scope

**Effect.** E1 makes a message an interpolated string with payload members
in scope. A decorator argument otherwise sees the module's scope, so this is
a scope rule of its own: names declared on the next line are visible, a
payload member shadows a module-level function of the same name inside the
message, and `$self` type-checks but calls the generated `to_string`
recursively.

**Candidate fixes.**

- **A. State the rule and reject `self`.** The message is checked as the
  body of the generated `to_string` arm, `self` is an error there, and
  shadowing follows ordinary pattern-binding rules. *Q:* is that enough for
  tooling (hover, rename) to treat the text as code?
- **B. Bind the payload explicitly.** The message names what it uses, as in
  `@message("$path:$line", path, line)`. *Q:* is the repetition worth the
  ordinary scope?

### R4. Two Spellings Of A Member `@source`

**Effect.** Decision 10's E4 writes
`Parse(path: string, line: i64, @source error: SyntaxError)`, which is a
`syntax-error`: payload members take no decorators (02 Enums). The Current
Design section writes `@source(error)` on the variant, which parses. The
record disagrees with itself, and the variant form resolves its argument
as a payload name rather than as an expression.

**Candidate fixes.**

- **A. Keep `@source(member)` on the variant** and correct E4's example.
  *Q:* is a marker argument that names a payload member, not an
  expression, acceptable?
- **B. Allow decorators on payload members,** as data fields and function
  parameters already allow. *Q:* should payload member facts then also
  reach templates' handles (a json rename of a payload member, round-1
  Q11-d)?

### R17. Error Markers Resolve By Bare Name

**Effect.** `@message`, `@from`, `@source`, and `@transparent` are
recognized under `@derive(Error)`, but decorators are otherwise resolved as
expressions. A module that imports a function named `message` or `source`
has two readings.

**Candidate fixes.**

- **A. Reserve the four names** as decorator keywords, like `derive`. *Q:* is
  taking `@source` from every library acceptable?
- **B. Qualify them** as `@error.message(...)` through `std.error`. *Q:* is
  the longer spelling acceptable?

### R20. Opaque Public Errors

**Effect.** thiserror's `#[error(transparent)] pub struct PublicError(#[from]
ErrorRepr)` hides an internal error enum behind a stable public type.
Decision 10 allows `@transparent` on a one-payload variant, and `@from` on
error enums only.

**Candidate fixes.**

- **A. Allow both on a one-field data type.** *Q:* should markers apply to
  data fields as well as variants?
- **B. A one-variant enum.** *Q:* is the extra variant name in every match
  acceptable?

### R18. Unnamed Payloads Cannot Be Named

**Effect.** `NotFound(string)` is a legal variant (08 Variant Payloads),
but nothing in a message's scope names its payload, and `@source(member)`
has no member name to use. thiserror writes `{0}` and `#[source]` on a
tuple field.

**Candidate fixes.**

- **A. Positional names in markers,** such as `$0` in messages and
  `@source(0)`. *Q:* is a positional name that exists only inside markers
  acceptable?
- **B. Require named payloads** under `@derive(Error)` whenever a message or
  `@source` refers to one. *Q:* is the diagnostic enough?

### R19. One Template Per Trait

**Effect.** Only the trait's module may declare its template (M8), so each
std trait has exactly one derived behavior program-wide. Round 1's message
templates needed a `Display` template; M15 moved errors out, but a
`key=value` log format or a redacting `Display` still cannot be derived by
a library.

**Candidate fixes.**

- **A. Accept it.** std designs one `Display` template. *Q:* which output
  should it produce for data types without facts?
- **B. Documented fact extension points** in std templates (for example,
  std's `Display` template honors a `format.Show` fact). *Q:* how many std
  facts is acceptable?

### R15. `default()` Allocates For Every Member Type

**Effect.** M14 says `.Some` allocates only for reference-shaped members,
but 04 Composite Representation gives every `.Some` its own identity.
Every decode of a missing key and every schema default allocates, and
`default()` evaluates the default expression each time (07 allows defaults
that mutate state).

**Candidate fixes.**

- **A. Correct the claim** and keep the API. *Q:* is one allocation per
  defaulted member acceptable?
- **B. A non-optional form.** `default_value() -> F`, panicking when
  `has_default()` is false. *Q:* is a panicking accessor acceptable after
  an explicit `has_default()` check?

### P10, P15, P16, P19, P20, And `T -> U` (Remaining)

- **P10.** No typed builders or patch types. *Q:* are they goals (Q10-a,
  Q10-b)?
- **P15.** Foreign types still hit `orphan-impl`. *Q:* root-application
  exception, mirror type, or newtype (Q15-a)?
- **P16.** Overrides replace template bodies; walkers cannot forward to an
  inner walker. *Q:* are extension and composition needed (Q16-a, Q16-b)?
- **P19.** Interfaces must carry template bodies, annotation functions, and
  now fact-check bodies; the cost of `h.get` (an offset load or an indirect
  call) is unstated. *Q:* should 04's implementation model state both?
- **P20.** Reordering members or variants changes positional encodings and
  derived `Ord`; schema names now follow qualified type names (R9). *Q:* a
  stability fact with a toolchain check, or documentation only (Q20-a)?
- **`T -> U` mapping** (a row type to a domain type by member name) is still
  open in M14. A source for `U`'s `build` that reads `T` would need a
  handle of `T` for each member of `U`, looked up by name at a type the
  source cannot name. *Q:* is it in scope (Q1-b)?

## Parse Log

Every `text` block above was extracted and checked with the chapter-02
reference parser (`spec/reference-parser`, `parseSource`) on 2026-09-27.
Parsing checks syntax only; names such as `Field`, `Walker`, `@message`,
and the library types are not resolved, nothing was type-checked, and the
prototype compiler implements none of this surface.

| Blocks | Result |
| --- | --- |
| Case 1: `RuleCoreError` and its use, the generated `Display`, `Error`, and `From` impls, the message examples, `AppError[E]` and `CliError`, the Current Design's `LoadError` with `@source(error)`, the `find` example, `ParseError(span: Span)` | Parse. Every error in their comments is semantic. |
| Case 1.4: E4's spelling `Parse(path: string, line: i64, @source error: SyntaxError)` | `syntax-error` at the `@source` member, as the text says (R4). Marked `# hypothetical syntax`. |
| Case 2: the assumed `Range.check` and the user side | Parse. `FactTarget` is an assumed name. |
| Cases 3-5: `Ord`, `Hash`, `Counter`, `HttpStatus`, `Patch`, `Shrink`, `SearchArgs`, `DdlWalker` | Parse. |
| Case 3: `Session` with its `Eq` and `Hash` blocks | `syntax-error` at `cache = pass`, the one M3 member line, marked `# hypothetical syntax`. With it removed, parses. |
| Cases 6-12: `Readers`, `Builder`, `Collector`, `BuilderSource`, `RelationPlanner` and `load!`, `WireReader`, the four-library `User` | Parse. |

Reference-parser findings from this round:

- A documentation comment directly before a `pub` field, as in
  `## Words to search for` followed by `pub query: string`, is reported as
  `doc-comment-without-target`. Without `pub`, or with a decorator line in
  between, it parses. 01 Lexical Structure says documentation comments
  attach to data fields, so this looks like a false positive in the
  contextual check. Case 5 writes its fields without `pub` for that reason.
- `@derive(Error)` parses as a `derive_decorator`, and the bare markers
  `@from`, `@source`, and `@transparent` parse as decorators with closed
  expressions, so decision 10 needs no grammar change except for R4.
