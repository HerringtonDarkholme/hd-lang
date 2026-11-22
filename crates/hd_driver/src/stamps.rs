//! Content stamps for the files a set lists by their metadata (#1801
//! draft 44, `crates/hd_cache/DESIGN.md` §5.5). A metadata stamp changes
//! when a file is saved with the same bytes, and a file written within
//! the settle window has none. Every key and record of a run takes the
//! hash of the file's text instead, found through the set's stamp table:
//! per settled file, its metadata stamp and the hash of the text read
//! under it. A file the table holds under its listed stamp is not read; a
//! racy, new or changed one is read once, as git, Zig and TypeScript
//! compare a suspect file's content, not its time.

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, HashMap, SliceSort as _};
use hd_cache::CacheStore;
use hd_project::{ModuleTable, SourceSet};

use crate::ResultKey;
use crate::texts::Texts;

/// Per path, the metadata stamp and the content stamp read under it.
type Rows = HashMap<String, (Hash128, Hash128)>;

/// The table's client hash under the set's key (`ResultKey`).
const TABLE: Hash128 = Hash128(0x5354_414d_5053);

fn read(store: &dyn CacheStore, at: ResultKey) -> Option<Rows> {
    let bytes = at.read(store, TABLE)?;
    let mut r = Reader::new(&bytes);
    let n = r.count();
    let mut rows = HashMap::with_capacity_and_hasher(n, hd_base::FixedState::default());
    for _ in 0..n {
        let path = r.str().to_owned();
        let meta = r.hash();
        rows.insert(path, (meta, r.hash()));
    }
    (r.ok() && r.at_end()).then_some(rows)
}

fn write(store: &dyn CacheStore, at: ResultKey, rows: &Rows) {
    let mut sorted: Vec<_> = rows.iter().collect();
    sorted.sort_stable_by(|a, b| a.0.cmp(b.0));
    let mut w = Writer::default();
    w.len_of(&sorted);
    for (path, (meta, content)) in sorted {
        w.str(path);
        w.hash(*meta);
        w.hash(*content);
    }
    at.write(store, TABLE, &w.bytes);
}

/// Gives each file of a metadata-stamped set its content stamp, reading
/// through `texts` only the files its table does not vouch for, and
/// writes a table that changed. Cost: one table read per set, a hash lookup per file, and a
/// read and hash per racy, new or changed file.
pub(crate) fn resolve(
    table: &mut ModuleTable,
    sets: &[&dyn SourceSet],
    texts: &Texts<'_>,
    store: &dyn CacheStore,
    (toolchain, executor): (Hash128, crate::Executor),
) {
    struct Set {
        at: ResultKey,
        old: Option<Rows>,
        new: Rows,
        changed: bool,
    }
    let mut state: Vec<Option<Set>> = sets
        .iter()
        .map(|s| {
            let id = s.identity().filter(|_| s.metadata_stamps())?;
            let at = ResultKey { toolchain, key: id };
            let old = read(store, at);
            let new = match &old {
                Some(rows) if s.partial() => rows.clone(),
                _ => Rows::default(),
            };
            Some(Set {
                at,
                old,
                new,
                changed: false,
            })
        })
        .collect();
    // Per module with a table: the content it holds under the listed
    // stamp, if any. The others are read and hashed on the run's threads,
    // then taken in file order.
    let held: Vec<Option<Option<Hash128>>> = (0..table.stamps.len())
        .map(|m| {
            let (package, path) = &table.sources[m];
            let set = state.get(usize::from(*package))?.as_ref()?;
            let row = set.old.as_ref().and_then(|rows| rows.get(path));
            Some(
                row.filter(|(at, _)| table.stamps[m] == Some(*at))
                    .map(|r| r.1),
            )
        })
        .collect();
    let unread: Vec<usize> = (0..held.len()).filter(|&m| held[m] == Some(None)).collect();
    let mut read = crate::par_files(executor, &unread, |m| {
        texts
            .get(m)
            .map(|text| hd_project::content_stamp(text.as_bytes()))
    })
    .into_iter();
    for (m, held) in held.into_iter().enumerate() {
        let (package, path) = &table.sources[m];
        let (Some(held), Some(Some(set))) = (held, state.get_mut(usize::from(*package))) else {
            continue;
        };
        let listed = table.stamps[m];
        let content = if let Some(content) = held {
            content
        } else {
            let Some(content) = read.next().flatten() else {
                continue;
            };
            // A racy file gets no row: it is read again until it settles,
            // as git's racily clean entries are.
            set.changed |= listed.is_some();
            content
        };
        if let Some(meta) = listed {
            set.new.insert(path.clone(), (meta, content));
        }
        table.stamps[m] = Some(content);
    }
    for set in state.iter().flatten() {
        let dropped = set
            .old
            .as_ref()
            .is_some_and(|old| old.len() > set.new.len());
        if set.changed || dropped {
            write(store, set.at, &set.new);
        }
    }
}
