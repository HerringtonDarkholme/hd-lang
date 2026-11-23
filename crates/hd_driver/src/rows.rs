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
