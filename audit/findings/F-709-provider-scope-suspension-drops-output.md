# F-709: A `$.with` block containing a bang call drops all console output of `main!`
Severity: major
Area: correctness
Evidence: spec/conformance/runtime/valid/println-around-suspending-provider-scope.hd (`run: stdout "" differs from expected "before\n2\nafter\n"`).
Effect: in `pub fn main!() -> void $ Console`, a `$.with(Clock=...)` block that calls a suspending function makes every `println` in `main!` print nothing, including one before the block. The same program without the bang call prints all three lines, and moving the `$.with` block into a helper `fn!` works.
Recommendation: implementation change: when suspension lowering installs a provider scope, keep the entry row's providers (here `Console`) in the resumed context.
