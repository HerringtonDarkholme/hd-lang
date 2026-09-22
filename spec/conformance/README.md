# Conformance Fixtures

These fixtures turn normative specification examples into future parser and
type-checker tests. They do not define semantics independently of the
specification.

## Layout

- `parse/valid`: must parse under the core grammar.
- `parse/invalid`: must be rejected during lexing or parsing.
- `typing/valid`: must parse and type-check.
- `typing/invalid`: must parse, then fail with the diagnostic category named in
  the file's first `# expect-error:` comment.
- `runtime/panic`: must parse and type-check, then panic with the category named
  in the file's first `# expect-panic:` comment when executed.

An implementation test runner should accept one file at a time. Files may rely
on primitive types and ordinary prelude names, but should declare all
user-defined types and functions they use.

Exact diagnostic wording is not normative. The category after
`# expect-error:` is stable enough for a future structured diagnostic code.

## Coverage

`cases.tsv` is the authoritative fixture index. Every fixture has one primary
specification section.

`examples.tsv` inventories every `text` code fence in the numbered core
chapters. Runnable examples map to one or more self-contained fixtures. Lexical
inventories, type fragments, filesystem layouts, and similar non-program text
are classified explicitly rather than being mistaken for compilable files.
`spec/check.sh` verifies that every core `text` fence has exactly one inventory
entry and that every runnable entry names an indexed fixture.
