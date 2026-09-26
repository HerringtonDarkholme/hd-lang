# F-201: The compiler accepts an undeclared requirement key on a non-entry function
Severity: minor
Area: correctness
Evidence: `hd check` on `fn f() -> i32 $ Zork: 1` plus `pub fn main() -> void` prints `ok`
Effect: spec/11 says "Requirement keys are traits", and spec/README.md defines
`unknown-trait`. hd accepts any unresolved name in a non-entry row; entry rows are
checked (`nonhost-entry-requirement`). `hd run --entry NAME` still fabricates
`{ requirement }` provider objects for such rows. Implementation fixtures
test/fixtures/requirements/{closure-provider,transitive-call-paths,loop-exit,lexical-override}.hd
rely on this.
Recommendation: implementation change: report `unknown-trait` for an unresolved
requirement key, and declare the traits in those fixtures.
