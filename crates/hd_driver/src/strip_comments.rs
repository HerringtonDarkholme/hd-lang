//! Comments removed from a source text, for the embedded std.
//!
//! Shared by `build.rs` (which embeds the result) and the tests (which prove
//! the result lexes like the original). A comment is found by the real lexer,
//! so a `#` inside a string, a raw string or an interpolation stays. Every
//! line break stays, so line numbers and columns of code do not change. The
//! native build keeps `##` doc comments, which `hd doc` reads; the browser
//! build drops them too.

use hd_syntax::{CommentKind, lex};

/// `source` without its plain `#` comments. A comment's own line keeps its
/// line break, and the spaces before a trailing comment go with it.
#[must_use]
pub fn strip_plain_comments(source: &str) -> String {
    strip(source, |kind| kind == CommentKind::Plain)
}

/// `source` without any comment, `##` documentation comments included: the
/// browser build embeds this, since nothing there reads documentation.
#[must_use]
pub fn strip_all_comments(source: &str) -> String {
    strip(source, |_| true)
}

fn strip(source: &str, drop: impl Fn(CommentKind) -> bool) -> String {
    let lexed = lex(source.as_bytes());
    let toks = &lexed.tokens;
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for i in 0..toks.com_kind.len() {
        if !drop(toks.com_kind[i]) {
            continue;
        }
        let start = toks.com_start[i] as usize;
        let end = toks.com_end[i] as usize;
        out.push_str(&source[at..start]);
        let kept = out.trim_end_matches([' ', '\t']).len();
        out.truncate(kept);
        at = end;
    }
    out.push_str(&source[at..]);
    out
}
