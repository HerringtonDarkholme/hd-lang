# One Compile-Time Intrinsic For Derivation, Delegation, And Facts?

> **Archived 2026-10-01.** Every decision in this record is applied or
> superseded, and the [specification](../../spec/README.md) is
> authoritative. The record is kept as history, so its examples and rule
> IDs may describe retired rules.

Status: design exploration, 2026-10-01. The owner answered its questions
in batch 36; [Owner Decisions](#owner-decisions) records the answers, and
spec pass 36 applied O3, O3b, and O7. The rest of this record is the
exploration as written, not accepted behavior.

The owner asked: "can you evaluate if i can use one powerful compiler
intrinsic to cover multiple areas? say comptime or macro esp.
derivation/trait delegation/field facts". The owner later added two
requests: weigh compiler performance (compile time and memory), and be
creative about new syntax, semantics, and concepts.

Under review: [Typed Derivation](../../spec/14-annotations.md#typed-derivation),
[Member Metadata](../../spec/14-annotations.md#member-metadata),
[Common Shape Representation](../../spec/14-annotations.md#common-shape-representation),
[Error Derivation](../../spec/14-annotations.md#error-derivation),
[Derived Implementations](../../spec/09-traits.md#derived-implementations),
[Derived Tuple Implementations](../../spec/09-traits.md#derived-tuple-implementations),
[Trait Delegation](../../spec/09-traits.md#trait-delegation),
[Debug Trait](../../spec/09-traits.md#debug-trait),
[Standard Combinators](../../spec/11-requirements-and-suspension.md#standard-combinators),
[Shape Descriptors](../../spec/04-type-system.md#shape-descriptors),
[Literal Suffixes](../../spec/05-expressions.md#literal-suffixes), and
[Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary). The
decisions behind them are typed derivation M1-M30, SR1, SIMPLE, ST8,
ALL-INTRINSIC, batch 31 Q6, and batch 32 Q1, logged in
[Revision Notes](../../spec/README.md#revision-notes) and
[Typed Derivation](TYPED_DERIVATION.md).

## Contents

- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Survey](#survey)
- [Options](#options)
- [Before/After](#beforeafter)
- [Rule And Code Accounting](#rule-and-code-accounting)
- [Compiler Performance](#compiler-performance)
- [Comparison](#comparison)
- [Risks](#risks)
- [Recommendation](#recommendation)
- [Owner Decisions](#owner-decisions)
- [Deferred: Code Generation Plus Compile-Time Reflection](#deferred-code-generation-plus-compile-time-reflection)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

In several hd features the compiler writes code for the user: `@derive`,
templates over `Structure`, `@error`, `by`, tuple comparison, and `all!`. Each has its own rules, and most
have their own TypeScript in the prototype. The question is whether one
general intrinsic, such as Zig's `comptime` or Rust-style macros, could
replace them.

Five use cases stay fixed for every option:

| # | Use | Today |
| --- | --- | --- |
| U1 | `@derive(Eq, Debug)` on a data type | `Eq` is a compiler derivation; `Debug` is a template |
| U2 | `impl Describe for Service by Logger` | 13 `trait.by.*` rules |
| U3 | a field fact `@json(name="mail")` read by a codec | a fact value, read through `facts.find::[F]()` |
| U4 | `a == b` and map keys for tuples of every arity | a compiler derivation, `trait.target.tuple.derived` |
| U5 | `all!(load_user(id), load_points(id))` | an intrinsic with four typing rules |

The record was asked two things the earlier derivation survey also
weighed. On 2026-09-26 the owner chose compiler-generated visitors
(Design G) over compile-time deriver functions (Design B). The survey had
called Design B "checked per target" and "an interpreter". That survey is in git
history (commit `d171c264`). This record says which of its reasons still
hold after packs were removed in batch 31.

## What hd Has Today

Rule counts are numbered `r[...]` items, taken on 2026-10-01 from
`pnpm run spec counts --by topic` and a grep of rule IDs. The whole
specification has 3,616 language-tier and 180 stdlib-tier rules.

### Rules In Scope

| Area | Rule topics | Rules | Tier | Design Cost Order kind |
| --- | --- | ---: | --- | --- |
| Typed derivation: opt-in, templates, blocks, member lines, omit, trait-less blocks, facts, members, variants, `self_ref`, derived bounds, limits | `annot.derive`, `.template`, `.block`, `.line`, `.omit`, `.traitless`, `.fact`, `.member`, `.variant`, `.self-ref`, `.bound`, `.limit` | 119 | language | 2 semantic, 3 intrinsic |
| Traversal protocol: `std.structure`, `Structure`, `name()`, generated `walk`/`describe`/`build`, handles, walkers | `annot.structure`, `.walk`, `.describe`, `.build`, `.handle`, `.walker` | 57 | language | 3 intrinsic |
| Decorators, member metadata, target kinds | `annot.decorator`, `.metadata`, `.target` | 35 | language | 2 semantic |
| Error derivation, `@error` | `annot.error` | 54 | language | 3 intrinsic |
| Intrinsic derives: `Eq`, `PartialOrd`, `Ord`, `Hash`, law partners, newtypes | `trait.derive`, `data.derive` | 52 | language | 3 intrinsic |
| `Debug`, its tuple impls, and its builders | `trait.debug`, `std-format.debug` | 26 | 8 language, 18 stdlib | 4 core library |
| `by` delegation | `trait.by` | 13 | language | 1 syntax, 2 semantic |
| Tuple `Eq`/`PartialOrd`/`Ord`/`Hash` at every arity | `trait.target.tuple.derived*` | 3 | language | 3 intrinsic |
| `all!` typing | `req.combinator.all-*` | 4 | language | 3 intrinsic |
| Derived `Arbitrary`, `arbitrary.with` | `std-testing.arbitrary.derive*`, `.with*` | 21 | stdlib | 4 core library |
| Literal-function markers `@num_suffix`, `@str_prefix` | `expr.literal-fn`, `expr.suffix`, `expr.prefix` | 14 | language | 3 intrinsic |
| `shape[T]()`, `shape_of(f)` | `types.shape` (the shape types in chapter 14 are unnumbered) | 5 | language | 3 intrinsic |
| Compiler-supplied `Inspectable` | `trait.inspect`, `trait.inspectable` | 25 | language | 3 intrinsic |
| Grammar for decorators, `by`, member lines | `grammar.annot.*`, `grammar.impl.by-structure`, `.delegation-field`, `.derivation-line*`, `.traitless-by*` | 10 | language | 1 syntax |
| **Total** | | **438** | **399 language, 39 stdlib** | |

Other compile-time rules that no option here changes: fact and metadata
expressions are evaluated once at compile time
([`annot.fact.eval`](../../spec/14-annotations.md#r-annot.fact.eval)), as is
shared constructor data
([`data.shared.compile-time`](../../spec/08-data-and-enums.md#r-data.shared.compile-time)).
So hd already has a small compile-time evaluator, limited to
requirement-free expressions without `block_on`. Suspension lowering
([`req.lowering.sugar`](../../spec/11-requirements-and-suspension.md#r-req.lowering.sugar))
also generates code, but it is a calling convention, not metaprogramming.

Four facts about today's design matter below:

1. **Library code is checked once.** A template's walker is an ordinary
   generic impl. Only the member obligation is checked per opt-in, and it
   names the member
   ([`annot.walker.obligation.error`](../../spec/14-annotations.md#r-annot.walker.obligation.error)).
2. **The template body is checked at the opt-in**
   ([`annot.template.checked`](../../spec/14-annotations.md#r-annot.template.checked)),
   and package interfaces carry template bodies
   ([`annot.limit.interfaces`](../../spec/14-annotations.md#r-annot.limit.interfaces)).
   So the 2026-09-26 objection to Design B, per-target checking, already
   applies in part to templates.
3. **The Walker trait is hd's polymorphic closure.** Closures are
   monomorphic, so a per-member callback must be a trait with a generic
   method, `member[F]`. Every "map over members" design meets this.
4. **Generic code is compiled once per shape**, at most five bodies
   ([Shapes and Generic Code](../../spec/04-type-system.md#shapes-and-generic-code)).
   Code that unrolls per member must be specialized per target, as
   `reified` code is ([`types.generic.specialized`](../../spec/04-type-system.md#r-types.generic.specialized)).

> **Since batch 37 (2026-10-01).** The implementation model changed: each
> value layout, including each tuple type, now gets its own specialized body,
> and only reference types share one. See
> [Shapes and Generic Code](../../spec/04-type-system.md#shapes-and-generic-code).
> This record keeps its original reasoning as history.

### Compiler Code In Scope

Lines of TypeScript in the prototype, `src/`, on 2026-10-01. The
prototype is a toy; the counts show where the work sits, not what a real
compiler needs.

| Area | Files | Lines |
| --- | --- | ---: |
| Typed derivation | `checker/typed-derivation.ts` 1,435, `member-lines.ts` 366, `derivation-models.ts` 111, `self-ref.ts` 143, `declaration-facts.ts` 58, `arbitrary-module.ts` 59, `generated-source.ts` 81 | 2,253 |
| Intrinsic derives, including `Debug` | `checker/derive-intrinsics.ts` 440, `debug.ts` 57 | 497 |
| `@error` | `checker/error-derivation.ts` 414, `error-generation.ts` 249 | 663 |
| Decorators and shapes | `checker/decorators.ts` 313, `shapes.ts` 461 | 774 |
| `by` delegation | `prepareDelegation` and `validateDelegations` in `checker/program-implementations.ts` | about 125 |
| Literal functions | `checker/literal-suffixes.ts` | 148 |
| `Inspectable` | `checker/inspectable.ts` 146, `expression-inspect.ts` 362 | 508 |
| Tuple equality and ordering strategies | `checker/context.ts`, the `equalityStrategy` and `orderingStrategy` methods | about 115 |
| `all!` | not implemented; every call is `unsupported-task-combinator` | about 10 |
| **Total** | | **about 5,090** |

`lib/std` also holds about 240 lines of per-arity tuple impls. They are
`Debug` and `Display` to 12 elements in `format.hd`, `Default` to 12 in
`ops.hd`, and `Arbitrary` for pairs and triples in `testing.hd`.

Two incidental findings, not proposals:
- The prototype derives `Debug` in TypeScript (`deriveDebug`), but the
  spec says `Debug` derives through a template
  ([`trait.debug.derive`](../../spec/09-traits.md#r-trait.debug.derive)).
- The [Package Interfaces](../../spec/10-modules.md#package-interfaces) table
  still lists "the bodies of pack and reified code", though packs were
  removed in batch 31.

## Survey

| Language | Mechanism | Derivation | Delegation | Field metadata | Checked | Errors and tooling |
| --- | --- | --- | --- | --- | --- | --- |
| Zig | `comptime`: types are values, `@typeInfo`, `inline for`, `@Type` ([docs][zig-comptime]) | `std.meta.eql` and `std.json` loop over fields with `inline for` ([meta.zig][zig-meta], [json][zig-json]) | none: `@Type` cannot create declarations, and `usingnamespace` was removed in 0.15 ([notes][zig-015]) | none: types add hook functions such as `jsonStringify` | after unrolling, per instantiation | errors inside library code; ZLS "is not able to resolve complex comptime expressions" ([ZLS][zls]) |
| Rust | declarative `macro_rules!` and procedural macros, including `#[derive]` ([reference][rust-proc]) | proc-macro derive, serde | crates: `ambassador` needs a macro on the trait too ([docs][ambassador]) | inert helper attributes, read as tokens | after expansion | `macro_rules!` is hygienic for locals ([reference][rust-hygiene]); proc macros are not; IDEs run a separate expansion server ([rust-analyzer][ra-macros]) |
| Scala 3 | `inline`, quoted macros, and compiler `Mirror`s for `derives` ([derivation][scala-derivation], [inline][scala-inline], [macros][scala-macros]) | a library `derived` over `MirroredElemTypes` and labels | `export` clauses, a language feature ([export][scala-export]) | not in `Mirror`; libraries use macros | per use site after inlining | inline recursion is capped at 32 by `-Xmax-inlines` ([options][scala-options]) |
| Swift | attached and freestanding macros over SwiftSyntax ([SE-0389][swift-attached], [SE-0382][swift-expr]) | `Codable` is compiler-derived; macros add members | member and peer macros see only their declaration's syntax | macro arguments | expansion is type-checked | macros run in a sandboxed plugin process |
| Nim | `template` and `macro`, with `typed` and `untyped` parameters ([manual][nim-macros]) | macros over the typed AST | macros | pragmas | `typed` arguments are checked before expansion | runs in the compiler's VM |
| D | `static if`, `static foreach`, `__traits`, string and template mixins ([static if][d-static-if], [traits][d-traits], [mixin][d-mixin]) | `__traits(allMembers, T)` with `static foreach` | `alias this`, and `std.typecons.Proxy` as a mixin ([Proxy][d-proxy]) | user-defined attributes via `__traits(getAttributes)` | after mixin, per instantiation | string mixins hide code from tools |
| Jai | `#run` runs any code at compile time; `#insert` pastes generated code ([primer][jai-primer], unofficial) | by `#insert` over type info | by `#insert` | notes on fields | after insertion | no public spec |
| MoonBit | a closed, compiler-known `derive` list: `Show`, `Eq`, `Compare`, `Hash`, `Default`, `Arbitrary`, `ToJson`, `FromJson` ([docs][moonbit-derive]) | compiler only | none | `derive(ToJson(...))` arguments | compiler | simplest; no user derivers |
| Haskell, Lean | Template Haskell splices ([GHC][ghc-th]); `GHC.Generics` representation types ([base][ghc-generics]); Lean `deriving` handlers in its meta language ([book][lean-meta]) | Generics: one ordinary instance over `Rep`, checked once | TH | none in Generics | Generics: once; TH: after splice | TH has stage restrictions and forces recompilation |

### Small Languages

Several young languages chose the shape of O1 below: a compile-time loop
over a type's fields. Two chose a closed, compiler-derived list.

| Language | Mechanism | Notes |
| --- | --- | --- |
| V | `$for field in T.fields` with `$if field.typ is string` ([docs][v-comptime], [example][v-reflection]) | used for its JSON decoder; field access by name with `$(field.name)` |
| Mojo | `comptime for` and `comptime if`, earlier `@parameter for` ([docs][mojo-parameter], [comptime][mojo-comptime]) | the docs warn that full unrolling can cause "binary bloat" |
| C3 | `$foreach` over `$Type::members`, inside `macro` declarations ([reflection][c3-reflection], [compile time][c3-comptime]) | the member list is "untyped", usable only at compile time |
| Crystal | macros that read `@type.instance_vars` ([macros][crystal-macros]) | a method that uses `@type` becomes a macro method, expanded per type |
| Roc | builtin abilities such as `Eq`, `Hash`, `Inspect`, and encoding, derived by the compiler for records, tuples, and tag unions ([encoding][roc-encode], [PR 11776][roc-derive]) | closed, like MoonBit; no user derivers |
| C# | incremental source generators over Roslyn ([cookbook][cs-incremental]) | the first generator API re-ran on every keystroke in the IDE; the incremental API caches each pipeline step |

### Research

| Work | Idea | What it means for hd |
| --- | --- | --- |
| Magalhães et al., generic deriving, 2010 ([ACM][generic-deriving]) | a class gives a default over a generic representation `Rep`; `deriving` picks it | the same shape as hd's templates over `Structure`: library code checked once |
| Blöndal, Löh, and Scott, Deriving Via, 2018 ([paper][deriving-via]) | an instance is borrowed from an isomorphic type | the idea behind O5: a data type borrows its tuple's impls |
| Yallop and White, lightweight higher-kinded polymorphism, 2014 ([Springer][hkt-lite]) | type functions encoded as "brands" with a defunctionalized application | the idea behind O6: a trait's associated type acts as a type function, with no higher-kinded parameter |
| Xie et al., Staging with Class, 2022 ([ACM][staging-class]) | typed quotes whose generated code is well-typed by construction | a variant of O1 could check the generator once instead of each expansion |
| Kovács, two-level type theory, 2022 ([arXiv][two-level]) | a compile-time level and a run-time level in one type system | the formal basis for O1b below; it needs dependent types at the compile-time level |
| Ullrich and de Moura, hygienic macros for Lean, 2020 ([arXiv][lean-hygiene]); Flatt, sets of scopes, 2016 ([ACM][sets-of-scopes]) | hygiene as scopes attached to syntax | the hygiene machinery O2 would need |

Takeaways:

1. **No surveyed comptime design does delegation.** Zig cannot generate
   methods, and it removed `usingnamespace` partly because tools could not
   see through it. Delegation lives in a language form (Kotlin `by`,
   Scala `export`, D `alias this`) or in an untyped macro.
2. **The systems that check library code once use a representation
   type**: `GHC.Generics`, Scala `Mirror`, and hd's `Structure`. The
   unrolling systems, Zig, D, and Jai, check per instantiation.
3. **Field metadata is typed only where the language attaches values.**
   Macros read attributes as tokens; Zig has none; `Mirror` omits them.
   hd's facts are already typed values.
4. **Every-arity tuple impls need either a compiler rule or a type-level
   map.** Rust stops at 12 elements ([tuple][rust-tuple]); Zig loops over
   `anytype` tuples with no bound to state.
5. **Heterogeneous join is a macro or a language form**: `tokio::join!`,
   Swift `async let`. A type computed at compile time is Zig's answer,
   which needs types as values.
6. **Small systems languages converge on a field loop**: V, Mojo, C3,
   Crystal, and Zig. Small functional languages converge on a closed
   derive list: Roc and MoonBit.
7. **Research gives two checked-once routes**: a generic representation
   (Generics, `Structure`), or typed staging. Only the first fits hd
   without dependent types.

## Options

O1 to O4 are the options the task named. O5 to O7 are new concepts, as
the owner asked; each is smaller than O1 or O2, and each can combine with
O3. A contrast row closes the section.

### O1. Typed Compile-Time Reflection With `inline for`

A generic body may loop over a type's members at compile time. Each
iteration is a copy of the body with the loop variable's own type, so
`f.get(self)` has that field's type. `inline if` prunes a branch before
it is checked. Facts are compile-time values, so a reader can stop the
build with `compile_error`. Types are not values: no function returns a
type.

| Adds | Kind | Tier |
| --- | --- | --- |
| `inline for x in e:` and `inline if c:` forms | 1 syntax | language |
| per-iteration typing, pruning, compile-time conditions, `break` and `continue` in unrolled loops, an evaluation budget | 2 semantic | language |
| a body that holds `inline for` is checked per instantiation and specialized per target, as `reified` code is | 2 semantic | language |
| `T::fields()`, `T::variants()`, `elements_of[Tup]()`, a construct intrinsic for `build`, `compile_error` | 3 intrinsic | language |

What it replaces: the traversal protocol (`Walker`, `Describer`,
`Source`, generated `walk`/`describe`/`build`, keys), and the
`generic-member-call` and strengthened-bound exceptions. Opt-in stays
`@derive` and templates, because a blanket impl is
`bare-parameter-impl-target`.

What it cannot replace:
- **`by` delegation.** A loop in a body cannot declare methods.
- **Tuple traits at every arity.** `impl[Tup < Tuple] Eq for Tup` has no
  way to say "each element is `Eq`". That needs a pack-like bound or
  "the impl exists when its body checks", which C++ calls SFINAE.
- **`all!`.** Its result type is computed from its argument types. That
  needs types as values, which is stronger than higher-kinded types.
- **`@error` messages.** They are interpolated strings scoped over a
  variant's members; a loop cannot turn a string into code.

**O1b, checked once.** Typed staging would check the loop body once, as
Typed Template Haskell and two-level type theory do. Each iteration's
field type is different, so the body must be typed for "some member type
`F` with the bounds the loop states". A bound on the loop, as in
`inline for f in T::fields() where F < Eq`, would make that possible. It
is today's walker bound moved into a loop header: the same check, in a
new syntax.

### O2. Hygienic Syntax Macros

A `macro` declaration is hd code that the compiler runs on syntax trees
and whose output it then checks, as Rust and Swift do. `@derive`,
`@error`, and a `delegate` macro become `lib/std` macros.

| Adds | Kind | Tier |
| --- | --- | --- |
| a macro declaration, an invocation spelling, quote and splice | 1 syntax | language |
| expansion order, hygiene, name resolution of expanded code, recursion limit, purity, determinism, spans | 2 semantic | language |
| an hd syntax-tree API, which every later grammar change must keep compatible | 4 core library, but large | stdlib |

The invocation needs a new sigil: hd's `!` already marks a suspending
call, and `#` starts a comment. A syntax macro attached to `Service`
cannot see `Describe`'s methods. So `delegate` needs a second macro on the
trait, as `ambassador` does, or a typed macro API. Tuple traits stop
at a fixed arity. `all!` becomes a macro over fixed-arity intrinsics.

### O3. Generalize Today's Templates

Keep `Structure` as the one derivation intrinsic and route more through
it. No new mechanism; two generalizations:

- **O3a.** `Eq`, `PartialOrd`, `Ord`, and `Hash` derive through `std`
  templates, like `Debug`. The compiler stops knowing their bodies. Their
  meaning moves to the stdlib tier.
- **O3b.** A tuple has a `Structure`: one data variant whose members are
  its elements `_0`, `_1`, and so on, marked positional. Every tuple type
  derives a fixed list of `std` traits through their templates, so
  `Debug`, `Display`, and `Default` reach every arity. Derived bounds
  (`annot.bound.params`) give "each element implements the trait"
  without any user syntax.

`by` and `all!` stay as they are; `by` already means a written forwarding
method since batch 32.

### O4. Status Quo

The baseline: 438 rules and about 5,090 prototype lines, as inventoried.

### O5. Every Data Type Is Its Tuple (New Concept)

`Structure` gains an associated type `Members`, the tuple of a data
type's member types, and two intrinsic methods `as_members(self)` and
`from_members(m)`. Tuples are already derived at every arity, so
`@derive(Eq, Hash, Ord)` on a data type means "compare as the members
tuple". The tuple derivation becomes the one primitive for comparison
traits, as `GHC.Generics` makes `Rep` the one primitive.

| Covers | How |
| --- | --- |
| U1 `Eq`, `Hash`, `Ord` for data | `self.as_members() == other.as_members()` |
| `Default`, `Arbitrary`, `Clone` for data | `from_members` over the tuple's impl |
| U4 | unchanged: it is the primitive |
| Enums | weak: an enum has no single tuple; it needs a variant index and one optional payload tuple per variant |
| Names and facts | not covered: `Debug` and codecs keep templates |

### O6. `all!` Gets A Written Signature (New Concept)

A tuple whose elements implement a trait with an associated type maps
that type element-wise. With a `std.task.Join` trait, `all!` becomes an
ordinary signature whose body alone is intrinsic:

| Rule | Kind | Tier |
| --- | --- | --- |
| `trait Join: type Output`, and `impl[T] Join for mut Suspend[T]` with `Output = T` | 4 core library | stdlib |
| A tuple implements `Join` when each element does; its `Output` is the tuple of the elements' `Output` types | 3 intrinsic | language |
| `fn all![Args < Tuple & Join](children...: Args) -> Args::Output`, with an intrinsic body | none new: `req.combinator.intrinsic` | stdlib signature |

This is the rejected `Each[Args, F]` without the higher-kinded `F`:
the map is fixed to one associated type of one trait. It removes the
"`all!` has no written signature" exception, so hover, docs, and errors
show a signature. It names `Join` in a language rule, so it is an
intrinsic, not a pure library change.

### O7. Member-Typed Facts (New Concept)

A fact type may declare one type parameter as its member type. At
attachment, the compiler binds it to the member's declared type, so the
fact is checked where it is written. A handle reads it typed:
`h.fact::[With]()` returns `With[F]?` for a `Field[S, F]`.

| Rule | Kind | Tier |
| --- | --- | --- |
| A `member` marker on a fact type's parameter | 1 syntax | language |
| The parameter is the member's type at attachment; a mismatch is `type-mismatch` on the decorator | 2 semantic | language |
| A typed read through a handle | 3 intrinsic | language |

It deletes the erase-and-downcast path of `arbitrary.with` and its
runtime panic. It changes one recorded stance, for opted-in fact types
only: "the language does not check that a metadata value suits its
member's type" ([Member Metadata](../../spec/14-annotations.md#member-metadata)).

### Contrast: A Closed Derive List

MoonBit's model: the compiler derives a fixed list and libraries cannot
add derivers. It would delete about 170 rules of typed derivation. It
reverses M1-M30, which opened `@derive` to library traits, so it is shown
only as the floor.

## Before/After

Library code is shown once per option. Lines with syntax that no chapter
specifies end in `# hypothetical`. Parsing checks syntax only.

### Today (O4)

The user side of all five use cases:

```text
use std.task.all

@derive(Eq, Debug)
data Point:
    x: i32
    y: i32

data JsonName:
    name: string

fn json(name: string) -> JsonName:
    JsonName { name: name }

data User:
    @json(name="mail")
    email: string

trait Describe:
    fn describe(self) -> string

data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger
    port: i32

impl Describe for Service by Logger

fn same(a: (i32, string, bool), b: (i32, string, bool)) -> bool:
    a == b

fn load!(id: i64) -> (User, List[Point]):
    all!(load_user(id), load_points(id))
```

A codec reads the fact in its walker, through the member's facts:

```text
fn key_of(m: Member) -> string:
    match m.facts.find::[JsonName]():
        .Some(found) => found.name
        .None => m.name
```

### O1. `inline for`

U1, `Eq` as a template body that unrolls over variants and fields:

```text
use std.structure.Structure

impl[T] Eq for T by Structure:
    fn eq(self, other: T) -> bool:
        inline for v in T::variants():  # hypothetical
            if v.holds(self) && v.holds(other):
                inline for f in v.fields():  # hypothetical
                    if f.get(self) != f.get(other):
                        return false
                return true
        false
```

A member whose type is not `Eq` fails at `!=`, inside this body, once per
target that derives it. Today it fails as `member-not-derivable`, naming
the member at the opt-in.

U3, a fact read and checked at compile time:

```text
impl[T] Encode for T by Structure:
    fn encode(self) -> string:
        let mut out = ""
        inline for f in T::fields():  # hypothetical
            inline if f.info.facts.has::[Skip]():  # hypothetical
                continue
            out = out + key_of(f.info) + "=" + f.get(self).encode() + ";"
        out
```

U2 is unchanged. U4 needs a bound the language cannot state:

```text
impl[Tup < Tuple] Eq for Tup:  # error today: bare-parameter-impl-target
    fn eq(self, other: Tup) -> bool:
        inline for e in elements_of[Tup]():  # hypothetical
            if e.get(self) != e.get(other):  # needs "each element < Eq"
                return false
        true
```

U5 needs a type computed at compile time:

```text
fn all![Args < Tuple](children...: Args) -> results_of(Args):  # hypothetical
    pass
```

### O2. Macros

U1 and U2 become `std` macros; the user side keeps its shape, but each
line now expands code the reader does not see:

```text
@derive(Eq, Debug)
data Point:
    x: i32
    y: i32

@delegate(Describe, to=Logger)  # hypothetical: an attached macro
data Service:
    Logger
    port: i32
```

The library writes syntax, not typed code:

```text
macro delegate(item: Item, trait_name: Path, to: Name) -> List[Item]:  # hypothetical
    pass
```

U3's fact is a token tree the macro parses. U4 is a macro that writes
impls up to a fixed arity. U5 expands `all!(a, b)` to a two-child
intrinsic, as `tokio::join!` does.

### O3. Generalized Templates

U1, `Eq` as an ordinary `std` template. It needs no new syntax: the
walker holds the second value, as the
[Handles](../../spec/14-annotations.md#handles) Note describes for a diff.

```text
use std.structure.{Structure, Field, Variant, Walker}

data Unequal: pass

data EqWalker[S]:
    other: S

impl[S] Walker[S] for EqWalker[S]:
    type Error = Unequal

    fn variant(mut self, v: Variant[S]) -> Result[void, Unequal]:
        if !v.holds(self.other):
            return .Err(Unequal {})
        .Ok()

    fn member[F < Eq](mut self, h: Field[S, F], value: F) -> Result[void, Unequal]:
        if h.get(self.other) != value:
            return .Err(Unequal {})
        .Ok()

impl[T] Eq for T by Structure:
    fn eq(self, other: T) -> bool:
        let mut w = EqWalker::[T] { other: other }
        match Structure::walk(self, w):
            .Ok(_) => true
            .Err(_) => false
```

U4, tuples derive through the same template; the marker names which
traits every tuple derives:

```text
use std.structure.Structure

@tuple_derive  # hypothetical: a std fact the compiler reads by name
impl[T] Debug for T by Structure:
    fn debug(self, out: mut DebugWriter) -> void:
        let mut w = DebugWalker { out: out }
        _ := Structure::walk(self, w)
```

After O3b, `debug((1, "a", true, 2.0, 'c', 1, 2, 3, 4, 5, 6, 7, 8))`
works at 13 elements, with no per-arity impl in `lib/std`. U2, U3, and
U5 are unchanged.

### O5. Members Tuple

U1, the meaning of `@derive(Eq, Hash)` on a data type:

```text
@derive(Eq, Hash)
data Account:
    id: i64
    email: string

impl Eq for Account:  # what the derive means under O5
    fn eq(self, other: Account) -> bool:
        self.as_members() == other.as_members()  # hypothetical: (i64, string)
```

### O6. `Join`

U5 with a written signature:

```text
trait Join:
    type Output

impl[T] Join for mut Suspend[T]:
    type Output = T

fn all![Args < Tuple & Join](children...: Args) -> Args::Output:  # element-wise Output is hypothetical
    pass
```

A wrong child then reports against the signature, for example "argument
2 has type `i32`, which does not implement `Join`".

### O7. Member-Typed Facts

U3 and `arbitrary.with`, checked where the fact is written:

```text
use std.annotation.annotate

@annotate(.Field)
data With[member F]:  # hypothetical
    gen: fn(mut Choices) -> F

fn with[F](gen: fn(mut Choices) -> F) -> With[F]:
    With::[F] { gen: gen }

fn small_age(c: mut Choices) -> i32:
    c.int(0, 120)

data Person:
    @with(small_age)
    age: i32
    @with(small_age)  # error under O7: type-mismatch, i32 is not string
    name: string
```

## Rule And Code Accounting

Counts are estimates from the inventory. "Moved" means a rule leaves the
language tier for `spec/std/`.

### Rules By Design Cost Order Kind

| Option | 1 syntax | 2 semantic | 3 intrinsic | 4 core library | Language net | Use cases covered |
| --- | --- | --- | --- | --- | ---: | --- |
| O1 `inline for` | +3 | +12 | -36 (protocol, net of 6 new intrinsics), -30 moved | +30 | about -51 | U1, U3 |
| O2 macros | +8, -10 | +30, -160 | -125 | +100 or more (syntax-tree API), +60 moved | about -257 | U1, U2, U3; U4 and U5 to a fixed arity |
| O3 generalized templates | 0 | +2 | -30 moved, +1 | +20 moved, -0 | about -27 | U1, U4 |
| O4 status quo | 0 | 0 | 0 | 0 | 0 | all, by separate mechanisms |
| O5 members tuple | 0 | +1 | -25 moved, +4 | +15 | about -20 | U1 for data; enums weak |
| O6 `Join` | 0 | 0 | -3, +1 | +2 | about -2 | U5 |
| O7 member-typed facts | +1 | +2 | +1 | -6 | about +4 | U3 checked |

Notes on the rows:
- **O1** removes the 57 protocol rules except about 15 that describe
  `Member`, `VariantInfo`, `self_ref`, `get`, `holds`, and defaults. The
  30 moved rules are the `Eq`/`Ord`/`Hash` meanings, which O3a also
  moves without O1.
- **O2** removes most of chapter 14's typed derivation, `@error`, the
  intrinsic derives, and `by`. Facts become inert attributes, so `annot.fact`
  goes too. It adds a syntax-tree API whose size grows with the grammar.
- **O3** keeps law partners and newtype derivation. It rewords
  `trait.target.tuple.derived` and `trait.debug.std-types`.
- **O7** grows the language by about 4 rules but deletes about 6 stdlib
  rules of `arbitrary.with` and its downcast panic.

### Compiler Code: What Moves From TypeScript To hd

| Option | Prototype TypeScript deleted | TypeScript added | `lib/std` hd |
| --- | ---: | ---: | --- |
| O1 | about 370 (generated traversals, protocol checks) + 330 (intrinsic derive bodies) | 1,000 to 2,000: a compile-time evaluator, an unroller, per-iteration checking | + `Eq`/`Ord`/`Hash`/`Debug` bodies, about 80 lines |
| O2 | about 3,500 (typed derivation, intrinsic derives, `@error`, delegation) | 3,000 to 6,000: a macro runner or plugin host, hygiene, a syntax-tree API | + every derive as a macro, + the syntax-tree module |
| O3 | about 500 (intrinsic derive bodies 330, `debug.ts` 57, tuple strategies 115) | about 100 (a tuple's `Structure`) | + about 80 lines of templates; - about 240 lines of per-arity tuple impls |
| O4 | 0 | 0 | 0 |
| O5 | about 330 | about 150 | + three-line derives |
| O6 | about 10 | about 60 | + `Join` |
| O7 | about 60 (`arbitrary.with` wrapping) | about 120 | - downcast helpers |

Only O2 deletes most of the compiler-implemented library features, and
it replaces them with a larger compiler feature. O3 deletes the most
TypeScript per rule added. The added-code figures are estimates, not
measurements.

## Compiler Performance

The owner asked how each option affects compile time and memory.

### Evidence From Other Languages

| Language | Reported cost |
| --- | --- |
| Zig | `comptime` runs in the compiler's interpreter, with a default budget of 1,000 backward branches before a compile error ([quota][zig-quota]). Each distinct set of `comptime` arguments makes a new instance. A comptime memory reform has been open since 2020 ([#5895][zig-5895]). |
| Rust | Proc macros are crates built before use; `syn` is often the long pole. serde_derive shipped a precompiled binary in 2023 to cut compile time, then reverted it after packagers objected ([#2538][serde-2538]). `watt` exists to run precompiled macros as Wasm ([watt][watt]). |
| Swift | Building SwiftSyntax for macros took about 20 seconds in debug and over 4 minutes in release; Swift 6.1.1 added prebuilt SwiftSyntax ([forum][swift-macro-time], [prebuilts][swift-prebuilts]). |
| Scala 3 | Inline derivation unrolls per use site; large types hit the 32-inline cap and need `-Xmax-inlines` ([options][scala-options], [tapir][scala-tapir]). |
| Haskell | Template Haskell splices force recompilation of dependents; `GHC.Generics` instances are checked once ([GHC][ghc-th]). |

The pattern: an interpreter (Zig, D CTFE) costs per evaluation; a macro
costs per expansion plus its own build; inlining costs per use site.
Representation types, such as Generics and `Structure`, cost one generic
check plus one cheap specialization.

### A Measurement Of hd's Prototype

A generated program declared `N` data types of `M` `i64` fields each,
and derived one trait per type. Times are wall-clock for `hd check` and
`hd build --wat`, including about 0.4 s of start-up. One run each, on a local machine,
2026-10-01; treat them as rough.

| Derive | Types x fields | `check` ms | `build` ms | WAT bytes |
| --- | --- | ---: | ---: | ---: |
| none | 100 x 40 | 400 | 609 | 168,719 |
| `Eq`, intrinsic | 100 x 40 | 529 | 849 | 1,298,629 |
| `Eq, Hash`, intrinsic | 100 x 40 | 616 | 1,115 | 2,092,873 |
| `Debug`, intrinsic in the prototype | 100 x 40 | 659 | 1,040 | 1,291,445 |
| a user template walking each member | 100 x 40 | 2,692 | 5,658 | 12,265,363 |
| a user template walking each member | 50 x 20 | 803 | 1,642 | 3,225,402 |

Per member, over the no-derive baseline:

| Path | Extra `check` time | Extra WAT |
| --- | ---: | ---: |
| intrinsic `Eq` | about 0.03 ms | about 0.28 KB |
| template | about 0.57 ms | about 3.0 KB |

So the prototype's template path costs about 18 times the check time
and 10 times the code of an intrinsic derive. The reason is the
prototype's lowering, not the design: it writes hd source for each
member's facts, handles, and traversal, then re-checks it. That is
macro-style expansion. The spec's model is one specialization per target
and walker, with constant handles
([`annot.limit.specialize`](../../spec/14-annotations.md#r-annot.limit.specialize),
[Handles](../../spec/14-annotations.md#handles)).

### Per Option

| Option | Compile-time work | Code size | Caching and incremental reuse |
| --- | --- | --- | --- |
| O1 | evaluate conditions, unroll, and check the body once per target and field | grows with fields times body size, per derived trait | a target's instance depends on the library body and on fact values; both must be in the cache key |
| O2 | build or load the macro, run it, re-parse and check the output | grows with the generated text | cacheable only if macros are deterministic and pure; IDEs need an expansion server |
| O3 | as today: one generic check of the template, one specialization per target | as today; moving `Eq` and `Hash` onto the template path costs about 10 times the code in the prototype until its lowering is fixed | as today: interfaces carry template bodies |
| O4 | as measured | as measured | as today |
| O5 | one tuple conversion per derive, then the tuple's impl | small; may allocate a tuple unless the backend scalarizes it | the tuple impl is shared by every data type with the same member types |
| O6, O7 | one extra check at a signature or a decorator | none | none |

## Comparison

| | O1 `inline for` | O2 macros | O3 templates | O4 today | O5 tuple | O6 `Join` | O7 typed facts |
| --- | --- | --- | --- | --- | --- | --- | --- |
| U1 derive | yes | yes | yes | yes | data only | no | no |
| U2 `by` | no | with a trait macro | no | yes | no | no | no |
| U3 facts | read and checked at compile time | as tokens | as today | as today | no | no | checked at attachment |
| U4 every arity | no | to a fixed arity | yes | yes | yes | no | no |
| U5 `all!` | needs types as values | fixed arity | no | intrinsic | no | written signature | no |
| Language rules, net | about -51 | about -257 | about -27 | 0 | about -20 | about -2 | about +4 |
| Costliest change | 1 syntax | 1 syntax | 3 intrinsic | none | 3 intrinsic | 3 intrinsic | 1 syntax |
| Library code checked | per target | per expansion | once | once | once | once | once |
| Error site | inside library code | inside expanded code | the member, at the opt-in | the member, at the opt-in | the member | the argument | the decorator |
| Weak-model errors | poor | poor | good | good | good | better than today | better than today |
| Separate compilation | bodies in interfaces, specialized downstream | macro binaries per package | as today | as today | as today | as today | as today |
| LSP | hard, as ZLS shows | needs an expansion server | as today | as today | as today | better: hover shows a signature | as today |
| Pack-like user syntax | per-element loops over tuples | no | no | no | no | no | no |
| Compiler performance | interpreter and unroll per target | expansion and re-check per use | as today; worse in the prototype for `Eq` | measured | cheap | none | none |

## Risks

| Risk | Options | Effect |
| --- | --- | --- |
| Errors move into library code | O1, O2 | A weak model sees a type error at a line of `std` it never wrote, as C++ template errors read. Today it sees the member name at its own `@derive`. |
| Pack-like syntax returns | O1 | `inline for e in elements_of[Tup]()` is a value-level pack loop, and every-arity bounds need "each element", which batch 31 removed. |
| Two derivation mechanisms | O1, O2 | Templates stay for opt-in, so users learn templates and the new mechanism. |
| A second stable surface | O2 | A syntax-tree API freezes the grammar; every grammar change breaks macros. |
| Facts become checked | O1, O7 | Both change "the language does not check that a metadata value suits its member". O7 does it only for fact types that opt in. |
| Slower compiles | O1, O2; O3 in the prototype | Measured: the prototype's template path is about 18 times slower per member than an intrinsic derive. |
| Shape specialization | O1 | Unrolled bodies cannot share the five shape bodies; each target needs its own, as `reified` code does. |
| Signatures inferred from bodies | O1 with types as values | A result type such as `results_of(Args)` is computed, not written, against [Fully Annotated Declarations](../../spec/10-modules.md#fully-annotated-declarations). |
| Enum encoding | O5 | An enum as an index and optional payload tuples gives correct `Eq` and `Ord`, but `Default` and `Arbitrary` can build invalid combinations. |
| Reopens Design G | O1, O2 | The owner chose compiler-generated visitors over Design B on 2026-09-26. The new evidence since then is that packs, Design D's base, are gone. That evidence argues for O3 and O6, not for O1. |

## Recommendation

**Recommendation: no single comptime or macro intrinsic. Make
`Structure` the one derivation intrinsic and add three small pieces: O3,
then O7 and O6.**

- **The single intrinsic hd already has is the right one.** `Structure`
  checks library code once and reports errors at the member. O3 routes
  `Eq`, `Ord`, `Hash`, and every-arity tuples through it. That removes
  about 27 language rules and about 500 prototype lines, and moves the
  bodies into `lib/std`.
- **O1 buys little over O3.** Its net extra gain is the traversal
  protocol, about 25 rules, for a new syntax tier, per-target checking,
  and an interpreter. It still leaves `by`, `all!`, every-arity tuples,
  and `@error` to other rules.
- **O2 removes the most rules but adds the most compiler.** It needs a
  macro runner, a syntax-tree API, a new sigil, and an expansion server
  for the LSP. It reverses the "no macro system" basis of
  [Typed Derivation](../../spec/14-annotations.md#typed-derivation).
- **O7 is the creative piece worth taking.** Typed facts catch a wrong
  `arbitrary.with` at the decorator instead of at the first test case.
- **O6 removes the one intrinsic with no signature.** Its cost is one
  element-wise rule that names `Join`.
- **Condition for O3a.** Fix the prototype's template lowering before
  `Eq` and `Hash` move onto it, or keep them intrinsic in the prototype.
  The measurement shows a 10 times code-size cost per member today.

What this gives up: `by` and `@error` stay separate mechanisms, as Kotlin
and Scala keep `by` and `export`. The next best option is O3 alone.

After batch 36: macros and comptime are deferred. Code generation plus
compile-time reflection is logged as the direction to revisit; see
[Deferred: Code Generation Plus Compile-Time Reflection](#deferred-code-generation-plus-compile-time-reflection).

## Owner Decisions

Batch 36, 2026-10-01. Applied in spec pass 36; the prototype follows in
a separate task.

| Question | Option | Answer | Applied as |
| --- | --- | --- | --- |
| Q1 | O7 typed member facts | **Accepted**, as recommended, opt-in | [Member-Typed Facts](../../spec/14-annotations.md#member-typed-facts): `@member_typed` marks a fact type, whose first type parameter binds to the field's declared type. `arbitrary.with` returns `With[F]`, so its runtime downcast panic is gone. Superseded in batch 39: `@annotate::[F](.Field)` declares a typed fact type. |
| Q2 | O6 `Join` | **Rejected** | `all!` stays a compiler intrinsic with its written typing rules. |
| Q3 | O3 comparison derives | **Accepted**, as recommended | [`trait.derive.cmp-templates`](../../spec/09-traits.md#r-trait.derive.cmp-templates); meaning in [Cmp](../../spec/std/cmp.md) and [Hash](../../spec/std/hash.md). The prototype moves after its template lowering is fixed. |
| Q4 | O3b tuple `Structure` | **Accepted**, as recommended | [Tuple Structure](../../spec/14-annotations.md#tuple-structure) and [Tuple Templates](../../spec/14-annotations.md#tuple-templates); the 12-element limit is gone. |
| Q5 | O5 members tuple | **Rejected for now**, as recommended | none |
| Q6 | O1 `inline for` | **Rejected for now**, as recommended | none |
| Q7 | O2 macros | **Rejected for now**, as recommended | none |

Reasons for the rejections:

- **O6.** The owner's reason: `impl Join for Suspend[T]` does not make a
  tuple `Args` implement `Join`. That needs per-arity impls or a hidden
  compiler rule for tuples, which is the rejected `Each` in disguise. So
  O6 adds a named intrinsic without removing one.
- **O1.** O3 gets most of its gain without a new syntax tier. A loop
  still leaves `by`, `all!`, and `@error` as they are, and checking per
  target slows compiles.
- **O2.** It trades about 3,500 prototype lines for a larger macro
  engine, a syntax-tree API, and a new sigil. Errors would move into
  expanded code, and it reverses the "no macro system" basis of typed
  derivation.
- **O5.** O3 covers the same derives for data and enums alike, while a
  members tuple is awkward for enums.

The spelling of O7's marker: the record's `data With[member F]` is a
`syntax-error` in the reference parser. A `@member_typed` decorator from
`std.annotation` parses today and needs no grammar change, so it is the
cheaper kind in the [Design Cost Order](../../AGENTS.md#design-cost-order).
It marks the first type parameter.

A rest tuple's `Structure` exposes its rest element as one member of type
`List[T]`. Comparison and hashing then treat it as its list. Text needs
the items one by one, so `Walker` gains `rest`, whose default body calls
`member`.

## Deferred: Code Generation Plus Compile-Time Reflection

A deferred direction, not a decision. The owner, batch 36: "defer the
idea of macro/comptime now, but i think probably we can do code gen +
comptime reflection to achieve some thing similar ... we want something
between golang and macro".

| Model | How | Cost |
| --- | --- | --- |
| Go | `go generate` runs external generators (stringer, mockgen, protoc-gen-go, sqlc, easyjson) that write ordinary checked-in `.go` files; runtime `reflect` and struct tags; no macros | stale generated files, one tool per generator, no "derive for every type" |
| Rust and Swift macros | expansion inside the compiler | an engine, the syntax tree as an API, errors in expanded code |
| Middle ground, later | the compiler or `hd` tooling gives generators typed, read-only compile-time reflection: types, fields, and facts, the data `Structure` exposes. Generators emit ordinary hd source, type-checked like hand-written code | open, below |

Open points: in-build or checked-in output; caching and incremental
builds; whether generators are hd programs that the toolchain runs; and
how errors map back to the generator's input.

`Structure` templates stay the derivation mechanism. This direction aims
at what templates cannot do: `by`, `@error`, and external schemas such
as protobuf and SQL.

## Questions For The Owner

The owner answered these in batch 36; see [Owner Decisions](#owner-decisions).
Smallest first. Each question is one idea.

### Q1. Typed Member Facts

`arbitrary.with(gen)` with the wrong generator type panics on the first
test case today. A fact type could name its member type, so the mismatch
is a compile error at the decorator.

- **A.** Add member-typed facts for fact types that opt in, as O7.
  *Recommended.*
- **B.** Keep facts unchecked; readers check.

```text
data Person:
    @with(small_age)
    name: string  # under A: type-mismatch at the decorator
```

### Q2. A Written Signature For `all!`

`all!` has four typing rules and no signature, so hover and docs show
nothing.

- **A.** Add `std.task.Join` and an element-wise `Output` for tuples, as
  O6. *Recommended.*
- **B.** Keep the four typing rules.

```text
fn all![Args < Tuple & Join](children...: Args) -> Args::Output:  # hypothetical
    pass
```

### Q3. Comparison Derives Through Templates

`Eq`, `PartialOrd`, `Ord`, and `Hash` are the last derives whose bodies
the compiler writes. As `std` templates, their meaning moves to
`spec/std/` and about 330 prototype lines go.

- **A.** Move them to templates, after the prototype's template lowering
  is fixed. *Recommended.*
- **B.** Move them now and accept slower prototype compiles.
- **C.** Keep them intrinsic.

```text
@derive(Eq, Hash)
data Key:
    id: i64
```

### Q4. Tuples Derive Through Templates

Tuple `Debug`, `Display`, and `Default` stop at 12 elements, while `Eq`
works at every arity. A tuple with a `Structure` would derive marked
templates at every arity.

- **A.** Give tuples a `Structure` and derive marked templates, as O3b.
  *Recommended.*
- **B.** Keep the compiler rule for comparison and the per-arity impls.

```text
fn show(t: (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32)) -> string:
    debug(t)
```

### Q5. Data Types As Their Members Tuple

O5 makes a data type's comparison derives "compare as the tuple of its
members". It is simple for data and awkward for enums.

- **A.** Not now; O3 covers the same derives. *Recommended.*
- **B.** Explore O5 for data types only.

```text
fn same(a: Account, b: Account) -> bool:
    a.as_members() == b.as_members()  # hypothetical
```

### Q6. Compile-Time `inline for`

O1 would replace walkers with loops over members, checked per target. It
leaves `by`, `all!`, tuples, and `@error` as they are.

- **A.** No. *Recommended:* O3 gets most of its gain without a new tier.
- **B.** Yes, as a replacement for the traversal protocol only.

```text
impl[T] Eq for T by Structure:
    fn eq(self, other: T) -> bool:
        inline for f in T::fields():  # hypothetical
            if f.get(self) != f.get(other):
                return false
        true
```

### Q7. Syntax Macros

O2 would move `@derive`, `@error`, and `by` into `lib/std` macros, with
a syntax-tree API and a macro runner in the compiler.

- **A.** No. *Recommended:* it trades about 3,500 prototype lines for a
  larger macro engine, and moves errors into expanded code.
- **B.** Yes, as a long-term direction, after the core is settled.

```text
@delegate(Describe, to=Logger)  # hypothetical
data Service:
    Logger
```

## Sources

[zig-comptime]: https://ziglang.org/documentation/master/#comptime
[zig-meta]: https://github.com/ziglang/zig/blob/master/lib/std/meta.zig
[zig-json]: https://github.com/ziglang/zig/blob/master/lib/std/json/static.zig
[zig-015]: https://ziglang.org/download/0.15.1/release-notes.html
[zig-quota]: https://ziggit.dev/t/what-is-the-eval-branch-quota/7852
[zig-5895]: https://github.com/ziglang/zig/issues/5895
[zls]: https://github.com/zigtools/zls/blob/master/README.md
[rust-proc]: https://doc.rust-lang.org/reference/procedural-macros.html
[rust-hygiene]: https://doc.rust-lang.org/reference/macros-by-example.html#hygiene
[rust-tuple]: https://doc.rust-lang.org/std/primitive.tuple.html
[ambassador]: https://docs.rs/ambassador
[ra-macros]: https://rust-analyzer.github.io/blog/2021/11/21/ides-and-macros.html
[serde-2538]: https://github.com/serde-rs/serde/issues/2538
[watt]: https://github.com/dtolnay/watt
[scala-derivation]: https://docs.scala-lang.org/scala3/reference/contextual/derivation.html
[scala-inline]: https://docs.scala-lang.org/scala3/reference/metaprogramming/inline.html
[scala-macros]: https://docs.scala-lang.org/scala3/reference/metaprogramming/macros.html
[scala-export]: https://docs.scala-lang.org/scala3/reference/other-new-features/export.html
[scala-options]: https://docs.scala-lang.org/scala3/guides/migration/options-new.html
[scala-tapir]: https://softwaremill.community/t/maximal-number-of-successive-inlines-32-exceeded/440
[swift-attached]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0389-attached-macros.md
[swift-expr]: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0382-expression-macros.md
[swift-macro-time]: https://forums.swift.org/t/swift-macros-build-time-overhead-concerns/70443
[swift-prebuilts]: https://forums.swift.org/t/preview-swift-syntax-prebuilts-for-macros/80202
[nim-macros]: https://nim-lang.org/docs/manual.html#macros
[d-static-if]: https://dlang.org/spec/version.html#staticif
[d-traits]: https://dlang.org/spec/traits.html
[d-mixin]: https://dlang.org/spec/statement.html#mixin-statement
[d-proxy]: https://dlang.org/phobos/std_typecons.html#Proxy
[jai-primer]: https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md
[moonbit-derive]: https://docs.moonbitlang.com/en/latest/language/derive.html
[ghc-th]: https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/template_haskell.html
[ghc-generics]: https://hackage.haskell.org/package/base/docs/GHC-Generics.html
[lean-meta]: https://leanprover-community.github.io/lean4-metaprogramming-book/
[v-comptime]: https://docs.vlang.io/conditional-compilation.html
[v-reflection]: https://github.com/vlang/v/blob/master/examples/compiletime/reflection.v
[mojo-parameter]: https://docs.modular.com/mojo/manual/decorators/unroll
[mojo-comptime]: https://docs.modular.com/mojo/manual/metaprogramming/comptime-evaluation/
[c3-reflection]: https://c3-lang.org/generic-programming/reflection/
[c3-comptime]: https://c3-lang.org/generic-programming/compiletime/
[crystal-macros]: https://crystal-lang.org/reference/1.19/syntax_and_semantics/macros/macro_methods.html
[roc-encode]: https://www.roc-lang.org/examples/EncodeDecode/README
[roc-derive]: https://github.com/roc-lang/roc/pull/11776
[cs-incremental]: https://github.com/dotnet/roslyn/blob/main/docs/features/incremental-generators.cookbook.md
[generic-deriving]: https://doi.org/10.1145/1863523.1863529
[deriving-via]: https://ryanglscott.github.io/papers/deriving-via.pdf
[hkt-lite]: https://doi.org/10.1007/978-3-319-07151-0_8
[staging-class]: https://doi.org/10.1145/3498723
[two-level]: https://arxiv.org/abs/2209.09729
[lean-hygiene]: https://arxiv.org/abs/2001.10490
[sets-of-scopes]: https://doi.org/10.1145/2837614.2837620

| Topic | Source |
| --- | --- |
| Zig comptime, `@typeInfo`, `inline for`, `@Type` | [Zig reference][zig-comptime] |
| Zig field loops in the standard library | [`std.meta`][zig-meta], [`std.json`][zig-json] |
| Zig `usingnamespace` removal and its tooling reason | [0.15.1 release notes][zig-015] |
| Zig eval branch quota and comptime memory | [Ziggit][zig-quota], [issue 5895][zig-5895] |
| ZLS and comptime | [ZLS README][zls] |
| Rust macros, hygiene, tuple impls to 12 | [proc macros][rust-proc], [hygiene][rust-hygiene], [tuple][rust-tuple] |
| Rust delegation by macro | [ambassador][ambassador] |
| Rust macros in the IDE | [rust-analyzer][ra-macros] |
| Rust proc-macro compile time | [serde #2538][serde-2538], [watt][watt] |
| Scala 3 derivation, inline, macros, export, inline cap | [derivation][scala-derivation], [inline][scala-inline], [macros][scala-macros], [export][scala-export], [options][scala-options], [tapir thread][scala-tapir] |
| Swift macros and their build cost | [SE-0389][swift-attached], [SE-0382][swift-expr], [overhead thread][swift-macro-time], [prebuilts][swift-prebuilts] |
| Nim macros | [manual][nim-macros] |
| D compile-time features and forwarding | [static if][d-static-if], [traits][d-traits], [mixin][d-mixin], [Proxy][d-proxy] |
| Jai, unofficial | [Jai primer][jai-primer] |
| MoonBit derive | [docs][moonbit-derive] |
| Template Haskell, Generics, Lean | [GHC][ghc-th], [Generics][ghc-generics], [metaprogramming book][lean-meta] |
| V, Mojo, C3, Crystal field loops | [V docs][v-comptime], [V example][v-reflection], [Mojo][mojo-parameter], [Mojo comptime][mojo-comptime], [C3 reflection][c3-reflection], [C3 compile time][c3-comptime], [Crystal][crystal-macros] |
| Roc derived abilities | [encoding example][roc-encode], [PR 11776][roc-derive] |
| C# incremental generators | [Roslyn cookbook][cs-incremental] |
| Research | [generic deriving][generic-deriving], [Deriving Via][deriving-via], [lightweight HKT][hkt-lite], [Staging with Class][staging-class], [two-level type theory][two-level], [Lean hygiene][lean-hygiene], [sets of scopes][sets-of-scopes] |
| hd's earlier derivation survey and Design G | git commits `d171c264` and `6d248cb9` |
| hd packs removal and `Each[Args, F]` | [Reopen: Packs And Literal Sugar](REOPEN_PACKS_LITERALS.md#future-option-a-type-level-tuple-map) |

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../../spec/reference-parser/parser.ts) on
2026-10-01. Parsing checks syntax only; no block is claimed to
type-check. Blocks that use a name without declaring it, such as
`load_user`, `Encode`, or `DebugWalker`, rely on declarations elsewhere.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | Today: user side of U1-U5 | parses |
| 2 | Today: a fact read by a codec | parses |
| 3 | O1: `Eq` with `inline for` | `syntax-error` at line 5, the first `inline for`, marked hypothetical |
| 4 | O1: a fact read at compile time | `syntax-error` at line 4, `inline for`, marked hypothetical |
| 5 | O1: tuple `Eq` | `syntax-error` at line 3, `inline for`, marked hypothetical; the header parses and is a checker error today |
| 6 | O1: `all!` with a computed type | `syntax-error` at line 1, the computed result type, marked hypothetical |
| 7 | O2: user side | parses; `@delegate` parses as a decorator, and its macro meaning is hypothetical |
| 8 | O2: a macro declaration | `syntax-error` at line 1, `macro`, marked hypothetical |
| 9 | O3: `Eq` as a template | parses |
| 10 | O3: tuple derivation marker | parses; `@tuple_derive` parses as a decorator, and its meaning is hypothetical |
| 11 | O5: members tuple | parses; `as_members` is a hypothetical method |
| 12 | O6: `Join` and `all!` | parses; the element-wise `Output` is a hypothetical rule |
| 13 | O7: member-typed facts | `syntax-error` at line 4, `member F`, marked hypothetical |
| 14 | Q1 | parses; the error is a checker result under O7 |
| 15 | Q2 | parses; the element-wise `Output` is hypothetical |
| 16 | Q3 | parses |
| 17 | Q4 | parses |
| 18 | Q5 | parses; `as_members` is hypothetical |
| 19 | Q6 | `syntax-error` at line 3, `inline for`, marked hypothetical |
| 20 | Q7 | parses; the macro meaning is hypothetical |
