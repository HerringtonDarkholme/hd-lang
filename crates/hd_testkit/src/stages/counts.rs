use std::fmt::Write as _;

/// Count operations that survived emission/optimization inside the subject.
/// The backend supplies semantic labels and real source origins; this view
/// never searches WAT text or guesses values from numbered locals.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let built = super::inspection::build(p, true, at);
    let mut pool = hd_driver::PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = super::Dump::new(p);
    for f in &built.instruction_origins {
        let Some(item) = &f.item else {
            continue;
        };
        if !item.starts_with("app/") {
            continue;
        }
        let operations: Vec<_> = f
            .counts
            .iter()
            .filter(|c| {
                c.origin
                    .and_then(|i| positions.span(item, i))
                    .is_some_and(|(file, lo, hi)| {
                        file == "main.hd" && p.inside(lo as usize, hi as usize)
                    })
            })
            .collect();
        if operations.is_empty() {
            continue;
        }
        let first = operations
            .iter()
            .filter_map(|c| c.origin.and_then(|i| positions.span(item, i)))
            .map(|(_, lo, _)| lo)
            .min()
            .expect("kept origin");
        out.at(first as usize);
        let _ = writeln!(out, "{item}");
        for c in operations {
            let (_, lo, _) = c
                .origin
                .and_then(|i| positions.span(item, i))
                .expect("kept origin");
            out.at(lo as usize);
            let _ = writeln!(out, "  {} {}", c.action.name(), c.subject);
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
