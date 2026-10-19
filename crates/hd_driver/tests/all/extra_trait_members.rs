//! A trait implementation holds only members its trait declares (spec 09
//! `trait.impl.extra-methods`): an extra method or associated type is
//! `extra-trait-member`, reported once and left out of the implementation.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str) -> Output {
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
    build(&host, "app", &Goal::Analyze)
}

fn errors(src: &str) -> Vec<Code> {
    let out = build_main(src);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const SHAPE: &str = "trait Shape:\n    fn area(self) -> f64\n\nstruct Square:\n    side: f64\n\n";

#[test]
fn an_extra_method_is_reported_once() {
    let bad = format!(
        "{SHAPE}impl Shape for Square:\n    fn area(self) -> f64: self.side\n    fn extra(self) -> f64: 0.0\n"
    );
    assert_eq!(errors(&bad), vec![Code::ExtraTraitMember]);
}

#[test]
fn a_call_to_an_extra_method_is_an_unknown_method() {
    let bad = format!(
        "{SHAPE}impl Shape for Square:\n    fn area(self) -> f64: self.side\n    fn extra(self) -> f64: 0.0\n\nfn use_it(s: Square) -> f64:\n    s.extra()\n"
    );
    assert_eq!(
        errors(&bad),
        vec![Code::ExtraTraitMember, Code::UnknownMethod]
    );
}

#[test]
fn an_extra_associated_type_is_reported_once() {
    let bad = "trait Named:\n    fn name(self) -> string\n\nstruct Admin: pass\n\nimpl Named for Admin:\n    type Key = i32\n    fn name(self) -> string: \"a\"\n";
    assert_eq!(errors(bad), vec![Code::ExtraTraitMember]);
}

#[test]
fn a_supertrait_method_in_a_subtrait_impl_is_extra() {
    let bad = "trait Parent:\n    fn up(self) -> i32\n\ntrait Child < Parent:\n    fn down(self) -> i32\n\nstruct D: pass\n\nimpl Parent for D:\n    fn up(self) -> i32: 1\n\nimpl Child for D:\n    fn up(self) -> i32: 2\n    fn down(self) -> i32: 3\n";
    assert_eq!(errors(bad), vec![Code::ExtraTraitMember]);
}

#[test]
fn a_default_body_naming_an_implementor_field_is_an_unknown_method() {
    // Fields of an implementing type are not accessible in a default
    // body, even when every implementor declares them (spec 09
    // `trait.default.no-self-members`).
    let bad = "trait Greeter:\n    fn name(self) -> string\n\n    fn tag(self) -> string:\n        self.email\n\nstruct User:\n    email: string\n\nimpl Greeter for User:\n    fn name(self) -> string: self.email\n";
    assert_eq!(errors(bad), vec![Code::UnknownMethod]);
}

#[test]
fn a_default_method_may_be_overridden() {
    let good = "trait Greet:\n    fn name(self) -> string\n    fn hello(self) -> string: self.name()\n\nstruct P: pass\n\nimpl Greet for P:\n    fn name(self) -> string: \"p\"\n    fn hello(self) -> string: \"hi\"\n";
    assert_eq!(errors(good), vec![]);
}

#[test]
fn an_inherent_method_beside_the_trait_impl_is_valid() {
    let good = format!(
        "{SHAPE}impl Shape for Square:\n    fn area(self) -> f64: self.side\n\nimpl Square:\n    fn extra(self) -> f64: 0.0\n"
    );
    assert_eq!(errors(&good), vec![]);
}

#[test]
fn an_error_impl_cannot_add_chain() {
    let bad = "use std.error.Error\n\nenum E:\n    Bad\n\nimpl Display for E:\n    fn to_string(self) -> string: \"bad\"\n\nimpl Error for E:\n    fn chain(self) -> string: \"x\"\n";
    assert_eq!(errors(bad), vec![Code::ExtraTraitMember]);
}

#[test]
fn an_impl_cannot_override_an_inherent_dyn_method() {
    let bad = "trait Shape:\n    fn area(self) -> f64\n\nimpl dyn Shape:\n    fn double(self) -> f64:\n        self.area() * 2.0\n\nstruct Square:\n    side: f64\n\nimpl Shape for Square:\n    fn area(self) -> f64: self.side\n    fn double(self) -> f64: 0.0\n";
    assert_eq!(errors(bad), vec![Code::ExtraTraitMember]);
}
