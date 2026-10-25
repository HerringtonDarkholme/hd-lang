//! First inferred argument sets the join (spec 04, group Q):
//! conflicting later arguments for one bare type parameter report one
//! `type-mismatch`, while single inferred arguments and literals keep the
//! behavior the other conformance fixtures require.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

const PICK: &str = "fn pick[T](left: T, right: T) -> T:\n    left\n";

#[test]
fn readonly_first_variance_points_at_use() {
    let out = analyze(&format!(
        "{PICK}\nfn first(shared: List[*User], owned: List[*mut User]) -> List[*User]:\n    pick(shared, owned)\n\nstruct User:\n    name: string\n"
    ));
    assert_eq!(codes(&out), vec![Code::TypeMismatch], "{}", out.render());
    assert!(
        out.render()
            .contains("expected List[*User], found List[*mut User]"),
        "{}",
        out.render()
    );
}
