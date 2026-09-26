# F-361: Is a payload-free variant with shared enum data canonical for `is`?
Severity: note
Area: spec
Evidence: audit/evidence/03-blind-run/probes.log (p-shared-data-variant-canonical.hd returns 0: two `StatusCode.NotFound` occurrences are distinct; control p-fieldless-variant-canonical.hd returns 1); audit/evidence/03-blind-run/variants.log (a5-fieldless-variant.no-any.hd: `assertion-failed`)
Effect: the implementation allocates one value per occurrence, so `StatusCode.NotFound is StatusCode.NotFound` is false. spec/05-expressions.md says "A payload-free enum value is canonical for its variant", and 08-data-and-enums.md calls such a variant "an enum value ... selected without `()`". Yet 08 also evaluates shared defaults "for each construction" and allows assignment to shared fields through a mutable root, which a canonical value would make global.
Recommendation: OPEN_ISSUES question: are shared-data variants canonical? If so, are their shared fields readonly, and are defaults evaluated once?
