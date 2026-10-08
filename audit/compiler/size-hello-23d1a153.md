# P2: Hello-World Size Growth (`23d1a153`)

Status: measurement report, 2026-10-08, at `23d1a153`; no design change
is accepted by this report. Bisects `size-hello-32d0f278.md` (deleted
in this commit; git history keeps it): hello (`println(42)`) was
**4,775** bytes at `32d0f278` and is **4,908** now (+133; the job text
says 4,914/+139 — 6 bytes of module-name strings from the adapted
reproduction below, my number reproduces byte-exact).

## Reproduction (adapted)

The old `hd build samples/hello -o OUT.wasm` lost `-o` mid-range, so
each step uses that commit's own invocation on the same program: `-o`
builds of `samples/hello` where supported, otherwise a scratch package
with the identical `hello.hd` (`fn main() -> void $ Console:
println(42)`) built with `hd build`. The `-o`→package switch falls
between bisect positions 11 and 22; the +3 measured across it may be
packaging (module-name strings), not code. Sizes below come from
`compiler/bench/wasm-size.mjs` (one fixed copy for every step).

## Bisect log (hello bytes per compiler commit)

| Commit | Size | Δ | Section/function attribution |
| --- | ---: | ---: | --- |
| `32d0f278` (base) | 4775 | — | matches the old report byte-exact |
| `caa4cfc6` Q14 (report) | 4775 | +0 | report-only, as expected |
| `b0bee535` M4d back-half (init, suspension, defer) | 5792 | **+1017** | entry/suspension runtime linked into every program (below) |
| `f288b05c` M4d end-to-end | 5792 | +0 | — |
| `6c0204be` run scripts | 5832 | +40 across 7 commits (M4c test e2e, run-scripts, checker refutability) | not isolated per commit; small |
| `acff6c50` | 5835 | +3 across 11 commits | spans the `-o`→package CLI switch; possibly packaging strings |
| `4e5c9618`, `f3addbbc` | 5835 | +0 | — |
| `2eeba4a0` literal default types (bare usize, signed i32) | 4918 | **−917** | i64 formatting drops out of hello (below) |
| `0116cd37`, `1e3342d4`, `83fe8199`, `833c83bd` | 4918 | +0 across 22 commits | flat |
| `86344c77` (#70) | 4912 | −6 drift | — |
| `23d1a153` (now) | 4908 | −4 more (tail drift −10, unattributed) | name/hash churn scale, no step isolated |

Net: +133. Two moves explain it; the rest is ±40 noise and −10 drift.

## The +1017: M4d links the entry/suspension runtime (`b0bee535`)

Sections: type +34, import +14, function +7, global +12, export +23,
element +1, code +571, data +79, name +276.

New function bodies: `rt:HostPoll` 201, `rt:WakeMark` 174,
`rt:HostCancel` 97, `rt:WakeTake` 88, `rt:EntryPoll` 31,
`rt:Panic(suspension-invalid-state…)` 28, `rt:Lit` 27, `rt:EntryWake`
10, `rt:EntryInit` 2 (658 total); `rt:HostCold` +73; the data segment
grows 154 → 233 bytes (panic strings incl. the new
suspension-invalid-state text). Verdict: the feature explains it —
module init, suspension state machines and the defer exit ladder mean
every program, even hello, carries the entry/suspension runtime.

## The −917: literal defaults drop i64 formatting (`2eeba4a0`)

Sections: code −851, name −50, global −12, function −3, data −1. Gone:
`std/format/signed_text` (797), `Display for i32.to_string` (59),
small `Lit` helpers; added back: `Display for usize.to_string` (59).
Mechanism: bare `42` no longer defaults to i64, so `println(42)`
monomorphizes the narrower `Display` instance instead of the i64
machinery. Verdict: the feature explains it — spec literal defaults
directly select smaller code.

## Sections at current main (4,908 bytes)

| Section | Encoded bytes | Share |
| --- | ---: | ---: |
| header | 8 | 0.2% |
| type | 153 | 3.1% |
| import | 107 | 2.2% |
| function | 37 | 0.8% |
| memory | 5 | 0.1% |
| global | 105 | 2.1% |
| export | 40 | 0.8% |
| element | 9 | 0.2% |
| data count | 3 | 0.1% |
| code | 2942 | 59.9% |
| data | 252 | 5.1% |
| standard `name` custom section | 1247 | 25.4% |
| **total** | **4908** | **100.0%** |

Code is down 280 bytes since M4b while `name` (+246), data (+91),
type (+34) and export (+23) grew: hello's size story is now the
runtime it links and the names it keeps, not its own code. The `name`
section alone is over 60% of the 2 KB target.

## Limits

- Bisect granularity: ±40 and smaller stretches were not split to
  single commits (timebox); the two ±900 moves are commit-exact.
- The −10 tail drift was not attributed; candidates are name-section
  and hash churn across 22 commits.
- Old-CLI (`-o`) vs package builds differ in module naming by a few
  bytes; only the +3 step spans the switch and is flagged above.
