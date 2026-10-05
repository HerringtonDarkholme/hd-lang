# Job AD: Re-Sweep After The Literal Change

Swept at `32856a65` (origin/main): the literal join model (`62a084a2`,
`288105cf`) plus Jobs Y, Z1–Z3, AA, AB, AC.

Method: the Job 3 script (`hd check` over `spec/conformance/parse/invalid`
and `typing/invalid`: 1085 fixtures, 1052 with diagnostics), and the Job 4
sweep (the real conformance runner with a manifest of all 98
`KNOWN_FAILURES.tsv` rows, same adapter as `pnpm run test:portable`).

## Error-message leaks: 37 → 1

The Job Y brief counted 37 fixtures printing compiler-internal names. Now 1:

- `typing/invalid/least-common-type-supertrait-widening.hd:13`:
  `no-common-type: list elements have no common type: trait:Shown,
  trait:Tagged`. The site (`src/checker/expression-literals.ts:96`) still
  calls `typeSourceText`, not Job Y's `displayType` (`src/types.ts:332`).
  That file is in the literal-typing don't-touch set, so the migration
  could not reach it. Other `typeSourceText` diagnostic sites left:
  `expression-operators.ts` (don't-touch), `derive-intrinsics.ts:112`,
  `trait-calls.ts`, `parser.ts` — none leak through the swept dirs today.

## Over-long messages: 0

No diagnostic over 160 characters in either swept directory.

## Numeric types the user never wrote: 46, all joined literals

46 fixtures name a numeric type absent from the source, e.g. `price ** 2`
→ "operator operands have types Money and u32". Every case traced is the
joined or defaulted type of a bare literal the user did write, so these
read as accurate, not confusing.

One observation for the owner: no diagnostic anywhere in the sweep shows
`usize`, although the literal change says a bare literal defaults to
`usize`. A top-level `let limit = 100` reports "expected i32, found u32".
Either the default is `u32` in most positions, or diagnostics render
`usize` as `u32`. If the former, the "defaults to `usize`" summary needs
a position qualifier; if the latter, that rendering is itself confusing.

## Known failures: 98/98 still fail, 0 stale passes

Every `KNOWN_FAILURES.tsv` row still fails. Reasons match actual output
except five rows whose exit code is stale:

- `typed-fact-fn-pattern.hd`, `typed-fact-concrete-pattern.hd`,
  `typed-fact-pattern-other-params.hd`, `typed-fact-pattern-mut-field.hd`,
  `handle-fact-pattern.hd`: each reason says "check exits 1"; all five
  exit 101. (The `cli/doc-private` row also says "exits 1" and that one
  still matches: `hd doc` really exits 1 there.)

Suggested follow-up (not this job; it edits the TSV): replace "exits 1"
with "exits 101" in those five FACT-PATTERN reasons.
