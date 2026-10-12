//! A bare trait name in a type written inside a body is
//! `trait-used-as-type` at the name, one error per name and no knock-on
//! errors (spec/lang/04-type-system.md types.trait.value.dyn-required,
//! spec/lang/09-traits.md trait.dyn.keyword). `dyn Trait`, bounds and
//! `impl Trait for` stay valid.

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
fn let_annotation_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: Factory = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_list_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: List[Factory] = []\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_tuple_and_option_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: (Factory, i32) = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_data_argument_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: Box[Factory] = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn closure_parameter_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    f := fn(x: Factory) -> i32: 1\n    _ := f\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn closure_result_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    f := fn(x: i32) -> Factory: panic(\"x\")\n    _ := f\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn explicit_type_argument_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    a := id::[Factory](panic(\"x\"))\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn local_function_parameter_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\ndata Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    fn local(p: Factory) -> void: pass\n    local(panic(\"x\"))\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn dyn_bounds_and_impls_stay_valid() {
    let src = "trait Greet:
    fn greet(self) -> string

data P:
    n: i32

impl Greet for P:
    fn greet(self) -> string: \"hi\"

fn use_it[T < Greet](x: T) -> string: x.greet()

pub fn main() -> void $ Console:
    let x: dyn Greet = P { n: 1 }
    let y: List[dyn Greet] = [x]
    println(use_it(P { n: 2 }))
    _ := y
";
    assert_eq!(codes(src), Vec::<Code>::new());
}
