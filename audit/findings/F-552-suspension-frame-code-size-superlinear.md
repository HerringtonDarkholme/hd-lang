# F-552: suspension lowering code size grows super-linearly with bang-call sites
Severity: major
Area: architecture
Evidence: audit/evidence/05-requirements/susp-probes.md "Frame code size versus suspension sites" and "CFG lowering code size versus sites" (`node --experimental-strip-types audit/scripts/arch/susp-probes.ts`), audit/evidence/05-requirements/susp-shape.wat, audit/evidence/05-requirements/susp-cfg-2sites.wat
Effect: one function with N sequential `x := step!(x)` statements emits a poll function whose size grows about N^3. 32 sites: 2.5 MB of WAT, 182 KB of Wasm. 48 sites: 9.3 MB WAT, 549 KB Wasm, and compile takes 1.17 s in the recorded run (emit 152 ms, assemble 1013 ms). A run under heavier load took 4.3 s. The CFG lowering (`step!(x) + 0`) grows about N^2: 48 sites give 229 KB of Wasm.
Recommendation: implementation change. Emit each continuation once and enter it through one dispatch, and spill only locals live across the site:

```wat
(block $s2 (block $s1 (block $s0
  (br_table $s0 $s1 $s2 (local.get $resume-state)))
  ;; site 0 code, falls through to site 1
  ) ;; site 1 code ...
```

Mechanism: the linear lowering copies the remainder of the body into every resume branch, and every suspension point stores every frame local.
