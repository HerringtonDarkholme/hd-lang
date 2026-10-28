//! Generated Structure walks specialize only in the release pipeline. A
//! hand-written consumer stays ordinary user code.

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = r"
use std.annotation.inline
use std.structure.{Field, Structure, Variant, Walker}

trait Add:
    fn add(self, total: *mut i32) -> void

impl Add for i32:
    fn add(self, total: *mut i32) -> void:
        *total += self

trait Total:
    fn total(self) -> i32

struct Totaller:
    out: *mut i32

impl[S] Walker[S] for Totaller:
    type Error = never
    fn variant(&mut self, variant: Variant[S]) -> Result[void, never]:
        .Ok(())
    fn member[F < Add](&mut self, handle: Field[S, F], value: F) -> Result[void, never]:
        value.add(self.out)
        .Ok(())

impl[T] Total for T by Structure:
    fn total(self) -> i32:
        out := &mut 0i32
        let mut walker = Totaller { out: out }
        _ := Structure::walk(self, &mut walker)
        *out

@derive(Total, Eq)
struct Pair:
    left: i32
    right: i32

struct Manual:
    value: i32

impl Total for Manual:
    @inline(.Never)
    fn total(self) -> i32:
        self.value

pub fn main() -> void $ Console:
    println(Pair { left: 20, right: 22 }.total() + Manual { value: 1 }.total())
    println(Pair { left: 1, right: 2 } == Pair { left: 1, right: 2 })
";

fn built(level: OptLevel) -> hd_driver::Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", SOURCE);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &["app/*"],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        build_profile: BuildProfile::Release,
        level,
        ..Packages::default()
    };
    build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn own<'a>(output: &'a hd_driver::Output, path: &str) -> &'a str {
    match output
        .own_text
        .iter()
        .find(|(candidate, _)| candidate.contains(path))
    {
        Some((_, body)) => body.text.as_str(),
        None => panic!("no {path} body; have {:?}", output.own_text.keys()),
    }
}

#[test]
fn a_closed_generated_walk_specializes_and_dev_keeps_the_generic_call() {
    let release = built(OptLevel::Optimized);
    assert!(release.diags.is_empty(), "{}", release.render());
    let release_total = own(&release, "derive Total for Pair.total");
    assert!(!release_total.contains("structure walk"), "{release_total}");
    assert!(!release_total.contains("Walker.member"), "{release_total}");
    let release_eq = own(&release, "derive Eq for Pair.eq");
    assert!(
        !release_eq.contains("VariantRecord [ HdS ].holds"),
        "{release_eq}"
    );
    assert!(
        !release_eq.contains("FieldRecord [ HdS , HdF ].get"),
        "{release_eq}"
    );
    assert!(
        release_eq.contains("[%13 .left]") || release_eq.contains(".left]"),
        "{release_eq}"
    );

    let dev = built(OptLevel::Dev);
    assert!(dev.diags.is_empty(), "{}", dev.render());
    let dev_total = own(&dev, "derive Total for Pair.total");
    assert!(dev_total.contains("structure walk"), "{dev_total}");

    let manual = own(&release, "impl Total for Manual.total");
    assert!(!manual.contains("structure walk"), "{manual}");
}
