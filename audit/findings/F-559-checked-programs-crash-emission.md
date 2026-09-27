# F-559: A type-checked multi-provider use crashes Wasm emission with an internal error
Severity: minor
Area: correctness
Evidence: audit/evidence/05-requirements/emit-crash.md (historical); current repro below (re-checked 2026-09-26)
Effect: `hd check` accepts the program, but `hd build`/`test`/`run` exit 1 with a JS stack trace and no diagnostic. A multi-provider use that mixes a host provider with a source provider, such as `con, c := $.use(Console, Clock)` inside `pub fn main() -> void $ Console` under `$.with(Clock=...)`, throws `cannot unbox 'provider:Console'` in src/emitter/context.ts. The other crash recorded in the evidence (`resource-disposed-result.hd` failing the Binaryen validator) no longer occurs: the program builds once its declarations are public and its body is not the entry point.
Recommendation: implementation change. Box `externref` providers before storing them in the tuple list. Report emitter failures as a structured internal diagnostic that includes Binaryen's message.
