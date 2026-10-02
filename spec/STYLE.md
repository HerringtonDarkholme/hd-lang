# Specification Style

This guide defines how the chapters are written: the numbered language
chapters in [`lang/`](README.md#contents), the stdlib chapters in
[`std/`](std/README.md), and the CLI chapter in [`cli/`](cli/README.md).
The goal is a reference that is precise but quick to read: one rule per
sentence, the common case first, and every rule citable by a stable ID.

[Data Types and Enums](lang/08-data-and-enums.md) is the pilot chapter written in
this style. The other chapters are restyled one at a time, and a restyle must
not change what the chapter means.

## Section Template

Each section, or subsection, follows this order:

1. **Definition.** One sentence that says what the section defines.
2. **Example.** A minimal example of the common case.
3. **Rules.** A numbered list, one rule per item, each with a
   [rule ID](#rule-ids). A rule that names a diagnostic ends with
   "Error: `code`.", and a rule that names a runtime panic category ends
   with "Panic: `code`."
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
- Put a diagnostic code or panic category at the end of its rule, never in
  the middle of a sentence.
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
[`r-data.embed.width`](lang/08-data-and-enums.md#r-data.embed.width).

### Syntax And Hierarchy

An ID has two or more segments separated by dots. Each segment is lowercase
kebab-case: a letter, then letters, digits, and single hyphens.

1. The first segment is the chapter prefix from the table below.
2. The second segment names the topic, usually the section, as in
   `data.embed` or `data.enum`.
3. Further segments name the rule. A rule that refines another extends its
   ID, as in `data.embed.depth` and `data.embed.depth.self`.

Segments describe content, never position. An ID contains no section number
or list index, so reordering a list or a chapter renumbers nothing.

| Chapter | Prefix |
| --- | --- |
| [Lexical Structure](lang/01-lexical-structure.md) | `lex` |
| [Grammar](lang/02-grammar.md) | `grammar` |
| [Names and Scopes](lang/03-names-and-scopes.md) | `names` |
| [Type System](lang/04-type-system.md) | `types` |
| [Expressions](lang/05-expressions.md) | `expr` |
| [Control Flow](lang/06-control-flow.md) | `flow` |
| [Functions](lang/07-functions.md) | `fn` |
| [Data Types and Enums](lang/08-data-and-enums.md) | `data` |
| [Traits](lang/09-traits.md) | `trait` |
| [Modules](lang/10-modules.md) | `module` |
| [Requirements and Suspension](lang/11-requirements-and-suspension.md) | `req` |
| [Variadic Generics](lang/12-variadic-generics.md) | `pack` |
| [GADTs](lang/13-gadts.md) | `gadt` |
| [Annotations](lang/14-annotations.md) | `annot` |
| `std/cmp.md` | `std-cmp` |
| `std/format.md` | `std-format` |
| `std/hash.md` | `std-hash` |
| `std/iter.md` | `std-iter` |
| `std/ops.md` | `std-ops` |
| `std/testing.md` | `std-testing` |
| `std/text.md` | `std-text` |
| `std/time.md` | `std-time` |
| `std/task.md` | `std-task` |
| [Command Line](cli/command-line.md) | `cli` |

A stdlib chapter's prefix is `std-` and its module name. The CLI tier's
one chapter, `cli/command-line.md`, uses `cli`. The
[stdlib chapter table](std/README.md#chapters) lists each chapter; a later
move task adds its file.

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
5. A retired ID is deleted from the specification, not listed. The
   commit message for the change names it, with its replacement when there
   is one. The commit message records what a pass applied; OPEN_ISSUES
   holds only open questions.

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
malformed, misplaced, outside a chapter, without its chapter's prefix, or
duplicated anywhere in the specification. The rule inventory
diff (see [Restyling A Chapter](#restyling-a-chapter)) reports a retired ID
as lost, and fails when an added ID appears in the history of the
chapters, numbered or stdlib, which means it was retired before.

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
       main:spec/lang/08-data-and-enums.md spec/lang/08-data-and-enums.md --out /tmp/08-diff.md
   ```

4. The diff must report nothing lost: no diagnostic code, no example line,
   and no rule ID. Examples may move or split, and error examples may be
   added.
5. Compare the old and new normative content sentence by sentence. The
   report pairs each old sentence with its closest new one, and marks weak
   pairs.
6. Preserve meaning exactly. Add, drop, or change no rule. Record a genuine
   ambiguity or contradiction for the owner instead of resolving it.
7. Run `bash spec/check.sh` and `pnpm run website:build`.

Without `--diff`, the tool prints one chapter's inventory: its codes,
normative sentences, examples, rule IDs, and prose statistics.

## Before And After

This excerpt from [Data Declarations](lang/08-data-and-enums.md#data-declarations)
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
