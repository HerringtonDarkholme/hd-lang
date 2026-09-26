# F-706: A backslash before a quote in a raw string is a lexer error
Severity: minor
Area: correctness
Evidence: audit/evidence/w9/verify-findings.log (F-706 block: `hd parse spec/conformance/parse/valid/raw-string-escaped-quote.hd` reports `unexpected-character: unexpected character '\'` at 2:21 and `unterminated-string` at 3:33, exit 1)
Effect: `r"say \"hi\""` and `r"""a \""" still inside"""` fail to lex. Spec 01 (String And Character Literals) says a backslash may keep the next quote from ending a raw literal, and the backslash stays in the string. Users cannot put a quote in a raw string.
Recommendation: implementation change: in raw literals, let a backslash consume the following quote as content, keeping both characters.
