//! Documentation comment text (`lex.doc.*`): which `##` lines attach to a
//! declaration or member, and their text without the markers. The parser
//! uses [`attached_above`] to claim lines (`lex.doc.attach`); `hd doc`
//! reads the text through [`doc_text`] and [`module_doc_text`]. Comments
//! are no tokens, so none of this reaches an interface hash.

use core::ops::Range;

use hd_base::TokenIdx;

use crate::{CommentKind, TokenBuf};

/// The indices of the `##` comments directly above the token starting at
/// `start`, one per line, at `column`: each on its own line, with nothing
/// but whitespace between it and the next.
pub(crate) fn attached_above(src: &str, toks: &TokenBuf, start: u32, column: u16) -> Range<usize> {
    let end = toks.com_end.partition_point(|&e| e <= start);
    let mut index = end;
    let mut next = start;
    while index > 0 {
        let comment = index - 1;
        if toks.com_kind[comment] != CommentKind::Doc {
            break;
        }
        let lo = toks.com_start[comment] as usize;
        let hi = toks.com_end[comment] as usize;
        let gap = &src.as_bytes()[hi..next as usize];
        let lines = gap.split(|byte| *byte == b'\n').count();
        if lines != 2 || !gap.iter().all(u8::is_ascii_whitespace) {
            break;
        }
        let line = toks.line_of(toks.com_start[comment]);
        let own = toks.line_start[line] as usize + usize::from(toks.line_indent[line]) == lo;
        if !own || toks.line_indent[line] != column {
            break;
        }
        next = toks.com_start[comment];
        index -= 1;
    }
    index..end
}

/// The column of a token that starts its line, or `u16::MAX` when another
/// token stands before it on the line.
pub(crate) fn column_of(src: &str, toks: &TokenBuf, start: u32) -> u16 {
    let line = toks.line_of(start);
    let line_start = toks.line_start[line];
    let before = &src.as_bytes()[line_start as usize..start as usize];
    if before.iter().all(|byte| *byte == b' ') {
        toks.line_indent[line]
    } else {
        u16::MAX
    }
}

/// One comment's text: past `##` and one space.
fn line_text<'s>(src: &'s str, toks: &TokenBuf, comment: usize) -> &'s str {
    let text = &src[toks.com_start[comment] as usize..toks.com_end[comment] as usize];
    let text = text.strip_prefix("##").unwrap_or(text);
    text.strip_prefix(' ').unwrap_or(text).trim_end()
}

fn joined(src: &str, toks: &TokenBuf, comments: impl Iterator<Item = usize>) -> String {
    comments
        .map(|c| line_text(src, toks, c))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The documentation of the declaration or member whose first token is
/// `token`, one line per `##` line, without the markers; empty when it
/// has none (`lex.doc.attach`).
#[must_use]
pub fn doc_text(src: &str, toks: &TokenBuf, token: TokenIdx) -> String {
    let Some(&start) = toks.start.get(token.idx()) else {
        return String::new();
    };
    let column = column_of(src, toks, start);
    joined(src, toks, attached_above(src, toks, start, column))
}

/// The module documentation (`lex.doc.module`): the file's first `##`
/// block, before every token and followed by a blank line; empty when the
/// file has none.
#[must_use]
pub fn module_doc_text(src: &str, toks: &TokenBuf) -> String {
    joined(
        src,
        toks,
        (0..toks.com_kind.len()).filter(|&c| toks.com_kind[c] == CommentKind::ModuleDoc),
    )
}

#[cfg(test)]
mod tests {
    use super::{doc_text, module_doc_text};
    use crate::{Decl, parse};

    #[test]
    fn item_and_module_text() {
        let src = "## The module.\n## Second line.\n\n## Adds one.\n##\n##    indented\npub fn f(x: i32) -> i32: x\n\n# plain\nfn g() -> i32: 1\n";
        let p = parse(src.as_bytes());
        assert_eq!(module_doc_text(src, &p.tokens), "The module.\nSecond line.");
        let docs: Vec<String> = p
            .tree
            .root()
            .children()
            .filter_map(Decl::cast)
            .map(|d| doc_text(src, &p.tokens, d.node().first_token()))
            .collect();
        assert_eq!(docs, ["Adds one.\n\n   indented", ""]);
    }
}
