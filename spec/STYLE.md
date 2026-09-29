# Specification Style

This guide defines how the numbered chapters are written. The goal is a
reference that is precise but quick to read: one rule per sentence, the
common case first, and every rule citable by a stable ID.

[Data Types and Enums](08-data-and-enums.md) is the pilot chapter written in
this style. The other chapters are restyled one at a time, and a restyle must
not change what the chapter means.

## Section Template

Each section, or subsection, follows this order:

1. **Definition.** One sentence that says what the section defines.
2. **Example.** A minimal example of the common case.
3. **Rules.** A numbered list, one rule per item, each with a
   [rule ID](#rule-ids). A rule that names a diagnostic ends with
   "Error: `code`."
4. **Error examples.** A code block in which each rejected line ends with an
   `# error: code` comment. See [Examples](#examples).
5. **Why.** An optional rationale callout, set apart from the rules. See
   [Callouts](#callouts).
6. **See also.** Links to related sections, in place of inline
   parenthetical cross-references.

A section may add `###` or `####` subsections for separate topics. Headings
that already exist, and so their anchors, never change: the website, the
anchor checker, audits, and conformance cases link to them.

## Writing Rules

- One topic per paragraph. A paragraph has at most four sentences and about
  60 words.
- One rule per sentence, and at most 25 words per sentence.
- Use the normative vocabulary that [the specification README](README.md#normative-vocabulary)
  defines. Do not invent synonyms for it.
- Put anything enumerative in a table: limits, forms, spellings, or cases.
- Put the common case first. Obsolete forms and edge cases come last in
  their section.
- Write a defined term in bold where it is defined, and add it to the
  [glossary](README.md#glossary).
- Put a diagnostic code at the end of its rule, never in the middle of a
  sentence.
- Keep rationale out of rules. It goes in a Why callout.

The style lint in `spec/check.sh` warns about a paragraph over 90 words or a
sentence over 35 words. Those limits are a backstop; aim for the targets
above.

## Normative Vocabulary

The words **must**, **must not**, **should**, **should not**, and **may**,
and the phrases **is an error**, **is invalid**, and **is rejected**, are
defined once, in [Normative Vocabulary](README.md#normative-vocabulary).

Prefer these forms:

| Write | Instead of |
| --- | --- |
| "`X` is an error. Error: `code`." | "`X` is diagnosed as `code` because ..." |
| "`X` must be `Y`." | "`X` has to be `Y`", "`X` needs to be `Y`" |
| "`X` may be `Y`." | "`X` can be `Y`", "`X` is allowed to be `Y`" |
| "`X` is invalid." (no code named) | "`X` is not permitted" |

A restyle keeps the original verb when changing it could change the meaning.
For example, the pilot keeps "cannot" in three rules.

## Rule IDs

Every normative rule in a restyled chapter carries a **rule ID**: a stable,
unique, dotted name, like the Rust Reference's `r[items.fn.generics]`.

### Marker

A rule ID marker is `r[`, the ID, and `]`, followed by a space. It opens a
numbered list item, a paragraph, or a table cell:

```markdown
1. r[data.field.unique] Field names must be unique within the data type.
```

A marker anywhere else in prose is an error, so a marker can never hide in
the middle of a sentence. On GitHub the marker shows as plain text. The
website renders it as a small link beside the rule, whose HTML id is `r-`
followed by the ID, as in
[`r-data.embed.width`](08-data-and-enums.md#r-data.embed.width).

### Syntax And Hierarchy

An ID has two or more segments separated by dots. Each segment is lowercase
kebab-case: a letter, then letters, digits, and single hyphens.

1. The first segment is the chapter prefix from the table below.
2. The second segment names the topic, usually the section, as in
   `data.embed` or `data.enum`.
3. Further segments name the rule. A rule that refines another extends its
   ID, as in `data.part.copy-time` and `data.part.copy-time.field`.

Segments describe content, never position. An ID contains no section number
or list index, so reordering a list or a chapter renumbers nothing.

| Chapter | Prefix |
| --- | --- |
| [Lexical Structure](01-lexical-structure.md) | `lex` |
| [Grammar](02-grammar.md) | `grammar` |
| [Names and Scopes](03-names-and-scopes.md) | `names` |
| [Type System](04-type-system.md) | `types` |
| [Expressions](05-expressions.md) | `expr` |
| [Control Flow](06-control-flow.md) | `flow` |
| [Functions](07-functions.md) | `fn` |
| [Data Types and Enums](08-data-and-enums.md) | `data` |
| [Traits](09-traits.md) | `trait` |
| [Modules](10-modules.md) | `module` |
| [Requirements and Suspension](11-requirements-and-suspension.md) | `req` |
| [Variadic Generics](12-variadic-generics.md) | `pack` |
| [GADTs](13-gadts.md) | `gadt` |
| [Annotations](14-annotations.md) | `annot` |

`spec/tools/spec-prose.ts` holds the same table for the checks; change both
together.

### Stability

1. An ID never changes once it is committed. Rewording a rule, moving it
   within its chapter, or renaming a heading keeps the ID.
2. A rule whose meaning changes gets a new ID, and the old ID is retired.
   A citation of the old ID then never silently changes meaning.
3. A deleted rule's ID is retired. A rule that moves to another chapter is
   retired and gets an ID with the new chapter's prefix.
4. A retired ID is never reused, and IDs are never renumbered.
5. Every retired ID is listed under [Retired Rule IDs](#retired-rule-ids),
   with its replacement when there is one.

### Citing A Rule

- A link cites a rule by its anchor, as in
  `08-data-and-enums.md#r-data.embed.width`. The anchor checker accepts rule
  anchors wherever it accepts heading anchors.
- The `specification` column of `conformance/cases.tsv` may name a rule
  anchor instead of a section anchor.
- A fixture's `# test:` line, an audit, or an implementation's diagnostic
  text may name a rule ID, such as "see data.embed.width".

### Checks

`spec/check.sh` runs `spec/check-spec-style.ts`, which fails when an ID is
malformed, misplaced, outside a numbered chapter, without its chapter's
prefix, duplicated anywhere in the specification, or retired.

## Examples

Every `text` fence in a numbered chapter has one row in
`conformance/examples.tsv`, keyed by its position among the chapter's `text`
fences. Adding, moving, or splitting an example means updating those rows.

An **error example** is a `text` fence in which each rejected line ends with
`# error: code`, or with a bare `# error` when the chapter names no code for
that rule. A valid line may sit beside it with an ordinary comment:

```text
data User:
    mut name: string    # error: mutable-field-modifier
    mut Base            # error: mutable-embedded-field
    friend: mut User    # valid: the type grants mutable access
```

1. An error example is classified `mixed` in `examples.tsv`, and names the
   invalid fixtures that demonstrate its errors.
2. The overlap check requires most of the example's salient names to occur
   in those fixtures, so take names from the fixtures.
3. A code after `# error:` must be a code in the
   [Diagnostics](README.md#diagnostics) table.

The website shows an error example under an "Error example" tab and tints
each marked line.

## Callouts

A callout is a block quote that opens with a bold label:

```markdown
> **Why.** Access to an embedded part already follows its container.
```

1. `> **Why.**` gives the rationale for the rules before it.
2. `> **Note.**` states a consequence or gives advice.

Both are explanatory, as [Conformance Language](README.md#conformance-language)
says of notes: they add no requirement. A sentence that states a rule
belongs in the rule list, not in a callout.

## Tables

A table may hold rules. The first cell of each rule row opens with its
marker, and the website renders the table as a rule table:

```markdown
| Rule | Limit | Error | Reported on |
| --- | --- | --- | --- |
| r[data.embed.width] Width | A data type may declare at most three embedded fields. | `too-many-embedded-fields` | the fourth embedded field |
```

## Restyling A Chapter

1. Rewrite the chapter in this style, with a rule ID on every rule.
2. Update the chapter's rows in `conformance/examples.tsv`.
3. Run the rule inventory diff against the committed version:

   ```sh
   node --experimental-strip-types spec/tools/rule-inventory.ts --diff --all \
       main:spec/08-data-and-enums.md spec/08-data-and-enums.md --out /tmp/08-diff.md
   ```

4. The diff must report nothing lost: no diagnostic code, no example line,
   and no rule ID. Examples may move or split, and error examples may be
   added.
5. Compare the old and new normative content sentence by sentence. The
   report pairs each old sentence with its closest new one, and marks weak
   pairs.
6. Preserve meaning exactly. Add, drop, or change no rule. Record a genuine
   ambiguity or contradiction for the owner instead of resolving it.
7. Run `bash spec/check.sh` and `npm run website:build`.

Without `--diff`, the tool prints one chapter's inventory: its codes,
normative sentences, examples, rule IDs, and prose statistics.

## Before And After

This excerpt from [Data Declarations](08-data-and-enums.md#data-declarations)
shows the change. Before, one paragraph mixed five topics and put a code in
the middle of a sentence:

```markdown
Field names must be unique within the data type. Every field has an explicit type.
Fields have no standalone `mut` modifier. `friend: mut User` declares a field
whose type grants mutable access through that reference; `mut friend: User`
is invalid. An embedded field is written without `mut`: `Base` embeds `Base`,
and `mut Base` is a `mutable-embedded-field` error, because access to an
embedded part already follows its container
([Data Embedding](#data-embedding)).
An ordinary named field may have a default expression. ...
```

After, the topic has its own subsection, a rule list, an error example, and
a Why callout:

~~~markdown
### Fields

1. r[data.field.unique] Field names must be unique within the data type. Error: `duplicate-field`.
2. r[data.field.typed] Every field has an explicit type.
3. r[data.field.no-mut-modifier] Fields have no standalone `mut` modifier: `mut friend: User` is an error. Error: `mutable-field-modifier`.
4. r[data.field.mut-type] `friend: mut User` declares a field whose type grants mutable access through that reference.
5. r[data.field.embedded-no-mut] An embedded field is written without `mut`: `Base` embeds `Base`, and `mut Base` is an error. Error: `mutable-embedded-field`.

```text
data User:
    mut name: string    # error: mutable-field-modifier
    mut Base            # error: mutable-embedded-field
    friend: mut User    # valid: the type grants mutable access
```

> **Why.** Access to an embedded part already follows its container.
~~~

## Retired Rule IDs

Each retired ID is listed here as a bullet that starts with the ID in
backticks, followed by the date, the reason, and any replacement ID. The
style lint rejects a chapter that reuses one.

- `data.unsupported.field-blocks`: retired 2026-09-27. It restated
  `data.enum.payload.no-field-blocks`, which replaces it.
- `data.edge.upgrade`: retired 2026-09-27. The owner ruled that a store of a
  readonly copy is an error only where its target requires a mutable part.
  Replaced by `data.edge.upgrade-literal` and `data.edge.upgrade-store`.
- `grammar.expr.tuple-index`: retired 2026-09-27. Owner decision TUP-1
  spells tuple selection with an identifier, so no integer follows `.`.
  Replaced by `grammar.expr.member-identifier` and
  `grammar.expr.no-numeric-member`.
- `names.tuple-member.numeric`: retired 2026-09-27. Owner decision TUP-1
  renamed tuple members from `.0` to `._0`. Replaced by
  `names.tuple-member.underscore`.
- `names.tuple-member.not-identifiers`: retired 2026-09-27. Owner decision
  TUP-1 made tuple member names ordinary identifiers. Replaced by
  `names.tuple-member.identifier`.
- `expr.tuple.select`: retired 2026-09-27. Owner decision TUP-1 changed the
  spelling from `point.0` to `point._0`. Replaced by
  `expr.tuple.select-underscore` and
  `expr.tuple.select-underscore.identifier`.
- `expr.member.select`: retired 2026-09-27. Owner decision TUP-1 changed its
  tuple example from `tuple.0` to `tuple._0`. Replaced by
  `expr.member.select-identifier`.
- `data.shared.numeric-field`: retired 2026-09-27. Owner decision TUP-1
  spells unnamed shared parameters `_0`, `_1`, and so on. Replaced by
  `data.shared.underscore-field`.
- `trait.cmp.partial-eq`: retired 2026-09-27. Owner decision EQ-1 removed
  `PartialEq`. Replaced by `trait.cmp.equality`.
- `trait.cmp.eq`: retired 2026-09-27. Owner decision EQ-1 made `Eq` declare
  `eq` instead of being a marker. Replaced by `trait.cmp.equality` and
  `trait.cmp.reflexive`.
- `trait.cmp.partial-ord`: retired 2026-09-27. Owner decision EQ-1 made
  `Eq` the supertrait of `PartialOrd`. Replaced by
  `trait.cmp.partial-ordering`.
- `trait.cmp.ord`: retired 2026-09-27. Owner decision EQ-1 made
  `PartialOrd` the only supertrait of `Ord`. Replaced by
  `trait.cmp.total-ordering`.
- `trait.cmp.module`: retired 2026-09-27. Owner decision EQ-1 removed
  `PartialEq` from `std.cmp`. Replaced by `trait.cmp.std-module`.
- `trait.cmp.agree`: retired 2026-09-27. Owner decision EQ-1 left no
  partial equality to agree with. Replaced by `trait.cmp.ord-agree`.
- `trait.cmp.float`: retired 2026-09-27. Owner decision EQ-1 made floats
  implement `Eq`. Replaced by `trait.cmp.float-eq` and
  `trait.cmp.float-ord`.
- `trait.derive.intrinsic`: retired 2026-09-27. Owner decision TQ-11 allows
  `@derive` on newtypes. Replaced by `trait.derive.intrinsic-decl`.
- `trait.derive.consistency`: retired 2026-09-27. Owner decision TQ-12
  forbids mixing derived and hand-written law partners. Replaced by
  `trait.derive.partners.no-mix`.
- `trait.derive.eq.fields`: retired 2026-09-27. Owner decision EQ-1 derives
  `Eq` instead of `PartialEq`. Replaced by `trait.derive.eq.compare-fields`.
- `trait.inherent.unique`: retired 2026-09-27. Owner decision TQ-19 lets
  inherent implementations with non-unifying targets repeat a name.
  Replaced by `trait.inherent.unique-unifying`.
- `types.map-key.float-eq`: retired 2026-09-27. Owner decision EQ-1 made
  floats implement `Eq`. Replaced by `types.map-key.float-no-hash`.
- `expr.eq.partial-eq`: retired 2026-09-27. Owner decision EQ-1 removed
  `PartialEq`. Replaced by `expr.eq.calls-eq`.
- `module.testing.partial-eq`: retired 2026-09-27. Owner decision EQ-1
  removed `PartialEq`. Replaced by `module.testing.uses-eq`.
- `data.shared.value`: retired 2026-09-27. Enum semantics decision 4 stores
  shared data per variant, not in each value. Replaced by
  `data.shared.value-read` and `data.shared.not-stored`.
- `data.shared.assignment`: retired 2026-09-27. Enum semantics decision 2
  made shared data read-only. Replaced by `data.shared.read-only`.
- `data.shared.default.eval`: retired 2026-09-27. Enum semantics decision 4
  evaluates shared data once per variant at compile time. Replaced by
  `data.shared.default.eval-once` and `data.shared.compile-time`.
- `expr.is.shared-data`: retired 2026-09-27. Enum semantics decision 4
  keeps shared data out of enum values, so it gives no identity. Replaced
  by `expr.is.shared-data-canonical`.
- `types.map-key.builtin`: retired 2026-09-27. Enum semantics decision 3
  gives payload-free enums no automatic `Hash`. Replaced by
  `types.map-key.builtin-types`.
- `types.map-key.user-impl`: retired 2026-09-27. Enum semantics decision 3
  covers every user enum. Replaced by `types.map-key.user-enums`.
- `trait.dyn.safe.method-params`: retired 2026-09-27. The owner revised
  TQ-10 to allow row parameters on dynamically safe traits. Replaced by
  `trait.dyn.safe.reified-or-pack` and `trait.dyn.safe.row-parameter`.
- `types.trait.safe.no-specialized-params`: retired 2026-09-27. The owner
  revised TQ-10 to allow row parameters. Replaced by
  `types.trait.safe.no-reified-or-pack`.
- `trait.dyn.safe.anyref`: retired 2026-09-27. The owner's TQ-10 revision
  exempts row parameters from the `AnyRef` bound. Replaced by
  `trait.dyn.safe.anyref-type-param`.
- `types.trait.safe.method-generic`: retired 2026-09-27. The owner's TQ-10
  revision exempts row parameters from the `AnyRef` bound. Replaced by
  `types.trait.safe.method-type-param`.
- `types.sealed.anyval`: retired 2026-09-27. Owner decision VC-1 adds
  `void` to `AnyVal`. Replaced by `types.sealed.anyval-types`.
- `trait.sealed.anyval`: retired 2026-09-27. Owner decisions VC-1 and VC-2
  add `void` and newtypes over `AnyVal` types. Replaced by
  `trait.sealed.anyval-types`.
- `module.prelude.anyval`: retired 2026-09-27. Owner decisions VC-1 and
  VC-2 add `void` and newtypes. Replaced by `module.prelude.anyval-types`.
- `req.determinism.hash`: retired 2026-09-27. Durable replay decision 8
  seeds the standard `Hasher` from the code identity and runtime profile.
  Replaced by `req.determinism.hash-seeded`.
- `trait.derive.hash.unstable`: retired 2026-09-27. Durable replay
  decision 8 makes standard hash values stable within one code identity and
  runtime profile. Replaced by `trait.derive.hash.seeded`.
- `data.repr.deferred`: retired 2026-09-27. Durable replay decision 12
  confines weak references and finalizers to the standard runtime. Replaced
  by `data.repr.runtime-only`.
- `trait.own.inherent.targets`: retired 2026-09-27. Standard library
  decision 8 lets `std` declare inherent implementations for primitives.
  Replaced by `trait.own.inherent.target-kinds` and `trait.own.inherent.std`.
- `trait.own.module.inherent`: retired 2026-09-27. Standard library
  decision 8 lets `std` declare such implementations in any of its modules.
  Replaced by `trait.own.module.inherent-target` and
  `trait.own.module.inherent.std`.
- `types.forms`: retired 2026-09-27. FN_TYPE owner decision 2 removed the
  mutable function type form. Replaced by `types.forms.set`.
- `types.fn.structural`: retired 2026-09-27. FN_TYPE owner decisions 1 and
  2 made function types constructor applications without mutability.
  Replaced by `types.fn.constructor` and `types.fn.same`.
- `types.fn.invariant`: retired 2026-09-27. FN_TYPE owner decision 5
  declares function variance. Replaced by `types.fn.declared-variance` and
  `types.variance.function`.
- `types.fn.no-variance`: retired 2026-09-27. FN_TYPE owner decision 5
  allows standalone function-type variance conversions. Replaced by
  `types.variance.function`.
- `types.fn.container`: retired 2026-09-27. FN_TYPE owner decision 5 made
  the container-view special case an ordinary variance conversion. Replaced
  by `types.variance.function`.
- `types.fn.container.change`: retired 2026-09-27. FN_TYPE owner decision 5.
  Replaced by `types.variance.function` and `fn.type.variance-repr`.
- `types.fn.container.access`: retired 2026-09-27. FN_TYPE owner decision 5.
  Replaced by `types.variance.function`.
- `names.capture.mutation`: retired 2026-09-27. FN_TYPE owner decision 2
  removed the function type's part in capture mutation. Replaced by
  `names.capture.mutation-access`.
- `expr.is.closure`: retired 2026-09-27. FN_TYPE owner decision 9 made
  function identity unspecified. Replaced by `expr.is.function-unspecified`
  and `expr.is.function-sharing`.
- `fn.local.capture`: retired 2026-09-27. FN_TYPE owner decision 2 removed
  readonly capture. Replaced by `fn.local.capture-rules`.
- `fn.type.parts`: retired 2026-09-27. FN_TYPE owner decision 2 removed
  function mutability. Replaced by `fn.type.signature-parts`.
- `fn.type.invariant`: retired 2026-09-27. FN_TYPE owner decision 5
  declares function variance. Replaced by `fn.type.declared-variance`.
- `fn.type.exact`: retired 2026-09-27. FN_TYPE owner decision 5 allows
  permission changes. Replaced by `fn.type.variance-repr`.
- `fn.type.no-coercion`: retired 2026-09-27. FN_TYPE owner decision 5.
  Replaced by `fn.type.variance-repr.excluded`.
- `fn.capture.plain.read-only`: retired 2026-09-27. FN_TYPE owner decision
  2 lets closures mutate captures. Replaced by `fn.capture.access` and
  `fn.capture.mutate`.
- `fn.capture.plain.view`: retired 2026-09-27. FN_TYPE owner decision 2.
  Replaced by `fn.capture.access`.
- `fn.capture.mut-fn-required`: retired 2026-09-27. FN_TYPE owner decision
  2 removed `mut fn`. Replaced by `fn.capture.mutate`.
- `fn.capture.mut-fn-required.method`: retired 2026-09-27. FN_TYPE owner
  decision 2. Replaced by `fn.capture.mutate.forms`.
- `fn.capture.plain.error`: retired 2026-09-27. FN_TYPE owner decision 2
  removed `mutable-capture-requires-mut-fn`. No replacement.
- `fn.capture.plain.error.argument`: retired 2026-09-27. FN_TYPE owner
  decision 2. Replaced by `fn.capture.mutate.forms`.
- `fn.capture.return-mut`: retired 2026-09-27. FN_TYPE owner decision 2.
  Replaced by `fn.capture.mutate.forms`.
- `fn.capture.call-needs-mut`: retired 2026-09-27. FN_TYPE owner decision 2
  removed mutable function access. Replaced by `fn.type.no-permission`.
- `fn.capture.mut-fn.type`: retired 2026-09-27. FN_TYPE owner decision 2
  removed `mut fn`. Replaced by `fn.capture.no-mut-form`.
- `fn.capture.mut-fn.call`: retired 2026-09-27. FN_TYPE owner decision 2.
  Replaced by `fn.type.no-permission`.
- `trait.target.function`: retired 2026-09-27. FN_TYPE owner decision 6
  made function types implementation targets. Replaced by
  `trait.target.function-type` and `trait.target.function-type.valid`.
- `expr.try.target`: retired 2026-09-27. Error conversion decision 14 makes
  a `test` block a propagation target. Replaced by
  `expr.try.target.nearest` and `expr.try.target.test`.
- `expr.try.target.top-level`: retired 2026-09-27. Error conversion
  decision 14 makes a `test` block a propagation target. Replaced by
  `expr.try.target.module-top-level` and `expr.try.target.test`.
- `module.entry.err-render`: retired 2026-09-27. Error conversion decisions
  13 and 17 print an `Error` chain and let `ExitStatus` choose the status.
  Replaced by `module.entry.err-render-chain`,
  `module.entry.err-render-display`, and `module.entry.exit-status.default`.
- `fn.type.generic.instantiate`: retired 2026-09-27. Error conversion
  decision 19 lets a call argument infer its type arguments. Replaced by
  `fn.type.generic.instantiate-every` and
  `fn.type.generic.instantiate-sources`.
- `data.enum.fn-value.generic`: retired 2026-09-27. Error conversion
  decision 19 lets a call argument infer its type arguments. Replaced by
  `data.enum.fn-value.generic-rule` and
  `data.enum.fn-value.generic-argument`.
- `lex.op.tokens`: retired 2026-09-27. Typed derivation (M3) adds the
  `+=` token for member lines. Replaced by `lex.op.token-list` and
  `lex.op.plus-equals`.
- `grammar.impl.delegation`: retired 2026-09-27. Typed derivation (M8)
  makes `by Structure` a derivation, not a delegation. Replaced by
  `grammar.impl.delegation-field` and `grammar.impl.by-structure`.
- `grammar.fn.decorator`: retired 2026-09-27. Typed derivation (M20 R3-9)
  and Error Conversion decision 12 let payload parameters carry decorators.
  Replaced by `grammar.fn.decorator-targets` and
  `grammar.enum.payload-decorator`.
- `trait.impl.generics.no-change`: retired 2026-09-27. Typed derivation
  (M9) lets walkers, describers, and sources strengthen their member bound.
  Replaced by `trait.impl.generics.fixed-bounds`.
- `trait.sealed.use`: retired 2026-09-27. Typed derivation (M8) lets only a
  template name `Structure`. Replaced by `trait.sealed.use-positions`.
- `trait.by.valid`: retired 2026-09-27. Typed derivation (M8) makes
  `by Structure` a derivation. Replaced by `trait.by.valid-part` and
  `trait.by.structure`.
- `flow.panic.categories`: retired 2026-09-27. Typed derivation (M14) adds
  the `structure-variant-mismatch` category. Replaced by
  `flow.panic.category-list`.
- `trait.own.annotate.root`: retired 2026-09-27. The owner dropped the
  root-application orphan exception. Replaced by
  `trait.own.annotate.no-orphan`.
- `trait.own.annotate.not-impl`: retired 2026-09-27. With no orphan
  exception, it has nothing to restrict. Replaced by
  `trait.own.annotate.no-orphan`.
- `grammar.type.row.mut-key`: retired 2026-09-27. The requirement access
  decision removed the `mut` key prefix. Replaced by
  `grammar.type.row.no-mut-key`.
- `module.entry.row.mut`: retired 2026-09-27. The requirement access
  decision removed `mut` entry-row keys. Replaced by `req.mut.no-spelling`.
- `module.profile.access`: retired 2026-09-27. The requirement access
  decision derives provider access from the trait. Replaced by
  `module.profile.trait-access`.
- `module.profile.mut-key`: retired 2026-09-27. The requirement access
  decision removed `mut` entry-row keys. Replaced by `req.mut.no-spelling`.
- `module.register.contract`: retired 2026-09-27. The requirement access
  decision removed the access a contract declared. Replaced by
  `module.register.contract-traits`.
- `req.row.syntax.mut-key`: retired 2026-09-27. The requirement access
  decision removed the `mut` key prefix. Replaced by
  `req.row.syntax.no-mut`.
- `req.use.value.readonly`: retired 2026-09-27. The requirement access
  decision derives provider access from the trait. Replaced by
  `req.use.value.ordinary` and `req.use.value.access`.
- `req.use.value.mut`: retired 2026-09-27. The requirement access decision
  removed `$.use(mut K)`. Replaced by `req.use.value.access`.
- `req.use.registered-profile`: retired 2026-09-27. The requirement access
  decision removed the access a contract declared. Replaced by
  `req.use.registered-profile-set`.
- `req.mut.access`: retired 2026-09-27. The requirement access decision
  derives provider access from the trait. Replaced by
  `req.mut.trait-access`.
- `req.mut.not-identity`: retired 2026-09-27. The requirement access
  decision removed the `mut` key prefix. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.compare`: retired 2026-09-27. The requirement access decision
  removed the `mut` key prefix. Replaced by `req.mut.row.no-access-rules`.
- `req.mut.install`: retired 2026-09-27. The requirement access decision
  removed `mut K=expression` bindings. Replaced by
  `req.mut.install-mutable`.
- `req.mut.install.type`: retired 2026-09-27. The requirement access
  decision ties the mutable install to the trait. Replaced by
  `req.mut.install-mutable`.
- `req.mut.install.readonly`: retired 2026-09-27. The requirement access
  decision ties installed access to the trait. Replaced by
  `req.mut.install-readonly-trait`.
- `req.mut.install.replace`: retired 2026-09-27. The requirement access
  decision left no per-binding access to replace. No replacement.
- `req.mut.use`: retired 2026-09-27. The requirement access decision removed
  `$.use(mut K)`. Replaced by `req.mut.use-mutable`.
- `req.mut.use.readonly`: retired 2026-09-27. The requirement access
  decision ties retrieved access to the trait. Replaced by
  `req.mut.use-readonly-trait`.
- `req.mut.use.upgrade`: retired 2026-09-27. The requirement access decision
  left no readonly provider of a mutable trait. No replacement.
- `req.mut.row.entry`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. Replaced by `req.mut.row.plain`.
- `req.mut.row.normalize`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. Replaced by `req.mut.row.plain`.
- `req.mut.row.satisfy`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. Replaced by `req.mut.row.plain`.
- `req.mut.row.upgrade`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. No replacement.
- `req.mut.row.missing`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. No replacement.
- `req.mut.row.inferred`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. No replacement.
- `req.mut.row.trait`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. Replaced by `req.mut.row.plain`.
- `req.mut.removal.normalized`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.removal.mut`: retired 2026-09-27. The requirement access decision
  removed `mut` row entries. Replaced by `req.mut.row.no-access-rules`.
- `req.mut.removal.readonly`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.removal.keeps-mut`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.removal.not-removed`: retired 2026-09-27. The requirement access
  decision removed `mut` row entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.context.row`: retired 2026-09-27. The requirement access decision
  removed `mut` context entries. Replaced by `req.mut.row.no-access-rules`.
- `req.mut.context.binding`: retired 2026-09-27. The requirement access
  decision removed `mut` context entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.context.spread`: retired 2026-09-27. The requirement access
  decision removed `mut` context entries. Replaced by
  `req.mut.row.no-access-rules`.
- `req.mut.entry.readonly`: retired 2026-09-27. The requirement access
  decision removed profile mutable markings. Replaced by
  `req.mut.entry.trait-access`.
- `req.mut.entry.mutable`: retired 2026-09-27. The requirement access
  decision removed profile mutable markings. Replaced by
  `req.mut.entry.trait-access`.
- `req.mut.entry.row`: retired 2026-09-27. The requirement access decision
  removed `mut` entry-row keys. Replaced by `req.mut.no-spelling`.
- `req.mut.entry.registered`: retired 2026-09-27. The requirement access
  decision removed the access a contract declared. Replaced by
  `req.mut.entry.registered-access`.
- `req.mut.entry.other`: retired 2026-09-27. The requirement access decision
  removed `mut` entry-row keys. Replaced by `req.mut.no-spelling`.
- `req.mut.entry.plain`: retired 2026-09-27. The requirement access decision
  derives provider access from the trait. Replaced by
  `req.mut.entry.trait-access`.
- `req.mut.capture`: retired 2026-09-27. The requirement access decision
  derives captured access from the trait. Replaced by
  `req.mut.capture-trait`.
- `annot.block.by-structure`: retired 2026-09-27. Typed derivation (M23)
  requires `use std.structure.Structure` for `by Structure`. Replaced by
  `annot.block.structure-use` and `annot.block.not-delegation`.
- `annot.structure.template-only`: retired 2026-09-27. Typed derivation
  (M23) lets the `use` declaration name `Structure`. Replaced by
  `annot.structure.named-positions`.
- `annot.structure.template-only.error`: retired 2026-09-27. Its parent
  rule gained the `use` position. Replaced by
  `annot.structure.named-positions.error`.
- `trait.sealed.use-positions`: retired 2026-09-27. Typed derivation (M23)
  and the block-header change let code outside a template name
  `Structure`. Replaced by `trait.sealed.use-positions-structure`.
- `annot.walker.generic-call`: retired 2026-09-27. Typed derivation (M23)
  lets source code call `missing` through a generic source. Replaced by
  `annot.walker.generic-member-call` and `annot.walker.generic-missing`.
- `module.entry.exit-status.trait`: retired 2026-09-27. Error entry-point
  follow-ups question 1 made `status()` return `StatusCode`. Replaced by
  `module.entry.exit-status.trait-code`, `module.entry.status-code`, and
  `module.entry.status-code.new`.
- `module.entry.exit-status`: retired 2026-09-27. Error entry-point
  follow-ups question 1 exits with the `u8` a `StatusCode` holds. Replaced by
  `module.entry.exit-status.code`.
- `lex.contextual.annotation`: retired 2026-09-27. Typed derivation decision
  10 removed the facet protocol, so `annotation` and `annotation_ref` are no
  longer contextual words. No replacement.
- `grammar.primary.annotation-access`: retired 2026-09-27. Typed derivation
  decision 10 removed the facet protocol and `annotation_runtime_access`. No
  replacement.
- `grammar.closed.headers`: retired 2026-09-27. Typed derivation decision 10
  removed the facet protocol, so no annotation facet precedes `for`.
  Replaced by `grammar.closed.header-positions`.
- `grammar.annot.facet-type`: retired 2026-09-27. Typed derivation decision
  10 removed the facet protocol and its facet operand. No replacement.
- `grammar.annot.facet-expression`: retired 2026-09-27. Typed derivation
  decision 10 removed the facet protocol and its facet operand. No
  replacement.
- `grammar.annot.facet-resolution`: retired 2026-09-27. Typed derivation
  decision 10 removed the facet protocol and its facet operand. No
  replacement.
- `grammar.annot.generic-bracket`: retired 2026-09-27. Only member metadata
  blocks remain, and they take no generic parameters. No replacement.
- `grammar.annot.generic`: retired 2026-09-27. Only member metadata blocks
  remain, and they take no generic parameters. No replacement.
- `grammar.annot.generic-target`: retired 2026-09-27. Only member metadata
  blocks remain, and they take no generic parameters. No replacement.
- `grammar.annot.coherence`: retired 2026-09-27. Only member metadata blocks
  remain, and they take no generic parameters. No replacement.
- `flow.panic.category-list`: retired 2026-09-27. Typed derivation decision
  10 removed the facet protocol and its two annotation panic categories.
  Replaced by `flow.panic.category-set`.
- `trait.derive.no-annotate`: retired 2026-09-27. Typed derivation decision
  10 removed the facet protocol, so there is no `Annotate` conformance to
  exclude. No replacement.
- `trait.own.annotate`: retired 2026-09-27. Typed derivation decision 10
  removed the facet protocol and `annotate Facet for Target`. No
  replacement.
- `trait.own.annotate.owners`: retired 2026-09-27. Typed derivation decision
  10 removed the facet protocol and `annotate Facet for Target`. No
  replacement.
- `trait.own.annotate.no-orphan`: retired 2026-09-27. No annotations remain
  to own. Replaced by `trait.own.no-orphan-exception`.
- `annot.fact.type-level`: retired 2026-09-27. With no facets, every
  decorator before a data or enum declaration attaches a fact. Replaced by
  `annot.fact.type-level-decorator`.
- `req.drive.block-on.forbidden`: retired 2026-09-27. Typed derivation
  decision 10 removed annotation builders, one of its contexts. Replaced by
  `req.drive.block-on.forbidden-contexts`.
- `names.local.annotations-global`: retired 2026-09-27. Typed derivation
  decision 10 removed annotation coherence, initialization, and memoization.
  No replacement.
- `types.shape.annotations`: retired 2026-09-27. Typed derivation decision
  10 removed annotation lookup. No replacement.
- `module.entry.result`: retired 2026-09-27. Testing T5 makes the entry
  result an ordinary `Termination` bound. Replaced by
  `module.entry.result-termination`.
- `module.entry.err`: retired 2026-09-27. Testing T8 exits with the code
  that `report()` returns. Replaced by `module.entry.exit-report`.
- `module.entry.err-display`: retired 2026-09-27. Testing T5 makes the entry
  result an ordinary `Termination` bound. Replaced by
  `module.entry.result-termination`.
- `module.entry.exit-status.trait-code`: retired 2026-09-27. Testing T8
  replaces `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`.
  Replaced by `module.entry.termination`.
- `module.entry.status-code`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. Replaced
  by `module.entry.exit-code`.
- `module.entry.status-code.new`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. Every
  `u8` is a valid `ExitCode`. No replacement.
- `module.entry.exit-status.code`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. Replaced
  by `module.entry.exit-report` and `module.entry.termination.err`.
- `module.entry.exit-status.static`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. No error
  type chooses a code. No replacement.
- `module.entry.exit-status.default`: retired 2026-09-27. Testing T8
  replaces `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`.
  Replaced by `module.entry.termination.err`.
- `module.entry.exit-status.code-only`: retired 2026-09-27. Testing T8
  replaces `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`.
  No error type chooses a code. No replacement.
- `module.entry.exit-status.import`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. Replaced
  by `module.entry.process-import`.
- `module.entry.status-code.import`: retired 2026-09-27. Testing T8 replaces
  `ExitStatus` and `StatusCode` with `ExitCode` and `Termination`. Replaced
  by `module.entry.process-import`.
- `module.testing.propagated-error`: retired 2026-09-27. Testing T4 makes a
  test body follow the `Termination` rule. Replaced by
  `module.testing.result-report`.
- `expr.try.test.result`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. Replaced by `expr.try.test.nearest` and
  `expr.try.test.inferred`.
- `expr.try.test.erased`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. The block no longer returns the erased
  `Error`. No replacement.
- `expr.try.test.convert`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. Replaced by `expr.try.test.inferred`.
- `expr.try.test.display`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. No error is wrapped. No replacement.
- `expr.try.test.display.only`: retired 2026-09-27. Testing T4 makes a test
  body follow the `Termination` rule. No error is wrapped. No replacement.
- `expr.try.test.not-display`: retired 2026-09-27. Testing T4 makes a test
  body follow the `Termination` rule. Replaced by
  `expr.try.test.termination`.
- `expr.try.test.optional`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. Replaced by `expr.try.test.termination`.
- `expr.try.test.fail`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. Replaced by `expr.try.test.fail-report`.
- `grammar.test.outcome`: retired 2026-09-27. Testing T4 makes a test body
  follow the `Termination` rule. Replaced by `grammar.test.outcome-report`.
- `flow.must-use.suite-final`: retired 2026-09-27. Testing T4 makes a test
  body's final expression its result, so it is no longer discarded.
  Replaced by `flow.must-use.discarded-suite` and
  `flow.must-use.test-result`.
- `lex.keyword.reserved`: retired 2026-09-27. Testing T17 adds `tests` to
  the reserved words. Replaced by `lex.keyword.reserved-list` and
  `lex.keyword.tests`.
- `lex.contextual.test`: retired 2026-09-27. Testing T3 and T16 removed the
  `test "name":` item, so `test` is an ordinary identifier. Replaced by
  `lex.keyword.tests`.
- `grammar.test.name`: retired 2026-09-27. Testing T3, T16, and T17 replace
  the `test "name":` item with a `tests:` block and `it(...)` calls.
  Replaced by `grammar.tests.block` and `module.testing.it.name`.
- `grammar.test.body`: retired 2026-09-27. Replaced by
  `module.testing.it.body`.
- `grammar.test.tooling`: retired 2026-09-27. Replaced by
  `module.testing.it`.
- `grammar.test.contextual`: retired 2026-09-27. Replaced by
  `grammar.tests.keyword`.
- `grammar.test.not-in-suites`: retired 2026-09-27. Replaced by
  `grammar.tests.top-level` and `module.testing.it.elsewhere`.
- `grammar.test.instance`: retired 2026-09-27. Moved to Standard Testing as
  `module.testing.instance` and `module.testing.driven`.
- `grammar.test.no-reuse`: retired 2026-09-27. Moved to Standard Testing as
  `module.testing.no-reuse`.
- `grammar.test.outcome-report`: retired 2026-09-27. Testing T19 makes a
  failed assertion a panic. Replaced by `module.testing.pass` and
  `module.testing.fail`.
- `names.scope.test`: retired 2026-09-27. A test body is now a closure with
  an ordinary local scope. Replaced by the `names.tests` rules for `tests:`
  blocks.
- `names.scope.test.isolated`: retired 2026-09-27. Replaced by
  `names.tests.inside-only`.
- `expr.try.target.nearest`: retired 2026-09-27. Testing T16 removed the
  `test` block as a propagation target. Replaced by
  `expr.try.target.nearest-function`.
- `expr.try.target.test`: retired 2026-09-27. Replaced by
  `expr.try.target.test-body`.
- `expr.try.test.nearest`: retired 2026-09-27. A test body is a closure, so
  it is the nearest function by the ordinary rules.
- `expr.try.test.inferred`: retired 2026-09-27. Testing T15 fixes a trailing
  test body's result instead of inferring it. Replaced by
  `expr.try.test.fixed-result`, `expr.try.test.with-try`, and
  `expr.try.test.without-try`.
- `expr.try.test.termination`: retired 2026-09-27. Replaced by
  `expr.try.test.explicit-closure`.
- `expr.try.test.fail-report`: retired 2026-09-27. Testing T18 moves the
  printing to the runner. Replaced by `module.testing.fail` and
  `module.testing.err-print`.
- `flow.must-use.test-result`: retired 2026-09-27. A test body is now a
  closure, whose final value is its result by the ordinary rules.
- `flow.return.script`: retired 2026-09-27. A test body is now a closure, so
  `return` inside it completes the body. Replaced by
  `flow.return.script-only`.
- `req.suspend.closure.inference`: retired 2026-09-27. Testing T14 makes a
  trailing block for an `fn!` parameter suspending. Replaced by
  `req.suspend.closure.row-inference`, `req.suspend.closure.trailing-only`,
  and `fn.trailing.suspending`.
- `req.bang.driver-context`: retired 2026-09-27. The `test` block is no
  longer its own driver context. Replaced by `req.bang.driver-contexts`.
- `req.bang.test-active`: retired 2026-09-27. Replaced by
  `req.bang.test-driven`.
- `module.testing.failure`: retired 2026-09-27. Testing T19 makes every
  failed assertion an `assertion-failed` panic. Replaced by
  `module.testing.assert-panic`.
- `module.testing.panic`: retired 2026-09-27. Replaced by
  `module.testing.assert-panic`.
- `module.testing.result-report`: retired 2026-09-27. Replaced by
  `module.testing.pass` and `module.testing.fail`.
- `module.init.test-bodies`: retired 2026-09-27. Test cases are `it` calls,
  which also sit at the top level of test modules. Replaced by
  `module.init.test-cases`.
- `module.testing.it.statements`: retired 2026-09-27. Testing T36 admits
  `it_prop` and `it_prop_with` calls too. Replaced by
  `module.testing.it.statement-calls`.
- `module.entry.termination.err`: retired 2026-09-27. Testing T18 has the
  host print the error. Replaced by `module.entry.termination.err-code` and
  `module.entry.termination.no-print`.
- `fn.default.order`: retired 2026-09-28. Testing T40 lets a final
  function-typed parameter follow defaulted parameters. Replaced by
  `fn.default.order-final-function`.
- `module.prelude.it`: retired 2026-09-28. Testing T40 made `it` an
  ordinary function, not an intrinsic. Replaced by
  `module.prelude.it-function`.
- `module.testing.it`: retired 2026-09-28. Testing T40 made `it` an
  ordinary function, not an intrinsic. Replaced by
  `module.testing.it-function`.
- `module.testing.it.statement-calls`: retired 2026-09-28. Testing T45 and
  T47 name integration test modules as test position too. Replaced by
  `module.testing.test-position` and `module.testing.position-statements`.
- `module.testing.it.elsewhere`: retired 2026-09-28. Testing T47 covers all
  four test-case functions and uses as values. Replaced by
  `module.testing.direct-call`.
- `module.testing.it-each.body`: retired 2026-09-28. Testing T41 puts the
  options before the body, so the body is passed by name. Replaced by
  `module.testing.it-each.body-closure`.
- `module.testing.it.options`: retired 2026-09-28. With literal suffixes
  (L1-L9), `timeout` takes a suffixed literal instead of a string. Replaced
  by `module.testing.it.options-literal`.
- `module.testing.option.timeout`: retired 2026-09-28. The timeout value is
  a `Duration` written as a suffixed literal, such as `5s`, instead of the
  string `"5s"`. Replaced by `module.testing.option.timeout-duration`.
- `annot.walker.strengthen`: retired 2026-09-28. Typed derivation (M24)
  forbids strengthening the bound on `missing[F]`. Replaced by
  `annot.walker.strengthen-member`, `annot.walker.missing-fixed`, and
  `annot.walker.missing-fixed.error`.
- `lex.suffix.radix`: retired 2026-09-28. Literal Suffixes L12 removed the
  `'` form, so radix literals take no suffix. Replaced by
  `lex.suffix.no-radix` and `lex.suffix.no-quote`.
- `lex.suffix.longest`: retired 2026-09-28. Literal Suffixes L13 makes a
  reserved-word suffix, as in `5else`, an `invalid-token`. Replaced by
  `lex.suffix.longest-match` and `lex.suffix.reserved`.
- `expr.suffix.plain-call`: retired 2026-09-28. Literal Suffixes L15 gives
  a suffixed literal no evaluation rule of its own. Replaced by
  `expr.suffix.exact-call` and `expr.suffix.position-rules`.
- `expr.suffix.compile-time`: retired 2026-09-28. Literal Suffixes L15
  removed the compile-time rule for suffixed literals. Replaced by
  `expr.suffix.position-rules`.
- `expr.suffix.std.ns`: retired 2026-09-28. Literal Suffixes L17 dropped
  the `ns` suffix; `Duration` holds milliseconds. No replacement.
- `expr.suffix.std.us`: retired 2026-09-28. Literal Suffixes L17 dropped
  the `us` suffix. No replacement.
- `expr.suffix.std.only`: retired 2026-09-28. Literal Suffixes L17 left
  four standard suffixes. Replaced by `expr.suffix.std.only-four`.
- `module.prelude.time`: retired 2026-09-28. Literal Suffixes L17 dropped
  `ns` and `us` from `std.time`. Replaced by `module.prelude.time-suffixes`.
- `module.testing.it.options-literal`: retired 2026-09-28. Literal Suffixes
  L16 lets `timeout` take any `Duration` value. Replaced by
  `module.testing.it.options-strings`.
- `module.testing.option.timeout-duration`: retired 2026-09-28. Literal
  Suffixes L16 lets `timeout` take any `Duration` value, not only a
  suffixed literal. Replaced by `module.testing.option.timeout-any-duration`.
- `module.testing.option.timeout-compile-time`: retired 2026-09-28. Literal
  Suffixes L16 evaluates a `timeout` value when the test case runs.
  Replaced by `module.testing.option.timeout-at-run`.
- `annot.fact.unused`: retired 2026-09-28. Typed derivation (M25) warns
  only for a fact whose type comes from a package other than `std`.
  Replaced by `annot.fact.unused-non-std` and `annot.fact.unused-std`.
- `lex.keyword.reserved-list`: retired 2026-09-28. Typed derivation M26
  removed the `annotate` block, so `annotate` is no longer reserved.
  Replaced by `lex.keyword.reserved-words`.
- `lex.contextual.by`: retired 2026-09-28. Typed derivation M26 allows `by`
  after the target of a header without a trait. Replaced by
  `lex.contextual.by-header`.
- `grammar.suite.top-level-only`: retired 2026-09-28. Typed derivation M26
  removed annotation declarations. Replaced by `grammar.suite.use-top-level`.
- `grammar.tests.items`: retired 2026-09-28. Typed derivation M26 removed
  annotation declarations. Replaced by `grammar.tests.item-forms`.
- `grammar.annot.pass`: retired 2026-09-28. Typed derivation M26 removed
  the `annotate` block and its body. No replacement.
- `names.local.no-decorators`: retired 2026-09-28. Typed derivation M26
  replaced `annotate` declarations with trait-less derivation blocks.
  Replaced by `names.local.no-metadata`.
- `annot.fact.member`: retired 2026-09-28. Typed derivation M26 removed the
  `annotate` block; a trait-less derivation block edits decorator values.
  Replaced by `annot.fact.member-metadata`.
- `annot.line.placement`: retired 2026-09-28. Typed derivation M26 allows
  member lines in a trait-less derivation block. Replaced by
  `annot.line.placement-blocks`.
- `annot.line.right`: retired 2026-09-28. Typed derivation M28 allows any
  expression of a list type on a member line's right side, not only a list
  expression. Replaced by `annot.line.right-typed`.
- `module.console.println-drive`: retired 2026-09-28. The MHP-1 second
  round made `println` exactly `block_on` of its `write_line!` call, which
  may wait on a host write. Replaced by `module.console.println-drive.block-on`
  and `module.console.println-drive.pending`.
- `module.console.println-pending`: retired 2026-09-28. The MHP-1 second
  round dropped the panic on a pending host write; `println` drives the
  write until it finishes. Replaced by `module.console.println-drive.pending`.
- `grammar.annot.newtype-derive`: retired 2026-09-28. Decorators D1 lets
  any decorator precede a newtype, a trait, and an implementation. Replaced
  by `grammar.annot.item-targets` and `grammar.annot.member-targets`.
- `grammar.annot.newtype-derive.error`: retired 2026-09-28. Decorators D1
  leaves only a transparent alias without decorators. Replaced by
  `grammar.annot.alias-no-decorator`.
- `grammar.fn.decorator-targets`: retired 2026-09-28. Decorators D1 lets
  method parameters take decorators. Replaced by
  `grammar.fn.decorator-param-targets`.
- `annot.decorator.function`: retired 2026-09-28. Decorators D1 lets any
  decorator precede a function. Replaced by `annot.decorator.targets` and
  `annot.decorator.attach`.
- `names.suffix.module`: retired 2026-09-28. Literal Suffixes L11 makes a
  suffix a function, not a type. Replaced by `names.suffix.module-name`.
- `names.suffix.unknown`: retired 2026-09-28. Literal Suffixes L11 resolves
  a suffix as a value name. Replaced by `names.suffix.unknown-name`.
- `expr.suffix.call`: retired 2026-09-28. Literal Suffixes L11 calls the
  suffix function, not `from_literal`. Replaced by `expr.suffix.fn-call`.
- `expr.suffix.call.example`: retired 2026-09-28. Literal Suffixes L11.
  Replaced by `expr.suffix.fn-call.example`.
- `expr.suffix.trait-only`: retired 2026-09-28. Literal Suffixes L11
  removed `LiteralSuffix`. No replacement.
- `expr.suffix.no-import`: retired 2026-09-28. Literal Suffixes L11
  removed `LiteralSuffix`. Replaced by `expr.suffix.no-marker-import`.
- `expr.suffix.not-implemented`: retired 2026-09-28. Literal Suffixes L11
  requires the `@num_suffix` marker instead of an implementation. Replaced
  by `expr.suffix.marker` and `expr.suffix.not-marked`.
- `expr.suffix.instantiations`: retired 2026-09-28. Literal Suffixes L11
  has no overloading. No replacement.
- `expr.suffix.std.impl`: retired 2026-09-28. Literal Suffixes L11 makes
  the standard suffixes functions. Replaced by `expr.suffix.std.fn`.
- `trait.suffix.module`: retired 2026-09-28. Literal Suffixes L11 removed
  `std.ops.LiteralSuffix`. Replaced by `expr.suffix.marker.module`.
- `trait.suffix.declare`: retired 2026-09-28. Literal Suffixes L11.
  Replaced by `expr.suffix.marker`.
- `trait.suffix.carrier`: retired 2026-09-28. Literal Suffixes L11 removed
  the newtype carriers. Replaced by `types.literal.suffixed`.
- `trait.suffix.row`: retired 2026-09-28. Literal Suffixes L11. Replaced by
  `expr.suffix.fn-shape` and `expr.suffix.fn-shape.reader`.
- `trait.suffix.coherence`: retired 2026-09-28. Literal Suffixes L11 has no
  implementations to be coherent. No replacement.
- `module.prelude.ops`: retired 2026-09-28. Literal Suffixes L11 and
  Decorators D9 replaced `LiteralSuffix` with `NumSuffix` and
  `num_suffix`. Replaced by `module.prelude.ops-num-suffix`.
- `grammar.type.row.keys`: retired 2026-09-28. Bound And Row Operators
  joins row keys with `+`, not commas. Replaced by
  `grammar.type.row.plus-keys`.
- `grammar.type.row.parenthesized`: retired 2026-09-28. Bound And Row
  Operators dropped the parenthesized row. Replaced by
  `grammar.type.row.empty` and `grammar.type.row.no-parentheses`.
- `grammar.type.row.header-bare`: retired 2026-09-28. Bound And Row
  Operators writes a row the same way everywhere. Replaced by
  `grammar.type.row.one-form`.
- `grammar.type.row.header-position`: retired 2026-09-28. Bound And Row
  Operators removed `header_requirement_clause`. Replaced by
  `grammar.type.row.one-form` and `grammar.type.row.one-form.examples`.
- `grammar.type.row.in-type`: retired 2026-09-28. Bound And Row Operators
  writes a row inside a type without parentheses. Replaced by
  `grammar.type.row.one-form`.
- `grammar.type.row.in-type.positions`: retired 2026-09-28. Bound And Row
  Operators. Replaced by `grammar.type.row.one-form`.
- `grammar.type.row.no-operators`: retired 2026-09-28. Bound And Row
  Operators makes `+` the row operator. Replaced by
  `grammar.type.row.plus-only`.
- `grammar.type.row.plus-bound`: retired 2026-09-28. Bound And Row
  Operators moved bounds to `&`. Replaced by
  `grammar.type.row.plus-rows-only` and `grammar.generic.bound.and`.
- `grammar.type.row.old-operator`: retired 2026-09-28. Bound And Row
  Operators replaced `old-row-operator`. Replaced by
  `grammar.type.row.old-separator`, `grammar.type.row.no-subtraction`, and
  `grammar.generic.bound.old-plus`.
- `req.row.syntax.comma-list`: retired 2026-09-28. Bound And Row Operators
  joins row keys with `+`. Replaced by `req.row.syntax.plus-list`.
- `req.row.syntax.header-bare`: retired 2026-09-28. Bound And Row Operators
  writes a row the same way everywhere. Replaced by
  `req.row.syntax.same-form`.
- `req.row.syntax.type-parenthesized`: retired 2026-09-28. Bound And Row
  Operators dropped the parenthesized row. Replaced by
  `req.row.syntax.same-form`.
- `req.row.syntax.no-operators`: retired 2026-09-28. Bound And Row
  Operators replaced `old-row-operator`. Replaced by
  `req.row.syntax.old-separator`.
- `trait.bound.compose`: retired 2026-09-28. Bound And Row Operators
  composes bounds with `&`. Replaced by `trait.bound.compose-and`.
- `fn.generic.bounds`: retired 2026-09-28. Bound And Row Operators composes
  bounds with `&`. Replaced by `fn.generic.bounds-and`.
- `req.row.least.no-solution`: retired 2026-09-28. Bound And Row Operators
  item 4 lets a callback whose row lacks the extra key match. Replaced by
  `req.row.least.absent-key`.
- `req.poly.absent`: retired 2026-09-28. Bound And Row Operators item 4.
  Replaced by `req.poly.absent-matches`.
- `lex.raw-string.form`: retired 2026-09-28. Literal Suffixes L19 removed
  the built-in raw string; `r"..."` is a prefixed string. Replaced by
  `lex.raw-string.none` and `lex.prefix.form`.
- `lex.raw-string.literal`: retired 2026-09-28. Literal Suffixes L19.
  Replaced by `lex.prefix.raw-text`.
- `lex.raw-string.backslash`: retired 2026-09-28. Literal Suffixes L19; a
  backslash also keeps `$` from interpolating. Replaced by
  `lex.prefix.backslash`.
- `lex.raw-string.odd-backslashes`: retired 2026-09-28. Literal Suffixes
  L19. Replaced by `lex.prefix.odd-backslashes`.
- `lex.raw-string.single-line`: retired 2026-09-28. Literal Suffixes L19.
  Replaced by `lex.prefix.single-line`.
- `lex.raw-string.multiline`: retired 2026-09-28. Literal Suffixes L19.
  Replaced by `lex.prefix.multiline`.
- `lex.raw-string.no-hash`: retired 2026-09-28. Literal Suffixes L19.
  Replaced by `lex.prefix.no-hash`.
- `lex.raw-string.no-interpolation`: retired 2026-09-28. Literal Suffixes
  L19 makes every prefixed string interpolate. Replaced by
  `lex.prefix.interpolation`, `lex.prefix.reserved-dollar`, and
  `lex.prefix.plain-dollar`.
- `expr.interp.raw`: retired 2026-09-28. Literal Suffixes L19: a prefixed
  string's values go to a template. Replaced by `expr.interp.prefixed`.
- `req.row.param.inferred`: retired 2026-09-28. Requirement Reuse RU3: a
  data type, enum, or trait takes no row parameter. Replaced by
  `req.row.param.callables` and `req.row.param.no-data`.
- `req.row.alias.where`: retired 2026-09-28. Requirement Reuse RU10 allows a
  bare row alias in a one-key row slot, not only after `$`. Replaced by
  `req.row.alias.slots`, `req.row.alias.one-key-slot`, and
  `req.row.alias.bare`.
- `req.row.alias.kind`: retired 2026-09-28. Requirement Reuse RU10 makes
  `$.Context[AppRow]` valid. Replaced by `req.row.alias.type-or-key`.
- `types.cast.range-check`: retired 2026-09-28. Open Issues item 1 makes
  integer casts wrap. Replaced by `types.cast.wrap`.
- `types.cast.out-of-range`: retired 2026-09-28. Open Issues item 1.
  Replaced by `types.cast.wrap` and `types.cast.literal-range`.
- `types.cast.int-int`: retired 2026-09-28. Open Issues item 1: integer to
  integer wraps instead of checking the range. Replaced by
  `types.cast.int-int-wrap`.
- `types.cast.panic`: retired 2026-09-28. Open Issues item 1: only a
  float-to-integer cast panics. Replaced by `types.cast.float-int-panic`.
- `expr.suffix.fn-shape`: retired 2026-09-28. Literal Suffixes L20 and L21
  lift the no-provider and no-type-parameter rules and count required
  parameters. Replaced by `expr.suffix.fn-shape-required` and
  `expr.suffix.ordinary-rules`.
- `expr.suffix.fn-shape.reader`: retired 2026-09-28. Literal Suffixes L21
  checks the shape at the marked definition. Replaced by
  `expr.suffix.fn-shape.definition`.
- `expr.suffix.requirement-free`: retired 2026-09-28. Literal Suffixes L20
  lets a suffix function need providers. Replaced by
  `expr.suffix.position-rules`.
- `expr.prefix.fn-shape`: retired 2026-09-28. Literal Suffixes L20 and L21.
  Replaced by `expr.prefix.fn-shape-required` and
  `expr.prefix.ordinary-rules`.
- `expr.prefix.fn-shape.reader`: retired 2026-09-28. Literal Suffixes L21.
  Replaced by `expr.prefix.fn-shape.definition`.
- `expr.prefix.call-errors`: retired 2026-09-28. Literal Suffixes L21
  reports a parameter that cannot take the template at the definition.
  Replaced by `expr.prefix.fn-shape.definition` and
  `expr.prefix.ordinary-rules`.
- `expr.prefix.std.import`: retired 2026-09-28. Literal Suffixes L20 moves
  `r` to `std.text`. Replaced by `expr.prefix.std.import-text`.
- `lex.raw-string.none`: retired 2026-09-28. Literal Suffixes L20 moves
  `r` to `std.text`. Replaced by `lex.raw-string.none-text`.
- `module.prelude.ops-str-prefix`: retired 2026-09-28. Literal Suffixes L20
  moves `r` to `std.text`. Replaced by
  `module.prelude.ops-str-prefix-markers` and `module.prelude.text-r`.
- `fn.decl.result-required`: retired 2026-09-29. Dependency cycles DC1
  requires a result type on a public inherent method too. Replaced by
  `fn.decl.result-required-pub`.
- `fn.decl.result-omitted`: retired 2026-09-29. Dependency cycles DC1
  lets only a non-public inherent method omit its result type. Replaced by
  `fn.decl.result-omitted-private`.
- `req.row.omitted.empty`: retired 2026-09-29. Dependency cycles DC1 gives
  a public inherent method without a clause the empty row. Replaced by
  `req.row.omitted.empty-pub`.
- `req.row.omitted.inferred`: retired 2026-09-29. Dependency cycles DC1
  infers the row of a non-public inherent method only. Replaced by
  `req.row.omitted.inferred-private`.
- `names.use.cycles`: retired 2026-09-29. Dependency cycles DC2 allows use
  loops inside one folder and forbids loops of folders. Replaced by
  `names.use.cycles-in-folder` and `names.use.folder-cycle`.
- `module.pub-use.cycles`: retired 2026-09-29. Dependency cycles DC2 and
  DC8 allow `pub use` loops inside one folder and require each chain to end
  at a declaration. Replaced by `module.cycle.acyclic`,
  `module.pub-use.chain`, and `module.pub-use.chain.loop`.
- `module.init.graph`: retired 2026-09-29. Dependency cycles DC2 lets the
  use graph have loops inside one folder. Replaced by
  `module.init.use-graph`.
- `module.init.once`: retired 2026-09-29. Dependency cycles DC7 initializes
  modules that use each other as one group. Replaced by `module.init.group`
  and `module.init.group.once`.
- `module.init.ready-order`: retired 2026-09-29. Dependency cycles DC7
  orders ready groups rather than modules. Replaced by
  `module.init.group.ready-order`.
- `module.init.source-order`: retired 2026-09-29. Dependency cycles DC7
  orders the statements of a larger group by dependency first. Replaced by
  `module.init.source-order-single` and `module.init.group.step`.
- `module.interface.determined`: retired 2026-09-29. Dependency cycles DC10
  records fact values in the package interface. Replaced by
  `module.interface.fact-values` and `module.interface.determined-facts`.
- `module.interface.early`: retired 2026-09-29. Dependency cycles DC10
  makes an interface wait for the bodies its facts call. Replaced by
  `module.interface.early-facts`.
- `expr.suffix.fn-shape-required`: retired 2026-09-29. Literal Suffixes L22
  allows exactly one parameter, so a further defaulted parameter is an
  error. Replaced by `expr.suffix.fn-shape-one`.
- `expr.prefix.fn-shape-required`: retired 2026-09-29. Literal Suffixes L22.
  Replaced by `expr.prefix.fn-shape-one`.
- `types.cast.float-int`: retired 2026-09-29. Open Issues casts follow-ups:
  a float-to-integer cast saturates instead of checking its range. Replaced
  by `types.cast.float-int-saturate`.
- `types.cast.float-int-panic`: retired 2026-09-29. Open Issues casts
  follow-ups. Replaced by `types.cast.saturate` and `types.cast.no-panic`.
