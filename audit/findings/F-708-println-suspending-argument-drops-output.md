# F-708: A bang call inside a `println` argument drops all console output of `main!`
Severity: major
Area: correctness
Evidence: spec/conformance/runtime/valid/println-suspending-call-argument.hd (`run: stdout "" differs from expected "2\ndone\n"`); `hd build --wat` on the same program contains no console call.
Effect: `pub fn main!() -> void $ Console: println("${two!()}")` followed by `println("done")` passes `check` and exits 0 but prints nothing. Binding the result first (`x := two!()`, then `println("${x}")`) prints correctly, so suspension lowering loses the `println` calls when an argument suspends.
Recommendation: implementation change: lower a call whose argument contains a bang call through the CFG path like other suspending expressions, so the call and later statements are emitted.
