# F-255: Prelude names `Any`, `Eq`, `Hash`, and `Hasher` are unknown
Severity: minor
Area: coverage
Evidence: the 9 cases tagged F-255 in test/portable/KNOWN_FAILURES.tsv, for example typing/valid/prelude-surface.hd (`unknown-trait 'Hash'`), typing/valid/generic-map-key.hd (`unknown-trait 'Eq'`), typing/invalid/none-to-any.hd (`unknown-type 'Any'`) (re-checked 2026-09-26)
Effect: Programs using prelude traits named by 10-modules.md#prelude fail with generic
`unknown-trait`/`unknown-type`. `none-to-any.hd` cannot reach its
`missing-contextual-enum-type` check, and decision A3 (optionals may be erased
to `Any`) cannot be exercised. Map keys bounded by `K < Eq + Hash` cannot be
written.
Recommendation: implementation change: add the prelude declarations, or reject them
with a structured unsupported code.
