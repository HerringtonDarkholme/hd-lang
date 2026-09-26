# F-504: The optional carrier allocates for every `nil`, every loop step, and every map get
Severity: minor
Area: runtime
Evidence: audit/evidence/05-object-model/wat/om-optional-result.wat (`$f0`), wat/om-list.wat (`$f0`); audit/evidence/05-object-model/alloc-per-iteration.tsv (loop_for_concrete); src/emitter/runtime/map.wat `$hd.map_get`
Effect: `nil` is `struct.new $hd.variant (i32.const 0) (ref.null any)` each time, not a null reference or a shared singleton. A present `i32?` costs 2 allocations: carrier plus box. A `for value in list` loop allocates one carrier per element, and `map.get` allocates one per lookup. Fieldless enum variants, by contrast, are shared globals.
Recommendation: implementation change. Emit `nil` as one immutable global, or as `ref.null` where the optional does not nest. Lower `for` over `list` and `map` to an index loop without the `Option` protocol. OPEN_ISSUES question: see the phase-7 question on unboxed `T?`.
