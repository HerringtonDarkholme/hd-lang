use hd_syntax::parse;

#[test]
fn moved_punctuation_and_documentation_fixes_parse() {
    for (source, corrected) in [
        ("fn f!(): value()!\n", "fn f!(): value!()\n"),
        (
            "fn f!(): value::[i32](+1)!\n",
            "fn f!(): value!::[i32](+1)\n",
        ),
        ("fn f(xs: P): [...xs, 1]\n", "fn f(xs: P): [xs..., 1]\n"),
        ("fn f(xs: P): [...`xs`]\n", "fn f(xs: P): [`xs`...]\n"),
        (
            "# café\n/// Returns a value.\nfn f(): +1\n",
            "# café\n## Returns a value.\nfn f(): +1\n",
        ),
        (
            "/// Public value.\npub struct Value: pass\n",
            "## Public value.\npub struct Value: pass\n",
        ),
    ] {
        let parsed = parse(source.as_bytes());
        let fixes: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .collect();
        assert_eq!(fixes.len(), 1, "{source}: {:?}", parsed.diagnostics);
        let mut edits = fixes[0].edits.clone();
        edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.lo));
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
fn incomplete_spreads_and_other_comment_positions_have_no_fix() {
    for source in [
        "fn f!(): value!()!\n",
        "fn f!(): value() !\n",
        "fn f(): [...]\n",
        "fn f(): [..., value]\n",
        "fn f(): [...value...]\n",
        "fn f(): [...value +]\n",
        "fn f(): [...copy(value)]\n",
        "fn f(): [...value.]\n",
        "fn f():\n    /// Not declaration documentation.\n    pass\n",
        "/// Separated.\n\nfn f(): pass\n",
        "/// Before a binding.\nvalue := +1\n",
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
