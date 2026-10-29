//! Generated Structure walks specialize only in the release pipeline. A
//! hand-written consumer stays ordinary user code.

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = r"
use std.annotation.inline
use std.serde.Serializer
use std.structure.{Facts, Field, Member, Structure, Variant, VariantInfo, Walker}

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

struct NullWriter: pass

impl Serializer for NullWriter:
    type Error = never
    fn null(&mut self) -> Result[void, never]: .Ok(())
    fn bool(&mut self, value: bool) -> Result[void, never]: .Ok(())
    fn int(&mut self, value: i64) -> Result[void, never]: .Ok(())
    fn uint(&mut self, value: u64) -> Result[void, never]: .Ok(())
    fn float(&mut self, value: f64) -> Result[void, never]: .Ok(())
    fn text(&mut self, value: string) -> Result[void, never]: .Ok(())
    fn begin_list(&mut self, len: usize) -> Result[void, never]: .Ok(())
    fn end_list(&mut self) -> Result[void, never]: .Ok(())
    fn begin_map(&mut self, len: usize) -> Result[void, never]: .Ok(())
    fn key(&mut self, key: string) -> Result[void, never]: .Ok(())
    fn end_map(&mut self) -> Result[void, never]: .Ok(())
    fn begin_variant(&mut self, facts: Facts, v: &VariantInfo) -> Result[void, never]: .Ok(())
    fn begin_newtype_variant(&mut self, facts: Facts, v: &VariantInfo) -> Result[void, never]: .Ok(())
    fn begin_tuple_variant(
        &mut self,
        facts: Facts,
        v: &VariantInfo,
        len: usize,
    ) -> Result[void, never]:
        .Ok(())
    fn member(&mut self, member: &Member) -> Result[void, never]: .Ok(())
    fn end_variant(&mut self) -> Result[void, never]: .Ok(())

struct FormatWalker:
    out: *mut NullWriter

impl[S] Walker[S] for FormatWalker:
    type Error = never
    fn data_variant(&mut self, variant: Variant[S]) -> Result[void, never]:
        _ := self.out.begin_data(Facts { items: [] }, &variant.info)?
        self.out.end_data()
    fn variant(&mut self, variant: Variant[S]) -> Result[void, never]: .Ok(())
    fn data_member[F](&mut self, handle: Field[S, F], value: F) -> Result[void, never]: .Ok(())
    fn member[F](&mut self, handle: Field[S, F], value: F) -> Result[void, never]: .Ok(())

trait FormatWalk:
    fn format_walk(self) -> void

impl[T] FormatWalk for T by Structure:
    fn format_walk(self) -> void:
        let mut writer = NullWriter {}
        let mut walker = FormatWalker { out: &mut writer }
        _ := Structure::walk(self, &mut walker)

@derive(Total, Eq, FormatWalk)
struct Pair:
    left: i32
    right: i32

struct Manual:
    value: i32

impl Total for Manual:
    @inline(.Never)
    fn total(self) -> i32:
        self.value

trait Same:
    fn same(self, other: Self) -> bool

impl Same for i32:
    fn same(self, other: i32) -> bool:
        self == other

struct SameWalker[S]:
    other: S
    equal: bool

impl[S] Walker[S] for SameWalker[S]:
    type Error = never
    fn variant(&mut self, variant: Variant[S]) -> Result[void, never]:
        .Ok(())
    fn member[F < Same](&mut self, handle: Field[S, F], value: F) -> Result[void, never]:
        other := handle.get(self.other)
        self.equal = self.equal && value.same(other)
        .Ok(())

impl[T] Same for T by Structure:
    fn same(self, other: T) -> bool:
        let mut walker = SameWalker { other: other, equal: true }
        _ := Structure::walk(self, &mut walker)
        walker.equal

@derive(Same)
enum Shape:
    Circle(radius: i32)
    Square(side: i32)

pub fn main() -> void $ Console:
    println(Pair { left: 20, right: 22 }.total() + Manual { value: 1 }.total())
    Pair { left: 20, right: 22 }.format_walk()
    println(Pair { left: 1, right: 2 } == Pair { left: 1, right: 2 })
    println(Shape.Circle(+1).same(Shape.Square(+2)))
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
fn a_closed_generated_walk_specializes_but_an_enum_get_keeps_its_check() {
    let release = built(OptLevel::Optimized);
    assert!(release.diags.is_empty(), "{}", release.render());
    let release_total = own(&release, "derive Total for Pair.total");
    assert!(!release_total.contains("structure walk"), "{release_total}");
    assert!(
        !release_total.contains("Walker.data_variant"),
        "{release_total}"
    );
    assert!(
        !release_total.contains("Walker.data_member"),
        "{release_total}"
    );
    assert!(!release_total.contains("Walker.member"), "{release_total}");
    let release_format = own(&release, "derive FormatWalk for Pair.format_walk");
    assert!(
        !release_format.contains("Serializer.begin_data"),
        "{release_format}"
    );
    assert!(
        !release_format.contains("Serializer.end_data"),
        "{release_format}"
    );
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
    let release_same = own(&release, "derive Same for Shape.same");
    assert!(
        release_same.contains("FieldRecord [ HdS , HdF ].get"),
        "{release_same}"
    );

    let dev = built(OptLevel::Dev);
    assert!(dev.diags.is_empty(), "{}", dev.render());
    let dev_total = own(&dev, "derive Total for Pair.total");
    assert!(dev_total.contains("structure walk"), "{dev_total}");

    let manual = own(&release, "impl Total for Manual.total");
    assert!(!manual.contains("structure walk"), "{manual}");
}
