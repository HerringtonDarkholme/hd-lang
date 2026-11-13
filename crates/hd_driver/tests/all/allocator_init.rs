//! A runtime init whose bindings no reached code writes does nothing once
//! link folds its constant stores, so `hd.init` leaves it out and the
//! module drops it (#1423). A program that allocates keeps the allocator's
//! init, called from `hd.init`.

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The debug names of the module's functions, and its global count, for
/// program `main` built in dev (`checked` off) or checked mode.
fn linked(main: &str, checked: bool) -> (Vec<String>, u32) {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        checked,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let mut names = Vec::new();
    let mut globals = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::GlobalSection(g) => globals = g.count(),
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            names.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (names, globals)
}

const INIT: &str = "std/rt.alloc/init.init#0";

#[test]
fn a_program_that_never_allocates_runs_no_allocator_init() {
    let (names, globals) = linked("fn helper() -> i32:\n    1\n", false);
    assert!(!names.iter().any(|n| n == INIT), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("std/rt.")), "{names:?}");
    // Nor the allocator's bindings, nor the forbidden-context depth its
    // call from `hd.init` was wrapped in.
    assert_eq!(globals, 0, "{names:?}");
}

#[test]
fn a_program_that_allocates_in_a_std_call_runs_the_allocator_init() {
    let (names, globals) = linked(
        "pub fn main() -> void $ Console:\n    println(\"ab\".repeat(3))\n",
        false,
    );
    assert!(names.iter().any(|n| n == INIT), "{names:?}");
    assert!(globals > 0, "{names:?}");
}

#[test]
fn a_checked_program_keeps_the_allocator_init_its_audit_needs() {
    let (names, _) = linked("fn helper() -> i32:\n    1\n", true);
    assert!(names.iter().any(|n| n == INIT), "{names:?}");
}

/// Instrument the production scan in the supplied std source, so the
/// generated heap-growth test counts work without adding runtime counters.
#[test]
fn heap_growth_does_not_rescan_the_occupied_prefix() -> Result<(), String> {
    let allocator = include_str!("../../../../lib/std/rt/alloc.hd");
    let site = "while page_number < known && run < count:\n";
    assert_eq!(allocator.matches(site).count(), 1, "one production scan");
    let instrumented = allocator
        .replace(site, &format!("{site}        scan_count += 1\n"))
        .replacen(
            "let table: usize = 0",
            "let scan_count: usize = 0\nlet table: usize = 0",
            1,
        )
        + "\npub fn scan_steps() -> usize: scan_count\n";
    let mut sources = hd_driver::std_sources();
    sources.insert("rt/alloc.hd", &instrumented);
    sources.insert(
        "rt_tests/allocator_scan.hd",
        include_str!("allocator_scan.hd"),
    );
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &[],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    for level in [hd_driver::OptLevel::Dev, hd_driver::OptLevel::Optimized] {
        let out = build_packages(
            &host,
            "std",
            &Packages {
                level,
                build_profile: if level == hd_driver::OptLevel::Dev {
                    hd_driver::BuildProfile::Checked
                } else {
                    hd_driver::BuildProfile::Release
                },
                std_leaf: Some("rt_tests/allocator_scan.hd"),
                ..Packages::default()
            },
            &Goal::Program {
                entry: "rt_tests.allocator_scan".into(),
            },
        );
        if out.diags.has_errors() {
            return Err(out.render());
        }
        let path =
            hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "allocator-scan-growth.wasm");
        std::fs::write(&path, out.wasm.ok_or("no allocator scan module")?)
            .map_err(|e| e.to_string())?;
        // cost: one Node process for the fixed generated width cases; scan work
        // is measured inside the guest rather than inferred from wall time.
        let ran = std::process::Command::new("node")
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        if !ran.status.success() {
            return Err(format!(
                "{}{}",
                String::from_utf8_lossy(&ran.stdout),
                String::from_utf8_lossy(&ran.stderr)
            ));
        }
        println!("{}", String::from_utf8_lossy(&ran.stdout));
    }
    Ok(())
}
