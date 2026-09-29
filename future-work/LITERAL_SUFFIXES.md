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

## Status

All points are decided (2026-09-29):
- 16 and 17 were covered by L20 ("keep the `$` readings"): `r"$true"` is
  `syntax-error`, and `\$` stops interpolation and keeps its backslash.
- 23 was confirmed by the owner: a prefixed string as a test name is
  `non-literal-test-argument`.

The specification already states all three, so nothing remains to apply.
This record can be deleted in the next future-work cleanup.
