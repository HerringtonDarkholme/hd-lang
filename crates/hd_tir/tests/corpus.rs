use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use hd_tir::ir::{CHECKED_INVARIANTS, Tag, parse, print, verify};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Ok,
    Reject(u8),
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn expectation(header: &str) -> Expect {
    let value = header
        .strip_prefix("# expect: ")
        .unwrap_or_else(|| panic!("missing expectation header: {header}"));
    if value == "ok" {
        return Expect::Ok;
    }
    let invariant = value
        .strip_prefix("reject ")
        .unwrap_or_else(|| panic!("bad expectation header: {header}"))
        .parse()
        .unwrap_or_else(|_| panic!("bad invariant in header: {header}"));
    Expect::Reject(invariant)
}

#[test]
fn text_corpus_round_trips_and_verifies() {
    let mut paths: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .expect("read corpus")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "tir"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "empty TIR corpus");

    let mut valid_tags = HashSet::new();
    let mut rejected_tags = HashSet::new();
    let mut rejected_invariants = BTreeSet::new();
    for path in paths {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let (header, body_text) = text
            .split_once('\n')
            .unwrap_or_else(|| panic!("{} has no body", path.display()));
        let expected = expectation(header);
        let body = parse(body_text)
            .unwrap_or_else(|error| panic!("{}:{}: {}", path.display(), error.line, error.what));
        let printed = print(&body);
        let reparsed = parse(&printed)
            .unwrap_or_else(|error| panic!("{}:{}: {}", path.display(), error.line, error.what));
        assert_eq!(print(&reparsed), printed, "{}", path.display());

        let errors = verify(&body);
        match expected {
            Expect::Ok => {
                assert!(errors.is_empty(), "{}: {errors:?}", path.display());
                valid_tags.extend(body.tags.iter().copied());
            }
            Expect::Reject(invariant) => {
                let actual: BTreeSet<u8> = errors.iter().map(|error| error.invariant).collect();
                assert_eq!(actual, BTreeSet::from([invariant]), "{}", path.display());
                rejected_tags.extend(body.tags.iter().copied());
                rejected_invariants.insert(invariant);
            }
        }
    }

    let missing: Vec<&str> = Tag::ALL
        .iter()
        .filter(|tag| **tag != Tag::Hole && !valid_tags.contains(tag))
        .map(|tag| tag.name())
        .collect();
    assert!(missing.is_empty(), "valid corpus misses tags: {missing:?}");
    assert!(
        rejected_tags.contains(&Tag::Hole),
        "the reserved Hole tag needs its invariant-14 case"
    );
    assert_eq!(
        rejected_invariants,
        CHECKED_INVARIANTS.iter().copied().collect(),
        "the corpus needs one rejection per checked verifier invariant"
    );
}
