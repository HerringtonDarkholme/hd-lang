use std::fmt::Write as _;

use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_sched::SerialOrder;

/// The `mir` view: the prototype memory IR of the IR study
/// (`docs/compiler/memory-ir.md`) of each function the case defines,
/// after its ownership passes, keeping the statements whose TIR origin
/// lies inside the subject; then what the passes did and the build's
/// diagnostics.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let sources = p.sources();
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &["app/*"],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let built = build_packages(
        &host,
        "app",
        &Packages {
            mir: true,
            target: at.target,
            level: if at.release {
                OptLevel::Optimized
            } else {
                OptLevel::Dev
            },
            build_profile: if at.release {
                BuildProfile::Release
            } else {
                BuildProfile::Checked
            },
            ..Packages::default()
        },
        &Goal::Fixture {
            module: "main".to_owned(),
        },
    );
    let mut pool = hd_driver::PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = super::Dump::new(p);
    for f in &built.fixture_mir {
        let item = f.item.as_str();
        let span = |o: u32| {
            positions
                .span(item, o)
                .filter(|(file, lo, hi)| *file == "main.hd" && p.inside(*lo as usize, *hi as usize))
                .map(|(_, lo, _)| lo as usize)
        };
        let names: std::collections::BTreeMap<u32, String> = built
            .tir_text
            .get(item)
            .map(|t| {
                t.lines()
                    .filter_map(|l| l.trim().strip_prefix("local $"))
                    .filter_map(|l| {
                        let (k, rest) = l.split_once(' ')?;
                        let name = rest.split(':').next()?.trim();
                        Some((k.parse().ok()?, name.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let local = |l: u32| names.get(&l).cloned();
        let keep = |o: u32| span(o).is_some();
        let text = f.print(&hd_driver::mir::Names {
            local: &local,
            keep: &keep,
        });
        let first = (0..f.tir_len).filter_map(&span).min();
        let Some(first) = first else {
            // A function outside the subject shows only verifier findings,
            // which every function must be free of.
            if !f.errors.is_empty() {
                out.at(p.text.len());
                let _ = writeln!(out, "{item}");
                for e in &f.errors {
                    let _ = writeln!(out, "  verifier: {e}");
                }
            }
            continue;
        };
        out.at(first);
        let _ = writeln!(out, "{item}");
        out.push_str(&text);
        let s = &f.stats;
        if f.unsupported.is_none() {
            let _ = writeln!(
                out,
                "  passes: {} retains, {} releases before; {} moved, {} placed, {} borrowed, {} null releases",
                s.retains_before, s.releases_before, s.moved, s.placed, s.borrowed, s.null_releases
            );
        }
    }
    if out.flat.is_empty() && !built.diags.has_errors() {
        out.push_str("no function in the subject\n");
    }
    let diagnostics = built.render();
    if !diagnostics.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", diagnostics.trim_end());
    }
    out.finish()
}
