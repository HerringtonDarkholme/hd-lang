# F-354: Readonly `map[K, V]` is not covariant in `V`
Severity: minor
Area: correctness
Evidence: audit/probes/blind-triage/p-map-value-covariance.hd and p-map-value-covariance-readonly-outer.hd (`type-mismatch: expected map[string,User], found map[string,mut:User]`; control p-list-element-covariance.hd returns 1)
Effect: passing `map[string, mut User]` to a `map[string, User]` parameter is rejected. The same conversion for `list` works. spec/04-type-system.md#variance: "Readonly `map[K, V]` is invariant in `K` ... and covariant in `V`".
Recommendation: implementation change: apply the list element covariance rule to the map value argument; add a portable case.
