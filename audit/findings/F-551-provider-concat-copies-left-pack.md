# F-551: `$hd.provider_concat(left, null)` copies the left pack instead of sharing it
Severity: minor
Area: runtime
Evidence: audit/evidence/05-requirements/constructs-alloc.md (function `$f6`, the `subtract[r]` body), audit/evidence/05-requirements/abi.wat (`$f3`), src/emitter/runtime/runtime.wat `$hd.provider_concat`
Effect: every `$.with` scope entered inside a row-generic function allocates one node per entry of the incoming row, even though packs are never mutated. A row-subtraction scope over a 10-entry row allocates 11 structs instead of 1.
Recommendation: implementation change. Return `left` when `right` is null, since pack nodes are immutable:

```wat
(if (ref.is_null (local.get $right)) (then (return (local.get $left))))
```
