use std::fmt::Write as _;

/// The ownership view: each body of the program's own package that the
/// build reads, as the ownership passes leave it
/// (`docs/compiler/tir-v2.md`, "Where and when the passes run"): the
/// instructions whose source lies inside the subject, by each
/// instruction's own syntax node, the locals they name, the body's exit
/// set and where its locals live, the unwind sets and frame rows of the suspension points shown and the stable
/// list places of the loops shown, after the body's path; then the
/// build's diagnostics.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    // Whatever the case links, this view reads the fixture build's bodies.
    let built = super::inspection::build(
        p,
        super::Build {
            program: false,
            ..at
        },
    );
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
                && file == if at.std { STD_CASE } else { "main.hd" }
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
                .any(|(_, l)| l.split([' ', ']', '[', '*']).any(|w| w == id))
            {
                let _ = writeln!(out, "  local {local}");
                // A value local's reads, each consuming or lending.
                let uses = format!("uses {id}:");
                // A shared local that could live in a frame slot.
                let cell = format!("frame cell [{id}]");
                for l in own.text.lines().map(str::trim) {
                    if l.starts_with(&uses) || l == cell {
                        let _ = writeln!(out, "    {l}");
                    }
                }
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
        for row in own
            .text
            .lines()
            .filter(|l| l.trim().starts_with("storage "))
        {
            let _ = writeln!(out, "  {}", row.trim());
        }
        // A release build's summary of the body.
        for summary in own.text.lines().filter(|l| l.starts_with("summary ")) {
            let _ = writeln!(out, "  {summary}");
        }
        // The unwind sets and frame rows of the suspension points shown.
        for unwind in own.text.lines().map(str::trim) {
            let Some(point) = unwind
                .strip_prefix("unwind ")
                .or_else(|| unwind.strip_prefix("live "))
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

/// Where a `std` build puts the case: a std module of its own folder, which
/// may use the runtime modules and the root std modules alike.
pub(super) const STD_CASE: &str = "rt_tests/stage_case.hd";
