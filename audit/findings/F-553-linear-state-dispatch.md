# F-553: resume and CFG dispatch use linear `if` chains instead of `br_table`
Severity: minor
Area: runtime
Evidence: audit/evidence/05-requirements/susp-shape.wat (`$poll1`), audit/bench/b4-suspend.dev.wat and audit/bench/b4-suspend.O2.wat (`$poll1`, `loop $cfg`)
Effect: resuming at site k compares the state k times, and every CFG block transition re-walks the `if (pc == n)` chain from the top. The 21-block loop in b4-suspend does about 200 comparisons per iteration, and binaryen -O2 keeps the chain.
Recommendation: implementation change. Dispatch on `$resume-state` and `$pc` with `br_table`, or emit structured control flow for blocks that never suspend.
