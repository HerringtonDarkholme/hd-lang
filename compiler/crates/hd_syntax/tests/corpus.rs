use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use hd_diag::Code;
use hd_syntax::{HeaderKind, SyntaxKind, lex, parse, skeleton_from_layout, skim};

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

/// Conformance cases whose expected diagnostic comes from the parser.
fn parse_rejects() -> BTreeSet<String> {
    let root = repository_root();
    let cases = fs::read_to_string(root.join("spec/conformance/cases.tsv")).expect("cases.tsv");
    cases
        .lines()
        .skip(1)
        .filter_map(|row| {
            let columns: Vec<&str> = row.split('\t').collect();
            let syntax = columns
                .get(2)
                .and_then(|expected| expected.strip_prefix("reject:"))
                .and_then(Code::from_name)
                .is_some_and(Code::is_syntax);
            syntax.then(|| columns[0].to_owned())
        })
        .collect()
}

/// Conformance sources outside the grammar of `02-grammar.md`, reported
/// as fixture conflicts rather than fitted (M2 report).
const SPEC_CONFLICTS: &[&str] = &[
    // `Clear & mut Any`: `trait_bounds` takes `mut` only before the first bound.
    "runtime/valid/mut-bound-value-passed-on.hd",
    // `use tests.common`: `tests` is reserved and no use root
    // (grammar.use.needs-root), yet the case expects `unknown-module`.
    "typing/invalid/integration-tests-root-use.hd",
    // A CLI case whose source is deliberately unparsable (`cli.fmt.syntax-error`).
    "cli/fmt-syntax-error/src/lib.hd",
];

/// Every source the parser must accept: `lib/std`, `examples/`, the
/// samples, the parse-gap repros, and every conformance source but the
/// parse-phase rejects.
fn valid_sources() -> Vec<PathBuf> {
    let root = repository_root();
    let rejects = parse_rejects();
    let mut files = Vec::new();
    hd_files(&root.join("spec/conformance"), &mut files);
    files.retain(|path| {
        let relative = path
            .strip_prefix(root.join("spec/conformance"))
            .expect("conformance path")
            .to_string_lossy()
            .replace('\\', "/");
        !rejects.contains(&relative) && !SPEC_CONFLICTS.contains(&relative.as_str())
    });
    for directory in [
        "lib/std",
        "examples",
        "compiler/samples",
        "compiler/tests/parse-gaps",
    ] {
        hd_files(&root.join(directory), &mut files);
    }
    files.sort();
    files
}

#[test]
fn parse_gap_repros_parse_clean() {
    let mut files = Vec::new();
    hd_files(
        &repository_root().join("compiler/tests/parse-gaps"),
        &mut files,
    );
    assert!(files.len() >= 17, "parse-gap repros missing");
    for path in files {
        let source = fs::read_to_string(&path).expect("read repro");
        let parsed = parse(source.as_bytes());
        assert!(
            parsed.is_ok(),
            "{}: {:?}",
            path.display(),
            parsed.diagnostic_codes()
        );
        assert!(
            !parsed
                .tree
                .root()
                .descendants()
                .any(|node| node.kind() == SyntaxKind::Error),
            "{}",
            path.display()
        );
    }
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
fn valid_sources_parse_clean_and_round_trip() {
    let mut failures = Vec::new();
    let files = valid_sources();
    assert!(files.len() > 2_500, "valid corpus unexpectedly small");
    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read UTF-8 {}: {error}", path.display()));
        let parsed = parse(source.as_bytes());
        assert_eq!(
            parsed.tree.reconstruct(&parsed.tokens, &source),
            source,
            "{}",
            path.display()
        );
        if let Some(diagnostic) = parsed.diagnostics.first() {
            let line = parsed.tokens.line_of(diagnostic.primary.lo) + 1;
            failures.push(format!(
                "{}:{line}: {}",
                path.display(),
                diagnostic.code.as_str()
            ));
        } else if parsed
            .tree
            .root()
            .descendants()
            .any(|node| node.kind() == SyntaxKind::Error)
        {
            failures.push(format!(
                "{}: error node without a diagnostic",
                path.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}\n", failures.join("\n"));
}

/// The declaration skeleton of the full tree: each top-level declaration's
/// kind, header start, and body start.
fn tree_skeleton(source: &str) -> Vec<(HeaderKind, u32, u32)> {
    let parsed = parse(source.as_bytes());
    let mut out = Vec::new();
    for item in parsed.tree.root().children() {
        let kind = match item.kind() {
            SyntaxKind::FnDecl => HeaderKind::Function,
            SyntaxKind::DataDecl => HeaderKind::Data,
            SyntaxKind::EnumDecl => HeaderKind::Enum,
            SyntaxKind::TraitDecl => HeaderKind::Trait,
            SyntaxKind::ImplDecl => HeaderKind::Impl,
            SyntaxKind::TestsBlock => HeaderKind::Tests,
            _ => continue,
        };
        let Some(body) = item.child(SyntaxKind::Block) else {
            continue;
        };
        let first = body.first_token();
        if first.get().is_none() {
            continue;
        }
        let header = item
            .children()
            .find(|child| child.kind() != SyntaxKind::Decorator)
            .map_or(item.first_token(), |_| {
                item.direct_tokens()
                    .find(|&token| !matches!(parsed.tokens.kind(token), hd_syntax::TokenKind::At))
                    .unwrap_or(item.first_token())
            });
        let line = parsed.tokens.line_of(parsed.tokens.span(header).0);
        let header_start = parsed.tokens.line_start[line];
        let body_start = parsed.tokens.span(first).0;
        let body_line = parsed.tokens.line_of(body_start);
        let body_start = if parsed.tokens.is_line_first(first) {
            parsed.tokens.line_start[body_line]
        } else {
            body_start
        };
        out.push((kind, header_start, body_start));
    }
    out
}

#[test]
fn skim_and_full_parse_agree_on_skeletons() {
    let mut failures = Vec::new();
    let mut files = Vec::new();
    let root = repository_root();
    hd_files(&root.join("lib/std"), &mut files);
    hd_files(&root.join("examples"), &mut files);
    for path in files {
        let source = fs::read_to_string(&path).expect("read");
        let skimmed: Vec<(HeaderKind, u32, u32)> = skim(source.as_bytes())
            .bodies
            .iter()
            .filter(|body| {
                body.header_indent == 0
                    && matches!(
                        body.kind,
                        HeaderKind::Function
                            | HeaderKind::Data
                            | HeaderKind::Enum
                            | HeaderKind::Trait
                            | HeaderKind::Impl
                            | HeaderKind::Tests
                    )
            })
            .map(|body| (body.kind, body.header_start, body.body_start))
            .collect();
        let full = tree_skeleton(&source);
        if skimmed != full {
            failures.push(format!(
                "{}: skim {} items, full {} items; first difference {:?}",
                path.display(),
                skimmed.len(),
                full.len(),
                skimmed.iter().zip(&full).find(|(a, b)| a != b)
            ));
        }
    }
    assert!(failures.is_empty(), "{}\n", failures.join("\n"));
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
                if known.contains_key(columns[0]) {
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
