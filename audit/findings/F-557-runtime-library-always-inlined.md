# F-557: every module embeds the full runtime library, used or not
Severity: minor
Area: architecture
Evidence: audit/evidence/05-requirements/rtlib-minimal.md and rtlib-hello.md (`node --experimental-strip-types audit/scripts/arch/rtlib-inventory.ts FILE`), audit/evidence/05-requirements/rtlib-assemble.md
Effect: `fn main() -> i32: 0` emits 28 functions, 9 globals, 47 types and a `hd.panic` import. Binaryen's remove-unused-module-elements drops 27 functions, all 9 globals, 46 types and the import, taking the module from 2458 B to 37 B. Hello-world keeps 2 of 29 functions. Assembling the emitted minimal module takes 5.5 ms, against 0.12 ms for a hand-written equivalent (best of 30). Assembly is about 90% of in-process compile time across the fixture corpus (fixture-loop.tsv).
Recommendation: implementation change. Emit runtime helpers on demand, as the float, console, split and transform pieces already are, or run remove-unused-module-elements after assembly.
