//! A method that exists but whose impl bound fails for the receiver is
//! `unsatisfied-trait-bound`; a method that does not exist stays
//! `unknown-method`.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn errors(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

#[test]
fn list_sum_on_strings_is_an_unmet_bound() {
    let src = "fn joined(words: List[string]) -> string:
    words.sum()
";
    assert_eq!(errors(src), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn list_sum_on_numbers_is_clean() {
    let src = "fn total(values: List[i32]) -> i32:
    values.sum()
";
    assert_eq!(errors(src), vec![]);
}

#[test]
fn a_missing_method_is_still_unknown() {
    let src = "fn f(values: List[i32]) -> i32:
    values.no_such_method()
";
    assert_eq!(errors(src), vec![Code::UnknownMethod]);
}

#[test]
fn a_user_impl_bound_is_an_unmet_bound() {
    let src = "struct Box[T]:
    value: T

impl[T < Display] Box[T]:
    fn show(self) -> string:
        self.value.to_string()

fn f(b: Box[fn() -> void]) -> string:
    b.show()
";
    assert_eq!(errors(src), vec![Code::UnsatisfiedTraitBound]);
}
