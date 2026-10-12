//! `hd doc std.MODULE.ITEM` (`cli.doc.name.std`): the signature, summary
//! and reference link of a standard library item, from the table that
//! `build.rs` generates out of `spec/std`. The table is compiled in, so
//! the answer needs neither the network nor the specification files.

use std::fmt::Write as _;

include!(concat!(env!("OUT_DIR"), "/std_table.rs"));

type Row = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// The page of `std.MODULE.ITEM` (`cli.doc.name.std.output`), where
/// `rest` is `MODULE.ITEM` and ITEM is `name` or `Type.name`. `Err` holds
/// the names to suggest (`cli.doc.name.missing`).
pub(crate) fn page(rest: &str) -> Result<String, Vec<String>> {
    let (module, item) = rest.split_once('.').unwrap_or((rest, ""));
    let in_module: Vec<&Row> = STD_ENTRIES.iter().filter(|e| e.0 == module).collect();
    let found: Vec<&&Row> = in_module
        .iter()
        .filter(|e| !item.is_empty() && (e.2 == item || format!("{}.{}", e.1, e.2) == item))
        .collect();
    if found.is_empty() {
        return Err(suggestions(module, item, &in_module));
    }
    let mut out = String::new();
    for (i, (module, _, name, signature, summary, link)) in
        found.into_iter().map(|e| **e).enumerate()
    {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "```hd\n{signature}\n```\n\n");
        if !summary.is_empty() {
            let _ = write!(out, "{summary}\n\n");
        }
        let _ = writeln!(out, "Reference: [std.{module}.{name}]({link})");
    }
    Ok(out)
}

fn suggestions(module: &str, item: &str, in_module: &[&Row]) -> Vec<String> {
    if in_module.is_empty() {
        let mut modules: Vec<String> = STD_ENTRIES.iter().map(|e| format!("std.{}", e.0)).collect();
        modules.dedup();
        return crate::rank(modules, &format!("std.{module}"));
    }
    let mut names: Vec<String> = in_module
        .iter()
        .map(|e| format!("std.{module}.{}", e.2))
        .collect();
    names.dedup();
    crate::rank(names, &format!("std.{module}.{item}"))
}
