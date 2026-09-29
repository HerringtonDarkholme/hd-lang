# Literal Suffixes: Open Points

Status: open. Nothing here is accepted behavior. Owner decisions L1-L22
(2026-09-28 and 2026-09-29) are applied, and the specification is
authoritative for them:
[Literal Suffixes](../spec/01-lexical-structure.md#literal-suffixes)
(lexing), [Literal Suffixes](../spec/05-expressions.md#literal-suffixes)
and [Prefixed Strings](../spec/05-expressions.md#prefixed-strings)
(meaning), [Suffixed Literals](../spec/04-type-system.md#suffixed-literals)
(typing), and [Test Cases](../spec/10-modules.md#test-cases) (the
`timeout` option). A suffix is a function marked `@num_suffix`, and a
string prefix is a function marked `@str_prefix`. The survey, the options,
and the decision log are in git history.

## Still Open

The L19 apply pass (2026-09-28) met these points. L19 said "lexed the way
raw strings are today, including `$name` interpolation", but raw strings
never interpolated, so the pass had to fill in how `$` and `\` behave. The
specification states the reading in the Applied column, so each can change
without breaking a decision. Each waits for the owner.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 16 | A `$` before a reserved word other than `self`, as in `r"$true"` | `syntax-error` ([`lex.prefix.reserved-dollar`](../spec/01-lexical-structure.md#r-lex.prefix.reserved-dollar)), as in interpreted strings | Keep. |
| 17 | How to write `$name` as text in a prefixed string | A backslash keeps a following `$` from interpolating and stays in the text ([`lex.prefix.backslash`](../spec/01-lexical-structure.md#r-lex.prefix.backslash)), as JavaScript's `String.raw` does; `process_escapes` turns `\$` into `$` | Keep. |
| 23 | A prefixed string as a test name, as in `it(r"a\b"):` | A call, so `non-literal-test-argument` by the existing rule | Keep. |

```text
use std.text.r

kept := r"\$name"      # the text \$name
```
