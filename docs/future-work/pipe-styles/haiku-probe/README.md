# Haiku pipe-style probe (2026-10-24)

Claude Haiku wrote the same 12 shell-style tasks ([tasks.md](tasks.md)) once per pipe style, a fresh agent per style, using the same hypothetical `sh` function sheet ([common.md](common.md); F#'s is curried by its rule). Each style directory holds its rule sheet (`RULE.md`), the programs `t01.hd` to `t12.hd`, and the agent's `notes.md`. The `sh` functions do not exist, so nothing was compiled; the programs were graded by reading.

| Style | Wrong slot | Rule broken | Total chars | Chars in pipe lines | Closures written |
| --- | ---: | ---: | ---: | ---: | ---: |
| Elixir (value is the first argument) | 0 | 0 | 2,320 | 1,367 | 12 |
| F# (`x \|> f(a)` is `f(a)(x)`, curried std) | 0 | 0 | 2,213 | 1,227 | 8 |
| Hack (`_` exactly once per step) | 0 | 0 | 2,690 | 1,722 | 19 |
| Placeholder (bare name or `_`; `_` is also partial application) | 0 | 0 | 2,362 | 1,371 | 9 |

Observations:
- No style produced a slot or rule error in 48 programs; the agents' doubts were about the function sheet (bang functions, tie order, empty averages), not the pipe rule.
- Hack is about 25% longer in pipe lines: the agent wrapped even plain function arguments in closures (`map(_, fn(a: string) -> string: upper(a))`).
- F# is the most point-free (`map(pad(10))`), at the cost of a curried std.
- Placeholder's t08 writes `map(_, pad(_, 10))`: two `_` in one step with different meanings (the pipe value, and a nested partial application). It follows the rule, but it is a readability question for the stress test.
