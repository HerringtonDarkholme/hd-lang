//! `hd doc NAME` (`cli.doc.name`): one item, one module, or one standard
//! library entry, as Markdown.

use super::{Module, Package, impls, module_page, shown_name, stdref};

/// The most suggestions a missing NAME gets.
const SUGGESTIONS: usize = 8;

/// The names of `names` to suggest for `wanted`: those that hold its last
/// segment first, then the rest, at most [`SUGGESTIONS`].
pub(crate) fn rank(mut names: Vec<String>, wanted: &str) -> Vec<String> {
    let last = wanted.rsplit('.').next().unwrap_or(wanted).to_lowercase();
    names.sort();
    names.dedup();
    let (mut close, rest): (Vec<String>, Vec<String>) = names
        .into_iter()
        .partition(|n| !last.is_empty() && n.to_lowercase().contains(&last));
    close.extend(rest);
    close.truncate(SUGGESTIONS);
    close
}

/// The headings of a Markdown page outside code blocks: (level, text in
/// code, line index).
pub(crate) fn headings(md: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in md.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            let level = line.bytes().take_while(|b| *b == b'#').count();
            let text = line[level..].trim();
            let text = text.strip_prefix("Module ").unwrap_or(text);
            out.push((level, text.trim_matches('`').to_owned(), i));
        }
    }
    out
}

/// The section of `heading`: from its line to the next heading of the same
/// or a higher level (`cli.doc.name`).
fn section(md: &str, heading: usize) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let all = headings(md);
    let Some(pos) = all.iter().position(|h| h.2 == heading) else {
        return String::new();
    };
    let level = all[pos].0;
    let mut end = all[pos + 1..]
        .iter()
        .find(|h| h.0 <= level)
        .map_or(lines.len(), |h| h.2);
    // The next heading's anchor line belongs to it.
    if end < lines.len() && end > 0 && lines[end - 1].starts_with("<a id=") {
        end -= 1;
    }
    let mut text = lines[heading..end].join("\n");
    text.truncate(text.trim_end().len());
    text.push('\n');
    text
}

/// What `hd doc NAME` prints for `name` in the package `p`
/// (`cli.doc.name.form`, `cli.doc.name.module`, `cli.doc.name.std`).
/// `Err` holds the names to suggest when NAME names nothing documented
/// (`cli.doc.name.missing`).
pub(crate) fn lookup(p: &Package<'_>, name: &str) -> Result<String, Vec<String>> {
    if name == "std" {
        return stdref::page(p, "");
    }
    if let Some(rest) = name.strip_prefix("std.") {
        return stdref::page(p, rest);
    }
    let parts: Vec<&str> = name.split('.').collect();
    let found: Option<(usize, &Module<'_>)> = (1..=parts.len()).rev().find_map(|k| {
        let prefix = parts[..k].join(".");
        p.modules
            .iter()
            .find(|m| shown_name(m.path) == prefix)
            .map(|m| (k, m))
    });
    let Some((k, module)) = found else {
        let modules = p.modules.iter().map(|m| shown_name(m.path).to_owned());
        return Err(rank(modules.collect(), name));
    };
    let (page, _) = module_page(
        module,
        p,
        &impls::impl_lines(p.root, p.ifaces),
        None,
        true,
        false,
    );
    if k == parts.len() {
        return Ok(page.md);
    }
    let item = parts[k..].join(".");
    let all = headings(&page.md);
    if let Some(h) = all.iter().find(|h| h.0 >= 2 && h.1 == item) {
        return Ok(section(&page.md, h.2));
    }
    let shown = shown_name(module.path);
    let names = all
        .iter()
        .filter(|h| h.0 >= 2)
        .map(|h| format!("{shown}.{}", h.1))
        .collect();
    Err(rank(names, &item))
}
