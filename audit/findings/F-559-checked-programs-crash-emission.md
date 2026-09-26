# F-559: type-checked programs crash Wasm emission with an internal error
Severity: minor
Area: correctness
Evidence: audit/evidence/05-requirements/emit-crash.md (historical); current repros below
Effect: `hd check` accepts these programs, but `hd build`/`test`/`run` exit 1 with a JS stack trace and no diagnostic:
- `spec/conformance/typing/valid/resource-disposed-result.hd` fails the Binaryen validator in `poll0`: "return value should be a subtype of the function result type". The suite only checks this file.
- A multi-provider use that mixes a host provider with a source provider, such as `con, c := $.use(Console, Clock)` inside `pub fn main() -> void $ Console` under `$.with(Clock=...)`, throws `cannot unbox 'provider:Console'` in src/emitter/context.ts.
Recommendation: implementation change. Box `externref` providers before storing them in the tuple list. Report emitter failures as a structured internal diagnostic that includes Binaryen's message.
