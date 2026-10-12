//! CRLF line endings (lex.line.physical, lex.line.crlf, lex.line.bare-cr):
//! a carriage return followed by a line feed is one line ending, and a
//! bare carriage return is an `invalid-token` error.

use hd_syntax::parse;

/// The 1-based line and column of each diagnostic, with its code.
fn diagnostics(source: &str) -> Vec<(usize, u32, &'static str)> {
    let parsed = parse(source.as_bytes());
    parsed
        .diagnostics
        .iter()
        .map(|d| {
            let line = parsed.tokens.line_of(d.primary.lo);
            let column = d.primary.lo - parsed.tokens.line_start[line];
            (line + 1, column + 1, d.code.as_str())
        })
        .collect()
}

#[test]
fn crlf_source_has_no_diagnostics() {
    let source = "fn f() -> i32:\r\n    x := 1\r\n    x\r\n\r\n# note\r\nfn g():\r\n    pass\r\n";
    assert_eq!(diagnostics(source), []);
}

#[test]
fn crlf_counts_one_line_and_keeps_columns() {
    let lf = parse(b"a := 1\nb := 2\nc := 3\n");
    let crlf = parse(b"a := 1\r\nb := 2\r\nc := 3\r\n");
    assert_eq!(lf.tokens.line_first.len(), crlf.tokens.line_first.len());
    assert_eq!(crlf.tokens.line_start, vec![0, 8, 16, 24]);
    // Each token sits at the same column as in the LF source.
    for (i, &start) in crlf.tokens.start.iter().enumerate() {
        let line = crlf.tokens.line_of(start);
        let lf_line = lf.tokens.line_of(lf.tokens.start[i]);
        assert_eq!(line, lf_line);
        assert_eq!(
            start - crlf.tokens.line_start[line],
            lf.tokens.start[i] - lf.tokens.line_start[lf_line]
        );
    }
}

#[test]
fn a_bad_token_after_crlf_lines_is_on_its_own_line() {
    assert_eq!(
        diagnostics("a := 1\r\nb := 2\r\nc := 'ab'\r\n"),
        [(3, 6, "invalid-token")]
    );
}

#[test]
fn crlf_inside_a_multiline_string_is_one_line_ending() {
    let source = "x := \"\"\"a\r\nb\"\"\"\r\ny := 'ab'\r\n";
    assert_eq!(diagnostics(source), [(3, 6, "invalid-token")]);
}

#[test]
fn a_bare_carriage_return_is_invalid() {
    assert_eq!(diagnostics("a := 1\rb := 2\n"), [(1, 7, "invalid-token")]);
}

#[test]
fn a_comment_ends_at_crlf_without_the_carriage_return() {
    let parsed = parse(b"# note\r\nx := 1\r\n");
    assert_eq!(parsed.diagnostics.len(), 0);
    assert_eq!(parsed.tokens.com_end[0] - parsed.tokens.com_start[0], 6);
}
