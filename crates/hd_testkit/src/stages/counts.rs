use std::fmt::Write as _;

/// Count operations, each with its reason, that survived emission/optimization inside the subject,
/// plus a subject function's operations with no source position (exit
/// releases, a temporary's free), marked `(no span)` at its last line.
/// The backend supplies semantic labels and real source origins; this view
/// never searches WAT text or guesses values from numbered locals.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let built = super::inspection::build(p, at);
    let mut pool = hd_driver::PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = super::Dump::new(p);
    // A `std` build's case is a module of the standard library.
    let (prefix, case) = if at.std {
        ("std/rt_tests.stage_case/", super::own::STD_CASE)
    } else {
        ("app/", "main.hd")
    };
    for f in &built.instruction_origins {
        let Some(item) = &f.item else {
            continue;
        };
        if !item.starts_with(prefix) {
            continue;
        }
        let span = |origin: Option<u32>| {
            origin
                .and_then(|i| positions.span(item, i))
                .filter(|(file, _, _)| *file == case)
                .map(|(_, lo, hi)| (lo as usize, hi as usize))
        };
        // The function belongs to the subject when any instruction does;
        // its operations with no source position print at its last line.
        let Some(last) = f
            .instructions
            .iter()
            .filter_map(|i| span(i.origin))
            .filter(|(lo, hi)| p.inside(*lo, *hi))
            .map(|(lo, _)| lo)
            .max()
        else {
            continue;
        };
        let spanned: Vec<_> = f
            .counts
            .iter()
            .filter_map(|c| span(c.origin).map(|s| (s, c)))
            .filter(|((lo, hi), _)| p.inside(*lo, *hi))
            .map(|((lo, _), c)| (lo, c))
            .collect();
        let unspanned: Vec<_> = f
            .counts
            .iter()
            .filter(|c| span(c.origin).is_none())
            .collect();
        if spanned.is_empty() && unspanned.is_empty() {
            continue;
        }
        let first = spanned.iter().map(|(lo, _)| *lo).min().unwrap_or(last);
        out.at(first);
        let _ = writeln!(out, "{item}");
        for (lo, c) in spanned {
            out.at(lo);
            let _ = writeln!(out, "  {} {} ({})", c.action.name(), c.subject, c.reason);
        }
        out.at(last);
        for c in unspanned {
            let _ = writeln!(
                out,
                "  (no span) {} {} ({})",
                c.action.name(),
                c.subject,
                c.reason
            );
        }
    }
    if out.flat.is_empty() && !built.diags.has_errors() {
        out.push_str("no count operations\n");
    }
    let diagnostics = built.render();
    if !diagnostics.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", diagnostics.trim_end());
    }
    out.finish()
}
