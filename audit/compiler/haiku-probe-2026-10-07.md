# Haiku probe, 2026-10-07

A Haiku model wrote five hd programs from `guide/LEARN_IN_10_MINUTES.md`,
`guide/LANGUAGE_TOUR.md` and `guide/WORKING_WITH_DATA.md` only. Programs are in
`audit/compiler/haiku-probe-2026-10-07/`. Mistakes are in `audit/hd-writing-log.md` (rows dated 2026-10-07).

| program | attempts to work on old compiler | mistakes logged | works on new compiler? (or its error) | old compiler time (s) | new compiler time cold / warm (s) | output matches between compilers? |
| --- | --- | --- | --- | --- | --- | --- |
| words.hd | 6 (5 failed; the first was the wrong command, `pnpm hd run FILE`) | 5 | no: `missing-entry-point: probe.words has no fn main` | 1.08 | n/a | n/a |
| bank.hd | 1 | 0 | no: `unsupported: Body: a data literal spread` | 1.01 | n/a | n/a |
| shapes.hd | 2 (1 failed) | 1 | no: `missing-entry-point: shapes.shapes has no fn main` (run from a copy outside `audit/compiler/haiku-probe-2026-10-07/`) | 1.03 | n/a | n/a |
| tokens.hd | 1 | 0 | no: `missing-entry-point: tokens.tokens has no fn main` (run from a copy outside `audit/compiler/haiku-probe-2026-10-07/`) | 1.05 | n/a | n/a |
| primes.hd | 1 | 0 | no as written: `missing-entry-point: primes.primes has no fn main`. A copy with `pub fn main() -> void $ Console:` around the final `println` works | 1.01 | 0.07 / 0.06 (on the copy) | yes on the copy: `primes below 200000: 17984` from both |

Notes on the timings and runs:
- Old compiler times are `pnpm hd FILE.hd`, median of 3 with `/usr/bin/time -p`. Most of the time is pnpm and Node startup.
- Running `hd run audit/compiler/haiku-probe-2026-10-07/X.hd` in place compiles every `.hd` file in `audit/compiler/haiku-probe-2026-10-07/` together, so the errors name sibling files (for example `words.hd:900..924 missing-requirement`). The "run from a copy" rows ran each file alone from `/tmp/haiku-audit/compiler/haiku-probe-2026-10-07/`.
- The primes copy is a scratch file outside the repo. Its "cold" runs changed the file content first, and the cold and warm medians are close (0.07 and 0.06), so the cache effect is small or the cold runs were not cold.
- The task said `pnpm hd run FILE.hd`; that command is wrong and the old compiler says to use `pnpm hd FILE.hd`.

## Five points

- **Hardest part:** unsigned arithmetic (`usize`). A descending sort key `0 - count` panicked at runtime with integer overflow, and the mutability rules (`mut` on a primitive is rejected) took two fixes.
- **Most helpful error:** `missing-let`. It names the exact fix (`let x: T = ...`).
- **Most helpful error (second):** `syntax-error ... a tuple element is written '._1'`. It gave the exact replacement.
- **Least helpful error:** `integer-overflow: runtime panic`. It gives a column and no operands, and it appears only at run time.
- **Least helpful on the new compiler:** `error: :0..0: error unsupported: ...`. These point at no line in the file, and in-place runs report errors from sibling files.
