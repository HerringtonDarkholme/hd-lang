//! Slice 2's exit test (`f1c55020:future-work/compiler/build-order.md` §9) with std as a program's
//! dependency: std's unit interfaces build with no diagnostic, blobs
//! decode to equal interfaces, blob bytes are equal across shuffled serial
//! orders, and a deep-hash edit reaches a dependent's key.

use hd_base::SliceSort as _;
use std::collections::BTreeMap;
use std::sync::Arc;

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build, build_packages};
use hd_intern::{PathTable, ShardedInterner};
use hd_project::MemorySources;
use hd_resolve::{ItemData, Names, decode_items, encode_items};
use hd_sched::SerialOrder;
use hd_types::InternPool;

fn run_with(src: &MemorySources, store: &MemoryStore, order: SerialOrder) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(order),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn run(src: &MemorySources) -> Output {
    run_with(src, &MemoryStore::default(), SerialOrder::Priority)
}

fn hello() -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("main.hd", include_str!("programs/std_interfaces/hello.hd"));
    s
}

fn fresh<T>(f: impl FnOnce(&Names<'_>) -> T) -> T {
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = hd_resolve::KnownItems::new(&paths);
    f(&Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    })
}

#[test]
fn std_interfaces_build_with_no_diagnostic() {
    let out = run(&hello());
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some(), "hello world links");
    for st in [Stage::UnitIface, Stage::HeaderCheck, Stage::Coherence] {
        let t = out.report.tally(st);
        assert_eq!(
            (t.not_implemented, t.blocked),
            (0, 0),
            "{}",
            out.report.render()
        );
    }
    let units: Vec<&str> = out.ifaces.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        units,
        // Only real-use SCCs and fixed implicit providers are built.
        [
            "app.main",
            "std.cmp",
            "std.crypto",
            "std.crypto.sha256",
            "std.encoding.base64",
            "std.encoding.hex",
            "std.encoding.json",
            "std.host",
            "std.http",
            "std.option",
            "std.path",
            "std.process",
            "std.random",
            "std.remote",
            "std.result",
            "std.rt.checked",
            "std.rt.crossing",
            "std.rt.entry",
            "std.testing"
        ]
    );
    assert_eq!(out.report.tally(Stage::UnitIface).ok, 19);
}

#[test]
fn the_prelude_module_reexports_the_prelude_table() {
    let mut sources = MemorySources::default();
    sources.insert(
        "main.hd",
        include_str!("programs/std_interfaces/prelude.hd"),
    );
    let out = run(&sources);
    assert!(out.diags.is_empty(), "{}", out.render());
    // An unused facade no longer joins every program's implicit layer.
    // This program uses it explicitly; find its actual SCC by its exports.
    fresh(|n| {
        let exports: Vec<_> = out
            .ifaces
            .iter()
            .flat_map(|(_, blob)| decode_items(n, blob).expect("decode").1)
            .collect();
        // A row's name is declared in its origin module, or in `std.core`
        // when the origin re-exports it (`module.core.re-exports`).
        let mut got: Vec<(String, String)> = exports
            .iter()
            .filter(|e| e.module == "std.prelude")
            .map(|e| (n.module_of(e.def), n.text(e.name).to_owned()))
            .collect();
        got.sort_stable();
        let mut want: Vec<(String, String)> = hd_resolve::PRELUDE
            .iter()
            .flat_map(|(m, names)| names.iter().map(|x| ((*m).to_owned(), (*x).to_owned())))
            .map(|(m, x)| {
                let core = got.iter().any(|g| g.0 == "std.core" && g.1 == x);
                (if core { "std.core".to_owned() } else { m }, x)
            })
            .collect();
        want.sort_stable();
        assert_eq!(got, want);
    });
}

#[test]
fn std_interfaces_hold_no_poison_type() {
    let out = run(&hello());
    for (unit, blob) in out.ifaces.iter().filter(|f| f.0.starts_with("std")) {
        fresh(|n| {
            let (items, _) = decode_items(n, blob).expect("decode");
            // A real SCC can hold only inherent impls or namespace exports;
            // neither requires a public item export. Check its types directly.
            let mut bad = Vec::new();
            for it in &items {
                let mut tys: Vec<hd_types::Ty> = it
                    .generics
                    .iter()
                    .flat_map(|g| g.bounds.iter().copied().chain(g.default))
                    .collect();
                match &it.data {
                    ItemData::Fn(s, _) | ItemData::Method { sig: s, .. } => {
                        tys.extend(s.params.iter().map(|p| p.1));
                        tys.push(s.ret);
                        for g in &s.generics {
                            tys.extend(g.bounds.iter().copied().chain(g.default));
                        }
                    }
                    ItemData::Struct(fs, _) => tys.extend(fs.iter().map(|f| f.ty)),
                    ItemData::Enum {
                        shared, variants, ..
                    } => tys.extend(
                        shared
                            .iter()
                            .chain(variants.iter().flat_map(|v| &v.fields))
                            .map(|f| f.ty),
                    ),
                    ItemData::Trait(t) => tys.extend(t.supers.iter().copied()),
                    ItemData::Impl { self_ty, assoc, .. } => {
                        tys.push(*self_ty);
                        tys.extend(assoc.iter().map(|a| a.1));
                    }
                    ItemData::AssocType { .. } => {}
                    ItemData::Alias { body, .. }
                    | ItemData::Newtype(body, _)
                    | ItemData::Binding(body) => tys.push(*body),
                    ItemData::Builtin => {}
                }
                if tys.iter().any(|t| n.pool.has_poison(*t)) {
                    bad.push(format!("{} {:?}", n.path(it.def), it.data));
                }
            }
            assert!(
                bad.is_empty(),
                "{unit}: {} items with poison: {:#?}",
                bad.len(),
                &bad[..bad.len().min(10)]
            );
        });
    }
}

#[test]
fn blobs_decode_to_equal_interfaces() {
    let out = run(&hello());
    assert!(!out.ifaces.is_empty());
    for (unit, blob) in &out.ifaces {
        let again = fresh(|n| {
            let (items, exports) = decode_items(n, blob).expect("decode");
            let (items2, exports2) = decode_items(n, blob).expect("decode twice");
            assert_eq!((&items, &exports), (&items2, &exports2), "{unit}");
            encode_items(n, &items, &exports).expect("encode")
        });
        assert_eq!(
            &again[..],
            &blob[..],
            "{unit}: re-encoding changed the bytes"
        );
        let third = fresh(|n| {
            let (items, exports) = decode_items(n, &again).expect("decode");
            encode_items(n, &items, &exports).expect("encode")
        });
        assert_eq!(third, again, "{unit}");
    }
}

#[test]
fn blob_bytes_are_equal_across_shuffled_serial_orders() {
    let src = hello();
    let blobs = |order| -> BTreeMap<String, Arc<[u8]>> {
        run_with(&src, &MemoryStore::default(), order)
            .ifaces
            .into_iter()
            .collect()
    };
    let base = blobs(SerialOrder::Priority);
    assert_eq!(base.len(), 19);
    for order in [
        SerialOrder::Fifo,
        SerialOrder::Shuffled(1),
        SerialOrder::Shuffled(7),
        SerialOrder::Shuffled(0x5eed),
    ] {
        assert_eq!(blobs(order), base, "{order:?}");
    }
}

#[test]
fn a_type_reached_only_through_a_signature_changes_the_dependents_key() {
    let types = |field: &str| format!("pub struct User:\n    pub name: {field}\n");
    let service = include_str!("programs/std_interfaces/service.hd");
    let main = include_str!("programs/std_interfaces/service_main.hd");
    let program = |field: &str| {
        let mut s = MemorySources::default();
        s.insert("main.hd", main);
        s.insert("service/load.hd", service);
        s.insert("user/types.hd", &types(field));
        crate::add_facades(&mut s);
        s
    };
    let store = MemoryStore::default();
    let a = run_with(&program("i32"), &store, SerialOrder::Priority);
    assert!(a.diags.is_empty(), "{}", a.render());
    let b = run_with(&program("i64"), &store, SerialOrder::Priority);
    let deep = |o: &Output, f: &str| o.counters.deep_hashes.get(f).copied();
    assert_ne!(deep(&a, "app.user.types"), deep(&b, "app.user.types"));
    // `service` did not change, but its signature names `User`.
    assert_ne!(deep(&a, "app.service.load"), deep(&b, "app.service.load"));
    // `main` names neither `User` nor its unit.
    assert_ne!(
        a.counters.check_keys.get("app.main"),
        b.counters.check_keys.get("app.main")
    );
    assert!(
        b.counters
            .ifaces_built
            .contains(&"app.service.load".to_owned())
    );
    assert!(
        b.counters
            .ifaces_built
            .iter()
            .all(|m| !m.starts_with("std."))
    );
}

#[test]
fn every_program_shares_stds_check_entries_under_one_profile() {
    let run = |package: &str, profile: Option<Vec<String>>| {
        let mut sources = MemorySources::default();
        sources.insert("main.hd", include_str!("programs/std_interfaces/one.hd"));
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            profile,
            ..Packages::default()
        };
        let goal = Goal::Program {
            entry: "main".to_owned(),
        };
        build_packages(&host, package, &packages, &goal)
    };
    let std_keys = |o: &Output| -> Vec<_> {
        let keys = o.counters.check_keys.iter();
        keys.filter(|(m, _)| m.starts_with("std"))
            .map(|(m, k)| (m.clone(), *k))
            .collect()
    };
    let a = run("app", None);
    let b = run("shop", None);
    let console = run("app", Some(vec!["std.console.Console".to_owned()]));
    assert!(!std_keys(&a).is_empty());
    assert_eq!(std_keys(&a), std_keys(&b));
    assert_ne!(
        a.counters.check_keys["app.main"],
        b.counters.check_keys["shop.main"]
    );
    assert_ne!(std_keys(&a), std_keys(&console));
}

const STATS: &str = include_str!("programs/std_interfaces/stats.hd");

#[test]
fn std_typed_signatures_compile_through_headers() {
    let mut s = MemorySources::default();
    s.insert("stats/calc.hd", STATS);
    s.insert(
        "main.hd",
        include_str!("programs/std_interfaces/stats_main.hd"),
    );
    crate::add_facades(&mut s);
    let out = run(&s);
    let t = out.report.tally(Stage::UnitIface);
    assert_eq!((t.ok, t.not_implemented, t.blocked), (20, 0, 0));
    assert_eq!(out.report.tally(Stage::HeaderCheck).ok, 20);
    assert_eq!(out.report.tally(Stage::Coherence).ok, 1);
    let text = out.render();
    assert!(
        !text.contains("unknown-type") && !text.contains("unknown-import"),
        "{text}"
    );
}

/// A signature's written lifetimes, one per local
/// reference slot of the parameters and then the result, survive the
/// interface blob, a list of local references' among them
/// (`ref.lifetime.store.argument`).
#[test]
fn written_lifetimes_map_reference_slots() {
    let mut s = MemorySources::default();
    s.insert(
        "main.hd",
        include_str!("programs/std_interfaces/lifetimes.hd"),
    );
    let out = run(&s);
    assert!(
        !out.diags
            .code
            .contains(&hd_diag::Code::LocalReferenceEscapes),
        "{}",
        out.render()
    );
    let (_, blob) = out
        .ifaces
        .iter()
        .find(|f| !f.0.starts_with("std"))
        .expect("the program's interface");
    fresh(|n| {
        let (items, _) = decode_items(n, blob).expect("decode");
        let mut got: Vec<(String, Vec<u16>)> = items
            .iter()
            .filter_map(|it| match &it.data {
                ItemData::Fn(s, _) | ItemData::Method { sig: s, .. } => {
                    Some((n.text(it.name).to_owned(), s.lifetimes.clone()))
                }
                _ => None,
            })
            .collect();
        got.sort_stable();
        assert_eq!(
            got,
            [
                ("choose".to_owned(), vec![1, 0, 1, 0, 1]),
                ("left_or".to_owned(), vec![1, 2, 1, 2]),
                ("main".to_owned(), vec![]),
                ("plain".to_owned(), vec![]),
            ]
        );
    });
}

/// Unused std files are not implicit interface/body roots (#2363).
/// Count actual work at tenfold width and depth20, not elapsed time.
#[test]
fn unreachable_std_growth_does_not_expand_parsing_or_checking() {
    let mut before = None;
    for (width, depth) in [(100, 2), (1000, 20)] {
        let mut sources = hd_driver::std_sources();
        sources.insert(
            "conformance/probe.hd",
            include_str!("programs/std_interfaces/hello.hd"),
        );
        for item in 0..width {
            let mut path = format!("unrelated_{item}");
            for _ in 1..depth {
                path.push_str("/nested");
            }
            path.push_str("/leaf.hd");
            sources.insert(&path, include_str!("programs/std_interfaces/one.hd"));
        }
        let store = MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Shuffled(7)),
        };
        let (out, hint_work) = hd_resolve::work::count_hint_work(|| {
            build_packages(
                &host,
                "std",
                &Packages::default(),
                &Goal::Program {
                    entry: "conformance.probe".to_owned(),
                },
            )
        });
        assert_eq!(hint_work, 0, "valid code never asks the std advice index");
        assert!(out.diags.is_empty(), "{}", out.render());
        assert!(out.wasm.is_some(), "std-root probe links");
        assert!(
            out.counters
                .parsed
                .iter()
                .all(|m| !m.starts_with("std.unrelated_"))
        );
        let work = (
            out.counters.parsed.len(),
            out.counters.ifaces_built.len(),
            out.counters.modules_checked.len(),
            out.report.tally(Stage::HeaderCheck).ok,
        );
        if let Some(before) = before {
            assert_eq!(work, before, "width{width}/depth{depth}");
        } else {
            before = Some(work);
        }
    }
}
