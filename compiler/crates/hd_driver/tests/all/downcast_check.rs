//! A method with explicit type arguments on a trait value
//! (`value.downcast::[T]()`, trait.downcast.some) resolves like the free
//! `downcast_val::[T](value)`, on `dyn Inspectable` and on a trait that
//! extends it, `dyn Error` (trait.downcast.ordinary). The match over the
//! result is exhaustive over `.Some` and `.None` (#178).

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
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
fn downcast_with_type_arguments_on_dyn_inspectable() {
    let src = "use std.inspect.Inspectable

data User:
    name: string

fn name_of(value: dyn Inspectable) -> string:
    match value.downcast::[User]():
        .Some(user) => user.name
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn a_value_type_target_is_one_error_not_a_cascade() {
    let src = "use std.inspect.Inspectable

fn name_of(value: dyn Inspectable) -> string:
    match value.downcast::[string]():
        .Some(user) => user
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn downcast_with_type_arguments_on_dyn_error() {
    let src = "use std.error.Error

data DiskError:
    device: string

impl Display for DiskError:
    fn to_string(self) -> string: \"disk\"

impl Error for DiskError

fn device_of(error: dyn Error) -> string:
    match error.downcast::[DiskError]():
        .Some(found) => found.device
        .None => \"none\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn downcast_mut_with_type_arguments_on_a_bounded_parameter() {
    let src = "use std.inspect.Inspectable

data Counter:
    count: i32

fn bump(value: mut dyn Inspectable) -> void:
    match value.downcast_mut::[Counter]():
        .Some(counter) => counter.count = counter.count + 1
        .None => pass

fn same[T < Inspectable](value: T) -> bool:
    match value.downcast::[Counter]():
        .Some(_) => true
        .None => false

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn an_unknown_method_with_type_arguments_on_a_trait_value_is_one_error() {
    let src = "use std.inspect.Inspectable

fn name_of(value: dyn Inspectable) -> string:
    match value.nothing::[string]():
        .Some(user) => user
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownMethod]);
}
