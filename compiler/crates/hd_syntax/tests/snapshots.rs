//! `.tree` snapshots of `tests/trees/*.hd`: the outline of each green tree
//! plus its diagnostics. `HD_BLESS=1 cargo test -p hd_syntax --test
//! snapshots` rewrites them.

use std::fmt::Write;
use std::fs;
use std::path::Path;

use hd_syntax::parse;

fn render(source: &str) -> String {
    let parsed = parse(source.as_bytes());
    let mut out = parsed.tree.outline(&parsed.tokens, source);
    for diagnostic in &parsed.diagnostics {
        let line = parsed.tokens.line_of(diagnostic.primary.lo);
        let column = diagnostic.primary.lo - parsed.tokens.line_start[line];
        let _ = writeln!(
            out,
            "diagnostic {}:{}: {}",
            line + 1,
            column + 1,
            diagnostic.code.as_str()
        );
    }
    out
}

#[test]
fn tree_snapshots_match() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/trees");
    let bless = std::env::var_os("HD_BLESS").is_some();
    let mut inputs: Vec<_> = fs::read_dir(&dir)
        .expect("tests/trees")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "hd"))
        .collect();
    inputs.sort();
    assert!(inputs.len() >= 9, "snapshot inputs missing");
    let mut failures = Vec::new();
    for input in inputs {
        let source = fs::read_to_string(&input).expect("read input");
        let actual = render(&source);
        let expected_path = input.with_extension("tree");
        if bless {
            fs::write(&expected_path, &actual).expect("write snapshot");
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if expected != actual {
            failures.push(format!(
                "{} differs; rerun with HD_BLESS=1 to accept:\n{actual}",
                expected_path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
