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

struct Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Show](&mut self, h: Field[S, F], value: &F) -> Result[void, never]:
        self.out = self.out + value.show() + \";\"
        .Ok(())

fn shower() -> Shower:
    Shower {{ out: \"\" }}

fn wrapped(start: string) -> Shower:
    s := shower()
    Shower {{ out: start + s.out }}

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let mut w = {walker}
        _ := Structure::walk(&self, &mut w)
        w.out
"
    )
}

/// The template lives in another folder of the package; editing only its
/// helper's body reaches the deriving module's check.
#[test]
fn template_edit_in_another_module_rechecks_the_opt_in() {
    let lib = |walker: &str| {
        show_lib(walker).replace("trait Show:", "pub trait Show:")
            + "
struct Quiet:
    out: string

impl[S] Walker[S] for Quiet:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: &F) -> Result[void, never]:
        .Ok(())
"
    };
    let main = "use pkg.lib.show.Show

@derive(Show)
struct Label:
    text: string

pub fn main() -> void:
    pass
";
    let store = crate::warm_store();
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
