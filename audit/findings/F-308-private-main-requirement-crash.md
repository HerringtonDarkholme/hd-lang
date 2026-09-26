# F-308: A non-`pub` `main` with a non-host requirement passes check and crashes `run`
Severity: minor
Area: correctness
Evidence: audit/evidence/03-fuzz/replay.txt (F-308 block); round 1 contract signatures 10/11 (seed typing/invalid/nonhost-entry-requirement.hd with `pub` deleted)
Effect: `trait Database` plus `fn main() -> void $ Database: pass` passes `check`. `run` then dies with `TypeError: type incompatibility when transforming from/to JS`. With `pub`, `check` reports `nonhost-entry-requirement`. So `run` treats a private `main` as the entry, but `check` does not apply entry rules to it.
Recommendation: implementation change: use one entry definition in both `check` and `run`. OPEN_ISSUES question: is a non-`pub` `main` an entry point? Chapters 07 and 10 show only `pub fn main`, but the MVP fixtures use `fn main() -> i32`.
