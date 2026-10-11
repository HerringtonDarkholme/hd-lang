//! The instantiated-template check at a derivation opt-in (spec 14
//! `annot.template.checked`, `annot.walker.obligation.error`): a member
//! that misses the bound of the walker, describer or source the template
//! passes is one `member-not-derivable` at the opt-in, naming the member,
//! however the template obtains its walker.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(src: &MemorySources, store: &MemoryStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    build_with(&src, &MemoryStore::default())
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

fn messages(out: &Output) -> Vec<String> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.code[i] == Code::MemberNotDerivable)
        .map(|i| out.diags.get_text(out.diags.message[i]).to_owned())
        .collect()
}

/// A `Show` library: a walker whose `member` needs `Show`, and the
/// template, which gets its walker from `WALKER` (an expression).
fn show_lib(walker: &str) -> String {
    format!(
        "use std.structure.{{Structure, Field, Variant, Walker}}

trait Show:
    fn show(self) -> string

impl Show for i64:
    fn show(self) -> string:
        \"$self\"

data Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Show](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + value.show() + \";\"
        .Ok(())

fn shower() -> mut Shower:
    Shower {{ out: \"\" }}

fn wrapped(start: string) -> mut Shower:
    s := shower()
    Shower {{ out: start + s.out }}

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let w: mut Shower = {walker}
        _ := Structure::walk(self, w)
        w.out
"
    )
}

#[test]
fn derive_over_member_without_trait_reports_once_naming_it() {
    let src = "use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Vault:
    secret: Secret
    count: i32

pub fn main() -> void:
    pass
";
    let out = analyze(src);
    assert_eq!(
        codes(&out),
        vec![Code::MemberNotDerivable],
        "{}",
        out.render()
    );
    let msgs = messages(&out);
    assert!(msgs[0].contains("`secret`"), "{msgs:?}");
}

#[test]
fn derive_over_valid_members_is_clean() {
    let src = "use std.ops.Default

@derive(Default)
data Inner:
    value: i32

@derive(Default)
data Vault:
    inner: Inner
    count: i32
    name: string

pub fn main() -> void:
    pass
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new(), "{}", out.render());
}

#[test]
fn a_declared_default_exempts_its_member_from_default() {
    let src = "use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Vault:
    secret: Secret = Secret { value: 7 }
    count: i32

pub fn main() -> void:
    pass
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new(), "{}", out.render());
}

#[test]
fn walker_written_in_place() {
    let src = show_lib("Shower { out: \"\" }")
        + "
@derive(Show)
data Label:
    text: string
    count: i64
";
    let out = analyze(&src);
    assert_eq!(
        codes(&out),
        vec![Code::MemberNotDerivable],
        "{}",
        out.render()
    );
    assert!(messages(&out)[0].contains("`text`"));
}

#[test]
fn walker_built_by_a_helper() {
    // The template names no walker type: checking infers it from the
    // helper's result.
    for walker in ["shower()", "wrapped(\"<\")"] {
        let src = show_lib(walker).replace("let w: mut Shower = ", "let mut w = ")
            + "
@derive(Show)
data Label:
    text: string
    count: i64

@derive(Show)
data Count:
    count: i64
";
        let out = analyze(&src);
        assert_eq!(
            codes(&out),
            vec![Code::MemberNotDerivable],
            "{walker}: {}",
            out.render()
        );
        let msgs = messages(&out);
        assert!(
            msgs[0].contains("`text`") && msgs[0].contains("Label"),
            "{msgs:?}"
        );
    }
}

#[test]
fn enum_payload_members_and_omitted_block_members() {
    let src = show_lib("shower()").replace("let w: mut Shower = ", "let mut w = ")
        + "
@derive(Show)
enum Reply:
    Sent(id: i64)
    Raw(string)

data Cache: pass

data Order:
    id: i64
    cache: Cache = Cache {}

impl Show for Order by Structure:
    cache = pass

data Bad:
    id: i64
    cache: Cache

impl Show for Bad by Structure:
    id = []
";
    let out = analyze(&src);
    let msgs = messages(&out);
    assert_eq!(msgs.len(), 2, "{}", out.render());
    assert!(msgs[0].contains("`Raw._0`"), "{msgs:?}");
    assert!(msgs[1].contains("`cache`"), "{msgs:?}");
}

#[test]
fn generic_target_meets_the_bound_through_its_derived_bound() {
    let src = show_lib("shower()")
        + "
@derive(Show)
data Box[T]:
    value: T
    count: i64
";
    let out = analyze(&src);
    assert_eq!(codes(&out), Vec::<Code>::new(), "{}", out.render());
}

/// The template lives in another folder of the package; editing only its
/// helper's body reaches the deriving module's check.
#[test]
fn template_edit_in_another_module_rechecks_the_opt_in() {
    let lib = |walker: &str| {
        show_lib(walker)
            .replace("trait Show:", "pub trait Show:")
            .replace("let w: mut Shower = ", "let mut w = ")
            + "
data Quiet:
    out: string

impl[S] Walker[S] for Quiet:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        .Ok(())
"
    };
    let main = "use pkg.lib.show.Show

@derive(Show)
data Label:
    text: string

pub fn main() -> void:
    pass
";
    let store = MemoryStore::default();
    let mut src = MemorySources::default();
    src.insert("app/main.hd", main);
    src.insert("lib/show.hd", &lib("Quiet { out: \"\" }"));
    let quiet = build_with(&src, &store);
    assert!(messages(&quiet).is_empty(), "{}", quiet.render());
    // Only a body inside the template changes: the interface does not.
    src.insert("lib/show.hd", &lib("shower()"));
    let strict = build_with(&src, &store);
    assert_eq!(messages(&strict).len(), 1, "{}", strict.render());
}

/// The line (1-based) of the first diagnostic of `code`.
fn line_of(out: &Output, src: &str, code: Code) -> Option<usize> {
    let i = out
        .diags
        .content_order()
        .into_iter()
        .find(|&i| out.diags.code[i] == code)?;
    let lo = out.diags.primary[i].lo as usize;
    Some(src[..lo].matches('\n').count() + 1)
}

#[test]
fn derived_hash_field_without_hash_is_reported_at_the_field() {
    let src = "data Opaque: pass

@derive(Eq, Hash)
data Key:
    value: Opaque
";
    let out = analyze(src);
    // The field misses Eq and Hash: one error at the field.
    assert_eq!(
        codes(&out),
        vec![Code::DeriveFieldMissingTrait],
        "{}",
        out.render()
    );
    assert_eq!(line_of(&out, src, Code::DeriveFieldMissingTrait), Some(5));
}

#[test]
fn derived_total_order_over_a_float_is_reported_at_the_field() {
    let src = "@derive(Eq, PartialOrd, Ord)
data Measurement:
    value: f64
";
    let out = analyze(src);
    assert_eq!(
        codes(&out),
        vec![Code::DeriveFieldMissingTrait],
        "{}",
        out.render()
    );
    assert_eq!(line_of(&out, src, Code::DeriveFieldMissingTrait), Some(3));
}

#[test]
fn derived_eq_enum_payload_without_eq_is_reported_at_the_payload() {
    let src = "data Socket:
    port: i32

@derive(Eq)
enum Endpoint:
    Local(path: string)
    Remote(socket: Socket)
";
    let out = analyze(src);
    assert_eq!(
        codes(&out),
        vec![Code::DeriveFieldMissingTrait],
        "{}",
        out.render()
    );
    assert_eq!(line_of(&out, src, Code::DeriveFieldMissingTrait), Some(7));
}

#[test]
fn derived_newtype_base_without_the_trait_is_reported_at_the_base() {
    let src = "data Opaque: pass

@derive(Eq)
type Wrapped(Opaque)
";
    let out = analyze(src);
    assert_eq!(
        codes(&out),
        vec![Code::DeriveFieldMissingTrait],
        "{}",
        out.render()
    );
    assert_eq!(line_of(&out, src, Code::DeriveFieldMissingTrait), Some(4));
}

#[test]
fn derived_comparison_over_generic_and_hand_written_members_is_clean() {
    let src = "@derive(Eq)
data Box[T]:
    value: T

data Token:
    id: i32

impl Eq for Token:
    fn eq(self, other: Token) -> bool:
        self.id == other.id

@derive(Eq)
data Holder:
    token: Token

@derive(Eq, Hash)
type Mile(i32)
";
    let out = analyze(src);
    assert_eq!(codes(&out), Vec::<Code>::new(), "{}", out.render());
}

#[test]
fn derive_of_other_traits_still_reports_member_not_derivable() {
    let src = "use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Vault:
    secret: Secret
";
    let out = analyze(src);
    assert_eq!(
        codes(&out),
        vec![Code::MemberNotDerivable],
        "{}",
        out.render()
    );
}
