//! Generated Structure walks specialize only in the release pipeline. A
//! hand-written consumer stays ordinary user code.

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SOURCE: &str = include_str!("programs/structure_specialize/source.hd");

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
        !release_eq.contains("VariantRecord[HdS].holds"),
        "{release_eq}"
    );
    assert!(
        !release_eq.contains("FieldRecord[HdS, HdF].get"),
        "{release_eq}"
    );
    assert!(
        release_eq.contains("[%13 .left]") || release_eq.contains(".left]"),
        "{release_eq}"
    );
    let release_same = own(&release, "derive Same for Shape.same");
    assert!(
        release_same.contains("FieldRecord[HdS, HdF].get"),
        "{release_same}"
    );

    let dev = built(OptLevel::Dev);
    assert!(dev.diags.is_empty(), "{}", dev.render());
    let dev_total = own(&dev, "derive Total for Pair.total");
    assert!(dev_total.contains("structure walk"), "{dev_total}");

    let manual = own(&release, "impl Total for Manual.total");
    assert!(!manual.contains("structure walk"), "{manual}");
}
