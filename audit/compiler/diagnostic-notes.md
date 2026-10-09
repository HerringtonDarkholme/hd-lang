# Diagnostic Notes

Findings about error messages, codes, positions and fix-its, gathered while
building the Rust compiler. Owner rule (2026-10-08): log them here; do not
change the spec or the conformance fixtures for them. The compiler does what
it does until the owner picks items from this list.

| Date | Where | Finding | Today |
| --- | --- | --- | --- |
| 2026-10-08 | `@derive(Eq, Ord)` (no `PartialOrd`) | Two errors on one line: `mixed-derived-law` and `missing-supertrait-implementation`. Already settled by the owner before this rule: spec `trait.derive.related.replaces` (517696e9) and fixture `derived-ord-without-partial-ord.hd`; compiler side was task #126 (now only this note). | Both errors |
| 2026-10-08 | Field missing several derived traits | `@derive(Eq, Hash)` over a field lacking both gets one `derive-field-missing-trait`, naming the first trait; the second shows only after the first is fixed. | One error, first trait |
| 2026-10-08 | Derivation block (`impl Eq for D by Structure`) | A member that fails the walker bound is reported on the block header, not at the field: a block has no field positions of its own. | At the header |
| 2026-10-08 | `unused-derivation-fact` fix-it | Spec `annot.fact.unused-block-decorator.fix` asks for a fix-it; the compiler has no way to attach one yet: `DiagBuf` has fix fields but no API to fill them, so the suggestion is in the message text. | Text only |
| 2026-10-08 | `@error` message argument | Only a plain string literal counts as "one message"; a prefixed string (`str_prefix` form) is `invalid-error-marker`. The spec does not say either way. | Rejected |
| 2026-10-08 | `mixed-derived-law` message | The message does not name the missing or mixed trait ("`@derive` must list the traits a derived trait needs, and cannot mix with hand-written related traits"). | Generic text |
| 2026-10-08 | Same-name items in one message | Two items with one short name print as `std.cmp.Eq` (module-qualified); not yet exercised by a test. | Untested |
| 2026-10-08 | `ambiguous-method` hint | Prints the call form as `Convert::[..]::convert(..)`; it could show the candidate trait arguments as written (`Convert::[i32]::convert(c)`). Was task #75. | Generic form |
| 2026-10-08 | Row parameters in messages | Rows print as `owner#index`, not by their declared name. Was task #51. | Internal form |
| 2026-10-08 | `trailing-block-position` (and `syntax-error`) | The message is the code repeated (`trailing-block-position: trailing-block-position`). Seen when a named argument is written `name: value` inside parentheses (hd uses `name=value`); the message could say that `:` there starts a trailing block, which brackets do not allow. | Code only |
| 2026-10-08 | `unknown-trait` on an impl head | Cascades into `error: :1:1: unsupported: stage Body: body of … has no header`, with an empty file path. Was task #94. | Extra junk error |
| 2026-10-08 | `identity-requires-references` on `x is x` | Reported twice, once per operand, for one mistake. | Two errors |
