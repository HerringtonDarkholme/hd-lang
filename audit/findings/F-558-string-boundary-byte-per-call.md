# F-558: strings cross the host boundary one byte per import call
Severity: minor
Area: runtime
Evidence: audit/evidence/05-requirements/rtlib-strings.md (`node --experimental-strip-types audit/scripts/arch/rtlib-strings.ts`), audit/evidence/05-requirements/rtlib-imports.md
Effect: a host-provider round trip of a 1 MiB string takes about 374 ms (250-510 ns per byte). `println` of 1 MiB takes about 22 ms (about 20 ns per byte). Each byte is one Wasm-to-JS call: `host_T_M_argument_byte`, `host_T_M_result_byte`, or `console_byte`.
Recommendation: implementation change. Move bytes in bulk, for example through an exported memory buffer or a length-prefixed copy, which is also what the Component Model canonical ABI requires for strings.
