# F-310: A line starting with `:` is parsed as a trailing block on the previous statement
Severity: minor
Area: correctness
Evidence: audit/evidence/03-fuzz/replay.txt (F-310 block); round 1 parse signatures syntax-error (14) and trailing-block-position (4)
Effect: after `if c:` with an indented `1`, a following line `:` with an indented `2` passes `hd parse`. `check` then reports `not-callable: type 'void' is not callable`, a confusing message for a syntax error. The same happens after a `match` suite. The reference parser reports `syntax-error` and `trailing-block-position` at the `:` line. Chapter 01 allows trailing-block colons only when the call is the complete statement.
Recommendation: implementation change: reject a statement that begins with `:`.
