# F-316: A `pub fn` inside an inherent `impl` is rejected with `doc-comment-without-target`
Severity: minor
Area: correctness
Evidence: audit/evidence/03-fuzz/replay.txt (F-316 block); round 2 parse signature 17 (audit/evidence/03-fuzz/round2/parse/signatures.tsv)
Effect: `impl Point:` with `pub fn get(self) -> i32: self.x` fails `hd parse` with "documentation comments must attach to a declaration or member". The file has no comment. The grammar allows `method_decl = [ "pub" ], "fn", ...`, and `spec/conformance/parse/valid/declarations.hd` line 23 uses the form. That fixture is unselected, and it fails earlier on `type`. Users can't declare public methods, and the message points at a comment that doesn't exist.
Recommendation: implementation change: accept `pub` on inherent methods, or report an unsupported-feature diagnostic. Never report a doc-comment code when there is no comment.
