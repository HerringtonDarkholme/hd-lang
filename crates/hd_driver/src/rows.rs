//! Source-set findings and canonical interface records share one row
//! format, indexed by source set and normal/test checking role.
//! For named source sets this record owns the canonical interface
//! sections; standalone Iface entries are used only without such a record.
//! Interface records are separate from changing check findings, so a body
//! edit never rewrites unchanged interface blobs. Both use this one codec.
//! Cost: O(current modules + units) setup, two records read per source set,
//! binary search per row, and a changed set's bytes written once.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, SliceSort as _};
use hd_cache::CacheStore;
use hd_project::ModuleTable;

use crate::{CacheSection, ResultKey};

const KEPT: [usize; 6] = [0, 1, 4, 5, 6, 8];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Kind {
    Module,
    Iface,
    Header,
}

struct Row(Kind, Range<usize>, Hash128, Hash128, Range<usize>);
type Written<'r> = (Kind, &'r [u8], Hash128, Hash128, &'r [u8]);

struct Old {
    bytes: Arc<[u8]>,
    rows: Vec<Row>,
}

impl Old {
    fn read(store: &dyn CacheStore, set: &Set) -> Option<Self> {
        let bytes: Arc<[u8]> = set.at.read(store, set.kind)?.into();
        let mut r = Reader::new(&bytes);
        let blob = |r: &mut Reader<'_>| {
            let at = r.pos() + 4;
            at..at + r.blob().len()
        };
        let mut rows = Vec::new();
        for _ in 0..r.count() {
            let kind = match r.u8() {
                0 => Kind::Module,
                1 => Kind::Iface,
                2 => Kind::Header,
                _ => return None,
            };
            rows.push(Row(kind, blob(&mut r), r.hash(), r.hash(), blob(&mut r)));
        }
        (r.ok() && r.at_end()).then_some(Old { bytes, rows })
    }

    fn written(&self, row: &Row) -> Written<'_> {
        (
            row.0,
            &self.bytes[row.1.clone()],
            row.2,
            row.3,
            &self.bytes[row.4.clone()],
        )
    }
}

struct Set {
    at: ResultKey,
    kind: Hash128,
    partial: bool,
    old: OnceLock<Option<Old>>,
}

/// Module, interface and header rows, indexed once by their source set.
pub(crate) struct Rows<'a> {
    store: &'a dyn CacheStore,
    sets: Vec<Option<Set>>,
    packages: Vec<Vec<usize>>,
    modules: usize,
    units: usize,
    made: Vec<OnceLock<(Hash128, Hash128, Vec<u8>)>>,
    found: Vec<OnceLock<Option<usize>>>,
}

/// Blob sections borrow the record's allocation.
fn sections_in(bytes: &Arc<[u8]>, range: &Range<usize>) -> Option<Vec<CacheSection>> {
    let mut r = Reader::new(&bytes[range.clone()]);
    let mut out = Vec::new();
    while !r.at_end() {
        let start = range.start + r.pos() + 4;
        let len = r.blob().len();
        if !r.ok() {
            return None;
        }
        out.push(CacheSection {
            bytes: Arc::clone(bytes),
            range: start..start + len,
        });
    }
    Some(out)
}

impl<'a> Rows<'a> {
    pub(crate) fn new(
        store: &'a dyn CacheStore,
        toolchain: Hash128,
        identities: &[Option<(Hash128, bool)>],
        kind: Hash128,
        table: &ModuleTable,
    ) -> Self {
        let kind = {
            let mut h = hd_base::StableHasher::new("check-rows-v3");
            h.hash(kind);
            h.finish()
        };
        let sets = identities
            .iter()
            .flat_map(|identity| {
                [false, true].map(|interfaces| {
                    let &(key, partial) = identity.as_ref()?;
                    let mut record = hd_base::StableHasher::new("source-set-rows");
                    record.hash(kind);
                    record.u8(u8::from(interfaces));
                    Some(Set {
                        at: ResultKey { toolchain, key },
                        kind: record.finish(),
                        partial,
                        old: OnceLock::new(),
                    })
                })
            })
            .collect();
        let modules = table.modules.len();
        let units = table.units.len();
        let count = modules + 2 * units;
        let mut packages = vec![Vec::new(); identities.len() * 2];
        for (m, module) in table.modules.iter().enumerate() {
            packages[usize::from(module.package) * 2].push(m);
        }
        for (u, unit) in table.units.iter().enumerate() {
            packages[usize::from(unit.package) * 2 + 1].push(modules + u);
            packages[usize::from(unit.package) * 2].push(modules + units + u);
        }
        Self {
            store,
            sets,
            packages,
            modules,
            units,
            made: (0..count).map(|_| OnceLock::new()).collect(),
            found: (0..count).map(|_| OnceLock::new()).collect(),
        }
    }

    fn old<'s>(&self, set: &'s Set) -> Option<&'s Old> {
        set.old.get_or_init(|| Old::read(self.store, set)).as_ref()
    }

    fn slot<'t>(&self, table: &'t ModuleTable, index: usize) -> (Kind, usize, &'t [u8]) {
        if index < self.modules {
            (
                Kind::Module,
                usize::from(table.modules[index].package) * 2,
                table.sources[index].1.as_bytes(),
            )
        } else {
            let (kind, u) = if index < self.modules + self.units {
                (Kind::Iface, index - self.modules)
            } else {
                (Kind::Header, index - self.modules - self.units)
            };
            let unit = &table.units[u];
            (
                kind,
                usize::from(unit.package) * 2 + usize::from(kind == Kind::Iface),
                unit.path.as_bytes(),
            )
        }
    }

    fn row(&self, table: &ModuleTable, index: usize) -> Option<(&Old, usize)> {
        let (kind, package, path) = self.slot(table, index);
        let old = self.old(self.sets[package].as_ref()?)?;
        let find = || {
            old.rows
                .binary_search_by(|r| {
                    r.0.cmp(&kind)
                        .then_with(|| old.bytes[r.1.clone()].cmp(path))
                })
                .ok()
        };
        Some((old, (*self.found[index].get_or_init(find))?))
    }

    fn write(
        &self,
        table: &ModuleTable,
        index: usize,
        key: Hash128,
        source: Hash128,
        encode: impl FnOnce(&mut Writer),
    ) {
        let (_, package, _) = self.slot(table, index);
        if self.sets[package].is_some() {
            let mut w = Writer::default();
            encode(&mut w);
            let _ = self.made[index].set((key, source, w.bytes));
        }
    }

    pub(crate) fn get(
        &self,
        table: &ModuleTable,
        m: usize,
        key: Hash128,
        source: Hash128,
    ) -> Option<Vec<CacheSection>> {
        let (old, at) = self.row(table, m)?;
        let row = &old.rows[at];
        if (row.2, row.3) != (key, source) {
            return None;
        }
        let kept = sections_in(&old.bytes, &row.4)?;
        if kept.len() != KEPT.len() {
            return None;
        }
        let empty = CacheSection {
            bytes: Arc::clone(&old.bytes),
            range: 0..0,
        };
        let mut sections = vec![empty; KEPT[KEPT.len() - 1] + 1];
        for (index, section) in KEPT.into_iter().zip(kept) {
            sections[index] = section;
        }
        Some(sections)
    }

    pub(crate) fn parsed(&self, table: &ModuleTable, m: usize, source: Hash128) -> bool {
        self.row(table, m)
            .is_some_and(|(old, at)| old.rows[at].3 == source)
    }

    pub(crate) fn record(
        &self,
        table: &ModuleTable,
        m: usize,
        key: Hash128,
        source: Hash128,
        sections: &[impl AsRef<[u8]>],
    ) {
        self.write(table, m, key, source, |w| {
            for i in KEPT {
                w.blob(sections.get(i).map_or(&[][..], AsRef::as_ref));
            }
        });
    }

    pub(crate) fn iface(
        &self,
        table: &ModuleTable,
        u: usize,
        own: Hash128,
    ) -> Option<(Hash128, Vec<CacheSection>)> {
        let (old, at) = self.row(table, self.modules + u)?;
        let row = &old.rows[at];
        if row.2 != own {
            return None;
        }
        let sections = sections_in(&old.bytes, &row.4)?;
        (sections.len() == 10).then_some((row.3, sections))
    }

    pub(crate) fn header(
        &self,
        table: &ModuleTable,
        u: usize,
        key: Hash128,
    ) -> Option<Vec<CacheSection>> {
        let (old, at) = self.row(table, self.modules + self.units + u)?;
        let row = &old.rows[at];
        (row.2 == key)
            .then(|| sections_in(&old.bytes, &row.4))
            .flatten()
    }

    pub(crate) fn keeps_interfaces(&self, table: &ModuleTable, u: usize) -> bool {
        self.sets[usize::from(table.units[u].package) * 2 + 1].is_some()
    }

    pub(crate) fn record_iface(
        &self,
        table: &ModuleTable,
        u: usize,
        (own, key): (Hash128, Hash128),
        reads: &[u8],
        sections: &[impl AsRef<[u8]>],
    ) {
        self.write(table, self.modules + u, own, key, |w| {
            w.blob(reads);
            for section in sections {
                w.blob(section.as_ref());
            }
        });
    }

    pub(crate) fn record_header(
        &self,
        table: &ModuleTable,
        u: usize,
        key: Hash128,
        sections: &[impl AsRef<[u8]>],
    ) {
        self.write(table, self.modules + self.units + u, key, Hash128(0), |w| {
            for section in sections {
                w.blob(section.as_ref());
            }
        });
    }

    /// Keep unchanged rows and unlisted partial-set rows; publish one record.
    /// The per-package row index avoids a package-by-all-rows nested scan.
    pub(crate) fn finish(&self, table: &ModuleTable) {
        for (p, set) in self.sets.iter().enumerate() {
            let Some(set) = set else { continue };
            let old = self.old(set);
            let mut kept = vec![false; old.map_or(0, |o| o.rows.len())];
            let (mut rows, mut changed): (Vec<Written<'_>>, bool) = (Vec::new(), false);
            for &index in &self.packages[p] {
                let (kind, _, path) = self.slot(table, index);
                let prior = self
                    .row(table, index)
                    .map(|(o, at)| (o.written(&o.rows[at]), at));
                if let Some((_, at)) = prior {
                    kept[at] = true;
                }
                if let Some((key, source, sections)) = self.made[index].get() {
                    changed |=
                        prior.is_none_or(|(r, _)| (r.2, r.3) != (*key, *source) || r.4 != sections);
                    rows.push((kind, path, *key, *source, sections));
                } else if let Some((r, _)) = prior {
                    rows.push(r);
                }
            }
            if let Some(o) = old {
                let mut left = o.rows.iter().zip(&kept).filter(|(_, k)| !**k);
                if set.partial {
                    rows.extend(left.map(|(r, _)| o.written(r)));
                } else {
                    changed |= left.next().is_some();
                }
            }
            if !changed {
                continue;
            }
            rows.sort_stable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
            let mut w = Writer::default();
            w.len_of(&rows);
            for (kind, path, key, source, sections) in &rows {
                w.u8(*kind as u8);
                w.blob(path);
                w.hash(*key);
                w.hash(*source);
                w.blob(sections);
            }
            set.at.write(self.store, set.kind, &w.bytes);
        }
    }
}
