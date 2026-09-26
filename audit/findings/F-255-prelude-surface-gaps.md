# F-255: Prelude names `Any`, `Eq`, `Hash`, and `Hasher` are unknown
Severity: minor
Area: coverage
Evidence: audit/evidence/02-coverage/unselected-classified.tsv rows typing/valid/prelude-surface.hd (`unknown-trait 'Hash'`), typing/valid/generic-map-key.hd (`unknown-trait 'Eq'`), typing/invalid/nil-to-any.hd (`unknown-type 'Any'`)
Effect: Programs using prelude traits named by 10-modules.md#prelude fail with generic
`unknown-trait`/`unknown-type`. `nil-to-any.hd` cannot reach its `nil-to-nonoptional`
check. Map keys bounded by `K: Eq + Hash` cannot be written.
Recommendation: implementation change: add the prelude declarations, or reject them
with a structured unsupported code.
