//! Inspect the std formatting projection without editing any std source.
use std::fs;
use std::path::{Path, PathBuf};

use hd_base::SliceSort as _;
use hd_fmt::{MAX_COLUMNS, format};
use hd_syntax::SyntaxKind;

fn sources(directory: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("std directory") {
        let path = entry.expect("std entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|s| s == "hd") {
            out.push(path);
        }
    }
}

#[test]
fn std_projection_is_idempotent_and_every_breakable_code_line_fits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
    let mut files = Vec::new();
    sources(&root, &mut files);
    files.sort_stable();
    let mut wide = Vec::new();
    for file in files {
        let original = fs::read_to_string(&file).expect("std source");
        let formatted =
            format(&original).unwrap_or_else(|diags| panic!("{}: {diags:?}", file.display()));
        assert_eq!(
            format(&formatted).expect("formatted std parses"),
            formatted,
            "{}",
            file.display()
        );
        let parsed = hd_syntax::parse(formatted.as_bytes());
        let literals: Vec<_> = parsed
            .tree
            .root()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::StringExpr)
            .map(|n| {
                (
                    parsed.tokens.start[n.first_token().idx()] as usize,
                    parsed.tokens.end[n.last_token().idx()] as usize,
                )
            })
            .collect();
        let mut start = 0;
        for (line, text) in formatted.split_inclusive('\n').enumerate() {
            let stop = start + text.trim_end_matches('\n').len();
            let code_end = parsed
                .tokens
                .com_start
                .iter()
                .map(|&p| p as usize)
                .find(|&p| p >= start && p < stop)
                .unwrap_or(stop);
            let code = formatted[start..code_end].trim_end();
            if code.chars().count() > MAX_COLUMNS {
                let indentation = code.chars().take_while(|c| matches!(c, ' ' | '\t')).count();
                let literal_cannot_fit = literals.iter().any(|&(lo, hi)| {
                    let (lo, hi) = (lo.max(start), hi.min(code_end));
                    lo < hi && indentation + formatted[lo..hi].chars().count() > MAX_COLUMNS
                });
                if !literal_cannot_fit {
                    wide.push(format!(
                        "{}:{}: {} columns: {code}",
                        file.display(),
                        line + 1,
                        code.chars().count()
                    ));
                }
            }
            start += text.len();
        }
    }
    assert!(wide.is_empty(), "{}", wide.join("\n"));
}
