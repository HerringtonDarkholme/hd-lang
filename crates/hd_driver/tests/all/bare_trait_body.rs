//! A bare trait name in a type written inside a body is
//! `trait-used-as-type` at the name, one error per name and no knock-on
//! errors (spec/lang/04-type-system.md types.trait.value.dyn-required.reference-values,
//! spec/lang/09-traits.md trait.dyn.reference-keyword). `dyn Trait`, bounds and
//! `impl Trait for` stay valid.

use hd_diag::{Code, DiagBuf, FixSafety};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn diags(main: &str) -> DiagBuf {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
    .diags
}

fn codes(main: &str) -> Vec<Code> {
    diags(main)
        .code
        .iter()
        .copied()
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

/// The diagnostic's fix-it inserts `dyn` before the trait name: one
/// Exact fix with one zero-width edit, which applied reads `dyn Factory`.
fn assert_dyn_fix(main: &str, diags: &DiagBuf) {
    let i = diags
        .code
        .iter()
        .position(|c| *c == Code::TraitUsedAsType)
        .expect("a trait-used-as-type diagnostic");
    let fixes = diags.fixes[i];
    assert_eq!(fixes.len, 1, "{main}");
    let title = diags.get_text(diags.fix_title[fixes.start as usize]);
    assert_eq!(title, "insert `&dyn` before the trait name", "{main}");
    assert_eq!(diags.fix_safety[fixes.start as usize], FixSafety::Exact);
    let edits = diags.fix_edits[fixes.start as usize];
    assert_eq!(edits.len, 1, "{main}");
    let edit = &diags.edits[edits.start as usize];
    let (lo, hi) = (edit.span.lo as usize, edit.span.hi as usize);
    assert_eq!(lo, hi, "the edit inserts, not replaces");
    assert!(main[lo..].starts_with("Factory"), "{main}");
    assert_eq!(diags.get_text(edit.text), "&dyn ");
    let repaired = format!("{}&dyn {}", &main[..lo], &main[lo..]);
    assert!(repaired.contains("&dyn Factory"), "{repaired}");
}

#[test]
fn let_annotation_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: Factory = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_list_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: List[Factory] = []\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_tuple_and_option_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: (Factory, i32) = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn nested_in_data_argument_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    let a: Box[Factory] = panic(\"x\")\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn closure_parameter_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    f := fn(x: Factory) -> i32: 1\n    _ := f\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn closure_result_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    f := fn(x: i32) -> Factory: panic(\"x\")\n    _ := f\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn explicit_type_argument_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    a := id::[Factory](panic(\"x\"))\n    _ := a\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn local_function_parameter_is_one_error() {
    let src = "trait Factory:\n    fn create() -> Self\n\nstruct Box[T]:\n    value: T\n\nfn id[T](x: T) -> T: x\n\npub fn main() -> void:\n    fn local(p: Factory) -> void: pass\n    local(panic(\"x\"))\n";
    assert_eq!(codes(src), vec![Code::TraitUsedAsType]);
}

#[test]
fn body_fix_inserts_dyn() {
    let src = "trait Factory:\n    fn create() -> Self\n\npub fn main() -> void:\n    let a: Factory = panic(\"x\")\n    _ := a\n";
    let d = diags(src);
    assert_dyn_fix(src, &d);
}

#[test]
fn signature_fix_inserts_dyn() {
    let src = "trait Factory:\n    fn create() -> Self\n\nfn use_it(x: Factory) -> void: pass\n\npub fn main() -> void:\n    pass\n";
    let d = diags(src);
    assert_eq!(
        d.code
            .iter()
            .copied()
            .filter(|c| *c != Code::Unsupported)
            .collect::<Vec<_>>(),
        vec![Code::TraitUsedAsType]
    );
    assert_dyn_fix(src, &d);
}

#[test]
fn dyn_bounds_and_impls_stay_valid() {
    let src = "trait Greet:
    fn greet(self) -> string

struct P:
    n: i32

impl Greet for P:
    fn greet(self) -> string: \"hi\"

fn use_it[T < Greet](x: T) -> string: x.greet()

pub fn main() -> void $ Console:
    let x: &dyn Greet = &P { n: 1 }
    let y: List[&dyn Greet] = [x]
    println(use_it(P { n: 2 }))
    _ := y
";
    assert_eq!(codes(src), Vec::<Code>::new());
}
