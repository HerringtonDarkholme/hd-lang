use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_sched::SerialOrder;

/// The ownership view: each body of the program's own package that the
/// build reads, as the ownership passes leave it
/// (`docs/compiler/tir-v2.md`, "Where and when the passes run"): the
/// instructions whose source lies inside the subject, by each
/// instruction's own syntax node, the locals they name, the body's exit
/// set, the unwind sets of the suspension points shown and the stable
/// list places of the loops shown, after the body's path; then the
/// build's diagnostics.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let built = build(p, at);
    let mut pool = hd_driver::PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = super::Dump::new(p);
    for (item, own) in &built.own_text {
        let mut lines = Vec::new();
        for line in own.text.lines() {
            let Some(rest) = line.trim_start().strip_prefix('%') else {
                continue;
            };
            let Some(k) = rest.split(' ').next().and_then(|n| n.parse::<usize>().ok()) else {
                continue;
            };
            if let Some(&node) = own.syn.get(k)
                && let Some((file, lo, hi)) = positions.node_span(item, node)
                && file == "main.hd"
                && p.inside(lo as usize, hi as usize)
            {
                lines.push((lo as usize, line.trim()));
            }
        }
        let Some(first) = lines.iter().map(|(lo, _)| *lo).min() else {
            continue;
        };
        out.at(first);
        let _ = writeln!(out, "{item}");
        for local in own
            .text
            .lines()
            .filter_map(|l| l.trim().strip_prefix("local "))
        {
            let id = local.split(' ').next().unwrap_or_default();
            if lines
                .iter()
                .any(|(_, l)| l.split([' ', ']', '[']).any(|w| w == id))
            {
                let _ = writeln!(out, "  local {local}");
            }
        }
        let last = lines.iter().map(|(lo, _)| *lo).max().unwrap_or(first);
        let shown: Vec<&str> = lines.iter().map(|&(_, l)| l).collect();
        for &(lo, l) in &lines {
            out.at(lo);
            let _ = writeln!(out, "  {l}");
        }
        out.at(last);
        for exit in own.text.lines().filter(|l| l.trim().starts_with("exit ")) {
            let _ = writeln!(out, "  {}", exit.trim());
        }
        // The unwind sets of the suspension points shown.
        for unwind in own.text.lines().map(str::trim) {
            let Some(point) = unwind
                .strip_prefix("unwind ")
                .and_then(|r| r.split(' ').next())
            else {
                continue;
            };
            if shown.iter().any(|&l| l.split(' ').next() == Some(point)) {
                let _ = writeln!(out, "  {unwind}");
            }
        }
        // The stable list places of the loops shown.
        for keeps in own.text.lines().map(str::trim) {
            let Some(lp) = keeps
                .strip_prefix("loop ")
                .and_then(|r| r.split(' ').next())
            else {
                continue;
            };
            if shown.iter().any(|&l| l.split(' ').next() == Some(lp)) {
                let _ = writeln!(out, "  {keeps}");
            }
        }
    }
    if out.flat.is_empty() && !built.diags.has_errors() {
        out.push_str("no body read\n");
    }
    let rendered = built.render();
    if !rendered.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", rendered.trim_end());
    }
    out.finish()
}

/// The case module's functions, built as a fixture through the ownership
/// passes (`Packages::own_passes`), with their bodies rendered after them.
fn build(p: &super::Program, at: super::Build) -> Output {
    let sources = p.sources();
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        sources: &sources,
        store: &store,
        render_tir: &["app/*"],
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build_packages(
        &host,
        "app",
        &Packages {
            target: at.target,
            level: if at.release {
                hd_driver::OptLevel::Optimized
            } else {
                hd_driver::OptLevel::Dev
            },
            build_profile: if at.release {
                hd_driver::BuildProfile::Release
            } else {
                hd_driver::BuildProfile::Checked
            },
            own_passes: true,
            ..Packages::default()
        },
        &Goal::Fixture {
            module: "main".to_owned(),
        },
    )
}
