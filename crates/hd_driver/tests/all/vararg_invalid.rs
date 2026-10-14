//! Invalid rest and vararg types (spec 04 `types.tuple.rest.list`,
//! spec 07 `fn.vararg.type.kinds`): a function type's rest element must
//! be a `List`, and a vararg parameter's type must be a `List`, a tuple
//! type, or a type parameter bounded by `Tuple`. Each is one
//! `type-mismatch`, and the valid forms stay clean.

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

#[test]
fn function_type_rest_not_list_is_one_error() {
    let src =
        "fn apply(callback: fn(i32...) -> i32) -> i32: 0\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(src), vec![Code::TypeMismatch]);
}

#[test]
fn vararg_not_collection_is_one_error() {
    let src = "fn count(values...: i32) -> i32: 0\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(src), vec![Code::TypeMismatch]);
}

#[test]
fn valid_rest_and_vararg_forms_stay_clean() {
    let cases = [
        "fn apply(callback: fn(List[i32]...) -> i32) -> i32: 0\n\npub fn main() -> void:\n    pass\n",
        "fn sum(values...: List[i32]) -> i32: 0\n\npub fn main() -> void:\n    pass\n",
        "fn first(values...: (i32, string)) -> i32: 0\n\npub fn main() -> void:\n    pass\n",
        "use std.function.Tuple\n\nfn count[Args < Tuple](values...: Args) -> i32: 0\n\npub fn main() -> void:\n    pass\n",
        "fn grow(values...: mut List[i32]) -> void: pass\n\npub fn main() -> void:\n    pass\n",
    ];
    for main in cases {
        assert_eq!(codes(main), Vec::<Code>::new(), "{main}");
    }
}
