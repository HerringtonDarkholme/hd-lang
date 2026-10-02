# hd-lang fuzzer (implementation-neutral)

This fuzzer tests any hd-lang implementation through its command line. The
current TypeScript compiler is only its first target.

## Portability rules

- The implementation under test is a command, never a module. The fuzzer does
  not read the AST, HIR, WAT, or stack traces. The command contract is
  [`spec/conformance/README.md`, "Command Contract"](../../conformance/README.md#command-contract);
  [CONTRACT.md](CONTRACT.md) has the fuzzer-specific notes.
- Oracles come from `spec/` only:
  - the diagnostic inventory in `spec/README.md`;
  - the panic categories in `spec/lang/06-control-flow.md`;
  - the EBNF fences in `spec/lang/02-grammar.md`, which `generate.ts` parses itself;
  - `spec/conformance/cases.tsv`, which says which seeds parse.
- Seeds come from `spec/conformance/**/*.hd`.
- There is no reference parser. Where an input must parse, the first
  implementation's `parse` decides.
- Imports are limited to Node built-ins, this folder, and `spec/`. This
  command enforces the rule and exits 1 on any other import, or on any string
  that points into `src/`. `spec/check.sh` runs it:

  ```sh
  node --experimental-strip-types spec/tools/fuzz/check-imports.ts
  ```

## Running

```sh
# Default implementation: HD_FUZZ_COMMAND, else "node --experimental-strip-types bin/hd.js".
node --experimental-strip-types spec/tools/fuzz/fuzz.ts --seed 1 --cases 5000 --jobs 8 --out /tmp/fuzz-out

# Another implementation.
HD_FUZZ_COMMAND="other-hd" node --experimental-strip-types spec/tools/fuzz/fuzz.ts --seed 1
node --experimental-strip-types spec/tools/fuzz/fuzz.ts --compiler "other-hd" --seed 1

# N-way comparison: give --compiler more than once. This enables the cross-impl fuzzer.
node --experimental-strip-types spec/tools/fuzz/fuzz.ts \
  --compiler "node --experimental-strip-types bin/hd.js" --compiler "other-hd" --seed 1

# Optional Wasm backend adapter (build + Binaryen validation).
node --experimental-strip-types spec/tools/fuzz/fuzz.ts --adapter wasm --seed 1

# Re-check one file against every oracle (exit 1 if any signature fires).
node --experimental-strip-types spec/tools/fuzz/fuzz.ts --replay audit/evidence/03-fuzz/findings/some.hd --fuzzer parse,contract,phase
```

The command runs from the repository root, so relative command paths resolve
there. `spec/tools/fuzz/forward.sh` is a trivial second "implementation" that
forwards to `bin/hd.js`. Use it to check that cross-impl mode reports zero
disagreements.

### Options

| Option                       | Default               | Meaning                                                   |
| ---------------------------- | --------------------- | --------------------------------------------------------- |
| `--compiler "<cmd>"`         | `HD_FUZZ_COMMAND`     | implementation under test; repeatable                     |
| `--seed S`                   | `1`                   | any string; the same seed and `spec/conformance/` give the same inputs |
| `--cases N`                  | `5000`                | cases per fuzzer                                          |
| `--jobs J`                   | `HD_TEST_JOBS`, else `min(8, cores)` | concurrent cases                           |
| `--fuzzer a,b`               | all applicable        | `parse`, `contract`, `phase`, `cross`, `wasm`, `all`      |
| `--timeout MS`               | `10000`               | per invocation; a timeout is a contract violation         |
| `--max-signatures N`         | `20`                  | a fuzzer stops after N distinct signatures                |
| `--minimize`                 | off                   | delta-minimize the first example of each signature        |
| `--min-tests N`              | `300`                 | predicate budget per minimization                         |
| `--adapter wasm`             | off                   | enables the `wasm` fuzzer                                 |
| `--out DIR`, `--work DIR`    | tmp, tmp              | report directory; scratch directory for case files (the default scratch directory is deleted on exit) |
| `--fail-on a,b`              | none                  | exit 1 if a listed fuzzer (or `all`) records any signature |
| `--replay FILE`              |                       | evaluate one file and print its outcomes and signatures   |

## Fuzzers

| Fuzzer     | Input                                                              | Oracle                                                                 |
| ---------- | ------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| `parse`    | 50% mutated seeds, 50% EBNF-generated programs                     | `parse` obeys CONTRACT.md                                              |
| `contract` | mutated seeds the first implementation's `parse` accepts           | `parse`, `check`, `run`, `test` obey CONTRACT.md                       |
| `phase`    | the same inputs as `contract` (results are cached, not rerun)      | parse rejects, so check rejects; check rejects, so run and test reject |
| `cross`    | alternating `parse` and `contract` inputs                          | every implementation gets the same outcome class for every command    |
| `wasm`     | `contract` inputs that `check` accepts                             | `build` succeeds and the module validates in `wasm-opt` (and `wasm-tools` if on PATH) |

A case's input depends only on `(seed, fuzzer, index)` and, for the
`contract` stream, on which mutants the first implementation's `parse`
accepts. Parallelism does not change it. Mutation operators (`mutate.ts`): token
delete, duplicate, swap, and insert (from the EBNF literals); reindent; line
delete, duplicate, swap, and splice (from another seed); and literal
replacement with boundary values. Conformance markers are stripped from
mutants.

A **signature** is `fuzzer | outcome class | first code`. For results with no
code, a normalized first message line serves as the label. Each fuzzer stops
after `--max-signatures` distinct signatures.

## Generator cross-check

`grammar-check.ts` sends small EBNF derivations to an implementation's
`parse` and lists the smallest rejected ones. It finds cases where the
implementation and the grammar disagree. The generator's layout rendering is
approximate: it renders `SUITE_END` followed by `NEWLINE` as one line break.
Triage every sample by hand.

```sh
node --experimental-strip-types spec/tools/fuzz/grammar-check.ts --seed g1 --cases 4000 --show 40
node --experimental-strip-types spec/tools/fuzz/grammar-check.ts --compiler "other-hd" --cases 500
```

## Smoke runs

The smoke run uses the seed `smoke`. A case's input also depends on the seed
files under `spec/conformance/`, so adding a fixture can change the inputs.

- `spec/check.sh` runs only the import gate.
- `pnpm run fuzz:smoke`, part of `pnpm run check`, runs the `parse`,
  `contract`, and `phase` fuzzers with 100 cases each against
  `node --experimental-strip-types bin/hd.js`. It takes about 45 s. Only
  `phase` can fail it (`--fail-on phase`). The `parse` and `contract`
  signatures are known implementation findings, so they are reported, not
  gated.

## Output

For each fuzzer, `--out DIR/<fuzzer>/` contains:

- `signatures.tsv`: id, count, first case index, signature, input origin,
  sample details;
- `examples/<id>.hd`: the first input for each signature. With `--minimize`,
  also `<id>.min.hd`;
- `outcomes.tsv`: the outcome-label distribution per compiler and command;
- `uninventoried-codes.tsv`: located codes that are not in the spec inventory;
- `run.json`: the metadata (seed, counts, timing) and every signature record.

## Minimizer

`minimize.ts` removes comments first. It then runs ddmin over lines, empties
balanced bracket groups, and runs ddmin over the tokens of each line. The
predicate reruns the same fuzzer's oracle and keeps a candidate only if the
same signature still fires. For contract-stream fuzzers, the first
implementation's `parse` must also still accept the candidate. The minimizer uses only the command
contract, so it is as portable as the fuzzer.

## Triage

Save each triaged example as a portable fixture (`# test:`, `# expect:`,
`# diagnostic:`, or `# panic:` markers). The audit's fixtures are in
`audit/evidence/03-fuzz/findings/`. A fixture can then be promoted to
`spec/conformance/`. Classes: implementation bug, spec ambiguity, or correct
handling (discarded).

## Layout

```text
fuzz.ts            entry point
common.ts          command spawning, contract classification, inventory, PRNG
mutate.ts          seed mutation operators
generate.ts        EBNF reader and generator (reads spec/lang/02-grammar.md)
oracles/           contract, phase-consistency, cross-impl
adapters/wasm.ts   optional Wasm backend adapter
minimize.ts        contract-preserving delta minimizer
check-imports.ts   import-boundary enforcement
grammar-check.ts   EBNF derivations vs an implementation's parse
forward.sh         forwarding wrapper for the cross-impl self-test
```

The minimized fixtures from the audit are in `audit/evidence/03-fuzz/findings/`.
