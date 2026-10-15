//! Text a test splices into hd source. Included by path into the library's
//! unit tests and into integration tests, so there is one definition.

#![allow(dead_code)]

/// `text` as an hd string literal, quotes included. A Windows path holds
/// backslashes, which hd reads as escapes, so `\`, `"` and `$` (which
/// starts an interpolation) are escaped, as are the control characters a
/// literal cannot hold.
pub fn hd_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A path as an hd string literal.
pub fn hd_path(path: &std::path::Path) -> String {
    hd_string(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_path_keeps_its_backslashes() {
        assert_eq!(
            hd_string(r"C:\Users\RUNNER~1\AppData\Local\Temp\hd-io-fs-1\data.txt"),
            r#""C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\hd-io-fs-1\\data.txt""#
        );
    }

    #[test]
    fn quotes_dollars_and_controls_are_escaped() {
        assert_eq!(hd_string("a\"b$c\nd\te\rf"), r#""a\"b\$c\nd\te\rf""#);
        assert_eq!(hd_string("plain/path.txt"), "\"plain/path.txt\"");
    }

    #[test]
    fn escape_lookalikes_in_a_path_stay_literal() {
        assert_eq!(hd_string(r"C:\new\$x\u{41}"), r#""C:\\new\\\$x\\u{41}""#);
    }
}
