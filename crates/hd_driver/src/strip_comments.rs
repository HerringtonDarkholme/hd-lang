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

#[cfg(test)]
mod tests {
    use super::{strip_all_comments, strip_plain_comments};
    use crate::{STD_FILES, STD_FILES_WITH_DOCS, STD_FILES_WITHOUT_DOCS};
    use hd_syntax::lex;
    use std::path::Path;

    /// Every token's kind and text, then every comment's kind and text, then
    /// the diagnostics' count: all that the compiler and `hd doc` read.
    #[derive(Debug, PartialEq)]
    struct Reading {
        tokens: Vec<(u8, String)>,
        docs: Vec<(u8, String)>,
        diagnostics: usize,
        lines: usize,
    }

    fn reading(source: &str) -> Reading {
        let lexed = lex(source.as_bytes());
        let t = &lexed.tokens;
        let tokens = (0..t.len())
            .map(|i| {
                (
                    t.kind[i] as u8,
                    source[t.start[i] as usize..t.end[i] as usize].to_owned(),
                )
            })
            .collect();
        let docs = (0..t.com_kind.len())
            .filter(|&i| t.com_kind[i] != hd_syntax::CommentKind::Plain)
            .map(|i| {
                (
                    t.com_kind[i] as u8,
                    source[t.com_start[i] as usize..t.com_end[i] as usize].to_owned(),
                )
            })
            .collect();
        Reading {
            tokens,
            docs,
            diagnostics: lexed.diagnostics.len(),
            lines: t.line_start.len(),
        }
    }

    #[test]
    fn strings_keep_their_hash() {
        let src = "let a = \"x # y\"  # note\nlet b = r\"a # b ${f(\"#\")} # c\" # t\nlet c = \"v {1 # w\n}\"\n## doc\n# plain\n  # indented\n";
        let out = strip_plain_comments(src);
        assert_eq!(
            out,
            "let a = \"x # y\"\nlet b = r\"a # b ${f(\"#\")} # c\"\nlet c = \"v {1 # w\n}\"\n## doc\n\n\n"
        );
        assert_eq!(reading(src).tokens, reading(&out).tokens);
    }

    #[test]
    fn std_embeds_without_plain_comments_and_lexes_the_same() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
        assert!(!STD_FILES.is_empty());
        let mut shrunk = 0;
        for (rel, embedded) in STD_FILES {
            let original = std::fs::read_to_string(root.join(rel)).expect("std file");
            assert_eq!(*embedded, strip_plain_comments(&original), "{rel}");
            assert_eq!(reading(&original), reading(embedded), "{rel}");
            assert_eq!(original.lines().count(), embedded.lines().count(), "{rel}");
            for (before, after) in original.lines().zip(embedded.lines()) {
                if before != after {
                    assert_eq!(after, after.trim_end(), "{rel}: {before:?}");
                }
            }
            shrunk += original.len() - embedded.len();
        }
        assert!(shrunk > 50_000, "plain comments should be stripped");
    }

    #[test]
    fn all_comments_go_and_lines_stay() {
        let src = "## doc\nlet a = \"## s\"  ## tail\n# plain\n";
        assert_eq!(strip_all_comments(src), "\nlet a = \"## s\"\n\n");
    }

    #[test]
    fn web_embed_has_no_doc_comments_and_native_keeps_them() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
        assert_eq!(STD_FILES_WITH_DOCS.len(), STD_FILES_WITHOUT_DOCS.len());
        // This test builds natively, which embeds the copy with docs.
        assert_eq!(STD_FILES, STD_FILES_WITH_DOCS);
        let mut docs = 0;
        for ((rel, with), (rel2, without)) in STD_FILES_WITH_DOCS.iter().zip(STD_FILES_WITHOUT_DOCS)
        {
            assert_eq!(rel, rel2);
            let original = std::fs::read_to_string(root.join(rel)).expect("std file");
            assert_eq!(*with, strip_plain_comments(&original), "{rel}");
            assert_eq!(*without, strip_all_comments(&original), "{rel}");
            assert!(
                !without.lines().any(|l| l.trim_start().starts_with('#')),
                "{rel}: web embed has a comment line"
            );
            assert_eq!(with.lines().count(), without.lines().count(), "{rel}");
            docs += with
                .lines()
                .filter(|l| l.trim_start().starts_with("##"))
                .count();
        }
        assert!(docs > 1000, "native embed keeps doc comments: {docs}");
    }
}
