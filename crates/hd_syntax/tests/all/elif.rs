use hd_syntax::parse;

#[test]
fn elif_continuations_offer_the_keyword_edit() {
    for source in [
        "fn f(n: i32):\n    if n > 100:\n        3\n    elif n > 10:\n        2\n    else:\n        1\n",
        "fn f(n: i32):\n    if n > 100: 3\n    elif n > 10: 2\n    else: 1\n",
        "fn f(n: bool):\n    if n: 3\n    elif false: 2\n    else: 1\n",
    ] {
        let parsed = parse(source.as_bytes());
        let fixes: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .collect();
        assert_eq!(fixes.len(), 1, "{source}: {:?}", parsed.diagnostics);
        assert_eq!(fixes[0].edits.len(), 1);
        let (span, replacement) = &fixes[0].edits[0];
        assert_eq!(&source[span.lo as usize..span.hi as usize], "elif");
        assert_eq!(replacement, "else if");
        let mut edited = source.to_owned();
        edited.replace_range(span.lo as usize..span.hi as usize, replacement);
        let checked = parse(edited.as_bytes());
        assert!(checked.is_ok(), "{edited}: {:?}", checked.diagnostics);
    }
}

#[test]
fn elif_remains_a_name_and_incomplete_headers_have_no_guessed_fix() {
    for source in [
        "fn f(n: bool):\n    if n: pass\n    elif(1)\n",
        "fn f(n: bool):\n    if n: pass\n    elif := 1\n    elif\n",
        "fn f(n: bool):\n    if n: pass\n    elif.ready()\n",
        "fn f(n: bool):\n    if n: pass\n    elif::[i32](1)\n",
    ] {
        let parsed = parse(source.as_bytes());
        assert!(parsed.is_ok(), "{source}: {:?}", parsed.diagnostics);
        assert!(parsed.diagnostics.iter().all(|d| d.fix.is_none()));
    }
    for source in [
        "fn f(n: bool):\n    if n: pass\n    elif ready\n",
        "fn f(n: bool):\n    if n: pass\n    elif ready(call: X)\n",
        "fn f():\n    elif ready: pass\n",
    ] {
        let parsed = parse(source.as_bytes());
        assert!(!parsed.is_ok(), "{source}");
        assert!(
            parsed.diagnostics.iter().all(|d| d.fix.is_none()),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
}
