# Diagnostics Without A Message (M10, 2026-10-10)

Feeds task #252. Mechanism: `hd_syntax` diagnostics carry no message
text, and `hd_driver/src/lib.rs:1167` pushes them with message = code,
so users see `CODE: CODE`. Every probe below was confirmed on the
release `hd` and the TS prototype (`pnpm run hd check FILE`); TS texts
are verbatim. Probes live in `/tmp/probes-{a,b}/` and `/tmp/probes-m10/`
(uncommitted, per the job). Span mismatches (TS vs Rust) are noted where
found; message ports are independent of them.

| Code | Emitting function | Situation | TS text | Proposed Rust text |
| --- | --- | --- | --- | --- |
| `argument-order` | `parser/exprs.rs:arguments()` | positional after `name=...`: `resize(width=640, 480)` | `positional arguments must precede named arguments` | adopt verbatim |
| `comparison-chaining` | `parser/exprs.rs:binary()` | `0 < value < 10` | `comparisons do not chain` | adopt verbatim |
| `decorator-not-top-level` | `parser/items.rs:decorators()` | `@…` on a body-nested declaration | `decorators are only valid on top-level declarations` | adopt verbatim |
| `direct-variant-use` | `parser/items.rs:use_group()` | `use pkg.status.{Status.Queued}` | `enum variants cannot be imported directly` | adopt verbatim |
| `doc-comment-without-target` | `parser/mod.rs:doc_comments()` (single post-parse pass) | `##` attached to nothing | `documentation comments cannot attach to executable statements` (+ 4 variants) | generic `documentation comments must attach to a declaration or member` |
| `invalid-dedent` | `layout.rs:351` (+ `:330`) | dedent to no active level (span 3:3 vs TS 3:1) | `column 3 is not an active indentation level` | adopt template, interpolate column |
| `invalid-escape` | `lexer.rs:string()` / `:character()` | `"bad\xescape"` (span differs) | ``invalid escape sequence '\x'`` (+ `invalid Unicode escape sequence`) | adopt verbatim, interpolate escape |
| `invalid-token` | `lexer.rs` fallback + separator/char sites | stray `\`, bad separators, bad char literal | `unexpected character '\'` (+ per-site variants) | per-site ports of the TS variants |
| `missing-let` | `parser/exprs.rs:trailing_block()` + `parser/stmts.rs:simple_statement()` | `display_name: string = "Ada"`; pattern before `:=` (spans differ) | two site-specific texts (see row) | adopt both verbatim by site |
| `mutable-field-modifier` | `parser/items.rs:data_field_or_embedded()` | `mut` field vs `mut` embedded (spans differ) | two context texts (see row) | adopt both verbatim by context |
| `nesting-too-deep` | `parser/mod.rs:enter()` (160) + `lexer.rs:string()` (64) | 200 nested parens | TS prints nothing (no such diagnostic) | `source nests deeper than the implementation limit` (spec `grammar.limit.nesting.error`) |
| `old-bound-operator` | `parser/items.rs:bound_list()` | `T < Named + Tagged` | `several bounds are joined with '&': write 'A & B'` | adopt verbatim |
| `old-export-declaration` | `parser/items.rs:item()` | leading `export` | `'export' was replaced by 'pub use'` | adopt verbatim |
| `old-import-declaration` | `parser/items.rs:item()` | leading `import` | `'import' was replaced by 'use'` | adopt verbatim |
| `old-row-separator` | `parser/types.rs:requirement_clause()` + `items.rs:fn_decl()` | `$ Clock, Logger` | `requirement keys are joined with '+': write '$ A + B'` | adopt verbatim |
| `old-struct-declaration` | `parser/items.rs:item()` | leading `struct` | `'struct' was replaced by 'data'` | adopt verbatim |
| `pattern-order` | `parser/patterns.rs:pattern_arguments()` | `Pair.Values(left=l, r)` | `positional patterns must precede named patterns` | adopt verbatim |
| `qualified-string-prefix` | `parser/exprs.rs:postfix()` | `value.tag"item"` | `a string prefix is a bare name in scope: write 'tag"..."' without the qualifier` | adopt verbatim |
| `reserved-semicolon` | `lexer.rs:punctuation_or_error()` | `name := "Ada";` | `semicolon is reserved and cannot separate statements` | adopt verbatim |
| `syntax-error` | 105 sites; representatives: colon after `if` condition; second `$` clause; suite-as-header; reserved `$name` | `if a and b:` etc. (Rust emits cascades; spans differ on `$`) | `expected ':', found 'and'` / `expected ':', found '$'` / `a control-flow header outside brackets cannot end in an indented suite` / `an unescaped '$' must be followed by an identifier, 'self', or '{'` | adopt the expected/found shape per site |
| `tab-whitespace` | `lexer.rs:step()` / `:string()` / `:character()` | tab between tokens | `tab characters are not permitted as whitespace` (indentation: `indentation must contain spaces only`) | adopt verbatim |
| `trailing-block-position` | `parser/stmts.rs:statement()` + `exprs.rs:list_error()` | second block; block in brackets (+ suite-following form) | three site texts (see row) | adopt all three verbatim by site |
| `trait-method-visibility` | `parser/items.rs:trait_member()` + `:impl_member()` | `pub fn` in trait / trait impl | `trait methods inherit the trait's visibility and cannot be declared pub` | adopt verbatim |
| `unclosed-delimiter` | `lexer.rs:match_delimiters()` | `x := (1 + 2` | `unclosed '(' delimiter` | adopt verbatim, interpolate opener |
| `unexpected-bom` | `lexer.rs:punctuation_or_error()` | stray BOM mid-file | `a byte-order mark is only allowed at the start of a file` | adopt verbatim |
| `unexpected-indentation` | `layout.rs:open_suite()` | suite body not deeper than header | `a nested suite's body must be deeper than the statement that contains its header` | adopt verbatim |
| `unmatched-delimiter` | `lexer.rs:match_delimiters()` | stray `]` | `unexpected ']' delimiter` | adopt verbatim, interpolate closer |
| `unterminated-string` | `lexer.rs:string()` / `:character()` | `name := "Ada` | `unterminated literal` (interpolation: `unterminated '${...}' interpolation`) | adopt verbatim |
| `variant-result-type-removed` | `parser/items.rs:enum_variant()` (3 sub-situations) | `IntLit(v: i64) -> Expr[i64]`; `Pair[U](…)`; `Bad -> Other` | TS prints `ok` (accepts i/ii; reinterprets iii, checker then reports `unknown-type` + `variant-result-owner`) | per spec `grammar.enum.no-result-type/no-type-params`: `a variant result names its enum and gives arguments, not a result type`; `enum variants take no type parameters`; `a variant result names its enum and gives an argument clause`. Flag: TS/RC divergence. |
| `unknown-import` (+ 2 sibling sites) | `hd_resolve/src/lower.rs:923,973,1180` (`export()` failure; message `CODE \`x\` in \`y\``) | `use std.cmp.{NoSuch}` | `module 'std.cmp' declares no 'NoSuch'` | `module 'std.cmp' declares no 'NoSuch'` (adopt TS shape; drop the code echo) |

Notes for #252: 27 of 29 syntax texts can be adopted verbatim (with
interpolation); `nesting-too-deep` and `variant-result-type-removed`
need original messages (TS has neither). Five codes need per-site /
per-context branching (`missing-let`, `mutable-field-modifier`,
`invalid-token`, `trailing-block-position`, `syntax-error`).
`report.rs` already strips a `CODE: ` prefix, so adopted texts must not
repeat the code. Span mismatches (`invalid-dedent`, `invalid-escape`,
`missing-let`, `mutable-field-modifier`, `$name`) and the `if a and b:`
cascade count are parser differences, out of scope here.
