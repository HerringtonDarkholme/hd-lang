use std::fmt::Write;

use crate::{SyntaxKind, parse};

#[test]
fn an_else_if_chain_is_one_flat_node() {
    // grammar.limit.nesting, flow.if.else-if: each branch is a sibling, so
    // a chain far longer than the nesting limit parses, at any length.
    let mut source = String::from("fn pick(x: i32) -> i32:\n    if x == +0: +0\n");
    for n in 1..10_000 {
        writeln!(source, "    else if x == +{n}: +{n}").unwrap();
    }
    source.push_str("    else: -1\n");
    let parsed = parse(source.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostic_codes());
    let chain = parsed
        .tree
        .root()
        .descendants()
        .find(|node| node.kind() == SyntaxKind::IfExpr)
        .expect("if");
    let kinds: Vec<SyntaxKind> = chain.children().map(crate::NodeRef::kind).collect();
    assert_eq!(kinds.len(), 2 + 9_999 + 1);
    assert_eq!(kinds[2], SyntaxKind::ElseIfClause);
    assert_eq!(kinds.last(), Some(&SyntaxKind::ElseClause));
    let ifs = parsed.tree.root().descendants();
    assert_eq!(ifs.filter(|n| n.kind() == SyntaxKind::IfExpr).count(), 1);
    assert_eq!(parsed.tree.reconstruct(&parsed.tokens, &source), source);
}

#[test]
fn an_if_nested_in_each_else_suite_still_nests_too_deep() {
    // An `if` inside an indented `else` suite is a real nesting: each one
    // opens a suite, so 200 of them pass the limit.
    let mut source = String::from("fn pick(x: i32) -> i32:\n");
    for n in 0..200 {
        let pad = "    ".repeat(n + 1);
        write!(source, "{pad}if x == +{n}:\n{pad}    +{n}\n{pad}else:\n").unwrap();
    }
    writeln!(source, "{}-1", "    ".repeat(201)).unwrap();
    let parsed = parse(source.as_bytes());
    assert!(
        parsed
            .diagnostic_codes()
            .iter()
            .any(|code| code.as_str() == "nesting-too-deep")
    );
}

/// `fn f(x: T) -> T:` and one expression of `x` followed by `links`
/// copies of `link`.
fn chain(link: &str, links: usize) -> String {
    format!("fn f(x: i32) -> i32:\n    x{}\n", link.repeat(links))
}

fn too_deep(source: &str) -> Vec<(u32, &'static str)> {
    parse(source.as_bytes())
        .diagnostics
        .iter()
        .map(|d| (d.primary.lo, d.code.as_str()))
        .collect()
}

#[test]
fn a_chain_of_ten_thousand_links_parses_and_one_more_is_too_deep() {
    // grammar.limit.chain: a binary operator, a member, a call, an index
    // or a `?` is one link; `.m(...)` and `T::f(...)` are one link each.
    // A pipe step's call is a link too, and `.m!` and `::[i32]` are one
    // each.
    for (link, links) in [
        (" + x", 10_000),
        (" |> f(_)", 9_999),
        (".a", 10_000),
        ("()", 10_000),
        ("[0]", 10_000),
        ("?", 10_000),
        (".m()", 10_000),
        (".m!::[i32]()", 3_333),
    ] {
        let codes = too_deep(&chain(link, links));
        assert!(
            codes.iter().all(|(_, c)| *c != "nesting-too-deep"),
            "{link}"
        );
    }
    for link in [" + x", ".a", "()", "[0]", "?", ".m()"] {
        let source = chain(link, 10_001);
        let last = u32::try_from(source.len() - 1 - link.trim_start().len()).unwrap();
        assert_eq!(too_deep(&source), [(last, "nesting-too-deep")], "{link}");
    }
}

#[test]
fn a_type_suffix_of_ten_thousand_marks_parses_and_one_more_is_too_deep() {
    // grammar.limit.chain: each `?` after a type is a link (#1827).
    let suffix = |n: usize| format!("fn f(x: i32{}) -> i32: 0\n", "?".repeat(n));
    assert!(too_deep(&suffix(10_000)).is_empty());
    let past = suffix(10_001);
    let first_past = u32::try_from("fn f(x: i32".len() + 10_000).unwrap();
    assert_eq!(too_deep(&past), [(first_past, "nesting-too-deep")]);
    // 100,000 overflowed the stack; the rest of the suffix is skipped.
    assert_eq!(too_deep(&suffix(100_000)).len(), 1);
    let typed = format!(
        "fn f() -> i32:\n    let x: i32{} = 1\n    0\n",
        "?".repeat(10_001)
    );
    assert_eq!(too_deep(&typed).len(), 1);
}

#[test]
fn a_chain_counts_the_links_of_its_bracketed_operand() {
    // 5,000 links inside the parentheses and 5,001 after them: the
    // deepest path has 10,001.
    let source = format!(
        "fn f(x: i32) -> i32:\n    (x{}){}\n",
        " + x".repeat(5_000),
        " + x".repeat(5_001)
    );
    let codes = too_deep(&source);
    assert_eq!(codes.len(), 1);
    assert_eq!(codes[0].1, "nesting-too-deep");
    let fits = format!(
        "fn f(x: i32) -> i32:\n    (x{}){}\n",
        " + x".repeat(5_000),
        " + x".repeat(5_000)
    );
    assert!(too_deep(&fits).is_empty());
    // Two chains side by side are not one path.
    let wide = format!(
        "fn f(x: i32) -> i32:\n    g(x{}, x{})\n",
        " + x".repeat(6_000),
        " + x".repeat(6_000)
    );
    assert!(too_deep(&wide).is_empty());
}

/// `fn f(x: T) -> T:` and one expression of `count` copies of the prefix
/// operator `op` before `x`.
fn prefixed(op: &str, count: usize) -> String {
    format!("fn f(x: i32) -> i32:\n    {}x\n", op.repeat(count))
}

#[test]
fn a_prefix_chain_of_ten_thousand_operators_parses_and_one_more_is_too_deep() {
    // grammar.limit.chain: each prefix operator, `*` included, is one
    // link (#1955). The parser takes the chain in a loop, so 100,000 of
    // them neither overflow its stack nor nest past the limit twice.
    let head = "fn f(x: i32) -> i32:\n    ".len();
    for op in ["* ", "- ", "+ ", "! ", "~ ", "& ", "&mut ", "* - "] {
        let links = if op == "* - " { 5_000 } else { 10_000 };
        let parsed = parse(prefixed(op, links).as_bytes());
        assert!(parsed.is_ok(), "{op:?}: {:?}", parsed.diagnostic_codes());
        let source = prefixed(op, links);
        assert_eq!(parsed.tree.reconstruct(&parsed.tokens, &source), source);
        let operators = parsed
            .tree
            .root()
            .descendants()
            .filter(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::DerefExpr | SyntaxKind::UnaryExpr | SyntaxKind::RefExpr
                )
            })
            .count();
        assert_eq!(operators, 10_000, "{op:?}");
    }
    for op in ["* ", "- ", "! ", "&mut "] {
        let first_past = u32::try_from(head + op.len() * 10_000).unwrap();
        assert_eq!(
            too_deep(&prefixed(op, 10_001)),
            [(first_past, "nesting-too-deep")],
            "{op:?}"
        );
        assert_eq!(too_deep(&prefixed(op, 100_000)).len(), 1, "{op:?}");
    }
}

#[test]
fn a_power_chain_of_ten_thousand_operators_parses_and_one_more_is_too_deep() {
    // grammar.limit.chain: `**` is a binary operator, one link each,
    // right-associative, so the last operand is 10,000 links deep.
    let fits = chain(" ** x", 10_000);
    let parsed = parse(fits.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostic_codes());
    let powers = parsed.tree.root().descendants();
    assert_eq!(
        powers.filter(|n| n.kind() == SyntaxKind::PowerExpr).count(),
        10_000
    );
    let past = chain(" ** x", 10_001);
    let last = u32::try_from(past.len() - 1 - "** x".len()).unwrap();
    assert_eq!(too_deep(&past), [(last, "nesting-too-deep")]);
    assert_eq!(too_deep(&chain(" ** x", 100_000)).len(), 1);
}

#[test]
fn a_prefix_chain_counts_the_links_around_it() {
    let head = "fn f(x: i32) -> i32:\n    ";
    // The operand's own postfix links lie below the prefix operators:
    // 9,999 and 2 make 10,001, reported at the operand.
    let deep = format!("{head}{}x.a.b\n", "- ".repeat(9_999));
    let at = u32::try_from(head.len() + 2 * 9_999).unwrap();
    assert_eq!(too_deep(&deep), [(at, "nesting-too-deep")]);
    assert!(too_deep(&format!("{head}{}x.a\n", "- ".repeat(9_999))).is_empty());
    // A `**`'s left operand lies below it: 10,000 links and the `**`.
    let left = format!("{head}x{} ** x\n", ".a".repeat(10_000));
    let at = u32::try_from(head.len() + 1 + 2 * 10_000 + 1).unwrap();
    assert_eq!(too_deep(&left), [(at, "nesting-too-deep")]);
    assert!(too_deep(&format!("{head}x{} ** x\n", ".a".repeat(9_999))).is_empty());
    // A binary operator above a prefix chain adds its link.
    let above = format!("{head}x + {}x\n", "- ".repeat(10_000));
    assert_eq!(too_deep(&above).len(), 1);
    assert!(too_deep(&format!("{head}x + {}x\n", "- ".repeat(9_999))).is_empty());
}

#[test]
fn opaque_result_occurrences_nest_in_return_types() {
    for (source, occurrences) in [
        ("fn f() -> Result[impl Iterator[i32], E]: +1\n", 1),
        ("fn f() -> (impl A, impl B): (+1, +2)\n", 2),
        ("fn f() -> impl Display?: .Some(+1)\n", 1),
        ("fn f() -> List[(impl A, Result[impl B, E])]: []\n", 2),
    ] {
        let parsed = parse(source.as_bytes());
        assert!(parsed.is_ok(), "{source}: {:?}", parsed.diagnostic_codes());
        assert_eq!(
            parsed
                .tree
                .root()
                .descendants()
                .filter(|node| node.kind() == SyntaxKind::ImplType)
                .count(),
            occurrences,
            "{source}"
        );
        assert_eq!(parsed.tree.reconstruct(&parsed.tokens, source), source);
    }
}

#[test]
fn an_opaque_bound_rejects_another_opaque_occurrence() {
    for source in [
        "fn f() -> impl Iterator[impl Display]: +1\n",
        "fn f() -> Result[impl Iterator[impl Display], E]: +1\n",
        "fn f() -> impl Source[Item = impl Display]: +1\n",
    ] {
        let parsed = parse(source.as_bytes());
        let diagnostic = parsed
            .diagnostics
            .iter()
            .find(|d| d.message == "an `impl` type may not appear inside another `impl` bound")
            .expect("nested opaque bound diagnostic");
        assert_eq!(diagnostic.code, hd_diag::Code::SyntaxError);
        assert_eq!(
            diagnostic.primary.lo as usize,
            source.find("impl Display").expect("inner occurrence")
        );
    }
}

/// `grammar.recovery.item` (#1879): the kinds of the top-level children a
/// parse leaves out.
fn broken_kinds(source: &str) -> Vec<SyntaxKind> {
    let parsed = parse(source.as_bytes());
    parsed
        .tree
        .root()
        .children()
        .filter(|c| parsed.is_broken(c.index()))
        .map(crate::green::NodeRef::kind)
        .collect()
}

/// A name missing before the next item's keyword is reported at that
/// keyword, yet the item that lacks it is the broken one: an item's own
/// errors are its own wherever they stand, so no nameless declaration
/// reaches a check, and a whole item after it still does.
#[test]
fn a_name_missing_before_the_next_item_breaks_the_item_that_lacks_it() {
    // The `impl` the enum's line recovery skips is junk, broken too.
    assert_eq!(
        broken_kinds("enum impl\n"),
        [SyntaxKind::EnumDecl, SyntaxKind::Error]
    );
    // At a line end, the error stands at the next line's first token.
    assert_eq!(
        broken_kinds(include_str!("nameless_struct.hd")),
        [SyntaxKind::StructDecl]
    );
    // An error raised between items, at a stray indent, still belongs to
    // the child whose source holds it.
    assert_eq!(
        broken_kinds(include_str!("stray_indent_between_items.hd")).len(),
        1
    );
}

/// Recovery can revisit an exhausted token stream.
/// Empty-token queries and suppressed diagnostic fixes must not mutate it.
#[test]
fn exhausted_parser_queries_and_suppressed_fixes_are_noops() {
    let lexed = crate::lex(b"");
    let mut parser = super::Parser::new("", &lexed.tokens, &lexed.diagnostics);
    assert_eq!(parser.nth_text(0), "");
    assert_eq!(parser.nth_text(1), "");
    assert!(parser.current().is_none());
    let events = parser.events.len();
    parser.bump();
    parser.bump();
    parser.attach_docs();
    parser.fix_error(0, "a suppressed diagnostic has no fix", &[]);
    assert_eq!(parser.events.len(), events);
    assert!(parser.errors.is_empty());
    assert!(parser.attached.iter().all(|attached| !attached));
}

/// An exhausted stream terminates line/block recovery without consuming any
/// token or manufacturing a diagnostic. These are internal cursor contracts.
#[test]
fn exhausted_parser_recovery_terminates_without_consuming_tokens() {
    let lexed = crate::lex(b"");
    let mut parser = super::Parser::new("", &lexed.tokens, &lexed.diagnostics);
    parser.recover_line();
    let events = parser.events.len();
    parser.statements(false);
    assert_eq!(parser.events.len(), events);
    assert_eq!(parser.pos(), 0);
    assert!(parser.errors.is_empty());
}
