use std::fmt::Write;

use crate::{SyntaxKind, parse};

#[test]
fn tree_has_typed_function_and_dyn_type() {
    let source = "fn show(value: dyn Display) -> string:\n    value.to_string()\n";
    let parsed = parse(source.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostic_codes());
    let function = parsed.tree.root().children().next().expect("function");
    assert_eq!(function.kind(), SyntaxKind::FnDecl);
    assert!(
        function
            .descendants()
            .any(|node| node.kind() == SyntaxKind::DynType)
    );
    assert_eq!(parsed.tree.reconstruct(&parsed.tokens, source), source);
}

#[test]
fn delimiter_recovery_reports_and_continues() {
    let parsed = parse(b"first := [1, 2)\nsecond := +2\n");
    assert!(
        parsed
            .diagnostic_codes()
            .iter()
            .any(|code| code.as_str() == "unmatched-delimiter")
    );
    assert!(parsed.tree.len() >= 3);
}

/// Each diagnostic of `source` as `line: code`.
fn diagnostics_by_line(source: &str) -> Vec<String> {
    parse(source.as_bytes())
        .diagnostics
        .iter()
        .map(|d| {
            let at = usize::try_from(d.primary.lo).map_or(source.len(), |at| at.min(source.len()));
            let line = source[..at].matches('\n').count() + 1;
            format!("{line}: {}", d.code.as_str())
        })
        .collect()
}

/// `grammar.recovery.statement`: an error's fallout (the body its header
/// would have opened, its `else` lines, the rest of a line the lexer
/// rejected) adds no second diagnostic. (source, `line: code`)
const ONE_ERROR: &[(&str, &str)] = &[
    (
        "fn both(a: bool, b: bool) -> bool:\n    if a and b:\n        true\n    else:\n        false\n",
        "2: syntax-error",
    ),
    (
        "trait Log\n\nfn make() -> fn() -> i32 $ Log $ Console:\n    fn() -> i32 $ Log: 1\n",
        "3: syntax-error",
    ),
    (
        "trait Source:\n    type Item\n\nimpl[T] Source for Box[T] where T < Any:\n    type Item = T\n\n    fn get(self) -> T: self.value\n",
        "4: syntax-error",
    ),
    (
        "fn counter() -> mut fn() -> i32:\n    fn() -> i32: 1\n",
        "1: syntax-error",
    ),
    (
        "fn counter() -> i32:\n    let count = +0\n    next := mut fn() -> i32:\n        count = count + 1\n        count\n    next()\n",
        "3: syntax-error",
    ),
    (
        "fn pick(flag: bool) -> i32:\n    if flag: 5else: 3\n",
        "2: syntax-error",
    ),
    (
        "fn total(n: i32) -> i32:\n    result := n |> run(_):\n        1\n    result\n",
        "2: syntax-error",
    ),
    (
        "fn close_all(close: bool) -> void:\n    if close: trailing:\n        pass\n",
        "2: syntax-error",
    ),
    (
        "fn names(scores: Map[string, i32]) -> List[string]:\n    for name, score in scores:\n        pass\n    [for (name, score) in scores => name]\n",
        "2: syntax-error",
    ),
    ("fn invalid() -> void:\n    d := 1..=\n", "2: syntax-error"),
    (
        "fn count() -> i32:\n    tests := 3\n    tests\n",
        "2: syntax-error",
    ),
    ("value := 1 + \\\n    2\n", "1: invalid-token"),
    (
        "fn first(pair: (i32, i32)) -> i32:\n    value := pair\n        .0\n    value\n",
        "3: syntax-error",
    ),
    ("fn invalid() -> void:\n\tpass\n", "2: tab-whitespace"),
    ("fn f() -> i32:\n\t1\n", "2: tab-whitespace"),
    (
        "fn f() -> void:\n    x := +1\n\ty := +2\n",
        "3: tab-whitespace",
    ),
    (
        "fn f(x: bool) -> void:\n    if x:\n\ty := +2\n",
        "3: tab-whitespace",
    ),
];

#[test]
fn an_error_reports_once_without_its_fallout() {
    let failures: Vec<String> = ONE_ERROR
        .iter()
        .filter_map(|&(source, wanted)| {
            let got = diagnostics_by_line(source);
            (got != [wanted]).then(|| format!("{source:?}: wanted [{wanted}], got {got:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn independent_errors_in_two_items_both_report() {
    let source = "fn first(a: bool, b: bool) -> bool:\n    if a and b:\n        true\n    else:\n        false\n\nfn second() -> i32:\n    x := (1 2)\n    x\n";
    assert_eq!(
        diagnostics_by_line(source),
        ["2: syntax-error", "8: syntax-error"]
    );
}

#[test]
fn a_later_statement_in_the_same_body_still_reports() {
    let source =
        "fn f(a: bool, b: bool) -> i32:\n    if a and b:\n        pass\n    y := [1 2]\n    y\n";
    assert_eq!(
        diagnostics_by_line(source),
        ["2: syntax-error", "4: syntax-error"]
    );
}

#[test]
fn stray_indentation_reports_once_and_its_later_lines_report_their_own() {
    let source = "fn f() -> i32:\n    x := +1\n        .0\n        y := (1 2)\n    x\n";
    assert_eq!(
        diagnostics_by_line(source),
        ["3: syntax-error", "4: syntax-error"]
    );
}

#[test]
fn a_reserved_binding_name_is_scoped_to_its_item() {
    let source = "fn f() -> i32:\n    tests := 3\n    tests\n\nfn g() -> i32:\n    tests\n";
    assert_eq!(
        diagnostics_by_line(source),
        ["2: syntax-error", "6: syntax-error"]
    );
}

/// `grammar.recovery.unclosed`: an unclosed bracket's scope ends at the
/// next column-0 item start, and later items report their own errors.
/// (source, `line: code` list)
const UNCLOSED_SCOPE: &[(&str, &[&str])] = &[
    (
        "fn first() -> i32:\n    total := add(1, 2\n    total\n\nfn second() -> i32:\n    x := (1 2)\n    x\n",
        &["2: unclosed-delimiter", "6: syntax-error"],
    ),
    (
        "fn first() -> i32:\n    values := [\n        +1\n    +2 +\n\ndata Point:\n    x i32\n",
        &["2: unclosed-delimiter", "7: syntax-error"],
    ),
    (
        "fn first() -> i32:\n    run(fn(x): [x,\n        +1 +\n\npub fn second() -> i32:\n    y := [1 2]\n    y\n",
        &[
            "2: unclosed-delimiter",
            "2: unclosed-delimiter",
            "6: syntax-error",
        ],
    ),
    (
        "fn first() -> i32:\n    run(fn(x):\nfn second() -> i32:\n    +1 +\n",
        &["2: unclosed-delimiter", "5: syntax-error"],
    ),
    (
        "fn first() -> i32:\n    total := add(1,\n\ntests:\n    it(\"adds\"):\n        x := (1 2)\n",
        &["2: unclosed-delimiter", "6: syntax-error"],
    ),
    (
        "fn first() -> i32:\n    total := add(1, 2\n    total\n\nfn second() -> i32:\n    +2\n",
        &["2: unclosed-delimiter"],
    ),
];

#[test]
fn an_unclosed_bracket_ends_at_the_next_item() {
    let failures: Vec<String> = UNCLOSED_SCOPE
        .iter()
        .filter_map(|&(source, wanted)| {
            let got = diagnostics_by_line(source);
            (got != wanted).then(|| format!("{source:?}: wanted {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A column-0 line inside balanced brackets that no item can start still
/// continues them.
#[test]
fn column_zero_continuation_inside_brackets_still_parses() {
    for source in [
        "values := [\n+1,\n+2,\n]\nnext := +3\n",
        "fn total() -> i32:\n    add(\n1,\nfn(x): x,\n)\n",
        "fn total() -> i32:\n    run(\nfn(x: i32) -> i32:\n        x\n,\n)\n",
        "fn f(\nvalue: i32,\n) -> i32:\n    value\n",
    ] {
        let parsed = parse(source.as_bytes());
        assert!(
            parsed.is_ok(),
            "{source:?}: {:?}",
            parsed.diagnostic_codes()
        );
        assert_eq!(parsed.tree.reconstruct(&parsed.tokens, source), source);
    }
}

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
