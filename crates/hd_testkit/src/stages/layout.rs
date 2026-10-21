use std::fmt::Write as _;

/// Exact memory layouts of the concrete declarations inside the subject.
pub(super) fn view(p: &super::Program, at: super::Build) -> String {
    let built = super::inspection::build(p, false, at);
    let mut out = super::Dump::new(p);
    for ty in &built.fixture_layouts {
        if ty.file != "main.hd" || !p.inside(ty.lo as usize, ty.hi as usize) {
            continue;
        }
        out.at(ty.lo as usize);
        let layout = &ty.layout;
        let _ = writeln!(
            out,
            "{} size={} align={} cycle-capable={}",
            layout.name, layout.size, layout.align, layout.cycle_capable
        );
        let counted = |leaves: &[hd_driver::CountedLeaf]| {
            if leaves.is_empty() {
                return "none".to_owned();
            }
            leaves
                .iter()
                .map(|l| format!("{}{}", l.offset, if l.conditional { "?" } else { "" }))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(out, "  counted: {}", counted(&layout.counted));
        for f in &layout.fields {
            let _ = writeln!(
                out,
                "  {}: {} offset={} size={} align={} counted={}",
                f.path,
                f.ty,
                f.offset,
                f.size,
                f.align,
                counted(&f.counted)
            );
        }
    }
    if out.flat.is_empty() && !built.diags.has_errors() {
        out.push_str("no concrete struct layouts\n");
    }
    let diagnostics = built.render();
    if !diagnostics.trim().is_empty() {
        out.at(p.text.len());
        let _ = writeln!(out, "{}", diagnostics.trim_end());
    }
    out.finish()
}
