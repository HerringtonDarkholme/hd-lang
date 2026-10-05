# Fuzzer notes on the command contract

The normative command contract is
[`spec/conformance/README.md`, "Command Contract"](../../conformance/README.md#command-contract).
The fuzzer relies on it and on nothing else about an implementation: not the
implementation language, the backend, the AST, HIR, or WAT, stack traces, or
message text. This file lists only what the fuzzer adds on top of that
contract.

## Invocation

- An implementation is a command prefix, such as
  `node --experimental-strip-types bin/hd.js` or `other-hd`.
- The fuzzer passes an absolute `FILE`, and `--tests` to `check` so that it
  covers test code; it passes no other options.
- The fuzzer runs every command from the repository root, so relative command
  paths resolve there. The contract itself does not fix the working
  directory.
- The fuzzer uses `parse`, `check`, and `test` as the contract defines them.
  It also runs `FILE` with no action, the contract's run step, and reports
  it as `run`; `run` is judged like `test`. The optional Wasm adapter also
  uses `build FILE`, with FILE copied into a fresh package, because
  `hd build` works only in a package.
- The fuzzer skips `run` on an input that `check` accepts and that has no
  entry point: no `pub fn main` or `main!` line and no top-level statement.
  `run` has nothing to execute there, and how it should treat such a module
  is open (F-265 in `src/KNOWN_ISSUES.md`). An input that `check` rejects still goes to `run`.

## Outcomes

| Command      | Success                  | Rejection                                                                        |
| ------------ | ------------------------ | -------------------------------------------------------------------------------- |
| `parse FILE` | exit 0                   | exit 101, at least one located diagnostic                                        |
| `check FILE` | exit 0, warnings allowed | exit 101, at least one located diagnostic with a code from the spec inventory     |
| `FILE` (`run`) | exit 0                 | exit 101, a located diagnostic with an inventoried code; or exit 1, a chapter-06 panic code |
| `test FILE`  | exit 0                   | same as `run`                                                                    |
| `build FILE` | exit 0, module emitted   | exit 101, located diagnostic (Wasm adapter only)                                 |

The **spec inventory** is the Error, Error (general), Warning, and
Boundary-failure rows of the normative table in
`spec/README.md#diagnostics`.

Panic codes are the stable categories in
`spec/lang/06-control-flow.md#runtime-panics`. Only `run` and `test` may report
one.

## Violation kinds

- `bad-exit`: an exit status other than 0, 1, or 101, or exit 1 from `parse`,
  `check`, or `build`, which exit 101 to reject.
- `no-located-code`: exit 101 with no located code, or exit 1 with only an
  unrecognized panic line.
- `uninventoried-code`: on `check`, `run`, or `test`, a located code outside
  the spec inventory. All such codes share one signature per command, and
  each code is tallied in `uninventoried-codes.tsv`.
- `signal` or `timeout`: termination by a signal, or no exit within the
  timeout (10 s by default).
- Phase inconsistency (the `phase` fuzzer): `check` accepts while `parse`
  rejects the same file, or `run` or `test` succeeds (exits 0 or panics) while
  `check` rejects it.

## Outcome class

Each result is reduced to one label: `accept`, `reject:<first code>`,
`panic:<code>`, or `violation:<kind>`. Cross-implementation mode requires equal
labels for every command on every input. For grouping only, a result with no
code may be labeled by its normalized first message line.

## Not covered

- What `run` and `test` print on success. The fuzzer has no output oracle.
- Which entry point `run` uses.
- Diagnostic columns, message text, and the order of multiple diagnostics.
