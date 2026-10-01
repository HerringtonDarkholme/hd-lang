# Simplify Embedding, Promotion, And Delegation

Status: simplification review, 2026-09-30. The owner answered its
questions in batch 32; see [Owner Decisions](#owner-decisions). All six
are applied: Q1, Q2, Q5, and Q6 in pass 32a, and Q3 and Q4 in pass 32b.
The rest of this record is the review as written, not accepted behavior.

The owner asked for a round of simplification over embedding, `by`
delegation, and the features tied to them. The owner decided to keep
embedding, so this record trims the mechanism. Removal appears only as a
contrast row. It follows
[R1](SYNTAX_SEMANTICS_COST.md#r1-embedding-promotion-and-delegation) and
rows 20-21 of the [cost table](SYNTAX_SEMANTICS_COST.md#cost-table).

Under review: [Data Embedding](../spec/08-data-and-enums.md#data-embedding)
and its subsections, [Copy-Update Literals](../spec/08-data-and-enums.md#copy-update-literals),
[Member Resolution](../spec/03-names-and-scopes.md#member-resolution),
[Embedding And Trait Satisfaction](../spec/09-traits.md#embedding-and-trait-satisfaction),
[Trait Delegation](../spec/09-traits.md#trait-delegation),
[Mutable Paths](../spec/04-type-system.md#mutable-paths), and the
embedded-member rules of [chapter 14](../spec/14-annotations.md). The
decisions behind them are VE1-VE4, VE-S, the second embedding review, the
embedding limits, the single view, and trait delegation, all logged in
[Revision Notes](../spec/README.md#revision-notes).

## Contents

- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Real Uses](#real-uses)
- [Survey](#survey)
- [Options](#options)
- [Rule Accounting](#rule-accounting)
- [What Users Lose](#what-users-lose)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Owner Decisions](#owner-decisions)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

Embedding, promotion, part copies, and `by` delegation touch 219 numbered
rules in nine chapters. Real code uses them in 16 guide lines, and
`lib/std` and the examples use none. Most of the rule count is not
mechanism. It is the same few facts stated in several chapters.

| Fact | Times stated today |
| --- | ---: |
| A promoted method never fills a trait method; embedding grants no conformance | 10 |
| Only `pub` fields and `pub` inherent methods are promoted; trait methods never | 9 |
| A trait method beside a promoted method is `ambiguous-method` | 7 |
| A promoted member is the explicit path, with the part as receiver | 9 |
| A copy into a part follows the copy-update rules for readonly sources | 10 |
| The copy marker is required on an embedded field and invalid elsewhere | 8 |

The aim is one statement per fact, then a look at the corners that no
real code reaches.

```text
data Timestamps:
    pub created_at: i64
    pub updated_at: i64

impl Timestamps:
    pub fn touch(mut self, at: i64) -> void:
        self.updated_at = at

data Post:
    Timestamps
    id: string

fn edit(post: mut Post, stamps: Timestamps) -> void:
    post.touch(1700000100)
    post.Timestamps ...= stamps
```

## What hd Has Today

Counts are numbered `r[...]` items in chapters 01-14 on 2026-09-30. The
cost review counted 137 rules with a narrower prefix set. This record also
counts the rules elsewhere that exist only because of embedding, so it
finds 219.

### By Section

| Where | Rules | Spec lines | Anchors |
| --- | ---: | ---: | --- |
| 08 Data Embedding, all subsections | 80 | 304 | [names](../spec/08-data-and-enums.md#embedded-field-names) 8, [limits](../spec/08-data-and-enums.md#embedding-limits) 7, [parts and copies](../spec/08-data-and-enums.md#parts-and-copies) 39, [mutable edges](../spec/08-data-and-enums.md#mutable-edges) 7, [variance](../spec/08-data-and-enums.md#embedded-field-variance) 1, [promotion](../spec/08-data-and-enums.md#member-promotion) 10, [composition](../spec/08-data-and-enums.md#composition-not-subtyping) 5, [metadata](../spec/08-data-and-enums.md#embedded-field-metadata) 3 |
| 08 elsewhere | 9 | about 10 | `data.field.embedded-no-mut`, `data.default.embedded`, `data.vis.*` (3), `data.update.embedded`, `data.access.embedded-copy`, `data.derive.embedded`, `data.unsupported.mut-embedded` |
| 03 Member Resolution, promotion parts | 57 | 140 | [depths](../spec/03-names-and-scopes.md#depths-and-promoted-members) 8, [take part](../spec/03-names-and-scopes.md#members-that-take-part) 7, [hiding and conflicts](../spec/03-names-and-scopes.md#hiding-and-conflicts) 13, lookup refinements 10, [examples](../spec/03-names-and-scopes.md#method-lookup-example) 5, [promoted access](../spec/03-names-and-scopes.md#promoted-member-access) 6, [dependency changes](../spec/03-names-and-scopes.md#dependency-changes) 4, [no overriding](../spec/03-names-and-scopes.md#no-overriding) 3, visibility 1 |
| 09 Traits | 43 | 171 | [`trait.embed.*`](../spec/09-traits.md#embedding-and-trait-satisfaction) 16, [`trait.by.*`](../spec/09-traits.md#trait-delegation) 22, five rules in other sections |
| 02 Grammar | 12 | about 20 | `grammar.data.embedded*` 5, `grammar.stmt.copy-assign*` 2, `grammar.primary.field-copy*` 2, `grammar.primary.prefix-copies`, `grammar.impl.delegation-field`, `grammar.impl.promoted` |
| 04 Types | 7 | about 10 | `types.mut.embedded`, `types.fresh.embedded-copy`, `types.path.field.embedded` (2), `types.path.promoted`, `types.path.store-embedded`, `types.polarity.embedded` |
| 05 Expressions | 7 | about 8 | `expr.assign.embedded`, `expr.data.embedded*` 2, `expr.update.*embedded*` 3, `expr.member.embedded-trait` |
| 10 Modules | 1 | 1 | `module.vis.embedded` |
| 14 Annotations | 3 | about 5 | `annot.member.embedded`, `.embedded.part`, `.no-flatten` |
| **Total** | **219** | **about 670** | |

### By Kind

Kinds follow the [Design Cost Order](../AGENTS.md#design-cost-order).

| Kind | Rules | Examples |
| --- | ---: | --- |
| 1. Syntax | 34 | the bare type-name member, `Label: ...value`, `...=`, `by E`, the marker-required and marker-invalid rules |
| 2. Semantic | 181 | copies, access through parts, mutable edges, promotion depth, hiding, conflicts, lookup, delegation |
| 3. Intrinsic | 4 | the `embedded` flag of `Member` and the part as a derivation member |
| 4. Core library | 0 | |

Diagnostics: `duplicate-embedded-field`, `embedded-non-data`,
`too-many-embedded-fields`, `embedding-too-deep`, `embedded-copy-required`,
`copy-into-ordinary-field`, `mutable-embedded-field`,
`ambiguous-promoted-member`, and `invalid-delegation`. Error codes will be
revamped later, so no question here is about them.

## Real Uses

"Real" means the guide, `lib/std`, `examples/`, and the playground
examples. Counts come from a scan of `data` bodies for bare type-name
members, and a search for `...` labels, `...=`, and `by` headers.

| Corpus | Embedded fields | Copies `E: ...e` and `...=` | `impl Tr for C by E` |
| --- | ---: | ---: | ---: |
| [Language Tour](../guide/LANGUAGE_TOUR.md) | 10, of which 5 show a conflict | 5 | 3 |
| [Learn In 10 Minutes](../guide/LEARN_IN_10_MINUTES.md) | 2 | 1 | 1 |
| `lib/std` | 0 | 0 | 0 |
| `examples/` and playground | 0 | 0 | 0 |
| Fixtures | 83 files | many | 9 headers in 7 files |

The 81 fixture rows in [cases.tsv](../spec/conformance/cases.tsv) that use
embedding are 43 accepts and 38 rejects over 17 codes. The largest reject
groups are `ambiguous-promoted-member` (7), `invalid-delegation` (4),
`embedded-copy-required` (3), `embedded-non-data` (3), and
`mutable-upgrade` (3).

Every guide use is a teaching block. Nothing in real code embeds more than
two types or nests embedding. Only an error example puts a private member
beside a promoted one, and no delegated trait has associated items.

## Survey

| Language | Embedding or delegation | Conflicts | Limits | Copies | Conformance |
| --- | --- | --- | --- | --- | --- |
| Go | An embedded field is `T` or `*T`; its name is the type name ([spec][go-struct]) | the shallowest `f` wins; not exactly one at that depth makes the selector illegal at the use ([spec][go-selectors]) | none; only a struct may not contain itself through structs or arrays ([spec][go-struct]) | struct values copy on assignment; `*T` shares | promoted methods join the method set, so interfaces are satisfied implicitly ([spec][go-method-sets]) |
| Kotlin | `class D(b: Base) : Base by b` forwards an interface to a stored object ([docs][kotlin-delegation]) | an `override` in the class replaces the forwarded member | interfaces only, no fields promoted | the delegate is a reference | yes, for the named interface |
| Rust | no embedding; `Deref` gives a type the `&self` methods of its target ([docs][rust-deref]) | method probing walks the deref chain ([reference][rust-method-call]) | one target per type | none | none: `Deref` grants no traits |
| Swift, Python, MoonBit | none | | | | |

Go embeds an interface value the way Kotlin delegates, as the standard
library's `sort.Reverse` shows ([source][go-sort]):

```go
type reverse struct {
	Interface
}

func (r reverse) Less(i, j int) bool {
	return r.Interface.Less(j, i)
}
```

Kotlin forwards every interface member and lets the class override some:

```kotlin
interface Describe { fun describe(): String; fun headline() = "* " + describe() }
class Logger(val name: String) : Describe { override fun describe() = name }
class Service(l: Logger) : Describe by l
```

Rust's docs say to implement `Deref` only when a value "transparently
behaves like a value of the target type", and not when methods are likely
to collide ([docs][rust-deref]).

Takeaways:

1. Go's whole embedding fits in three spec paragraphs: the field name, the
   shallowest-depth selector, and method sets. hd's extra rules come from
   value copies over reference-shaped data, the single view, declaration-time
   conflicts, limits, and explicit conformance.
2. Neither Go nor Kotlin limits width or depth. Go needs only a rule
   against a struct containing itself.
3. Go reports ambiguity at the use. hd reports it at the declaration,
   which the single view makes uniform; the record keeps that.
4. Kotlin's `by` is a forwarding method per member, with written members
   winning. hd's `by` already means this; it just says so in six rules.
5. No language promotes trait methods without granting conformance. Go
   grants it, Kotlin grants it for one interface, Rust grants neither.

## Options

Each option lists the rules it changes, the hd code it changes, and its
soundness. O1 changes no program. O2 makes a few invalid programs valid.
O3 changes the meaning of valid programs. O4 is the contrast.

| Option | Rules after (of 219) | Programs that change | Costliest change |
| --- | ---: | --- | --- |
| O1. Say each rule once | 67 | none | none: merges and deletions |
| O2. O1, and drop two invented corners | 61 | two invalid shapes become valid | semantic rule removed |
| O3. O2, and parts become ordinary references | 48 | every embedded literal and store; promoted `mut self` calls | reverses VE1-VE4, VE-S |
| O4. Remove embedding (contrast only) | about 8 | every use | removes the feature |

The 67 of O1 include 13 error-detail rules (report sites, messages, hints)
left for the error revamp, and 3 general rules that only mention
promotion. Rules a user must learn drop from about 200 to 51.

### O1. Say Each Rule Once

O1 merges restatements and deletes consequences. Three of its merges carry
most of the weight; the rest are listed in [Rule Accounting](#rule-accounting).

**O1a. A part copy is a copy-update.** Filling or storing an embedded
field `E` with `...e` stores `E { ...e }`. The copy-update rules for a
readonly source already give the result's access, so the seven
`data.edge.*` rules and the copy rules of chapters 04 and 05 merge into it.

| Before | After | Absorbed by |
| --- | --- | --- |
| `data.part.construct`, `.copy`, `.not-shared`, `.shallow`, `.readonly-source`, `.store`, `data.embed.key`, `expr.data.embedded*`, `expr.assign.embedded`, `types.fresh.embedded-copy`, `expr.update.embedded.readonly` | `data.part.construct`: `E: ...e` and `x.E ...= e` store `E { ...e }` | [Copy-Update Literals](../spec/08-data-and-enums.md#copy-update-literals) |
| `data.edge.principle`, `.copy-type`, `.readonly-literal`, `.upgrade-literal`, `.generic`, `.definition` | none; a Note says a mutable edge counts at any depth | [`data.update.readonly-source`](../spec/08-data-and-enums.md#r-data.update.readonly-source), [`data.update.mutable-result`](../spec/08-data-and-enums.md#r-data.update.mutable-result), [`types.fresh.mut-literal`](../spec/04-type-system.md#r-types.fresh.mut-literal) |
| `data.update.embedded`, `expr.update.embedded`, `expr.update.shallow.embedded` | `data.part.copy-update` | one statement |
| `data.embed.variance`, `types.polarity.embedded` | one clause of `types.polarity.mut`: an embedded field counts as beneath `mut` | [`types.polarity.mut`](../spec/04-type-system.md#r-types.polarity.mut) |

The same program is rejected for the same reason before and after; only
the rule that explains it moves:

```text
data Owner:
    name: string

data Stamp:
    at: i32
    owner: mut Owner

data Post:
    Stamp
    id: string

fn invalid(stamp: Stamp) -> void:
    let mut copy = Stamp { ...stamp }                 # error: mutable-upgrade
    let mut post = Post { Stamp: ...stamp, id: "p" }  # error: mutable-upgrade, same rule
    kept := Post { Stamp: ...stamp, id: "q" }         # valid: readonly
```

**Soundness: holds.** The copy-update `Stamp { ...stamp }` of a readonly
`stamp` is readonly when `Stamp` has a direct `mut` field. A part whose own
parts have one gets a readonly part copy, so the literal is readonly by
`types.fresh.mut-literal`. That is exactly `data.edge.copy-type`.

**O1b. A delegated method is a written forwarding method.** `by E` adds,
for each method with a receiver that the body does not write, the method
that forwards to `Trait::m(self.E, ...)`, checked as if written.

| Before | After | Absorbed by |
| --- | --- | --- |
| `trait.by.generated`, `.signature`, `.variadic`, `.mut`, `.part-receiver` | `trait.by.generated` | ordinary checking of the written method |
| `trait.by.ordinary`, `.rules`, `.candidates`, `.dot-call` | `trait.by.ordinary` | [Method Candidates](../spec/03-names-and-scopes.md#method-candidates) |
| `trait.by.assoc-fn`, `.assoc-fn.written` | `trait.by.assoc-fn`: written as in any implementation | [Implementation Declarations](../spec/09-traits.md#implementation-declarations) |
| `trait.by.direct`, `data.embed.delegation`, `trait.embed.delegate` | deleted or merged into `trait.by.form` | a field name names a direct field |

```text
trait Describe:
    fn describe(self) -> string
    fn headline(self) -> string:
        "* " + self.describe()

data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger

impl Describe for Service by Logger
```

means, with no other rule:

```text
impl Describe for Service:
    fn describe(self) -> string:
        Describe::describe(self.Logger)

    fn headline(self) -> string:
        Describe::headline(self.Logger)
```

**Soundness: holds.** The spec already defines the generated method as
the call `Trait::method(self.E, arguments...)`. `mut self` forwarding,
variadic spreads, and the part as receiver are what that call does.

**O1c. One forwarding rule for promotion.** A promoted member `x.name`
means the explicit path `x.E1...Ek.name`. Access, `mut self` calls, and
"no overriding" all follow from the path. Trait methods are never
promoted, and a promoted method never fills a trait method: each is
stated once. This answers whether trait impls and methods can share one
rule. They share the forwarding meaning: a promoted call is
`x.E.m(...)`, and a delegated one is `Trait::m(x.E, ...)`. They keep
separate triggers, because promotion is implicit for inherent methods and
delegation is written for one trait.

| Before | After |
| --- | --- |
| `names.promoted.path`, `.access`, `.mut`, `names.no-override*`, `data.embed.no-override`, `.self-call`, `data.part.access.promoted`, `types.path.promoted` | `names.promoted.path` |
| `trait.impl.fill.never`, `trait.embed.explicit`, `.no-fill`, `trait.marker.no-promotion`, `grammar.impl.promoted` | `trait.impl.fill.never` |
| `names.promote.member`, `.private`, `.no-trait`, `data.vis.promotion`, `data.promote.depth`, `.private`, `.no-trait-methods`, `trait.embed.no-promotion`, `expr.member.embedded-trait` | `names.promote.member` |
| `names.method-lookup.ambiguous`, `.no-silent`, `trait.embed.ambiguous`, `trait.by.dot-call`, `trait.resolve.ambiguous.promoted`, `data.promote.receiver-trait` | `names.method-lookup.ambiguous` |
| `data.embed.not-subtype`, `trait.embed.not-subtype`, `.not-assignable` | deleted; nominal typing already denies it |

**Soundness: holds.** Each deleted rule restates one kept rule or follows
from nominal typing and the binding rules. The five method-lookup example
rules become an unnumbered example.

### O2. Drop Two Invented Corners

O2 takes O1 and removes two rules that Go and Kotlin do not have.

**O2a. No width or depth limit; one cycle rule.** A data type may embed
any number of types at any depth. A data type that embeds itself, directly
or through other embedded fields, is an error.

| Before | After | Absorbed by |
| --- | --- | --- |
| `data.embed.width`, `data.embed.depth`, `.depth.chain`, `.every-type`, `.message`, `.generic`, `.self`, `names.part.depth.levels` | `data.embed.depth`, reworded as the cycle rule | Go's rule against a struct that contains itself ([spec][go-struct]) |

```text
data Audit:
    pub by: string

data Record:
    Audit

data Page:
    Record

data Site:
    Page                # valid after O2a; embedding-too-deep today
    host: string

data Loop:
    Loop                # still an error: an embedding cycle
```

**Soundness: holds.** The only bad program the depth limit stops for good
is a cycle, whose copy never ends. A type parameter cannot be embedded,
so a cycle is visible in declaration names.

**O2b. A private own member hides a promoted one.** The shallowest
member wins, private or not. Outside its module, the use is
`private-member`, and the promoted member is reached by its path.

| Before | After | Absorbed by |
| --- | --- | --- |
| `names.conflict.private-own`, `.private-site`, `.private-message`, `data.promote.private-own` | none | [`names.hide.depth`](../spec/03-names-and-scopes.md#r-names.hide.depth), [`names.field-lookup.private`](../spec/03-names-and-scopes.md#r-names.field-lookup.private) |

```text
data CreatedBySystem:
    pub id: string

data AuditDraft:
    CreatedBySystem
    id: string          # valid after O2b; ambiguous-promoted-member today

fn read(draft: AuditDraft) -> string:
    draft.CreatedBySystem.id
```

**Soundness: holds.** The single view stays: every module selects the own
`id`. A caller in another module gets `private-member`, never a silent
switch to the promoted `id`. The guide's `AuditDraft` example changes from
invalid to valid.

### O3. Parts Become Ordinary References

O3 takes O2 and drops value embedding. An embedded field is an ordinary
field named by its type, with promotion. It is filled `E: e`, stored
`x.E = e`, and may be declared `mut E`, as Go's `*T` embedding behaves.

| Before | After | Absorbed by |
| --- | --- | --- |
| `data.part.construct`, `.copy-update`, `.marker-required`, `.copy-time`, `.unobservable`, `data.part.suggestion` | none | ordinary field rules |
| `grammar.stmt.copy-assign`, `grammar.primary.field-copy`, the `...=` token | none | plain assignment |
| `types.path.field.embedded`, `types.path.store-embedded`, `data.part.access` | none | the readonly-edge and mutable-edge rows of [Mutable Paths](../spec/04-type-system.md#mutable-paths) |
| `data.field.embedded-no-mut`, `data.embed.unique` | none | `mut E` is a mutable edge; `duplicate-field` |
| `data.embed.member`, `grammar.primary.prefix-copies` | reworded | prefix `...` is copy-update only |

Before, today and under O1 and O2:

```text
data Post:
    Timestamps
    id: string

fn build(stamps: Timestamps) -> Post:
    let mut post = Post { Timestamps: ...stamps, id: "p" }
    post.touch(1700000100)
    post.Timestamps ...= stamps
    post
```

After O3:

```text
data Post:
    mut Timestamps      # hypothetical syntax: a mutable edge under O3
    id: string

fn build(stamps: Timestamps) -> Post:
    let mut post = Post { Timestamps: Timestamps { ...stamps }, id: "p" }
    post.touch(1700000100)
    post.Timestamps = Timestamps { ...stamps }
    post
```

**Soundness: holds, with a changed guarantee.** No bad program becomes
valid: a mutation through a readonly edge is still `readonly-edge`. But
two values may now share a part, and copy-update shares parts. The second
embedding review accepted copies on purpose (points 1, 4, 5), and
embedding exists to replace inheritance (point 7). O3 reopens that
without new evidence about its reason.

### O4. Remove Embedding (Contrast)

A named field and explicit paths, with Kotlin-style `by` on any named
field. About 211 rules leave and about 8 stay for `by`. The owner decided
to keep embedding, so this row only shows the floor.

```text
data Service:
    logger: Logger
    port: i32

impl Describe for Service by logger  # hypothetical syntax
```

### Considered And Not Proposed

| Idea | Why not |
| --- | --- |
| Report conflicts at the use, as Go does | Saves mainly report-site rules, which wait for the error revamp; the single view makes declaration checks uniform |
| Shallowest depth replaced by "any two promoting fields conflict" | Stricter than Go and changes valid programs, for about four rules |
| Promote trait methods for dot calls | Adds availability and ambiguity cases, and still grants no conformance |
| Promoted methods fill trait methods, as Go's method sets do | Reverses point 7 of the second embedding review: embedding is not conformance |
| `by` on any named field, as Kotlin allows | A generalization, not a cut: same rule count |
| `x.E = ...e` instead of `...=` | Adds a third prefix-copy position to save one token |
| Associated types written in a delegating body | Changes valid programs; real code has no delegated associated type |
| A new keyword for `by Structure` | `Structure` derivation is decided; the three disambiguation rules are cheap |

## Rule Accounting

### By Design Cost Order Kind

| Kind | Today | O1 | O1+O2 | O1+O2+O3 |
| --- | ---: | ---: | ---: | ---: |
| 1. Syntax | 34 | 15 | 15 | 10 |
| 2. Semantic | 181 | 51 | 45 | 37 |
| 3. Intrinsic | 4 | 1 | 1 | 1 |
| 4. Core library | 0 | 0 | 0 | 0 |
| **Total** | **219** | **67** | **61** | **48** |
| of which error detail left for the revamp | | 13 | 9 | 7 |
| of which general rules that mention promotion | | 3 | 3 | 3 |

O1 adds no rule. O2a rewords one rule as the cycle rule. O3 rewords two.
No option adds a syntax form, an exception, or an intrinsic.

| O1 verdict | Rules |
| --- | ---: |
| kept as is | 30 |
| kept, reworded to absorb merged rules | 21 |
| merged into another rule | 100 |
| deleted as a consequence or restatement | 41 |
| become an unnumbered example or Note | 11 |
| error detail, left for the revamp | 13 |
| unchanged general rules | 3 |
| **Total** | **219** |

Diagnostics: O1 and O2 delete none; O2a leaves one of the two limit
codes for cycles. O3 deletes `embedded-copy-required`,
`copy-into-ordinary-field`, and `mutable-embedded-field`. Fixtures: O1
changes only specification columns. O2 turns `embedding-too-deep`,
`too-many-embedded-fields`, and the private-own conflict rejects into
accepts, except the cycle case. O3 rewrites every fixture that writes
`...` on a label or `...=`.

### O1, Rule By Rule

Every one of the 219 rules has a row.

| Rules | O1 verdict |
| --- | --- |
| `data.field.embedded-no-mut`, `data.default.embedded`, `data.embed.width`, `data.embed.depth`, `names.part.definition`, `names.take-part.definition`, `names.hide.depth`, `names.conflict.definition`, `names.conflict.private-own`, `names.conflict.error`, `names.method-lookup.promoted-candidate`, `names.promoted.readonly`, `names.promoted.explicit`, `trait.by.form`, `trait.by.valid-part`, `trait.by.part-impl`, `trait.by.structure`, `trait.by.trait-less`, `trait.by.trait-less.error`, `trait.by.written`, `trait.by.assoc-types`, `trait.by.assoc-binding`, `grammar.stmt.copy-assign`, `grammar.data.embedded`, `grammar.data.embedded.no-pub`, `grammar.impl.delegation-field`, `grammar.primary.field-copy`, `grammar.primary.prefix-copies`, `types.path.field.embedded`, `annot.member.embedded` | kept |
| `data.vis.embedded-public`, `data.embed.member`, `data.embed.data-only`, `data.part.construct`, `data.part.copy-update`, `data.part.marker-required`, `data.part.copy-time`, `data.part.access`, `data.part.aliases-untracked`, `data.part.unobservable`, `data.promote.members`, `names.part.depth`, `names.promote.member`, `names.take-part.uniform`, `names.method-lookup.ambiguous`, `names.promoted.path`, `trait.embed.no-conformance`, `trait.by.generated`, `trait.by.assoc-fn`, `trait.by.ordinary`, `types.path.store-embedded` | kept, reworded to absorb the rows that merge into it |
| `data.derive.embedded` | merged into `annot.member.embedded` |
| `data.embed.metadata` | merged into `annot.target.kind.field` |
| `data.embed.named-type`, `data.embed.non-data` | merged into `data.embed.data-only` |
| `data.embed.name`, `data.embed.generic-name`, `grammar.data.embedded.generic`, `grammar.data.embedded.name` | merged into `data.embed.member` |
| `grammar.data.embedded.unique` | merged into `data.embed.unique` |
| `data.unsupported.mut-embedded` | merged into `data.field.embedded-no-mut` |
| `data.part.access.step`, `data.part.alias`, `annot.member.embedded.part` | merged into `data.part.access` |
| `data.part.owned`, `data.part.kept-references` | merged into `data.part.aliases-untracked` |
| `data.access.embedded-copy`, `data.embed.key`, `data.part.marker`, `data.part.marker-scope`, `data.part.copy`, `data.part.readonly-source`, `data.part.store`, `types.fresh.embedded-copy`, `expr.assign.embedded`, `expr.data.embedded`, `expr.data.embedded.copy`, `expr.update.embedded.readonly` | merged into `data.part.construct` |
| `data.part.copy-time.field`, `data.part.copy-time.spread` | merged into `data.part.copy-time` |
| `data.update.embedded`, `expr.update.embedded`, `expr.update.shallow.embedded` | merged into `data.part.copy-update` |
| `data.part.marker-fresh`, `data.part.missing-marker`, `data.part.plain-assignment`, `data.part.ordinary-label`, `data.part.ordinary-store`, `grammar.stmt.copy-assign.embedded`, `grammar.primary.field-copy.required` | merged into `data.part.marker-required` |
| `data.part.layout`, `data.part.elided-copy` | merged into `data.part.unobservable` |
| `data.edge.generic` | merged into `data.update.generic` |
| `data.edge.copy-type` | merged into `data.update.mutable-result` |
| `data.edge.principle` | merged into `data.update.readonly-source` |
| `module.vis.embedded` | merged into `data.vis.embedded-public` |
| `data.part.prefix`, `data.part.suffix` | merged into `grammar.primary.prefix-copies` |
| `data.vis.private-embed` | merged into `module.vis.signature.coverage` |
| `data.promote.same-depth` | merged into `names.conflict.definition` |
| `data.promote.at-declaration` | merged into `names.conflict.error` |
| `data.promote.private-own` | merged into `names.conflict.private-own` |
| `data.promote.shallowest` | merged into `names.hide.depth` |
| `data.promote.receiver-trait`, `names.method-lookup.no-silent`, `trait.embed.ambiguous`, `trait.by.dot-call`, `trait.resolve.ambiguous.promoted` | merged into `names.method-lookup.ambiguous` |
| `names.part.depth.own` | merged into `names.part.depth` |
| `data.vis.promotion`, `data.promote.depth`, `data.promote.private`, `data.promote.no-trait-methods`, `names.promote.private`, `names.promote.no-trait`, `trait.embed.no-promotion`, `expr.member.embedded-trait` | merged into `names.promote.member` |
| `names.promote.private.path` | merged into `names.promoted.explicit` |
| `data.embed.no-override`, `data.embed.self-call`, `data.part.access.promoted`, `names.promoted.access`, `names.promoted.mut`, `names.no-override`, `names.no-override.self`, `types.path.promoted` | merged into `names.promoted.path` |
| `data.part.access.readonly-promoted` | merged into `names.promoted.readonly` |
| `data.promote.uniform`, `names.take-part.visibility` | merged into `names.take-part.uniform` |
| `trait.by.assoc-fn.written` | merged into `trait.by.assoc-fn` |
| `data.embed.delegation`, `trait.embed.delegate` | merged into `trait.by.form` |
| `trait.by.generated.signature`, `trait.by.generated.variadic`, `trait.by.generated.mut`, `trait.by.part-receiver` | merged into `trait.by.generated` |
| `trait.by.ordinary.rules`, `trait.by.ordinary.candidates` | merged into `trait.by.ordinary` |
| `data.embed.no-conformance`, `names.no-conformance`, `trait.embed.no-bound` | merged into `trait.embed.no-conformance` |
| `trait.embed.explicit`, `trait.embed.no-fill`, `trait.marker.no-promotion`, `grammar.impl.promoted` | merged into `trait.impl.fill.never` |
| `data.edge.upgrade-literal` | merged into `types.fresh.expected-mut` |
| `data.edge.readonly-literal` | merged into `types.fresh.mut-literal` |
| `types.mut.embedded` | merged into `types.path.field.embedded` |
| `data.edge.upgrade-store` | merged into `types.path.store-embedded` |
| `data.embed.variance`, `types.polarity.embedded` | merged into `types.polarity.mut` |
| `data.embed.depth.chain`, `names.method-example.promoted`, `names.method-example.part`, `names.method-example.explicit`, `names.method-example.outer-trait`, `names.method-example.no-base`, `names.change.shallower`, `trait.embed.forward`, `trait.embed.bodyless`, `trait.embed.call-forms`, `trait.embed.no-promotion.example` | becomes an example or Note |
| `names.change.no-other` | deleted; kept as a Note |
| `data.edge.definition` | deleted; a Note says a mutable edge counts at any depth |
| `data.part.not-shared` | deleted: follows from a copy-update is a new value |
| `data.embed.depth.self` | deleted: follows from a cycle has unbounded depth |
| `trait.by.direct` | deleted: follows from a field name names a direct field |
| `data.embed.metadata.target` | deleted: follows from an embedded field is a field |
| `data.part.alias.let-mut`, `data.part.alias.readonly`, `data.part.alias.binding`, `data.part.alias.plain-let` | deleted: follows from binding rules, types.bind |
| `names.conflict.diamond` | deleted: follows from conflict.definition |
| `names.change.conflict` | deleted: follows from conflict.error |
| `names.field-lookup.private.alone`, `names.method-lookup.inherent.skip` | deleted: follows from conflict.private-own |
| `data.part.shallow` | deleted: follows from copy-update is shallow, expr.update.shallow |
| `trait.inherent.vs-promoted` | deleted: follows from definitions |
| `data.part.copy-time.effects` | deleted: follows from evaluation order |
| `names.hide.own-names` | deleted: follows from hide.depth |
| `names.change.candidate` | deleted: follows from method-lookup.ambiguous |
| `trait.embed.forward-mut` | deleted: follows from names.promoted.path access |
| `data.embed.not-subtype`, `trait.embed.not-subtype`, `trait.embed.not-assignable` | deleted: follows from nominal typing, data.decl.nominal |
| `annot.member.no-flatten` | deleted: follows from one member |
| `data.part.copy-sites`, `data.part.no-implicit-copy` | deleted: follows from only `...` copies |
| `names.visible.promoted`, `names.take-part.private-part`, `names.field-lookup.part-private`, `names.method-lookup.part-private` | deleted: follows from only pub members promote |
| `data.embed.metadata.not-promoted` | deleted: follows from promoted members are not fields of the outer type |
| `names.part.depth.levels` | deleted: restates data.embed.depth |
| `types.path.field.embedded.not-readonly-edge` | deleted: restates the table row |
| `names.conflict.namespace` | deleted: follows from separate namespaces, names.member.no-hiding |
| `data.embed.depth.generic` | deleted: follows from substitution, types.path.field.substituted |
| `names.take-part.caller`, `names.take-part.private-member`, `names.conflict.one-check` | deleted: follows from take-part.uniform |
| `names.take-part.trait-caller` | deleted: follows from trait availability, names.member.trait-available |
| `trait.embed.no-hiding` | deleted: follows from trait methods are not members of the outer type |
| `names.promoted.trait-qualified` | deleted: follows from trait-qualified calls, chapter 09 |
| `data.embed.unique`, `data.embed.depth.every-type`, `data.embed.depth.message`, `data.part.suggestion`, `names.conflict.promoted-site`, `names.conflict.same-field`, `names.conflict.paths-message`, `names.conflict.private-site`, `names.conflict.private-message`, `names.method-lookup.ambiguous.hint`, `names.method-lookup.hint.part-trait`, `trait.embed.unknown`, `trait.by.invalid` | error detail, left for the error revamp |
| `names.method-lookup.candidates`, `trait.impl.fill.never`, `trait.avail.as-absent` | unchanged (general rule) |

O2 changes, on top of O1:

| Rules | O2 verdict |
| --- | --- |
| `data.embed.width` | deleted (O2a) |
| `data.embed.depth` | reworded as the cycle rule (O2a) |
| `data.embed.depth.every-type`, `data.embed.depth.message` | deleted with the limit (O2a) |
| `names.conflict.private-own`, `names.conflict.private-site`, `names.conflict.private-message` | deleted (O2b); `names.hide.depth` covers it |

O3 changes, on top of O2: `data.part.construct`, `data.part.copy-update`,
`data.part.marker-required`, `data.part.copy-time`,
`data.part.unobservable`, `data.part.access`, `data.part.suggestion`,
`data.embed.unique`, `data.field.embedded-no-mut`,
`grammar.stmt.copy-assign`, `grammar.primary.field-copy`,
`types.path.field.embedded`, and `types.path.store-embedded` are deleted.
`data.embed.member` and `grammar.primary.prefix-copies` are reworded.

## What Users Lose

| Option | Loss |
| --- | --- |
| O1 | Nothing in behavior. Readers lose some local repetition: chapter 08 points to chapters 03 and 09 instead of restating them. |
| O2a | A guard against deep or wide embedding trees. Review or a lint must catch a five-level chain. |
| O2b | A private own field no longer flags a clash with a promoted field. An outside caller of `x.id` gets `private-member` and writes `x.Part.id`. |
| O3 | Part independence: two values may share a part, and a copy-update shares it. Promoted `mut self` calls need a `mut E` declaration. Readonly copies of mutable parts need an explicit `E { ...e }`. |
| O4 | Promotion and the inheritance replacement itself. |

## Recommendation

**Recommendation: O1 and O2, 219 rules to 61.**

- O1 removes 152 rules and changes no program. Its three main merges
  name existing rules as the absorbers: copy-update, forwarding calls,
  and the explicit path.
- O2 removes 6 more and matches Go: no width or depth limit, and the
  shallowest member wins even when private. Both cuts turn errors into
  valid programs, so no valid program changes meaning.
- Of the 61, 9 are error details for the revamp, so users learn about 49.
- O3 is not recommended. It saves 13 more rules, but reverses VE1-VE4 and
  VE-S, which the owner confirmed in the second embedding review. Its
  cost would land on every literal that fills a part.
- If the owner wants only no-change cuts, O1 alone gives 67.

## Questions For The Owner

Smallest first. Q1 to Q3 change no program; Q4 and Q5 make an invalid
program valid; Q6 changes valid programs.

### Q1. Delegation As Written Forwarding

`by E` is defined in six rules that restate one call. Merging them and
the nearby restatements removes 11 rules and changes nothing.

- **A.** Define each delegated method as the forwarding method
  `Trait::m(self.E, ...)`, checked as if written. *Recommended.*
- **B.** Keep the six rules.

```text
impl Describe for Service by Logger

impl Describe for Worker by Logger:
    fn headline(self) -> string:
        "worker " + Describe::headline(self.Logger)
```

### Q2. A Part Copy Is A Copy-Update

`Label: ...e` and `x.Label ...= e` would store `Label { ...e }`, so the
readonly-copy rules come from copy-update. This removes about 25 rules,
including the seven mutable edge rules.

- **A.** Merge, as O1a. *Recommended.*
- **B.** Keep mutable edges as their own section.

```text
data Post:
    Stamp
    id: string

fn copy(post: Post, stamp: Stamp) -> Post:
    Post { Stamp: ...stamp, id: post.id }
```

### Q3. State Each Rule Once

Promotion, no conformance, no overriding, and the method ambiguity are
each stated in three to ten places. Stating each once removes about 115
more rules with no change in meaning. Chapter 08 would keep a short
summary that links to chapters 03 and 09.

- **A.** Merge, as O1c and the accounting table. *Recommended.*
- **B.** Keep the restatements, which let each chapter stand alone.

```text
fn show(page: Page) -> string:
    page.Label.describe()
```

### Q4. A Private Own Member Hides

Today a private own field with a promoted field's name is an error at the
declaration. Under Go's rule the own member wins, private or not.

- **A.** The own member hides; another module gets `private-member`.
  *Recommended.*
- **B.** Keep `ambiguous-promoted-member` for a private own member.

```text
data AuditDraft:
    CreatedBySystem
    id: string
```

### Q5. Embedding Limits

At most three embedded fields and three levels are invented rules that no
real code reaches. Go has no limit and rejects only a type that contains
itself.

- **A.** Drop both limits and keep one cycle rule. *Recommended.*
- **B.** Keep the limits, stated in two rules without the five
  reporting sub-rules.
- **C.** Keep everything as is.

```text
data Post:
    Created
    Updated
    Owned
    Tagged
    title: string
```

### Q6. Value Parts Or Reference Parts

Value embedding costs 13 more rules, the `...=` token, and three
diagnostics. Reference parts would make an embedded field an ordinary
field with promotion, as Go's `*T`.

- **A.** Keep value parts, VE1-VE4 and VE-S. *Recommended:* the second
  embedding review chose copies on purpose, and nothing since contradicts
  its reason.
- **B.** Reference parts, as O3.

```text
let mut post = Post { Timestamps: ...stamps, id: "p" }
post.Timestamps ...= stamps
```

## Owner Decisions

Batch 32, 2026-09-30. Net: O1 plus O2b, with the limits kept.

| # | Decision | Status |
| --- | --- | --- |
| Q1 | A: delegation is written forwarding, `Trait::m(self.E, ...)`, checked as if written. | Applied in 32a: [`trait.by.generated`](../spec/09-traits.md#r-trait.by.generated). `trait.by.generated.variadic` stayed while varargs were under discussion, and merged into it in 33a (TUPLE-REST). |
| Q2 | A: a part copy is a copy-update, `E { ...e }`. | Applied in 32a: [`data.part.construct`](../spec/08-data-and-enums.md#r-data.part.construct), and a Note in [Mutable Edges](../spec/08-data-and-enums.md#mutable-edges). |
| Q3 | A: state each rule once (O1c and the rest of O1). | Applied in 32b: [`names.promoted.path`](../spec/03-names-and-scopes.md#r-names.promoted.path), [`names.promote.member`](../spec/03-names-and-scopes.md#r-names.promote.member), [`names.method-lookup.ambiguous`](../spec/03-names-and-scopes.md#r-names.method-lookup.ambiguous), [`trait.impl.fill.never`](../spec/09-traits.md#r-trait.impl.fill.never), and a linking summary in [Member Promotion](../spec/08-data-and-enums.md#member-promotion). |
| Q4 | A: a private own member hides a promoted one. | Applied in 32b, then reversed by PRIVATE-SHADOW (batch 33, 2026-10-01): a private own member with a promoted member's name is `ambiguous-promoted-member` again, [`names.conflict.private-shadow`](../spec/03-names-and-scopes.md#r-names.conflict.private-shadow). |
| Q5 | B, not the recommended A: keep `data.embed.width` and `data.embed.depth`. `depth.chain` becomes an example; `depth.every-type` and `depth.message` become a diagnostics Note; `depth.generic` and `names.part.depth.levels` are deleted. `data.embed.depth.self` stays, with its own code `embedding-cycle`. | Applied in 32a: [Embedding Limits](../spec/08-data-and-enums.md#embedding-limits). |
| Q6 | A: value parts stay. | Nothing to apply. |

**As applied.** Of the 219 rules, 57 remain numbered, against the 61 of
O1 and O2. The owner's limits add `data.embed.width` and
`data.embed.depth.self`. `trait.by.generated.variadic` stayed while
varargs were under discussion; 33a merged it into `trait.by.generated`. `data.embed.unique` and `trait.by.invalid` stay,
because no other rule names their codes. The other error-detail rules
became diagnostics Notes for the error revamp, so they are not counted.

## Sources

[go-struct]: https://go.dev/ref/spec#Struct_types
[go-selectors]: https://go.dev/ref/spec#Selectors
[go-method-sets]: https://go.dev/ref/spec#Method_sets
[go-effective]: https://go.dev/doc/effective_go#embedding
[go-sort]: https://go.dev/src/sort/sort.go
[kotlin-delegation]: https://kotlinlang.org/docs/delegation.html
[rust-deref]: https://doc.rust-lang.org/std/ops/trait.Deref.html
[rust-method-call]: https://doc.rust-lang.org/reference/expressions/method-call-expr.html

| Topic | Source |
| --- | --- |
| Go embedded fields, promotion, and self-containing structs | [Struct types][go-struct] |
| Go shallowest-depth selectors | [Selectors][go-selectors] |
| Go promoted methods in method sets | [Method sets][go-method-sets] |
| Go embedding in practice | [Effective Go][go-effective], [`sort.Reverse`][go-sort] |
| Kotlin interface delegation and overrides | [Delegation][kotlin-delegation] |
| Rust `Deref` guidance, and that it grants methods but no traits | [`Deref`][rust-deref] |
| Rust method probing through deref steps | [Method-call expressions][rust-method-call] |
| hd decisions VE1-VE4, VE-S, limits, single view, delegation | [Revision Notes](../spec/README.md#revision-notes) |
| hd cost evidence | [Syntax And Semantics Cost Review](SYNTAX_SEMANTICS_COST.md#r1-embedding-promotion-and-delegation) |

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts) on
2026-09-30. Parsing checks syntax only; no block is claimed to
type-check. Codes in comments are checker results the parser does not
report. Blocks that use a type without declaring it rely on the
declarations above them in the record.

| Block | Section | Result |
| ---: | --- | --- |
| 1 | Problem: today's embedding | parses |
| 2 | O1a: readonly copies | parses; the errors are checker results |
| 3 | O1b: delegation | parses |
| 4 | O1b: the written forwarding it means | parses |
| 5 | O2a: depth and cycle | parses; the cycle error is a checker result |
| 6 | O2b: private own member | parses |
| 7 | O3 before | parses |
| 8 | O3 after | `mutable-embedded-field` at line 2, the rule O3 deletes; the rest parses |
| 9 | O4 contrast | parses; `by logger` is hypothetical |
| 10 | Q1 | parses |
| 11 | Q2 | parses |
| 12 | Q3 | parses |
| 13 | Q4 | parses |
| 14 | Q5 | parses; `too-many-embedded-fields` today is a checker result |
| 15 | Q6 | parses |
