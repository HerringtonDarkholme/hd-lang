# F-307: A `main` returning `Result[void, E]` passes check but cannot run
Severity: major
Area: correctness
Related: F-162 (same crash message for `-> string` main); here the signature is a spec-defined entry form
Evidence: audit/evidence/03-fuzz/replay.txt (F-307 block); round 1 contract signatures 08/09; also `hd run spec/conformance/typing/valid/console-error-entry.hd`
Effect: `pub fn main() -> Result[void, ConsoleError]: Ok()` passes `check`. `run` and `test` crash with `Error: main has no runnable export`. Chapters 07 and 10 name this as one of the two standard entry signatures. The selected fixture `typing/valid/console-error-entry.hd` is checked but never run.
Recommendation: implementation change: support Result-returning entry points, or reject them with a located unsupported diagnostic at `check`.
