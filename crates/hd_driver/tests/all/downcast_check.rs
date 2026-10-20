//! Reference recovery resolves through Inspectable and Error evidence.

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
fn downcast_with_type_arguments_on_dyn_inspectable() {
    let src = "use std.inspect.Inspectable

struct User:
    name: string

fn name_of(value: &dyn Inspectable) -> string:
    match value.downcast_ref::[User]():
        .Some(user) => user.name
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), Vec::<Code>::new());
}

#[test]
fn a_function_target_is_one_error_not_a_cascade() {
    let src = "use std.inspect.Inspectable

fn name_of(value: &dyn Inspectable) -> string:
    match value.downcast_ref::[fn() -> string]():
        .Some(_) => \"x\"
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn downcast_with_type_arguments_on_dyn_error() {
    let src = "use std.error.Error

struct DiskError:
    device: string

impl Display for DiskError:
    fn to_string(self) -> string: \"disk\"

impl Error for DiskError

fn device_of(error: &dyn Error) -> string:
    match error.downcast_ref::[DiskError]():
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

struct Counter:
    count: i32

fn bump(value: &mut dyn Inspectable) -> void:
    match value.downcast_mut::[Counter]():
        .Some(counter) => counter.count = counter.count + 1
        .None => pass

fn same[T < Inspectable](value: &T) -> bool:
    let erased: &dyn Inspectable = value
    match erased.downcast_ref::[Counter]():
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

fn name_of(value: &dyn Inspectable) -> string:
    match value.nothing::[fn() -> string]():
        .Some(_) => \"x\"
        .None => \"x\"

pub fn main() -> void:
    pass
";
    assert_eq!(codes(src), vec![Code::UnknownMethod]);
}
