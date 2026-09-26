# F-360: The reference parser rejects `$.with(Tag=x, ...ctx)`
Severity: minor
Area: spec
Evidence: audit/evidence/03-blind-run/refparser.log (amb03-context-spread-after-binding.hd: `argument-order@7`; `hd parse` and `hd check` accept; control amb03-control-spread-first.hd accepted by both)
Effect: a context spread after an explicit binding is rejected, but spec/11-requirements-and-suspension.md `context_entries = context_entry, { ",", context_entry }` allows any order, and the text says "Context spreads and explicit bindings are applied left to right, and the later binding wins". The call-argument order rule is applied to context entries.
Recommendation: spec change: exempt `$.with(...)` and `$.context(...)` from the argument-order check; add a parse/valid fixture with the spread last.
