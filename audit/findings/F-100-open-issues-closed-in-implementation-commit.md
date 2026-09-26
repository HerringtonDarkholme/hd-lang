# F-100: The first implementation commit closed three open design issues with no fixtures
Severity: minor
Area: spec
Evidence: audit/evidence/01-spec-edits/hunks.tsv (H1a, H2, H3, H5); audit/evidence/01-spec-edits/probe-runs.log; `git show 4cc1312:future-work/OPEN_ISSUES.md` lines 11-31, 33-51, 97-114; `git log -S"### Provider Escape And Authority Visibility" -- future-work/OPEN_ISSUES.md` returns 1e55fc9.
Effect: Commit 1e55fc9 added `src/`. The same commit deleted three `OPEN_ISSUES.md` entries and wrote normative text in chapters 07, 11, 12, and 14:
- "Provider Escape And Authority Visibility". The base entry recommended prototyping second-class providers and said "Do not rely on wording alone." Chapter 11 now makes provider values ordinary values that stay usable after their scope, which is the reworded-claim option. The MVP already accepts this (probes `h2-provider-escape-*.hd` pass).
- "Purity In Function Types". The base entry recommended an explicit `pure` qualifier. Chapters 07 and 14 now say "function types do not carry purity."
- "Task Combinator Extensibility". Chapter 11 now makes `all!`/`race!` compiler intrinsics. This follows the entry's recommendation, but the MVP still rejects both (`unsupported-task-combinator`).
The OPEN_ISSUES "Resolution Process" requires valid and invalid fixtures for each resolution. No window commit adds a fixture for returned, stored, or global provider values, for intrinsic `all!`/`race!`, or for the `pure` decision. A reader cannot tell whether the owner or the implementer made these three decisions.
Recommendation: OPEN_ISSUES question: did the owner accept each of the three resolutions? If yes, add the fixtures the resolution process requires. If no, restore the entries.
