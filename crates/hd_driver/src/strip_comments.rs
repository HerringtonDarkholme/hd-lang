//! Plain `#` comments removed from a source text, for the embedded std.
//!
//! Shared by `build.rs` (which embeds the result) and the tests (which prove
//! the result lexes like the original). A comment is found by the real lexer,
//! so a `#` inside a string, a raw string or an interpolation stays. Every
//! line break stays and `##` doc comments stay, so line numbers, columns of
//! code and `hd doc` output do not change.

use hd_syntax::{CommentKind, lex};

/// `source` without its plain `#` comments. A comment's own line keeps its
/// line break, and the spaces before a trailing comment go with it.
#[must_use]
pub fn strip_plain_comments(source: &str) -> String {
    let lexed = lex(source.as_bytes());
    let toks = &lexed.tokens;
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for i in 0..toks.com_kind.len() {
        if toks.com_kind[i] != CommentKind::Plain {
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
    use super::strip_plain_comments;
    use crate::STD_FILES;
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
}
