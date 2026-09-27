# F-354: Readonly `Map[K, V]` is not covariant in `V`
Severity: minor
Area: correctness
Evidence: audit/probes/blind-triage/p-map-value-covariance.hd and p-map-value-covariance-readonly-outer.hd (`type-mismatch: expected Map[string,User], found Map[string,mut:User]`; control p-list-element-covariance.hd returns 1)
Effect: passing `Map[string, mut User]` to a `Map[string, User]` parameter is rejected. The same conversion for `List` works. spec/04-type-system.md#variance: "Readonly `Map[K, V]` is invariant in `K` ... and covariant in `V`".
Recommendation: implementation change: apply the list element covariance rule to the map value argument; add a portable case.
