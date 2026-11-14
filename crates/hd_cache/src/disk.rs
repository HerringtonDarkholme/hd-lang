//! The disk store (feature `disk`): ordinary data in immutable per-run
//! packs and a shared index, sharded by key, under `<root>/pack-v2/`; private native
//! code remains in `<root>/<kind>/<shard>/<key>`. See DESIGN.md §5.4 and
//! §5.7.
//! Only this module touches the file system (design-overview §2.2 rule 1).
//!
//! Pack readers take no lock. A pending pack's file lock leases it while
//! its writer runs; the writer lock covers index commits only, never a
//! directory walk or a pack copy. Every addressed range remains an
//! unchanged HDCE frame. Last-use marks are per entry, at most once an
//! hour, and compaction preserves them. The index and physical pack bytes
//! count toward the cap.
//! A command's settlement is O(entries written and marked) and never
//! walks (#1932). A total over the cap or unknown asks for a collection,
//! which runs at most once per [`GC_INTERVAL`] under `<root>/gc`'s lock,
//! detached when the caller installs a spawner, and evicts to 90%.
//! GC follows no links and deletes only recognized managed file names.
//! The legacy shard ledger and use marks still serve private Clpack data;
//! obsolete ordinary loose files are retired by that same bounded scanner.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use hd_base::Hash128;

use crate::store::{CacheStore, EntryKind, EntryStat, plan_eviction, shard_of};

/// What this thread's stores did to the file system, for the cost tests
/// (Cost Model: a warm run's work does not grow with the cache). Counted
/// in test builds only; elsewhere [`count!`] expands to nothing.
#[cfg(test)]
mod cost {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) struct Counts {
        /// `read_dir` calls.
        pub dirs_listed: u64,
        /// Entries those calls yielded.
        pub entries_listed: u64,
        /// Bytes read from the packed index, headers included.
        pub index_read: u64,
        /// Bytes written to the packed index.
        pub index_written: u64,
        /// Pack files opened for reading.
        pub packs_opened: u64,
        /// Walks of the whole pack namespace: a collection makes two.
        pub walks: u64,
        /// Times a shard's writer lock was taken.
        pub writer_locks: u64,
        /// Times one was taken only after waiting for another holder.
        pub lock_waits: u64,
    }

    thread_local! {
        static COUNTS: Cell<Counts> = Cell::new(Counts::default());
    }

    pub(super) fn add(f: impl FnOnce(&mut Counts)) {
        COUNTS.with(|c| {
            let mut counts = c.get();
            f(&mut counts);
            c.set(counts);
        });
    }

    /// This thread's counts since the last call, which resets them.
    pub(super) fn take() -> Counts {
        COUNTS.with(Cell::take)
    }
}

/// Adds `n` to this thread's cost counter `field` in a test build.
macro_rules! count {
    ($field:ident, $n:expr) => {
        #[cfg(test)]
        cost::add(|c| c.$field += u64::try_from($n).unwrap_or(u64::MAX));
    };
}

/// The default size cap of the compiled entries, 10 GiB (`cli.cache.obj.cap`).
pub const DEFAULT_CAP: u64 = 10 << 30;

/// How many legacy shards that writes put over their budget one command
/// cleans; later commands clean the rest (#1932). A count, not a time.
const SETTLE_SHARDS: usize = 1;

/// How often automatic collection may run (#1932), as cargo's automatic
/// cache GC runs at most once per `gc.auto.frequency`. A smaller cap than
/// the last collection's collects again sooner.
pub const GC_INTERVAL: Duration = Duration::from_hours(1);

/// The last collection's start, in Unix seconds, and its cap, under
/// `<root>/gc`, whose lock one collector holds.
const GC: &str = "gc";
const GC_MAGIC: &[u8; 8] = b"HDGCRUN1";
const GC_LEN: usize = 24;

/// An entry used again within this age keeps its mtime.
const USE_MARK_AGE: Duration = Duration::from_hours(1);

const SHARDS: usize = 256;
const STATS: &str = "stats";
const STATS_MAGIC: &[u8; 8] = b"HDSTATS2";
const STATS_LEN: usize = 16 + SHARDS * 8;
/// A shard size the ledger does not know yet.
const UNKNOWN: u64 = u64::MAX;

/// The disk store over one `obj` directory.
pub struct DiskStore {
    root: PathBuf,
    packed: packed::Store,
    cap: u64,
    used: AtomicBool,
    /// Set by the first failed publish; the store writes nothing after it.
    write_failed: AtomicBool,
    /// Told of the first failed publish, with the store's root (§5.4).
    on_write_failure: Option<fn(&Path, &std::io::Error)>,
    /// Asked to collect `root` elsewhere when a collection is due; without
    /// one the dropping store collects itself.
    on_collection_due: Option<fn(&Path)>,
}

/// What [`DiskStore::collect`] found and left, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Collected {
    pub before: u64,
    pub after: u64,
}

/// One indexed data entry, as [`DiskStore::index_entries`] reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedEntry {
    pub kind: EntryKind,
    pub key: Hash128,
    /// The pack file that holds it.
    pub pack: PathBuf,
    /// The pack's length when the index was read, `None` when it was
    /// missing; the pack may go after the publication lock is released.
    pub pack_len: Option<u64>,
    pub offset: u64,
    pub len: u64,
    /// Last use, in Unix seconds.
    pub used: u64,
}

impl DiskStore {
    /// A store over `root` with the default cap.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            packed: packed::Store::new(&root),
            root,
            cap: DEFAULT_CAP,
            used: AtomicBool::new(false),
            write_failed: AtomicBool::new(false),
            on_write_failure: None,
            on_collection_due: None,
        }
    }

    /// The same store, calling `spawn` with its root, instead of
    /// collecting itself, when its drop finds a collection due. The CLI
    /// starts a detached `hd cache gc` this way, so no command waits for
    /// a walk (#1932).
    #[must_use]
    pub fn on_collection_due(mut self, spawn: fn(&Path)) -> Self {
        self.on_collection_due = Some(spawn);
        self
    }

    /// The same store, calling `warn` on its first failed publish, such as
    /// one to a read-only or full disk. The run continues uncached
    /// (`crates/hd_cache/DESIGN.md` §5.4).
    #[must_use]
    pub fn on_write_failure(mut self, warn: fn(&Path, &std::io::Error)) -> Self {
        self.on_write_failure = Some(warn);
        self
    }

    /// The same store with the size cap `cap` in bytes.
    #[must_use]
    pub fn with_cap(mut self, cap: u64) -> Self {
        self.cap = cap;
        self
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A legacy/private entry's path; ordinary data is addressed by the
    /// packed index, so a logical data entry has no independent file path.
    #[must_use]
    pub fn path(&self, kind: EntryKind, key: &Hash128) -> PathBuf {
        self.root
            .join(kind.as_str())
            .join(format!("{:02x}", shard_of(*key)))
            .join(format!("{:032x}", key.0))
    }

    fn budget(&self) -> u64 {
        self.cap / SHARDS as u64
    }

    /// Every indexed data entry with its pack's length, both read under
    /// the publication lock. It reads the whole index: for tests and tools,
    /// never a command.
    #[doc(hidden)]
    #[must_use]
    pub fn index_entries(&self) -> Vec<IndexedEntry> {
        self.packed.entries()
    }

    /// Sets an indexed entry's last use to `unix_secs`, as a hit then
    /// would have; false when it is not indexed. For tests.
    #[doc(hidden)]
    pub fn set_last_use(&self, kind: EntryKind, key: &Hash128, unix_secs: u64) -> bool {
        self.packed.set_used(kind, key, unix_secs).unwrap_or(false)
    }

    /// `hd cache gc` (`cli.cache.gc`): collects now, with no time bound,
    /// after any collection another process is running, and writes the
    /// exact shard sizes to the ledger.
    pub fn collect(&self) -> std::io::Result<Collected> {
        let claim = self.gc_file().and_then(|file| {
            file.lock().ok()?;
            Some(file)
        });
        if let Some(mut file) = claim {
            self.claim(&mut file);
            self.collect_now(false)
        } else {
            self.collect_now(false)
        }
    }

    /// The collection: every legacy shard, then the packed store under
    /// the cap the legacy bytes leave. Automatic collection tolerates an
    /// unreadable ledger, counting the legacy bytes it knows; `hd cache
    /// gc` reports it before reclaiming packed data.
    fn collect_now(&self, automatic: bool) -> std::io::Result<Collected> {
        let loose = match self.clean(&(0..=u8::MAX).collect::<Vec<_>>()) {
            Ok(loose) => loose,
            Err(_) if automatic => {
                let known = self.known_loose();
                Collected {
                    before: known,
                    after: known,
                }
            }
            Err(error) => return Err(error),
        };
        Ok(
            match self.packed.collect(self.cap.saturating_sub(loose.after)) {
                Ok(packed) => Collected {
                    before: loose.before.saturating_add(packed.before),
                    after: loose.after.saturating_add(packed.after),
                },
                // A collection that fails reports the bytes it left; one
                // that cannot take the writer lock removed none.
                Err(error) => {
                    self.warn(&error);
                    let left = loose.after.saturating_add(self.packed.bytes());
                    Collected {
                        before: left.max(loose.before),
                        after: left,
                    }
                }
            },
        )
    }

    /// `<root>/gc`, opened for its lock; `None` when the root is no
    /// directory, so a collection of a missing cache creates nothing.
    fn gc_file(&self) -> Option<File> {
        if !self.root.is_dir() {
            return None;
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(GC))
            .ok()
    }

    /// Whether the last collection, as `<root>/gc` holds it, still covers
    /// this store: it started less than [`GC_INTERVAL`] ago, under a cap
    /// no smaller than this one. A missing, torn or foreign record, or one
    /// from the future, does not.
    fn covered(&self, mut file: impl Read) -> bool {
        let mut bytes = Vec::with_capacity(GC_LEN);
        if file
            .by_ref()
            .take(u64::try_from(GC_LEN + 1).expect("small"))
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() != GC_LEN
            || &bytes[..8] != GC_MAGIC
        {
            return false;
        }
        let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
        let (at, cap) = (word(8), word(16));
        let now = unix_now();
        at <= now && now - at < GC_INTERVAL.as_secs() && self.cap >= cap
    }

    /// Records that a collection with this store's cap starts now.
    fn claim(&self, file: &mut File) {
        let mut bytes = Vec::with_capacity(GC_LEN);
        bytes.extend_from_slice(GC_MAGIC);
        bytes.extend_from_slice(&unix_now().to_le_bytes());
        bytes.extend_from_slice(&self.cap.to_le_bytes());
        let _ = file
            .seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(&bytes))
            .and_then(|()| file.set_len(GC_LEN as u64));
    }

    /// Automatic collection (#1932): at most once per [`GC_INTERVAL`], or
    /// sooner under a smaller cap, and never waiting for another process
    /// that is collecting. With a spawner installed it only asks for one,
    /// so the command itself never walks.
    fn collect_if_due(&self) {
        // cost: one 24-byte read, only when the total is over or unknown.
        if std::fs::File::open(self.root.join(GC)).is_ok_and(|file| self.covered(file)) {
            return;
        }
        if let Some(spawn) = self.on_collection_due {
            // cost: one detached process per interval at most; the
            // command does not wait for it.
            spawn(&self.root);
            return;
        }
        self.collect_due();
    }

    /// Collects when no other process is collecting and no collection
    /// covers this store; what a detached `hd cache gc` runs for a command
    /// that asked for one.
    pub fn collect_due(&self) {
        let Some(mut file) = self.gc_file() else {
            return;
        };
        if file.try_lock().is_err() || self.covered(&mut file) {
            return;
        }
        self.claim(&mut file);
        let _ = self.collect_now(true);
    }

    /// The legacy bytes the ledger knows, unknown shards counting none.
    fn known_loose(&self) -> u64 {
        File::open(self.root.join(STATS))
            .ok()
            .and_then(|file| {
                let mut bytes = Vec::with_capacity(STATS_LEN);
                file.take(u64::try_from(STATS_LEN + 1).expect("ledger limit"))
                    .read_to_end(&mut bytes)
                    .ok()?;
                Some(bytes)
            })
            .map_or(0, |bytes| {
                Ledger::decode(&bytes)
                    .sizes
                    .into_iter()
                    .filter(|&n| n != UNKNOWN)
                    .fold(0_u64, u64::saturating_add)
            })
    }

    /// A command's legacy settlement: cleans the known shards over their
    /// budget, at most [`SETTLE_SHARDS`] of them, and says whether any shard's size
    /// is unknown, which wants a collection. A command never lists an
    /// unknown shard: only a collection does (#1932). Called on drop; a
    /// store never used does nothing.
    fn settle(&self) -> bool {
        if !self.used.load(Ordering::Relaxed) {
            return false;
        }
        let budget = self.budget();
        let mut work = Vec::new();
        let mut unknown = false;
        let snapshot = Ledger::update(&self.root, |l| {
            // The first packed publication retires ordinary loose files,
            // even when their previously recorded sizes fit the budget.
            if !l.migrated && is_real_dir(&self.root.join(packed::NAMESPACE)) {
                l.migrated = true;
                l.sizes.fill(UNKNOWN);
            }
            unknown = l.sizes.contains(&UNKNOWN);
            work.extend(
                (0..=u8::MAX)
                    .zip(l.sizes)
                    .filter(|(_, size)| *size != UNKNOWN && *size > budget)
                    .map(|(s, _)| s),
            );
        });
        if snapshot.is_some() {
            work.truncate(SETTLE_SHARDS);
            let _ = self.clean(&work);
        }
        snapshot.is_some() && unknown
    }

    /// Cleans `shards` in order, each down to 90% of its budget when it is
    /// over it, recording each one's exact size under the ledger lock.
    fn clean(&self, shards: &[u8]) -> std::io::Result<Collected> {
        let budget = self.budget();
        let kinds = EntryKind::ALL;
        let mut total = Collected {
            before: 0,
            after: 0,
        };
        if !self.root.try_exists()? {
            return Ok(total);
        }
        for &shard in shards {
            let result = Ledger::transaction(&self.root, |l| {
                if !l.migrated && is_real_dir(&self.root.join(packed::NAMESPACE)) {
                    l.migrated = true;
                    l.sizes.fill(UNKNOWN);
                }
                let result = self.clean_shard(&kinds, shard, budget);
                l.sizes[shard as usize] = match &result {
                    Ok((_, after)) => *after,
                    // An incomplete inventory must never become a known
                    // under-budget size: recovery retries this shard.
                    Err(_) => UNKNOWN,
                };
                result
            });
            let (_, result) =
                result.ok_or_else(|| std::io::Error::other("cache ledger transaction failed"))?;
            let (before, after) = result?;
            total.before = total.before.saturating_add(before);
            total.after = total.after.saturating_add(after);
        }
        Ok(total)
    }

    /// The kinds whose directory exists and is no symbolic link.
    fn kinds(&self) -> Vec<EntryKind> {
        EntryKind::ALL
            .into_iter()
            .filter(|k| is_real_dir(&self.root.join(k.as_str())))
            .collect()
    }

    /// The entries of one shard across the kind directories `kinds`, with
    /// their total size.
    fn scan(&self, kinds: &[EntryKind], shard: u8) -> std::io::Result<(u64, Vec<EntryStat>)> {
        let mut out = Vec::new();
        for &kind in kinds {
            let kind_dir = self.root.join(kind.as_str());
            if !managed_dir(&kind_dir)? {
                continue;
            }
            let dir = kind_dir.join(format!("{shard:02x}"));
            if !managed_dir(&dir)? {
                continue;
            }
            // cost: only explicit GC, unknown recovery, or a recorded
            // over-budget shard lists its entries; warm known shards do not.
            let list = std::fs::read_dir(&dir)?;
            count!(dirs_listed, 1);
            for e in list {
                let e = e?;
                count!(entries_listed, 1);
                let Some(key) = e.file_name().to_str().and_then(parse_key) else {
                    continue;
                };
                // `DirEntry::metadata` does not follow a symbolic link.
                let meta = e.metadata()?;
                if shard_of(key) != shard || !meta.file_type().is_file() {
                    continue;
                }
                out.push(EntryStat {
                    kind,
                    key,
                    bytes: meta.len(),
                    mtime_s: mtime_s(&meta),
                });
            }
        }
        Ok((out.iter().map(|e| e.bytes).sum(), out))
    }

    /// Scans one shard and evicts its oldest entries when it is over
    /// `budget`: its size before and after.
    fn clean_shard(
        &self,
        kinds: &[EntryKind],
        shard: u8,
        budget: u64,
    ) -> std::io::Result<(u64, u64)> {
        let (before, entries) = self.scan(kinds, shard)?;
        let mut after = before;
        let migrated = is_real_dir(&self.root.join(packed::NAMESPACE));
        let (obsolete, retained): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .partition(|e| migrated && e.kind != EntryKind::Clpack);
        for e in obsolete.into_iter().chain(plan_eviction(&retained, budget)) {
            // A file another process removed first, or one in use on
            // Windows, is skipped.
            if std::fs::remove_file(self.path(e.kind, &e.key)).is_ok() {
                after -= e.bytes;
            }
        }
        Ok((before, after))
    }
}

impl Drop for DiskStore {
    /// A command's settlement: O(entries written and marked), no walk
    /// (#1932). A total over the cap or unknown asks for a collection.
    fn drop(&mut self) {
        if self.packed.was_used() && (!self.kinds().is_empty() || self.root.join(STATS).is_file()) {
            self.used.store(true, Ordering::Relaxed);
        }
        let mut wanted = self.settle();
        if self.packed.was_used() {
            // The legacy ledger is approximate between collections; a
            // collection scans both namespaces and uses exact bytes.
            match self
                .packed
                .settle(self.cap.saturating_sub(self.known_loose()))
            {
                Ok(over) => wanted |= over,
                Err(error) => self.warn(&error),
            }
        }
        if wanted {
            self.collect_if_due();
        }
    }
}

impl DiskStore {
    /// Reads an entry whose open file `accept`s, marking its use. A
    /// missing, vanished, refused or unreadable entry is a miss.
    fn read(
        &self,
        kind: EntryKind,
        key: &Hash128,
        accept: impl FnOnce(&Path, &std::fs::Metadata) -> bool,
    ) -> Option<Arc<[u8]>> {
        self.used.store(true, Ordering::Relaxed);
        let path = self.path(kind, key);
        let mut file = File::open(&path).ok()?;
        let meta = file.metadata().ok()?;
        if !accept(&path, &meta) {
            return None;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
        file.read_to_end(&mut bytes).ok()?;
        if stale(&meta) {
            mark_used(&path);
        }
        Some(Arc::from(bytes))
    }

    /// Publishes an entry by write-then-rename, the temporary file made by
    /// `create`. A failed publish is reported once and turns later puts
    /// off, so the run continues uncached (§5.4).
    fn publish(
        &self,
        kind: EntryKind,
        key: &Hash128,
        bytes: &[u8],
        create: impl FnOnce(&Path) -> std::io::Result<File>,
    ) {
        /// Tells apart the temporary files of one process's threads.
        static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
        self.used.store(true, Ordering::Relaxed);
        if self.write_failed.load(Ordering::Relaxed) {
            return;
        }
        let path = self.path(kind, key);
        let tmp = path.with_extension(format!(
            "tmp{}-{}",
            std::process::id(),
            NEXT_TMP.fetch_add(1, Ordering::Relaxed)
        ));
        let publish = || {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let written = create(&tmp).and_then(|mut file| file.write_all(bytes));
            if let Err(e) = written {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            let ledger = Ledger::transaction(&self.root, |l| {
                let mut commit = || {
                    let before = match std::fs::symlink_metadata(&path) {
                        Ok(meta) if meta.file_type().is_file() => meta.len(),
                        Ok(_) => 0,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                        Err(error) => return Err(error),
                    };
                    if let Err(error) = std::fs::rename(&tmp, &path) {
                        // A lost publication race adds no bytes. On Windows
                        // an open destination may prevent replacement.
                        return if path.is_file() { Ok(()) } else { Err(error) };
                    }
                    let size = &mut l.sizes[shard_of(*key) as usize];
                    if *size != UNKNOWN {
                        *size = size
                            .saturating_sub(before)
                            .saturating_add(bytes.len() as u64);
                    }
                    Ok(())
                };
                commit()
            });
            let _ = std::fs::remove_file(&tmp);
            match ledger {
                Some((_, result)) => result,
                None => Err(std::io::Error::other("cache ledger transaction failed")),
            }
        };
        if let Err(e) = publish()
            && !self.write_failed.swap(true, Ordering::Relaxed)
            && let Some(warn) = self.on_write_failure
        {
            warn(&self.root, &e);
        }
    }

    /// Reads a private entry: one that holds native code, such as a
    /// `clpack` (docs/compiler/engines-and-test-runner.md §18.2). Only a
    /// regular file at the entry's path, no symbolic link, owned by
    /// `owner` (the reader's user ID), with one link and mode 0600 (no
    /// permission for group or others), is read. No other user can plant or change one,
    /// whoever can write the directories above it.
    #[cfg(unix)]
    pub fn get_private(&self, kind: EntryKind, key: &Hash128, owner: u32) -> Option<Arc<[u8]>> {
        use std::os::unix::fs::MetadataExt;
        self.read(kind, key, |path, meta| {
            // The path names the opened file itself, not a link to it.
            let named = std::fs::symlink_metadata(path).is_ok_and(|m| {
                m.file_type().is_file() && m.dev() == meta.dev() && m.ino() == meta.ino()
            });
            named
                && meta.file_type().is_file()
                && meta.uid() == owner
                && meta.nlink() == 1
                && meta.mode() & 0o777 == 0o600
        })
    }

    /// Publishes a private entry, readable and writable by its owner only
    /// (mode 0600). The temporary file is created new, never opened
    /// through a link.
    #[cfg(unix)]
    pub fn put_private(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        use std::os::unix::fs::OpenOptionsExt;
        self.publish(kind, key, bytes, |tmp| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(tmp)
        });
    }
}

impl CacheStore for DiskStore {
    /// A missing, corrupt, vanished or unreadable entry is a miss.
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        if kind == EntryKind::Clpack {
            self.read(kind, key, |_, _| true)
        } else {
            self.packed.get(kind, key)
        }
    }

    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        if kind == EntryKind::Clpack {
            self.publish(kind, key, bytes, |tmp| File::create(tmp));
        } else {
            self.put_many(&[(kind, *key, Arc::from(bytes))]);
        }
    }

    fn put_many(&self, entries: &[(EntryKind, Hash128, Arc<[u8]>)]) {
        if self.write_failed.load(Ordering::Relaxed) {
            return;
        }
        if entries
            .iter()
            .any(|(kind, _, _)| *kind == EntryKind::Clpack)
        {
            for (kind, key, bytes) in entries {
                self.put(*kind, key, bytes);
            }
        } else if let Err(error) = self.packed.publish(entries) {
            self.warn(&error);
        }
    }

    fn touch(&self, kind: EntryKind, key: &Hash128) {
        if kind == EntryKind::Clpack {
            self.touch_loose(kind, key);
        } else {
            self.packed.touch(kind, key);
        }
    }
}

impl DiskStore {
    #[cfg(test)]
    fn put_loose(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.publish(kind, key, bytes, |path| File::create(path));
    }
    #[cfg(test)]
    fn get_loose(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        self.read(kind, key, |_, _| true)
    }

    fn touch_loose(&self, kind: EntryKind, key: &Hash128) {
        self.used.store(true, Ordering::Relaxed);
        let path = self.path(kind, key);
        if std::fs::metadata(&path).is_ok_and(|m| stale(&m)) {
            mark_used(&path);
        }
    }

    fn warn(&self, error: &std::io::Error) {
        if !self.write_failed.swap(true, Ordering::Relaxed)
            && let Some(warn) = self.on_write_failure
        {
            warn(&self.root, error);
        }
    }
}

/// An entry whose mtime is more than [`USE_MARK_AGE`] old.
fn stale(meta: &std::fs::Metadata) -> bool {
    stale_by(meta, USE_MARK_AGE)
}

/// A file whose mtime is more than `age` old.
fn stale_by(meta: &std::fs::Metadata, age: Duration) -> bool {
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|elapsed| elapsed > age)
}

/// Sets an existing entry's mtime to now; never creates one.
fn mark_used(path: &Path) {
    if let Ok(f) = OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn mtime_s(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// Missing and foreign entries are outside the managed namespace; an
/// unreadable directory is an incomplete inventory, not an empty one.
fn managed_dir(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// A file name of exactly 32 lowercase hex digits, as [`DiskStore::path`]
/// writes it.
fn parse_key(name: &str) -> Option<Hash128> {
    (name.len() == 32 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| u128::from_str_radix(name, 16).ok().map(Hash128))
        .flatten()
}

/// The shard-size ledger `<root>/stats`: a magic, the packed-migration
/// flag, and 256 sizes, little-endian. A prior version recovers as unknown.
struct Ledger {
    migrated: bool,
    sizes: [u64; SHARDS],
}

impl Ledger {
    fn decode(bytes: &[u8]) -> Ledger {
        let mut l = Ledger {
            migrated: false,
            sizes: [UNKNOWN; SHARDS],
        };
        if bytes.len() == STATS_LEN && &bytes[..8] == STATS_MAGIC {
            let word = |i: usize| {
                u64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().expect("8 bytes"))
            };
            l.migrated = word(0) != 0;
            for (i, s) in l.sizes.iter_mut().enumerate() {
                *s = word(i + 1);
            }
        }
        l
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(STATS_LEN);
        out.extend_from_slice(STATS_MAGIC);
        out.extend_from_slice(&u64::from(self.migrated).to_le_bytes());
        for s in &self.sizes {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// Reads the ledger under an exclusive lock, applies `f`, and writes it
    /// back: the ledger as `f` left it. A missing or foreign ledger reads as
    /// every size unknown. `None` when `root` is no directory or the ledger
    /// cannot be locked; the command then leaves eviction to a later one.
    fn update(root: &Path, f: impl FnOnce(&mut Ledger)) -> Option<Ledger> {
        Self::transaction(root, f).map(|(ledger, ())| ledger)
    }

    /// File mutations and their accounting use one exclusive lock. The
    /// fixed-size invalid header is the recovery intent, committed last.
    fn transaction<R>(root: &Path, f: impl FnOnce(&mut Ledger) -> R) -> Option<(Ledger, R)> {
        // The caller-selected root may alias its cache directory (the CLI
        // shares object stores this way). Links below that root are still
        // excluded by managed_dir and private-entry ownership checks.
        if !root.is_dir() {
            return None;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(STATS))
            .ok()?;
        file.lock().ok()?;
        let mut bytes = Vec::with_capacity(STATS_LEN);
        (&mut file)
            .take(u64::try_from(STATS_LEN + 1).expect("ledger limit"))
            .read_to_end(&mut bytes)
            .ok()?;
        let mut l = Ledger::decode(&bytes);
        // Persist an invalid header before any file mutation. Process
        // interruption leaves every shard unknown, rather than silently
        // trusting a size recorded before a successful rename or removal.
        file.seek(SeekFrom::Start(0)).ok()?;
        file.write_all(&[0; 8]).ok()?;
        let result = f(&mut l);
        let out = l.encode();
        file.seek(SeekFrom::Start(8)).ok()?;
        file.write_all(&out[8..]).ok()?;
        file.set_len(out.len() as u64).ok()?;
        file.seek(SeekFrom::Start(0)).ok()?;
        file.write_all(STATS_MAGIC).ok()?;
        Some((l, result))
    }
}

/// Immutable data packs and a paged index (DESIGN.md §5.4). The index is a
/// linear hash table per shard in `pack-v2/index/<shard>/`: a fixed head holds the level and
/// split pointer, and each bucket is one page file, so a lookup reads one
/// page and a write rewrites only the pages it changes. A pack's exclusive
/// file lock leases it until publication, or until its writer dies.
mod packed {
    use hd_base::SliceSort as _;
    use std::collections::btree_map::Entry as Slot;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::{File, OpenOptions, TryLockError};
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::SystemTime;

    use hd_base::{Hash128, StableHasher};

    #[cfg(test)]
    use super::cost;
    use super::{Collected, EntryKind, IndexedEntry, USE_MARK_AGE, is_real_dir, parse_key};

    /// The namespace of this format. The index is sharded by key (#1932),
    /// so `pack-v1`, whose single index every writer locked, is retired.
    pub(super) const NAMESPACE: &str = "pack-v2";
    /// Index shards, each with its own lock, by a key's top four bits.
    const SHARDS: u8 = 16;
    const PAGE_MAGIC: &[u8; 8] = b"HDPAGE02";
    const HEAD_MAGIC: &[u8; 8] = b"HDHEAD01";
    const TABLE_MAGIC: &[u8; 8] = b"HDPKTB01";
    const FOOT_MAGIC: &[u8; 8] = b"HDFOOT01";
    const HEADER: usize = 32;
    const ROW: usize = 58;
    /// Rows per page: the 32-byte header and 70 rows fill 4,092 bytes.
    /// At 35 rows per bucket on average, a bucket not yet split this round
    /// holds up to twice that, still one page.
    const SLOTS: usize = 70;
    /// A bucket split follows when the rows exceed this many per bucket,
    /// a merge when they fall under half of it.
    const LOAD: u64 = 35;
    const MAX_LEVEL: u32 = 24;
    const HEAD_LEN: usize = 64;
    const TABLE_ROW: usize = 24;
    const FOOT_ROW: usize = 34;
    const MAX_INDEX: usize = 64 * 1024 * 1024;
    const MAX_ENTRY: u64 = 64 * 1024 * 1024;
    const MAX_HANDLES: usize = 64;
    /// Tracked packs of one power-of-two size class merge into one pack
    /// once there are this many.
    const FANOUT: usize = 4;
    /// A pack this large is never merged; compaction still reclaims it.
    /// One merge copies at most `FANOUT` times this.
    const FULL: u64 = 8 << 20;
    const LOG_MAGIC: &[u8; 8] = b"HDLOG001";
    /// A log header: magic, epoch, a head copy, the total, a checksum.
    const LOG_HEADER: usize = 112;
    /// A record batch's header: its op count and their checksum.
    const LOG_FRAME: usize = 20;
    /// One op: a tag and a page row.
    const OP: usize = 1 + ROW;
    /// A shard's log is folded into its pages once its records pass this
    /// many bytes, about one page: a reader reads at most that much more
    /// than the head it replaced.
    const FOLD_AT: u64 = 4096;
    type Key = (u16, u128);
    type Entries = BTreeMap<Key, Location>;
    type Batch = [(EntryKind, Hash128, Arc<[u8]>)];
    /// Footer rows: key, offset and length of each frame in a pack.
    type Frames = Vec<(Key, u64, u64)>;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Location {
        pack: u128,
        offset: u64,
        len: u64,
        used: u64,
    }

    struct Opened {
        file: File,
        len: u64,
    }

    /// What a reader has read: each shard's head once (`None` when it
    /// has none), and the pages under it.
    #[derive(Default)]
    struct Snapshot {
        views: BTreeMap<u8, View>,
        /// Pages read under the views' heads, by shard and bucket, with
        /// the fold epoch that wrote each.
        pages: BTreeMap<(u8, u64), (Entries, u32)>,
        files: BTreeMap<u128, Opened>,
        touched: BTreeMap<Key, u64>,
        /// Packs found missing after the index was read again: each one
        /// refreshes the view at most once per run.
        gone: BTreeSet<u128>,
    }

    pub(super) struct Store {
        root: PathBuf,
        snapshot: Mutex<Option<Snapshot>>,
        used: AtomicU64,
        /// The namespace directory was found or made.
        rooted: AtomicBool,
        /// The home shards of the packs this process published, a bit
        /// each: its settlement merges small packs there.
        published: AtomicU32,
        /// Called at a collection's walk and a rewrite's copy, so a test can
        /// act while one is in progress.
        #[cfg(test)]
        pub(super) on_pause: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
    }

    /// What a collection's walk found.
    #[derive(Default)]
    struct Inventory {
        /// Final packs and their lengths.
        packs: BTreeMap<u128, u64>,
        /// Lease files, live or crashed.
        pending: Vec<PathBuf>,
        /// `.new` lease files an interval old: a crash between creating
        /// and locking one left them.
        abandoned: Vec<PathBuf>,
        /// Crashed writers' temporary index files, by shard.
        temps: Vec<(u8, PathBuf)>,
        /// The index's page files, by shard and bucket, and their bytes.
        pages: Vec<(u8, u64)>,
        page_bytes: [u64; SHARDS as usize],
        /// Each shard's index bytes.
        index: [u64; SHARDS as usize],
        /// Managed bytes: final packs and the index's files.
        total: u64,
    }

    /// A pack a rewrite copies out of: the frames to copy, and every key
    /// whose row may name it.
    struct Source {
        id: u128,
        copy: Frames,
        keys: Vec<Key>,
    }

    /// A live writer owns the file lock before it drops the index lock.
    /// An error removes its uncommitted file; a crash releases the lease,
    /// allowing a later GC to collect the orphan without an age heuristic.
    struct Lease {
        id: u128,
        path: PathBuf,
        file: File,
        committed: bool,
    }

    impl Drop for Lease {
        fn drop(&mut self) {
            if !self.committed {
                let _ = self.file.unlock();
                // Windows may retain an open file; GC retries after close.
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    /// The index shard of a key, or the home shard of a pack ID.
    fn shard_of(key: u128) -> u8 {
        u8::try_from(key >> 124).expect("four bits")
    }

    fn invalid(why: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, why)
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn unique() -> u128 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut h = StableHasher::new("cache-storage-pack");
        h.bytes(
            &SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .to_le_bytes(),
        );
        h.u32(std::process::id());
        h.u64(NEXT.fetch_add(1, Ordering::Relaxed));
        h.finish().0
    }

    fn checksum(bytes: &[u8]) -> u128 {
        let mut h = StableHasher::new("cache-storage-index-v1");
        h.bytes(bytes);
        h.finish().0
    }

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(b[at..at + 2].try_into().expect("2 bytes"))
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
    }
    fn u64_at(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
    }
    fn u128_at(b: &[u8], at: usize) -> u128 {
        u128::from_le_bytes(b[at..at + 16].try_into().expect("16 bytes"))
    }

    fn put_row(out: &mut Vec<u8>, &(kind, key): &Key, loc: &Location) {
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&loc.pack.to_le_bytes());
        out.extend_from_slice(&loc.offset.to_le_bytes());
        out.extend_from_slice(&loc.len.to_le_bytes());
        out.extend_from_slice(&loc.used.to_le_bytes());
    }

    /// Sorted unique rows of a known kind and a bounded range.
    fn rows(bytes: &[u8]) -> io::Result<Entries> {
        let mut entries = Entries::new();
        let mut previous = None;
        for row in bytes.chunks_exact(ROW) {
            let key = (u16_at(row, 0), u128_at(row, 2));
            if !EntryKind::ALL
                .iter()
                .any(|&k| k as u16 == key.0 && k != EntryKind::Clpack)
                || previous.is_some_and(|p| p >= key)
            {
                return Err(invalid("invalid or unordered cache index key"));
            }
            let loc = Location {
                pack: u128_at(row, 18),
                offset: u64_at(row, 34),
                len: u64_at(row, 42),
                used: u64_at(row, 50),
            };
            if loc.len > MAX_ENTRY || loc.offset.checked_add(loc.len).is_none() {
                return Err(invalid("invalid cache index range"));
            }
            previous = Some(key);
            entries.insert(key, loc);
        }
        Ok(entries)
    }

    /// A 32-byte header (magic, row count, zero flags, checksum of the
    /// rows) and the rows, the layout of the legacy index and of a page.
    fn frame(magic: [u8; 8], entries: &Entries, len: usize, epoch: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&magic);
        out.extend_from_slice(
            &u32::try_from(entries.len())
                .expect("bounded rows")
                .to_le_bytes(),
        );
        out.extend_from_slice(&epoch.to_le_bytes());
        out.extend_from_slice(&[0; 16]);
        for (key, loc) in entries {
            put_row(&mut out, key, loc);
        }
        let sum = checksum(&out[HEADER..]);
        out[16..HEADER].copy_from_slice(&sum.to_le_bytes());
        out.resize(len, 0);
        out
    }

    /// The rows and the fold epoch of a page when `bytes` is a whole frame
    /// of `magic` whose length `len_of(count)` gives.
    fn unframe(
        magic: [u8; 8],
        bytes: &[u8],
        len_of: impl Fn(usize) -> Option<usize>,
    ) -> io::Result<(Entries, u32)> {
        if bytes.len() < HEADER || bytes[..8] != magic {
            return Err(invalid("invalid cache index header"));
        }
        let count = usize::try_from(u32_at(bytes, 8)).map_err(io::Error::other)?;
        let end = count
            .checked_mul(ROW)
            .and_then(|n| n.checked_add(HEADER))
            .filter(|_| len_of(count) == Some(bytes.len()))
            .ok_or_else(|| invalid("invalid cache index length"))?;
        if checksum(&bytes[HEADER..end]) != u128_at(bytes, 16) {
            return Err(invalid("invalid cache index checksum"));
        }
        Ok((rows(&bytes[HEADER..end])?, u32_at(bytes, 12)))
    }

    /// A page file's length: whole pages of [`SLOTS`] rows, at least one.
    fn page_len(count: usize) -> usize {
        HEADER + count.div_ceil(SLOTS).max(1) * SLOTS * ROW
    }

    /// A page of `entries`, stamped with the fold `epoch` that wrote it.
    fn page_encode(entries: &Entries, epoch: u32) -> Vec<u8> {
        frame(*PAGE_MAGIC, entries, page_len(entries.len()), epoch)
    }

    fn page_decode(bytes: &[u8]) -> io::Result<(Entries, u32)> {
        unframe(*PAGE_MAGIC, bytes, |n| {
            Some(page_len(n)).filter(|&l| l <= MAX_INDEX)
        })
    }

    fn page_name(bucket: u64) -> String {
        format!("{bucket:016x}")
    }

    /// The index head: level `i` and split pointer `s` of the linear hash
    /// table (buckets `2^i + s`), the row count, and the fold epoch that
    /// wrote it (in `generation`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Head {
        level: u32,
        split: u64,
        rows: u64,
        generation: u128,
    }

    impl Head {
        fn buckets(self) -> u64 {
            (1_u64 << self.level) + self.split
        }

        /// The bucket of `key`: its low `i` bits, or `i + 1` bits once
        /// that bucket has split.
        fn bucket(self, key: u128) -> u64 {
            let hash = u64::try_from(key & u128::from(u64::MAX)).expect("low 64 bits");
            let low = hash & ((1_u64 << self.level) - 1);
            if low < self.split {
                hash & ((2_u64 << self.level) - 1)
            } else {
                low
            }
        }

        fn next(self) -> Head {
            let split = self.split + 1;
            if split == 1_u64 << self.level {
                Head {
                    level: self.level + 1,
                    split: 0,
                    ..self
                }
            } else {
                Head { split, ..self }
            }
        }

        fn previous(self) -> Head {
            if self.split == 0 {
                Head {
                    level: self.level - 1,
                    split: (1_u64 << (self.level - 1)) - 1,
                    ..self
                }
            } else {
                Head {
                    split: self.split - 1,
                    ..self
                }
            }
        }

        fn encode(self) -> Vec<u8> {
            let mut out = Vec::with_capacity(HEAD_LEN);
            out.extend_from_slice(HEAD_MAGIC);
            out.extend_from_slice(&self.level.to_le_bytes());
            out.extend_from_slice(&0_u32.to_le_bytes());
            out.extend_from_slice(&self.split.to_le_bytes());
            out.extend_from_slice(&self.rows.to_le_bytes());
            out.extend_from_slice(&self.generation.to_le_bytes());
            let sum = checksum(&out);
            out.extend_from_slice(&sum.to_le_bytes());
            out
        }

        fn decode(bytes: &[u8]) -> io::Result<Head> {
            if bytes.len() != HEAD_LEN
                || &bytes[..8] != HEAD_MAGIC
                || u32_at(bytes, 12) != 0
                || checksum(&bytes[..48]) != u128_at(bytes, 48)
            {
                return Err(invalid("invalid cache index head"));
            }
            let head = Head {
                level: u32_at(bytes, 8),
                split: u64_at(bytes, 16),
                rows: u64_at(bytes, 24),
                generation: u128_at(bytes, 32),
            };
            if head.level > MAX_LEVEL || head.split >= 1_u64 << head.level {
                return Err(invalid("invalid cache index head"));
            }
            Ok(head)
        }
    }

    /// The pack table `index/packs`: every pack this index format wrote,
    /// with its length, so merging needs no directory walk.
    fn table_encode(packs: &BTreeMap<u128, u64>) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + packs.len() * TABLE_ROW);
        out.extend_from_slice(TABLE_MAGIC);
        out.extend_from_slice(
            &u32::try_from(packs.len())
                .expect("bounded packs")
                .to_le_bytes(),
        );
        out.extend_from_slice(&[0; 20]);
        for (id, len) in packs {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
        }
        let sum = checksum(&out[HEADER..]);
        out[16..HEADER].copy_from_slice(&sum.to_le_bytes());
        out
    }

    fn table_decode(bytes: &[u8]) -> io::Result<BTreeMap<u128, u64>> {
        let count = bytes
            .get(8..12)
            .map(|_| usize::try_from(u32_at(bytes, 8)).unwrap_or(usize::MAX));
        if bytes.len() < HEADER
            || &bytes[..8] != TABLE_MAGIC
            || bytes[12..16] != [0; 4]
            || count.and_then(|n| n.checked_mul(TABLE_ROW)) != Some(bytes.len() - HEADER)
            || checksum(&bytes[HEADER..]) != u128_at(bytes, 16)
        {
            return Err(invalid("invalid cache pack table"));
        }
        Ok(bytes[HEADER..]
            .chunks_exact(TABLE_ROW)
            .map(|row| (u128_at(row, 0), u64_at(row, 16)))
            .collect())
    }

    /// A pack's footer: each frame's key, offset and length, then the row
    /// count, their checksum and a magic. Readers address frames by range
    /// and never see it; merging reads it instead of the whole index.
    fn footer(frames: &Frames) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames.len() * FOOT_ROW + HEADER);
        for &((kind, key), offset, len) in frames {
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
        }
        let sum = checksum(&out);
        out.extend_from_slice(&(frames.len() as u64).to_le_bytes());
        out.extend_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(FOOT_MAGIC);
        out
    }

    fn read_footer(file: &mut File, len: u64) -> io::Result<Frames> {
        let trailer = u64::try_from(HEADER).expect("small");
        if len < trailer {
            return Err(invalid("cache pack has no footer"));
        }
        let mut tail = [0; HEADER];
        file.seek(SeekFrom::Start(len - trailer))?;
        file.read_exact(&mut tail)?;
        let count = u64_at(&tail, 0);
        let rows_len = count
            .checked_mul(u64::try_from(FOOT_ROW).expect("small"))
            .filter(|&n| n <= len - trailer && n <= MAX_ENTRY)
            .ok_or_else(|| invalid("invalid cache pack footer"))?;
        if &tail[24..] != FOOT_MAGIC {
            return Err(invalid("cache pack has no footer"));
        }
        let start = len - trailer - rows_len;
        let mut bytes = vec![0; usize::try_from(rows_len).map_err(io::Error::other)?];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut bytes)?;
        if checksum(&bytes) != u128_at(&tail, 8) {
            return Err(invalid("invalid cache pack footer"));
        }
        bytes
            .chunks_exact(FOOT_ROW)
            .map(|row| {
                let (offset, len) = (u64_at(row, 18), u64_at(row, 26));
                if offset.checked_add(len).is_none_or(|end| end > start) {
                    return Err(invalid("invalid cache pack footer range"));
                }
                Ok(((u16_at(row, 0), u128_at(row, 2)), offset, len))
            })
            .collect()
    }

    /// Reads a regular file of at most `limit` bytes; `None` when missing.
    fn read_small(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
            Ok(meta)
                if !meta.file_type().is_file()
                    || meta.len() > u64::try_from(limit).expect("limit") =>
            {
                return Err(invalid("cache index is not a bounded regular file"));
            }
            Ok(_) => {}
        }
        let mut bytes = Vec::new();
        match File::open(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
            Ok(file) => file
                .take(u64::try_from(limit + 1).expect("limit"))
                .read_to_end(&mut bytes)?,
        };
        count!(index_read, bytes.len());
        Ok(Some(bytes))
    }

    fn read_head(dir: &Path) -> io::Result<Option<Head>> {
        read_small(&dir.join("head"), HEAD_LEN)?
            .map(|b| Head::decode(&b))
            .transpose()
    }

    /// A page's rows, its file length and its fold epoch; `None` when it
    /// has no file.
    fn read_page(dir: &Path, bucket: u64) -> io::Result<Option<(Entries, u64, u32)>> {
        let Some(bytes) = read_small(&dir.join(page_name(bucket)), MAX_INDEX)? else {
            return Ok(None);
        };
        let len = bytes.len() as u64;
        let (rows, epoch) = page_decode(&bytes)?;
        Ok(Some((rows, len, epoch)))
    }

    /// Replaces `dir/name` with `bytes` by a renamed temporary file.
    fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = dir.join(format!("{name}.{:032x}.tmp", unique()));
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            file.write_all(bytes)?;
            count!(index_written, bytes.len());
            drop(file);
            std::fs::rename(&tmp, dir.join(name))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    fn remove_missing_ok(path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// One bucket as a writer holds it.
    struct Page {
        rows: Entries,
        /// The rows as read, for the superset a split writes first.
        disk: Entries,
        /// The file's length, 0 when it has none.
        len: u64,
        dirty: bool,
    }

    /// A writer's view of the index, under the publication lock. It reads
    /// the head and only the pages it touches; [`Txn::commit`] writes the
    /// changed pages, then the head, so readers taking no lock see each
    /// page whole and every key in the page the head names, or a miss.
    struct Txn {
        dir: PathBuf,
        head: Head,
        /// The head as on disk, `None` when there is none.
        stored: Option<Head>,
        pages: BTreeMap<u64, Page>,
        /// Pages a merge emptied, removed after the head commit.
        doomed: BTreeSet<u64>,
        table: Option<BTreeMap<u128, u64>>,
        table_len: u64,
        table_dirty: bool,
    }

    impl Txn {
        /// The index in `dir`. A corrupt head is derived data: the index
        /// starts again empty, and its stale pages are reclaimed by GC.
        fn open(dir: PathBuf) -> io::Result<Txn> {
            let stored = match read_head(&dir) {
                Ok(head) => head,
                Err(e) if e.kind() == io::ErrorKind::InvalidData => None,
                Err(e) => return Err(e),
            };
            Ok(Txn {
                dir,
                head: stored.unwrap_or_default(),
                stored,
                pages: BTreeMap::new(),
                doomed: BTreeSet::new(),
                table: None,
                table_len: 0,
                table_dirty: false,
            })
        }

        fn page(&mut self, bucket: u64) -> io::Result<&mut Page> {
            match self.pages.entry(bucket) {
                Slot::Occupied(page) => Ok(page.into_mut()),
                Slot::Vacant(slot) => {
                    let (mut rows, len, dirty) = match read_page(&self.dir, bucket) {
                        Ok(Some((rows, len, _))) => (rows, len, false),
                        Ok(None) => (Entries::new(), 0, false),
                        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                            let len = std::fs::symlink_metadata(self.dir.join(page_name(bucket)))
                                .map_or(0, |m| m.len());
                            (Entries::new(), len, true)
                        }
                        Err(e) => return Err(e),
                    };
                    // Rows a split moved on before a crash are no longer
                    // this bucket's.
                    let head = self.head;
                    rows.retain(|key, _| head.bucket(key.1) == bucket);
                    Ok(slot.insert(Page {
                        disk: rows.clone(),
                        rows,
                        len,
                        dirty,
                    }))
                }
            }
        }

        fn get(&mut self, key: Key) -> io::Result<Option<Location>> {
            let bucket = self.head.bucket(key.1);
            Ok(self.page(bucket)?.rows.get(&key).copied())
        }

        /// Sets `key`'s location; a new key may split a bucket.
        fn set(&mut self, key: Key, loc: Location) -> io::Result<()> {
            let bucket = self.head.bucket(key.1);
            let page = self.page(bucket)?;
            page.dirty = true;
            if page.rows.insert(key, loc).is_none() {
                self.head.rows += 1;
                while self.head.rows > LOAD * self.head.buckets() && self.head.level < MAX_LEVEL {
                    self.split()?;
                }
            }
            Ok(())
        }

        fn remove(&mut self, key: Key) -> io::Result<()> {
            let bucket = self.head.bucket(key.1);
            let page = self.page(bucket)?;
            if page.rows.remove(&key).is_some() {
                page.dirty = true;
                self.head.rows = self.head.rows.saturating_sub(1);
            }
            Ok(())
        }

        /// Splits bucket `s` into `s` and `s + 2^i`.
        fn split(&mut self) -> io::Result<()> {
            let from = self.head.split;
            let to = from + (1_u64 << self.head.level);
            self.page(from)?;
            let next = self.head.next();
            let moved = self.pages.get_mut(&from).map_or_else(Entries::new, |page| {
                page.dirty = true;
                let (stay, moved): (Entries, Entries) = std::mem::take(&mut page.rows)
                    .into_iter()
                    .partition(|(key, _)| next.bucket(key.1) == from);
                page.rows = stay;
                moved
            });
            self.doomed.remove(&to);
            // A page beyond the buckets is a crashed writer's: replace it.
            let len = match self.pages.remove(&to) {
                Some(page) => page.len,
                None => {
                    std::fs::symlink_metadata(self.dir.join(page_name(to))).map_or(0, |m| m.len())
                }
            };
            self.pages.insert(
                to,
                Page {
                    rows: moved,
                    disk: Entries::new(),
                    len,
                    dirty: true,
                },
            );
            self.head = next;
            Ok(())
        }

        /// Merges the last bucket back into its buddy while the rows fit
        /// half the load, and drops every page once there are none.
        fn shrink(&mut self) -> io::Result<()> {
            while self.head.buckets() > 1 && self.head.rows * 2 < LOAD * (self.head.buckets() - 1) {
                let previous = self.head.previous();
                let (from, to) = (self.head.buckets() - 1, previous.split);
                self.page(from)?;
                self.page(to)?;
                let page = self.pages.get_mut(&from).expect("loaded");
                let moved = std::mem::take(&mut page.rows);
                page.dirty = false;
                let into = self.pages.get_mut(&to).expect("loaded");
                into.rows.extend(moved);
                into.dirty = true;
                self.doomed.insert(from);
                self.head = previous;
            }
            if self.head.rows == 0 && self.head.buckets() == 1 {
                self.page(0)?.dirty = false;
                self.doomed.insert(0);
            }
            Ok(())
        }

        /// Every row, reading every page: for collections and tools.
        fn all(&mut self) -> io::Result<Entries> {
            let mut entries = Entries::new();
            for bucket in 0..self.head.buckets() {
                entries.extend(self.page(bucket)?.rows.iter().map(|(k, l)| (*k, *l)));
            }
            // A crash can leave the head's count behind the pages.
            self.head.rows = entries.len() as u64;
            Ok(entries)
        }

        fn table(&mut self) -> io::Result<&mut BTreeMap<u128, u64>> {
            if self.table.is_none() {
                let path = self.dir.join("packs");
                let (table, len) = match read_small(&path, MAX_INDEX) {
                    Ok(Some(bytes)) => {
                        (table_decode(&bytes).unwrap_or_default(), bytes.len() as u64)
                    }
                    Ok(None) => (BTreeMap::new(), 0),
                    Err(e) if e.kind() == io::ErrorKind::InvalidData => (BTreeMap::new(), 0),
                    Err(e) => return Err(e),
                };
                self.table = Some(table);
                self.table_len = len;
            }
            Ok(self.table.as_mut().expect("loaded"))
        }

        fn track(&mut self, id: u128, len: u64) -> io::Result<()> {
            self.table()?.insert(id, len);
            self.table_dirty = true;
            Ok(())
        }

        fn untrack(&mut self, id: u128) -> io::Result<()> {
            if self.table()?.remove(&id).is_some() {
                self.table_dirty = true;
            }
            Ok(())
        }

        /// Removes the pages a merge emptied, subtracting what it removed.
        fn remove_doomed(&mut self, delta: &mut i128) {
            for bucket in std::mem::take(&mut self.doomed) {
                let len = self.pages.remove(&bucket).map_or(0, |p| p.len);
                if remove_missing_ok(&self.dir.join(page_name(bucket))).is_ok() {
                    *delta -= i128::from(len);
                }
            }
        }

        fn head_dirty(&self) -> bool {
            match self.stored {
                None => self.head.rows > 0,
                Some(s) => {
                    Head {
                        generation: s.generation,
                        ..self.head
                    } != s
                }
            }
        }

        /// A fold's write (#1932): each changed page whole, stamped with
        /// `epoch`, a bucket that split still holding the rows it gave
        /// away, the pack table, the head, and last the split buckets
        /// without them and the removal of merged pages. `grow` is told
        /// the bytes this adds before any of it is written; the exact
        /// change is returned. A crash at any point leaves every key in the
        /// bucket the stored head names, or lost from this fold only, and
        /// the shard's log still holds what the fold applied.
        fn commit(
            &mut self,
            epoch: u32,
            grow: impl FnOnce(u64),
            headed: impl FnOnce(Option<Head>) -> io::Result<()>,
        ) -> io::Result<i128> {
            let head = self.head;
            let mut first = Vec::new();
            let mut last = Vec::new();
            // `delta` is the exact change, `growth` its increases alone.
            let mut delta = 0_i128;
            let mut growth = 0_i128;
            let mut change = |by: i128| {
                delta += by;
                growth += by.max(0);
            };
            for (&bucket, page) in &self.pages {
                if !page.dirty || self.doomed.contains(&bucket) {
                    continue;
                }
                let moved: Entries = page
                    .disk
                    .iter()
                    .filter(|(key, _)| head.bucket(key.1) != bucket && !page.rows.contains_key(key))
                    .map(|(k, l)| (*k, *l))
                    .collect();
                let bytes = if moved.is_empty() {
                    page_encode(&page.rows, epoch)
                } else {
                    let mut superset = page.rows.clone();
                    superset.extend(moved);
                    last.push(bucket);
                    page_encode(&superset, epoch)
                };
                change(i128::from(bytes.len() as u64) - i128::from(page.len));
                first.push((bucket, bytes));
            }
            let table = if self.table_dirty {
                let table = self.table.as_ref().expect("loaded");
                Some(if table.is_empty() {
                    Vec::new()
                } else {
                    table_encode(table)
                })
            } else {
                None
            };
            if let Some(bytes) = &table {
                change(i128::from(bytes.len() as u64) - i128::from(self.table_len));
            }
            // An index without rows has no files at all.
            let erase = self.doomed.contains(&0) && head.rows == 0;
            let head_bytes = if erase {
                self.head = Head::default();
                self.stored.map(|_| {
                    change(-(HEAD_LEN as i128));
                    Vec::new()
                })
            } else if self.head_dirty() {
                self.head.generation = u128::from(epoch);
                change(i128::from(self.stored.map_or(HEAD_LEN as u64, |_| 0)));
                Some(self.head.encode())
            } else {
                None
            };
            // Count the bytes before they are final, and no decrease before
            // it happens.
            grow(u64::try_from(growth).unwrap_or(0));
            if !first.is_empty() || table.is_some() || head_bytes.is_some() {
                std::fs::create_dir_all(&self.dir)?;
            }
            for (bucket, bytes) in &first {
                replace_file(&self.dir, &page_name(*bucket), bytes)?;
            }
            if let Some(bytes) = table {
                if bytes.is_empty() {
                    remove_missing_ok(&self.dir.join("packs"))?;
                } else {
                    replace_file(&self.dir, "packs", &bytes)?;
                }
                self.table_len = bytes.len() as u64;
                self.table_dirty = false;
            }
            if erase {
                // Pages first: a head left behind finds no stale rows.
                self.remove_doomed(&mut delta);
            }
            if let Some(bytes) = head_bytes {
                if bytes.is_empty() {
                    remove_missing_ok(&self.dir.join("head"))?;
                    self.stored = None;
                } else {
                    replace_file(&self.dir, "head", &bytes)?;
                    self.stored = Some(self.head);
                }
            }
            // Readers learn the new head before split buckets lose the rows
            // they gave away.
            headed(self.stored)?;
            for (bucket, bytes) in &mut first {
                let page = self.pages.get_mut(bucket).expect("written");
                if last.contains(bucket) {
                    let exact = page_encode(&page.rows, epoch);
                    delta += i128::from(exact.len() as u64) - i128::from(bytes.len() as u64);
                    replace_file(&self.dir, &page_name(*bucket), &exact)?;
                    page.len = exact.len() as u64;
                } else {
                    page.len = bytes.len() as u64;
                }
                page.disk.clone_from(&page.rows);
                page.dirty = false;
            }
            self.remove_doomed(&mut delta);
            Ok(delta)
        }
    }

    /// One change a shard's log records.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Op {
        Set(Key, Location),
        Remove(Key),
        Track(u128, u64),
        Untrack(u128),
    }

    impl Op {
        fn encode(self, out: &mut Vec<u8>) {
            let (tag, key, loc) = match self {
                Op::Set(key, loc) => (0_u8, key, loc),
                Op::Remove(key) => (1, key, Location::default()),
                Op::Track(id, len) => (
                    2,
                    (0, 0),
                    Location {
                        pack: id,
                        len,
                        ..Location::default()
                    },
                ),
                Op::Untrack(id) => (
                    3,
                    (0, 0),
                    Location {
                        pack: id,
                        ..Location::default()
                    },
                ),
            };
            out.push(tag);
            put_row(out, &key, &loc);
        }

        fn decode(b: &[u8]) -> Option<Op> {
            let key = (u16_at(b, 1), u128_at(b, 3));
            let loc = Location {
                pack: u128_at(b, 19),
                offset: u64_at(b, 35),
                len: u64_at(b, 43),
                used: u64_at(b, 51),
            };
            let known = EntryKind::ALL
                .iter()
                .any(|&k| k as u16 == key.0 && k != EntryKind::Clpack);
            match b[0] {
                0 if known && loc.len <= MAX_ENTRY && loc.offset.checked_add(loc.len).is_some() => {
                    Some(Op::Set(key, loc))
                }
                1 if known => Some(Op::Remove(key)),
                2 => Some(Op::Track(loc.pack, loc.len)),
                3 => Some(Op::Untrack(loc.pack)),
                _ => None,
            }
        }
    }

    /// A shard log's fixed header: its magic, the fold epoch its records
    /// extend, a copy of the head that fold wrote, the shard's running
    /// total, and a checksum of the rest.
    #[derive(Clone, Copy, Debug, Default)]
    struct LogHeader {
        epoch: u32,
        head: Option<Head>,
        total: Option<u64>,
    }

    impl LogHeader {
        fn encode(self) -> [u8; LOG_HEADER] {
            let mut out = [0_u8; LOG_HEADER];
            out[..8].copy_from_slice(LOG_MAGIC);
            out[8..12].copy_from_slice(&self.epoch.to_le_bytes());
            if let Some(head) = self.head {
                out[16..16 + HEAD_LEN].copy_from_slice(&head.encode());
            }
            if let Some(total) = self.total {
                out[80..88].copy_from_slice(&total.to_le_bytes());
                out[88] = 1;
            }
            let sum = checksum(&out[..96]);
            out[96..].copy_from_slice(&sum.to_le_bytes());
            out
        }

        fn decode(b: &[u8]) -> Option<LogHeader> {
            if b.len() < LOG_HEADER || &b[..8] != LOG_MAGIC || checksum(&b[..96]) != u128_at(b, 96)
            {
                return None;
            }
            let head = &b[16..16 + HEAD_LEN];
            Some(LogHeader {
                epoch: u32_at(b, 8),
                head: if head.iter().all(|&x| x == 0) {
                    None
                } else {
                    Some(Head::decode(head).ok()?)
                },
                total: (b[88] == 1).then(|| u64_at(b, 80)),
            })
        }
    }

    /// One appended batch: its op count, a checksum of its ops, the ops.
    fn log_frame(ops: &[Op]) -> Vec<u8> {
        let mut body = Vec::with_capacity(ops.len() * OP);
        for op in ops {
            op.encode(&mut body);
        }
        let mut out = Vec::with_capacity(LOG_FRAME + body.len());
        out.extend_from_slice(&u32::try_from(ops.len()).expect("bounded ops").to_le_bytes());
        out.extend_from_slice(&checksum(&body).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// A shard's log as read: its header, `None` when missing or torn; the
    /// ops of its whole frames, in order; and where they end. A torn or
    /// foreign frame ends the log.
    struct Log {
        header: Option<LogHeader>,
        ops: Vec<Op>,
        end: u64,
    }

    fn log_decode(bytes: &[u8]) -> Log {
        let header = LogHeader::decode(bytes);
        let mut ops = Vec::new();
        let mut at = LOG_HEADER.min(bytes.len());
        while let Some(head) = bytes.get(at..at + LOG_FRAME) {
            let count = usize::try_from(u32_at(head, 0)).unwrap_or(usize::MAX);
            let Some(body) = count
                .checked_mul(OP)
                .and_then(|n| bytes.get(at + LOG_FRAME..at + LOG_FRAME + n))
            else {
                break;
            };
            if count == 0 || checksum(body) != u128_at(head, 4) {
                break;
            }
            let Some(batch) = body
                .chunks_exact(OP)
                .map(Op::decode)
                .collect::<Option<Vec<_>>>()
            else {
                break;
            };
            ops.extend(batch);
            at += LOG_FRAME + body.len();
        }
        Log {
            header,
            ops,
            end: at as u64,
        }
    }

    /// What a shard's log adds over its pages: each key's last change
    /// (`None` for a removal) and each pack's tracking.
    #[derive(Clone, Default)]
    struct Overlay {
        rows: BTreeMap<Key, Option<Location>>,
        table: BTreeMap<u128, Option<u64>>,
    }

    impl Overlay {
        fn apply(&mut self, op: Op) {
            match op {
                Op::Set(key, loc) => {
                    self.rows.insert(key, Some(loc));
                }
                Op::Remove(key) => {
                    self.rows.insert(key, None);
                }
                Op::Track(id, len) => {
                    self.table.insert(id, Some(len));
                }
                Op::Untrack(id) => {
                    self.table.insert(id, None);
                }
            }
        }
    }

    /// A shard as a reader sees it, taking no lock: the head and epoch of
    /// the last fold its log names, and the log's overlay. A log whose
    /// header is torn takes its head and epoch from the head file.
    #[derive(Clone, Default)]
    struct View {
        epoch: u32,
        head: Option<Head>,
        overlay: Overlay,
        /// A view whose log names no fold has looked for a head file a
        /// fold since may have written.
        probed: bool,
        /// This process committed to the shard, leaving `total`.
        committed: bool,
        total: Option<u64>,
    }

    /// One shard under its writer lock (#1932): the log, which is also the
    /// lock file and holds the running total, the overlay it adds, the
    /// changes this writer appends, and the folded pages under them.
    struct Shard {
        id: u8,
        file: File,
        header: LogHeader,
        end: u64,
        overlay: Overlay,
        pending: Vec<Op>,
        /// The folded pages, head and table, read when first needed: a
        /// publication that only appends reads none of them.
        txn: Option<Txn>,
        dir: PathBuf,
    }

    impl Shard {
        fn txn(&mut self) -> io::Result<&mut Txn> {
            if self.txn.is_none() {
                self.txn = Some(Txn::open(self.dir.clone())?);
            }
            Ok(self.txn.as_mut().expect("opened"))
        }

        /// `key`'s location: the log's last change, else the pages'.
        fn get(&mut self, key: Key) -> io::Result<Option<Location>> {
            match self.overlay.rows.get(&key) {
                Some(change) => Ok(*change),
                None => self.txn()?.get(key),
            }
        }

        fn push(&mut self, op: Op) {
            self.overlay.apply(op);
            self.pending.push(op);
        }

        fn set(&mut self, key: Key, loc: Location) {
            self.push(Op::Set(key, loc));
        }

        fn remove(&mut self, key: Key) -> io::Result<()> {
            if self.get(key)?.is_some() {
                self.push(Op::Remove(key));
            }
            Ok(())
        }

        fn track(&mut self, id: u128, len: u64) {
            self.push(Op::Track(id, len));
        }

        fn untrack(&mut self, id: u128) -> io::Result<()> {
            if self.table()?.contains_key(&id) {
                self.push(Op::Untrack(id));
            }
            Ok(())
        }

        /// The packs this shard's table tracks, with the log applied.
        fn table(&mut self) -> io::Result<BTreeMap<u128, u64>> {
            let mut table = self.txn()?.table()?.clone();
            for (&id, &len) in &self.overlay.table {
                match len {
                    Some(len) => table.insert(id, len),
                    None => table.remove(&id),
                };
            }
            Ok(table)
        }

        /// Every row, the pages' with the log applied: for collections and
        /// tools.
        fn all(&mut self) -> io::Result<Entries> {
            let mut rows = self.txn()?.all()?;
            for (&key, change) in &self.overlay.rows {
                match change {
                    Some(loc) => rows.insert(key, *loc),
                    None => rows.remove(&key),
                };
            }
            Ok(rows)
        }

        fn dirty(&self) -> bool {
            !self.pending.is_empty()
        }

        fn write_header(&self) -> io::Result<()> {
            (&self.file).seek(SeekFrom::Start(0))?;
            (&self.file).write_all(&self.header.encode())
        }

        /// Sets the shard's running total, as a collection's walk found it.
        fn set_total(&mut self, total: u64) -> io::Result<()> {
            self.header.total = Some(total);
            self.write_header()
        }

        /// Lowers the running total by `bytes` a removal took.
        fn removed(&mut self, bytes: u64) -> io::Result<()> {
            if let Some(total) = &mut self.header.total {
                *total = total.saturating_sub(bytes);
            }
            self.write_header()
        }

        /// Appends this writer's changes as one record batch: one write, no
        /// temporary file, no rename (#1932). A home shard's total first
        /// grows by the `added` pack bytes `finish` then makes final; the
        /// log's records are not counted, as at most [`FOLD_AT`] bytes per
        /// shard. A log past [`FOLD_AT`] is folded into pages.
        fn commit(
            &mut self,
            added: u64,
            finish: impl FnOnce() -> io::Result<()>,
        ) -> io::Result<()> {
            let frame = (!self.pending.is_empty()).then(|| log_frame(&self.pending));
            if added > 0 {
                if let Some(total) = &mut self.header.total {
                    *total = total.saturating_add(added);
                }
                self.write_header()?;
            }
            finish()?;
            if let Some(frame) = frame {
                (&self.file).seek(SeekFrom::Start(self.end))?;
                (&self.file).write_all(&frame)?;
                count!(index_written, frame.len());
                self.end += frame.len() as u64;
                self.pending.clear();
            }
            if self.end > (LOG_HEADER as u64) + FOLD_AT {
                self.fold()?;
            }
            Ok(())
        }

        /// Folds the log into the pages: applies its changes, writes the
        /// changed pages, table and head stamped with the next epoch, then
        /// empties the log to a header naming that epoch and head. Cost:
        /// the pages the log's keys fall in, about one page per change at
        /// worst, once per [`FOLD_AT`] bytes appended.
        fn fold(&mut self) -> io::Result<()> {
            debug_assert!(self.pending.is_empty());
            for (&key, change) in &std::mem::take(&mut self.overlay.rows) {
                match change {
                    Some(loc) => self.txn()?.set(key, *loc)?,
                    None => self.txn()?.remove(key)?,
                }
            }
            for (&id, len) in &std::mem::take(&mut self.overlay.table) {
                match len {
                    Some(len) => self.txn()?.track(id, *len)?,
                    None => self.txn()?.untrack(id)?,
                }
            }
            self.txn()?.shrink()?;
            let epoch = self.header.epoch.wrapping_add(1);
            let base = self.header.total;
            let file = &self.file;
            let header = std::cell::RefCell::new(self.header);
            let write = |header: &LogHeader| -> io::Result<()> {
                let mut f = file;
                f.seek(SeekFrom::Start(0))?;
                f.write_all(&header.encode())
            };
            let txn = self.txn.get_or_insert(Txn::open(self.dir.clone())?);
            let delta = txn.commit(
                epoch,
                |growth| {
                    let mut h = header.borrow_mut();
                    h.total = base.map(|t| t.saturating_add(growth));
                    let _ = write(&h);
                },
                |head| {
                    // Shorten first: a crash before the header leaves the
                    // old epoch's header with no records, all of them in
                    // the pages, whose split buckets still hold the rows
                    // they gave away.
                    file.set_len(LOG_HEADER as u64)?;
                    let mut h = header.borrow_mut();
                    h.epoch = epoch;
                    h.head = head;
                    write(&h)
                },
            )?;
            self.header = header.into_inner();
            self.end = LOG_HEADER as u64;
            self.header.total =
                base.map(|t| u64::try_from((i128::from(t) + delta).max(0)).unwrap_or(0));
            self.write_header()
        }
    }

    impl Store {
        pub(super) fn new(root: &Path) -> Self {
            Self {
                root: root.join(NAMESPACE),
                snapshot: Mutex::new(None),
                used: AtomicU64::new(0),
                published: AtomicU32::new(0),
                rooted: AtomicBool::new(false),
                #[cfg(test)]
                on_pause: None,
            }
        }

        #[cfg(test)]
        fn pause(&self, at: &'static str) {
            if let Some(pause) = &self.on_pause {
                pause(at);
            }
        }

        fn state(&self) -> MutexGuard<'_, Option<Snapshot>> {
            self.snapshot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        fn path(&self, id: u128) -> PathBuf {
            self.root.join(format!("{id:032x}.pack"))
        }

        /// Shard `shard`'s directory of folded pages, head and table.
        fn index_dir(&self, shard: u8) -> PathBuf {
            self.root.join("index").join(format!("{shard:x}"))
        }

        /// Shard `shard`'s log, which is also its lock.
        fn log_path(&self, shard: u8) -> PathBuf {
            self.root.join("index").join(format!("{shard:x}.log"))
        }

        fn ensure_root(&self) -> io::Result<()> {
            if self.rooted.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::fs::create_dir_all(&self.root)?;
            if !is_real_dir(&self.root) {
                return Err(invalid("cache pack namespace is not a directory"));
            }
            self.rooted.store(true, Ordering::Relaxed);
            Ok(())
        }

        /// Shard `shard` under its writer lock, which covers that shard's
        /// commits only; a run takes one at a time. A new shard's log is
        /// created with a known total of zero, a torn tail is cut, and a
        /// torn header is rebuilt from the head file with the total unknown.
        fn open_shard(&self, shard: u8) -> io::Result<Shard> {
            self.ensure_root()?;
            let path = self.log_path(shard);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
                return Err(invalid("cache index shard log is not a regular file"));
            }
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            let file = match options.open(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(self.root.join("index"))?;
                    options.open(&path)?
                }
                opened => opened?,
            };
            count!(writer_locks, 1);
            if file.try_lock().is_err() {
                count!(lock_waits, 1);
                file.lock()?;
            }
            let mut bytes = Vec::new();
            (&file)
                .take(u64::try_from(MAX_INDEX).expect("limit"))
                .read_to_end(&mut bytes)?;
            count!(index_read, bytes.len());
            let log = log_decode(&bytes);
            let dir = self.index_dir(shard);
            let mut txn = None;
            let header = if let Some(header) = log.header {
                header
            } else {
                let fresh = bytes.is_empty() && !dir.exists();
                let stored = txn.insert(Txn::open(dir.clone())?).stored;
                LogHeader {
                    epoch: stored.map_or(0, |h| u32::try_from(h.generation).unwrap_or(0)),
                    head: stored,
                    total: fresh.then_some(0),
                }
            };
            let mut overlay = Overlay::default();
            // Records are absolute changes: replaying ones a fold already
            // applied changes nothing.
            for op in log.ops {
                overlay.apply(op);
            }
            let shard = Shard {
                id: shard,
                file,
                header,
                end: log.end.max(LOG_HEADER as u64),
                overlay,
                pending: Vec::new(),
                txn,
                dir,
            };
            if log.header.is_none() {
                shard.write_header()?;
            }
            if bytes.len() as u64 > shard.end {
                shard.file.set_len(shard.end)?;
            }
            Ok(shard)
        }

        /// Shard `shard` as a reader sees it, taking no lock.
        fn read_view(&self, shard: u8) -> View {
            let Ok(Some(bytes)) = read_small(&self.log_path(shard), MAX_INDEX) else {
                return View::default();
            };
            let log = log_decode(&bytes);
            // A header read while a writer rewrites it is torn: the head
            // file names the last fold, and the batches, checksummed on
            // their own, still apply.
            let (epoch, head) = if let Some(header) = log.header {
                (header.epoch, header.head)
            } else {
                let head = read_head(&self.index_dir(shard)).ok().flatten();
                let epoch = head.map_or(0, |h| u32::try_from(h.generation).unwrap_or(0));
                (epoch, head)
            };
            let mut overlay = Overlay::default();
            for op in log.ops {
                overlay.apply(op);
            }
            View {
                epoch,
                head,
                overlay,
                probed: false,
                committed: false,
                total: None,
            }
        }

        /// A file name the paged index writes.
        fn index_file(name: &str) -> bool {
            let base = name
                .strip_suffix(".tmp")
                .and_then(|n| n.rsplit_once('.'))
                .filter(|(_, id)| parse_key(id).is_some())
                .map_or(name, |(base, _)| base);
            base == "head"
                || base == "packs"
                || (base.len() == 16
                    && base.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        }

        /// A new pack leased to this run, its ID in shard `home`. The file
        /// is created as `.new`, locked, then renamed `.pending`, so a
        /// `.pending` file whose lock is free belongs to a dead writer: no
        /// lock is shared with other publishers. A `.new` file is left
        /// only by a crash between creation and locking.
        fn lease(&self, home: u8) -> io::Result<Lease> {
            self.ensure_root()?;
            loop {
                let id = (unique() & !(0xf << 124)) | (u128::from(home) << 124);
                if self.path(id).try_exists()? {
                    continue;
                }
                let new = self.root.join(format!("{id:032x}.new"));
                let file = match OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&new)
                {
                    Ok(file) => file,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                };
                let mut lease = Lease {
                    id,
                    path: new,
                    file,
                    committed: false,
                };
                lease.file.lock()?;
                let path = self.root.join(format!("{id:032x}.pending"));
                std::fs::rename(&lease.path, &path)?;
                lease.path = path;
                return Ok(lease);
            }
        }

        /// Renames the leased pack into place, still locked: until the
        /// run's last shard commit releases it, a collection keeps it
        /// even where no row names it yet. Called inside the home shard's
        /// commit, after its growth is recorded.
        fn finish(&self, lease: &mut Lease) -> io::Result<()> {
            let path = self.path(lease.id);
            if path.try_exists()? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "cache pack ID already exists",
                ));
            }
            std::fs::rename(&lease.path, &path)?;
            lease.path = path;
            // From here only a collection, checking every key of its
            // footer, removes it.
            lease.committed = true;
            Ok(())
        }

        fn open(&self, id: u128) -> io::Result<Opened> {
            let path = self.path(id);
            if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
                return Err(invalid("cache pack is not a regular file"));
            }
            let file = File::open(path)?;
            count!(packs_opened, 1);
            let len = file.metadata()?.len();
            Ok(Opened { file, len })
        }

        /// `key`'s location in the snapshot, reading its shard's log once
        /// and its page once. The log's change wins over a page no later
        /// fold wrote; a page a later fold wrote already holds it.
        fn locate(&self, state: &mut Snapshot, key: Key) -> Option<Location> {
            let shard = shard_of(key.1);
            let dir = self.index_dir(shard);
            for fresh in [false, true] {
                let Snapshot { views, pages, .. } = &mut *state;
                let view = views.entry(shard).or_insert_with(|| self.read_view(shard));
                let change = view.overlay.rows.get(&key).copied();
                if view.head.is_none() && change.is_some() && !view.probed {
                    // A fold after this view was read leaves a head: its
                    // pages are newer than the view's changes.
                    view.probed = true;
                    view.head = read_head(&dir).ok().flatten();
                }
                let Some(head) = view.head else {
                    return change.flatten();
                };
                let bucket = head.bucket(key.1);
                let (rows, epoch) = pages.entry((shard, bucket)).or_insert_with(|| {
                    read_page(&dir, bucket)
                        .ok()
                        .flatten()
                        .map(|(rows, _, epoch)| (rows, epoch))
                        .unwrap_or_default()
                });
                let newer = *epoch > view.epoch;
                match (change, rows.get(&key)) {
                    (Some(change), _) if !newer => return change,
                    (_, Some(loc)) => return Some(*loc),
                    // A page a later fold wrote, without the key: the fold
                    // may have moved it to a bucket this view's head does
                    // not have. Read the shard again, once.
                    _ if newer && !fresh => {
                        views.remove(&shard);
                        pages.retain(|(s, _), _| *s != shard);
                    }
                    _ => return None,
                }
            }
            None
        }

        fn snapshot<'a>(guard: &'a mut MutexGuard<'_, Option<Snapshot>>) -> &'a mut Snapshot {
            guard.get_or_insert_with(Snapshot::default)
        }

        pub(super) fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = Self::snapshot(&mut guard);
            let loc = self.opened(state, (kind as u16, key.0))?;
            let opened = state.files.get_mut(&loc.pack)?;
            if loc.len > MAX_ENTRY || loc.offset.checked_add(loc.len)? > opened.len {
                return None;
            }
            let mut bytes = vec![0; usize::try_from(loc.len).ok()?];
            opened.file.seek(SeekFrom::Start(loc.offset)).ok()?;
            opened.file.read_exact(&mut bytes).ok()?;
            let at = now();
            if at.saturating_sub(loc.used) > USE_MARK_AGE.as_secs() {
                state.touched.insert((kind as u16, key.0), at);
            }
            Some(Arc::from(bytes))
        }

        /// `key`'s location, with its pack open in the snapshot. Packs are
        /// immutable and a merge or GC removes one only after committing
        /// an index that no longer names it, so a pack the snapshot names
        /// but cannot find means its pages predate another process's
        /// commit. The snapshot then reads the heads and pages again, once
        /// per vanished pack, as git rescans its packs when one disappears
        /// (`reprepare_packed_git`). Cost: nothing on a hit or an ordinary
        /// miss; per vanished pack, the heads and pages read after it.
        fn opened(&self, state: &mut Snapshot, key: Key) -> Option<Location> {
            loop {
                let loc = self.locate(state, key)?;
                if state.files.contains_key(&loc.pack) {
                    return Some(loc);
                }
                match self.open(loc.pack) {
                    Ok(file) => {
                        if state.files.len() >= MAX_HANDLES {
                            state.files.pop_first();
                        }
                        state.files.insert(loc.pack, file);
                        return Some(loc);
                    }
                    // A pack the index still names after a refresh is
                    // missing for good: a miss, left to the collection.
                    Err(e)
                        if e.kind() == io::ErrorKind::NotFound && state.gone.insert(loc.pack) =>
                    {
                        state.views.clear();
                        state.pages.clear();
                    }
                    Err(_) => return None,
                }
            }
        }

        pub(super) fn touch(&self, kind: EntryKind, key: &Hash128) {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = Self::snapshot(&mut guard);
            let k = (kind as u16, key.0);
            let at = now();
            if self
                .locate(state, k)
                .is_some_and(|l| at.saturating_sub(l.used) > USE_MARK_AGE.as_secs())
            {
                state.touched.insert(k, at);
            }
        }

        /// Applies the pending recency marks of `shard` to the rows they
        /// name.
        fn merge_touches(&self, shard: &mut Shard) -> io::Result<()> {
            let id = shard.id;
            let touched: Vec<(Key, u64)> = self.state().as_mut().map_or_else(Vec::new, |s| {
                let mine: Vec<Key> = s
                    .touched
                    .keys()
                    .filter(|k| shard_of(k.1) == id)
                    .copied()
                    .collect();
                mine.into_iter()
                    .filter_map(|k| s.touched.remove(&k).map(|at| (k, at)))
                    .collect()
            });
            for (key, used) in touched {
                if let Some(loc) = shard.get(key)?
                    && used > loc.used
                {
                    shard.set(key, Location { used, ..loc });
                }
            }
            Ok(())
        }

        /// The shards with pending recency marks.
        fn touched_shards(&self) -> BTreeSet<u8> {
            self.state()
                .as_ref()
                .map(|s| s.touched.keys().map(|k| shard_of(k.1)).collect())
                .unwrap_or_default()
        }

        /// The snapshot after this process's commit to `shard`: its log as
        /// this writer left it, and the pages it folded; other pages are
        /// read again when needed.
        fn adopt(&self, shard: &Shard) {
            let mut guard = self.state();
            let state = guard.get_or_insert_with(Snapshot::default);
            state.views.insert(
                shard.id,
                View {
                    epoch: shard.header.epoch,
                    head: shard.header.head,
                    overlay: shard.overlay.clone(),
                    probed: false,
                    committed: true,
                    total: shard.header.total,
                },
            );
            state.pages.retain(|(s, _), _| *s != shard.id);
            for (&bucket, page) in shard.txn.iter().flat_map(|t| &t.pages) {
                if !page.dirty {
                    state
                        .pages
                        .insert((shard.id, bucket), (page.rows.clone(), shard.header.epoch));
                }
            }
        }

        /// One shard's commit: this run's marks there, then `apply`.
        fn commit_shard(
            &self,
            shard: u8,
            added: u64,
            finish: impl FnOnce() -> io::Result<()>,
            apply: impl FnOnce(&mut Shard) -> io::Result<()>,
        ) -> io::Result<()> {
            let mut shard = self.open_shard(shard)?;
            self.merge_touches(&mut shard)?;
            apply(&mut shard)?;
            // A home shard always adds its pack's bytes, so `finish` runs.
            if shard.dirty() || added > 0 {
                shard.commit(added, finish)?;
            }
            self.adopt(&shard);
            Ok(())
        }

        /// Publishes `batch` as one pack (#1932): written with no lock,
        /// then committed shard by shard, one shard lock at a time. The
        /// pack's home shard, where the pack table tracks it and which
        /// holds its first row, commits first, renaming the pack into
        /// place after recording its bytes. Runs whose keys lie in other
        /// shards never wait for this one. Cost: one appended batch of 59
        /// bytes per entry in each of at most min(entries, 16) shards, and
        /// a fold of a log that passes [`FOLD_AT`].
        pub(super) fn publish(&self, batch: &Batch) -> io::Result<()> {
            if batch.is_empty() {
                return Ok(());
            }
            self.used.store(1, Ordering::Relaxed);
            if batch.iter().any(|(kind, _, bytes)| {
                *kind == EntryKind::Clpack
                    || u64::try_from(bytes.len()).map_or(true, |n| n > MAX_ENTRY)
            }) {
                return Err(invalid("cache entry exceeds its size limit or is private"));
            }
            let home = shard_of(batch[0].1.0);
            let mut lease = self.lease(home)?;
            let mut offset = 0_u64;
            let mut rows = BTreeMap::<u8, Entries>::new();
            let mut frames = Frames::with_capacity(batch.len());
            let at = now();
            for (kind, key, bytes) in batch {
                lease.file.write_all(bytes)?;
                let len = u64::try_from(bytes.len()).map_err(io::Error::other)?;
                let k = (*kind as u16, key.0);
                rows.entry(shard_of(key.0)).or_default().insert(
                    k,
                    Location {
                        pack: lease.id,
                        offset,
                        len,
                        used: at,
                    },
                );
                frames.push((k, offset, len));
                offset = offset
                    .checked_add(len)
                    .ok_or_else(|| invalid("cache pack length overflow"))?;
            }
            let foot = footer(&frames);
            lease.file.write_all(&foot)?;
            let pack_len = offset + foot.len() as u64;
            let id = lease.id;
            let home_rows = rows.remove(&home).unwrap_or_default();
            self.commit_shard(
                home,
                pack_len,
                || self.finish(&mut lease),
                |shard| {
                    for (key, loc) in home_rows {
                        shard.set(key, loc);
                    }
                    shard.track(id, pack_len);
                    Ok(())
                },
            )?;
            for (shard, rows) in rows {
                self.commit_shard(
                    shard,
                    0,
                    || Ok(()),
                    |shard| {
                        for (key, loc) in rows {
                            shard.set(key, loc);
                        }
                        Ok(())
                    },
                )?;
            }
            // Every row is committed: a collection may judge the pack now.
            drop(lease);
            self.published.fetch_or(1 << home, Ordering::Relaxed);
            Ok(())
        }

        fn pack_id(name: &str) -> Option<u128> {
            name.strip_suffix(".pack").and_then(parse_key).map(|k| k.0)
        }

        /// Walks the pack namespace and its index shards, taking no lock:
        /// the collection's inventory. A file another process removes
        /// meanwhile is skipped. It never follows a link or counts a
        /// foreign filename.
        fn walk(&self) -> io::Result<Inventory> {
            #[cfg(test)]
            self.pause("walk");
            count!(walks, 1);
            let mut inv = Inventory::default();
            let len = |e: &std::fs::DirEntry| match e.metadata() {
                Ok(meta) => Ok(Some(meta)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            };
            count!(dirs_listed, 1);
            // cost: O(files), in a collection only: once per interval or
            // `hd cache gc`.
            for e in std::fs::read_dir(&self.root)? {
                count!(entries_listed, 1);
                let e = e?;
                if !e.file_type()?.is_file() {
                    continue;
                }
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(id) = Self::pack_id(name) {
                    if let Some(meta) = len(&e)? {
                        inv.total = inv.total.saturating_add(meta.len());
                        inv.packs.insert(id, meta.len());
                    }
                } else if name.strip_suffix(".pending").and_then(parse_key).is_some() {
                    inv.pending.push(e.path());
                } else if name.strip_suffix(".new").and_then(parse_key).is_some()
                    && len(&e)?.is_some_and(|meta| super::stale_by(&meta, super::GC_INTERVAL))
                {
                    // Only a crash between creating and locking a lease
                    // leaves one; a live writer renames it at once.
                    inv.abandoned.push(e.path());
                }
            }
            for shard in 0..SHARDS {
                let index = self.index_dir(shard);
                if !is_real_dir(&index) {
                    continue;
                }
                count!(dirs_listed, 1);
                // cost: O(pages), in a collection only.
                for e in std::fs::read_dir(index)? {
                    count!(entries_listed, 1);
                    let e = e?;
                    let name = e.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if !Self::index_file(name) || !e.file_type()?.is_file() {
                        continue;
                    }
                    if name.rsplit_once('.').is_some_and(|(_, ext)| ext == "tmp") {
                        inv.temps.push((shard, e.path()));
                        continue;
                    }
                    let n = len(&e)?.map_or(0, |meta| meta.len());
                    inv.total = inv.total.saturating_add(n);
                    inv.index[usize::from(shard)] += n;
                    if let Ok(bucket) = u64::from_str_radix(name, 16)
                        && name.len() == 16
                    {
                        inv.pages.push((shard, bucket));
                        inv.page_bytes[usize::from(shard)] += n;
                    }
                }
            }
            Ok(inv)
        }

        /// Every row, read with no lock: each shard's pages under the head
        /// its log names, with the log's changes applied where no later
        /// fold wrote the page. True when no shard's log was folded during
        /// its read, so no split or merge moved a row past it. Up to three
        /// tries per shard.
        fn read_all(&self) -> io::Result<(Entries, bool)> {
            let mut all = Entries::new();
            let mut consistent = true;
            for shard in 0..SHARDS {
                if !self.log_path(shard).exists() {
                    continue;
                }
                let dir = self.index_dir(shard);
                let mut last = (Entries::new(), true);
                for _ in 0..3 {
                    let view = self.read_view(shard);
                    let mut rows = Entries::new();
                    let mut folded = BTreeSet::new();
                    if let Some(head) = view.head {
                        for bucket in 0..head.buckets() {
                            match read_page(&dir, bucket) {
                                Ok(Some((page, _, epoch))) => {
                                    if epoch > view.epoch {
                                        folded.extend(page.keys().copied());
                                    }
                                    rows.extend(
                                        page.into_iter()
                                            .filter(|(k, _)| head.bucket(k.1) == bucket),
                                    );
                                }
                                // A missing or corrupt page is empty to
                                // writers too.
                                Ok(None) => {}
                                Err(e) if e.kind() == io::ErrorKind::InvalidData => {}
                                Err(e) => return Err(e),
                            }
                        }
                    }
                    for (&key, change) in &view.overlay.rows {
                        if folded.contains(&key) {
                            continue;
                        }
                        match change {
                            Some(loc) => rows.insert(key, *loc),
                            None => rows.remove(&key),
                        };
                    }
                    #[cfg(test)]
                    self.pause("read");
                    let same = self.read_view(shard).epoch == view.epoch;
                    last = (rows, same);
                    if same {
                        break;
                    }
                }
                consistent &= last.1;
                all.extend(last.0);
            }
            Ok((all, consistent))
        }

        /// The keys whose rows may name pack `id`: its footer's. `None` when
        /// it has no readable footer.
        fn footer_keys(&self, id: u128) -> Option<Vec<Key>> {
            let mut opened = self.open(id).ok()?;
            let frames = read_footer(&mut opened.file, opened.len).ok()?;
            Some(frames.into_iter().map(|(key, _, _)| key).collect())
        }

        /// Whether a run still holds pack `id`'s lease lock, committing
        /// its rows; a pack no run holds has all its rows committed.
        fn leased(&self, id: u128) -> bool {
            File::open(self.path(id))
                .is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
        }

        /// Removes the packs in `ids` from their home shards' tables, then
        /// their files, one shard lock at a time; the bytes removed.
        fn drop_packs(&self, ids: &[u128]) -> io::Result<u64> {
            let mut by_home = BTreeMap::<u8, Vec<u128>>::new();
            for &id in ids {
                by_home.entry(shard_of(id)).or_default().push(id);
            }
            let mut removed = 0;
            for (home, ids) in by_home {
                let mut shard = self.open_shard(home)?;
                for &id in &ids {
                    shard.untrack(id)?;
                }
                if shard.dirty() {
                    shard.commit(0, || Ok(()))?;
                }
                self.adopt(&shard);
                let mut gone = 0;
                for id in ids {
                    let path = self.path(id);
                    let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
                    // An open reader on Windows may postpone this; a later
                    // collection retries it.
                    if std::fs::remove_file(path).is_ok() {
                        gone += size;
                    }
                }
                if gone > 0 {
                    shard.removed(gone)?;
                }
                removed += gone;
            }
            Ok(removed)
        }

        /// Copies `sources`' frames into one new pack, taking no lock, then
        /// commits shard by shard, one lock at a time: each row still at
        /// its old range moves to the new pack. A source no key's row
        /// names any more is removed, as is the copy when no row took it.
        /// Rows never move into an existing pack, so a key found elsewhere
        /// in one shard stays elsewhere. Returns the bytes removed. Cost:
        /// the copied bytes, unlocked; the pages of the sources' keys.
        fn rewrite(&self, sources: &[Source]) -> io::Result<u64> {
            // The home of the first copied key, or of the first source when
            // nothing is live: the empty copy is then dropped with them.
            let home = sources
                .iter()
                .find_map(|s| s.copy.first())
                .map_or_else(|| shard_of(sources[0].id), |first| shard_of(first.0.1));
            let mut lease = self.lease(home)?;
            #[cfg(test)]
            self.pause("copy");
            let mut offset = 0_u64;
            let mut frames = Frames::new();
            let mut moved = BTreeMap::<u8, Vec<(Key, u128, u64, u64, u64)>>::new();
            for source in sources {
                let mut file = match self.open(source.id) {
                    Ok(opened) => opened.file,
                    // Another collection or merge took it first.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
                    Err(e) => return Err(e),
                };
                for &(key, from, len) in &source.copy {
                    file.seek(SeekFrom::Start(from))?;
                    let got = io::copy(&mut (&mut file).take(len), &mut lease.file)?;
                    if got != len {
                        return Err(invalid("truncated cache pack during compaction"));
                    }
                    frames.push((key, offset, len));
                    moved
                        .entry(shard_of(key.1))
                        .or_default()
                        .push((key, source.id, from, len, offset));
                    offset += len;
                }
            }
            let foot = footer(&frames);
            lease.file.write_all(&foot)?;
            let len = offset + foot.len() as u64;
            let new = lease.id;
            let mut keys = BTreeMap::<u8, Vec<(u128, Key)>>::new();
            let mut named = BTreeSet::new();
            for source in sources {
                for &key in &source.keys {
                    keys.entry(shard_of(key.1))
                        .or_default()
                        .push((source.id, key));
                }
            }
            let mut shards: Vec<u8> = moved.keys().chain(keys.keys()).copied().collect();
            shards.sort_stable();
            shards.dedup();
            shards.retain(|&s| s != home);
            shards.insert(0, home);
            let mut taken = false;
            for shard in shards {
                let moved = moved.remove(&shard).unwrap_or_default();
                let keys = keys.remove(&shard).unwrap_or_default();
                let apply = |txn: &mut Shard| -> io::Result<()> {
                    for (key, id, from, n, to) in moved {
                        if let Some(loc) = txn.get(key)?
                            && (loc.pack, loc.offset, loc.len) == (id, from, n)
                        {
                            txn.set(
                                key,
                                Location {
                                    pack: new,
                                    offset: to,
                                    ..loc
                                },
                            );
                            taken = true;
                        }
                    }
                    for (id, key) in keys {
                        if txn.get(key)?.is_some_and(|loc| loc.pack == id) {
                            named.insert(id);
                        }
                    }
                    if shard == home {
                        txn.track(new, len);
                    }
                    Ok(())
                };
                if shard == home {
                    // Readers holding pages from before may still name the
                    // old packs; they read the index again when one is gone.
                    self.commit_shard(shard, len, || self.finish(&mut lease), apply)?;
                } else {
                    self.commit_shard(shard, 0, || Ok(()), apply)?;
                }
            }
            drop(lease);
            let mut doomed: Vec<u128> = sources
                .iter()
                .map(|s| s.id)
                .filter(|id| !named.contains(id))
                .collect();
            if !taken {
                doomed.push(new);
            }
            self.drop_packs(&doomed)
        }

        /// Merges the smallest size class of tracked packs once it holds
        /// [`FANOUT`] packs, so their count stays bounded: about
        /// `FANOUT` per power of two up to [`FULL`]. Each frame is copied
        /// once per class it climbs. The tables, footers and pages are
        /// read with no lock and the copy is made with none (`rewrite`).
        /// Cost: the home shards' pack tables, the merged bytes, their
        /// footers and the pages of their rows; no directory walk.
        fn merge_small(&self, homes: u32) -> io::Result<()> {
            // What this run reads: its own snapshot, kept apart from the
            // one its reads use.
            let mut snapshot = Snapshot::default();
            let mut classes = BTreeMap::<u32, Vec<u128>>::new();
            for shard in 0..SHARDS {
                if homes & (1 << shard) == 0 || !self.log_path(shard).exists() {
                    continue;
                }
                let view = self.read_view(shard);
                let mut table = Txn::open(self.index_dir(shard))?.table()?.clone();
                for (&id, &len) in &view.overlay.table {
                    match len {
                        Some(len) => table.insert(id, len),
                        None => table.remove(&id),
                    };
                }
                for (id, len) in table {
                    if len < FULL {
                        classes.entry(len.max(1).ilog2()).or_default().push(id);
                    }
                }
                snapshot.views.insert(shard, view);
            }
            let Some((_, ids)) = classes.into_iter().find(|(_, ids)| ids.len() >= FANOUT) else {
                return Ok(());
            };
            let mut sources = Vec::with_capacity(FANOUT);
            let mut lost = Vec::new();
            for id in ids.into_iter().take(FANOUT) {
                match self
                    .open(id)
                    .and_then(|mut opened| read_footer(&mut opened.file, opened.len))
                {
                    Ok(frames) => {
                        let mut copy = Frames::new();
                        for &(key, from, len) in &frames {
                            if self
                                .locate(&mut snapshot, key)
                                .is_some_and(|l| (l.pack, l.offset, l.len) == (id, from, len))
                            {
                                copy.push((key, from, len));
                            }
                        }
                        let keys = frames.into_iter().map(|(key, _, _)| key).collect();
                        sources.push(Source { id, copy, keys });
                    }
                    // A pack another merge removed, or one without a footer,
                    // leaves the table; a collection's walk still finds it.
                    Err(_) => lost.push(id),
                }
            }
            if sources.len() >= 2 {
                self.rewrite(&sources)?;
            }
            for id in lost {
                self.commit_shard(shard_of(id), 0, || Ok(()), |shard| shard.untrack(id))?;
            }
            Ok(())
        }

        /// A command's settlement (#1932): this run's remaining recency
        /// marks, one shard at a time, and, after a publication, one small
        /// merge. Then, reading the shards' heads and running totals with
        /// no lock, whether the cache is over `cap` or its total unknown,
        /// which wants a collection. It never walks or reads the whole
        /// index. Cost: O(marks), one bounded merge, and 112 bytes per
        /// shard.
        pub(super) fn settle(&self, cap: u64) -> io::Result<bool> {
            if !is_real_dir(&self.root) {
                return Ok(false);
            }
            for shard in self.touched_shards() {
                self.commit_shard(shard, 0, || Ok(()), |_| Ok(()))?;
            }
            let published = self.published.swap(0, Ordering::Relaxed);
            if published != 0 {
                self.merge_small(published)?;
            }
            Ok(self.total().is_none_or(|total| total > cap))
        }

        /// The sum of the shards' running totals, read from their logs'
        /// headers with no lock, but for those this process just wrote;
        /// `None` when one is unknown or torn.
        fn total(&self) -> Option<u64> {
            let mut total = 0_u64;
            // The shards this process just committed to: their totals as it
            // left them, with no read.
            let known: BTreeMap<u8, Option<u64>> =
                self.state().as_ref().map_or_else(BTreeMap::new, |s| {
                    s.views
                        .iter()
                        .filter(|(_, v)| v.committed)
                        .map(|(&k, v)| (k, v.total))
                        .collect()
                });
            for shard in 0..SHARDS {
                if let Some(&known) = known.get(&shard) {
                    total = total.saturating_add(known?);
                    continue;
                }
                let path = self.log_path(shard);
                let mut bytes = Vec::with_capacity(LOG_HEADER);
                match File::open(&path) {
                    Ok(file) => {
                        file.take(LOG_HEADER as u64).read_to_end(&mut bytes).ok()?;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(_) => return None,
                }
                total = total.saturating_add(LogHeader::decode(&bytes)?.total?);
            }
            Some(total)
        }

        /// Retires the namespace of the unsharded index (`pack-v1`), whose
        /// format this one replaced: its managed files and directories go,
        /// and a lease a live older writer holds stays. Cost: once, the
        /// first collection after the change.
        fn retire_v1(&self) {
            let Some(old) = self.root.parent().map(|p| p.join("pack-v1")) else {
                return;
            };
            if !is_real_dir(&old) {
                return;
            }
            for dir in [old.join("index"), old.join("index.new"), old.clone()] {
                if !is_real_dir(&dir) {
                    continue;
                }
                // cost: once, the files of the retired namespace.
                for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let name = e.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if !e.file_type().is_ok_and(|t| t.is_file()) {
                        continue;
                    }
                    let lease = name.strip_suffix(".pending").and_then(parse_key).is_some();
                    let managed = lease
                        || Self::pack_id(name).is_some()
                        || Self::index_file(name)
                        || matches!(name, "lock" | "index" | "index.old");
                    let free = !lease || File::open(e.path()).is_ok_and(|f| f.try_lock().is_ok());
                    if managed && free {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
                let _ = std::fs::remove_dir(&dir);
            }
        }

        /// The collection (#1932): the only work that grows with the
        /// cache, run by `hd cache gc` and at most once per interval. It
        /// walks and reads the whole index with no lock, evicts the least
        /// recently used rows down to 90% of `cap`, removes packs no row
        /// names, compacts packs with dead bytes, sweeps crashed writers'
        /// files, and records each shard's total. Each shard lock covers
        /// that shard's commits only, never the walk or a copy.
        pub(super) fn collect(&self, cap: u64) -> io::Result<Collected> {
            self.retire_v1();
            if !is_real_dir(&self.root) {
                return Ok(Collected {
                    before: 0,
                    after: 0,
                });
            }
            // Eviction sees this run's marks.
            for shard in self.touched_shards() {
                self.commit_shard(shard, 0, || Ok(()), |_| Ok(()))?;
            }
            let inv = self.walk()?;
            let before = inv.total;
            let (mut entries, consistent) = self.read_all()?;
            // A row past its walked pack is dead for good: packs are
            // immutable. One naming a pack the walk missed may be newer
            // than the walk; the commit looks again.
            let mut suspects = Vec::new();
            entries.retain(|&key, loc| {
                let fits = inv.packs.get(&loc.pack).is_some_and(|len| {
                    loc.offset
                        .checked_add(loc.len)
                        .is_some_and(|end| end <= *len)
                });
                if !fits {
                    suspects.push((key, *loc));
                }
                fits
            });
            let victims = Self::victims(&entries, cap, &inv.page_bytes);
            for (key, _) in &victims {
                entries.remove(key);
            }
            let mut rows_of = BTreeMap::<u128, Frames>::new();
            for (&key, loc) in &entries {
                rows_of
                    .entry(loc.pack)
                    .or_default()
                    .push((key, loc.offset, loc.len));
            }
            // Footers are immutable: read them before taking a lock.
            let dead: Vec<(u128, Option<Vec<Key>>)> = inv
                .packs
                .keys()
                .filter(|id| !rows_of.contains_key(id))
                .map(|&id| {
                    (
                        id,
                        self.footer_keys(id).or_else(|| consistent.then(Vec::new)),
                    )
                })
                .collect();
            let doomed = self.evict(&inv, &suspects, &victims, &dead)?;
            // Packs no row names: no lock, as no row can name them again.
            for id in doomed {
                let _ = std::fs::remove_file(self.path(id));
            }
            let over = before > cap;
            for (id, copy) in rows_of {
                let physical = inv.packs.get(&id).copied().unwrap_or(0);
                let live = copy
                    .iter()
                    .map(|&(_, _, len)| len + FOOT_ROW as u64)
                    .sum::<u64>()
                    + HEADER as u64;
                if live < physical && (over || live.saturating_mul(4) < physical.saturating_mul(3))
                {
                    let keys = self.footer_keys(id).or_else(|| {
                        consistent.then(|| copy.iter().map(|&(key, _, _)| key).collect())
                    });
                    if let Some(keys) = keys {
                        self.rewrite(&[Source { id, copy, keys }])?;
                    }
                }
            }
            self.merge_small(u32::MAX)?;
            // A collection folds every log last: readers' logs start short,
            // and buckets its evictions emptied merge.
            for shard in 0..SHARDS {
                if self.log_path(shard).exists() {
                    let mut txn = self.open_shard(shard)?;
                    if txn.end > LOG_HEADER as u64 {
                        txn.fold()?;
                    }
                    self.adopt(&txn);
                }
            }
            let after = self.walk()?;
            let mut total = 0_u64;
            for shard in 0..SHARDS {
                let homes = after.packs.keys().any(|&id| shard_of(id) == shard);
                if !homes && !self.log_path(shard).exists() {
                    continue;
                }
                let mut txn = self.open_shard(shard)?;
                // The shard's log and index, the packs it tracks or would,
                // and the packs published into its table since the walk.
                let mut bytes = after.index[usize::from(shard)];
                bytes += after
                    .packs
                    .iter()
                    .filter(|(id, _)| shard_of(**id) == shard)
                    .map(|(_, len)| len)
                    .sum::<u64>();
                bytes += txn
                    .table()?
                    .iter()
                    .filter(|(id, _)| !after.packs.contains_key(id))
                    .map(|(_, len)| len)
                    .sum::<u64>();
                txn.set_total(bytes)?;
                self.adopt(&txn);
                total = total.saturating_add(bytes);
            }
            Ok(Collected {
                before,
                after: total,
            })
        }

        /// The least recently used rows to evict, oldest first, ties by
        /// key, when the live set and the index it needs exceed `cap`: down
        /// to 90% of it. Dead bytes never evict a live set that fits.
        /// `pages` holds each shard's page bytes now.
        fn victims(
            entries: &Entries,
            cap: u64,
            pages: &[u64; SHARDS as usize],
        ) -> Vec<(Key, Location)> {
            let mut rows_per_pack = BTreeMap::<u128, usize>::new();
            let mut rows_per_shard = [0_usize; SHARDS as usize];
            for (key, loc) in entries {
                *rows_per_pack.entry(loc.pack).or_default() += 1;
                rows_per_shard[usize::from(shard_of(key.1))] += 1;
            }
            // What the live set keeps: each frame with its footer row, each
            // pack with its trailer and table row, and in each shard that
            // keeps a row, its head and the pages its fold leaves: one per
            // bucket, a bucket per half load at most, and never more pages
            // than its rows fill. A shard with no row has no files.
            let overhead = |shards: &[usize; SHARDS as usize], packs: usize| -> u64 {
                let index: u64 = shards
                    .iter()
                    .zip(pages)
                    .filter(|(n, _)| **n > 0)
                    .map(|(&n, &bytes)| {
                        let buckets = n.div_ceil(usize::try_from(LOAD / 2).expect("small"));
                        let fold = (n.div_ceil(SLOTS) * page_len(0)) as u64;
                        HEAD_LEN as u64 + bytes.max(fold).min((buckets * page_len(0)) as u64)
                    })
                    .sum();
                index + (packs * (TABLE_ROW + HEADER)) as u64
            };
            let mut live = entries
                .values()
                .map(|loc| loc.len + FOOT_ROW as u64)
                .sum::<u64>();
            let kept = |live: u64, shards: &[usize; SHARDS as usize], packs: usize| {
                live.saturating_add(overhead(shards, packs))
            };
            let mut victims = Vec::new();
            if kept(live, &rows_per_shard, rows_per_pack.len()) <= cap {
                return victims;
            }
            let mut order = entries
                .iter()
                .map(|(&key, loc)| (loc.used, key))
                .collect::<Vec<_>>();
            order.sort_stable();
            for (_, key) in order {
                if kept(live, &rows_per_shard, rows_per_pack.len()) <= cap.saturating_mul(9) / 10 {
                    break;
                }
                let loc = entries[&key];
                live -= loc.len + FOOT_ROW as u64;
                rows_per_shard[usize::from(shard_of(key.1))] -= 1;
                if let Some(rows) = rows_per_pack.get_mut(&loc.pack) {
                    *rows -= 1;
                    if *rows == 0 {
                        rows_per_pack.remove(&loc.pack);
                    }
                }
                victims.push((key, loc));
            }
            victims
        }

        /// The collection's eviction commits, one shard at a time: rows
        /// whose pack is gone or too short, unchanged victims, crashed
        /// temporary files and pages past the last bucket, whose writers
        /// would hold that shard's lock. Then the dead packs that no run
        /// holds and no key's row names leave their tables; the caller
        /// removes their files. Last, leases whose lock is free. Cost: the
        /// pages of those rows and keys, and the crashed files.
        fn evict(
            &self,
            inv: &Inventory,
            suspects: &[(Key, Location)],
            victims: &[(Key, Location)],
            dead: &[(u128, Option<Vec<Key>>)],
        ) -> io::Result<Vec<u128>> {
            let mut work = BTreeMap::<u8, Vec<(Key, Location, bool)>>::new();
            for &(key, loc) in suspects {
                work.entry(shard_of(key.1))
                    .or_default()
                    .push((key, loc, true));
            }
            for &(key, loc) in victims {
                work.entry(shard_of(key.1))
                    .or_default()
                    .push((key, loc, false));
            }
            // A pack a run still holds is committing rows: it stays. Every
            // other pack's rows are all committed, so a check after this
            // point sees them.
            let mut keys = BTreeMap::<u8, Vec<(u128, Key)>>::new();
            let mut named = BTreeSet::new();
            for (id, list) in dead {
                if self.leased(*id) {
                    named.insert(*id);
                    continue;
                }
                match list {
                    Some(list) => {
                        for &key in list {
                            keys.entry(shard_of(key.1)).or_default().push((*id, key));
                        }
                    }
                    None => {
                        named.insert(*id);
                    }
                }
            }
            for shard in 0..SHARDS {
                if !self.log_path(shard).exists() && !is_real_dir(&self.index_dir(shard)) {
                    continue;
                }
                let mut txn = self.open_shard(shard)?;
                for (key, loc, suspect) in work.remove(&shard).unwrap_or_default() {
                    // A row hit or replaced since the read stays.
                    if txn.get(key)? != Some(loc) {
                        continue;
                    }
                    // A pack that cannot be read is a miss: its row goes.
                    let gone = !suspect
                        || std::fs::symlink_metadata(self.path(loc.pack)).map_or(true, |meta| {
                            !meta.file_type().is_file()
                                || loc
                                    .offset
                                    .checked_add(loc.len)
                                    .is_none_or(|end| end > meta.len())
                        });
                    if gone {
                        txn.remove(key)?;
                    }
                }
                for (id, key) in keys.remove(&shard).unwrap_or_default() {
                    if txn.get(key)?.is_some_and(|loc| loc.pack == id) {
                        named.insert(id);
                    }
                }
                // Tracked packs that no longer exist leave the table.
                let tracked: Vec<u128> = txn.table()?.keys().copied().collect();
                for id in tracked {
                    if !inv.packs.contains_key(&id) && !self.path(id).try_exists()? {
                        txn.untrack(id)?;
                    }
                }
                if txn.dirty() {
                    txn.commit(0, || Ok(()))?;
                }
                for (_, path) in inv.temps.iter().filter(|(s, _)| *s == shard) {
                    let _ = std::fs::remove_file(path);
                }
                let buckets = txn.txn()?.head.buckets();
                for &(_, bucket) in inv.pages.iter().filter(|(s, _)| *s == shard) {
                    if bucket >= buckets {
                        let _ = std::fs::remove_file(self.index_dir(shard).join(page_name(bucket)));
                    }
                }
                self.adopt(&txn);
            }
            let doomed: Vec<u128> = dead
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| !named.contains(id))
                .collect();
            let mut by_home = BTreeMap::<u8, Vec<u128>>::new();
            for &id in &doomed {
                by_home.entry(shard_of(id)).or_default().push(id);
            }
            for (home, ids) in by_home {
                self.commit_shard(
                    home,
                    0,
                    || Ok(()),
                    |shard| {
                        for id in ids {
                            shard.untrack(id)?;
                        }
                        Ok(())
                    },
                )?;
            }
            // A lease is locked before it is named `.pending`, so one whose
            // lock is free belongs to a dead writer.
            for path in &inv.pending {
                if File::open(path).is_ok_and(|file| file.try_lock().is_ok()) {
                    let _ = std::fs::remove_file(path);
                }
            }
            for path in &inv.abandoned {
                let _ = std::fs::remove_file(path);
            }
            Ok(doomed)
        }

        /// Best-effort physical accounting also works when no lock can be
        /// opened; a failed GC must not report zero data.
        pub(super) fn bytes(&self) -> u64 {
            if !is_real_dir(&self.root) {
                return 0;
            }
            self.walk().map_or(0, |inv| inv.total)
        }

        pub(super) fn was_used(&self) -> bool {
            self.used.load(Ordering::Relaxed) != 0
        }

        /// Every indexed entry: kind, key, pack file, offset, length, last
        /// use and the pack's length, `None` when it is missing. It holds
        /// every shard's lock, taken in shard order, so no merge or GC
        /// moves a row or removes its pack between reading the row and the
        /// pack's length; nothing else holds two shard locks, so this
        /// cannot deadlock. Locks it cannot take, as in a read-only cache,
        /// it skips. It reads the whole index: for tests and tools.
        pub(super) fn entries(&self) -> Vec<IndexedEntry> {
            if !is_real_dir(&self.root) {
                return Vec::new();
            }
            let mut shards: Vec<Shard> = (0..SHARDS)
                .filter(|&s| self.log_path(s).exists())
                .filter_map(|s| self.open_shard(s).ok())
                .collect();
            let mut rows = Entries::new();
            for shard in &mut shards {
                if let Ok(all) = shard.all() {
                    rows.extend(all);
                }
            }
            let mut packs = BTreeMap::<u128, Option<u64>>::new();
            rows.into_iter()
                .filter_map(|((kind, key), l)| {
                    let pack = self.path(l.pack);
                    let pack_len = *packs.entry(l.pack).or_insert_with(|| {
                        std::fs::symlink_metadata(&pack)
                            .ok()
                            .filter(|m| m.file_type().is_file())
                            .map(|m| m.len())
                    });
                    Some(IndexedEntry {
                        kind: EntryKind::ALL.into_iter().find(|k| *k as u16 == kind)?,
                        key: Hash128(key),
                        pack,
                        pack_len,
                        offset: l.offset,
                        len: l.len,
                        used: l.used,
                    })
                })
                .collect()
        }

        /// Sets an entry's last use, as if hit then; false when absent.
        pub(super) fn set_used(
            &self,
            kind: EntryKind,
            key: &Hash128,
            used: u64,
        ) -> io::Result<bool> {
            let mut shard = self.open_shard(shard_of(key.0))?;
            let k = (kind as u16, key.0);
            let Some(loc) = shard.get(k)? else {
                return Ok(false);
            };
            shard.set(k, Location { used, ..loc });
            shard.commit(0, || Ok(()))?;
            self.adopt(&shard);
            Ok(true)
        }
    }
    #[cfg(test)]
    mod tests {
        use super::super::DiskStore;
        use super::*;
        use crate::store::CacheStore;
        use std::time::{Duration, Instant};

        fn scratch(label: &str) -> PathBuf {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/hd_cache-tests")
                .join(format!("packed-{label}-{:032x}", unique()));
            std::fs::create_dir_all(&path).unwrap();
            path
        }
        fn item(key: u128, bytes: &[u8]) -> (EntryKind, Hash128, Arc<[u8]>) {
            (EntryKind::Check, Hash128(key), Arc::from(bytes))
        }
        fn ages(store: &Store, times: &[(u128, u64)]) {
            for &(key, at) in times {
                assert!(store.set_used(EntryKind::Check, &Hash128(key), at).unwrap());
            }
        }
        /// Every indexed row of `store`.
        fn load(store: &Store) -> Entries {
            let mut all = Entries::new();
            for shard in 0..SHARDS {
                if store.log_path(shard).exists() {
                    all.extend(store.open_shard(shard).unwrap().all().unwrap());
                }
            }
            all
        }
        /// A pack of `frames` entries of `bytes` bytes, footer included.
        fn pack_len(frames: usize, bytes: usize) -> u64 {
            (frames * (bytes + FOOT_ROW) + HEADER) as u64
        }
        /// A folded index of one page and a table of `packs` packs.
        fn index_len(packs: usize) -> u64 {
            (HEAD_LEN + page_len(1) + HEADER + packs * TABLE_ROW) as u64
        }
        /// Makes shard 0's running total unknown, as a torn header leaves
        /// it.
        fn unrecorded(store: &Store) {
            let mut shard = store.open_shard(0).unwrap();
            shard.header.total = None;
            shard.write_header().unwrap();
            store.adopt(&shard);
        }

        #[test]
        fn batch_and_same_key_updates_keep_the_final_complete_bytes() {
            let root = scratch("batch");
            let store = Store::new(&root);
            store
                .publish(&[item(1, b"first"), item(2, b"other"), item(1, b"last")])
                .unwrap();
            assert_eq!(
                store.get(EntryKind::Check, &Hash128(1)).as_deref(),
                Some(b"last".as_slice())
            );
            store.publish(&[item(1, b"replacement")]).unwrap();
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(1))
                    .as_deref(),
                Some(b"replacement".as_slice())
            );
            assert_eq!(load(&store).len(), 2);
        }

        #[test]
        fn independent_publishers_merge_without_lost_or_partial_entries() {
            let root = scratch("publishers");
            std::thread::scope(|scope| {
                for key in 0..16_u8 {
                    let root = &root;
                    scope.spawn(move || {
                        let store = Store::new(root);
                        for n in 0..4 {
                            store.publish(&[item(u128::from(key), &[key, n])]).unwrap();
                        }
                    });
                }
            });
            let reader = Store::new(&root);
            assert_eq!(reader.entries().len(), 16);
            for key in 0..16_u8 {
                assert_eq!(
                    reader
                        .get(EntryKind::Check, &Hash128(u128::from(key)))
                        .as_deref(),
                    Some([key, 3].as_slice())
                );
            }
        }

        /// Splits keep every key findable, by readers and writers, and a
        /// split that stopped after its first page write loses nothing.
        #[test]
        fn buckets_split_and_merge_without_losing_a_key() {
            let root = scratch("split");
            let store = Store::new(&root);
            for chunk in 0..20_u128 {
                let batch: Vec<_> = (chunk * 50..(chunk + 1) * 50)
                    .map(|k| item(k.wrapping_mul(0x9e37_79b9_7f4a_7c15), &k.to_le_bytes()))
                    .collect();
                store.publish(&batch).unwrap();
            }
            let reader = Store::new(&root);
            for k in 0..1000_u128 {
                let key = Hash128(k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                assert_eq!(
                    reader.get(EntryKind::Check, &key).as_deref(),
                    Some(k.to_le_bytes().as_slice())
                );
            }
            let mut shard = store.open_shard(0).unwrap();
            shard.fold().unwrap();
            assert_eq!(shard.txn().unwrap().head.rows, 1000);
            assert_eq!(shard.txn().unwrap().head.buckets(), 29);
            drop(shard);
            // A fold that crashed after its superset page writes, before
            // the head: the old head still finds every row.
            let mut shard = store.open_shard(0).unwrap();
            let epoch = shard.header.epoch;
            let txn = shard.txn().unwrap();
            let before = txn.head;
            for k in 1000..1100_u128 {
                txn.set(
                    (EntryKind::Check as u16, k),
                    Location {
                        pack: 1,
                        offset: 0,
                        len: 0,
                        used: 0,
                    },
                )
                .unwrap();
            }
            assert!(txn.head.buckets() > before.buckets());
            for (&bucket, page) in &txn.pages {
                let mut rows = page.rows.clone();
                rows.extend(
                    page.disk
                        .iter()
                        .filter(|(k, _)| !page.rows.contains_key(k))
                        .map(|(k, l)| (*k, *l)),
                );
                if bucket < before.buckets() {
                    replace_file(&txn.dir, &page_name(bucket), &page_encode(&rows, epoch)).unwrap();
                }
            }
            drop(shard);
            let reader = Store::new(&root);
            for k in 0..1000_u128 {
                let key = Hash128(k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                assert!(reader.get(EntryKind::Check, &key).is_some(), "{k}");
            }
            // Eviction merges buckets back and leaves no page, head or table.
            store.collect(0).unwrap();
            // cost: a test listing one small directory.
            assert!(
                std::fs::read_dir(store.index_dir(0))
                    .unwrap()
                    .next()
                    .is_none()
            );
        }

        #[test]
        fn gc_respects_a_live_unpublished_lease_and_reclaims_a_crashed_one() {
            let root = scratch("lease");
            let store = Store::new(&root);
            let mut lease = { store.lease(0).unwrap() };
            lease.file.write_all(b"unpublished").unwrap();
            let collector = Store::new(&root);
            assert_eq!(collector.collect(0).unwrap().after, 0);
            assert!(
                lease.path.exists(),
                "GC must not delete an active writer's pack"
            );
            let path = lease.path.clone();
            // Model OS release at process death, without running Lease's
            // normal error cleanup; the file has never entered the index.
            lease.committed = true;
            drop(lease);
            // A pack renamed into place whose run still commits its rows
            // stays, though no row names it yet.
            let mut running = store.lease(0).unwrap();
            running.file.write_all(&footer(&Vec::new())).unwrap();
            store.finish(&mut running).unwrap();
            // Crashed writers' files: a lease never locked, an hour old; a
            // temporary index file; a page past the last bucket.
            let dir = store.index_dir(0);
            std::fs::create_dir_all(&dir).unwrap();
            let new = store.root.join(format!("{:032x}.new", unique()));
            let old = SystemTime::now() - Duration::from_hours(2);
            File::create(&new).unwrap().set_modified(old).unwrap();
            let fresh = store.root.join(format!("{:032x}.new", unique()));
            File::create(&fresh).unwrap();
            let temp = dir.join(format!("head.{:032x}.tmp", unique()));
            std::fs::write(&temp, b"partial").unwrap();
            let stale = dir.join(page_name(7));
            std::fs::write(&stale, b"split page").unwrap();
            assert_eq!(collector.collect(0).unwrap().after, HEADER as u64);
            assert!(!path.exists());
            assert!(running.path.exists(), "a running publication's pack stays");
            assert!(!new.exists() && fresh.exists());
            assert!(!temp.exists() && !stale.exists());
            drop(running);
            assert_eq!(collector.collect(0).unwrap().after, 0);
        }

        /// Runs that share a shard wait only for each other's commit there,
        /// counted.
        #[test]
        fn a_publisher_sharing_a_shard_waits_for_its_commit_only() {
            let root = scratch("shared-shard");
            let store = Store::new(&root);
            store.publish(&[item(1, b"first")]).unwrap();
            let held = store.open_shard(0).unwrap();
            // cost: one test thread, one publication.
            let publisher = std::thread::spawn(move || {
                cost::take();
                Store::new(&root).publish(&[item(2, b"second")]).unwrap();
                cost::take()
            });
            std::thread::sleep(Duration::from_millis(100));
            drop(held);
            let counts = publisher.join().unwrap();
            assert_eq!(
                (counts.writer_locks, counts.lock_waits),
                (1, 1),
                "{counts:?}"
            );
        }

        #[test]
        fn crashed_subprocess_lease_is_recoverable() {
            let root = scratch("killed-writer");
            let marker = root.join("ready");
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "disk::packed::tests::crash_writer_fixture",
                    "--ignored",
                    "--skip",
                ])
                .arg(format!("cache-fixture:{}", root.display()))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let started = marker.exists();
            let kept = if started {
                Store::new(&root).collect(0).unwrap();
                // cost: a test listing one small directory.
                std::fs::read_dir(root.join(NAMESPACE))
                    .unwrap()
                    .filter_map(Result::ok)
                    .any(|e| {
                        e.file_name()
                            .to_str()
                            .is_some_and(|name| name.ends_with(".pending"))
                    })
            } else {
                false
            };
            let _ = child.kill();
            child.wait().unwrap();
            assert!(started, "writer fixture started");
            assert!(kept, "a live subprocess lease survives GC");
            assert_eq!(Store::new(&root).collect(0).unwrap().after, 0);
        }

        #[test]
        #[ignore = "subprocess fixture invoked by crashed_subprocess_lease_is_recoverable"]
        fn crash_writer_fixture() {
            let root = std::env::args()
                .find_map(|a| a.strip_prefix("cache-fixture:").map(PathBuf::from))
                .unwrap();
            let store = Store::new(&root);
            let mut lease = { store.lease(0).unwrap() };
            lease.file.write_all(b"partial frame").unwrap();
            std::fs::write(root.join("ready"), []).unwrap();
            std::thread::sleep(Duration::from_mins(1));
        }

        #[test]
        fn missing_and_truncated_pack_ranges_are_misses_without_recreation() {
            let root = scratch("missing");
            let store = Store::new(&root);
            store.publish(&[item(1, b"one"), item(2, b"two")]).unwrap();
            let path = store.entries()[0].pack.clone();
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(2)
                .unwrap();
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(1))
                    .is_none()
            );
            std::fs::remove_file(&path).unwrap();
            let reader = Store::new(&root);
            assert!(reader.get(EntryKind::Check, &Hash128(2)).is_none());
            reader.touch(EntryKind::Check, &Hash128(2));
            assert_eq!(reader.collect(0).unwrap().after, 0);
            assert!(!path.exists());
        }

        #[test]
        fn index_validation_bounds_allocations_and_recovers_after_corruption() {
            let root = scratch("corrupt");
            let store = Store::new(&root);
            store.publish(&[item(1, b"value")]).unwrap();
            store.open_shard(0).unwrap().fold().unwrap();
            let dir = store.index_dir(0);
            let page = dir.join(page_name(0));
            let good = std::fs::read(&page).unwrap();
            // Byte 12 is the page's fold epoch, which any value may take.
            for at in [0, 8, 16, HEADER, HEADER + ROW - 1] {
                let mut bad = good.clone();
                bad[at] ^= 0xff;
                assert!(page_decode(&bad).is_err());
                std::fs::write(&page, bad).unwrap();
                assert!(
                    Store::new(&root)
                        .get(EntryKind::Check, &Hash128(1))
                        .is_none()
                );
            }
            std::fs::write(&page, &good).unwrap();
            let head = std::fs::read(dir.join("head")).unwrap();
            for at in [0, 8, 12, 16, 24, 32, 48] {
                let mut bad = head.clone();
                bad[at] ^= 0xff;
                assert!(Head::decode(&bad).is_err());
            }
            let (mut entries, _) = page_decode(&good).unwrap();
            entries.values_mut().next().unwrap().len = MAX_ENTRY + 1;
            assert!(page_decode(&page_encode(&entries, 0)).is_err());
            entries.values_mut().next().unwrap().len = 2;
            entries.values_mut().next().unwrap().offset = u64::MAX;
            assert!(page_decode(&page_encode(&entries, 0)).is_err());
            let file = OpenOptions::new().write(true).open(&page).unwrap();
            file.set_len(u64::try_from(MAX_INDEX + 1).unwrap()).unwrap();
            assert!(read_page(&dir, 0).is_err());
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(1))
                    .is_none()
            );
            store.publish(&[item(2, b"recovered")]).unwrap();
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(2))
                    .as_deref(),
                Some(b"recovered".as_slice())
            );
            assert_eq!(load(&store).len(), 1);
            // A corrupt head under a torn log header loses the pages; the
            // log's batches, checksummed on their own, still name key 2.
            std::fs::write(dir.join("head"), b"HDHEAD01 not a head").unwrap();
            let mut log = OpenOptions::new()
                .write(true)
                .open(store.log_path(0))
                .unwrap();
            log.write_all(b"torn").unwrap();
            drop(log);
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(2))
                    .is_some()
            );
            store.publish(&[item(3, b"again")]).unwrap();
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(3))
                    .as_deref(),
                Some(b"again".as_slice())
            );
        }

        /// The unsharded namespace `pack-v1` an older hd wrote is not read:
        /// its entries are misses. The first collection removes its
        /// managed files, keeps a lease a live writer holds, and leaves a
        /// foreign file with its directory (#1932).
        #[test]
        fn the_unsharded_namespace_reads_as_misses_and_a_collection_retires_it() {
            let root = scratch("retire-v1");
            let old = root.join("pack-v1");
            std::fs::create_dir_all(old.join("index")).unwrap();
            let id = unique();
            std::fs::write(old.join(format!("{id:032x}.pack")), b"onetwo").unwrap();
            std::fs::write(old.join("lock"), b"").unwrap();
            std::fs::write(old.join("index").join("head"), b"old head").unwrap();
            std::fs::write(old.join("index").join(page_name(0)), b"old page").unwrap();
            let live = old.join(format!("{:032x}.pending", unique()));
            let lease = File::create(&live).unwrap();
            lease.lock().unwrap();
            std::fs::write(old.join("index.old"), b"older index").unwrap();
            let foreign = old.join("notes.txt");
            std::fs::write(&foreign, b"keep").unwrap();
            let store = Store::new(&root);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_none());
            store.collect(u64::MAX).unwrap();
            assert!(!old.join("index").exists());
            assert!(!old.join("lock").exists());
            assert!(!old.join("index.old").exists());
            assert!(!old.join(format!("{id:032x}.pack")).exists());
            assert!(live.exists(), "a live writer's lease stays");
            assert!(foreign.exists());
            drop(lease);
            std::fs::remove_file(foreign).unwrap();
            store.collect(u64::MAX).unwrap();
            assert!(!old.exists());
        }

        #[test]
        fn entry_recency_and_compaction_preserve_the_survivors_ages() {
            let root = scratch("lru");
            let store = Store::new(&root);
            let data = [7; 4000];
            store
                .publish(&[item(1, &data), item(2, &data), item(3, &data)])
                .unwrap();
            ages(&store, &[(1, 1), (2, 2), (3, 3)]);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            let before = store.bytes();
            let result = store.collect(14_000).unwrap();
            assert_eq!(result.before, before);
            assert_eq!(result.after, pack_len(2, 4000) + index_len(1));
            let entries = load(&store);
            assert!(!entries.contains_key(&(EntryKind::Check as u16, 2)));
            assert_eq!(entries[&(EntryKind::Check as u16, 3)].used, 3);
            assert!(entries[&(EntryKind::Check as u16, 1)].used > 3);
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(3))
                    .as_deref(),
                Some(data.as_slice())
            );
        }

        /// Three dead packs put the cache over a cap its one live record
        /// fits. One collection reclaims them all, down to the cap,
        /// without evicting that record (#1932).
        #[test]
        fn one_collection_reclaims_garbage_without_evicting_a_fitting_live_set() {
            let root = scratch("bounded-progress");
            let store = Store::new(&root);
            for _ in 0..4 {
                store.publish(&[item(1, &[7; 4000])]).unwrap();
            }
            let cap = pack_len(1, 4000) + index_len(1);
            let run = store.collect(cap).unwrap();
            assert!(run.before > cap);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            assert_eq!(run.after, cap);
        }

        /// A command's settlement never walks (#1932). It reads the
        /// recorded total and wants a collection when it is over the cap or
        /// unknown. Only a collection walks, and it records the total that
        /// the commands after it read.
        #[test]
        fn a_recorded_total_under_the_cap_skips_the_walk_and_a_stale_one_rescans() {
            let root = scratch("total");
            let store = Store::new(&root);
            let settle = |cap| {
                cost::take();
                let wanted = store.settle(cap).unwrap();
                let counts = cost::take();
                assert_eq!((counts.walks, counts.dirs_listed), (0, 0), "{counts:?}");
                wanted
            };
            // A publication adds its pack; log records are not counted.
            let one = pack_len(1, 64);
            store.publish(&[item(1, &[7; 64])]).unwrap();
            // A namespace the store created starts at a known zero.
            assert!(!settle(one));
            assert!(settle(one - 1), "over the cap");
            // An unindexed pack no writer recorded: only a walk sees it.
            let stray = store.path(unique());
            std::fs::write(&stray, [0; 100]).unwrap();
            store.publish(&[item(1, &[8; 64])]).unwrap();
            assert!(!settle(2 * one), "a publication adds its pack");
            assert!(stray.exists(), "a fitting total walks nothing");
            // The collection reclaims dead bytes without evicting the
            // fitting live set, folds the log, and records the exact total.
            let folded = pack_len(1, 64) + index_len(1);
            let run = store.collect(folded + 63).unwrap();
            assert_eq!((run.before, run.after), (2 * one + 100, folded));
            assert!(!stray.exists());
            assert_eq!(store.get(EntryKind::Check, &Hash128(1)).unwrap()[0], 8);
            assert!(!settle(folded));
            // An unknown total wants a collection.
            ages(&store, &[(1, 5)]);
            unrecorded(&store);
            assert!(settle(u64::MAX), "an unknown total wants a collection");
            assert_eq!(store.collect(u64::MAX).unwrap().after, folded);
            assert!(!settle(u64::MAX));
            // A hit's recency mark appends one change and keeps the total.
            store.get(EntryKind::Check, &Hash128(1)).unwrap();
            assert!(!settle(folded));
            assert!(load(&store).values().all(|loc| loc.used > 5));
            // A torn header is unknown, not a wrong total.
            let mut log = OpenOptions::new()
                .write(true)
                .open(store.log_path(0))
                .unwrap();
            log.seek(SeekFrom::Start(80)).unwrap();
            log.write_all(&[0xff; 4]).unwrap();
            // A run that has not written the shard reads its header.
            let other = Store::new(&root);
            other.published.store(1, Ordering::Relaxed);
            assert!(
                other.settle(u64::MAX).unwrap(),
                "a torn header wants a collection"
            );
        }

        /// The index files' names, bytes and modification times.
        fn index_files(store: &Store) -> Vec<(std::ffi::OsString, Vec<u8>, SystemTime)> {
            // cost: a test listing one small directory.
            let mut files: Vec<_> = std::fs::read_dir(store.index_dir(0))
                .unwrap()
                // The lock holds the running total, which a collection
                // rewrites.
                .filter(|e| e.as_ref().unwrap().file_name() != "lock")
                .map(|e| {
                    let e = e.unwrap();
                    let modified = e.metadata().unwrap().modified().unwrap();
                    (e.file_name(), std::fs::read(e.path()).unwrap(), modified)
                })
                .collect();
            files.sort_stable_by(|a, b| a.0.cmp(&b.0));
            files
        }

        #[test]
        fn recent_hits_do_not_rewrite_the_index() {
            let root = scratch("recent");
            let store = Store::new(&root);
            store.publish(&[item(1, b"value")]).unwrap();
            ages(&store, &[(1, now() - 60)]);
            store.open_shard(0).unwrap().fold().unwrap();
            let before = index_files(&store);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            store.touch(EntryKind::Check, &Hash128(1));
            store.collect(u64::MAX).unwrap();
            assert_eq!(index_files(&store), before);
        }

        #[test]
        fn open_readers_survive_compaction_and_pending_deletes_are_retried() {
            let root = scratch("open-reader");
            let store = Store::new(&root);
            let data = [5; 4000];
            store
                .publish(&[item(1, &data), item(2, &data), item(3, &data)])
                .unwrap();
            ages(&store, &[(1, 1), (2, 2), (3, 3)]);
            let path = store.entries()[0].pack.clone();
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // READ|WRITE, deliberately withholding FILE_SHARE_DELETE.
                options.share_mode(3);
            }
            let mut pinned = options.open(&path).unwrap();
            let cap = 14_000;
            let result = store.collect(cap).unwrap();

            #[cfg(windows)]
            {
                assert!(path.exists(), "a reader denied deletion");
                assert!(result.after > cap);
            }
            #[cfg(not(windows))]
            assert!(result.after <= cap);
            let mut old = Vec::new();
            pinned.read_to_end(&mut old).unwrap();
            assert_eq!(
                old[..12_000],
                [data.as_slice(), data.as_slice(), data.as_slice()].concat()
            );
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(3))
                    .as_deref(),
                Some(data.as_slice())
            );
            drop(pinned);
            assert!(store.collect(cap).unwrap().after <= cap);
            assert!(!path.exists());
        }

        #[test]
        fn readers_do_not_wait_for_the_publication_lock() {
            let root = scratch("unlocked-reader");
            let store = Store::new(&root);
            store.publish(&[item(1, b"readable")]).unwrap();
            let _lock = store.open_shard(0).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                tx.send(Store::new(&root).get(EntryKind::Check, &Hash128(1)))
                    .unwrap();
            });
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)).unwrap().as_deref(),
                Some(b"readable".as_slice())
            );
            reader.join().unwrap();
        }

        /// Asserts every row `root`'s index lists lies within its pack,
        /// and returns the distinct packs.
        fn indexed_packs(root: &Path) -> BTreeSet<PathBuf> {
            DiskStore::new(root.to_path_buf())
                .index_entries()
                .into_iter()
                .map(|e| {
                    let len = e.pack_len.expect("indexed pack");
                    assert!(e.offset + e.len <= len, "{e:?} past its pack");
                    e.pack
                })
                .collect()
        }

        /// Cold, then warm with another process's merge between (#1538):
        /// every indexed pack exists after each step, and a reader whose
        /// page predates the merge reads the index again once and hits.
        #[test]
        fn a_reader_older_than_a_merge_reads_the_index_again_once() {
            let root = scratch("stale-reader");
            let data = [7; 100];
            let writer = Store::new(&root);
            for key in 1..=FANOUT as u128 {
                writer.publish(&[item(key, &data)]).unwrap();
            }
            assert_eq!(indexed_packs(&root).len(), FANOUT);
            let reader = Store::new(&root);
            assert_eq!(
                reader.get(EntryKind::Check, &Hash128(1)).as_deref(),
                Some(data.as_slice())
            );
            Store::new(&root).collect(u64::MAX).unwrap();
            assert_eq!(indexed_packs(&root).len(), 1, "the four packs merged");
            let _ = cost::take();
            for key in 2..=FANOUT as u128 {
                assert_eq!(
                    reader.get(EntryKind::Check, &Hash128(key)).as_deref(),
                    Some(data.as_slice()),
                    "key {key} after the merge"
                );
            }
            // One refresh for the vanished pack: the head and the one page,
            // then the merged pack, opened once for all three keys.
            let counts = cost::take();
            assert_eq!(counts.index_read, (LOG_HEADER + page_len(FANOUT)) as u64);
            assert_eq!(counts.packs_opened, 1);
            assert_eq!(counts.dirs_listed, 0);
        }

        /// Two writers share one cache (#1538). One publishes and then
        /// merges, as each hd process does on exit; the other publishes,
        /// reads back every key it published, and lists the index. No
        /// read misses, and every listed row lies within its pack.
        #[test]
        fn two_writers_merging_keep_every_indexed_pack_readable() {
            const N: u128 = 64;
            let root = scratch("two-writers");
            std::thread::scope(|scope| {
                let root = &root;
                // cost: one test thread, N publications and merges.
                scope.spawn(move || {
                    for key in 0..N {
                        let store = Store::new(root);
                        store.publish(&[item(1_000 + key, &[1; 64])]).unwrap();
                        store.settle(u64::MAX).unwrap();
                    }
                });
                let store = Store::new(root);
                for key in 0..N {
                    store.publish(&[item(key, &[2; 64])]).unwrap();
                    for k in 0..=key {
                        assert_eq!(
                            store.get(EntryKind::Check, &Hash128(k)).as_deref(),
                            Some([2; 64].as_slice()),
                            "key {k} after publishing {key}"
                        );
                    }
                    indexed_packs(root);
                }
            });
        }

        /// The keys a warm run reads, the same at every cache size.
        const READ: u128 = 8;
        const DEFAULT: u64 = super::super::DEFAULT_CAP;

        /// A cache of `n` entries that earlier runs published. With
        /// `stale`, every entry was last used two hours ago, so each hit is
        /// due a recency mark.
        fn cache(n: u128, stale: bool) -> PathBuf {
            let root = scratch(&format!("warm-{n}"));
            {
                let store = DiskStore::new(root.clone());
                let batch: Vec<_> = (0..n).map(|key| item(key, &[7; 16])).collect();
                store.put_many(&batch);
            }
            if stale {
                let store = Store::new(&root);
                {
                    let mut shard = store.open_shard(0).unwrap();
                    let at = now() - 2 * 3600;
                    for (key, loc) in shard.all().unwrap() {
                        shard.set(key, Location { used: at, ..loc });
                    }
                    shard.commit(0, || Ok(())).unwrap();
                }
                // Record the total for the rewritten index, as the
                // command that wrote it would have.
                store.collect(super::super::DEFAULT_CAP).unwrap();
            }
            root
        }

        /// One warm run over a cache of `n` entries: a fresh store reads
        /// [`READ`] existing keys and is dropped, as a command ends. The
        /// run's file-system counts.
        fn warm_run(n: u128, stale: bool) -> cost::Counts {
            let root = cache(n, stale);
            cost::take();
            {
                let store = DiskStore::new(root);
                for key in 0..READ {
                    assert!(store.get(EntryKind::Check, &Hash128(key)).is_some());
                }
            }
            cost::take()
        }

        /// Cost Model: a warm run lists no directory and opens only the
        /// packs it reads, at any cache size.
        #[test]
        fn warm_runs_list_no_directory_and_open_the_same_packs_at_ten_times_the_entries() {
            for stale in [false, true] {
                let (small, large) = (warm_run(1000, stale), warm_run(10_000, stale));
                eprintln!("stale {stale}: {small:?} {large:?}");
                for run in [small, large] {
                    assert_eq!((run.dirs_listed, run.entries_listed), (0, 0), "{run:?}");
                }
                assert_eq!(
                    small.packs_opened, large.packs_opened,
                    "{small:?} {large:?}"
                );
                if !stale {
                    // Fresh hits mark nothing, so nothing is written.
                    assert_eq!((small.index_written, large.index_written), (0, 0));
                }
            }
        }

        /// The target of #1502: a warm run reads and writes index bytes in
        /// proportion to the keys it touches, not to the cache's size.
        #[test]
        fn warm_runs_read_and_write_index_bytes_flat_in_the_cache_size() {
            for stale in [false, true] {
                let (small, large) = (warm_run(1000, stale), warm_run(10_000, stale));
                assert_eq!(small.index_read, large.index_read, "stale {stale}");
                assert_eq!(small.index_written, large.index_written, "stale {stale}");
            }
        }

        /// A run that publishes one entry reads and writes a few pages, the
        /// head and the pack table, at any cache size, and walks nothing.
        #[test]
        fn a_publishing_run_writes_index_bytes_flat_in_the_cache_size() {
            let mut runs = Vec::new();
            for n in [1000, 10_000] {
                let root = cache(n, false);
                cost::take();
                {
                    let store = DiskStore::new(root);
                    assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
                    store.put_many(&[item(u128::MAX, b"new")]);
                }
                runs.push(cost::take());
            }
            eprintln!("{runs:?}");
            let bound = (6 * page_len(1) + 2 * HEAD_LEN + 2 * (HEADER + 2 * TABLE_ROW)) as u64;
            for run in &runs {
                assert_eq!((run.dirs_listed, run.entries_listed), (0, 0), "{run:?}");
                assert!(
                    run.index_read <= bound && run.index_written <= bound,
                    "{run:?}"
                );
            }
        }

        /// Publishing runs merge small packs as they go, so the pack count
        /// stays bounded with no directory walk.
        #[test]
        fn a_thousand_publishing_runs_leave_a_bounded_pack_count() {
            let root = scratch("pack-count");
            let packs = || {
                // cost: a test listing one small directory.
                std::fs::read_dir(root.join(NAMESPACE))
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_str().and_then(Store::pack_id).is_some())
                    .count()
            };
            let mut most = 0;
            for key in 0..1000_u128 {
                {
                    let store = DiskStore::new(root.clone());
                    store.put_many(&[item(key, &[7; 100])]);
                }
                most = most.max(packs());
            }
            eprintln!("packs: at most {most}, at the end {}", packs());
            assert!(most <= 24, "{most} packs");
            let reader = Store::new(&root);
            for key in 0..1000 {
                assert_eq!(
                    reader.get(EntryKind::Check, &Hash128(key)).as_deref(),
                    Some([7; 100].as_slice())
                );
            }
        }

        #[test]
        fn growth_with_a_thousand_historical_packs() {
            let root = scratch("growth");
            let store = Store::new(&root);
            for key in 0..1000 {
                store.publish(&[item(key, b"history")]).unwrap();
            }
            assert_eq!(
                std::fs::read_dir(&store.root)
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_str().and_then(Store::pack_id).is_some())
                    .count(),
                1000
            );
            let at = Instant::now();
            let entries = load(&store);
            let load_time = at.elapsed();
            assert_eq!(entries.len(), 1000);
            let at = Instant::now();
            store.publish(&[item(500, b"new")]).unwrap();
            let merge = at.elapsed();
            assert_eq!(load(&store).len(), 1000);
            let reader = Store::new(&root);
            for key in 0..1000 {
                assert_eq!(
                    reader.get(EntryKind::Check, &Hash128(key)).as_deref(),
                    Some(if key == 500 {
                        b"new".as_slice()
                    } else {
                        b"history".as_slice()
                    })
                );
            }
            assert!(reader.state().as_ref().unwrap().files.len() <= MAX_HANDLES);
            let collected = store.collect(16 * 1024).unwrap();
            assert!(collected.after <= 16 * 1024, "{collected:?}");
            eprintln!(
                "1000 packs: load {load_time:?}, publication including merge {merge:?}, bytes {} -> {}",
                collected.before, collected.after
            );
        }

        /// A collection in another process never delays a publication
        /// (#1932). One store collects and stops at its walk, then at its
        /// compaction copy. At each stop the writer lock is free, and a
        /// second store over the same cache publishes and reads back.
        #[test]
        fn publication_never_waits_for_a_collection() {
            let root = scratch("gc-beside-publish");
            let data = [3; 4000];
            let writer = Store::new(&root);
            writer
                .publish(&[item(1, &data), item(2, &data), item(3, &data)])
                .unwrap();
            ages(&writer, &[(1, 1), (2, 2), (3, 3)]);
            let (at, stopped) = std::sync::mpsc::channel::<&'static str>();
            let (resume, go) = std::sync::mpsc::channel::<()>();
            let go = Mutex::new(go);
            let mut collector = Store::new(&root);
            collector.on_pause = Some(Arc::new(move |stop| {
                at.send(stop).unwrap();
                go.lock().unwrap().recv().unwrap();
            }));
            std::thread::scope(|scope| {
                // Evicting key 1 leaves its pack a third dead: compacted.
                // cost: one test thread, one collection of three entries.
                let collecting = scope.spawn(|| collector.collect(14_000).unwrap());
                let mut stops = Vec::new();
                for key in 10.. {
                    let Ok(stop) = stopped.recv_timeout(Duration::from_secs(2)) else {
                        break;
                    };
                    stops.push(stop);
                    let lock = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(Store::new(&root).log_path(0))
                        .unwrap();
                    assert!(lock.try_lock().is_ok(), "the writer lock is held at {stop}");
                    lock.unlock().unwrap();
                    let publisher = Store::new(&root);
                    publisher.publish(&[item(key, b"beside")]).unwrap();
                    if stop == "read" {
                        // A fold during the read moves rows between pages.
                        publisher.open_shard(0).unwrap().fold().unwrap();
                    }
                    assert_eq!(
                        Store::new(&root)
                            .get(EntryKind::Check, &Hash128(key))
                            .as_deref(),
                        Some(b"beside".as_slice())
                    );
                    resume.send(()).unwrap();
                }
                drop(resume);
                // A fold during each read of the shard changes its epoch:
                // the read is tried three times, then taken as inconsistent.
                // Then the compaction's copy, the merge of the published
                // packs, and the last walk.
                assert_eq!(
                    stops,
                    ["walk", "read", "read", "read", "copy", "copy", "walk"]
                );
                collecting.join().unwrap();
            });
            let rows = load(&writer);
            for key in [2, 3, 10, 11, 12, 13, 14, 15] {
                assert!(rows.contains_key(&(EntryKind::Check as u16, key)), "{key}");
            }
            assert!(!rows.contains_key(&(EntryKind::Check as u16, 1)));
            indexed_packs(&root);
        }

        /// One command: a fresh store reads [`READ`] keys, writes one new
        /// entry and is dropped; its counts, and whether it asked for a
        /// collection. Its spawner records the request, so the command
        /// never collects itself.
        fn command(root: &Path, cap: u64, new: u128) -> (cost::Counts, bool) {
            static ASKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
            fn ask(root: &Path) {
                ASKED.lock().unwrap().push(root.to_path_buf());
            }
            let asked = |root: &Path| ASKED.lock().unwrap().iter().filter(|r| *r == root).count();
            let before = asked(root);
            cost::take();
            {
                let store = DiskStore::new(root.to_path_buf())
                    .with_cap(cap)
                    .on_collection_due(ask);
                for key in 0..READ {
                    store.get(EntryKind::Check, &Hash128(key));
                }
                store.put_many(&[item(new, &[9; 16])]);
            }
            (cost::take(), asked(root) > before)
        }

        /// The cache's invariant (#1932): a command never does more work
        /// with the cache than with none. The same command runs on an
        /// empty cache, then under every state a cache can be in. No
        /// cached run lists a directory or walks; none takes the writer
        /// lock more often than the cold run, or reads or writes more
        /// index bytes beyond the pages of the entries it reads, writes
        /// and marks, and it opens only the packs of its hits.
        #[test]
        fn a_command_does_no_more_work_with_the_cache_than_without() {
            // The cache directory does not exist yet, as under a new HD_CACHE.
            let (cold, asked) = command(&scratch("no-cache").join("obj"), DEFAULT, u128::MAX);
            assert!(!asked, "an empty cache's total is known");
            assert_eq!((cold.dirs_listed, cold.walks), (0, 0), "{cold:?}");
            let mut states: Vec<(&str, PathBuf, u64, bool)> = Vec::new();
            for n in [1000, 10_000] {
                let under = cache(n, false);
                states.push(("under the cap", under, DEFAULT, false));
                states.push(("over the cap", cache(n, false), 1, true));
                let unknown = cache(n, false);
                unrecorded(&Store::new(&unknown));
                states.push(("total unknown", unknown, DEFAULT, true));
                let ledger = cache(n, true);
                {
                    let store = DiskStore::new(ledger.clone());
                    store.put_private(EntryKind::Clpack, &Hash128(5), b"native code");
                }
                std::fs::write(ledger.join("stats"), b"stale").unwrap();
                // No collection covers it yet.
                std::fs::remove_file(ledger.join("gc")).unwrap();
                states.push(("stale ledger", ledger, DEFAULT, true));
                let collecting = cache(n, false);
                unrecorded(&Store::new(&collecting));
                let gc = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(collecting.join("gc"))
                    .unwrap();
                gc.lock().unwrap();
                std::mem::forget(gc);
                states.push(("another process collecting", collecting, DEFAULT, true));
                let crashed = cache(n, false);
                let store = Store::new(&crashed);
                let mut lease = { store.lease(0).unwrap() };
                lease.file.write_all(b"partial").unwrap();
                lease.committed = true;
                drop(lease);
                states.push(("crashed lease", crashed, DEFAULT, false));
            }
            // The head twice, the pages of the keys it reads and writes,
            // their pages again for the marks of stale hits, and a pack
            // table of a few rows, read and read again.
            let read = usize::try_from(READ).unwrap();
            let reads =
                (2 * HEAD_LEN + (2 * read + 1) * page_len(1) + 2 * (HEADER + 4 * TABLE_ROW)) as u64;
            let marks = (2 * read * page_len(1)) as u64;
            for (state, root, cap, wants) in states {
                let (run, asked) = command(&root, cap, u128::MAX);
                eprintln!("{state}: {run:?} cold {cold:?}");
                assert_eq!(asked, wants, "{state}");
                assert_eq!(
                    (run.dirs_listed, run.entries_listed, run.walks),
                    (0, 0, 0),
                    "{state}: {run:?}"
                );
                assert!(
                    run.writer_locks <= cold.writer_locks + 1,
                    "{state}: {run:?}"
                );
                assert!(run.packs_opened <= read as u64, "{state}: {run:?}");
                assert!(
                    run.index_read <= cold.index_read + reads,
                    "{state}: {run:?} {cold:?}"
                );
                assert!(
                    run.index_written <= cold.index_written + marks,
                    "{state}: {run:?} {cold:?}"
                );
            }
            // Readers never take the writer lock: a run of hits finishes
            // while another process holds it.
            let root = cache(1000, false);
            let held = Store::new(&root).open_shard(0).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            // cost: one test thread reading eight keys.
            let reader = std::thread::spawn(move || {
                cost::take();
                {
                    let store = DiskStore::new(root);
                    for key in 0..READ {
                        assert!(store.get(EntryKind::Check, &Hash128(key)).is_some());
                    }
                }
                tx.send(cost::take()).unwrap();
            });
            let run = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(run.writer_locks, 0, "{run:?}");
            drop(held);
            reader.join().unwrap();
        }

        /// Convergence (#1932): 10,000 packs, most of them dead, and a live
        /// set over the cap. Commands keep listing nothing and ask for a
        /// collection; one collection brings the bytes on disk to the
        /// low-water mark, and the commands after it ask for none.
        #[test]
        fn an_over_cap_cache_of_ten_thousand_packs_reaches_the_low_water_mark_in_one_collection() {
            const PACKS: u128 = 10_000;
            const LIVE: u128 = 2_000;
            let root = scratch("converge");
            let store = Store::new(&root);
            {
                let mut txn = store.open_shard(0).unwrap();
                for key in 0..PACKS {
                    // Home shard 0, whose table tracks it.
                    let id = unique() & !(0xf << 124);
                    let frame = vec![u8::try_from(key % 251).unwrap(); 100];
                    let mut bytes = frame.clone();
                    bytes.extend(footer(&vec![((EntryKind::Check as u16, key), 0, 100)]));
                    std::fs::write(store.path(id), &bytes).unwrap();
                    txn.track(id, bytes.len() as u64);
                    if key < LIVE {
                        let used = 1_000 + u64::try_from(key).unwrap();
                        let loc = Location {
                            pack: id,
                            offset: 0,
                            len: 100,
                            used,
                        };
                        txn.set((EntryKind::Check as u16, key), loc);
                    }
                }
                txn.commit(0, || Ok(())).unwrap();
            }
            let physical = || DiskStore::new(root.clone()).packed.bytes();
            let cap = 200_000;
            assert!(physical() > 5 * cap);
            for new in 0..3 {
                let (run, asked) = command(&root, cap, PACKS + new);
                assert!(asked);
                assert_eq!((run.dirs_listed, run.walks), (0, 0), "{run:?}");
            }
            cost::take();
            DiskStore::new(root.clone()).with_cap(cap).collect_due();
            assert_eq!(cost::take().walks, 2, "one collection walks twice");
            let after = physical();
            eprintln!("after one collection: {after} bytes, cap {cap}");
            assert!(after <= cap * 9 / 10 + page_len(1) as u64, "{after}");
            let (run, asked) = command(&root, cap, PACKS + 10);
            assert!(!asked);
            assert_eq!((run.dirs_listed, run.walks), (0, 0), "{run:?}");
            // The newest live entries survived.
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(LIVE - 1))
                    .is_some()
            );
        }

        /// Sixteen stores publish at once (#1932), each its own keys in
        /// its own shard, as runs of disjoint keys do. Counted: no
        /// publication waits for another's lock, each takes one shard lock
        /// per publish, and every entry reads back.
        #[test]
        fn sixteen_publishers_of_disjoint_keys_never_wait_for_each_other() {
            const RUNS: u128 = 20;
            let root = scratch("disjoint");
            let start = std::sync::Barrier::new(16);
            let counts: Vec<cost::Counts> = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..16_u128)
                    .map(|shard| {
                        let (root, start) = (&root, &start);
                        // cost: one test thread of RUNS publications.
                        scope.spawn(move || {
                            let store = Store::new(root);
                            start.wait();
                            cost::take();
                            for run in 0..RUNS {
                                let batch: Vec<_> = (0..8)
                                    .map(|n| item((shard << 124) | (run * 8 + n), &[7; 64]))
                                    .collect();
                                store.publish(&batch).unwrap();
                            }
                            cost::take()
                        })
                    })
                    .collect();
                workers.into_iter().map(|w| w.join().unwrap()).collect()
            });
            for c in &counts {
                assert_eq!(
                    (c.lock_waits, c.writer_locks),
                    (0, u64::try_from(RUNS).unwrap()),
                    "{c:?}"
                );
            }
            let reader = Store::new(&root);
            for shard in 0..16_u128 {
                for n in 0..RUNS * 8 {
                    assert!(
                        reader
                            .get(EntryKind::Check, &Hash128((shard << 124) | n))
                            .is_some()
                    );
                }
            }
            indexed_packs(&root);
        }

        /// Sixteen stores publish the same key at once, again and again:
        /// every read finds one whole value, and the index holds one row
        /// for it, inside its pack.
        #[test]
        fn publishers_of_one_key_leave_one_whole_entry() {
            let root = scratch("same-key");
            let start = std::sync::Barrier::new(16);
            std::thread::scope(|scope| {
                for writer in 0..16_u8 {
                    let (root, start) = (&root, &start);
                    // cost: one test thread of ten publications.
                    scope.spawn(move || {
                        let store = Store::new(root);
                        start.wait();
                        for _ in 0..10 {
                            store.publish(&[item(1, &[writer; 4096])]).unwrap();
                            let got = Store::new(root).get(EntryKind::Check, &Hash128(1)).unwrap();
                            assert!(got.iter().all(|&b| b == got[0]), "a torn value");
                        }
                    });
                }
            });
            let rows = DiskStore::new(root.clone()).index_entries();
            assert_eq!(rows.len(), 1);
            indexed_packs(&root);
            let got = Store::new(&root)
                .get(EntryKind::Check, &Hash128(1))
                .unwrap();
            assert_eq!(got.len(), 4096);
            assert!(got.iter().all(|&b| b == got[0]));
        }

        /// A reader whose view predates a fold that split a bucket finds a
        /// key the split moved: the newer page without it makes it read
        /// the shard again.
        #[test]
        fn a_reader_finds_a_key_a_later_fold_moved_to_a_new_bucket() {
            let root = scratch("fold-split");
            let writer = Store::new(&root);
            let key = |k: u128| k.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let batch = |from: u128, to: u128| -> Vec<_> {
                (from..to).map(|k| item(key(k), &k.to_le_bytes())).collect()
            };
            writer.publish(&batch(0, 30)).unwrap();
            writer.open_shard(0).unwrap().fold().unwrap();
            // The reader has read the shard's log, one bucket, and no page.
            let reader = Store::new(&root);
            let view = reader.read_view(0);
            assert_eq!(view.head.map(Head::buckets), Some(1));
            Store::snapshot(&mut reader.state()).views.insert(0, view);
            writer.publish(&batch(30, 60)).unwrap();
            let mut shard = writer.open_shard(0).unwrap();
            shard.fold().unwrap();
            let head = shard.txn().unwrap().head;
            assert_eq!(head.buckets(), 2);
            let moved = (0..30)
                .find(|&k| head.bucket(key(k)) == 1)
                .expect("a key the split moved");
            drop(shard);
            assert_eq!(
                reader
                    .get(EntryKind::Check, &Hash128(key(moved)))
                    .as_deref(),
                Some(moved.to_le_bytes().as_slice())
            );
        }

        /// A torn tail of a shard's log is skipped by readers and cut by
        /// the next writer; a batch whose checksum fails ends the log
        /// there, so the batches after it are not read either.
        #[test]
        fn a_torn_log_tail_is_cut_and_a_bad_batch_ends_the_log() {
            let root = scratch("torn-log");
            let store = Store::new(&root);
            store.publish(&[item(1, b"one")]).unwrap();
            let log = store.log_path(0);
            let append = |bytes: &[u8]| {
                let mut file = OpenOptions::new().append(true).open(&log).unwrap();
                file.write_all(bytes).unwrap();
            };
            let len = || std::fs::metadata(&log).unwrap().len();
            let whole = len();
            append(&[1, 2, 3]);
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(1))
                    .is_some()
            );
            drop(store.open_shard(0).unwrap());
            assert_eq!(len(), whole, "the writer cut the torn tail");
            let loc = |n: u64| Location {
                pack: 1,
                offset: 0,
                len: n,
                used: 0,
            };
            let mut bad = log_frame(&[Op::Set((EntryKind::Check as u16, 3), loc(3))]);
            let last = bad.len() - 1;
            bad[last] ^= 0xff;
            append(&bad);
            append(&log_frame(&[Op::Set((EntryKind::Check as u16, 4), loc(4))]));
            let reader = Store::new(&root);
            assert!(reader.get(EntryKind::Check, &Hash128(1)).is_some());
            assert!(reader.get(EntryKind::Check, &Hash128(4)).is_none());
            store.publish(&[item(5, b"five")]).unwrap();
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(5))
                    .as_deref(),
                Some(b"five".as_slice())
            );
            assert_eq!(load(&store).len(), 2);
        }

        /// A reader whose view of a log predates a fold takes the page the
        /// fold wrote, which is newer than the log's change it holds.
        #[test]
        fn a_page_a_later_fold_wrote_wins_over_an_older_log_view() {
            let root = scratch("fold-epoch");
            let writer = Store::new(&root);
            writer.publish(&[item(1, b"old")]).unwrap();
            let reader = Store::new(&root);
            // The reader reads shard 0's log, holding key 1's old change.
            assert!(reader.get(EntryKind::Check, &Hash128(2)).is_none());
            writer.publish(&[item(1, b"new")]).unwrap();
            writer.open_shard(0).unwrap().fold().unwrap();
            assert_eq!(
                reader.get(EntryKind::Check, &Hash128(1)).as_deref(),
                Some(b"new".as_slice())
            );
        }

        #[test]
        fn write_failure_warns_once_without_panicking() {
            static WARNINGS: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(0);
            fn warning(_: &Path, _: &io::Error) {
                WARNINGS.fetch_add(1, Ordering::Relaxed);
            }
            let root = scratch("write-failure");
            std::fs::write(root.join(NAMESPACE), b"not a directory").unwrap();
            let store = DiskStore::new(root).on_write_failure(warning);
            store.put(EntryKind::Check, &Hash128(1), b"value");
            store.put(EntryKind::Check, &Hash128(2), b"value");
            assert_eq!(WARNINGS.load(Ordering::Relaxed), 1);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_none());
            // A shard of the index that is no directory fails the same way.
            let root = scratch("shard-failure");
            std::fs::create_dir_all(root.join(NAMESPACE).join("index")).unwrap();
            std::fs::write(root.join(NAMESPACE).join("index/0"), b"not a shard").unwrap();
            let store = DiskStore::new(root).on_write_failure(warning);
            store.put(EntryKind::Check, &Hash128(1), b"value");
            assert_eq!(WARNINGS.load(Ordering::Relaxed), 2);
        }

        #[test]
        fn failed_collection_reports_the_data_it_could_not_remove() -> std::io::Result<()> {
            let root = scratch("gc-write-failure");
            let store = DiskStore::new(root.clone()).with_cap(0);
            store.put(EntryKind::Check, &Hash128(1), b"data");
            let lock = Store::new(&root).log_path(0);
            std::fs::remove_file(&lock).unwrap();
            std::fs::create_dir(&lock).unwrap();
            let before = store.packed.bytes();
            assert!(before > 0);
            let collected = store.collect()?;
            assert_eq!(collected.before, before);
            assert_eq!(collected.after, before);
            Ok(())
        }

        #[test]
        fn collection_shares_the_cap_with_private_entries_and_retires_old_data()
        -> std::io::Result<()> {
            let root = scratch("legacy-cap");
            let store = DiskStore::new(root).with_cap(256 * 1000);
            store.put(EntryKind::Clpack, &Hash128(1), &[2; 800]);
            store.publish(EntryKind::Check, &Hash128(2), b"obsolete", |path| {
                File::create(path)
            });
            store.put_many(&[item(3, &vec![3; 128 * 1024]), item(4, &vec![4; 128 * 1024])]);
            let result = store.collect()?;
            assert!(result.after <= 256 * 1000);
            assert!(!store.path(EntryKind::Check, &Hash128(2)).exists());
            assert!(store.get(EntryKind::Clpack, &Hash128(1)).is_some());
            assert!(store.get(EntryKind::Check, &Hash128(4)).is_some());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskStore, Ledger, SHARDS, UNKNOWN, parse_key};
    use crate::store::EntryKind;
    use hd_base::Hash128;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/hd_cache-tests"
        ))
        .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// A key in shard `shard`, told apart by `n`.
    fn key(shard: u8, n: u128) -> Hash128 {
        Hash128((u128::from(shard) << 120) | n)
    }

    fn age(store: &DiskStore, kind: EntryKind, k: Hash128, days: u64) {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(store.path(kind, &k))
            .expect("entry");
        let t = SystemTime::now() - Duration::from_secs(days * 86_400);
        f.set_modified(t).expect("mtime");
    }

    fn total(store: &DiskStore) -> u64 {
        let kinds = store.kinds();
        (0..=u8::MAX).fold(0_u64, |sum, shard| {
            sum.saturating_add(store.scan(&kinds, shard).map_or(u64::MAX, |scan| scan.0))
        })
    }

    fn ledger(root: &std::path::Path) -> std::io::Result<Ledger> {
        Ok(Ledger::decode(&std::fs::read(root.join("stats"))?))
    }

    /// Warm work depends on the keys read, not the private store's size,
    /// whether those entries occupy one shard or every shard.
    #[cfg(unix)]
    #[test]
    fn private_warm_runs_list_nothing_at_ten_times_the_entries() -> std::io::Result<()> {
        use crate::store::CacheStore as _;
        use std::os::unix::fs::MetadataExt as _;
        for spread in [false, true] {
            for n in [1_000_u128, 10_000] {
                let root = scratch(&format!("private-warm-{spread}-{n}"));
                let read_key = key(0, 0);
                let owner;
                {
                    let store = DiskStore::new(root.clone());
                    for i in 0..n {
                        let shard = if spread { i.to_le_bytes()[0] } else { 0 };
                        store.put_private(EntryKind::Clpack, &key(shard, i), b"native code");
                    }
                    owner = std::fs::metadata(store.path(EntryKind::Clpack, &read_key))?.uid();
                    store.put(EntryKind::Check, &key(0, 99), b"packed data");
                    assert!(store.collect().is_ok());
                }
                for packed in [false, true] {
                    super::cost::take();
                    {
                        let store = DiskStore::new(root.clone());
                        if packed {
                            assert!(store.get(EntryKind::Check, &key(0, 99)).is_some());
                        } else {
                            assert!(
                                store
                                    .get_private(EntryKind::Clpack, &read_key, owner)
                                    .is_some()
                            );
                        }
                    }
                    let counts = super::cost::take();
                    assert_eq!(
                        (counts.dirs_listed, counts.entries_listed),
                        (0, 0),
                        "entries={n}, spread={spread}, packed={packed}: {counts:?}"
                    );
                }
                std::fs::remove_dir_all(root)?;
            }
        }
        Ok(())
    }

    #[test]
    fn replacement_and_lost_publication_account_only_committed_bytes() -> std::io::Result<()> {
        let root = scratch("replacement-ledger");
        let store = DiskStore::new(root.clone());
        assert!(store.collect().is_ok());
        let k = key(5, 1);
        for size in [100, 300, 40, 40] {
            store.put_loose(EntryKind::Clpack, &k, &vec![0; size]);
            assert_eq!(ledger(&root)?.sizes[5], size as u64);
        }
        // An open temporary can disappear while its bytes are written.
        // The existing destination makes this a lost publication race.
        store.publish(EntryKind::Clpack, &k, &[0; 800], |tmp| {
            let file = std::fs::File::create(tmp)?;
            std::fs::remove_file(tmp)?;
            Ok(file)
        });
        assert_eq!(ledger(&root)?.sizes[5], 40);
        assert_eq!(
            std::fs::metadata(store.path(EntryKind::Clpack, &k))?.len(),
            40
        );
        drop(store);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn private_publication_accounts_through_a_selected_root_alias() -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt as _;
        let base = scratch("selected-root-alias");
        let root = base.join("objects");
        let alias = base.join("alias");
        std::fs::create_dir(&root)?;
        std::os::unix::fs::symlink(&root, &alias)?;
        let store = DiskStore::new(alias);
        store.collect()?;
        let k = key(5, 1);
        store.put_private(EntryKind::Clpack, &k, b"native code");
        assert!(
            !store
                .write_failed
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        assert_eq!(ledger(&root)?.sizes[5], 11);
        let owner = std::fs::metadata(store.path(EntryKind::Clpack, &k))?.uid();
        assert_eq!(
            store.get_private(EntryKind::Clpack, &k, owner).as_deref(),
            Some(&b"native code"[..])
        );
        super::cost::take();
        drop(store);
        let counts = super::cost::take();
        assert_eq!((counts.dirs_listed, counts.entries_listed), (0, 0));
        std::fs::remove_dir_all(base)?;
        Ok(())
    }

    #[test]
    fn interrupted_accounting_recovers_unknown_sizes_once() -> std::io::Result<()> {
        let root = scratch("interrupted-ledger");
        let store = DiskStore::new(root.clone());
        assert!(store.collect().is_ok());
        let path = store.path(EntryKind::Clpack, &key(0, 1));
        std::fs::create_dir_all(
            path.parent()
                .ok_or_else(|| std::io::Error::other("missing shard"))?,
        )?;
        let interrupted = std::panic::catch_unwind(|| {
            let _ = Ledger::update(&root, |_| {
                assert!(std::fs::write(&path, b"published before interruption").is_ok());
                assert!(
                    !path.exists(),
                    "simulate interruption before accounting commits"
                );
            });
        });
        assert!(interrupted.is_err());
        assert!(ledger(&root)?.sizes.iter().all(|&size| size == UNKNOWN));
        assert!(store.collect().is_ok());
        assert_eq!(ledger(&root)?.sizes[0], 29);
        super::cost::take();
        store.touch_loose(EntryKind::Clpack, &key(0, 1));
        drop(store);
        let counts = super::cost::take();
        assert_eq!((counts.dirs_listed, counts.entries_listed), (0, 0));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn packed_migration_retires_previously_known_loose_entries() -> std::io::Result<()> {
        use crate::store::CacheStore as _;
        let root = scratch("known-loose-migration");
        let store = DiskStore::new(root.clone());
        let k = key(0, 1);
        store.put_loose(EntryKind::Iface, &k, b"obsolete");
        let other = key(5, 1);
        store.put_loose(EntryKind::Iface, &other, b"also obsolete");
        assert!(store.collect().is_ok());
        assert!(!ledger(&root)?.migrated);
        store.put(EntryKind::Check, &k, b"packed");
        // Packed storage can appear between settle's work selection and
        // a shard's inventory. Untouched known shards must recover too.
        store.clean(&[0])?;
        assert_eq!(ledger(&root)?.sizes[5], UNKNOWN);
        assert!(store.collect().is_ok());
        assert!(!store.path(EntryKind::Iface, &k).exists());
        assert!(!store.path(EntryKind::Iface, &other).exists());
        assert!(ledger(&root)?.migrated);
        drop(store);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn incomplete_loose_inventory_cannot_grant_packed_storage_a_fresh_cap() -> std::io::Result<()> {
        use crate::store::CacheStore as _;
        let root = scratch("incomplete-ledger");
        let store = DiskStore::new(root.clone()).with_cap(0);
        store.put(EntryKind::Check, &key(0, 1), b"retained packed entry");
        store.put_loose(EntryKind::Clpack, &key(0, 2), b"retained private entry");
        std::fs::remove_file(root.join("stats"))?;
        std::fs::create_dir(root.join("stats"))?;
        assert!(store.collect().is_err());
        assert!(store.get(EntryKind::Check, &key(0, 1)).is_some());
        assert!(store.path(EntryKind::Clpack, &key(0, 2)).is_file());
        // Restore the ledger before Drop's independent automatic cleanup.
        std::fs::remove_dir(root.join("stats"))?;
        drop(store);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn a_hit_marks_use_so_the_entry_survives_eviction() -> std::io::Result<()> {
        let root = scratch("lru");
        let store = DiskStore::new(root.clone()).with_cap(256 * 1000);
        let (a, b, c) = (key(7, 1), key(7, 2), key(7, 3));
        for (k, days) in [(a, 3), (b, 2), (c, 1)] {
            store.put_loose(EntryKind::Check, &k, &[0; 400]);
            age(&store, EntryKind::Check, k, days);
        }
        // `a` is the oldest, but a hit makes it the newest.
        assert!(store.get_loose(EntryKind::Check, &a).is_some());
        let got = store.collect()?;
        assert_eq!((got.before, got.after), (1200, 800));
        assert!(store.path(EntryKind::Check, &a).exists());
        assert!(!store.path(EntryKind::Check, &b).exists());
        assert!(store.path(EntryKind::Check, &c).exists());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn a_recent_entry_keeps_its_mtime_on_a_hit() {
        let root = scratch("mark");
        let store = DiskStore::new(root.clone());
        let k = key(1, 1);
        store.put_loose(EntryKind::Iface, &k, b"x");
        let path = store.path(EntryKind::Iface, &k);
        let set = SystemTime::now() - Duration::from_mins(1);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("entry")
            .set_modified(set)
            .expect("mtime");
        store.touch_loose(EntryKind::Iface, &k);
        assert!(store.get_loose(EntryKind::Iface, &k).is_some());
        let now = std::fs::metadata(&path).and_then(|m| m.modified());
        assert_eq!(now.expect("mtime"), set);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_vanished_entry_is_a_miss_and_touch_never_recreates_it() {
        let root = scratch("vanish");
        let store = DiskStore::new(root.clone());
        let k = key(2, 9);
        store.put_loose(EntryKind::Link, &k, b"bytes");
        std::fs::remove_file(store.path(EntryKind::Link, &k)).expect("remove");
        assert!(store.get_loose(EntryKind::Link, &k).is_none());
        store.touch_loose(EntryKind::Link, &k);
        assert!(!store.path(EntryKind::Link, &k).exists());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn eviction_deletes_only_entry_files_of_the_obj_tree() -> std::io::Result<()> {
        let base = scratch("scope");
        let root = base.join("obj");
        let pkg = base.join("pkg/github.com/acme/json@1.0.0");
        std::fs::create_dir_all(&pkg).expect("pkg");
        std::fs::write(pkg.join("hd.toml"), "[package]\nname = \"json\"\n").expect("manifest");
        let store = DiskStore::new(root.clone()).with_cap(0);
        let k = key(3, 1);
        store.put_loose(EntryKind::Codepack, &k, &[1; 64]);
        let shard = root.join("codepack/03");
        // A temporary file, a foreign name, and a key of another shard stay.
        for name in [
            "00000000000000000000000000000001.tmp42",
            "notes.txt",
            "04000000000000000000000000000001",
        ] {
            std::fs::write(shard.join(name), b"keep").expect("file");
        }
        std::fs::write(root.join("README"), b"keep").expect("file");
        #[cfg(unix)]
        {
            // A symbolic link named like a key is no entry, nor is its target.
            let target = base.join("target-file");
            std::fs::write(&target, b"keep").expect("target");
            let link = shard.join(format!("{:032x}", key(3, 2).0));
            std::os::unix::fs::symlink(&target, link).expect("link");
            std::os::unix::fs::symlink(&pkg, root.join("check")).expect("dir link");
        }
        let got = store.collect()?;
        assert_eq!((got.before, got.after), (64, 0));
        assert!(!store.path(EntryKind::Codepack, &k).exists());
        assert_eq!(
            std::fs::read_dir(&shard).expect("shard").count(),
            if cfg!(unix) { 4 } else { 3 }
        );
        assert!(pkg.join("hd.toml").exists());
        assert!(root.join("README").exists());
        assert!(base.join("target-file").exists() || cfg!(not(unix)));
        drop(store);
        let _ = std::fs::remove_dir_all(base);
        Ok(())
    }

    #[test]
    fn a_dropped_store_records_its_bytes_and_evicts_over_the_cap() {
        let root = scratch("auto");
        let read = |r: &PathBuf| Ledger::decode(&std::fs::read(r.join("stats")).expect("ledger"));
        // Marks a store used with no write, as a run of only hits does.
        let run = |cap: u64| {
            DiskStore::new(root.clone())
                .with_cap(cap)
                .touch_loose(EntryKind::Check, &key(0, 1));
        };
        // A cap of 256 bytes is a budget of one byte per shard.
        let store = DiskStore::new(root.clone()).with_cap(256);
        for n in 0..8 {
            store.put_loose(EntryKind::Check, &key(n * 30, 1), &[0; 100]);
        }
        assert_eq!(total(&store), 800);
        drop(store);
        assert!(!read(&root).migrated);
        // A new ledger knows no shard's size, so each run scans unknown
        // shards within its time bound until every size is known.
        for _ in 0..SHARDS {
            if read(&root).sizes.iter().all(|s| *s != UNKNOWN) {
                break;
            }
            run(256);
        }
        assert_eq!(total(&DiskStore::new(root.clone())), 0);
        assert!(read(&root).sizes.iter().all(|s| *s == 0));
        // With known sizes, a run that pushes a shard over its budget of
        // 1000 bytes cleans it to 90%, oldest first; ties go by key.
        let cap = 256 * 1000;
        for n in 1..=2 {
            let store = DiskStore::new(root.clone()).with_cap(cap);
            store.put_loose(EntryKind::Check, &key(9, n), &[0; 600]);
        }
        for _ in 0..SHARDS {
            if read(&root).sizes[9] <= 1000 {
                break;
            }
            run(cap);
        }
        assert_eq!(read(&root).sizes[9], 600);
        let store = DiskStore::new(root.clone());
        assert!(!store.path(EntryKind::Check, &key(9, 1)).exists());
        assert!(store.path(EntryKind::Check, &key(9, 2)).exists());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_ledger_keeps_bytes_another_process_added() -> std::io::Result<()> {
        let root = scratch("ledger");
        let store = DiskStore::new(root.clone()).with_cap(256 * 1000);
        let k = key(5, 1);
        // Start from exact sizes, so the bounded drops below need no
        // rescan: the test is about byte merging, not eviction timing.
        assert_eq!(store.collect()?.after, 0);
        store.put_loose(EntryKind::Iface, &k, &[0; 300]);
        drop(store);
        let read = |r: &PathBuf| Ledger::decode(&std::fs::read(r.join("stats")).expect("ledger"));
        assert_eq!(read(&root).sizes[5], 300);
        assert_eq!(read(&root).sizes[6], 0);
        // A later store over the same tree adds its own bytes.
        let other = DiskStore::new(root.clone()).with_cap(256 * 1000);
        other.put_loose(EntryKind::Iface, &key(5, 2), &[0; 200]);
        drop(other);
        assert_eq!(read(&root).sizes[5], 500);
        // A foreign ledger reads as unknown sizes.
        std::fs::write(root.join("stats"), b"junk").expect("junk");
        assert!(read(&root).sizes.iter().all(|s| *s == UNKNOWN));
        assert_eq!(SHARDS, 256);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn readers_racing_eviction_see_whole_entries_or_misses() {
        let root = scratch("race");
        let store = DiskStore::new(root.clone()).with_cap(0);
        let keys: Vec<Hash128> = (0..64).map(|n| key(n, u128::from(n) + 1)).collect();
        let body: Vec<u8> = (0..=255).cycle().take(16 * 1024).collect();
        std::thread::scope(|s| {
            s.spawn(|| {
                for _ in 0..10 {
                    for k in &keys {
                        store.put_loose(EntryKind::Clpack, k, &body);
                    }
                    assert!(store.collect().is_ok());
                }
            });
            s.spawn(|| {
                for _ in 0..200 {
                    for k in &keys {
                        if let Some(got) = store.get_loose(EntryKind::Clpack, k) {
                            assert_eq!(&got[..], &body[..]);
                        }
                    }
                }
            });
        });
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_thirty_two_lowercase_hex_digits_name_a_key() {
        assert_eq!(parse_key(&format!("{:032x}", 255)), Some(Hash128(255)));
        assert!(parse_key("0000000000000000000000000000000G").is_none());
        assert!(parse_key("000000000000000000000000000000AB").is_none());
        assert!(parse_key("00ab").is_none());
        assert!(parse_key("00000000000000000000000000000001.tmp7").is_none());
    }

    #[test]
    fn threads_writing_one_key_publish_whole_entries() -> std::io::Result<()> {
        let root = scratch("same-key");
        let store = DiskStore::new(root.clone());
        assert!(store.collect().is_ok());
        let k = key(4, 1);
        let body: Vec<u8> = (0..64 * 1024_u32)
            .map(|i| u8::try_from(i % 251).expect("byte"))
            .collect();
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..50 {
                        store.put_loose(EntryKind::Check, &k, &body);
                    }
                });
            }
        });
        assert_eq!(
            store.get_loose(EntryKind::Check, &k).as_deref(),
            Some(&body[..])
        );
        assert_eq!(ledger(&root)?.sizes[4], body.len() as u64);
        let dir = store.path(EntryKind::Check, &k);
        let left = std::fs::read_dir(dir.parent().expect("shard"))
            .expect("shard")
            .count();
        assert_eq!(left, 1, "no temporary file is left behind");
        drop(store);
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn a_private_entry_is_read_only_when_its_owner_alone_can_use_it() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = scratch("private");
        let store = DiskStore::new(root.clone());
        let k = key(9, 1);
        store.put_private(EntryKind::Clpack, &k, b"code");
        let path = store.path(EntryKind::Clpack, &k);
        let meta = std::fs::metadata(&path).expect("entry");
        assert_eq!(meta.mode() & 0o777, 0o600);
        let me = meta.uid();
        assert_eq!(
            store.get_private(EntryKind::Clpack, &k, me).as_deref(),
            Some(&b"code"[..])
        );
        // Another owner, or a looser mode, is a miss.
        assert!(
            store
                .get_private(EntryKind::Clpack, &k, me.wrapping_add(1))
                .is_none()
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod");
        assert!(store.get_private(EntryKind::Clpack, &k, me).is_none());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        // A second link to the file is a miss.
        let other = root.join("other");
        std::fs::hard_link(&path, &other).expect("link");
        assert!(store.get_private(EntryKind::Clpack, &k, me).is_none());
        std::fs::remove_file(&other).expect("unlink");
        assert!(store.get_private(EntryKind::Clpack, &k, me).is_some());
        // A symbolic link at the entry's path is a miss, even to an entry
        // the reader owns.
        let k2 = key(9, 2);
        store.put_private(EntryKind::Clpack, &k2, b"other code");
        std::fs::remove_file(&path).expect("remove");
        std::os::unix::fs::symlink(store.path(EntryKind::Clpack, &k2), &path).expect("symlink");
        assert!(store.get_private(EntryKind::Clpack, &k, me).is_none());
        // An ordinary entry, made with the default mode, is a miss too.
        let k3 = key(9, 3);
        store.put_loose(EntryKind::Clpack, &k3, b"code");
        let loose = std::fs::metadata(store.path(EntryKind::Clpack, &k3)).expect("entry");
        if loose.mode() & 0o777 != 0o600 {
            assert!(store.get_private(EntryKind::Clpack, &k3, me).is_none());
        }
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }
}
