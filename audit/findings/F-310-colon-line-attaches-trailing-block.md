# F-310: A line starting with `:` is parsed as a trailing block on the previous statement
Severity: minor
Area: correctness
Evidence: spec/conformance/parse/invalid/colon-line-after-if-suite.hd, listed in test/portable/KNOWN_FAILURES.tsv (`hd parse` prints `ok`; the fixture expects `trailing-block-position` on the `:` line)
Effect: after `if c:` with an indented `1`, a following line `:` with an indented `2` passes `hd parse`. `check` then reports `not-callable: type 'void' is not callable`, a confusing message for a syntax error. The same happens after a `match` suite. Chapter 01 allows trailing-block colons only when the call is the complete statement.
Recommendation: implementation change: reject a statement that begins with `:`.
