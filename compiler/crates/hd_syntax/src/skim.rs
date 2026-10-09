use hd_base::{Hash128, hash128};

use crate::{CommentKind, TokenBuf, TokenKind, lex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Function,
    Data,
    Enum,
    Trait,
    Impl,
    Type,
    Tests,
    Control,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyRange {
    pub header_start: u32,
    pub body_start: u32,
    pub body_end: u32,
    pub header_indent: u16,
    pub kind: HeaderKind,
}

#[derive(Clone, Debug, Default)]
pub struct HeaderSkeleton {
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,
    /// The hash of the code lines' contents (`code_lines`): the source
    /// without plain comments, blank lines and trailing whitespace, so a
    /// comment edit keeps it.
    pub token_hash: Hash128,
    /// The start of each code line, in order.
    pub code_lines: Vec<u32>,
    pub bodies: Vec<BodyRange>,
    pub uses: Vec<(u32, u32)>,
    pub broken: bool,
}

#[must_use]
pub fn skim(source: &[u8]) -> HeaderSkeleton {
    let lexed = lex(source);
    let Ok(text) = core::str::from_utf8(source) else {
        return HeaderSkeleton {
            broken: true,
            ..HeaderSkeleton::default()
        };
    };
    let bodies = body_ranges(text, &lexed.tokens);
    let uses = use_ranges(&lexed.tokens);
    let api_text_hash = api_hash(text, &lexed.tokens, &bodies);
    let lines = code_lines(&lexed.tokens);
    let mut content = Vec::with_capacity(source.len());
    for &(start, end) in &lines {
        content.extend_from_slice(&source[start as usize..end as usize]);
        // Never a byte of UTF-8 text.
        content.push(0xff);
    }
    HeaderSkeleton {
        source_hash: hash128(source),
        api_text_hash,
        token_hash: hash128(&content),
        code_lines: lines.into_iter().map(|(start, _)| start).collect(),
        bodies,
        uses,
        broken: !lexed.diagnostics.is_empty(),
    }
}

/// Reference body extraction over the materialized token and line columns.
#[must_use]
pub fn skeleton_from_layout(source: &str, tokens: &TokenBuf) -> Vec<BodyRange> {
    body_ranges(source, tokens)
}

fn body_ranges(source: &str, tokens: &TokenBuf) -> Vec<BodyRange> {
    let mut ranges = Vec::new();
    let line_end = tokens.line_token_ends();
    for (line, &next_line_token) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        let first_idx = first.idx();
        if first_idx >= next_line_token {
            continue;
        }
        let colon = (first_idx..next_line_token)
            .rev()
            .find(|&index| tokens.kind[index] == TokenKind::Colon);
        let Some(colon) = colon else { continue };
        if !can_open_suite(&tokens.kind[first_idx..=colon]) {
            continue;
        }
        let indent = tokens.line_indent[line];
        let header_start = tokens.line_start[line];
        if colon + 1 < next_line_token {
            let start = tokens.start[colon + 1];
            let end = u32::try_from(line_content_end(
                source.as_bytes(),
                tokens.line_start[line] as usize,
            ))
            .unwrap_or(u32::MAX - 1);
            ranges.push(BodyRange {
                header_start,
                body_start: start,
                body_end: end,
                header_indent: indent,
                kind: header_kind(head_kind(&tokens.kind[first_idx..=colon])),
            });
            continue;
        }
        let Some(body_line) = ((line + 1)..tokens.line_start.len())
            .find(|&candidate| tokens.line_tok[candidate].get().is_some())
        else {
            ranges.push(BodyRange {
                header_start,
                body_start: tokens.end[colon],
                body_end: tokens.end[colon],
                header_indent: indent,
                kind: header_kind(head_kind(&tokens.kind[first_idx..=colon])),
            });
            continue;
        };
        if tokens.line_indent[body_line] <= indent {
            continue;
        }
        let body_start = tokens.line_start[body_line];
        let end_line = ((body_line + 1)..tokens.line_start.len())
            .find(|&candidate| {
                tokens.line_tok[candidate].get().is_some()
                    && tokens.line_indent[candidate] <= indent
            })
            .unwrap_or(tokens.line_start.len());
        let body_end = if end_line == tokens.line_start.len() {
            u32::try_from(source.len()).unwrap_or(u32::MAX - 1)
        } else {
            tokens.line_start[end_line]
        };
        ranges.push(BodyRange {
            header_start,
            body_start,
            body_end,
            header_indent: indent,
            kind: header_kind(head_kind(&tokens.kind[first_idx..=colon])),
        });
    }
    ranges
}

fn can_open_suite(kinds: &[TokenKind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            kind,
            TokenKind::KwFn
                | TokenKind::KwData
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwImpl
                | TokenKind::KwTests
                | TokenKind::KwIf
                | TokenKind::KwFor
                | TokenKind::KwWhile
                | TokenKind::KwMatch
                | TokenKind::KwElse
                | TokenKind::FatArrow
        )
    })
}

fn header_kind(kind: TokenKind) -> HeaderKind {
    match kind {
        TokenKind::KwFn => HeaderKind::Function,
        TokenKind::KwData => HeaderKind::Data,
        TokenKind::KwEnum => HeaderKind::Enum,
        TokenKind::KwTrait => HeaderKind::Trait,
        TokenKind::KwImpl => HeaderKind::Impl,
        TokenKind::KwType => HeaderKind::Type,
        TokenKind::KwTests => HeaderKind::Tests,
        TokenKind::KwIf
        | TokenKind::KwFor
        | TokenKind::KwWhile
        | TokenKind::KwMatch
        | TokenKind::KwElse => HeaderKind::Control,
        _ => HeaderKind::Other,
    }
}

fn use_ranges(tokens: &TokenBuf) -> Vec<(u32, u32)> {
    let mut result = Vec::new();
    let line_end = tokens.line_token_ends();
    for (line, &next) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        let first = first.idx();
        let first = if tokens.kind.get(first) == Some(&TokenKind::KwPub) && next > first + 1 {
            first + 1
        } else {
            first
        };
        let is_use = tokens
            .kind
            .get(first)
            .is_some_and(|kind| *kind == TokenKind::Ident)
            && tokens.start.get(first).is_some();
        if is_use && next > first {
            result.push((tokens.start[first], tokens.end[next - 1]));
        }
    }
    result
}

/// The code lines: each line on which a token or a documentation comment
/// starts, as `(line start, content end)`, where the content ends with the
/// last token or documentation comment that starts on the line (a
/// multi-line string runs on past the line). Plain comments, blank lines
/// and trailing whitespace lie outside every content.
fn code_lines(tokens: &TokenBuf) -> Vec<(u32, u32)> {
    // The token after the last one that starts on each line (or on the
    // string lines it continues into).
    let ends = tokens.line_token_ends();
    let mut docs = (0..tokens.com_kind.len())
        .filter(|&c| tokens.com_kind[c] != CommentKind::Plain)
        .peekable();
    let mut out = Vec::new();
    for (line, &start) in tokens.line_start.iter().enumerate() {
        let next = tokens.line_start.get(line + 1).copied().unwrap_or(u32::MAX);
        let mut end = tokens.line_tok[line]
            .get()
            .map(|first| tokens.end[ends[line].max(first.idx() + 1) - 1]);
        while let Some(&c) = docs.peek()
            && tokens.com_start[c] < next
        {
            docs.next();
            end = Some(end.map_or(tokens.com_end[c], |e| e.max(tokens.com_end[c])));
        }
        if let Some(end) = end {
            out.push((start, end));
        }
    }
    out
}

fn api_hash(source: &str, tokens: &TokenBuf, bodies: &[BodyRange]) -> Hash128 {
    let mut bytes = Vec::new();
    let mut indents = vec![0_u16];
    let mut hidden_bodies = bodies
        .iter()
        .filter(|body| matches!(body.kind, HeaderKind::Function | HeaderKind::Tests))
        .peekable();
    for index in 0..tokens.len() {
        let start = tokens.start[index];
        while hidden_bodies
            .peek()
            .is_some_and(|body| start >= body.body_end)
        {
            hidden_bodies.next();
        }
        if hidden_bodies
            .peek()
            .is_some_and(|body| start >= body.body_start && start < body.body_end)
        {
            continue;
        }
        let token = hd_base::TokenIdx::from_raw(u32::try_from(index).unwrap_or(u32::MAX - 1));
        if tokens.is_line_first(token) {
            bytes.push(0xf0);
            let indent = tokens.line_indent[tokens.line_of(tokens.start[index])];
            if indent > *indents.last().expect("indent stack") {
                indents.push(indent);
                bytes.push(0xf1);
            } else {
                while indents.last().is_some_and(|&active| active > indent) {
                    indents.pop();
                    bytes.push(0xf2);
                }
            }
        }
        bytes.push(tokens.kind[index] as u8);
        bytes.extend_from_slice(tokens.text(token, source).as_bytes());
    }
    hash128(&bytes)
}

/// The declaration keyword of a header line, past a leading `pub`
/// (walking skeleton: without this, `pub fn` bodies stayed in the api hash).
fn head_kind(kinds: &[TokenKind]) -> TokenKind {
    kinds
        .iter()
        .copied()
        .find(|kind| *kind != TokenKind::KwPub)
        .unwrap_or(TokenKind::Error)
}

fn line_content_end(source: &[u8], start: usize) -> usize {
    source[start..]
        .iter()
        .position(|byte| matches!(byte, b'\n' | b'\r'))
        .map_or(source.len(), |offset| start + offset)
}

#[cfg(test)]
mod tests {
    use crate::{lex, skeleton_from_layout, skim};

    #[test]
    fn skim_and_layout_reference_agree() {
        let source = "fn first() -> i32:\n    +1\nfn second() -> i32: +2\n";
        let tokens = lex(source.as_bytes()).tokens;
        assert_eq!(
            skim(source.as_bytes()).bodies,
            skeleton_from_layout(source, &tokens)
        );
    }

    #[test]
    fn token_hash_ignores_plain_comments_only() {
        let base = "fn f() -> i32:\n    x := 1\n    return x\n";
        let hash = |s: &str| skim(s.as_bytes()).token_hash;
        let commented =
            "# lead\n\nfn f() -> i32:  # trailing\n    # own line\n    x := 1\n\n    return x\n";
        assert_eq!(hash(base), hash(commented));
        assert_eq!(skim(commented.as_bytes()).code_lines, vec![8, 50, 62]);
        for changed in [
            "fn f() -> i32:\n    x := 2\n    return x\n",
            "fn f() -> i32:\n    x :=  1\n    return x\n",
            "fn f() -> i32:\n    x := 1\n\treturn x\n",
            "## doc\nfn f() -> i32:\n    x := 1\n    return x\n",
        ] {
            assert_ne!(hash(base), hash(changed), "{changed:?}");
        }
    }

    #[test]
    fn api_hash_includes_kept_layout() {
        let first =
            "impl User by Structure:\n    name = if flag:\n        \"x\"\n    other = \"y\"\n";
        let second =
            "impl User by Structure:\n    name = if flag:\n        \"x\"\n        other = \"y\"\n";
        assert_ne!(
            skim(first.as_bytes()).api_text_hash,
            skim(second.as_bytes()).api_text_hash
        );
    }
}
