# Spec Text Tools

These tools read the specification, the fixtures, and the records as text.
They import only Node built-ins and `spec/`, never `src/`, so they serve any
implementation. The one exception is an adapter module that
`run-conformance.ts --adapter` names on its command line.

| File | Purpose |
| --- | --- |
| `spec.ts` | the `pnpm run spec` entry point: `counts`, `audit`, `refs`, `rewrite`, and `glossary` |
| `spec-corpus.ts` | every chapter with its rule inventory, read from a directory or a git revision |
| `spec-rewrite.ts` | the before/after report of `rewrite` |
| `spec-glossary.ts` | the terms, the Markdown page, and the missing-term report of `glossary` |
| `spec-prose.ts` | the Markdown block scanner, rule ID markers, and the prefix table |
| `rule-inventory.ts` | one chapter's inventory, and the restyle diff ([STYLE.md](../STYLE.md#restyling-a-chapter)) |
| `run-conformance.ts` | the conformance runner for any implementation ([Adapters](#adapters)) |
| `fuzz/` | the implementation-neutral fuzzer ([README](fuzz/README.md)) |
| `spec-tools.test.ts` | the tests of these tools, run by `pnpm test` |

## Adapters

`run-conformance.ts` runs an implementation through the
[command contract](../conformance/README.md#command-contract). By default
it spawns the `--compiler` command once per command line. With
`--adapter MODULE`, it imports MODULE instead and runs every command line
inside the runner's process. Without `--adapter`, it imports no
implementation code.

An adapter module exports `createAdapter({ jobs })`. The object it returns
has two methods:

| Method | Does |
| --- | --- |
| `run(args, timeoutMs, cwd?, options?)` | runs `IMPL ARGS...` as if started in `cwd`, the runner's own directory when unset, with the fixture's [runner options](../conformance/README.md#runner-options), and resolves to `{ status, stdout, stderr, timedOut }`, as a spawned command would report them. A [CLI case](../conformance/README.md#cli-cases) sets `cwd` |
| `close()` | releases the adapter's workers |

`options` is an object whose optional fields are `profile`, `scenario`,
`pendingFunction`, `packageRole`, `dependencies` (a list of `{ name,
directory }`), `testLayout`, and `packageTree` (`{ directory, path }`). The
runner passes the options a fixture selects, and no others. They are not `hd`
command-line flags: the implementation takes them through its adapter, and its
`hd` command line, help text, and usage errors never mention them. Without
`--adapter`, the runner spawns `--compiler` with the command line alone, and a
case that selects a runner option fails.

`run` must stop a command that passes `timeoutMs` and resolve with
`timedOut: true`, and must keep serving later commands. `status` is the
exit status, or null for a command that did not finish. The repository's
own adapter is [`test/hd-adapter.ts`](../../test/hd-adapter.ts).

## Counting Rules

`counts` counts numbered rules, the `r[...]` markers that `rule-inventory.ts`
finds. Use it for every rule count in a batch report.

```sh
pnpm run spec counts                # per chapter, with tier totals and a total
pnpm run spec counts --by prefix    # per first ID segment
pnpm run spec counts --by topic     # per first two ID segments, as data.embed
pnpm run spec counts --by kind      # per Design Cost Order kind (heuristic)
pnpm run spec counts --json         # every view as JSON
```

The language tier is the numbered chapters in `spec/lang/`, the stdlib tier
is `spec/std/`, and the CLI tier is `spec/cli/`.
The kind column is a guess from the ID prefix, the section headings, and
the rule text, so the report marks it "heuristic":

| Kind | Guessed when |
| --- | --- |
| core-library | the rule is in `spec/std/` |
| tooling | the rule is in `spec/cli/`, outside the Design Cost Order |
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
pnpm run spec refs data.embed.width
pnpm run spec refs --dead           # every citation of a rule the spec lacks
pnpm run spec refs --dead --all     # also list citations allowed as history
```

`refs --dead` also reports an anchor whose file is not the rule's chapter.
For a retired ID it reads the chapters' git history in one `git log -p`
pass and names the newest commit that removed the rule's `r[...]` marker,
such as `retired in commit 35a38958 "Spec pass 42: remove shapes; ..."`.

| Where | A dead citation |
| --- | --- |
| `spec/`, fixtures, `guide/`, `lib/std` | fails, and `spec/check.sh` fails |
| `future-work/`, `KNOWN_FAILURES.tsv`, `src/` | warns |
| a line that records history, such as "(since retired)" | is allowed |

A bare ID counts only when it is a rule ID now or was one in the chapters'
git history. A field access in a comment is therefore not a citation.

## Checking A Rewrite

`rewrite BASE` is the check a batch runs after a spec pass. It reads the
spec at git revision `BASE` through `git show` and compares it with the
working tree, or with a second revision `HEAD` when one is given.

```sh
pnpm run spec rewrite origin/main                  # before/after report
pnpm run spec rewrite 42090f42 5125d42b            # two revisions
pnpm run spec rewrite origin/main --json
pnpm run spec rewrite origin/main --fail-on lost-codes,lost-examples,reused-ids
```

| Part | Reports |
| --- | --- |
| Rules | counts per chapter and tier: before, after, and delta |
| Retired, added, moved IDs | IDs only the base has, IDs only the new spec has, and IDs that changed chapters |
| Reused IDs | added IDs that the chapters' history used before the base |
| IDs kept with changed text | the before and after text, least similar first; STYLE.md gives a rule whose meaning changes a new ID |
| Diagnostic codes | codes the rules stop or start naming, and codes that leave or join the Diagnostics table |
| Examples | each base example's status from `rule-inventory.ts` `diff()`, plus `moved` for one found in another chapter, and the lines of each lost one |
| Retired IDs still cited | `refs --dead` citations of a retired ID in the working tree |

The report ends with a `Summary:` line; a batch report quotes it. Without
`--fail-on` the command exits 0. With it, the command exits 1 only when one
of the listed kinds is found:

| Kind | Trips on |
| --- | --- |
| `lost-codes` | a lost code, from the rules or the Diagnostics table |
| `lost-examples` | an example line found in no example after the rewrite |
| `reused-ids` | an ID kept with changed text, or a reused retired ID |

## Generating The Glossary

`glossary` collects the defined terms. The hand-written glossaries in
[spec/README.md](../README.md#glossary) and
[spec/std/README.md](../std/README.md#glossary) are the primary source. The
chapters add a bold term in prose, a rule, or a table row, and a heading
whose section opens "A term is a ..." or "The terms are ...". Each term
carries its chapter, its defining rule ID or section anchor, and a
definition quoted from the spec; the tool writes no definition.

```sh
pnpm run spec glossary              # counts, and chapter terms the glossaries lack
pnpm run spec glossary --markdown   # the page the website renders as spec/glossary.html
pnpm run spec glossary --json       # every term, and the missing ones
```

`pnpm run website:build` generates the Glossary page from the same data and
links it from the Reference section of the sidebar. A term missing from the
hand-written glossaries still appears on the page, marked "not in a
hand-written glossary". Add it to the README glossary to give it a reviewed
definition.
