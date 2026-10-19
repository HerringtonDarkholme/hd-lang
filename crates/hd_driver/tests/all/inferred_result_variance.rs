//! Inferred method results count toward variance (spec 04
//! `types.variance.surface`): a method that omits its result type still
//! reports `invalid-variance` when the inferred result uses a parameter
//! against its derived marker, and valid inferred results stay clean.

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
struct Handler[-T]:
    handle: fn(T) -> void
";

#[test]
fn inferred_result_against_marker_is_one_error() {
    let main = format!(
        "{PRELUDE}\nimpl[T] Handler[T]:\n    fn replay(self, source: fn() -> T):\n        source()\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes(&main), vec![Code::InvalidVariance]);
}

#[test]
fn inferred_result_wrapped_against_marker_is_one_error() {
    let main = format!(
        "{PRELUDE}\nstruct Slot[T]:\n    value: T\n\nimpl[T] Handler[T]:\n    fn boxed(self):\n        Slot {{ value: self.handle }}\n\npub fn main() -> void:\n    pass\n"
    );
    assert_eq!(codes(&main), vec![Code::InvalidVariance]);
}

#[test]
fn inferred_result_with_marker_stays_clean() {
    for main in [
        format!(
            "{PRELUDE}\nimpl[T] Handler[T]:\n    fn get_handle(self):\n        self.handle\n\npub fn main() -> void:\n    pass\n"
        ),
        "struct Box[+T]:\n    value: T\n\nimpl[T] Box[T]:\n    fn get(self):\n        self.value\n\npub fn main() -> void:\n    pass\n"
            .to_owned(),
    ] {
        assert_eq!(codes(&main), Vec::<Code>::new(), "{main}");
    }
}
