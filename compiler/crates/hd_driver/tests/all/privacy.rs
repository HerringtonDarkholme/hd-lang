//! Member and declaration privacy across modules of one package: a field
//! or inherent method without `pub` is visible only in its own module
//! (`names.visible.field-method`, `names.field-lookup.private`,
//! `names.method-lookup.private`, `module.vis.no-package-private`); another
//! module constructs or copies a type only when all its fields are public
//! (`data.vis.literal`, `data.vis.private-fields`), else through a factory
//! of the type's module (`data.vis.factory`); a module path to a
//! declaration without `pub` is `private-import`
//! (`expr.name.qualified.private`).

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const CART: &str = "\
pub data Cart:
    pub owner: string
    total: i32

impl Cart:
    pub fn open(owner: string, total: i32) -> Cart:
        Cart { owner: owner, total: total }

    pub fn summary(self) -> string:
        \"${self.owner}: ${self.audit()}\"

    fn audit(self) -> string:
        \"${self.total} cents\"

pub fn make(owner: string) -> Cart:
    Cart { owner: owner, total: 0 }

fn squash(text: string) -> string:
    text

pub fn total_of(cart: Cart) -> i32:
    cart.total
";

/// Analyzes the package of `cart.hd` and `main.hd`.
fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("cart.hd", CART);
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

/// The error codes of a run.
fn errors(out: &Output) -> Vec<&str> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| out.diags.code[i].as_str())
        .collect()
}

#[track_caller]
fn rejects(main: &str, code: &str) {
    let out = analyze(main);
    assert_eq!(errors(&out), [code], "{}", out.render());
}

#[track_caller]
fn accepts(main: &str) {
    let out = analyze(main);
    assert!(errors(&out).is_empty(), "{}", out.render());
}

#[test]
fn a_private_field_is_read_in_its_own_module_only() {
    // `cart.hd` reads `total` in `total_of` and `audit`: no error there.
    accepts("use pkg.cart.{Cart}\n\nfn owner(cart: Cart) -> string:\n    cart.owner\n");
    rejects(
        "use pkg.cart.{Cart}\n\nfn cents(cart: Cart) -> i32:\n    cart.total\n",
        "private-member",
    );
}

#[test]
fn a_private_field_is_not_assigned_or_matched_from_another_module() {
    rejects(
        "use pkg.cart.{Cart}\n\nfn reset(cart: mut Cart) -> void:\n    cart.total = 0\n",
        "private-member",
    );
    rejects(
        "use pkg.cart.{Cart}\n\nfn cents(cart: Cart) -> i32:\n    match cart:\n        Cart { total } => total\n",
        "private-member",
    );
}

#[test]
fn another_module_constructs_through_a_factory_only() {
    rejects(
        "use pkg.cart.{Cart}\n\nfn empty(owner: string) -> Cart:\n    Cart { owner: owner, total: 0 }\n",
        "private-member",
    );
    accepts("use pkg.cart.{Cart, make}\n\nfn empty(owner: string) -> Cart:\n    make(owner)\n");
    accepts("use pkg.cart.{Cart}\n\nfn full(owner: string) -> Cart:\n    Cart.open(owner, 100)\n");
}

#[test]
fn a_copy_update_in_another_module_carries_no_private_field() {
    rejects(
        "use pkg.cart.{Cart}\n\nfn rename(cart: Cart, owner: string) -> Cart:\n    Cart { ...cart, owner: owner }\n",
        "private-member",
    );
}

#[test]
fn a_private_method_is_called_in_its_own_module_only() {
    // `summary` calls the private `audit` in `cart.hd`.
    accepts("use pkg.cart.{Cart}\n\nfn show(cart: Cart) -> string:\n    cart.summary()\n");
    rejects(
        "use pkg.cart.{Cart}\n\nfn report(cart: Cart) -> string:\n    cart.audit()\n",
        "private-member",
    );
    rejects(
        "use pkg.cart.{Cart}\n\nfn report(cart: Cart) -> string:\n    cart.missing()\n",
        "unknown-method",
    );
}

#[test]
fn a_module_path_to_a_private_function_is_private_import() {
    rejects(
        "use pkg.cart\n\nfn shout(text: string) -> string:\n    cart.squash(text)\n",
        "private-import",
    );
    rejects(
        "use pkg.cart\n\nfn shout(text: string) -> string:\n    cart.squish(text)\n",
        "unknown-import",
    );
    rejects("use pkg.cart.{squash}\n", "private-import");
}

#[test]
fn a_module_uses_its_own_private_members() {
    let out = analyze(
        "data Entry:\n    label: string\n    cents: i32\n\nimpl Entry:\n    fn describe(self) -> string:\n        \"${self.label}: ${self.cents}\"\n\nfn demo() -> string:\n    entry := Entry { label: \"tea\", cents: 300 }\n    _ := Entry { ...entry, cents: entry.cents + 1 }\n    entry.describe()\n",
    );
    assert!(errors(&out).is_empty(), "{}", out.render());
}
