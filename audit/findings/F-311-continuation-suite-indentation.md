# F-311: hd accepts a bracketed nested suite whose body is not indented past its header
Severity: minor
Area: correctness
Evidence: audit/evidence/03-fuzz/replay.txt (F-311 block); round 1 parse signature unexpected-indentation
Effect: inside `apply(`, a closure header `fn(a: i32):` followed by `println(a)` at the same indentation passes `hd parse`. The reference parser reports `unexpected-indentation`. Chapter 01: "Its first body line must be indented farther than that reference." Programs whose layout differs between implementations get different parses.
Recommendation: implementation change: enforce the nested-suite indentation rule. Also add `unexpected-indentation` to the spec inventory (see F-313).
