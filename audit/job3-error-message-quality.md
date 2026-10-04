# Job 3 audit: error message quality

> Posted in chat 2026-10-04 during the read-only audit session; logged here verbatim by owner request. HEAD 4636f546. Body below is the posted report, unedited.

Job 3 done. No repo files changed. HEAD `4636f546`. All 1057 fixtures under `spec/conformance/{parse,typing}/invalid/` run with plain `hd check`; first stderr line collected per fixture.

## Method and buckets

Expected line = the line carrying `# diagnostic:` (inline markers; 0 multi-marker files). First message parsed as `path:line:col: [warning:] code: message`.

- 1024 fixtures print a first diagnostic (974 errors, 50 warnings — 47× `private-main`, 2× `unused-derivation-fact`, 1× `unused-local-binding`).
- 33 print nothing (exit 0). 6 need other fixture flags (`package-tree` 4, `test-layout` 2); 27 are test-code fixtures the real runner checks with `--tests` (`test/run-portable.ts:305,309`) — verified: `misplaced-test-case.hd` reports correctly under `--tests`. One (`unconstrained-impl-parameter.hd`) stays silent even with `--tests`; it is a recorded gap (`src/KNOWN_ISSUES.md:136`).
- Leak patterns (each verified against the fixture source that the name is compiler-internal, not user syntax): `$row:`, `__std_*`, `generic:`, `$implNNN`/`$inherentNNN`, `hd_E1`-style. Not counted: `$Logger`-style rows (source writes `$ Logger`), `$R1` (source writes `$R1`), `hd_run` (source writes `hd_run`).
- "Unnamed" = message with no quoted/backticked span (proxy; many `syntax-error`s legitimately have no item).

## Counts per problem

| problem | count |
| --- | --- |
| leak internals | 37 |
| unnamed (no quoted span) | 279 |
| wrong line: error on another line | 29 |
| wrong line: warning printed first | 29 |
| message over 160 chars | 19 |
| code differs from marker | 85 (35 error-first + 50 warning-first) |
| silent under plain `hd check` | 33 |

## The 40 worst rows (ranked: leak > error-wrong-line > long > code-mismatch)

Format: fixture | reported L+code → expected L+code | problems. Message in full.

1. `arbitrary-with-non-inspectable-member.hd` | L11 `unsatisfied-trait-bound` → L11 same | leak+long (165): `member 'run' cannot be derived: type 'fn()->i32' does not implement Arbitrary, required by the bound on 'F' of '$impl352.member'; write the impl of Arbitrary by hand`
2. `derived-arbitrary-function-member.hd` | L6 same → L6 same | leak+long (165): same message with `$impl353.member`
3. `derived-default-member-not-default.hd` | L8 `member-not-derivable` → L8 same | leak+long (163): `a member does not satisfy the walker's, describer's, or source's bound: type 'Secret' does not implement Default, required by the bound on 'F' of '$impl274.member'`
4. `json-derive-member-not-tojson.hd` | L8 same → L8 same | leak+long (162): same shape with `ToJson` / `$impl327.member`
5. `derived-arbitrary-tuned-member-not-arbitrary.hd` | L14 `unsatisfied-trait-bound` → L14 same | leak+long (161): `member 'port' cannot be derived: type 'Port' does not implement Arbitrary, required by the bound on 'F' of '$impl352.member'; write the impl of Arbitrary by hand`
6. `impossible-gadt-pattern.hd` | L3 `unsupported-gadt-result` → L9 `impossible-gadt-pattern` | wrongline+codemismatch+unnamed (83): `explicit variant results without shared enum data are outside the current MVP slice` (names neither the enum nor the arm; I read the fixture: the marker is on the `BoolLit` match arm, the compiler flags the `BoolLit` declaration line instead)
7. `literal-first-use-bound.hd` | L11 `mixed-signedness` → L10 `unsatisfied-trait-bound` | wrongline+codemismatch+unnamed (73): `signed and unsigned operands do not mix: i32 and u32; cast one explicitly` (marker is on `negate(i)` L10; the compiler reports the later `i < items.len()` line instead)
8. `literal-first-use-closure-conflict.hd` | L8 `mixed-signedness` → L9 `type-mismatch` | same trio (73): same message
9. `literal-first-use-conflict.hd` | L8 `mixed-signedness` → L10 `type-mismatch` | same trio (73): same message
10. `generic-erasure-without-bound.hd` | L6 `type-mismatch` → L6 same | leak+unnamed (43): `expected trait:Inspectable, found generic:T` (source writes `T`)
11. `literal-var-method-missing.hd` | L8 `type-mismatch` → L7 `unknown-method` | wrongline+codemismatch+unnamed (23): `expected u32, found i32`
12. `literal-first-use-range.hd` | L8 `type-mismatch` → L7 `integer-literal-range` | same trio (22): `expected u8, found i32`
13. `literal-var-tuple-fallback.hd` | L9 `type-mismatch` → L13 `implicit-narrowing` | same trio (22): `expected u8, found i32`
14. `derive-member-not-derivable.hd` | L30 `member-not-derivable` → L30 same | leak (158): `…required by the bound on 'F' of '$impl1.member'`
15. `string-prefix-extra-parameter.hd` | L5 `type-mismatch` → L5 same | leak (133): `expected '__std_ops_StrPrefix[fn(Template[string],string)->string]', but the call returns '__std_ops_StrPrefix[fn(Template[T])->R$Q]'`
16. `string-prefix-suspending.hd` | L5 same → L5 same | leak (127): `expected '__std_ops_StrPrefix[fn!(Template[string])->string]', but the call returns '__std_ops_StrPrefix[fn(Template[T])->R$Q]'`
17. `string-prefix-parameter-type.hd` | L5 same → L5 same | leak (120): `expected '__std_ops_StrPrefix[fn(i32)->i32]', but the call returns '__std_ops_StrPrefix[fn(__std_ops_Template[T])->R$Q]'`
18. `collect-target-not-fromiterator.hd` | L5 `unsatisfied-trait-bound` → L5 same | leak (109): `type 'i32' does not implement __std_iter_FromIterator, required by the bound on 'C' of '$inherent110.collect'`
19. `requirement-key-binding-collision.hd` | L12 `generic-requirement-key-collision` → L12 same | leak (108): `provider keys 'Store[Item=generic:T]' and 'Store[Item=User]' can become identical after generic substitution`
20. `facts-find-unbounded-key.hd` | L6 `unsatisfied-trait-bound` → L6 same | leak (105): `generic parameter 'M' does not implement Inspectable, required by the bound on 'F' of '$inherent252.find'`
21. `literal-suffix-extra-parameter.hd` | L5 `type-mismatch` → L5 same | leak (104): `expected '__std_ops_NumSuffix[fn(i64,i64)->i64]', but the call returns '__std_ops_NumSuffix[fn(N)->R$Q]'`
22. `iterator-chain-iterator-arg.hd` | L9 `unsatisfied-trait-bound` → L9 same | leak (102): `type 'Iterator[i32]' does not implement Iterable, required by the bound on 'I' of '$inherent110.chain'`
23. `generic-provider-key-collision-nested.hd` | L6 `generic-requirement-key-collision` → L6 same | leak (101): `provider keys 'Repo[generic:T]' and 'Repo[generic:U]' can become identical after generic substitution`
24. `generic-provider-key-collision-spread.hd` | L6 same → L6 same | leak (101): same message
25. `generic-requirement-key-collision.hd` | L5 same → L5 same | leak (101): same message
26. `literal-suffix-suspending.hd` | L5 `type-mismatch` → L5 same | leak (101): `expected '__std_ops_NumSuffix[fn!(i64)->i64]', but the call returns '__std_ops_NumSuffix[fn(N)->R$Q]'`
27. `json-map-key-not-string.hd` | L6 `unsatisfied-trait-bound` → L6 same | leak (98): `type 'Map[i32,i32]' does not implement __std_json_ToJson, required by the bound on 'T' of 'encode'`
28. `literal-suffix-no-parameter.hd` | L5 `type-mismatch` → L5 same | leak (97): `expected '__std_ops_NumSuffix[fn()->i64]', but the call returns '__std_ops_NumSuffix[fn(N)->R$Q]'`
29. `closure-row-key-collision.hd` | L9 `generic-requirement-key-collision` → L9 same | leak (96): `provider keys 'Repo[generic:T]' and 'Repo[User]' can become identical after generic substitution`
30. `declared-generic-key-lexical-collision.hd` | L9 same → L9 same | leak (96): same message
31. `json-decode-not-derived.hd` | L9 `unsatisfied-trait-bound` → L9 same | leak (93): `type 'Plain' does not implement __std_json_FromJson, required by the bound on 'T' of 'decode'`
32. `literal-suffix-parameter-type.hd` | L8 `unsatisfied-trait-bound` → L8 same | leak (92): `type 'string' does not implement __std_num_Num, required by the bound on 'N' of 'num_suffix'`
33. `colon-line-after-if-suite.hd` | L4 `not-callable` → L6 `trailing-block-position` | wrongline+codemismatch (91): `type 'void' is not callable: it is not a function type and does not implement std.ops.Apply` (I read the fixture: the `:` block-opener on L6 is never diagnosed; the compiler blames L4 instead)
34. `function-type-overlapping-impl.hd` | L7 `overlapping-impl` → L7 same | leak (91): `fn(i32)->i32 overlaps the Marker implementation for 'fn(*Args)->O$row:R': their heads unify`
35. `json-encode-not-derived.hd` | L9 `unsatisfied-trait-bound` → L9 same | leak (91): `type 'Plain' does not implement __std_json_ToJson, required by the bound on 'T' of 'encode'`
36. `default-tuple-element-without-default.hd` | L12 `unsatisfied-trait-bound` → L12 same | leak (89): `type 'Secret' does not implement Default, required by the implementation bound on 'hd_E1'`
37. `display-tuple-element-without-display.hd` | L7 `unsatisfied-trait-bound` → L7 same | leak (89): `type 'Secret' does not implement Display, required by the implementation bound on 'hd_E1'`
38. `range-to-inclusive-not-iterable.hd` | L4 `unsatisfied-trait-bound` → L4 same | leak (87): `type 'mut __std_ops_RangeTo[i32]' does not implement Iterable, required by the for loop`
39. `range-to-not-iterable.hd` | L4 same → L4 same | leak (87): `type 'mut __std_ops_RangeTo[i32]' does not implement Iterable, required by the for loop`
40. `field-key-bound-not-implied.hd` | L5 `unsatisfied-trait-bound` → L5 same | leak (86): `type 'generic:K' does not implement Eq and Hash, required by the bound on 'K' of 'Map'`

Patterns: leaks cluster in four message templates (derivation `$implNNN.member`, desugared `__std_*` shim traits, `generic:T` renderings, string/suffix-trait signatures). Wrong-line errors are mostly programs with two errors where the compiler stops at an earlier, different one (rows 7–9, 11–13) or genuinely mistargets (rows 6, 33). The 19 long messages are headed by the 5 leak+long rows above (161–165 chars) plus `let-mut-tuple.hd` (178), a `mut-on-tuple` message that *is* helpful but wordy.

Ready for Job 4.