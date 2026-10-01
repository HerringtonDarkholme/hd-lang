# Spec Text Tools

These tools read the specification, the fixtures, and the records as text.
They import only Node built-ins and `spec/`, never `src/`, so they serve any
implementation.

| File | Purpose |
| --- | --- |
| `spec.ts` | the `npm run spec` entry point: `counts`, `audit`, and `refs` |
| `spec-prose.ts` | the Markdown block scanner, rule ID markers, and the prefix table |
| `rule-inventory.ts` | one chapter's inventory, and the restyle diff ([STYLE.md](../STYLE.md#restyling-a-chapter)) |
| `run-conformance.ts` | the conformance runner for any implementation |
| `fuzz/` | the implementation-neutral fuzzer ([README](fuzz/README.md)) |

## Counting Rules

`counts` counts numbered rules, the `r[...]` markers that `rule-inventory.ts`
finds. Use it for every rule count in a batch report.

```sh
npm run spec -- counts                # per chapter, with tier totals and a total
npm run spec -- counts --by prefix    # per first ID segment
npm run spec -- counts --by topic     # per first two ID segments, as data.embed
npm run spec -- counts --by kind      # per Design Cost Order kind (heuristic)
npm run spec -- counts --json         # every view as JSON
```

The language tier is the numbered chapters; the stdlib tier is `spec/std/`.
The kind column is a guess from the ID prefix, the section headings, and
the rule text, so the report marks it "heuristic":

| Kind | Guessed when |
| --- | --- |
| core-library | the rule is in `spec/std/` |
| syntax | the prefix is `lex` or `grammar`, the rule names `syntax-error`, or a heading names syntax, grammar, spellings, tokens, or layout |
| intrinsic | an ID segment, a heading, or the rule text names an intrinsic, a lang item, or the prelude |
| semantic | any other language-tier rule |

## Auditing Style

`audit` checks the chapters against [STYLE.md](../STYLE.md) and prints a
summary by chapter. Its findings are warnings; `--strict` makes any warning
exit 1, and `--list` prints each one.

| Kind | Finding |
| --- | --- |
| long-sentence | a sentence over 25 words |
| long-paragraph | a paragraph, list item, or quote over 90 words |
| no-example | a section that holds rules but no code block |
| duplicate-rule | a rule repeats a sentence of an earlier rule, after normalizing case and punctuation |
| bad-rule-id | an ID that breaks the ID pattern, the prefix table, or uniqueness |
| unknown-code | an `Error:` or `Warning:` code not in the README Diagnostics table, or a `Panic:` category not in Control Flow |
| unused-code | a Diagnostics table code that no rule names |

## Finding Citations

`refs RULE-ID` lists every place that cites a rule: Markdown prose in
`spec/`, `guide/`, and `future-work/`, fixture comments and the index
files, `test/portable/KNOWN_FAILURES.tsv`, `lib/std` comments, and `src/`
comments. A citation is a `#r-<id>` anchor or a bare rule ID.

```sh
npm run spec -- refs data.embed.width
npm run spec -- refs --dead           # every citation of a rule the spec lacks
npm run spec -- refs --dead --all     # also list citations allowed as history
```

`refs --dead` also reports an anchor whose file is not the rule's chapter.
For a retired ID it names the Revision Notes entry that retired it, such as
"retired in batch 24".

| Where | A dead citation |
| --- | --- |
| `spec/`, fixtures, `guide/`, `lib/std` | fails, and `spec/check.sh` fails |
| `future-work/`, `KNOWN_FAILURES.tsv`, `src/` | warns |
| a line that records history, such as "(since retired)" or a Revision Notes entry | is allowed |

A bare ID counts only when it is a rule ID now or was one in the chapters'
git history. A field access in a comment is therefore not a citation.
