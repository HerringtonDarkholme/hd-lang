# F-503: String interpolation lowers to pairwise concatenation and re-allocates literals
Severity: minor
Area: runtime
Evidence: audit/evidence/05-object-model/alloc-per-iteration.tsv (loop_interpolation); audit/evidence/05-object-model/wat/om-string.wat (`$f3` interp_fresh); audit/evidence/05-object-model/timing-map-string.txt
Effect: `"item ${i} of ${n}"` allocates 7 byte arrays per evaluation: 2 literal copies, 2 `i32_to_string` results, and 3 intermediate concatenations. A k-part interpolation copies earlier bytes k-1 times. String literals are rebuilt with `array.new_fixed` on every evaluation. Accumulating `text = "${text}x"` is quadratic: 0.29 ms at 1k, 38 ms at 10k, 3.3 s at 100k. That growth matches `text + "x"`, and immutable strings make it expected.
Recommendation: implementation change. Compute the total length of all parts, allocate once, and copy each part once. Hoist literal byte arrays into immutable globals. Note that `string_len` is also O(n), because it counts scalars on each call.
