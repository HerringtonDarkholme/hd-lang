# F-560: every CLI command loads binaryen.js, even commands that emit no Wasm
Severity: note
Area: architecture
Evidence: audit/evidence/05-requirements/cli-floor.md, audit/evidence/05-requirements/fixture-loop.md (recorded under 5x CPU load); re-measured 2026-09-26 on an idle machine
Effect: src/compiler.ts imports src/wasm.ts, which imports `binaryen` at the top, so `hd parse` and `hd check` pay for it. Under the audit's load, `hd parse` on a one-line file took 2.1-3.3 s and 136 of 551 fixture invocations exceeded 1 s. Re-measured on an idle machine, `hd parse` on a two-line file takes 0.33 s, of which `await import('binaryen')` alone is 0.2 s. The 1 s edit-loop goal is met; the import is still about 60% of the time of a command that never emits Wasm.
Recommendation: implementation change, low priority. Import `src/wasm.ts` lazily, only for commands that emit Wasm, and give fixture runners a persistent or in-process mode.
