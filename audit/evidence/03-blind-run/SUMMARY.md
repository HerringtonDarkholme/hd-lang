# 3.1 Blind Fixtures: Run and Triage

Commit `bd985d7`, 2026-09-25. Compiler: `node --experimental-strip-types
bin/hd.js`. The worker could not write report files; the coordinator wrote
this file from the worker's final report.

The coordinator reproduced F-351 independently:
`audit/probes/blind-triage/p-plain-closure-mutates-runtime.hd` passes
`hd check`, and `hd run --entry probe` prints `7`, the length of "renamed".
The code `mutable-capture-requires-mut-fn` is listed in `spec/README.md`
but appears nowhere in `src/`.

Evidence in this directory: `run.log`, `runner.diff`, `probes.log`,
`variants.log`, `refparser.log`, `results.tsv`.

## Headline

- **43 of 60 blind fixtures pass (71.7%).**
- If the two ambiguous-code cases (AMB-15, AMB-17) are resolved in the
  implementation's favor, 45 of 60 pass (75.0%).
- None of the 17 failures is a fixture error.

## Commands

- Blind run: `node --experimental-strip-types audit/scripts/blind/run-blind.ts`,
  output `run.log`.
- Probes and variants: the same runner with
  `HD_BLIND_MANIFEST=audit/probes/blind-triage/MANIFEST.tsv` or
  `audit/probes/blind-triage/variants/MANIFEST.tsv`.
- Reference parser: `audit/probes/blind-triage/refparser/refparse-probe.ts`,
  plus `hd parse` and `hd check` on the same files.
- Both scripts pass `oxlint`.

## Runner

`audit/scripts/blind/run-blind.ts` is a copy of `test/run-portable.ts`.
Every pass/fail function is byte-identical to the original, checked by
script: `invoke`, `containsCode`, `containsLocatedCode`,
`runConformanceCase`, `readFixtureCase`, `runFixtureCase`, and the rest.

Only the manifest path and its six-column parsing changed, plus routing:

- files with `# expect:` or `# expect-result:`, or with more than one
  marker, use the fixture path (the conformance path throws on multiple
  markers);
- all other rows use the conformance path with the manifest phase, and the
  expectation derived from the markers must equal the manifest's.

45 rows use the fixture path and 15 the conformance path. The manifest has
49 runtime rows (7 panics), 10 type rows, and 1 parse row. The author's
notes said 47, 12, and 8; the manifest is authoritative.

## Pass Rate

| Area                              | Pass |
| --------------------------------- | ---- |
| 1 evaluation order                | 7/7  |
| 2 cancellation and `defer`        | 8/8  |
| 3 requirement rows                | 7/7  |
| 4 erasure boundaries              | 3/8  |
| 5 identity                        | 3/6  |
| 6 permission weakening            | 2/6  |
| 7 iterator invalidation           | 6/6  |
| 8 defaults and copy-update        | 3/5  |
| 9 strings                         | 4/7  |

| Phase or kind  | Pass  |
| -------------- | ----- |
| parse          | 1/1   |
| type           | 7/10  |
| runtime accept | 28/42 |
| runtime panic  | 7/7   |

## Triage of 17 Failures

| Class                                                                                                   | Fixtures |
| ------------------------------------------------------------------------------------------------------- | -------- |
| implementation bug, wrong behavior                                                                      | 9        |
| unsupported feature with a structured diagnostic (`unknown-type` for `i64`, `f32`, `Any` ×3; `unknown-method` for `replace` ×2) | 6 |
| unsupported feature with only a generic parse error (`mut fn` literal ×2; `+T` variance, a documented MVP deferral) | 3 |
| spec ambiguity (AMB-15, AMB-17, AMB-08)                                                                 | 3        |
| fixture error                                                                                           | 0        |

Some fixtures have more than one cause; per-row causes and spec citations are
in `results.tsv`.

The 11 variants in `audit/probes/blind-triage/variants/` remove only the
unsupported or buggy construct. 6 pass. The other 5 fail on bugs already
listed: F-160 (f64 ordering crashes Binaryen, 2 variants), F-353, F-354, and
F-361.

## Reference-Parser Checks

All four issues the blind author reported were reproduced with
`parseSource`. `hd` accepts each file, and each control file passes both
parsers.

| Ambiguity | Input                    | Reference parser | Filed as |
| --------- | ------------------------ | ---------------- | -------- |
| AMB-01    | `"x${f("y")}z"`          | `syntax-error`   | F-358    |
| AMB-02    | `g(f(a=1, b=2), 3)`      | `argument-order` (its named-argument regex matches `f(a=`) | F-359 |
| AMB-03    | `$.with(Tag=x, ...ctx)`  | `argument-order` (chapter 11 allows any order, "applied left to right") | F-360 |
| AMB-04    | `"\u{41}"`               | `invalid-escape` | F-206 (not refiled) |

For AMB-04, the reference lexer also over-accepts `\{` and `\ `, which `hd`
rejects.

## Findings

| ID    | Severity     | Title                                                                             |
| ----- | ------------ | --------------------------------------------------------------------------------- |
| F-350 | major        | primitives do not satisfy `T: Display` and cannot become `Display` trait values; `render(1)` and `let d: Display = 5` are rejected (extends F-500) |
| F-351 | major        | a plain closure mutates a captured `mut` value; `hd check` accepts it, and the capture changes at runtime |
| F-352 | minor        | `mut fn` closure literals do not parse; generic `expected-expression`             |
| F-353 | minor        | `is` rejects operands typed `mut T`, including list literals                      |
| F-354 | minor        | readonly `map[K, V]` is not covariant in `V` (list is)                            |
| F-355 | minor        | `trim` removes U+FEFF and keeps U+0085                                            |
| F-356 | note         | a default naming a later parameter reports `impure-parameter-default`, a code outside the spec list |
| F-357 | note         | assigning a captured `let` in a plain closure reports `unknown-name`              |
| F-358 | minor (spec) | reference parser rejects string literals inside `${...}`                          |
| F-359 | minor (spec) | reference parser flags nested named arguments as `argument-order`                 |
| F-360 | minor (spec) | reference parser rejects a spread after an explicit binding in `$.with`           |
| F-361 | note (spec)  | a shared-data variant (`StatusCode.NotFound`) is not canonical for `is`           |

Cited, not refiled: F-160, F-206, F-500.

## Phase-7 Questions

1. **AMB-15:** which code covers a default that names a later parameter, and
   which covers an impure parameter default?
2. **AMB-17:** is `let x: mut T = keep(readonly)` a `mutable-upgrade` or an
   inference conflict? The implementation reports `generic-type-mismatch`,
   which is not in the spec list.
3. **AMB-08:** are shared-data variants canonical? If so, are their shared
   fields readonly?
4. **AMB-24:** what are "compatible composite reference types" for `is`?
5. **AMB-01, AMB-03:** should the spec text state these cases outright?
6. Should the spec define an "unsupported by this implementation"
   diagnostic, so a deferral is distinguishable from a rejection?

Fixtures that depended on AMB-05, 06, 10, 12, 13, 14, 18 to 21, and 23
passed, so those ambiguities did not change any outcome. AMB-09 and AMB-11
were not reached.

## Not Done

- The 43 passes were not checked for vacuous passes (see F-150 to F-155).
- `a5-boxed-primitive-identity` has no variant: every assertion in it needs
  `Any` or a primitive trait value.
