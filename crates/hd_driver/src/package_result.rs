//! The package result (#1510, `crates/hd_cache/DESIGN.md` §5.5): one
//! `depfiles` entry per warm `hd check` whose inputs are unchanged, keyed by
//! every package's listing (each file's path and stamp) and the run's
//! settings. A run whose key it holds takes the stored result instead of
//! decoding each set's last-run record and looking up each module's entries.
//!
//! Only a run that reported nothing is stored, with its entry rows
//! (`Output::entry_capabilities`), the one part of a clean check's result a
//! caller reads besides the file lists. A run with any diagnostic, or with a
//! file listed without a stamp, takes the per-module path.
//!
//! Per-run cost, n files: the listing's one `stat` per file (as before), one
//! hash of each listed path and stamp, and one entry lookup; on a hit, no
//! record bytes and no other lookup. Writing costs one entry of O(entry rows)
//! bytes, only on a clean run that missed.

use hd_base::wire::{Reader, Writer};
use hd_base::{FileId, Hash128, Span, StableHasher};
use hd_cache::{CacheStore, EntryKind};
use hd_project::{ModuleTable, PackageIn};

use crate::{CapabilityEntry, EntryCapability, Goal, Host, Packages};

/// The key of a run's package result, or `None` when the run may not use
/// one: any goal but `hd check`'s, a request for more than diagnostics and
/// entry rows, or a file listed without a stamp.
#[allow(clippy::too_many_arguments)]
pub(crate) fn key(
    goal: &Goal,
    package: &str,
    packages: &Packages<'_>,
    host: &Host<'_>,
    analysis: bool,
    ins: &[PackageIn<'_>],
    table: &ModuleTable,
    toolchain: Hash128,
) -> Option<Hash128> {
    let plain = matches!(goal, Goal::Analyze)
        && package != "std"
        && !analysis
        && host.render_tir.is_empty()
        // The sets host declarations are read from must be the packages'
        // own, whose files the key holds.
        && packages.host_declaration_sources.iter().all(|d| {
            d.identity()
                .is_some_and(|id| ins.iter().any(|p| p.sources.identity() == Some(id)))
        })
        && !packages.instruction_origins
        && !packages.single_file
        && !packages.doc_scopes
        && packages.check_roots.is_none()
        && packages.session.is_none()
        && packages.reuse.is_none()
        && packages.inputs_from.is_none()
        && packages.failed_inputs.is_empty()
        && packages.body_fuel.is_none()
        && !packages.checked
        && !packages.verify_own;
    if !plain {
        return None;
    }
    let mut k = StableHasher::new("package-result-v1");
    k.hash(toolchain);
    k.u16(super::LAYOUT);
    k.str(package);
    k.u8(u8::from(packages.entry_capabilities));
    k.str(packages.std_leaf.unwrap_or(""));
    k.str(&format!(
        "{:?} {:?} {:?} {:?}",
        packages.profile, packages.target, packages.level, packages.build_profile
    ));
    for d in &packages.deps {
        k.u8(u8::from(d.fetched));
    }
    // Host packages: what the run's host-contract key reads, and the
    // declarations the manifest holds.
    for h in &packages.host_packages {
        k.u16(h.package);
        k.str(&h.identity);
        k.str(h.versioned_source.as_deref().unwrap_or(""));
        k.str(&h.manifest);
        k.str(&format!("{:?}", h.declarations));
    }
    k.u32(u32::try_from(ins.len()).ok()?);
    for p in ins {
        k.str(p.name);
        k.str(p.identity);
        k.str(&format!("{:?}", p.scope));
        for list in [&p.requires, &p.dev_requires] {
            k.u32(u32::try_from(list.len()).ok()?);
            for (name, at) in list {
                k.str(name);
                k.u16(*at);
            }
        }
        k.u32(u32::try_from(p.entries.len()).ok()?);
        for e in &p.entries {
            k.str(e);
        }
        // A set without an identity has no last-run record either; the
        // embedded std's files are the toolchain's.
        match p.sources.identity() {
            Some(id) => k.hash(id),
            None if std::ptr::eq(p, ins.last()?) => k.str("std"),
            None => return None,
        }
        k.u8(u8::from(p.sources.partial()));
    }
    k.u32(u32::try_from(table.sources.len()).ok()?);
    for ((package, path), stamp) in table.sources.iter().zip(&table.stamps) {
        k.u16(*package);
        k.str(path);
        k.hash((*stamp)?);
    }
    Some(k.finish())
}

fn entry_key(toolchain: Hash128, key: Hash128) -> Hash128 {
    hd_cache::depfiles_key(toolchain, &format!("pkg:{:032x}", key.0))
}

/// The stored result under `key`: the clean run's entry rows.
pub(crate) fn read(
    store: &dyn CacheStore,
    toolchain: Hash128,
    key: Hash128,
) -> Option<Vec<EntryCapability>> {
    let at = entry_key(toolchain, key);
    let bytes = store.get(EntryKind::Depfiles, &at)?;
    let (h, sections) = hd_cache::decode_entry(&bytes).ok()?;
    if h.key != at || h.toolchain != toolchain || h.layout != super::LAYOUT {
        return None;
    }
    let mut r = Reader::new(sections.first()?.1);
    let rows = (0..r.count())
        .map(|_| {
            let span = Span {
                file: FileId::from_raw(r.u32()),
                lo: r.u32(),
                hi: r.u32(),
            };
            let selectors = (0..r.count()).map(|_| r.str().to_owned()).collect();
            let entry = match r.u8() {
                0 => CapabilityEntry::Main,
                1 => CapabilityEntry::Task,
                _ => CapabilityEntry::Test(r.str().to_owned()),
            };
            EntryCapability {
                span,
                selectors,
                entry,
                written_row: r.u8() != 0,
            }
        })
        .collect();
    (r.ok() && r.at_end()).then_some(rows)
}

/// Stores a clean run's result under `key`.
pub(crate) fn write(
    store: &dyn CacheStore,
    toolchain: Hash128,
    key: Hash128,
    rows: &[EntryCapability],
) {
    let mut w = Writer::default();
    w.len_of(rows);
    for row in rows {
        w.u32(row.span.file.raw());
        w.u32(row.span.lo);
        w.u32(row.span.hi);
        w.len_of(&row.selectors);
        for s in &row.selectors {
            w.str(s);
        }
        match &row.entry {
            CapabilityEntry::Main => w.u8(0),
            CapabilityEntry::Task => w.u8(1),
            CapabilityEntry::Test(name) => {
                w.u8(2);
                w.str(name);
            }
        }
        w.u8(u8::from(row.written_row));
    }
    let at = entry_key(toolchain, key);
    let bytes = hd_cache::encode_entry(
        EntryKind::Depfiles,
        super::LAYOUT,
        at,
        toolchain,
        0,
        &[(0, 0, &w.bytes)],
    );
    store.put(EntryKind::Depfiles, &at, &bytes);
}
