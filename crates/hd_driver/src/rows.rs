//! Per-module rows of a source set's last-run record, one record per run
//! kind (#1801 option 2, `crates/hd_cache/DESIGN.md` §5.5). A row holds a
//! module's last check key, the hash of the text it checked, and the
//! small sections of its `check` entry a check run reads ([`KEPT`]). A
//! module whose key and text match its row takes the row and reads no
//! entry, as rustc's previous dep graph keeps each node's fingerprint and
//! loads its result only when asked. A module a later step reads whole
//! gets no row (`Run::record_row`), so an evicted entry is an ordinary
//! miss. Cost per run: one record read, O(n) bytes for n files; a binary
//! search per module; a write, O(n) bytes, only when a row changed.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, SliceSort as _};
use hd_cache::CacheStore;
use hd_project::ModuleTable;

use crate::{CacheSection, ResultKey};

/// The `check` entry's sections a row holds, by their place in the entry:
/// diagnostics, meta, test registrations, init summary, `dbg` references
/// (`hd lint` reads a fetched dependency's) and generic edges.
const KEPT: [usize; 6] = [0, 1, 4, 5, 6, 8];

/// One row of a read record: where its path is, its key and text hash, and
/// where its sections are, written as blobs.
struct Row(Range<usize>, Hash128, Hash128, Range<usize>);

/// A row as written: path, key, text hash and sections.
type Written<'r> = (&'r [u8], Hash128, Hash128, &'r [u8]);

/// A record as read: its bytes and its rows, sorted by path on write.
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
        let n = r.count();
        let rows = (0..n)
            .map(|_| Row(blob(&mut r), r.hash(), r.hash(), blob(&mut r)))
            .collect();
        (r.ok() && r.at_end()).then_some(Old { bytes, rows })
    }

    fn written(&self, row: &Row) -> Written<'_> {
        (
            &self.bytes[row.0.clone()],
            row.1,
            row.2,
            &self.bytes[row.3.clone()],
        )
    }
}

/// A source set's record, stored under the set's identity and the run
/// kind; whether the run lists only part of the set
/// (`SourceSet::partial`); and its rows once read.
struct Set {
    at: ResultKey,
    kind: Hash128,
    partial: bool,
    old: OnceLock<Option<Old>>,
}

/// The rows of every source set with an identity, in a check run.
pub(crate) struct ModuleRows<'a> {
    store: &'a dyn CacheStore,
    /// Per package: its set's record.
    sets: Vec<Option<Set>>,
    /// Per module: the row this run made from its entry.
    made: Vec<OnceLock<(Hash128, Hash128, Vec<u8>)>>,
    /// Per module: its row's place in the record, found on first use.
    found: Vec<OnceLock<Option<usize>>>,
}

impl<'a> ModuleRows<'a> {
    /// `kind` names the run kind, `None` for a run that keeps no rows;
    /// `identities` holds each package's set identity and partiality.
    pub(crate) fn new(
        store: &'a dyn CacheStore,
        toolchain: Hash128,
        identities: &[Option<(Hash128, bool)>],
        kind: Option<Hash128>,
        modules: usize,
    ) -> Self {
        let set = |&(key, partial): &(Hash128, bool), kind: Hash128| Set {
            at: ResultKey { toolchain, key },
            kind,
            partial,
            old: OnceLock::new(),
        };
        ModuleRows {
            store,
            sets: identities
                .iter()
                .map(|id| Some(set(id.as_ref()?, kind?)))
                .collect(),
            made: (0..modules).map(|_| OnceLock::new()).collect(),
            found: (0..modules).map(|_| OnceLock::new()).collect(),
        }
    }

    fn old<'s>(&self, set: &'s Set) -> Option<&'s Old> {
        set.old.get_or_init(|| Old::read(self.store, set)).as_ref()
    }

    /// Module `m`'s row: its set's record and the row's place in it.
    fn row(&self, table: &ModuleTable, m: usize) -> Option<(&Old, usize)> {
        let old = self.old(self.sets[usize::from(table.modules[m].package)].as_ref()?)?;
        let path = table.sources[m].1.as_bytes();
        let find = || {
            old.rows
                .binary_search_by(|r| old.bytes[r.0.clone()].cmp(path))
                .ok()
        };
        Some((old, (*self.found[m].get_or_init(find))?))
    }

    /// Module `m`'s row, when its last check had key `key` and read a text
    /// with hash `source`: the entry's sections in their places, empty
    /// where the row holds none.
    pub(crate) fn get(
        &self,
        table: &ModuleTable,
        m: usize,
        key: Hash128,
        source: Hash128,
    ) -> Option<Vec<CacheSection>> {
        let (old, at) = self.row(table, m)?;
        let Row(_, k, s, ref range) = old.rows[at];
        if (k, s) != (key, source) {
            return None;
        }
        let at = |range| CacheSection {
            bytes: Arc::clone(&old.bytes),
            range,
        };
        let mut sections = vec![at(0..0); KEPT[KEPT.len() - 1] + 1];
        let mut r = Reader::new(&old.bytes[range.clone()]);
        for i in KEPT {
            let start = range.start + r.pos() + 4;
            sections[i] = at(start..start + r.blob().len());
        }
        (r.ok() && r.at_end()).then_some(sections)
    }

    /// Whether module `m`'s row was made from a text with hash `source`:
    /// a row is made only from a stored entry, whose text parsed.
    pub(crate) fn parsed(&self, table: &ModuleTable, m: usize, source: Hash128) -> bool {
        self.row(table, m)
            .is_some_and(|(old, at)| old.rows[at].2 == source)
    }

    /// Makes module `m`'s row from the sections of its `check` entry under
    /// `key`, checked from a text with hash `source`.
    pub(crate) fn record(
        &self,
        table: &ModuleTable,
        m: usize,
        key: Hash128,
        source: Hash128,
        sections: &[impl AsRef<[u8]>],
    ) {
        if self.sets[usize::from(table.modules[m].package)].is_some() {
            let mut w = Writer::default();
            for i in KEPT {
                w.blob(sections.get(i).map_or(&[][..], AsRef::as_ref));
            }
            let _ = self.made[m].set((key, source, w.bytes));
        }
    }

    /// After the run: writes each set's record again when a row was made
    /// that it lacked, or a file it holds is gone. A module this run did
    /// not check keeps its old row; a partial listing keeps the old rows of
    /// the files it does not list. Cost: O(n) bytes, on a run that changed
    /// a row.
    pub(crate) fn finish(&self, table: &ModuleTable) {
        for (p, set) in self.sets.iter().enumerate() {
            let Some(set) = set else { continue };
            let old = self.old(set);
            let mut kept = vec![false; old.map_or(0, |o| o.rows.len())];
            let (mut rows, mut changed): (Vec<Written<'_>>, bool) = (Vec::new(), false);
            for m in
                (0..table.modules.len()).filter(|&m| usize::from(table.modules[m].package) == p)
            {
                let prior = self
                    .row(table, m)
                    .map(|(o, at)| (o.written(&o.rows[at]), at));
                if let Some((_, at)) = prior {
                    kept[at] = true;
                }
                if let Some((key, source, sections)) = self.made[m].get() {
                    changed |= prior.is_none_or(|(r, _)| (r.1, r.2) != (*key, *source));
                    rows.push((table.sources[m].1.as_bytes(), *key, *source, sections));
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
            rows.sort_stable_by(|a, b| a.0.cmp(b.0));
            let mut w = Writer::default();
            w.len_of(&rows);
            for (path, key, source, sections) in &rows {
                w.blob(path);
                w.hash(*key);
                w.hash(*source);
                w.blob(sections);
            }
            set.at.write(self.store, set.kind, &w.bytes);
        }
    }
}

/// Per-unit rows of a source set's last-run record, beside
/// [`ModuleRows`] and in the same format, under their own record kind. A
/// row holds a unit's `reads_key`, its interface key and its interface
/// entry's sections with the read list, and, once its header check ran,
/// the header key and the findings' sections. An unchanged unit, whose
/// `reads_key` and read answers give the row's interface key, takes its
/// interface from the row; its header check takes the findings when the
/// key matches. So a warm run reads no entry for an unchanged unit, as
/// rustc's previous dep graph keeps each node's result fingerprint.
/// Cost per run: one record read, O(bytes of the set's interfaces), the
/// bytes the entries would have been read for; a binary search per unit;
/// a write, O(those bytes), only when a row changed.
pub(crate) struct UnitRows<'a> {
    store: &'a dyn CacheStore,
    /// Per package: its set's record.
    sets: Vec<Option<Set>>,
    /// Per unit: the interface part this run made: `reads_key`, interface
    /// key, and the read list and sections, written as blobs.
    iface: Vec<OnceLock<(Hash128, Hash128, Vec<u8>)>>,
    /// Per unit: the header part this run made: key and sections.
    header: Vec<OnceLock<(Hash128, Vec<u8>)>>,
    /// Per unit: its row's place in the record, found on first use.
    found: Vec<OnceLock<Option<usize>>>,
}

/// A unit row's sections blob: the interface part, then the header part,
/// empty until a header check recorded one.
fn unit_parts(bytes: &[u8], range: &Range<usize>) -> Option<(Range<usize>, Range<usize>)> {
    let mut r = Reader::new(&bytes[range.clone()]);
    let part = |r: &mut Reader<'_>| {
        let at = range.start + r.pos() + 4;
        at..at + r.blob().len()
    };
    let (iface, header) = (part(&mut r), part(&mut r));
    (r.ok() && r.at_end()).then_some((iface, header))
}

/// The blobs of `range` in `bytes`, as sections that keep the record.
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

impl<'a> UnitRows<'a> {
    /// As [`ModuleRows::new`], over `units` units.
    pub(crate) fn new(
        store: &'a dyn CacheStore,
        toolchain: Hash128,
        identities: &[Option<(Hash128, bool)>],
        kind: Option<Hash128>,
        units: usize,
    ) -> Self {
        let kind = kind.map(|k| {
            let mut h = hd_base::StableHasher::new("unit-rows-v1");
            h.hash(k);
            h.finish()
        });
        let set = |&(key, partial): &(Hash128, bool), kind: Hash128| Set {
            at: ResultKey { toolchain, key },
            kind,
            partial,
            old: OnceLock::new(),
        };
        UnitRows {
            store,
            sets: identities
                .iter()
                .map(|id| Some(set(id.as_ref()?, kind?)))
                .collect(),
            iface: (0..units).map(|_| OnceLock::new()).collect(),
            header: (0..units).map(|_| OnceLock::new()).collect(),
            found: (0..units).map(|_| OnceLock::new()).collect(),
        }
    }

    fn old<'s>(&self, set: &'s Set) -> Option<&'s Old> {
        set.old.get_or_init(|| Old::read(self.store, set)).as_ref()
    }

    /// Unit `u`'s row: its set's record and the row's place in it.
    fn row(&self, table: &ModuleTable, u: usize) -> Option<(&Old, usize)> {
        let unit = &table.units[u];
        let old = self.old(self.sets[usize::from(unit.package)].as_ref()?)?;
        let path = unit.path.as_bytes();
        let find = || {
            old.rows
                .binary_search_by(|r| old.bytes[r.0.clone()].cmp(path))
                .ok()
        };
        Some((old, (*self.found[u].get_or_init(find))?))
    }

    /// Unit `u`'s interface part, when its last interface was built with
    /// `reads_key` `own`: the interface key, then the read list and the
    /// entry's sections.
    pub(crate) fn iface(
        &self,
        table: &ModuleTable,
        u: usize,
        own: Hash128,
    ) -> Option<(Hash128, Vec<CacheSection>)> {
        let (old, at) = self.row(table, u)?;
        let Row(_, o, key, ref range) = old.rows[at];
        if o != own {
            return None;
        }
        let (iface, _) = unit_parts(&old.bytes, range)?;
        Some((key, sections_in(&old.bytes, &iface)?))
    }

    /// Unit `u`'s header findings' sections, when its last header check
    /// had key `key`.
    pub(crate) fn header(
        &self,
        table: &ModuleTable,
        u: usize,
        key: Hash128,
    ) -> Option<Vec<CacheSection>> {
        let (old, at) = self.row(table, u)?;
        let (_, header) = unit_parts(&old.bytes, &old.rows[at].3)?;
        let mut r = Reader::new(&old.bytes[header.clone()]);
        if header.is_empty() || r.hash() != key || !r.ok() {
            return None;
        }
        let start = header.start + r.pos();
        sections_in(&old.bytes, &(start..header.end))
    }

    /// Makes unit `u`'s interface part: its `reads_key` `own`, interface
    /// key `key`, the encoded read list and the entry's sections.
    pub(crate) fn record_iface(
        &self,
        table: &ModuleTable,
        u: usize,
        (own, key): (Hash128, Hash128),
        reads: &[u8],
        sections: &[impl AsRef<[u8]>],
    ) {
        if self.sets[usize::from(table.units[u].package)].is_some() {
            let mut w = Writer::default();
            w.blob(reads);
            for s in sections {
                w.blob(s.as_ref());
            }
            let _ = self.iface[u].set((own, key, w.bytes));
        }
    }

    /// Makes unit `u`'s header part: key `key` and the findings' sections.
    pub(crate) fn record_header(
        &self,
        table: &ModuleTable,
        u: usize,
        key: Hash128,
        sections: &[impl AsRef<[u8]>],
    ) {
        if self.sets[usize::from(table.units[u].package)].is_some() {
            let mut w = Writer::default();
            w.hash(key);
            for s in sections {
                w.blob(s.as_ref());
            }
            let _ = self.header[u].set((key, w.bytes));
        }
    }

    /// After the run: writes each set's record again when a unit made a
    /// part its row lacked, or a unit it holds is gone, as
    /// [`ModuleRows::finish`] does. A unit keeps the parts this run did
    /// not make. Cost: O(the record's bytes), on a run that changed a row.
    pub(crate) fn finish(&self, table: &ModuleTable) {
        for (p, set) in self.sets.iter().enumerate() {
            let Some(set) = set else { continue };
            let old = self.old(set);
            let mut kept = vec![false; old.map_or(0, |o| o.rows.len())];
            let mut rows: Vec<(&[u8], Hash128, Hash128, Vec<u8>)> = Vec::new();
            let mut changed = false;
            for u in (0..table.units.len()).filter(|&u| usize::from(table.units[u].package) == p) {
                let prior = self.row(table, u).and_then(|(o, at)| {
                    kept[at] = true;
                    let Row(_, own, key, ref range) = o.rows[at];
                    let (iface, header) = unit_parts(&o.bytes, range)?;
                    Some((own, key, &o.bytes[iface], &o.bytes[header]))
                });
                let made_iface = self.iface[u].get();
                let made_header = self.header[u].get();
                let iface = match (made_iface, prior) {
                    (Some((own, key, b)), _) => Some((*own, *key, b.as_slice())),
                    (None, Some((own, key, b, _))) => Some((own, key, b)),
                    (None, None) => None,
                };
                let Some((own, key, iface)) = iface else {
                    continue;
                };
                let header = match (made_header, prior) {
                    (Some((_, b)), _) => b.as_slice(),
                    (None, Some((_, _, _, h))) => h,
                    (None, None) => &[],
                };
                changed |= prior
                    .is_none_or(|(o, k, i, h)| (o, k) != (own, key) || i != iface || h != header);
                let mut w = Writer::default();
                w.blob(iface);
                w.blob(header);
                rows.push((table.units[u].path.as_bytes(), own, key, w.bytes));
            }
            if let Some(o) = old {
                let mut left = o.rows.iter().zip(&kept).filter(|(_, k)| !**k);
                if set.partial {
                    for (r, _) in left {
                        let (path, own, key, sections) = o.written(r);
                        rows.push((path, own, key, sections.to_vec()));
                    }
                } else {
                    changed |= left.next().is_some();
                }
            }
            if !changed {
                continue;
            }
            rows.sort_stable_by(|a, b| a.0.cmp(b.0));
            let mut w = Writer::default();
            w.len_of(&rows);
            for (path, own, key, sections) in &rows {
                w.blob(path);
                w.hash(*own);
                w.hash(*key);
                w.blob(sections);
            }
            set.at.write(self.store, set.kind, &w.bytes);
        }
    }
}
