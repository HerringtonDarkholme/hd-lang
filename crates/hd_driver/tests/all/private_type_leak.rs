//! `module.vis.signature` and `module.vis.signature.coverage`: a public
//! declaration's written signature must not reach a module-private
//! declaration. The check covers parameters, results, public and embedded
//! fields, enum payloads and shared data, alias and newtype targets, bounds,
//! supertraits, associated types, generic arguments and defaults, and every
//! requirement-row key. Each position reports once, at the type that
//! reaches the private declaration, and nothing else. A private
//! declaration in a body, in a private field, or in a private declaration's
//! signature is no leak.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A private data type and a private trait.
const PRELUDE: &str = "struct Hidden:\n    n: i32\n\ntrait Tag:\n    fn tag(self) -> i32\n\n";
/// The lines `PRELUDE` takes, so a position counts from the first line of
/// the body under test.
const PRELUDE_LINES: usize = 6;

fn analyze(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
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

/// The `(line, column)` of each error in `body` (after `PRELUDE`), in
/// source order, once every error is a `private-type-leak`.
#[track_caller]
fn leaks(body: &str) -> Vec<(usize, usize)> {
    let out = analyze(&[("main.hd", &format!("{PRELUDE}{body}"))]);
    let mut at = Vec::new();
    for i in out.diags.content_order() {
        assert_eq!(out.diags.code[i], Code::PrivateTypeLeak, "{}", out.render());
        let (_, line, column) = out.locate(out.diags.primary[i]);
        at.push((line - PRELUDE_LINES, column));
    }
    at
}

#[track_caller]
fn accepts(body: &str) {
    let out = analyze(&[("main.hd", &format!("{PRELUDE}{body}"))]);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

#[test]
fn a_parameter_is_a_position() {
    assert_eq!(leaks("pub fn take(h: Hidden) -> i32: h.n\n"), [(1, 16)]);
}

#[test]
fn a_result_is_a_position() {
    assert_eq!(
        leaks("pub fn make() -> Hidden:\n    Hidden { n: 1 }\n"),
        [(1, 18)]
    );
}

#[test]
fn each_position_reports_once() {
    // Two parameters and the result, each reaching `Hidden`, twice in one
    // of them: three errors, none for the second `Hidden` of a position.
    assert_eq!(
        leaks("pub fn mix(a: Hidden, b: (Hidden, Hidden)) -> Hidden:\n    Hidden { n: 1 }\n"),
        [(1, 15), (1, 26), (1, 47)]
    );
}

#[test]
fn a_public_field_is_a_position() {
    assert_eq!(
        leaks("pub struct Shown:\n    pub h: Hidden\n    n: i32\n"),
        [(2, 12)]
    );
}

#[test]
fn an_embedded_field_is_always_public() {
    assert_eq!(
        leaks("pub struct Shown:\n    Hidden\n    title: string\n"),
        [(2, 5)]
    );
}

#[test]
fn a_private_field_of_a_public_type_is_no_leak() {
    accepts("pub struct Shown:\n    pub n: i32\n    h: Hidden\n");
}

#[test]
fn an_enum_payload_is_a_position() {
    assert_eq!(
        leaks("pub enum Shown:\n    Plain\n    Boxed(h: Hidden)\n"),
        [(3, 14)]
    );
}

#[test]
fn enum_shared_data_is_a_position() {
    assert_eq!(
        leaks("pub enum Shown(h: Hidden):\n    A -> Shown(Hidden { n: 1 })\n"),
        [(1, 19)]
    );
}

#[test]
fn an_alias_target_is_a_position() {
    assert_eq!(leaks("pub type Shown = Hidden\n"), [(1, 18)]);
}

#[test]
fn a_newtype_base_is_a_position() {
    assert_eq!(leaks("pub type Shown(Hidden)\n"), [(1, 16)]);
}

#[test]
fn a_bound_is_a_position() {
    assert_eq!(
        leaks("pub fn run[T < Tag](t: T) -> i32: t.tag()\n"),
        [(1, 1)]
    );
}

#[test]
fn a_bound_of_a_public_type_is_a_position() {
    assert_eq!(
        leaks("pub struct Shown[T < Tag]:\n    pub x: T\n"),
        [(1, 1)]
    );
}

#[test]
fn a_supertrait_is_a_position() {
    assert_eq!(
        leaks("pub trait Shown < Tag:\n    fn s(self) -> i32\n"),
        [(1, 1)]
    );
}

#[test]
fn a_generic_default_is_a_position() {
    assert_eq!(
        leaks("pub struct Shown[T = Hidden]:\n    pub x: T\n"),
        [(1, 1)]
    );
}

#[test]
fn a_generic_argument_is_a_position() {
    assert_eq!(
        leaks("pub fn count(xs: List[Hidden]) -> i32: 0\n"),
        [(1, 18)]
    );
}

#[test]
fn nested_forms_report_at_their_position() {
    for (sig, column) in [
        ("pub fn f(g: fn(Hidden) -> i32) -> i32: 0\n", 13),
        ("pub fn f(g: fn() -> Hidden) -> i32: 0\n", 13),
        ("pub fn f(x: Hidden?) -> i32: 0\n", 13),
        ("pub fn f() -> List[Hidden]:\n    []\n", 15),
        ("pub fn f(p: (i32, Hidden)) -> i32: 0\n", 13),
        ("pub fn f(m: Map[string, List[Hidden]]) -> i32: 0\n", 13),
        ("pub fn f(xs: &mut List[Hidden]) -> i32: 0\n", 14),
    ] {
        assert_eq!(leaks(sig), [(1, column)], "{sig}");
    }
}

#[test]
fn a_requirement_key_is_a_position() {
    assert_eq!(leaks("pub fn run() -> void $ Tag:\n    pass\n"), [(1, 1)]);
}

#[test]
fn a_requirement_key_of_a_function_type_is_a_position() {
    assert_eq!(
        leaks("pub fn run(f: fn() -> void $ Tag) -> void:\n    pass\n"),
        [(1, 15)]
    );
}

#[test]
fn a_public_trait_method_is_a_position() {
    assert_eq!(
        leaks("pub trait Shown:\n    fn get(self) -> Hidden\n"),
        [(2, 21)]
    );
}

#[test]
fn a_public_inherent_method_is_a_position() {
    assert_eq!(
        leaks(
            "pub struct Shown:\n    pub n: i32\n\nimpl Shown:\n    pub fn take(self, h: Hidden) -> i32: h.n\n"
        ),
        [(5, 26)]
    );
}

#[test]
fn a_private_inherent_method_is_no_leak() {
    accepts(
        "pub struct Shown:\n    pub n: i32\n\nimpl Shown:\n    fn take(self, h: Hidden) -> i32: h.n\n",
    );
}

#[test]
fn a_public_method_of_a_private_type_is_no_leak() {
    accepts(
        "struct Inner:\n    n: i32\n\nimpl Inner:\n    pub fn take(self, h: Hidden) -> i32: h.n\n",
    );
}

#[test]
fn a_private_declaration_may_name_private_ones() {
    accepts(
        "struct Wrap:\n    pub h: Hidden\n    Hidden\n\n\
         type Alias = Hidden\n\
         type Base(Hidden)\n\
         trait Sub < Tag:\n    fn sub(self) -> Hidden\n\n\
         fn take(h: Hidden, xs: List[Hidden]) -> Hidden:\n    h\n\n\
         fn bound[T < Tag](t: T) -> i32: t.tag()\n\n\
         fn row() -> void $ Tag:\n    pass\n",
    );
}

#[test]
fn a_private_item_in_a_public_body_is_no_leak() {
    accepts(
        "pub fn size() -> i32:\n    let h: Hidden = Hidden { n: 1 }\n    h.n\n\n\
         pub fn run[T](t: T) -> i32:\n    fn local(h: Hidden) -> i32: h.n\n    local(Hidden { n: 2 })\n",
    );
}

#[test]
fn public_types_in_public_signatures_are_no_leak() {
    accepts(
        "pub struct Open:\n    pub n: i32\n\n\
         pub trait Shape:\n    fn area(self) -> i32\n\n\
         pub type Names = List[string]\n\
         type Hidden2 = Open\n\
         pub fn take(o: Open, xs: List[Open], t: fn(Open) -> Open?) -> Open: o\n\
         pub fn bound[T < Shape](t: T) -> Hidden2:\n    Open { n: t.area() }\n",
    );
}

#[test]
fn a_private_alias_of_a_public_type_is_no_leak() {
    accepts(
        "pub struct Open:\n    pub n: i32\n\ntype Alias = Open\n\npub fn take(o: Alias) -> Alias: o\n",
    );
}

#[test]
fn a_trait_implementation_is_no_leak() {
    accepts(
        "pub struct Open:\n    pub n: i32\n\nimpl Tag for Open:\n    fn tag(self) -> i32: self.n\n",
    );
}

#[test]
fn a_leak_in_a_library_module_is_reported() {
    let out = analyze(&[
        (
            "main.hd",
            "use pkg.shop.{Open}\n\npub fn main() -> void:\n    pass\n",
        ),
        (
            "shop.hd",
            "struct Secret:\n    n: i32\n\npub struct Open:\n    pub s: Secret\n",
        ),
    ]);
    let codes: Vec<Code> = out.diags.code.clone();
    assert_eq!(codes, [Code::PrivateTypeLeak], "{}", out.render());
    let (file, line, column) = out.locate(out.diags.primary[0]);
    assert_eq!((file.as_str(), line, column), ("shop.hd", 5, 12));
}

#[test]
fn a_public_use_of_a_public_type_in_another_module_is_no_leak() {
    let out = analyze(&[
        (
            "main.hd",
            "use pkg.shop.{Open}\n\npub fn wrap(o: Open) -> Open: o\n\npub fn main() -> void:\n    pass\n",
        ),
        ("shop.hd", "pub struct Open:\n    pub n: i32\n"),
    ]);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

const MAKE: &str = "pub trait Make:\n    type Out\n    fn make(self) -> Self::Out\n\npub struct Maker:\n    k: i32\n\n";

#[test]
fn a_public_impls_associated_type_binding_is_a_position() {
    let body = format!(
        "{MAKE}impl Make for Maker:\n    type Out = List[Hidden]\n    fn make(self) -> List[Hidden]: []\n"
    );
    assert_eq!(leaks(&body), [(9, 16)]);
}

#[test]
fn an_impl_with_a_private_target_may_bind_a_private_type() {
    accepts(&format!(
        "{MAKE}struct Inner:\n    k: i32\n\nimpl Make for Inner:\n    type Out = Hidden\n    fn make(self) -> Hidden: Hidden {{ n: self.k }}\n"
    ));
}
