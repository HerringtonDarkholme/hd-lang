# Job 7 audit: tour snippet sweep

> HEAD `40a1f4bd`. Read-only; no tour files changed.

## Method

Learned from `website/build.ts` via `website/src/tour-pages.ts` and
`website/src/learn-check.ts`:

- The tour has **no marker for a snippet that is not meant to compile**.
  Every page's one ` ```hd ` block (the snippet, `src/main.hd`) must
  compile under `hd check --tests`. Only ` ```edit ` blocks carry an
  expectation: `error: <code: message>` must fail `check` with that
  diagnostic (matched by inclusion), while `failure: <text>` must still
  compile (the failure surfaces through Test, which `check` does not run).
- Multi-block programs are joined as a package: the snippet plus the
  page's extra ` ```hd src/<name>.hd ` blocks are checked together with
  `--package-tree <tree> --package-path src/main.hd`. Only page 14
  (`modules`) has an extra file block.
- ` ```output ` / ` ```tests ` blocks are runtime expectations, out of
  scope for `hd check` (the playground runner test covers them).

The sweep below mirrors `checkTourSnippets` exactly: `node bin/hd.js
check --tests` on each snippet (packaged where needed) and on each
edit applied via `editedCode`. 19 pages, 19 snippets, 17 edits (pages
15 and 19 have no edit block), 36 rows. Scratch file paths are shown
as `<scratch>/`.

## Verdict

36 of 36 rows match. Every snippet compiles; every `error:` edit fails
with exactly the named diagnostic; both `failure:` edits still compile.

## Table

| page | block | expected | actual |
| --- | --- | --- | --- |
| 1 hello | snippet | compile | exit 0 |
| 1 hello | edit | fail | exit 1: `<scratch>/1-hello-edit.hd:4:5: missing-requirement: call to 'println' requires Console` |
| 2 values-and-mut | snippet | compile | exit 0 |
| 2 values-and-mut | edit | fail | exit 1: `<scratch>/2-values-and-mut-edit.hd:2:5: readonly-root: field 'total_cents' cannot be assigned through readonly type 'Order'` |
| 3 functions | snippet | compile | exit 0 |
| 3 functions | edit | fail | exit 1: `<scratch>/3-functions-edit.hd:3:5: type-mismatch: expected i32, found string` |
| 4 enums-and-match | snippet | compile | exit 0 |
| 4 enums-and-match | edit | fail | exit 1: `<scratch>/4-enums-and-match-edit.hd:2:5: nonexhaustive-match: match does not cover: Refunded` |
| 5 option | snippet | compile | exit 0 |
| 5 option | edit | fail | exit 1: `<scratch>/5-option-edit.hd:10:17: unknown-method: type 'string?' has no supported method 'lower'` |
| 6 errors | snippet | compile | exit 0 |
| 6 errors | edit | fail | exit 1: `<scratch>/6-errors-edit.hd:3:36: type-mismatch: expected i32, found Result[i32,string]` |
| 7 expressions | snippet | compile | exit 0 |
| 7 expressions | edit | fail | exit 1: `<scratch>/7-expressions-edit.hd:2:5: void-binding: a binding cannot store a void value` |
| 8 pipes | snippet | compile | exit 0 |
| 8 pipes | edit | fail | exit 1: `<scratch>/8-pipes-edit.hd:3:12: pipe-step-needs-placeholder: a pipe step other than a bare name or path needs '_' to mark the piped value, as in 'f(_, y)'` |
| 9 data | snippet | compile | exit 0 |
| 9 data | edit | fail | exit 1: `<scratch>/9-data-edit.hd:7:19: missing-required-field: missing required field 'host'` |
| 10 generics | snippet | compile | exit 0 |
| 10 generics | edit | fail | exit 1: `<scratch>/10-generics-edit.hd:10:13: unsatisfied-trait-bound: type 'mut Order' does not implement Ord, required by the bound on 'K' of 'top_by'` |
| 11 traits | snippet | compile | exit 0 |
| 11 traits | edit | fail | exit 1: `<scratch>/11-traits-edit.hd:5:1: missing-trait-method: Shipped does not implement Email.subject` |
| 12 requirements | snippet | compile | exit 0 |
| 12 requirements | edit | fail | exit 1: `<scratch>/12-requirements-edit.hd:4:13: missing-requirement: call to 'println' requires Console` |
| 13 tests | snippet | compile | exit 0 |
| 13 tests | edit (`failure:`) | compile | exit 0 |
| 14 modules | snippet (+1 file) | compile | exit 0 |
| 14 modules | edit | fail | exit 1: `<scratch>/14-modules-edit.hd:1:1: private-import: 'fee' is private to module 'pricing'; mark it 'pub'` |
| 15 error-context | snippet | compile | exit 0 |
| 16 suspension | snippet | compile | exit 0 |
| 16 suspension | edit | fail | exit 1: `<scratch>/16-suspension-edit.hd:2:28: bang-call-outside-suspension: a bang call requires a suspending driver context; <scratch>/16-suspension-edit.hd:8:17: not-suspending: function 'dashboard' is not suspending` |
| 17 least-authority | snippet | compile | exit 0 |
| 17 least-authority | edit | fail | exit 1: `<scratch>/17-least-authority-edit.hd:2:13: missing-requirement: call to 'invoice_text_saved' requires FsWrite` |
| 18 derive | snippet | compile | exit 0 |
| 18 derive | edit (`failure:`) | compile | exit 0 |
| 19 where-next | snippet | compile | exit 0 |

## Notes

- The `edit` rows above name the `error:`/`failure:` the page declares;
  each `fail` actual contains exactly the declared `code: message`.
- Page 16's edit emits a second diagnostic (`not-suspending`) after the
  expected one; the build gate matches by inclusion, so it passes.
- Pages 15 and 19 declare no edit; pages 13 and 18 declare `failure:`
  edits, which compile as required.
