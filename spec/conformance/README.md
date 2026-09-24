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
- `typing/warnings`: must parse and type-check, then emit the category named in
  the file's first `# expect-warning:` comment.
- `runtime/valid`: must parse and type-check, then complete successfully when
  executed by its declared fixture environment.
- `runtime/panic`: must parse and type-check, then panic with the category named
  in the file's first `# expect-panic:` comment when executed.

An implementation test runner should accept one primary `.hd` file at a time.
Files may rely on primitive types and ordinary prelude names, but should declare
all user-defined types and functions they use unless a fixture-environment
directive says otherwise.

`# fixture-package-role: library` and
`# fixture-package-role: root-application` select the synthetic multi-package
environment used by annotation-orphan fixtures. In that environment the runner
supplies the `dep.validation` and `dep.models` packages referenced by the file;
the primary file remains the only case input. `# fixture-runtime-profile: NAME`
selects a named host profile and its providers for a runtime case. The
`disposed-file` profile supplies the `Files` capability declared by its fixture
and returns a handle whose operations after a successful close report
`ResourceError.Disposed`.

`# fixture-runtime-scenario: competing-drivers` tells a runtime harness to
construct the fixture's `main!` suspension once and attempt to drive that same
value from two drivers while the first poll is active.
`# fixture-runtime-scenario: reentrant-poll` tells it to invoke `poll` again on
the same suspension before the first poll returns. These directives exercise
runtime protocol states that ordinary single-threaded source cannot create by
itself.
`# fixture-runtime-scenario: cancellation-cleanup` tells it to supply a `Gate`
provider from the `pending-gate` profile, whose `wait!` remains pending, poll
`main!` to that boundary, cancel the suspension, and then require
`cleanup_ran()` to return `true`. The `pending-gate` profile therefore makes
the fixture-declared `Gate` trait a host capability admitted on the entry
requirement row.

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
