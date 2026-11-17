//! Literal token decoding shared by header lowering and body checking
//! (`lex.int.forms`, `lex.suffix.form`, `lex.escape.*`): string pieces and
//! their escapes, and the parts of a number token.

use hd_syntax::TokenKind;

/// Decodes a string piece's text: escapes, and `$name` references.
pub enum Piece {
    Text(String),
    Name(String),
}

pub fn decode_piece(raw: &str, out: &mut Vec<Piece>) {
    decode_piece_spanned(raw, |piece, _| out.push(piece));
}

/// Decode with each piece's range in the original UTF-8 source, including
/// `$` for a name. CRLF normalization and escapes do not move source spans.
pub fn decode_piece_spanned(raw: &str, mut emit: impl FnMut(Piece, std::ops::Range<usize>)) {
    let mut text = String::new();
    let mut text_start = 0;
    let mut chars = raw
        .char_indices()
        .filter(|&(i, c)| c != '\r' || raw.as_bytes().get(i + 1) != Some(&b'\n'))
        .peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => match chars.next().map(|(_, c)| c) {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('r') => text.push('\r'),
                Some('0') => text.push('\0'),
                Some('u') => {
                    let mut hex = String::new();
                    if chars.peek().map(|&(_, c)| c) == Some('{') {
                        chars.next();
                        while let Some(&(_, h)) = chars.peek() {
                            chars.next();
                            if h == '}' {
                                break;
                            }
                            hex.push(h);
                        }
                    }
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        text.push(ch);
                    }
                }
                Some(o) => text.push(o),
                None => {}
            },
            '$' if chars
                .peek()
                .is_some_and(|&(_, n)| n.is_alphabetic() || n == '_') =>
            {
                let mut name = String::new();
                let mut end = at + 1;
                while let Some(&(i, n)) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' {
                        name.push(n);
                        end = i + n.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !text.is_empty() {
                    emit(Piece::Text(std::mem::take(&mut text)), text_start..at);
                }
                emit(Piece::Name(name), at..end);
                text_start = end;
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        emit(Piece::Text(text), text_start..raw.len());
    }
}

/// The text with each CRLF line ending as one line feed
/// (`lex.multiline.line-feed`). A bare carriage return is a lexical error
/// and never reaches here.
#[must_use]
pub fn normalize_line_endings(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.contains("\r\n") {
        std::borrow::Cow::Owned(raw.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(raw)
    }
}

/// The text of a string token without its delimiters and interpolation
/// openers and closers.
#[must_use]
pub fn strip_piece(t: &str, kind: TokenKind) -> &str {
    strip_piece_offset(t, kind).0
}

/// The undecorated text and its starting byte within the token.
#[must_use]
pub fn strip_piece_offset(token: &str, kind: TokenKind) -> (&str, usize) {
    let t = match kind {
        TokenKind::StrHead | TokenKind::String => {
            let t = token.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
            t.strip_prefix("\"\"\"")
                .or_else(|| t.strip_prefix('"'))
                .unwrap_or(t)
        }
        _ => token.strip_prefix('}').unwrap_or(token),
    };
    let offset = token.len() - t.len();
    let raw = match kind {
        TokenKind::StrHead | TokenKind::StrMid => t.strip_suffix("${").unwrap_or(t),
        _ => t
            .strip_suffix("\"\"\"")
            .or_else(|| t.strip_suffix('"'))
            .unwrap_or(t),
    };
    (raw, offset)
}

/// The radix and digits of a prefixed integer literal (`lex.int.forms`):
/// the prefix letter is case-insensitive.
#[must_use]
pub fn radix_digits(text: &str) -> Option<(u32, &str)> {
    let b = text.as_bytes();
    if b.len() < 2 || b[0] != b'0' {
        return None;
    }
    let radix = match b[1] {
        b'x' | b'X' => 16,
        b'b' | b'B' => 2,
        b'o' | b'O' => 8,
        _ => return None,
    };
    Some((radix, &text[2..]))
}

/// Plain decimal digits, underscores allowed: true for an undecorated
/// base-10 integer, which may be larger than u64 can hold
/// (`types.literal.int-range`).
#[must_use]
pub fn decimal_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'_')
}

/// Splits a number token into its digits and its literal suffix
/// (`lex.suffix.form`, `lex.suffix.exponent`). Radix literals have none.
#[must_use]
pub fn split_suffix(text: &str) -> (&str, &str) {
    let b = text.as_bytes();
    if radix_digits(text).is_some() {
        return (text, "");
    }
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_' || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
                j += 1;
            }
            i = j;
        }
    }
    text.split_at(i)
}

#[cfg(test)]
mod span_tests {
    use super::*;

    fn decoded(raw: &str) -> Vec<(&str, String, String)> {
        let mut out = Vec::new();
        decode_piece_spanned(raw, |piece, span| {
            let (kind, text) = match piece {
                Piece::Text(text) => ("text", text),
                Piece::Name(name) => ("name", name),
            };
            out.push((kind, text, raw[span].to_owned()));
        });
        out
    }

    #[test]
    fn names_keep_original_utf8_spans_through_escapes_and_crlf() {
        let raw = "é $café\r\n\\$escaped \\u{24}fake $next";
        assert_eq!(
            decoded(raw),
            vec![
                ("text", "é ".into(), "é ".into()),
                ("name", "café".into(), "$café".into()),
                (
                    "text",
                    "\n$escaped $fake ".into(),
                    "\r\n\\$escaped \\u{24}fake ".into()
                ),
                ("name", "next".into(), "$next".into()),
            ]
        );
    }

    #[test]
    fn normalize_source_crlf_before_decoding_but_preserve_escaped_cr() {
        assert_eq!(
            decoded("\\\r\n$name\r\n"),
            vec![
                ("text", "\n".into(), "\\\r\n".into()),
                ("name", "name".into(), "$name".into()),
                ("text", "\n".into(), "\r\n".into()),
            ]
        );
        assert_eq!(decoded("\\u{d}\n")[0].1, "\r\n");
    }

    #[test]
    fn stripping_delimiters_reports_the_original_token_offset() {
        for (token, kind, text, offset) in [
            ("fmt\"\"\"é $x\"\"\"", TokenKind::String, "é $x", 6),
            ("fmt\"\"\"a${", TokenKind::StrHead, "a", 6),
            ("}b${", TokenKind::StrMid, "b", 1),
            ("}c\"\"\"", TokenKind::StrTail, "c", 1),
        ] {
            assert_eq!(strip_piece_offset(token, kind), (text, offset));
        }
    }
}
