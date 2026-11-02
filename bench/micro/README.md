# Runtime Microbenchmarks

The same small program in hd, Python, and Node: recursive fib(30), the sum
of 1..10M, building a string of 100k parts, a map with 100k inserts and
lookups, and sorting 100k items. Every program checks its own result, so
dead-code elimination cannot skip the work.

```sh
node --experimental-strip-types bench/micro/run.ts
```

The runner builds each hd program with the Rust `hd`
(`target/release/hd build --release`, built with cargo when
missing) in a scratch package (which reports the release Wasm size), then
times seven runs of its entry point on Node, compile excluded, and prints
the minimum. The Python and Node programs time themselves the same way,
each in its own process. A missing `python3` or `node` skips that column.

A case's hd file spells `-` as `_` (`string-build` → `string_build.hd`),
since hd module paths are identifiers.

The latest measurements are in the
baseline report (deleted 2026-10-15; `git show 44890c30:audit/compiler/baseline-2026-10-06.md`, section 6.2).

## String and collection cost audit

The one-operation programs for #1208 and #1244 are separate from the five routine
microbenchmarks above, so they do not expand the metrics manifest or CI. Run all of them, or
name a subset:

```sh
node --experimental-strip-types bench/micro/costs.ts
HD_MICRO_RUNS=1 node --experimental-strip-types bench/micro/costs.ts string-trim list-get-string
```

Each hd source marks its target expression for the compiler's release `counts` view. The
results and method are in [the string and collection cost audit](../../docs/audit/string-collection-costs.md).
The companion `string-literal-census.ts` reports fixed string-literal lengths in the shipped
benchmark and example programs.
