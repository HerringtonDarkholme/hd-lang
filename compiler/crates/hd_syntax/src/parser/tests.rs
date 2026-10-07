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
