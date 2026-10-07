use std::fs;
use std::path::{Path, PathBuf};

use hd_syntax::{lex, skeleton_from_layout, skim};

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
        if let Ok(text) = std::str::from_utf8(&source) {
            assert_eq!(
                lexed.tokens.reconstruct(text).as_bytes(),
                source,
                "{}",
                path.display()
            );
        }
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
