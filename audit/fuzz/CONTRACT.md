# hd-lang command contract

This is the only behavior the fuzzer in `audit/fuzz/` relies on. The portable
harness (`test/run-portable.ts`, `test/README.md`) relies on the same behavior,
but until now nothing wrote it down. Nothing here depends on the
implementation language, the backend, or the AST, HIR, or WAT.

## Invocation

An implementation is a command prefix, such as
`node --experimental-strip-types bin/hd.js` or `other-hd`. The fuzzer runs:

```text
<prefix...> <action> [options] FILE
```

- `FILE` is an absolute path to a UTF-8 `.hd` file.
- The working directory is the repository root.
- stdin is closed. stdout and stderr are captured.
- Every invocation has a timeout (10 s by default).

The fuzzer uses the actions `parse`, `check`, `run`, and `test`. The optional
Wasm adapter also uses `build`. No options are passed.

## Output lines the fuzzer reads

- **Located diagnostic.** `PATH:LINE:COL: CODE: message`, where `PATH` ends
  with the basename of `FILE`. A warning is `PATH:LINE:COL: warning: CODE:
  message`. `CODE` is lowercase kebab-case.
- **Panic.** `CODE: message` (optionally prefixed by `PATH:LINE:COL: `), where
  `CODE` is one of the stable panic categories in
  `spec/06-control-flow.md#runtime-panics`. Only `run` and `test` may report one.

The fuzzer reads nothing else: no stack traces, and no message text. For
grouping only, it may use a normalized first message line as a label when a
result has no code.

## Outcomes

| Command      | Success                  | Rejection                                                                        |
| ------------ | ------------------------ | -------------------------------------------------------------------------------- |
| `parse FILE` | exit 0                   | exit 1, at least one located diagnostic                                          |
| `check FILE` | exit 0, warnings allowed | exit 1, at least one located diagnostic with a code from the spec inventory       |
| `run FILE`   | exit 0                   | exit 1, a located diagnostic with an inventoried code, or a chapter-06 panic code |
| `test FILE`  | exit 0                   | same as `run`                                                                    |
| `build FILE` | exit 0, module emitted   | exit 1, located diagnostic (Wasm adapter only)                                   |

The **spec inventory** is the union of:

1. the Error, Warning, and Boundary-failure rows of the normative table in
   `spec/README.md#diagnostics`;
2. the codes the reference parser in `spec/reference-parser/` can emit. Several
   of these (for example `syntax-error`, `unclosed-delimiter`) are missing from
   table 1. The fuzzer counts them separately (`reference-only-codes.tsv`).

## Contract violations

Each of these is reported as a violation:

- exit status other than 0 or 1 (`bad-exit`);
- exit 1 with no located code (`no-located-code`), or with only an
  unrecognized panic line;
- on `check`, `run`, or `test`: a located code outside the spec inventory
  (`uninventoried-code`). All such codes share one signature per command, and
  each code is tallied in `uninventoried-codes.tsv`;
- termination by a signal (`signal`), or no exit within the timeout (`timeout`);
- phase inconsistency: `check` accepts while `parse` rejects the same file, or
  `run` or `test` succeeds (exits 0 or panics) while `check` rejects it.

## Outcome class (for comparing implementations)

Each result is reduced to one label: `accept`, `reject:<first code>`,
`panic:<code>`, or `violation:<kind>`. Cross-implementation mode requires equal
labels for every command on every input.

## Not covered

- What `run` and `test` print on success. The contract has no output oracle.
- Which entry point `run` uses. Today it is `main`. Whether a program without
  `main` must be rejected with a located code, and which code, is an open
  question.
- Diagnostic columns, message text, and the order of multiple diagnostics.
