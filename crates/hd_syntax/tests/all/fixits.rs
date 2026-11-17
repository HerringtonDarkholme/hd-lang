use hd_base::SliceSort as _;
use hd_syntax::{UseInsertion, parse};

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
        let fixes: Vec<_> = parsed.diagnostics.iter().flat_map(|d| &d.fixes).collect();
        assert_eq!(fixes.len(), 1, "{source}: {:?}", parsed.diagnostics);
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
            parsed.diagnostics.iter().all(|d| d.fixes.is_empty()),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
}

/// `Parse::use_insertion`: a missing import's `use` goes at the end of
/// the line of the last top-level `use`, past its comment; else after
/// the module documentation and its blank line; else at the top. A
/// nested `use` and a comment above the first item do not count. The
/// line takes the file's ending: in a `\r\n` file it goes before the
/// `\r` of the last `use` line, and ends in `\r\n` (#1862).
#[test]
fn a_missing_import_goes_after_the_last_use_or_the_module_doc() {
    let lf = |at, after_use| UseInsertion {
        at,
        after_use,
        newline: "\n",
    };
    let crlf = |at, after_use| UseInsertion {
        at,
        after_use,
        newline: "\r\n",
    };
    for (source, want) in [
        (
            "use std.a\nfn f(): +1\nuse std.d # last\n\nfn g(): +1\n",
            lf(37, true),
        ),
        (
            "## Module.\n\n## The function.\nfn f(): +1\n",
            lf(12, false),
        ),
        ("## Attached.\nfn f(): +1\n", lf(0, false)),
        ("# header\n\nfn f():\n    use std.a\n    +1\n", lf(0, false)),
        ("", lf(0, false)),
        // The last line, with no ending: the file's first line ending.
        ("fn f(): +1\r\nuse std.a", crlf(21, true)),
        ("use std.a", lf(9, true)),
        (
            "use std.a\r\nfn f(): +1\r\nuse std.d # last\r\n\r\nfn g(): +1\r\n",
            crlf(39, true),
        ),
        (
            "## Module.\r\n\r\n## The function.\r\nfn f(): +1\r\n",
            crlf(14, false),
        ),
        ("fn f(): +1\r\n", crlf(0, false)),
    ] {
        let parsed = parse(source.as_bytes());
        assert_eq!(parsed.use_insertion(source), want, "{source:?}");
        // The answer is kept for the next call.
        assert_eq!(parsed.use_insertion(source), want, "{source:?}");
    }
}

#[test]
fn repeated_mut_type_prefixes_have_one_diagnostic_and_linear_edit_work() {
    for n in [1000, 10000] {
        let source = format!("fn f(x: {}i32) -> void: pass\n", "mut ".repeat(n));
        let parsed = parse(source.as_bytes());
        assert_eq!(parsed.diagnostics.len(), 1, "{n}: {:?}", parsed.diagnostics);
        let fixes = &parsed.diagnostics[0].fixes;
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0].edits.len(), n + 1);
        assert_eq!(fixes[1].edits.len(), n);
    }
}
