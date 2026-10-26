use hd_base::SliceSort as _;
use hd_syntax::parse;

#[test]
fn legacy_syntax_fixes_preserve_the_actual_names() {
    for (source, corrected) in [
        ("const LIMIT: i32 = 10\n", "let LIMIT: i32 = 10\n"),
        (
            "fn f(number: i32): (number as u8)\n",
            "fn f(number: i32): (u8(number))\n",
        ),
        (
            "# café\nfn f(τιμή: i32): (τιμή as u8)\n",
            "# café\nfn f(τιμή: i32): (u8(τιμή))\n",
        ),
        (
            "fn f(x: X):\n    match x:\n        Cell[List[i32]].Full(v) => v\n",
            "fn f(x: X):\n    match x:\n        Cell.Full(v) => v\n",
        ),
    ] {
        let parsed = parse(source.as_bytes());
        let fixes: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .collect();
        assert_eq!(fixes.len(), 1, "{source}: {:?}", parsed.diagnostics);
        if source.contains("τιμή") {
            assert!(
                parsed
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains("u8(τιμή)"))
            );
        }
        let mut edits = fixes[0].edits.clone();
        edits.sort_stable_by_key(|(span, _)| std::cmp::Reverse(span.lo));
        let mut edited = source.to_owned();
        for (span, replacement) in edits {
            edited.replace_range(span.lo as usize..span.hi as usize, &replacement);
        }
        assert_eq!(edited, corrected);
        let checked = parse(edited.as_bytes());
        assert!(checked.is_ok(), "{edited}: {:?}", checked.diagnostics);
    }
}

#[test]
fn legacy_syntax_fixes_do_not_guess_other_forms() {
    for source in [
        "const LIMIT: i32\n",
        "const LIMIT: i32 =\n",
        "const LIMIT: Box[Item = i32]\n",
        "fn f(value: i32): (-value as u8)\n",
        "fn f(value: i32): (value as Other)\n",
        "fn f(value: i32): (value as u8 + 1)\n",
        "fn f(x: X):\n    match x:\n        Cell[0].Full(v) => v\n",
        "fn f(x: X):\n    match x:\n        Cell[choose()].Full(v) => v\n",
    ] {
        let parsed = parse(source.as_bytes());
        assert!(!parsed.is_ok(), "{source}");
        assert!(
            parsed.diagnostics.iter().all(|d| d.fix.is_none()),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
    let source = "fn const(x: i32): x\nfn as(x: i32): x\nfn f(): const(as(1))\n";
    let parsed = parse(source.as_bytes());
    assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
}
