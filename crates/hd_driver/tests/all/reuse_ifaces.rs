//! `Packages::reuse` (#1423): a later build takes each unit interface an
//! earlier build of the caller left, instead of reading and decoding its
//! `iface` entry again, and builds what a build without it builds. A unit
//! whose key moved is read as usual.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, Reuse, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build(main: &str, store: &MemoryStore, goal: &Goal, reuse: Option<&Reuse>) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        reuse,
        ..Packages::default()
    };
    build_packages(&host, "app", &packages, goal)
}

fn program() -> Goal {
    Goal::Program {
        entry: "main".to_owned(),
    }
}

fn iface_reads(out: &Output) -> usize {
    out.counters.hits.get("iface").copied().unwrap_or(0)
        + out.counters.misses.get("iface").copied().unwrap_or(0)
}

const MAIN: &str = include_str!("programs/reuse_ifaces/main.hd");

#[test]
fn a_program_build_takes_the_interfaces_its_check_build_left() {
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    let check = build(MAIN, &store, &Goal::CheckTests, Some(&reuse));
    assert!(!check.diags.has_errors(), "{}", check.render());
    let reused = build(MAIN, &store, &program(), Some(&reuse));
    let fresh = build(MAIN, &store, &program(), None);
    assert!(fresh.wasm.is_some(), "{}", fresh.render());
    assert_eq!(reused.wasm, fresh.wasm);
    assert_eq!(reused.render(), fresh.render());
    assert_eq!(iface_reads(&reused), 0);
    assert!(iface_reads(&fresh) > 0);
    // What the program build decoded is its own count, not the check's.
    assert!(
        reused.counters.iface_records <= fresh.counters.iface_records,
        "{} > {}",
        reused.counters.iface_records,
        fresh.counters.iface_records
    );
}

#[test]
fn a_unit_whose_key_moved_is_read_again() {
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    build(MAIN, &store, &Goal::CheckTests, Some(&reuse));
    let edited = MAIN.replace("-> i32:\n    41", "-> i64:\n    41");
    let reused = build(&edited, &store, &program(), Some(&reuse));
    let fresh = build(&edited, &MemoryStore::default(), &program(), None);
    assert!(fresh.wasm.is_some(), "{}", fresh.render());
    assert_eq!(reused.wasm, fresh.wasm);
    // The edited module's unit alone: std's interfaces are taken.
    assert_eq!(reused.counters.ifaces_built, ["app.main"]);
    assert_eq!(iface_reads(&reused), 1);
}

#[test]
fn a_reused_interface_reports_its_header_findings_again() {
    let text = include_str!("programs/reuse_ifaces/header_findings.hd");
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    let check = build(text, &store, &Goal::CheckTests, Some(&reuse));
    assert!(check.diags.has_errors(), "{}", check.render());
    let reused = build(text, &store, &program(), Some(&reuse));
    let fresh = build(text, &store, &program(), None);
    assert_eq!(iface_reads(&reused), 0);
    assert_eq!(reused.render(), fresh.render());
    assert!(reused.diags.has_errors());
}

/// A new run decodes the anonymous sum from its stored interface and bodies,
/// preserving nested positional identities through checking and collection.
#[test]
fn anonymous_sums_survive_warm_interface_and_body_reads() {
    let text = include_str!("programs/reuse_ifaces/anonymous_sums.hd");
    assert_anonymous_warm_reads(text);
}

/// Fixed, nested and rest products keep their shape through both cache layers.
#[test]
fn anonymous_products_survive_warm_interface_and_body_reads() {
    let text = include_str!("programs/reuse_ifaces/anonymous_products.hd");
    assert_anonymous_warm_reads(text);
}

fn assert_anonymous_warm_reads(text: &str) {
    let store = MemoryStore::default();
    let cold = build(text, &store, &program(), None);
    assert!(cold.wasm.is_some(), "{}", cold.render());
    let warm = build(text, &store, &program(), None);
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert_eq!(warm.wasm, cold.wasm);
    assert_eq!(warm.render(), cold.render());
    assert!(warm.counters.hit("iface") > 0);
    assert_eq!(warm.counters.miss("iface"), 0);
    assert!(warm.counters.bodies_owned.is_empty());
}

/// A facade's export target changes only diagnostic repair metadata: both
/// declarations and the importer stay byte-identical (#2376).
#[test]
fn facade_redirects_refresh_private_import_fixes_in_both_directions() -> Result<(), String> {
    use hd_base::Span;
    use hd_diag::FixSafety;

    fn analyze(text: &str, store: &MemoryStore, order: SerialOrder) -> Result<Output, String> {
        let case = hd_testkit::stages::parse(text)?;
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &case.program.text);
        for (path, text) in &case.program.files {
            sources.insert(path, text);
        }
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store,
            clock: &NoClock,
            executor: Executor::Serial(order),
        };
        Ok(hd_driver::build(&host, "facade_fix_cache", &Goal::Analyze))
    }
    type Fix = (String, FixSafety, Vec<(Span, String)>);
    fn snapshot(out: &Output) -> (String, Vec<Fix>) {
        let d = &out.diags;
        let fixes = d
            .content_order()
            .into_iter()
            .flat_map(|row| {
                let range = d.fixes[row];
                (range.start..range.start + range.len).map(|fix| {
                    let fix = fix as usize;
                    let range = d.fix_edits[fix];
                    let edits = (range.start..range.start + range.len)
                        .map(|edit| {
                            let edit = &d.edits[edit as usize];
                            (edit.span, d.get_text(edit.text).to_owned())
                        })
                        .collect();
                    (
                        d.get_text(d.fix_title[fix]).to_owned(),
                        d.fix_safety[fix],
                        edits,
                    )
                })
            })
            .collect();
        (out.render(), fixes)
    }
    let single = include_str!("programs/reuse_ifaces/facade-fix.hd");
    let grouped = single
        .replace(
            "use pkg.api.hidden.take",
            "use pkg.api.hidden.{take as grab}",
        )
        .replace("    take()", "    grab()");
    let namespace = single
        .replace("use pkg.api.hidden.take", "use pkg.api.hidden")
        .replace("    take()", "    hidden.take()");
    for (before, replacement) in [
        (single.to_owned(), Some("pkg.api.take")),
        (grouped, Some("pkg.api")),
        (namespace, None),
    ] {
        let after = before.replace("pub use self.visible.{take}", "pub use self.hidden.{take}");
        assert_ne!(before, after);
        let cold_before = snapshot(&analyze(
            &before,
            &MemoryStore::default(),
            SerialOrder::Priority,
        )?);
        let cold_after = snapshot(&analyze(
            &after,
            &MemoryStore::default(),
            SerialOrder::Priority,
        )?);
        assert!(cold_before.1.is_empty());
        assert_eq!(cold_after.1.len(), usize::from(replacement.is_some()));
        if let Some(replacement) = replacement {
            let fix = &cold_after.1[0];
            assert_eq!(fix.1, FixSafety::Exact);
            assert_eq!(fix.2.len(), 1);
            assert_eq!(fix.2[0].1, replacement);
        }
        for order in [
            SerialOrder::Priority,
            SerialOrder::Fifo,
            SerialOrder::Shuffled(7),
        ] {
            let store = MemoryStore::default();
            assert_eq!(snapshot(&analyze(&before, &store, order)?), cold_before);
            for (source, expected) in [
                (&before, &cold_before),
                (&after, &cold_after),
                (&before, &cold_before),
                (&after, &cold_after),
            ] {
                let warm = analyze(source, &store, order)?;
                assert_eq!(&snapshot(&warm), expected, "{order:?}");
                assert!(
                    warm.counters
                        .ifaces_built
                        .iter()
                        .all(|module| module.starts_with("facade_fix_cache.")),
                    "{:?}",
                    warm.counters.ifaces_built
                );
            }
            let unchanged = analyze(&after, &store, order)?;
            assert_eq!(snapshot(&unchanged), cold_after);
            assert!(unchanged.counters.ifaces_built.is_empty());
        }
    }
    Ok(())
}
