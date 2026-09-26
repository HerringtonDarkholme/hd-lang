# F-201: 29 fixtures use undeclared requirement keys that the compiler wrongly accepts
Severity: major
Area: correctness
Evidence: audit/evidence/01-test-quality/selfcontain.tsv (flag `undeclared-requirement-key`, 29 files; `entry-with-requirement-row`, 13 files); audit/evidence/01-test-quality/probes.log (`fn main() -> i32 $ Zork: read()` passes `hd check` and `hd run` prints 1)
Effect: fixtures such as compiler-types/requirements/abi.hd write `$ Clock`, `$ Logger`, `$ Backup` without declaring those traits. spec/11 "Requirement keys are traits" and entry rows "may contain only host capability traits declared by the selected runtime profile". A conforming implementation rejects these files, so 29 portable cases fail on it for a reason unrelated to their claims. The 13 runtime cases also run only because `hd run` fabricates `{ requirement }` provider objects for any row on a non-`pub` `main`.
Recommendation: implementation change (reject an undeclared requirement key; decide how a non-entry `main` with a row is invoked), and fix the fixtures to declare the traits and select a profile. spec/README.md also has no code for an unknown type or trait name.
