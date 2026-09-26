# F-162: `hd run` and `hd test` crash with stack traces on entry-shape errors
Severity: minor
Area: runtime
Evidence: audit/evidence/01-special-casing/crash-triggers.log (k4, k5); audit/evidence/01-special-casing/renamed-variants.log (r3)
Effect: `fn main() -> string: "x"` and `fn main(x: i32) -> i32: x` both pass `hd check`. Then `hd run` throws an uncaught `Error` ("main has no runnable export", "main must not declare ordinary parameters") with a Node stack trace. A scenario fixture without `cleanup_ran` crashes the same way. Users get an internal-looking crash, not a diagnostic.
Recommendation: implementation change: report entry-shape problems as structured CLI errors with a stable code, or reject them in `check`.
