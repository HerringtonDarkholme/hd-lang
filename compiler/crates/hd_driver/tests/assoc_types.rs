//! Associated-type projections (spec 09 "Associated Types", "Associated
//! Type Bindings"): `S::Item` in a generic signature normalizes to each
//! impl's binding at its instances, a bound's binding infers a parameter,
//! and an impl whose binding does not fit a bound is rejected.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

fn errors(src: &str) -> Vec<Code> {
    let out = build_main(src, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// Builds `main` and runs it on the Node host; returns its stdout.
fn run_main(name: &str, src: &str) -> String {
    let out = build_main(
        src,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("assoc-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(path);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).into_owned()
}

const SUPPLIERS: &str = "\
trait Supplier:
    type Item
    fn get(self) -> Self::Item

data Counter:
    start: i32

impl Supplier for Counter:
    type Item = i32
    fn get(self) -> i32:
        self.start

data Name:
    text: string

impl Supplier for Name:
    type Item = string
    fn get(self) -> string:
        self.text

data Opaque:
    id: i32

data Vault:
    inside: Opaque

impl Supplier for Vault:
    type Item = Opaque
    fn get(self) -> Opaque:
        self.inside
";

#[test]
fn a_projection_in_a_generic_signature_takes_each_impls_binding() {
    let stdout = run_main(
        "first-item",
        &format!(
            "{SUPPLIERS}
fn first_item[S < Supplier](s: S) -> S::Item:
    s.get()

fn shown[T < Display, S < Supplier[Item = T]](s: S) -> string:
    let item: S::Item = s.get()
    item.to_string()

fn plus_one[S < Supplier[Item = i32]](s: S) -> i32:
    s.get() + 1

data Feed[I]:
    source: I

impl[T < Display, I < Supplier[Item = T]] Display for Feed[I]:
    fn to_string(self) -> string:
        \"feed of \" + shown(self.source)

fn main() -> void $ Console:
    println(Feed::[Counter] {{ source: Counter {{ start: 5 }} }}.to_string())
    println(Feed::[Name] {{ source: Name {{ text: \"Lin\" }} }})
    let count: i32 = first_item(Counter {{ start: 41 }})
    println(count + 1)
    println(first_item(Name {{ text: \"Ada\" }}))
    println(shown(Counter {{ start: 7 }}))
    println(shown(Name {{ text: \"Grace\" }}))
    println(plus_one(Counter {{ start: 1 }}))
    println(first_item(Vault {{ inside: Opaque {{ id: 9 }} }}).id)
"
        ),
    );
    assert_eq!(stdout, "feed of 5\nfeed of Lin\n42\nAda\n7\nGrace\n2\n9\n");
}

#[test]
fn a_bound_on_the_projection_rejects_an_impl_whose_item_does_not_fit() {
    let codes = errors(&format!(
        "{SUPPLIERS}
fn shown[T < Display, S < Supplier[Item = T]](s: S) -> string:
    s.get().to_string()

fn plus_one[S < Supplier[Item = i32]](s: S) -> i32:
    s.get() + 1

fn main() -> void $ Console:
    println(shown(Vault {{ inside: Opaque {{ id: 1 }} }}))
    println(plus_one(Name {{ text: \"Ada\" }}))
"
    ));
    assert_eq!(
        codes,
        vec![Code::UnsatisfiedTraitBound, Code::UnsatisfiedTraitBound]
    );
}

#[test]
fn a_rigid_projection_has_only_its_bounds() {
    let codes = errors(&format!(
        "{SUPPLIERS}
fn shown[S < Supplier](s: S) -> string:
    s.get().to_string()

fn wrong[S < Supplier](s: S) -> i32:
    s.get()
"
    ));
    assert_eq!(codes, vec![Code::UnknownMethod, Code::TypeMismatch]);
}
