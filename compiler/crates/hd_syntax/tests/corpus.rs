use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use hd_syntax::{lex, parse, skeleton_from_layout, skim};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("hd_syntax is nested below the repository root")
        .to_path_buf()
}

fn hd_files(root: &Path, output: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(root).unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            hd_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "hd") {
            output.push(path);
        }
    }
}

fn corpus() -> Vec<PathBuf> {
    let root = repository_root();
    let mut files = Vec::new();
    hd_files(&root.join("spec/conformance"), &mut files);
    hd_files(&root.join("lib/std"), &mut files);
    files.sort();
    files
}

#[test]
fn every_source_lexes_without_panic_and_round_trips() {
    let files = corpus();
    assert!(files.len() > 2_000, "corpus unexpectedly small");
    for path in files {
        let source =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let lexed = lex(&source);
        let text = std::str::from_utf8(&source)
            .unwrap_or_else(|error| panic!("UTF-8 {}: {error}", path.display()));
        assert_eq!(
            lexed.tokens.reconstruct(text).as_bytes(),
            source,
            "{}",
            path.display()
        );
    }
}

#[test]
fn skim_body_ranges_match_materialized_layout_reference() {
    for path in corpus() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read UTF-8 {}: {error}", path.display()));
        let tokens = lex(source.as_bytes()).tokens;
        assert_eq!(
            skim(source.as_bytes()).bodies,
            skeleton_from_layout(&source, &tokens),
            "{}",
            path.display()
        );
    }
}

#[test]
fn every_tree_round_trips_and_full_skeleton_matches_skim() {
    for path in corpus() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read UTF-8 {}: {error}", path.display()));
        let parsed = parse(source.as_bytes());
        assert_eq!(
            parsed.tree.reconstruct(&parsed.tokens, &source),
            source,
            "{}",
            path.display()
        );
        assert_eq!(
            skim(source.as_bytes()).bodies,
            skeleton_from_layout(&source, &parsed.tokens),
            "{}",
            path.display()
        );
    }
}

#[test]
fn parse_phase_conformance() {
    let root = repository_root();
    let known_text = fs::read_to_string(root.join("compiler/KNOWN_FAILURES.tsv"))
        .expect("compiler/KNOWN_FAILURES.tsv");
    let known: BTreeMap<&str, &str> = known_text
        .lines()
        .skip(1)
        .filter_map(|row| row.split_once('\t'))
        .collect();
    let mut seen_known = BTreeSet::new();
    let cases = fs::read_to_string(root.join("spec/conformance/cases.tsv")).expect("cases.tsv");
    let mut failures = Vec::new();
    for row in cases.lines().skip(1) {
        let columns: Vec<&str> = row.split('\t').collect();
        if columns.len() < 3 || columns[1] != "parse" {
            continue;
        }
        let source = fs::read(root.join("spec/conformance").join(columns[0]))
            .unwrap_or_else(|error| panic!("read {}: {error}", columns[0]));
        let parsed = parse(&source);
        let actual: Vec<&str> = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();
        if columns[2] == "accept" {
            if !actual.is_empty() {
                if known.get(columns[0]) == Some(&"pending-gadt-removal") {
                    seen_known.insert(columns[0]);
                } else {
                    failures.push(format!("{} expected accept, got {actual:?}", columns[0]));
                }
            }
        } else if let Some(expected) = columns[2].strip_prefix("reject:")
            && !actual.contains(&expected)
        {
            if known.get(columns[0]) == Some(&expected) {
                seen_known.insert(columns[0]);
            } else {
                failures.push(format!(
                    "{} expected {expected}, got {actual:?}",
                    columns[0]
                ));
            }
        }
    }
    for path in known.keys() {
        if !seen_known.contains(path) {
            failures.push(format!("stale known failure: {path}"));
        }
    }
    assert!(failures.is_empty(), "{}\n", failures.join("\n"));
}
