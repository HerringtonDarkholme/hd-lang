# Review Of New `src/` Commits

Codex's ongoing review (job J in [codex_task.md](codex_task.md)). Only open
findings stay here; a finding is deleted once it is fixed. Past passes are
in git history.

Reviewed through `a3f8e3b3` (2026-10-04): three passes, no findings.

Reviewed through `d094040f` (2026-10-05): 16 commits, no findings.
Pass covered the package-mode series (`5ee572be`, `e2b6326e`,
`0433f6fb`, `4ad3f285`, `e6d47a23`, `9769d91f`, `1c467c3f`,
`ee893489`), the module-path series (`09dc77d8`, `823c186b`,
`d094040f`), the cleanup line (`4c95e66f`), and Muse's own
`32856a65`, `e4208b2f`, `c4642cc5`, `3f647f8a`. Every `r[…]` rule ID
and every non-rule spec anchor cited in messages and new comments was
checked against the spec text; all resolve (the two apparent misses,
`cli.exe.unselected` and `cli.profile.flag`, are prefixes of the
cited `cli.exe.unselected-main` and `cli.profile.flag-only`). The
`__pkg_` hidden prefix introduced by `d094040f` appears in no
diagnostic of the 1085 swept invalid fixtures. The literal-join and
overflow commits the queue names remain covered by the earlier
passes.

Reviewed through `6c8344b5` (2026-10-05): 72 commits, two findings.
The pass covered the dependency, workspace, host-profile, test-tier,
documentation-test, debug-printing, `usize`-display, inference, `void`,
generic-pattern, typed-fact, and GADT series, plus their review follow-ups.
Every active rule ID cited in a commit message resolves in the current
specification; cited retired IDs are absent as intended. The literal-join
and overflow commits named by the queue remain covered by the earlier
passes.

Reviewed through `4567de8a` (2026-10-05): one further commit, no new
findings. The distinct-`usize` checker and emitter changes match
`types.usize.primitive`, `types.num.same-width`, the literal-default and
index rules, and the companion spec commit `05404d28`; the later shift-count
owner decision is queued separately as BS.

## Open Findings

1. **Blocker.** Effect: two `hd` processes fetching the same version can
   both pass the `existsSync` check and one then fails its `rename` with an
   uncaught filesystem error. In another valid interleaving, the second
   writer replaces the hash record before noticing the first writer's tree,
   leaving the cache with one tree and the other tree's hash. A later
   `hd.sum` comparison can therefore approve bytes whose hash it never
   checked, contrary to `cli.cache.shared`, `cli.cache.complete`, and
   `cli.cache.hash`. Fix: publish the tree and its hash as one collision-safe
   unit (or serialize writers with a per-entry lock); on losing a publish
   race, discard the staged pair and read the winner, never overwrite one
   half independently. File: `src/dependencies/cache.ts:119`.
2. **Non-blocking.** Effect: `readSum` accepts a non-host key, a non-version,
   an arbitrary suffix after the version, duplicate or unsorted entries, and
   a final line without a newline. Such a file is outside `cli.sum.line` and
   `cli.sum.order`, but commands treat it as a valid `hd.sum`; duplicate
   hashes silently use the last one. Fix: parse each key as a host path plus
   version and an optional exact `/hd.toml`, reject duplicate/out-of-order
   keys, and require the final newline. File: `src/dependencies/sum.ts:40`.
