//! Parser fix-it internals: where a missing import goes, by byte offset in LF
//! and CRLF text, and the edit count of a long `mut` prefix. What each parser
//! fix-it edits is a stage case under
//! `crates/hd_check/tests/cases/syntax-fix-its/`.

use hd_base::SliceSort as _;
use hd_syntax::{UseInsertion, parse};

use super::corpus::{blocks, stage_cases};

/// A parser fix-it case is of one kind. Either every diagnostic has exactly
/// one fix-it, and the file with all of them applied parses clean; or no
/// diagnostic has a fix-it, and each snippet is rejected alone.
#[test]
fn a_fix_it_case_fixes_every_diagnostic_or_none() {
    // cost: one walk of the topic's folder; each case is parsed once whole,
    // then once more fixed or once per snippet.
    for path in stage_cases("hd_check", "syntax-fix-its") {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let parsed = parse(source.as_bytes());
        assert!(!parsed.is_ok(), "{}: no diagnostic", path.display());
        if parsed.diagnostics.iter().all(|d| d.fixes.is_empty()) {
            for snippet in blocks(&source) {
                let alone = parse(snippet.as_bytes());
                assert!(!alone.is_ok(), "{snippet}");
                assert!(
                    alone.diagnostics.iter().all(|d| d.fixes.is_empty()),
                    "{snippet}: {:?}",
                    alone.diagnostics
                );
            }
            continue;
        }
        let mut edits = Vec::new();
        for d in &parsed.diagnostics {
            assert_eq!(d.fixes.len(), 1, "{}: {d:?}", path.display());
            edits.extend(d.fixes[0].edits.iter().cloned());
        }
        edits.sort_stable_by_key(|(span, _)| std::cmp::Reverse(span.lo));
        let mut edited = source.clone();
        for (span, replacement) in edits {
            edited.replace_range(span.lo as usize..span.hi as usize, &replacement);
        }
        let checked = parse(edited.as_bytes());
        assert!(checked.is_ok(), "{edited}: {:?}", checked.diagnostics);
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
