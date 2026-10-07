# New Compiler: Architecture And Design

The design documents of the new compiler, in reading order.

- [goals.md](goals.md): goals, the Arena, metrics, feature triage and decisions.
- [research.md](research.md): the architecture research and its open questions.
- [prior-art-issues.md](prior-art-issues.md): known issues of prior compiler front ends.
- [design-overview.md](design-overview.md): how to read the design, the pipeline and the crate graph (§1 to §2).
- [data-structures.md](data-structures.md): IDs, interners, types, arenas, data-oriented encoding and IR contracts (§3).
- [syntax.md](syntax.md): lexer, layout, skim and header pass, parser, header extraction and item index (§4.1 to §4.6).
- [resolution-and-interfaces.md](resolution-and-interfaces.md): discovery, folder graph, name resolution, folder interface, blob, traits and coherence (§4.7 to §4.12).
- [checking-and-tir.md](checking-and-tir.md): body checking summary, the TIR definition, diagnostics and limits (§4.13 to §4.15).
- [type-checking.md](type-checking.md): the detailed type-checking design (frontend lane).
- [trait-solver.md](trait-solver.md): the trait solver design (frontend lane).
- [cache.md](cache.md): the cache (§5).
- [scheduler.md](scheduler.md): the scheduler and task graph (§6).
- [commands.md](commands.md): command flows (§7 and §20).
- [testing-the-compiler.md](testing-the-compiler.md): determinism and soundness tests (§8 and §21).
- [build-order.md](build-order.md): build order (§9 and §22).
- [codex-review-response.md](codex-review-response.md) and [codex-review-response-frontend.md](codex-review-response-frontend.md): verdicts on the Codex review of 8bb6860d, backend and frontend rows.
- [codex-rereview-response.md](codex-rereview-response.md): verdicts on the Codex re-review through 54f249b7, backend rows and owner questions.
- [codex-rereview-response-frontend.md](codex-rereview-response-frontend.md): frontend verdicts on the Codex re-review through 54f249b7.
- [open-questions.md](open-questions.md): open questions, inconsistencies and the changes D2 made to D1 (§10 and §23).
- [codegen.md](codegen.md): the back half overview, emitting from TIR, monomorphization and merging (§11 to §13).
- [suspension.md](suspension.md): suspension lowering (§14).
- [wasm-layout.md](wasm-layout.md): Wasm GC layout and emission (§15).
- [lowering-catalog.md](lowering-catalog.md): the lowering of every data type and syntax form, with code speed and size per entry, the numbering rules and the named optimization passes, merged from [representation-runtime.md](representation-runtime.md) and [representation-compile.md](representation-compile.md).
- [runtime-and-host.md](runtime-and-host.md): the runtime, host interface and embedding (§16 to §17).
- [engines-and-test-runner.md](engines-and-test-runner.md): execution engines, tiers and the test runner (§18 to §19).
- [skeleton-findings.md](skeleton-findings.md): what the walking skeleton (a tiny subset through every stage to Wasm GC on V8) found about the design, 2026-10-07.
- [live-execution.md](live-execution.md): live execution: the REPL session model, redefinition, the execution journal, replay and resume (design, not decided).
