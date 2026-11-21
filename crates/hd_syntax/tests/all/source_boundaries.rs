//! Source queries remain usable at empty, truncated and non-UTF-8 boundaries.

use hd_base::{Hash128, TokenIdx};
use hd_syntax::{
    GreenTree, LayoutCursor, TokenOrLayout, doc_text, lex, parse, skim, top_level_names,
};

#[test]
fn a_default_tree_is_empty_but_an_empty_source_has_a_root() {
    let tree = GreenTree::default();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    let parsed = parse(b"");
    assert!(!parsed.tree.is_empty());
    assert_eq!(parsed.tree.len(), 1);
    assert_eq!(parsed.tree.root().children().count(), 0);
    assert_eq!(parsed.tree.root().direct_tokens().count(), 0);
    assert_eq!(parsed.tree.parent(parsed.tree.root().index()), None);
}

#[test]
fn absent_declaration_tokens_have_no_documentation() {
    for source in [
        "",
        "## Module only.\n\n",
        "## Item.\nfn item() -> void: pass\n",
    ] {
        let lexed = lex(source.as_bytes());
        assert_eq!(
            doc_text(source, &lexed.tokens, TokenIdx::from_raw(u32::MAX - 1)),
            ""
        );
    }
}

#[test]
fn bumping_an_exhausted_layout_cursor_preserves_eof() {
    for source in ["", "# comment\n", "name"] {
        let lexed = lex(source.as_bytes());
        let mut cursor = LayoutCursor::new(&lexed.tokens, source);
        // A cursor emits at most one token and two layout markers per source token.
        for _ in 0..3 * lexed.tokens.len() + 3 {
            if cursor.peek() == TokenOrLayout::Eof {
                break;
            }
            cursor.bump();
        }
        assert_eq!(cursor.peek(), TokenOrLayout::Eof);
        let position = cursor.pos();
        for _ in 0..3 {
            cursor.bump();
            assert_eq!(cursor.peek(), TokenOrLayout::Eof);
            assert_eq!(cursor.pos(), position);
            assert!(cursor.pending().is_empty());
        }
    }
}

#[test]
fn invalid_utf8_skim_has_no_reusable_skeleton() {
    for source in [
        b"\xff".as_slice(),
        b"fn good() -> void: pass\n\xff",
        b"\xf0\x9f",
    ] {
        let skeleton = skim(source);
        assert!(skeleton.broken);
        assert!(skeleton.bodies.is_empty());
        assert!(skeleton.uses.is_empty());
        assert!(skeleton.test_uses.is_empty());
        assert!(skeleton.code_lines.is_empty());
        assert_eq!(skeleton.source_hash, Hash128::default());
        assert_eq!(skeleton.api_text_hash, Hash128::default());
        assert_eq!(skeleton.template_text_hash, Hash128::default());
        assert_eq!(skeleton.token_hash, Hash128::default());
    }
}

#[test]
fn an_unfinished_annotation_does_not_invent_a_declaration_name() {
    for tail in ["@derive(", "@derive(Eq", "@derive((Eq)"] {
        let source = format!("fn earlier() -> void: pass\n{tail}");
        assert_eq!(top_level_names(&source), ["earlier"]);
    }
    assert_eq!(
        top_level_names("@derive(Eq) pub struct Kept: pass\n"),
        ["Kept"]
    );
}
