# F-255: Prelude names `Eq`, `Hash`, and `Hasher` are unknown
Severity: minor
Area: coverage
Evidence: the 2 cases tagged F-255 in test/portable/KNOWN_FAILURES.tsv, typing/valid/prelude-surface.hd (`unknown-trait 'Hash'`) and typing/valid/generic-map-key.hd (`unknown-trait 'Eq'`) (re-checked 2026-09-27)
Effect: Programs using the prelude traits `Eq`, `Hash`, and `Hasher` named by
10-modules.md#prelude fail with generic `unknown-trait`, and map keys bounded
by `K < Eq + Hash` cannot be written. `Any` is implemented.
Recommendation: implementation change once owner decision EQ-1 (one `Eq`
trait, `PartialEq` dropped; audit/types/QUESTIONS.md) is applied to the
specification, since it changes the comparison traits these cases use.
