//! `hd doc std.MODULE.ITEM` (`cli.doc.name.std`): a standard library
//! module or item, rendered from the `lib/std` source in `Package::std` by
//! the renderer that user packages use. An item's reference link is the
//! `spec/std` link its documentation holds.

use std::fmt::Write as _;

use crate::items::{self, Item, Section};
use crate::{Module, Package, impls, module_page, rank};

/// The module `path` (`std.text`) of `Package::std`.
fn module<'a>(p: &'a Package<'a>, path: &str) -> Option<&'a Module<'a>> {
    let short = path.strip_prefix("std.")?;
    p.std.iter().find(|m| m.path == short)
}

/// Whether `path` (`std.text`) is `std` or one of its modules.
pub(crate) fn has_module(p: &Package<'_>, path: &str) -> bool {
    path == "std" || module(p, path).is_some()
}

/// The sections of a module: each item and each member.
fn sections(items: &[Item]) -> impl Iterator<Item = &Section> {
    items
        .iter()
        .flat_map(|it| std::iter::once(&it.section).chain(&it.members))
}

/// The module's items, as `hd doc` documents them.
fn read(m: &Module<'_>) -> Vec<Item> {
    items::items(m.text, &hd_syntax::parse(m.text.as_bytes()), false, true)
}

/// The sections `item` names: `name` or `Type.name`, or else, with no
/// such section, the members called `name` of any type.
fn named<'a>(items: &'a [Item], item: &str) -> Vec<&'a Section> {
    let exact: Vec<&Section> = sections(items).filter(|s| s.anchor == item).collect();
    if !exact.is_empty() || item.contains('.') {
        return exact;
    }
    let tail = format!(".{item}");
    sections(items)
        .filter(|s| s.anchor.ends_with(&tail))
        .collect()
}

/// The target of the first `spec/std` link in a documentation text, as
/// `spec/std/text.md#r-std-text.string.split`.
fn spec_link(doc: &str) -> Option<&str> {
    let rest = &doc[doc.find("spec/std/")?..];
    let end = rest.find(|c: char| c == ')' || c.is_whitespace())?;
    Some(&rest[..end])
}

/// The reference link of a module: the file of the first link its items
/// carry.
fn module_link(items: &[Item]) -> Option<&str> {
    sections(items)
        .find_map(|s| spec_link(&s.doc))
        .map(|l| l.split_once('#').map_or(l, |(file, _)| file))
}

/// The page of `std.MODULE.ITEM` (`cli.doc.name.std.output`), where `rest`
/// is `MODULE.ITEM`, ITEM is `name` or `Type.name`, and a `MODULE` alone
/// gives the module's whole page. `Err` holds the names to suggest
/// (`cli.doc.name.missing`).
pub(crate) fn page(p: &Package<'_>, rest: &str) -> Result<String, Vec<String>> {
    let parts: Vec<&str> = rest.split('.').collect();
    let found = (1..=parts.len()).rev().find_map(|k| {
        let path = format!("std.{}", parts[..k].join("."));
        module(p, &path).map(|m| (k, m))
    });
    let Some((k, m)) = found else {
        let modules = p.std.iter().map(|m| format!("std.{}", m.path)).collect();
        return Err(rank(modules, &format!("std.{rest}")));
    };
    let shown = format!("std.{}", m.path);
    let items = read(m);
    if k == parts.len() {
        let doc = Module {
            path: &shown,
            text: m.text,
        };
        let lines = impls::impl_lines(p.root, &[]);
        let (page, _) = module_page(&doc, p, &lines, None, false, true);
        // The sources link the specification from `lib/std`.
        let mut out = page.md.replace("](../../spec/", "](spec/");
        if let Some(link) = module_link(&items) {
            let _ = writeln!(out, "Reference: [{shown}]({link})");
        }
        return Ok(out);
    }
    let item = parts[k..].join(".");
    let found = named(&items, &item);
    if found.is_empty() {
        let names = sections(&items).map(|s| format!("{shown}.{}", s.anchor));
        return Err(rank(names.collect(), &format!("{shown}.{item}")));
    }
    let mut out = String::new();
    for (i, s) in found.into_iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "```hd\n{}\n```\n\n", s.signature);
        let summary = crate::summary(&s.doc);
        if !summary.is_empty() {
            let _ = write!(out, "{summary}\n\n");
        }
        if let Some(link) = spec_link(&s.doc).or_else(|| module_link(&items)) {
            let _ = writeln!(out, "Reference: [{shown}.{}]({link})", s.anchor);
        }
    }
    Ok(out)
}

/// The reference link of a standard library item (`cli.doc.link.external`):
/// the link `hd doc std.MODULE.ITEM` prints. `path` is the full module path
/// (`std.text`), `item` is `name` or `Type.name`.
pub(crate) fn link(p: &Package<'_>, path: &str, item: &str) -> Option<String> {
    let items = read(module(p, path)?);
    let first = *named(&items, item).first()?;
    spec_link(&first.doc)
        .or_else(|| module_link(&items))
        .map(str::to_owned)
}
