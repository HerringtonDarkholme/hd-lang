# F-560: every CLI command loads binaryen.js, which dominates the edit loop
Severity: minor
Area: architecture
Evidence: audit/evidence/05-requirements/cli-floor.md, audit/evidence/05-requirements/fixture-loop.md
Effect: `hd parse` on a one-line file takes 2.1-3.3 s under this audit's CPU load, and `node -e "await import('binaryen')"` alone takes 0.9-1.8 s. In-process, compile, validate and Wasm compile take at most 90 ms per fixture (median 11 ms). The 1 s goal is lost to process startup: 136 of 551 fixture invocations exceeded 1 s.
Recommendation: implementation change. Import `src/wasm.ts` lazily, only for commands that emit Wasm, and give fixture runners a persistent or in-process mode. Load was 5x the core count, so re-measure on an idle machine before prioritizing.
