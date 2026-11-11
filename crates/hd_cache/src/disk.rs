//! The disk store (feature `disk`): ordinary data in immutable per-run
//! packs and a shared paged index under `<root>/pack-v1/`; private native
//! code remains in `<root>/<kind>/<shard>/<key>`. See DESIGN.md §5.4 and
//! §5.7.
//! Only this module touches the file system (design-overview §2.2 rule 1).
//!
//! Pack readers take no publication lock. A pending pack's file lock
//! leases it while its writer runs; index publication and GC share one
//! writer lock. Every addressed range remains an unchanged HDCE frame.
//! Last-use marks are per entry, at most once an hour, and compaction
//! preserves them. The index and physical pack bytes count toward the cap.
//! Automatic cleanup walks the pack directory only when the running total
//! in the lock file is over the cap or unknown. It has a soft time budget,
//! completing its first pack operation even if scanning used that budget;
//! later runs finish the rest.
//! GC follows no links and deletes only recognized managed file names.
//! The legacy shard ledger and use marks still serve private Clpack data;
//! obsolete ordinary loose files are retired by that same bounded scanner.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

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

/// How long a command's automatic eviction may run before it leaves the
/// rest to later commands. One shard is always cleaned whole.
pub const EVICT_BOUND: Duration = Duration::from_millis(20);

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
    /// Automatic eviction's time budget; tests may force it to zero.
    evict_bound: Duration,
    used: AtomicBool,
    /// Set by the first failed publish; the store writes nothing after it.
    write_failed: AtomicBool,
    /// Told of the first failed publish, with the store's root (§5.4).
    on_write_failure: Option<fn(&Path, &std::io::Error)>,
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
            evict_bound: EVICT_BOUND,
            used: AtomicBool::new(false),
            write_failed: AtomicBool::new(false),
            on_write_failure: None,
        }
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

    /// The same store with a different automatic-eviction time budget.
    #[cfg(test)]
    #[must_use]
    fn with_evict_bound(mut self, bound: Duration) -> Self {
        self.evict_bound = bound;
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

    /// `hd cache gc` (`cli.cache.gc`): cleans every shard now, with no time
    /// bound, and writes the exact shard sizes to the ledger.
    pub fn collect(&self) -> std::io::Result<Collected> {
        let loose = self.clean(&(0..=u8::MAX).collect::<Vec<_>>(), None)?;
        let packed_before = self.packed.bytes();
        Ok(
            match self
                .packed
                .maintain(self.cap.saturating_sub(loose.after), None)
            {
                Ok(packed) => Collected {
                    before: loose.before.saturating_add(packed.before),
                    after: loose.after.saturating_add(packed.after),
                },
                Err(error) => {
                    self.warn(&error);
                    Collected {
                        before: loose.before.saturating_add(packed_before),
                        after: loose.after.saturating_add(self.packed.bytes()),
                    }
                }
            },
        )
    }

    /// Runs bounded automatic eviction only for unknown or over-budget
    /// shards. Called on drop; a store never used does nothing.
    fn settle(&self) {
        if !self.used.load(Ordering::Relaxed) {
            return;
        }
        let deadline = Instant::now() + self.evict_bound;
        let budget = self.budget();
        let mut work = Vec::new();
        let snapshot = Ledger::update(&self.root, |l| {
            // The first packed publication retires ordinary loose files,
            // even when their previously recorded sizes fit the budget.
            if !l.migrated && is_real_dir(&self.root.join("pack-v1")) {
                l.migrated = true;
                l.sizes.fill(UNKNOWN);
            }
            let shards = || (0..=u8::MAX).zip(l.sizes);
            work.extend(
                shards()
                    .filter(|(_, size)| *size != UNKNOWN && *size > budget)
                    .map(|(s, _)| s),
            );
            work.extend(
                shards()
                    .filter(|(_, size)| *size == UNKNOWN)
                    .map(|(s, _)| s),
            );
        });
        if snapshot.is_some() {
            let _ = self.clean(&work, Some(deadline));
        }
    }

    /// Cleans `shards` in order, each down to 90% of its budget when it is
    /// over it, recording each one's exact size under the publication lock.
    /// With a deadline, the shards after the first are left to
    /// a later command once it has passed.
    fn clean(&self, shards: &[u8], deadline: Option<Instant>) -> std::io::Result<Collected> {
        let budget = self.budget();
        let kinds = EntryKind::ALL;
        let mut total = Collected {
            before: 0,
            after: 0,
        };
        if !self.root.try_exists()? {
            return Ok(total);
        }
        for (i, &shard) in shards.iter().enumerate() {
            if i > 0 && deadline.is_some_and(|d| Instant::now() >= d) {
                break;
            }
            let result = Ledger::transaction(&self.root, |l| {
                if !l.migrated && is_real_dir(&self.root.join("pack-v1")) {
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
        let migrated = is_real_dir(&self.root.join("pack-v1"));
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
    fn drop(&mut self) {
        let started = Instant::now();
        if self.packed.was_used() && (!self.kinds().is_empty() || self.root.join(STATS).is_file()) {
            self.used.store(true, Ordering::Relaxed);
        }
        self.settle();
        if self.packed.was_used() {
            // The legacy ledger is approximate during automatic eviction;
            // explicit collect scans both namespaces and uses exact bytes.
            let loose = File::open(self.root.join(STATS))
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
                });
            if let Err(error) = self.packed.maintain(
                self.cap.saturating_sub(loose),
                Some(self.evict_bound.saturating_sub(started.elapsed())),
            ) {
                self.warn(&error);
            }
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
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > USE_MARK_AGE)
}

/// Sets an existing entry's mtime to now; never creates one.
fn mark_used(path: &Path) {
    if let Ok(f) = OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
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
/// linear hash table in `pack-v1/index/`: a fixed head holds the level and
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant, SystemTime};

    use hd_base::{Hash128, StableHasher};

    #[cfg(test)]
    use super::cost;
    use super::{Collected, EntryKind, IndexedEntry, USE_MARK_AGE, is_real_dir, parse_key};

    /// The single-file index of older hd, read until the first writer
    /// migrates it.
    const LEGACY_MAGIC: &[u8; 8] = b"HDPACK01";
    const PAGE_MAGIC: &[u8; 8] = b"HDPAGE01";
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
    /// The running total in the lock file: magic, managed bytes (final
    /// packs and the index files), and the generation of the index head it
    /// describes.
    const TOTAL_MAGIC: &[u8; 8] = b"HDBYTES1";
    const RECORD: usize = 32;
    type Key = (u16, u128);
    type Entries = BTreeMap<Key, Location>;
    type Batch = [(EntryKind, Hash128, Arc<[u8]>)];
    /// Footer rows: key, offset and length of each frame in a pack.
    type Frames = Vec<(Key, u64, u64)>;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// What a reader sees: no index, an older hd's single file, or the
    /// paged index under one head.
    #[derive(Default)]
    enum View {
        #[default]
        Empty,
        Legacy(Entries),
        Paged(Head),
    }

    #[derive(Default)]
    struct Snapshot {
        view: View,
        /// Pages read under the view's head, by bucket.
        pages: BTreeMap<u64, Entries>,
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
        /// This process published a pack, so its settlement may merge.
        published: AtomicBool,
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
    fn frame(magic: [u8; 8], entries: &Entries, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&magic);
        out.extend_from_slice(
            &u32::try_from(entries.len())
                .expect("bounded rows")
                .to_le_bytes(),
        );
        out.extend_from_slice(&[0; 20]);
        for (key, loc) in entries {
            put_row(&mut out, key, loc);
        }
        let sum = checksum(&out[HEADER..]);
        out[16..HEADER].copy_from_slice(&sum.to_le_bytes());
        out.resize(len, 0);
        out
    }

    /// The header's row count and the rows' checksum when `bytes` is a
    /// whole frame of `magic` whose length `len_of(count)` gives.
    fn unframe(
        magic: [u8; 8],
        bytes: &[u8],
        len_of: impl Fn(usize) -> Option<usize>,
    ) -> io::Result<Entries> {
        if bytes.len() < HEADER || bytes[..8] != magic || bytes[12..16] != [0; 4] {
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
        rows(&bytes[HEADER..end])
    }

    fn legacy_len(count: usize) -> Option<usize> {
        count
            .checked_mul(ROW)
            .and_then(|n| n.checked_add(HEADER))
            .filter(|&n| n <= MAX_INDEX)
    }

    #[cfg(test)]
    fn legacy_encode(entries: &Entries) -> Vec<u8> {
        frame(
            *LEGACY_MAGIC,
            entries,
            legacy_len(entries.len()).expect("bounded"),
        )
    }

    fn legacy_decode(bytes: &[u8]) -> io::Result<Entries> {
        unframe(*LEGACY_MAGIC, bytes, legacy_len)
    }

    /// A page file's length: whole pages of [`SLOTS`] rows, at least one.
    fn page_len(count: usize) -> usize {
        HEADER + count.div_ceil(SLOTS).max(1) * SLOTS * ROW
    }

    fn page_encode(entries: &Entries) -> Vec<u8> {
        frame(*PAGE_MAGIC, entries, page_len(entries.len()))
    }

    fn page_decode(bytes: &[u8]) -> io::Result<Entries> {
        unframe(*PAGE_MAGIC, bytes, |n| {
            Some(page_len(n)).filter(|&l| l <= MAX_INDEX)
        })
    }

    fn page_name(bucket: u64) -> String {
        format!("{bucket:016x}")
    }

    /// The index head: level `i` and split pointer `s` of the linear hash
    /// table (buckets `2^i + s`), the row count, and a generation that
    /// changes with every head write, which the running total names.
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

    /// A page's rows and its file length; `None` when it has no file.
    fn read_page(dir: &Path, bucket: u64) -> io::Result<Option<(Entries, u64)>> {
        let Some(bytes) = read_small(&dir.join(page_name(bucket)), MAX_INDEX)? else {
            return Ok(None);
        };
        let len = bytes.len() as u64;
        Ok(Some((page_decode(&bytes)?, len)))
    }

    fn read_legacy(path: &Path) -> io::Result<Option<Entries>> {
        read_small(path, MAX_INDEX)?
            .map(|b| legacy_decode(&b))
            .transpose()
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

        /// The generation the running total must name; 0 for no head.
        fn generation(&self) -> u128 {
            self.stored.map_or(0, |h| h.generation)
        }

        fn page(&mut self, bucket: u64) -> io::Result<&mut Page> {
            match self.pages.entry(bucket) {
                Slot::Occupied(page) => Ok(page.into_mut()),
                Slot::Vacant(slot) => {
                    let (mut rows, len, dirty) = match read_page(&self.dir, bucket) {
                        Ok(Some((rows, len))) => (rows, len, false),
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

        /// Every row, reading every page: the slow path's whole view.
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

        /// The bytes of the pages this writer holds, as it would leave them.
        fn page_bytes(&self) -> u64 {
            self.pages
                .values()
                .map(|p| {
                    if p.dirty {
                        page_len(p.rows.len()) as u64
                    } else {
                        p.len
                    }
                })
                .sum()
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

        fn dirty(&self) -> bool {
            self.table_dirty
                || !self.doomed.is_empty()
                || self.pages.values().any(|p| p.dirty)
                || self.head_dirty()
        }

        /// Writes the changes: the running total first (adding `added`
        /// bytes that `finish` makes final), then each changed page whole,
        /// a bucket that split still holding the rows it gave away, the
        /// pack table, the head, and last the split buckets without them
        /// and the removal of merged pages. A crash at any point leaves
        /// every key in the bucket the stored head names, or lost from
        /// this commit only.
        fn commit(
            &mut self,
            lock: Option<&File>,
            added: u64,
            finish: impl FnOnce() -> io::Result<()>,
        ) -> io::Result<()> {
            let recorded = lock.and_then(|l| Store::recorded(l, self.generation()));
            let head = self.head;
            let mut first = Vec::new();
            let mut last = Vec::new();
            // `delta` is the exact change, `growth` its increases alone.
            let mut delta = i128::from(added);
            let mut growth = delta;
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
                    page_encode(&page.rows)
                } else {
                    let mut superset = page.rows.clone();
                    superset.extend(moved);
                    last.push(bucket);
                    page_encode(&superset)
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
                self.head.generation = unique();
                change(i128::from(self.stored.map_or(HEAD_LEN as u64, |_| 0)));
                Some(self.head.encode())
            } else {
                None
            };
            let total = |delta: i128| {
                recorded.map(|t| u64::try_from((i128::from(t) + delta).max(0)).unwrap_or(u64::MAX))
            };
            // Count the bytes before they are final, and no decrease before
            // it happens: dying before the head commit leaves a record of a
            // head that never existed, or one too large.
            if let (Some(lock), Some(t)) = (lock, total(growth)) {
                Store::record(lock, t, self.head.generation);
            }
            finish()?;
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
            for (bucket, bytes) in &mut first {
                let page = self.pages.get_mut(bucket).expect("written");
                if last.contains(bucket) {
                    let exact = page_encode(&page.rows);
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
            if let (Some(lock), Some(t)) = (lock, total(delta)) {
                Store::record(lock, t, self.generation());
            }
            Ok(())
        }
    }

    impl Store {
        pub(super) fn new(root: &Path) -> Self {
            Self {
                root: root.join("pack-v1"),
                snapshot: Mutex::new(None),
                used: AtomicU64::new(0),
                published: AtomicBool::new(false),
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

        fn index_dir(&self) -> PathBuf {
            self.root.join("index")
        }

        fn ensure_root(&self) -> io::Result<()> {
            std::fs::create_dir_all(&self.root)?;
            if !is_real_dir(&self.root) {
                return Err(invalid("cache pack namespace is not a directory"));
            }
            Ok(())
        }

        fn lock(&self) -> io::Result<File> {
            self.ensure_root()?;
            let path = self.root.join("lock");
            if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
                return Err(invalid("cache publication lock is not a regular file"));
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            file.lock()?;
            Ok(file)
        }

        /// What a reader sees, taking no lock: the paged index's head, or
        /// an older hd's single file (also mid-migration, as `index.old`).
        fn view(&self) -> View {
            let index = self.index_dir();
            let legacy = |path: &Path| {
                read_legacy(path)
                    .ok()
                    .flatten()
                    .map_or(View::Empty, View::Legacy)
            };
            match std::fs::symlink_metadata(&index) {
                Ok(meta) if meta.file_type().is_dir() => read_head(&index)
                    .ok()
                    .flatten()
                    .map_or(View::Empty, View::Paged),
                Ok(meta) if meta.file_type().is_file() => legacy(&index),
                Ok(_) => View::Empty,
                Err(_) => legacy(&self.root.join("index.old")),
            }
        }

        /// The publication lock and the index, migrated from an older hd's
        /// single file first.
        fn begin(&self) -> io::Result<(File, Txn)> {
            let lock = self.lock()?;
            self.migrate()?;
            Ok((lock, Txn::open(self.index_dir())?))
        }

        /// Caller holds the publication lock. Rewrites an older hd's single
        /// index file as pages in `index.new/`, then swaps it in: `index`
        /// becomes `index.old`, `index.new` becomes `index`, and `index.old`
        /// goes. Readers see the old file under either name meanwhile; a
        /// crash between the renames is finished by the next writer. An
        /// older hd finds a directory where its index was and reads misses.
        fn migrate(&self) -> io::Result<()> {
            let index = self.index_dir();
            let old = self.root.join("index.old");
            let source = match std::fs::symlink_metadata(&index) {
                Ok(meta) if meta.file_type().is_dir() => {
                    if std::fs::symlink_metadata(&old).is_ok_and(|m| m.file_type().is_file()) {
                        remove_missing_ok(&old)?;
                    }
                    return Ok(());
                }
                Ok(meta) if meta.file_type().is_file() => index.clone(),
                Ok(_) => return Err(invalid("cache index is not a directory")),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if std::fs::symlink_metadata(&old).is_ok_and(|m| m.file_type().is_file()) {
                        old.clone()
                    } else {
                        std::fs::create_dir(&index)?;
                        return Ok(());
                    }
                }
                Err(e) => return Err(e),
            };
            let entries = match read_legacy(&source) {
                Ok(entries) => entries.unwrap_or_default(),
                Err(e) if e.kind() == io::ErrorKind::InvalidData => Entries::new(),
                Err(e) => return Err(e),
            };
            let new = self.root.join("index.new");
            if is_real_dir(&new) {
                Self::clear(&new)?;
            } else {
                remove_missing_ok(&new)?;
            }
            std::fs::create_dir(&new)?;
            let mut txn = Txn::open(new.clone())?;
            for (key, loc) in entries {
                txn.set(key, loc)?;
            }
            // The running total names the legacy file: unknown from here.
            txn.commit(None, 0, || Ok(()))?;
            if source == index {
                std::fs::rename(&index, &old)?;
            }
            std::fs::rename(&new, &index)?;
            remove_missing_ok(&old)
        }

        /// Removes the managed files of an abandoned migration directory,
        /// then the directory; a foreign file keeps it.
        fn clear(dir: &Path) -> io::Result<()> {
            // cost: only after a migration that crashed, once.
            for e in std::fs::read_dir(dir)? {
                let e = e?;
                let name = e.file_name();
                if name.to_str().is_some_and(Self::index_file) && e.file_type()?.is_file() {
                    remove_missing_ok(&e.path())?;
                }
            }
            std::fs::remove_dir(dir)
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

        /// Caller holds the publication lock across creation and locking.
        fn lease(&self) -> io::Result<Lease> {
            loop {
                let id = unique();
                if self.path(id).try_exists()? {
                    continue;
                }
                let path = self.root.join(format!("{id:032x}.pending"));
                match OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(file) => {
                        let lease = Lease {
                            id,
                            path,
                            file,
                            committed: false,
                        };
                        lease.file.lock()?;
                        return Ok(lease);
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
        }

        /// Caller holds the publication lock. Pending bytes are not yet
        /// compiled entries; after unlocking and renaming, GC still cannot
        /// race the index commit because it needs that same writer lock.
        fn finish(&self, lease: &mut Lease) -> io::Result<()> {
            let path = self.path(lease.id);
            if path.try_exists()? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "cache pack ID already exists",
                ));
            }
            lease.file.unlock()?;
            std::fs::rename(&lease.path, &path)?;
            lease.path = path;
            // Pages that name it may be on disk before the commit fails:
            // from here only GC, under the lock and by the index, removes it.
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

        /// `key`'s location in the snapshot, reading its page once.
        fn locate(&self, state: &mut Snapshot, key: Key) -> Option<Location> {
            match &state.view {
                View::Empty => None,
                View::Legacy(entries) => entries.get(&key).copied(),
                View::Paged(head) => {
                    let bucket = head.bucket(key.1);
                    let dir = self.index_dir();
                    state
                        .pages
                        .entry(bucket)
                        .or_insert_with(|| {
                            read_page(&dir, bucket)
                                .ok()
                                .flatten()
                                .map(|(rows, _)| rows)
                                .unwrap_or_default()
                        })
                        .get(&key)
                        .copied()
                }
            }
        }

        fn snapshot<'a>(
            &self,
            guard: &'a mut MutexGuard<'_, Option<Snapshot>>,
        ) -> &'a mut Snapshot {
            guard.get_or_insert_with(|| Snapshot {
                view: self.view(),
                ..Snapshot::default()
            })
        }

        pub(super) fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = self.snapshot(&mut guard);
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
        /// commit. The snapshot then reads the head and pages again, once
        /// per vanished pack, as git rescans its packs when one disappears
        /// (`reprepare_packed_git`). Cost: nothing on a hit or an ordinary
        /// miss; per vanished pack, one head and the pages read after it.
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
                    // missing for good: a miss, left to GC's slow path.
                    Err(e)
                        if e.kind() == io::ErrorKind::NotFound && state.gone.insert(loc.pack) =>
                    {
                        state.view = self.view();
                        state.pages.clear();
                    }
                    Err(_) => return None,
                }
            }
        }

        pub(super) fn touch(&self, kind: EntryKind, key: &Hash128) {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = self.snapshot(&mut guard);
            let k = (kind as u16, key.0);
            let at = now();
            if self
                .locate(state, k)
                .is_some_and(|l| at.saturating_sub(l.used) > USE_MARK_AGE.as_secs())
            {
                state.touched.insert(k, at);
            }
        }

        /// Applies the pending recency marks to the rows they name.
        fn merge_touches(&self, txn: &mut Txn) -> io::Result<()> {
            let touched = self
                .state()
                .as_mut()
                .map(|s| std::mem::take(&mut s.touched))
                .unwrap_or_default();
            for (key, used) in touched {
                if let Some(loc) = txn.get(key)?
                    && used > loc.used
                {
                    txn.set(key, Location { used, ..loc })?;
                }
            }
            Ok(())
        }

        /// The snapshot after this process's commit: its head and the pages
        /// it wrote; other pages are read again when needed.
        fn adopt(&self, txn: &Txn) {
            let mut guard = self.state();
            let state = guard.get_or_insert_with(Snapshot::default);
            state.view = txn.stored.map_or(View::Empty, View::Paged);
            if txn.stored.is_none() {
                state.pages.clear();
            }
            for (&bucket, page) in &txn.pages {
                state.pages.insert(bucket, page.rows.clone());
            }
        }

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
            let mut lease = {
                let _lock = self.lock()?;
                self.lease()?
            };
            let mut offset = 0_u64;
            let mut pending = Entries::new();
            let mut frames = Frames::with_capacity(batch.len());
            let at = now();
            for (kind, key, bytes) in batch {
                lease.file.write_all(bytes)?;
                let len = u64::try_from(bytes.len()).map_err(io::Error::other)?;
                let k = (*kind as u16, key.0);
                pending.insert(
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
            let (lock, mut txn) = self.begin()?;
            self.merge_touches(&mut txn)?;
            for (key, loc) in pending {
                txn.set(key, loc)?;
            }
            txn.track(lease.id, pack_len)?;
            // The writer lock still excludes GC; unlock before readers
            // can see the new rows (Windows byte-range locks block reads).
            txn.commit(Some(&lock), pack_len, || self.finish(&mut lease))?;
            self.published.store(true, Ordering::Relaxed);
            self.adopt(&txn);
            Ok(())
        }

        fn pack_id(name: &str) -> Option<u128> {
            name.strip_suffix(".pack").and_then(parse_key).map(|k| k.0)
        }

        /// Scans managed files with bounded memory, including orphan bytes:
        /// the total and the sizes of the `wanted` packs. It never follows
        /// a link or counts a foreign filename.
        fn scan(&self, wanted: &BTreeSet<u128>) -> io::Result<(u64, BTreeMap<u128, u64>)> {
            let mut total = 0_u64;
            let mut sizes = BTreeMap::new();
            // cost: walks the pack directory only for `hd cache gc`, or when the
            // recorded total is over the cap or unknown (#1501).
            count!(dirs_listed, 1);
            for e in std::fs::read_dir(&self.root)? {
                count!(entries_listed, 1);
                let e = e?;
                if !e.file_type()?.is_file() {
                    continue;
                }
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(id) = Self::pack_id(name) {
                    let len = e.metadata()?.len();
                    total = total.saturating_add(len);
                    if wanted.contains(&id) {
                        sizes.insert(id, len);
                    }
                } else if name == "index" || name == "index.old" {
                    total = total.saturating_add(e.metadata()?.len());
                }
            }
            let index = self.index_dir();
            if is_real_dir(&index) {
                // cost: the same slow path; the index holds one page per
                // forty entries.
                count!(dirs_listed, 1);
                for e in std::fs::read_dir(index)? {
                    count!(entries_listed, 1);
                    let e = e?;
                    if e.file_type()?.is_file()
                        && e.file_name().to_str().is_some_and(Self::index_file)
                    {
                        total = total.saturating_add(e.metadata()?.len());
                    }
                }
            }
            Ok((total, sizes))
        }

        fn prune_orphans(
            &self,
            live: &BTreeSet<u128>,
            buckets: u64,
            deadline: Option<Instant>,
        ) -> io::Result<()> {
            let mut attempted = false;
            count!(dirs_listed, 1);
            // cost: the slow path of `scan` only: `hd cache gc`, or a recorded
            // total over the cap or unknown.
            for e in std::fs::read_dir(&self.root)? {
                if attempted && deadline.is_some_and(|d| Instant::now() >= d) {
                    break;
                }
                count!(entries_listed, 1);
                let e = e?;
                if !e.file_type()?.is_file() {
                    continue;
                }
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                let pending = name
                    .strip_suffix(".pending")
                    .and_then(parse_key)
                    .map(|key| key.0);
                if let Some(id) = Self::pack_id(name).or(pending) {
                    if pending.is_none() && live.contains(&id) {
                        continue;
                    }
                    let file = OpenOptions::new().read(true).write(true).open(e.path())?;
                    match file.try_lock() {
                        Ok(()) => {
                            let _ = std::fs::remove_file(e.path());
                            attempted = true;
                        }
                        Err(TryLockError::WouldBlock) => {}
                        Err(TryLockError::Error(error)) => return Err(error),
                    }
                } else if name
                    .strip_prefix("index.")
                    .and_then(|n| n.strip_suffix(".tmp"))
                    .and_then(parse_key)
                    .is_some()
                {
                    let _ = std::fs::remove_file(e.path());
                    attempted = true;
                }
            }
            let index = self.index_dir();
            if !is_real_dir(&index) {
                return Ok(());
            }
            // cost: the same slow path. Temporary files and pages past the
            // last bucket are crashed writers'.
            count!(dirs_listed, 1);
            for e in std::fs::read_dir(index)? {
                count!(entries_listed, 1);
                let e = e?;
                let name = e.file_name();
                let Some(name) = name.to_str() else { continue };
                let stale = name.rsplit_once('.').is_some_and(|(_, ext)| ext == "tmp")
                    || u64::from_str_radix(name, 16)
                        .is_ok_and(|b| name.len() == 16 && b >= buckets);
                if stale && Self::index_file(name) && e.file_type()?.is_file() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            Ok(())
        }

        /// Copies the live frames of `ids` into one new pack, by their
        /// rows `frames`, and commits their new locations. Returns the
        /// bytes of the removed old packs.
        fn rewrite(&self, lock: &File, txn: &mut Txn, ids: &[(u128, Frames)]) -> io::Result<u64> {
            let mut lease = self.lease()?;
            let mut offset = 0_u64;
            let mut frames = Frames::new();
            for (id, rows) in ids {
                let mut source = self.open(*id)?.file;
                for &(key, from, len) in rows {
                    let Some(loc) = txn.get(key)? else { continue };
                    if (loc.pack, loc.offset, loc.len) != (*id, from, len) {
                        continue;
                    }
                    source.seek(SeekFrom::Start(from))?;
                    let got = io::copy(&mut (&mut source).take(len), &mut lease.file)?;
                    if got != len {
                        return Err(invalid("truncated cache pack during compaction"));
                    }
                    txn.set(
                        key,
                        Location {
                            pack: lease.id,
                            offset,
                            ..loc
                        },
                    )?;
                    frames.push((key, offset, len));
                    offset += len;
                }
            }
            let foot = footer(&frames);
            lease.file.write_all(&foot)?;
            let len = offset + foot.len() as u64;
            txn.track(lease.id, len)?;
            for (id, _) in ids {
                txn.untrack(*id)?;
            }
            // Readers holding pages from before this commit may still name
            // the old packs; they read the index again when one is gone.
            txn.commit(Some(lock), len, || self.finish(&mut lease))?;
            let mut removed = 0;
            for (id, _) in ids {
                let path = self.path(*id);
                let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
                // An open reader on Windows may postpone this; GC retries.
                if std::fs::remove_file(path).is_ok() {
                    removed += size;
                }
            }
            if let Some(total) = Self::recorded(lock, txn.generation()) {
                Self::record(lock, total.saturating_sub(removed), txn.generation());
            }
            Ok(removed)
        }

        /// Compacts pack `id` by the rows of `entries`, which follow.
        fn compact(
            &self,
            lock: &File,
            txn: &mut Txn,
            entries: &mut Entries,
            id: u128,
        ) -> io::Result<()> {
            let frames: Frames = entries
                .iter()
                .filter(|(_, loc)| loc.pack == id)
                .map(|(&key, loc)| (key, loc.offset, loc.len))
                .collect();
            self.rewrite(lock, txn, &[(id, frames.clone())])?;
            for (key, _, _) in frames {
                if let Some(loc) = txn.get(key)? {
                    entries.insert(key, loc);
                }
            }
            Ok(())
        }

        /// Merges the smallest size class of tracked packs once it holds
        /// [`FANOUT`] packs, so their count stays bounded: about
        /// `FANOUT` per power of two up to [`FULL`]. Each frame is copied
        /// once per class it climbs. Cost: the merged bytes, their footers
        /// and the pages of their rows; no directory walk.
        fn merge_small(&self, lock: &File, txn: &mut Txn) -> io::Result<()> {
            let mut classes = BTreeMap::<u32, Vec<u128>>::new();
            for (&id, &len) in txn.table()?.iter() {
                if len < FULL {
                    classes.entry(len.max(1).ilog2()).or_default().push(id);
                }
            }
            let Some((_, ids)) = classes.into_iter().find(|(_, ids)| ids.len() >= FANOUT) else {
                return Ok(());
            };
            let mut packs = Vec::with_capacity(FANOUT);
            for id in ids.into_iter().take(FANOUT) {
                match self
                    .open(id)
                    .and_then(|mut opened| read_footer(&mut opened.file, opened.len))
                {
                    Ok(frames) => packs.push((id, frames)),
                    // A pack another GC removed, or one without a footer,
                    // is left to the slow path.
                    Err(_) => txn.untrack(id)?,
                }
            }
            if packs.len() >= 2 {
                self.rewrite(lock, txn, &packs)?;
            } else if txn.dirty() {
                txn.commit(Some(lock), 0, || Ok(()))?;
            }
            Ok(())
        }

        pub(super) fn maintain(&self, cap: u64, bound: Option<Duration>) -> io::Result<Collected> {
            if !is_real_dir(&self.root) {
                return Ok(Collected {
                    before: 0,
                    after: 0,
                });
            }
            let (lock, mut txn) = self.begin()?;
            let recorded = Self::recorded(&lock, txn.generation()).filter(|&total| total <= cap);
            if let (Some(_), Some(total)) = (bound, recorded) {
                // The running total fits: no directory walk, only the
                // pending recency marks and, after a publication, merging
                // small packs.
                self.merge_touches(&mut txn)?;
                if txn.dirty() {
                    txn.commit(Some(&lock), 0, || Ok(()))?;
                }
                if self.published.swap(false, Ordering::Relaxed) {
                    self.merge_small(&lock, &mut txn)?;
                }
                self.adopt(&txn);
                let after = Self::recorded(&lock, txn.generation()).unwrap_or(total);
                return Ok(Collected {
                    before: total,
                    after,
                });
            }
            let deadline = bound.map(|b| Instant::now() + b);
            self.merge_touches(&mut txn)?;
            let mut entries = txn.all()?;
            let wanted = entries.values().map(|loc| loc.pack).collect();
            let (before, sizes) = self.scan(&wanted)?;
            let invalid: Vec<Key> = entries
                .iter()
                .filter(|(_, loc)| {
                    !sizes.get(&loc.pack).is_some_and(|len| {
                        loc.offset
                            .checked_add(loc.len)
                            .is_some_and(|end| end <= *len)
                    })
                })
                .map(|(&key, _)| key)
                .collect();
            for key in invalid {
                entries.remove(&key);
                txn.remove(key)?;
            }
            // The index's bytes for `n` live rows once merged buckets go.
            // What the live set keeps: each frame with its footer row, each
            // pack with its trailer and table row, and the head and pages.
            // Merging buckets leaves at most one page per half load, never
            // more pages than now.
            let mut rows_per_pack = BTreeMap::<u128, usize>::new();
            for loc in entries.values() {
                *rows_per_pack.entry(loc.pack).or_default() += 1;
            }
            let pages = txn.page_bytes();
            let overhead = |n: usize, packs: usize| -> u64 {
                if n == 0 {
                    0
                } else {
                    let buckets = n.div_ceil(usize::try_from(LOAD / 2).expect("small"));
                    let fixed = HEAD_LEN + HEADER + packs * (TABLE_ROW + HEADER);
                    (fixed as u64).saturating_add(pages.min((buckets * page_len(0)) as u64))
                }
            };
            let mut live_bytes = entries
                .values()
                .map(|loc| loc.len + FOOT_ROW as u64)
                .sum::<u64>();
            let kept =
                |bytes: u64, n: usize, packs: usize| bytes.saturating_add(overhead(n, packs));
            // Dead records and orphans must be reclaimed before they cost
            // live entries: only evict when the live set itself exceeds cap.
            if kept(live_bytes, entries.len(), rows_per_pack.len()) > cap {
                let mut order = entries
                    .iter()
                    .map(|(&key, loc)| (loc.used, key))
                    .collect::<Vec<_>>();
                order.sort_stable();
                for (_, key) in order {
                    if kept(live_bytes, entries.len(), rows_per_pack.len())
                        <= cap.saturating_mul(9) / 10
                    {
                        break;
                    }
                    if let Some(loc) = entries.remove(&key) {
                        live_bytes -= loc.len + FOOT_ROW as u64;
                        if let Some(rows) = rows_per_pack.get_mut(&loc.pack) {
                            *rows -= 1;
                            if *rows == 0 {
                                rows_per_pack.remove(&loc.pack);
                            }
                        }
                        txn.remove(key)?;
                    }
                }
            }
            txn.shrink()?;
            if txn.dirty() {
                txn.commit(Some(&lock), 0, || Ok(()))?;
            }
            // A pack's live bytes include the footer its rows would keep.
            let mut live_per_pack = BTreeMap::<u128, u64>::new();
            for loc in entries.values() {
                *live_per_pack.entry(loc.pack).or_insert(HEADER as u64) +=
                    loc.len + FOOT_ROW as u64;
            }
            // Packs no row names any more leave the table.
            let tracked: Vec<u128> = txn.table()?.keys().copied().collect();
            for id in tracked {
                if !live_per_pack.contains_key(&id) {
                    txn.untrack(id)?;
                }
            }
            let mut compacted = false;
            for (id, live) in live_per_pack {
                let physical = sizes.get(&id).copied().unwrap_or(0);
                if live < physical
                    && (before > cap || live.saturating_mul(4) < physical.saturating_mul(3))
                {
                    if compacted && deadline.is_some_and(|d| Instant::now() >= d) {
                        break;
                    }
                    self.compact(&lock, &mut txn, &mut entries, id)?;
                    compacted = true;
                }
            }
            if !compacted || deadline.is_none_or(|d| Instant::now() < d) {
                self.merge_small(&lock, &mut txn)?;
            }
            if txn.dirty() {
                txn.commit(Some(&lock), 0, || Ok(()))?;
            }
            let live = txn
                .all()?
                .values()
                .map(|loc| loc.pack)
                .chain(txn.table()?.keys().copied())
                .collect();
            self.prune_orphans(&live, txn.head.buckets(), deadline)?;
            self.adopt(&txn);
            let (after, _) = self.scan(&BTreeSet::new())?;
            // Under the lock the walk is exact: record it for later runs.
            Self::record(&lock, after, txn.generation());
            Ok(Collected { before, after })
        }

        /// The managed bytes recorded for the current index, or `None` when
        /// the record describes another head: one written before a
        /// migration, or by a writer that died before its commit. Caller
        /// holds `lock`, the publication lock file, which stores the record.
        fn recorded(mut lock: &File, generation: u128) -> Option<u64> {
            let mut bytes = [0; RECORD];
            lock.seek(SeekFrom::Start(0)).ok()?;
            lock.read_exact(&mut bytes).ok()?;
            (&bytes[..8] == TOTAL_MAGIC && u128_at(&bytes, 16) == generation)
                .then(|| u64_at(&bytes, 8))
        }

        /// Records `total` managed bytes for the head of `generation`. A
        /// failed write leaves a record that matches no head.
        fn record(mut lock: &File, total: u64, generation: u128) {
            let mut bytes = [0; RECORD];
            bytes[..8].copy_from_slice(TOTAL_MAGIC);
            bytes[8..16].copy_from_slice(&total.to_le_bytes());
            bytes[16..].copy_from_slice(&generation.to_le_bytes());
            let _ = lock
                .seek(SeekFrom::Start(0))
                .and_then(|_| lock.write_all(&bytes));
        }

        /// Best-effort physical accounting also works when the writer
        /// lock cannot be opened; a failed GC must not report zero data.
        pub(super) fn bytes(&self) -> u64 {
            if !is_real_dir(&self.root) {
                return 0;
            }
            self.scan(&BTreeSet::new()).map_or(0, |(bytes, _)| bytes)
        }

        pub(super) fn was_used(&self) -> bool {
            self.used.load(Ordering::Relaxed) != 0
        }

        /// Every indexed entry: kind, key, pack file, offset, length, last
        /// use and the pack's length, `None` when it is missing. It holds
        /// the publication lock, so no merge or GC moves a row or removes
        /// its pack between reading the row and the pack's length. When the
        /// lock cannot be taken, as in a read-only cache, it reads as a
        /// command does. It reads the whole index: for tests and tools.
        pub(super) fn entries(&self) -> Vec<IndexedEntry> {
            if !is_real_dir(&self.root) {
                return Vec::new();
            }
            let _lock = self.lock().ok();
            let rows = match self.view() {
                View::Empty => Entries::new(),
                View::Legacy(entries) => entries,
                View::Paged(head) => {
                    let dir = self.index_dir();
                    let mut entries = Entries::new();
                    for bucket in 0..head.buckets() {
                        if let Ok(Some((rows, _))) = read_page(&dir, bucket) {
                            entries.extend(
                                rows.into_iter().filter(|(k, _)| head.bucket(k.1) == bucket),
                            );
                        }
                    }
                    entries
                }
            };
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
            let (lock, mut txn) = self.begin()?;
            let k = (kind as u16, key.0);
            let Some(loc) = txn.get(k)? else {
                return Ok(false);
            };
            txn.set(k, Location { used, ..loc })?;
            txn.commit(Some(&lock), 0, || Ok(()))?;
            self.adopt(&txn);
            Ok(true)
        }
    }
    #[cfg(test)]
    mod tests {
        use super::super::DiskStore;
        use super::*;
        use crate::store::CacheStore;

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
            let (_lock, mut txn) = store.begin().unwrap();
            txn.all().unwrap()
        }
        /// A pack of `frames` entries of `bytes` bytes, footer included.
        fn pack_len(frames: usize, bytes: usize) -> u64 {
            (frames * (bytes + FOOT_ROW) + HEADER) as u64
        }
        /// The index of one page and a table of `packs` packs.
        fn index_len(packs: usize) -> u64 {
            (HEAD_LEN + page_len(1) + HEADER + packs * TABLE_ROW) as u64
        }
        /// Gives the head a generation no running total names, as a
        /// writer that died between its record and its commit leaves it.
        fn unrecorded(store: &Store) {
            let (_lock, txn) = store.begin().unwrap();
            let head = Head {
                generation: unique(),
                ..txn.stored.unwrap()
            };
            replace_file(&txn.dir, "head", &head.encode()).unwrap();
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
            let (lock, txn) = store.begin().unwrap();
            assert_eq!(txn.head.rows, 1000);
            assert_eq!(txn.head.buckets(), 29);
            drop((lock, txn));
            // A crash after the superset page writes, before the head: the
            // old head still finds every row.
            let (lock, mut txn) = store.begin().unwrap();
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
                    replace_file(&txn.dir, &page_name(bucket), &page_encode(&rows)).unwrap();
                }
            }
            drop((lock, txn));
            let reader = Store::new(&root);
            for k in 0..1000_u128 {
                let key = Hash128(k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
                assert!(reader.get(EntryKind::Check, &key).is_some(), "{k}");
            }
            // Eviction merges buckets back and leaves no file at all.
            assert_eq!(store.maintain(0, None).unwrap().after, 0);
            assert!(
                std::fs::read_dir(store.index_dir())
                    .unwrap()
                    .next()
                    .is_none()
            );
        }

        #[test]
        fn gc_respects_a_live_unpublished_lease_and_reclaims_a_crashed_one() {
            let root = scratch("lease");
            let store = Store::new(&root);
            let mut lease = {
                let _lock = store.lock().unwrap();
                store.lease().unwrap()
            };
            lease.file.write_all(b"unpublished").unwrap();
            let collector = Store::new(&root);
            assert_eq!(collector.maintain(0, None).unwrap().after, 0);
            assert!(
                lease.path.exists(),
                "GC must not delete an active writer's pack"
            );
            let path = lease.path.clone();
            // Model OS release at process death, without running Lease's
            // normal error cleanup; the file has never entered the index.
            lease.committed = true;
            drop(lease);
            assert_eq!(collector.maintain(0, None).unwrap().after, 0);
            assert!(!path.exists());
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
                Store::new(&root).maintain(0, None).unwrap();
                std::fs::read_dir(root.join("pack-v1"))
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
            assert_eq!(Store::new(&root).maintain(0, None).unwrap().after, 0);
        }

        #[test]
        #[ignore = "subprocess fixture invoked by crashed_subprocess_lease_is_recoverable"]
        fn crash_writer_fixture() {
            let root = std::env::args()
                .find_map(|a| a.strip_prefix("cache-fixture:").map(PathBuf::from))
                .unwrap();
            let store = Store::new(&root);
            let mut lease = {
                let _lock = store.lock().unwrap();
                store.lease().unwrap()
            };
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
            assert_eq!(reader.maintain(0, None).unwrap().after, 0);
            assert!(!path.exists());
        }

        #[test]
        fn index_validation_bounds_allocations_and_recovers_after_corruption() {
            let root = scratch("corrupt");
            let store = Store::new(&root);
            store.publish(&[item(1, b"value")]).unwrap();
            let dir = store.index_dir();
            let page = dir.join(page_name(0));
            let good = std::fs::read(&page).unwrap();
            for at in [0, 8, 12, 16, HEADER, HEADER + ROW - 1] {
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
            let mut entries = page_decode(&good).unwrap();
            entries.values_mut().next().unwrap().len = MAX_ENTRY + 1;
            assert!(page_decode(&page_encode(&entries)).is_err());
            entries.values_mut().next().unwrap().len = 2;
            entries.values_mut().next().unwrap().offset = u64::MAX;
            assert!(page_decode(&page_encode(&entries)).is_err());
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
            // A corrupt head starts a new index.
            std::fs::write(dir.join("head"), b"HDHEAD01 not a head").unwrap();
            assert!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(2))
                    .is_none()
            );
            store.publish(&[item(3, b"again")]).unwrap();
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(3))
                    .as_deref(),
                Some(b"again".as_slice())
            );
        }

        /// An older hd's single index file is read as it is until the first
        /// writer rewrites it as pages, also after a crash between the
        /// migration's renames.
        #[test]
        fn an_older_single_file_index_migrates_once_and_keeps_its_entries() {
            for crashed in [false, true] {
                let root = scratch("legacy");
                let store = Store::new(&root);
                store.ensure_root().unwrap();
                let id = unique();
                std::fs::write(store.path(id), b"onetwo").unwrap();
                let mut entries = Entries::new();
                for (key, offset) in [(1, 0), (2, 3)] {
                    entries.insert(
                        (EntryKind::Check as u16, key),
                        Location {
                            pack: id,
                            offset,
                            len: 3,
                            used: 7,
                        },
                    );
                }
                let legacy = if crashed {
                    // The pages were half written when the writer died.
                    std::fs::create_dir(root.join("pack-v1/index.new")).unwrap();
                    std::fs::write(root.join("pack-v1/index.new/head"), b"partial").unwrap();
                    root.join("pack-v1/index.old")
                } else {
                    store.index_dir()
                };
                std::fs::write(&legacy, legacy_encode(&entries)).unwrap();
                let reader = Store::new(&root);
                assert_eq!(
                    reader.get(EntryKind::Check, &Hash128(2)).as_deref(),
                    Some(b"two".as_slice())
                );
                assert!(legacy.is_file(), "a reader migrates nothing");
                // No table: the old pack has no footer to merge by.
                assert_eq!(
                    store.maintain(u64::MAX, None).unwrap().after,
                    6 + index_len(0) - HEADER as u64
                );
                assert!(is_real_dir(&store.index_dir()));
                assert!(!root.join("pack-v1/index.old").exists());
                assert!(!root.join("pack-v1/index.new").exists());
                assert_eq!(load(&store), entries);
                let reader = Store::new(&root);
                assert_eq!(
                    reader.get(EntryKind::Check, &Hash128(1)).as_deref(),
                    Some(b"one".as_slice())
                );
            }
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
            let result = store.maintain(14_000, None).unwrap();
            assert_eq!(result.before, pack_len(3, 4000) + index_len(1));
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

        #[test]
        fn exhausted_automatic_budget_reclaims_garbage_without_evicting_a_fitting_live_set() {
            let root = scratch("bounded-progress");
            let store = Store::new(&root);
            for _ in 0..4 {
                store.publish(&[item(1, &[7; 64])]).unwrap();
            }
            // One live record and its index fit the cap. Three old packs
            // exceed it but must not evict that still useful record.
            let cap = pack_len(1, 64) + index_len(1);
            let mut after = u64::MAX;
            for _ in 0..4 {
                after = store.maintain(cap, Some(Duration::ZERO)).unwrap().after;
                assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
                if after <= cap {
                    break;
                }
            }
            assert!(after <= cap, "bounded runs make physical progress");
        }

        #[test]
        fn a_recorded_total_under_the_cap_skips_the_walk_and_a_stale_one_rescans() {
            let root = scratch("total");
            let store = Store::new(&root);
            let bounded = |cap| store.maintain(cap, Some(Duration::from_mins(1))).unwrap();
            let one = pack_len(1, 64) + index_len(1);
            store.publish(&[item(1, &[7; 64])]).unwrap();
            // Nothing is recorded yet: the first bounded run walks.
            assert_eq!(bounded(u64::MAX).after, one);
            // An unindexed pack no writer recorded: only a walk sees it.
            let stray = store.path(unique());
            let plant = || std::fs::write(&stray, [0; 100]).unwrap();
            plant();
            store.publish(&[item(1, &[8; 64])]).unwrap();
            let run = bounded(u64::MAX);
            let added = pack_len(1, 64) + TABLE_ROW as u64;
            assert_eq!(run.after, one + added, "a publication adds its pack");
            assert!(stray.exists(), "a fitting total walks nothing");
            // Over the cap, the bounded run walks and reclaims dead bytes
            // without evicting the fitting live set.
            let run = bounded(one + 63);
            assert_eq!((run.before, run.after), (one + added + 100, one));
            assert!(!stray.exists());
            assert_eq!(store.get(EntryKind::Check, &Hash128(1)).unwrap()[0], 8);
            // A head no record names rescans.
            ages(&store, &[(1, 5)]);
            unrecorded(&store);
            plant();
            assert_eq!(bounded(u64::MAX).after, one, "an unrecorded index rescans");
            assert!(!stray.exists());
            // A hit's recency mark rewrites its page and keeps the record.
            store.get(EntryKind::Check, &Hash128(1)).unwrap();
            plant();
            assert_eq!(bounded(u64::MAX).after, one);
            assert!(load(&store).values().all(|loc| loc.used > 5));
            assert!(stray.exists());
            // Explicit collection always walks.
            assert_eq!(store.maintain(u64::MAX, None).unwrap().after, one);
            assert!(!stray.exists());
        }

        /// The index files' names, bytes and modification times.
        fn index_files(store: &Store) -> Vec<(std::ffi::OsString, Vec<u8>, SystemTime)> {
            let mut files: Vec<_> = std::fs::read_dir(store.index_dir())
                .unwrap()
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
            let before = index_files(&store);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            store.touch(EntryKind::Check, &Hash128(1));
            store.maintain(u64::MAX, None).unwrap();
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
            let result = store.maintain(cap, None).unwrap();
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
            assert!(store.maintain(cap, None).unwrap().after <= cap);
            assert!(!path.exists());
        }

        #[test]
        fn readers_do_not_wait_for_the_publication_lock() {
            let root = scratch("unlocked-reader");
            let store = Store::new(&root);
            store.publish(&[item(1, b"readable")]).unwrap();
            let _lock = store.lock().unwrap();
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
            Store::new(&root).maintain(u64::MAX, None).unwrap();
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
            assert_eq!(counts.index_read, (HEAD_LEN + page_len(FANOUT)) as u64);
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
                        store
                            .maintain(u64::MAX, Some(Duration::from_secs(5)))
                            .unwrap();
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
                    let (_lock, mut txn) = store.begin().unwrap();
                    let at = now() - 2 * 3600;
                    for (key, loc) in txn.all().unwrap() {
                        txn.set(key, Location { used: at, ..loc }).unwrap();
                    }
                    txn.commit(None, 0, || Ok(())).unwrap();
                }
                // Record the total for the rewritten index, as the
                // command that wrote it would have.
                store.maintain(super::super::DEFAULT_CAP, None).unwrap();
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
                std::fs::read_dir(root.join("pack-v1"))
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
            let collected = store.maintain(16 * 1024, None).unwrap();
            assert!(collected.after <= 16 * 1024, "{collected:?}");
            eprintln!(
                "1000 packs: load {load_time:?}, publication including merge {merge:?}, bytes {} -> {}",
                collected.before, collected.after
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
            std::fs::write(root.join("pack-v1"), b"not a directory").unwrap();
            let store = DiskStore::new(root).on_write_failure(warning);
            store.put(EntryKind::Check, &Hash128(1), b"value");
            store.put(EntryKind::Check, &Hash128(2), b"value");
            assert_eq!(WARNINGS.load(Ordering::Relaxed), 1);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_none());
        }

        #[test]
        fn failed_collection_reports_the_data_it_could_not_remove() -> std::io::Result<()> {
            let root = scratch("gc-write-failure");
            let store = DiskStore::new(root.clone()).with_cap(0);
            store.put(EntryKind::Check, &Hash128(1), b"data");
            let before = store.packed.bytes();
            assert!(before > 0);
            let lock = root.join("pack-v1/lock");
            std::fs::remove_file(&lock).unwrap();
            std::fs::create_dir(&lock).unwrap();
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
        store.clean(&[0], None)?;
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
        let store = DiskStore::new(root.clone())
            .with_cap(256 * 1000)
            .with_evict_bound(Duration::ZERO);
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
        let other = DiskStore::new(root.clone())
            .with_cap(256 * 1000)
            .with_evict_bound(Duration::ZERO);
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
