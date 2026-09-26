# F-505: `list[i32]` stores boxes even in concrete code, so every store allocates
Severity: note
Area: architecture
Evidence: audit/evidence/05-object-model/erasure-bench.txt, erasure-bench-run{1,2,3}.txt (insertion sort); audit/evidence/05-object-model/wat/om-list.wat (`$f2` build); audit/probes/arch/erasure/erasure-shared-storage.hd (run result 82)
Effect: the concrete `sort_i32` allocates 149 boxes per element. Each `values[j] = tmp` re-boxes the value it just unboxed. It is no faster than the erased generic sort: generic/concrete time ratio 0.26 to 1.02 over six runs. The reason is that the generic version moves boxes without unboxing. This follows the plan's "Primitive values are boxed in erased positions". The shared-storage probe shows the constraint: a concrete `list[(i32, i32)]` mutated by generic code must use the erased element layout.
Recommendation: OPEN_ISSUES question. Should `list[i32]` and `list[f64]` get specialized unboxed storage, with conversion at generic boundaries? If so, how does that interact with list identity and in-place mutation from generic code?
