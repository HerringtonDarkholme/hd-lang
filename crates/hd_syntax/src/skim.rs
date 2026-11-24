use hd_base::SliceSort as _;
use hd_base::{Hash128, hash128};

use crate::{CommentKind, TokenBuf, TokenKind, lex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderKind {
    Function,
    Struct,
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
    /// The hash of what a template's check reads of its module
    /// ([`template_text_hash`]).
    pub template_text_hash: Hash128,
    /// The hash of the code lines' contents (`code_lines`): the source
    /// without plain comments, blank lines and trailing whitespace, so a
    /// comment edit keeps it.
    pub token_hash: Hash128,
    /// The start of each code line, in order.
    pub code_lines: Vec<u32>,
    pub bodies: Vec<BodyRange>,
    /// The byte range of each `use` line outside the `tests:` blocks, in
    /// order (`use_ranges`).
    pub uses: Vec<(u32, u32)>,
    /// The byte range of each `use` line inside a `tests:` block, in order.
    pub test_uses: Vec<(u32, u32)>,
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
    let logical = logical_lines(&lexed.tokens);
    let bodies = body_ranges(text, &lexed.tokens, &logical);
    let (uses, test_uses) = split_test_uses(use_ranges(text, &lexed.tokens), &bodies);
    let api_text_hash = api_hash(text, &lexed.tokens, &bodies, &logical);
    let template_text_hash = template_text_hash(text, &lexed.tokens, &bodies, &logical);
    let lines = code_lines(&lexed.tokens);
    let mut content = Vec::with_capacity(source.len());
    for &(start, end) in &lines {
        content.extend_from_slice(&source[start as usize..end as usize]);
        // Never a byte of UTF-8 text.
        content.push(0xff);
    }
    // A cached diagnostic's fix-it holds byte offsets and line-ending text
    // (a missing import's `use` line takes the file's ending), so every
    // hash that keys diagnostics covers the line-ending form too. An LF
    // file's hashes are as before.
    let endings = crlf_lines(source, &lexed.tokens);
    let with_endings = |hash: Hash128| {
        if endings.is_empty() {
            return hash;
        }
        let mut bytes = hash.0.to_le_bytes().to_vec();
        bytes.extend_from_slice(&endings);
        hash128(&bytes)
    };
    if !endings.is_empty() {
        // Never a byte of UTF-8 text.
        content.push(0xfe);
        content.extend_from_slice(&endings);
    }
    HeaderSkeleton {
        source_hash: hash128(source),
        api_text_hash: with_endings(api_text_hash),
        template_text_hash: with_endings(template_text_hash),
        token_hash: hash128(&content),
        code_lines: lines.into_iter().map(|(start, _)| start).collect(),
        bodies,
        uses,
        test_uses,
        broken: !lexed.diagnostics.is_empty(),
    }
}

/// Reference body extraction over the materialized token and line columns.
#[must_use]
pub fn skeleton_from_layout(source: &str, tokens: &TokenBuf) -> Vec<BodyRange> {
    body_ranges(source, tokens, &logical_lines(tokens))
}

/// The logical lines (`lex.line.logical`): each line whose first token is
/// outside brackets, as (physical line, first token, last token), the
/// lines inside brackets that it continues included.
fn logical_lines(tokens: &TokenBuf) -> Vec<(usize, usize, usize)> {
    let mut out: Vec<(usize, usize, usize)> = Vec::new();
    let mut depth = 0_u32;
    let mut line = 0_usize;
    for index in 0..tokens.len() {
        let token = hd_base::TokenIdx::from_raw(u32::try_from(index).unwrap_or(u32::MAX - 1));
        if depth == 0 && (index == 0 || tokens.is_line_first(token)) {
            let start = tokens.start[index];
            while tokens.line_start.get(line + 1).is_some_and(|&s| s <= start) {
                line += 1;
            }
            out.push((line, index, index));
        } else if let Some(last) = out.last_mut() {
            last.2 = index;
        }
        let kind = tokens.kind[index];
        if kind.opens() {
            depth += 1;
        } else if kind.closes() {
            depth = depth.saturating_sub(1);
        }
    }
    out
}

fn body_ranges(
    source: &str,
    tokens: &TokenBuf,
    logical: &[(usize, usize, usize)],
) -> Vec<BodyRange> {
    let mut ranges = Vec::new();
    for (n, &(line, first_idx, last_idx)) in logical.iter().enumerate() {
        // The suite's colon: the last one outside the line's brackets.
        let mut depth = 0_u32;
        let mut colon = None;
        for index in first_idx..=last_idx {
            let kind = tokens.kind[index];
            if kind.opens() {
                depth += 1;
            } else if kind.closes() {
                depth = depth.saturating_sub(1);
            } else if kind == TokenKind::Colon && depth == 0 {
                colon = Some(index);
            }
        }
        let Some(colon) = colon else { continue };
        if !can_open_suite(&tokens.kind[first_idx..=colon]) {
            continue;
        }
        let indent = tokens.line_indent[line];
        let header_start = tokens.line_start[line];
        let kind = header_kind(head_kind(&tokens.kind[first_idx..=colon]));
        if colon < last_idx {
            ranges.push(BodyRange {
                header_start,
                body_start: tokens.start[colon + 1],
                body_end: tokens.end[last_idx],
                header_indent: indent,
                kind,
            });
            continue;
        }
        let Some(&(body_line, _, _)) = logical.get(n + 1) else {
            ranges.push(BodyRange {
                header_start,
                body_start: tokens.end[colon],
                body_end: tokens.end[colon],
                header_indent: indent,
                kind,
            });
            continue;
        };
        if tokens.line_indent[body_line] <= indent {
            continue;
        }
        let body_start = tokens.line_start[body_line];
        let body_end = logical[n + 1..]
            .iter()
            .find(|&&(l, _, _)| tokens.line_indent[l] <= indent)
            .map_or(
                u32::try_from(source.len()).unwrap_or(u32::MAX - 1),
                |&(l, _, _)| tokens.line_start[l],
            );
        ranges.push(BodyRange {
            header_start,
            body_start,
            body_end,
            header_indent: indent,
            kind,
        });
    }
    ranges
}

fn can_open_suite(kinds: &[TokenKind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            kind,
            TokenKind::KwFn
                | TokenKind::KwStruct
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
        TokenKind::KwStruct => HeaderKind::Struct,
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

/// The byte range of each `use` line, from its `use` to its last token: a
/// line whose first token, past a leading `pub`, is the word `use` followed
/// by a path's first segment. One pass over the lines.
fn use_ranges(source: &str, tokens: &TokenBuf) -> Vec<(u32, u32)> {
    let mut result = Vec::new();
    let line_end = tokens.line_token_ends();
    for (line, &next) in line_end.iter().enumerate() {
        let Some(first) = tokens.line_tok[line].get() else {
            continue;
        };
        let mut at = first.idx();
        if tokens.kind[at] == TokenKind::KwPub {
            at += 1;
        }
        let is_use = at + 1 < next
            && tokens.kind[at] == TokenKind::Ident
            && &source[tokens.start[at] as usize..tokens.end[at] as usize] == "use"
            && matches!(
                tokens.kind[at + 1],
                TokenKind::Ident | TokenKind::KwSelfValue
            );
        if is_use {
            result.push((tokens.start[at], tokens.end[next - 1]));
        }
    }
    result
}

/// A byte range of the text.
type Span = (u32, u32);

/// The `use` lines split into those outside every `tests:` block and
/// those inside one. Both the lines and the blocks are in text order, so
/// one merge places them all: the blocks that start at or before a line
/// are passed once, and the line is inside one exactly when the furthest
/// end among them lies past it.
fn split_test_uses(uses: Vec<Span>, bodies: &[BodyRange]) -> (Vec<Span>, Vec<Span>) {
    let mut blocks = bodies
        .iter()
        .filter(|b| b.kind == HeaderKind::Tests)
        .peekable();
    let mut reach = 0;
    let (mut outside, mut inside) = (Vec::new(), Vec::new());
    for (lo, hi) in uses {
        crate::work::skim_step();
        while let Some(block) = blocks.next_if(|b| b.body_start <= lo) {
            crate::work::skim_step();
            reach = reach.max(block.body_end);
        }
        if lo < reach {
            inside.push((lo, hi));
        } else {
            outside.push((lo, hi));
        }
    }
    (outside, inside)
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

/// The indices of the lines that end in `\r\n`, as little-endian `u32`s:
/// the file's line-ending form. One look at the two bytes before each line
/// start the lexer recorded; no pass over the text.
fn crlf_lines(source: &[u8], tokens: &TokenBuf) -> Vec<u8> {
    let mut out = Vec::new();
    for (line, &next) in tokens.line_start.iter().enumerate().skip(1) {
        let next = next as usize;
        if next >= 2 && source.get(next - 2) == Some(&b'\r') && source.get(next - 1) == Some(&b'\n')
        {
            let ended = u32::try_from(line - 1).unwrap_or(u32::MAX);
            out.extend_from_slice(&ended.to_le_bytes());
        }
    }
    out
}

/// The starts of the lines that begin a `data`, `enum`, `trait`, `impl`
/// or `type` declaration, in order.
fn decl_line_starts(tokens: &TokenBuf) -> Vec<u32> {
    let mut out = Vec::new();
    for first in tokens.line_tok.iter().filter_map(|t| t.get()) {
        let mut i = first.idx();
        if tokens.kind.get(i) == Some(&TokenKind::KwPub) {
            i += 1;
        }
        if matches!(
            tokens.kind.get(i),
            Some(
                TokenKind::KwStruct
                    | TokenKind::KwEnum
                    | TokenKind::KwTrait
                    | TokenKind::KwImpl
                    | TokenKind::KwType
            )
        ) {
            out.push(tokens.start[first.idx()]);
        }
    }
    out
}

/// The api text hash (`crates/hd_syntax/DESIGN.md` §4.5): every token,
/// with a marker per logical line and per indentation change, except
/// function bodies, `tests:` blocks and top-level statements. A body or
/// statement that declares a local `data`, `enum`, `trait`, `impl` or
/// `type` stays.
fn api_hash(
    source: &str,
    tokens: &TokenBuf,
    bodies: &[BodyRange],
    logical: &[(usize, usize, usize)],
) -> Hash128 {
    // A function body that declares a local type, trait or impl stays in
    // the API text: those lift to module items, and a local impl is in
    // the folder interface (`crates/hd_tir/DESIGN.md` "Local items lift to
    // hidden module items").
    let decls = decl_line_starts(tokens);
    let declares = |start: u32, end: u32| declares_item(&decls, start, end);
    let mut hidden: Vec<(u32, u32)> = bodies
        .iter()
        .filter(|body| match body.kind {
            HeaderKind::Function => !declares(body.body_start, body.body_end),
            HeaderKind::Tests => true,
            _ => false,
        })
        .map(|body| (body.body_start, body.body_end))
        .collect();
    hidden.extend(
        top_level_statements(source, tokens, logical)
            .into_iter()
            .filter(|&(start, end)| !declares(start, end)),
    );
    text_hash(source, tokens, logical, hidden)
}

/// The hash of what checking a template of this module can read, at an
/// opt-in in another module or for a tuple template's member steps: every
/// token, as the api text hash marks them, except `tests:` blocks and the
/// function bodies no caller's view depends on. A body stays when its
/// function omits its result type, or is private and omits its row,
/// since a caller then sees types inferred from the body
/// (`module.package.template-private.result`,
/// `module.package.template-private.row`). Every method body of an
/// implementation written with `by`, a template's among them, stays too,
/// and so do top-level statements. So a body edit of a function with a
/// written result, public or with a written row, outside such an
/// implementation keeps the hash (`crates/hd_resolve/DESIGN.md` §4.10
/// "Hidden items and their closure").
fn template_text_hash(
    source: &str,
    tokens: &TokenBuf,
    bodies: &[BodyRange],
    logical: &[(usize, usize, usize)],
) -> Hash128 {
    let decls = decl_line_starts(tokens);
    // The bodies are in header order, so one merge places each function
    // in the `by` implementations: those whose header starts at or before
    // its header are passed once, and it lies inside one exactly when the
    // furthest end among them reaches its end.
    let mut by_impls = bodies
        .iter()
        .filter(|b| b.kind == HeaderKind::Impl && header_has_by(source, tokens, b))
        .peekable();
    let mut reach = 0;
    let mut in_by_impl = |body: &BodyRange| {
        crate::work::skim_step();
        while let Some(by) = by_impls.next_if(|by| by.header_start <= body.header_start) {
            crate::work::skim_step();
            reach = reach.max(by.body_end);
        }
        body.body_end <= reach
    };
    let hidden = bodies
        .iter()
        .filter(|body| match body.kind {
            HeaderKind::Function => {
                !declares_item(&decls, body.body_start, body.body_end)
                    && !in_by_impl(body)
                    && signature_written(tokens, body)
            }
            HeaderKind::Tests => true,
            _ => false,
        })
        .map(|body| (body.body_start, body.body_end))
        .collect();
    text_hash(source, tokens, logical, hidden)
}

/// Whether the header of `body` holds the word `by`.
fn header_has_by(source: &str, tokens: &TokenBuf, body: &BodyRange) -> bool {
    let first = tokens.start.partition_point(|s| *s < body.header_start);
    let last = tokens.start.partition_point(|s| *s < body.body_start);
    (first..last).any(|i| {
        tokens.kind[i] == TokenKind::Ident
            && tokens.text(
                hd_base::TokenIdx::from_raw(u32::try_from(i).unwrap_or(u32::MAX - 1)),
                source,
            ) == "by"
    })
}

/// Whether the byte range from `start` to `end` holds a line that
/// declares a local type, trait or impl.
fn declares_item(decls: &[u32], start: u32, end: u32) -> bool {
    let i = decls.partition_point(|s| *s < start);
    decls.get(i).is_some_and(|s| *s < end)
}

/// Whether the function header of `body` writes everything a caller sees:
/// a result type (`->` outside brackets), and a row (`$` outside
/// brackets) unless it is `pub`, whose omitted row is the empty row.
fn signature_written(tokens: &TokenBuf, body: &BodyRange) -> bool {
    let first = tokens.start.partition_point(|s| *s < body.header_start);
    let last = tokens.start.partition_point(|s| *s < body.body_start);
    let (mut depth, mut result, mut row) = (0_i32, false, false);
    for kind in &tokens.kind[first..last] {
        if kind.opens() {
            depth += 1;
        } else if kind.closes() {
            depth -= 1;
        } else if depth == 0 && *kind == TokenKind::Arrow {
            result = true;
        } else if depth == 0 && *kind == TokenKind::Dollar {
            row = true;
        }
    }
    let public = tokens.kind.get(first) == Some(&TokenKind::KwPub);
    result && (row || public)
}

/// The hash of the source's tokens, with a marker per logical line and
/// per indentation change, without the tokens in each `hidden` byte
/// range.
fn text_hash(
    source: &str,
    tokens: &TokenBuf,
    logical: &[(usize, usize, usize)],
    mut hidden: Vec<(u32, u32)>,
) -> Hash128 {
    let mut bytes = Vec::new();
    let mut indents = vec![0_u16];
    hidden.sort_stable();
    let mut hidden = hidden.into_iter().peekable();
    let mut lines = logical.iter().peekable();
    for index in 0..tokens.len() {
        let start = tokens.start[index];
        let kind = tokens.kind[index];
        // A logical line: a line break inside brackets is not one.
        let line = lines.next_if(|l| l.1 == index).map(|l| l.0);
        while hidden.peek().is_some_and(|&(_, end)| start >= end) {
            hidden.next();
        }
        if hidden
            .peek()
            .is_some_and(|&(body_start, end)| start >= body_start && start < end)
        {
            continue;
        }
        let token = hd_base::TokenIdx::from_raw(u32::try_from(index).unwrap_or(u32::MAX - 1));
        if let Some(line) = line {
            bytes.push(0xf0);
            let indent = tokens.line_indent[line];
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
        bytes.push(kind as u8);
        bytes.extend_from_slice(tokens.text(token, source).as_bytes());
    }
    hash128(&bytes)
}

/// The byte ranges of the top-level statements: each logical line at
/// column 0 that starts no declaration, `use` line, decorator or `tests:`
/// block, through the lines it indents.
fn top_level_statements(
    source: &str,
    tokens: &TokenBuf,
    logical: &[(usize, usize, usize)],
) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut open: Option<u32> = None;
    for &(line, first, _) in logical {
        if tokens.line_indent[line] != 0 {
            continue;
        }
        let start = tokens.start[first];
        if let Some(from) = open.take() {
            out.push((from, start));
        }
        let kind = tokens.kind[first];
        let token = hd_base::TokenIdx::from_raw(u32::try_from(first).unwrap_or(u32::MAX - 1));
        let head = matches!(
            kind,
            TokenKind::KwFn
                | TokenKind::KwStruct
                | TokenKind::KwEnum
                | TokenKind::KwTrait
                | TokenKind::KwImpl
                | TokenKind::KwType
                | TokenKind::KwPub
                | TokenKind::KwTests
                | TokenKind::At
        ) || (kind == TokenKind::Ident && tokens.text(token, source) == "use");
        if !head {
            open = Some(start);
        }
    }
    if let Some(from) = open {
        out.push((from, u32::try_from(source.len()).unwrap_or(u32::MAX - 1)));
    }
    out
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

#[cfg(test)]
mod tests {
    use crate::{lex, skeleton_from_layout, skim};

    #[test]
    fn skim_and_layout_reference_agree() {
        let source = include_str!("skim/two_functions.hd");
        let tokens = lex(source.as_bytes()).tokens;
        assert_eq!(
            skim(source.as_bytes()).bodies,
            skeleton_from_layout(source, &tokens)
        );
    }

    /// The hashes that key cached diagnostics see the line-ending form
    /// (#1862): a fix-it holds the file's ending and byte offsets.
    #[test]
    fn keyed_hashes_follow_the_line_endings() {
        let lf = include_str!("skim/use_and_function.hd");
        let crlf = lf.replace('\n', "\r\n");
        let one = lf.replacen('\n', "\r\n", 1);
        let (a, b, c) = (
            skim(lf.as_bytes()),
            skim(crlf.as_bytes()),
            skim(one.as_bytes()),
        );
        for (x, y) in [(&a, &b), (&a, &c), (&b, &c)] {
            assert_ne!(x.token_hash, y.token_hash);
            assert_ne!(x.api_text_hash, y.api_text_hash);
            assert_ne!(x.template_text_hash, y.template_text_hash);
        }
        // A plain comment edit still keeps them, in a CRLF file too.
        let edited = skim(crlf.replace("# x", "# y").as_bytes());
        assert_eq!(edited.token_hash, b.token_hash);
        assert_eq!(edited.api_text_hash, b.api_text_hash);
    }

    #[test]
    fn token_hash_ignores_plain_comments_only() {
        let base = include_str!("skim/body.hd");
        let hash = |s: &str| skim(s.as_bytes()).token_hash;
        // The same body with a leading, a trailing and an own-line comment
        // and a blank line.
        let commented = include_str!("skim/body_commented.hd");
        assert_eq!(hash(base), hash(commented));
        // Its code lines are the lines of `base`, by their starts.
        let starts: Vec<u32> = base
            .lines()
            .map(|line| {
                let at = commented.find(line).expect("a line of the body");
                u32::try_from(at).expect("a small file")
            })
            .collect();
        assert_eq!(skim(commented.as_bytes()).code_lines, starts);
        for changed in [
            base.replace("x := 1", "x := 2"),
            base.replace("x := 1", "x :=  1"),
            base.replace("    return", "\treturn"),
            format!("## doc\n{base}"),
        ] {
            assert_ne!(hash(base), hash(&changed), "{changed:?}");
        }
    }

    #[test]
    fn api_hash_leaves_out_top_level_statements_and_bracketed_breaks() {
        let hash = |s: &str| skim(s.as_bytes()).api_text_hash;
        let base = include_str!("skim/api.hd");
        for same in [
            // A top-level statement edited, or removed and one added.
            base.replace("limit := 10", "limit := 11")
                .replace("limit > 3", "limit > 4")
                .replace("\"big\"", "\"bigger\""),
            include_str!("skim/api_statement_after.hd").to_owned(),
            // A line break inside a header's brackets.
            base.replace("(a: i32, b: i32)", "(\n    a: i32,\n    b: i32\n)"),
        ] {
            assert_ne!(base, same);
            assert_eq!(hash(base), hash(&same), "{same:?}");
        }
        for changed in [
            base.replace("std.io", "std.fs"),
            base.replace("b: i32", "b: i64"),
        ] {
            assert_ne!(hash(base), hash(&changed), "{changed:?}");
        }
    }

    #[test]
    fn api_hash_includes_kept_layout() {
        let first = include_str!("skim/by_structure.hd");
        // The last line indented into the `if` suite.
        let second = first.replace("\n    other", "\n        other");
        assert_ne!(first, second);
        assert_ne!(
            skim(first.as_bytes()).api_text_hash,
            skim(second.as_bytes()).api_text_hash
        );
    }

    #[test]
    fn template_hash_keeps_bodies_whose_signature_is_inferred() {
        let base = include_str!("skim/template_bodies.hd");
        let hash = |s: &str| skim(s.as_bytes()).template_text_hash;
        let api = |s: &str| skim(s.as_bytes()).api_text_hash;
        for (from, to, kept) in [
            ("    1\n", "    10\n", true),
            ("    3\n", "    30\n", true),
            ("    2\n", "    20\n", false),
            ("    4\n", "    40\n", false),
            ("\"5\"", "\"50\"", false),
            ("        6\n", "        60\n", false),
        ] {
            let edited = base.replace(from, to);
            assert_ne!(base, edited, "{from:?}");
            assert_eq!(api(base), api(&edited), "{from:?}");
            assert_eq!(hash(base) == hash(&edited), kept, "{from:?}");
        }
    }

    /// Only `use` lines are uses (#1881), placed by the `tests:` blocks:
    /// each form of line that starts with a name, and a `use` inside a
    /// multiline string, in LF and CRLF text.
    #[test]
    fn uses_are_the_use_lines_placed_by_the_tests_blocks() {
        let lf = include_str!("skim/uses.hd");
        for text in [lf.to_owned(), lf.replace('\n', "\r\n")] {
            let sk = skim(text.as_bytes());
            let lines = |ranges: &[(u32, u32)]| -> Vec<String> {
                ranges
                    .iter()
                    .map(|&(lo, hi)| text[lo as usize..hi as usize].to_owned())
                    .collect()
            };
            assert_eq!(
                lines(&sk.uses),
                [
                    "use std.a",
                    "use std.b.{c}",
                    "use self.d as e",
                    "use std.after"
                ]
            );
            assert_eq!(lines(&sk.test_uses), ["use pkg.t", "use std.u.{v}"]);
        }
    }

    /// A function body in a `by` implementation stays in the template
    /// hash however the implementations nest and order: before, inside,
    /// between and after them.
    #[test]
    fn template_hash_keeps_each_body_of_a_by_implementation() {
        let base = include_str!("skim/by_bodies.hd");
        let hash = |s: &str| skim(s.as_bytes()).template_text_hash;
        for (body, kept) in [
            ("1", true),
            ("2", false),
            ("3", true),
            ("4", true),
            ("5", false),
            ("6", false),
            ("7", true),
        ] {
            let edited = base.replace(&format!("{body}\n"), &format!("{body}0\n"));
            assert_ne!(base, edited, "body {body}");
            assert_eq!(hash(base) == hash(&edited), kept, "body {body}");
        }
    }

    /// The skim's placing passes are linear (#1881): n functions, each
    /// with a `use` line, a `by` implementation and a `tests:` block.
    #[test]
    fn skim_placing_work_is_linear() {
        use std::fmt::Write as _;
        let source = |n: usize| {
            let mut out = String::new();
            for i in 0..n {
                let _ = write!(
                    out,
                    "use std.m{i}\nfn g{i}() -> i32 $ Console:\n    p := P {{ x: {i} }}\n    p.x\nimpl Show for T{i} by Structure:\n    fn show(self) -> string $ Console:\n        \"{i}\"\ntests:\n    use pkg.t{i}\n"
                );
            }
            out
        };
        let work = |n: usize| {
            let (sk, steps) = crate::work::count_skim_steps(|| skim(source(n).as_bytes()));
            assert_eq!((sk.uses.len(), sk.test_uses.len()), (n, n));
            steps
        };
        let (small, large) = (work(2_000), work(20_000));
        assert!(small > 0 && large <= 10 * small, "{small} then {large}");
    }
}
