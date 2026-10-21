use std::fmt::Write as _;

/// Count operations that survived emission/optimization inside the subject.
/// The backend supplies semantic labels and real source origins; this view
/// never searches WAT text or guesses values from numbered locals.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let built = super::inspection::build(p, true, at);
    let mut pool = hd_driver::PositionPool::default();
    let positions = built.positions(&mut pool);
    let mut out = String::new();
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
        let _ = writeln!(out, "{item}");
        for c in operations {
            let _ = writeln!(out, "  {} {}", c.action.name(), c.subject);
        }
    }
    if out.is_empty() && !built.diags.has_errors() {
        out.push_str("no count operations\n");
    }
    let diagnostics = built.render();
    if !diagnostics.trim().is_empty() {
        let _ = writeln!(out, "{}", diagnostics.trim_end());
    }
    out
}
