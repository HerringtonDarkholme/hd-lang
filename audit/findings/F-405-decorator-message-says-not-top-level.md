# F-405: Every decorator is rejected as "only valid on top-level declarations"
Severity: minor
Area: correctness
Evidence: audit/evidence/04-runtime/diagnostics-probes.log (`d1-top-level-decorator.hd`); audit/evidence/04-runtime/diagnostics-sample.log (`configured-facet-not-annotator.hd`)
Effect: `@enabled` directly above a top-level `fn` reports `4:1: decorator-not-top-level: decorators are only valid on top-level declarations`. The decorator is already top level, so the message points the user the wrong way. The expected code for that fixture is `decorator-not-annotator`.
Recommendation: implementation change: until annotations land, report a dedicated unsupported-feature diagnostic for decorators at top level. Keep `decorator-not-top-level` for decorators inside suites.
