use hd_base::HashSet;
use hd_base::SliceSort as _;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use hd_base::DefId;
use hd_tir::ir::{
    CHECKED_INVARIANTS, Callee, Tag, VerifyCx, parse, print, verify, verify_in, verify_owned,
};
use hd_types::Ty;

/// The checker's facts the rejection cases assume: type 77 is an enum of
/// three variants, builtin impl 3 answers trait 50, method 60 has two
/// bounds, and item 70's row has one key.
struct CorpusFacts;

impl VerifyCx for CorpusFacts {
    fn variant_count(&self, ty: Ty) -> Option<u32> {
        (ty == Ty(77)).then_some(3)
    }
    fn builtin_trait(&self, index: u32) -> Option<DefId> {
        (index == 3).then_some(DefId::from_raw(50))
    }
    fn method_bounds(&self, method: DefId) -> Option<u32> {
        (method == DefId::from_raw(60)).then_some(2)
    }
    fn row_keys(&self, callee: &Callee) -> Option<u32> {
        matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(70)).then_some(1)
    }
}

/// The owned cases' facts: type 78 has a counted leaf, and item 8 has
/// one non-suspending owned ABI parameter; item 9 has a cold parameter,
/// and item 10 a cold requirement row. Other types have no counted
/// leaf; ownership balance is exercised in the unit tests. Item 7
/// constructs a borrowed-buffer string result with an explicit count;
/// item 11 is the map-probe intrinsic.
struct OwnershipFacts;

impl hd_tir::own::OwnCx for OwnershipFacts {
    fn is_map_probe(&self, callee: &Callee) -> bool {
        matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(11))
    }
    fn retains_result(&self, callee: &Callee) -> bool {
        matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(7))
    }
    fn parameter_count(&self, item: DefId) -> Option<usize> {
        match item.raw() {
            8 | 9 => Some(1),
            10 => Some(0),
            _ => None,
        }
    }
    fn has_requirement_row(&self, item: DefId) -> bool {
        item == DefId::from_raw(10)
    }
    fn suspending(&self, item: DefId) -> bool {
        matches!(item.raw(), 9 | 10)
    }
    fn counted(&self, ty: Ty) -> bool {
        ty == Ty(78)
    }
    fn pass(&self, _: &Callee, _: usize, _: usize) -> hd_tir::own::Pass {
        hd_tir::own::Pass::Lent
    }
    fn pass_value(&self, _: Ty, _: usize, _: usize) -> hd_tir::own::Pass {
        hd_tir::own::Pass::Lent
    }
    fn may_write(&self, _: &Callee) -> bool {
        true
    }
    fn parts(&self, _: Ty, _: Option<u32>, out: &mut Vec<Ty>) -> bool {
        out.clear();
        false
    }
    fn received(&self, _: hd_base::DefId, _: usize, _: usize) -> hd_tir::own::Pass {
        hd_tir::own::Pass::Owned
    }
    fn holds_coll(&self, _: Ty) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Ok,
    /// A body after the ownership passes (`verify_owned`).
    Owned,
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
    if value == "owned" {
        return Expect::Owned;
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
    paths.sort_stable();
    assert!(!paths.is_empty(), "empty TIR corpus");

    let mut valid_tags = HashSet::default();
    let mut rejected_tags = HashSet::default();
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
            Expect::Owned => {
                let errors = verify_owned(&body, &OwnershipFacts);
                assert!(errors.is_empty(), "{}: {errors:?}", path.display());
                valid_tags.extend(body.tags.iter().copied());
            }
            Expect::Reject(invariant) => {
                // A rejection case may need the checker's facts; invariant
                // 23 holds after the ownership passes.
                let errors = if invariant == 23 {
                    verify_owned(&body, &OwnershipFacts)
                } else {
                    verify_in(&body, &CorpusFacts)
                };
                let actual: BTreeSet<u8> = errors.iter().map(|error| error.invariant).collect();
                assert_eq!(actual, BTreeSet::from([invariant]), "{}", path.display());
                rejected_tags.extend(body.tags.iter().copied());
                rejected_invariants.insert(invariant);
            }
        }
    }

    let missing: Vec<&str> = Tag::ALL
        .iter()
        .filter(|tag| !reserved(**tag) && !valid_tags.contains(tag))
        .map(|tag| tag.name())
        .collect();
    assert!(missing.is_empty(), "valid corpus misses tags: {missing:?}");
    assert!(
        rejected_tags.contains(&Tag::Hole),
        "the reserved Hole tag needs its invariant-14 case"
    );
    assert!(
        rejected_tags.contains(&Tag::FlagSet) && rejected_tags.contains(&Tag::FlagClear),
        "the tier-2 flag tags need their invariant-23 case"
    );
    assert_eq!(
        rejected_invariants,
        CHECKED_INVARIANTS.iter().copied().collect(),
        "the corpus needs one rejection per checked verifier invariant"
    );
}

/// Tags no valid body holds: a reserved slot (invariant 14), tier 2's
/// borrow flags (invariant 23), and `LoanEnd`, which only the last ownership
/// pass writes.
fn reserved(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Hole | Tag::FlagSet | Tag::FlagClear | Tag::LoanEnd
    )
}
