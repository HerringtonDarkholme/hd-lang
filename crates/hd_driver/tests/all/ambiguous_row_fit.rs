//! Requirement-key inference with two solutions (spec 04
//! `types.infer.ambiguous`): fitting keys that match two ways is
//! `ambiguous-type`, not a silent first match. A unique fit and explicit
//! type arguments stay clean.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const PRELUDE: &str = "\
trait Repo[T]:
    fn number(self) -> i32

struct User: pass
struct Post: pass

struct Job[A, B]:
    callback: fn() -> i32 $ Repo[A] + Repo[B]

fn read_both() -> i32 $ Repo[User] + Repo[Post]:
    0
";

#[test]
fn two_key_matchings_are_ambiguous() {
    let main = format!(
        "{PRELUDE}\nfn schedule() -> void:\n    _ := Job {{ callback: read_both }}\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes(&main), vec![Code::AmbiguousType]);
}

#[test]
fn unique_key_matching_stays_clean() {
    let main = "trait Repo[T]:\n    fn number(self) -> i32\n\nstruct User: pass\n\nstruct Box[A]:\n    callback: fn() -> i32 $ Repo[A]\n\nfn read_user() -> i32 $ Repo[User]:\n    0\n\nfn schedule() -> void:\n    _ := Box { callback: read_user }\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(main), Vec::<Code>::new());
}

#[test]
fn explicit_arguments_disambiguate() {
    let main = format!(
        "{PRELUDE}\nfn schedule() -> void:\n    _ := Job::[User, Post] {{ callback: read_both }}\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes(&main), Vec::<Code>::new());
}
