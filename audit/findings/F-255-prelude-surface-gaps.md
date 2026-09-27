# F-255: Prelude names `Hash` and `Hasher` are unknown
Severity: minor
Area: coverage
Evidence: the 2 cases tagged F-255 in test/portable/KNOWN_FAILURES.tsv, typing/valid/prelude-surface.hd (`unknown-trait 'Hash'`) and typing/valid/generic-map-key.hd (`unknown-trait 'Hash'`) (re-checked 2026-09-27)
Effect: Programs using the prelude traits `Hash` and `Hasher` named by
10-modules.md#prelude fail with generic `unknown-trait`, and map keys bounded
by `K < Eq + Hash` cannot be written. `Any` and `Eq` (owner decision EQ-1,
applied 2026-09-27) are implemented.
Recommendation: implementation change: add `Hash` and `Hasher`.
