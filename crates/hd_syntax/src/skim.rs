use hd_base::SliceSort as _;
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
    let logical = logical_lines(&lexed.tokens);
    let bodies = body_ranges(text, &lexed.tokens, &logical);
    let uses = use_ranges(&lexed.tokens);
    let api_text_hash = api_hash(text, &lexed.tokens, &bodies, &logical);
    let template_text_hash = template_text_hash(text, &lexed.tokens, &bodies, &logical);
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
        template_text_hash,
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
        TokenKind::KwStruct => HeaderKind::Data,
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
    let by_impls: Vec<(u32, u32)> = bodies
        .iter()
        .filter(|b| b.kind == HeaderKind::Impl && header_has_by(source, tokens, b))
        .map(|b| (b.header_start, b.body_end))
        .collect();
    let in_by_impl = |body: &BodyRange| {
        by_impls
            .iter()
            .any(|&(lo, hi)| lo <= body.header_start && body.body_end <= hi)
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
    fn api_hash_leaves_out_top_level_statements_and_bracketed_breaks() {
        let hash = |s: &str| skim(s.as_bytes()).api_text_hash;
        let base = "use std.io\n\nlimit := 10\nif limit > 3:\n    println(\"big\")\n\nfn f(a: i32, b: i32) -> i32: a + b\n";
        for same in [
            // A top-level statement edited, added or removed.
            "use std.io\n\nlimit := 11\nif limit > 4:\n    println(\"bigger\")\n\nfn f(a: i32, b: i32) -> i32: a + b\n",
            "use std.io\n\nfn f(a: i32, b: i32) -> i32: a + b\nprintln(\"after\")\n",
            // A line break inside a header's brackets.
            "use std.io\n\nlimit := 10\nif limit > 3:\n    println(\"big\")\n\nfn f(\n    a: i32,\n    b: i32\n) -> i32: a + b\n",
        ] {
            assert_eq!(hash(base), hash(same), "{same:?}");
        }
        for changed in [
            "use std.fs\n\nlimit := 10\nif limit > 3:\n    println(\"big\")\n\nfn f(a: i32, b: i32) -> i32: a + b\n",
            "use std.io\n\nlimit := 10\nif limit > 3:\n    println(\"big\")\n\nfn f(a: i32, b: i64) -> i32: a + b\n",
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

    #[test]
    fn template_hash_keeps_bodies_whose_signature_is_inferred() {
        let base = "pub fn f() -> i32:\n    1\nfn g() -> i32:\n    2\nfn h(x: fn() -> i32) -> i32 $ Console:\n    3\nfn k():\n    4\nimpl[T] Show for T by Structure:\n    fn show(self) -> string:\n        \"5\"\n    fn say(self) -> void $ Console:\n        6\n";
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
            assert_eq!(api(base), api(&edited), "{from:?}");
            assert_eq!(hash(base) == hash(&edited), kept, "{from:?}");
        }
    }
}
