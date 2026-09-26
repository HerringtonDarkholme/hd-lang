# F-501: `map[K, V]` is an unhashed association list with O(n) get and insert
Severity: major
Area: runtime
Evidence: audit/evidence/05-object-model/timing-map-string.txt (`node --experimental-strip-types audit/scripts/arch/om-timing.ts`); audit/evidence/05-object-model/wat/om-map.wat; `$hd.map_get` / `$hd.map_insert` in src/emitter/runtime/map.wat
Effect: one `get` costs 22 ns at 10 entries, 383 ns at 1,000 and 2,490 ns at 10,000. Building a 10,000-entry map with `m[i] = i` takes 131 ms, and 0.6 ms at 1,000, so building is quadratic. Every insert scans all keys before appending. Every `remove` scans and shifts. Keys are boxed and compared one by one: `i32` by value, strings by `$hd.string_compare`. There is no hash.
Recommendation: implementation change. Use a hash index next to the insertion-ordered key and value arrays, keeping iteration order. OPEN_ISSUES question: should the spec state complexity expectations for `map` get, insert, and remove?
