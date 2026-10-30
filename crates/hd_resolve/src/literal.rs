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
    let raw = &normalize_line_endings(raw);
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('r') => text.push('\r'),
                Some('0') => text.push('\0'),
                Some('u') => {
                    let mut hex = String::new();
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        while let Some(&h) = chars.peek() {
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
            '$' if chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '_') => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_alphanumeric() || n == '_' {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                out.push(Piece::Name(name));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
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
    let t = match kind {
        TokenKind::StrHead | TokenKind::String => {
            let t = t.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
            t.strip_prefix("\"\"\"")
                .or_else(|| t.strip_prefix('"'))
                .unwrap_or(t)
        }
        _ => t.strip_prefix('}').unwrap_or(t),
    };
    match kind {
        TokenKind::StrHead | TokenKind::StrMid => t.strip_suffix("${").unwrap_or(t),
        _ => t
            .strip_suffix("\"\"\"")
            .or_else(|| t.strip_suffix('"'))
            .unwrap_or(t),
    }
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
