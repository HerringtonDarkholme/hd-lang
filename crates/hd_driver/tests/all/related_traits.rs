//! The related-trait rules of derivation (spec 09 `trait.derive.related.*`):
//! `Hash` needs `Eq`, `PartialOrd` needs `Eq`, and `Ord` needs `Eq` and
//! `PartialOrd`, all in the same `@derive` list, and a derived related trait never sits beside a
//! hand-written one. Each `@derive` line that breaks a rule is one
//! `mixed-derived-law`.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(src: &MemorySources, store: &MemoryStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    build_with(&src, &MemoryStore::default())
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const HAND_EQ: &str = "
impl Eq for Key:
    fn eq(self, other: Key) -> bool:
        self.name == other.name
";

const HAND_HASH: &str = "
impl Hash for Key:
    fn hash(self, state: mut dyn Hasher) -> void:
        pass
";

#[test]
fn derived_hash_without_eq_is_one_error() {
    let src = "
@derive(Hash)
data Key:
    name: string
";
    let out = analyze(src);
    assert_eq!(codes(&out), vec![Code::MixedDerivedLaw]);
}

#[test]
fn derived_eq_beside_hand_written_hash_is_one_error() {
    let src = "
@derive(Eq)
data Key:
    name: string
"
    .to_owned()
        + HAND_HASH;
    let out = analyze(&src);
    assert_eq!(codes(&out), vec![Code::MixedDerivedLaw]);
}

#[test]
fn derived_hash_beside_hand_written_eq_is_one_error() {
    let src = "
@derive(Hash)
data Key:
    name: string
"
    .to_owned()
        + HAND_EQ;
    let out = analyze(&src);
    assert_eq!(codes(&out), vec![Code::MixedDerivedLaw]);
}

#[test]
fn derived_ord_missing_partial_ord_is_one_error() {
    let src = "
@derive(Eq, Ord)
data Key:
    name: string
";
    let out = analyze(src);
    let laws = codes(&out)
        .into_iter()
        .filter(|c| *c == Code::MixedDerivedLaw)
        .count();
    assert_eq!(laws, 1);
}

#[test]
fn derived_eq_and_hash_together_is_clean() {
    let src = "
@derive(Eq, Hash)
data Key:
    name: string
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new());
}

#[test]
fn derived_eq_partial_ord_and_ord_together_is_clean() {
    let src = "
@derive(Eq, PartialOrd, Ord)
data Key:
    name: string
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new());
}

#[test]
fn hand_written_eq_and_hash_with_no_derive_is_clean() {
    let src = "
data Key:
    name: string
"
    .to_owned()
        + HAND_EQ
        + HAND_HASH;
    let out = analyze(&src);
    assert_eq!(codes(&out), Vec::<Code>::new());
}

#[test]
fn derived_eq_alone_is_clean() {
    let src = "
@derive(Eq)
data Key:
    name: string
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new());
}

#[test]
fn derived_eq_beside_hand_written_display_is_clean() {
    let src = "
@derive(Eq)
data Key:
    name: string

impl Display for Key:
    fn to_string(self) -> string:
        self.name
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new());
}
