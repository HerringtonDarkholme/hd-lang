# Runtime Microbenchmarks

The same small program in hd, Python, and Node: recursive fib(30), the sum
of 1..10M, building a string of 100k parts, a map with 100k inserts and
lookups, and sorting 100k items. Every program checks its own result, so
dead-code elimination cannot skip the work.

```sh
node --experimental-strip-types test/perf/micro/run.ts
```

The runner builds each hd program with `hd build --release` in a scratch
package (which reports the release Wasm size), then times three runs of its
entry point, compile excluded, and prints the median. The Python and Node
programs time themselves the same way, each in its own process. A missing
`python3` or `node` skips that column.

A case's hd file spells `-` as `_` (`string-build` → `string_build.hd`),
since hd module paths are identifiers.

Findings are in [../../audit/compiler/perf-audit.md](../../audit/compiler/perf-audit.md),
under "Runtime Microbenchmarks And Wasm Size".
