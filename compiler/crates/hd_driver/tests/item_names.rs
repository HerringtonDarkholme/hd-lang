//! Diagnostics name items as the user writes them (`Eq`), not by their
//! stable internal path (`std/cmp/Eq`).

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn message(out: &Output, code: Code) -> String {
    let found = out
        .diags
        .content_order()
        .into_iter()
        .find(|&i| out.diags.code[i] == code);
    let Some(i) = found else {
        panic!("no {code:?} in {}", out.render());
    };
    out.diags.get_text(out.diags.message[i]).to_owned()
}

#[test]
fn a_derive_message_names_the_trait_plainly() {
    let out = analyze(
        "data Opaque: pass

@derive(Eq)
data Key:
    value: Opaque
",
    );
    let text = message(&out, Code::DeriveFieldMissingTrait);
    assert!(text.contains("Eq"), "{text}");
    assert!(!text.contains('/'), "{text}");
}

#[test]
fn an_unsatisfied_bound_names_the_trait_plainly() {
    let out = analyze(
        "data Opaque: pass

fn same[T < Eq](a: T, b: T) -> bool:
    a == b

pub fn main():
    same(Opaque {}, Opaque {})
",
    );
    let text = message(&out, Code::UnsatisfiedTraitBound);
    assert!(text.ends_with("does not implement Eq"), "{text}");
}
