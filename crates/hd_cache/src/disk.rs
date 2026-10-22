//! The disk store (feature `disk`): ordinary data in immutable per-run
//! packs and a shared index under `<root>/pack-v1/`; private native code
//! remains in `<root>/<kind>/<shard>/<key>`. See DESIGN.md §5.4 and §5.7.
//! Only this module touches the file system (design-overview §2.2 rule 1).
//!
//! Pack readers take no publication lock. A pending pack's file lock
//! leases it while its writer runs; index publication and GC share one
//! writer lock. Every addressed range remains an unchanged HDCE frame.
//! Last-use marks are per entry, at most once an hour, and compaction
//! preserves them. The index and physical pack bytes count toward the cap.
//! Automatic cleanup has a soft time budget, completing its first pack
//! operation even if scanning used that budget; later runs finish the rest.
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

/// The default size cap of the compiled entries, 10 GiB (`cli.cache.obj.cap`).
pub const DEFAULT_CAP: u64 = 10 << 30;

/// How long a command's automatic eviction may run before it leaves the
/// rest to later commands. One shard is always cleaned whole.
pub const EVICT_BOUND: Duration = Duration::from_millis(20);

/// An entry used again within this age keeps its mtime.
const USE_MARK_AGE: Duration = Duration::from_hours(1);

const SHARDS: usize = 256;
const STATS: &str = "stats";
const STATS_MAGIC: &[u8; 8] = b"HDSTATS1";
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
    /// The bytes this store published, per shard, not yet in the ledger.
    written: [AtomicU64; SHARDS],
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

impl DiskStore {
    /// A store over `root` with the default cap.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            packed: packed::Store::new(&root),
            root,
            cap: DEFAULT_CAP,
            evict_bound: EVICT_BOUND,
            written: std::array::from_fn(|_| AtomicU64::new(0)),
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

    /// `hd cache gc` (`cli.cache.gc`): cleans every shard now, with no time
    /// bound, and writes the exact shard sizes to the ledger.
    #[must_use]
    pub fn collect(&self) -> Collected {
        let snapshot = Ledger::update(&self.root, |_| {});
        let loose = self.clean(&(0..=u8::MAX).collect::<Vec<_>>(), None, snapshot);
        let packed_before = self.packed.bytes();
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
        }
    }

    /// Records this store's published bytes and runs the bounded automatic
    /// eviction. Called on drop; a store never used does nothing.
    fn settle(&self) {
        if !self.used.load(Ordering::Relaxed) {
            return;
        }
        let deadline = Instant::now() + self.evict_bound;
        let budget = self.budget();
        let written: Vec<u64> = self
            .written
            .iter()
            .map(|w| w.swap(0, Ordering::Relaxed))
            .collect();
        let mut work = Vec::new();
        let snapshot = Ledger::update(&self.root, |l| {
            for (size, add) in l.sizes.iter_mut().zip(&written) {
                if *size != UNKNOWN {
                    *size = size.saturating_add(*add);
                }
            }
            // The drift rescan first, then the shards over budget, then
            // the shards of unknown size.
            let drift = u8::try_from(l.cursor % SHARDS as u64).expect("shard");
            l.cursor = (l.cursor + 1) % SHARDS as u64;
            work.push(drift);
            let shards = || (0..=u8::MAX).zip(l.sizes).filter(|(s, _)| *s != drift);
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
            let _ = self.clean(&work, Some(deadline), snapshot);
        }
    }

    /// Cleans `shards` in order, each down to 90% of its budget when it is
    /// over it, and records each one's exact size in the ledger read as
    /// `snapshot`. With a deadline, the shards after the first are left to
    /// a later command once it has passed.
    fn clean(
        &self,
        shards: &[u8],
        deadline: Option<Instant>,
        snapshot: Option<Ledger>,
    ) -> Collected {
        let budget = self.budget();
        let kinds = self.kinds();
        let mut done = Vec::new();
        let mut total = Collected {
            before: 0,
            after: 0,
        };
        for (i, &shard) in shards.iter().enumerate() {
            if i > 0 && deadline.is_some_and(|d| Instant::now() >= d) {
                break;
            }
            let (before, after) = self.clean_shard(&kinds, shard, budget);
            total.before += before;
            total.after += after;
            done.push((shard as usize, after));
        }
        // Another process may have added bytes since the snapshot: keep
        // them, and replace only what the snapshot said about the shard.
        if let Some(snapshot) = snapshot {
            let _ = Ledger::update(&self.root, |l| {
                for &(shard, after) in &done {
                    let (now, then) = (l.sizes[shard], snapshot.sizes[shard]);
                    l.sizes[shard] = if now == UNKNOWN || then == UNKNOWN {
                        after
                    } else {
                        now.saturating_sub(then).saturating_add(after)
                    };
                }
            });
        }
        total
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
    fn scan(&self, kinds: &[EntryKind], shard: u8) -> (u64, Vec<EntryStat>) {
        let mut out = Vec::new();
        for &kind in kinds {
            let dir = self.root.join(kind.as_str()).join(format!("{shard:02x}"));
            if !is_real_dir(&dir) {
                continue;
            }
            let Ok(list) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in list.flatten() {
                let Some(key) = e.file_name().to_str().and_then(parse_key) else {
                    continue;
                };
                // `DirEntry::metadata` does not follow a symbolic link.
                let Ok(meta) = e.metadata() else { continue };
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
        (out.iter().map(|e| e.bytes).sum(), out)
    }

    /// Scans one shard and evicts its oldest entries when it is over
    /// `budget`: its size before and after.
    fn clean_shard(&self, kinds: &[EntryKind], shard: u8, budget: u64) -> (u64, u64) {
        let (before, entries) = self.scan(kinds, shard);
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
        (before, after)
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
            std::fs::rename(&tmp, &path).or_else(|e| {
                let _ = std::fs::remove_file(&tmp);
                // Another writer of the same key published first: a lost
                // race, not a failure.
                if path.is_file() { Ok(()) } else { Err(e) }
            })
        };
        match publish() {
            Ok(()) => {
                self.written[shard_of(*key) as usize]
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            Err(e) => {
                if !self.write_failed.swap(true, Ordering::Relaxed)
                    && let Some(warn) = self.on_write_failure
                {
                    warn(&self.root, &e);
                }
            }
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

/// A file name of exactly 32 lowercase hex digits, as [`DiskStore::path`]
/// writes it.
fn parse_key(name: &str) -> Option<Hash128> {
    (name.len() == 32 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| u128::from_str_radix(name, 16).ok().map(Hash128))
        .flatten()
}

/// The shard-size ledger `<root>/stats`: a magic, the next shard to
/// rescan, and 256 sizes, little-endian.
struct Ledger {
    cursor: u64,
    sizes: [u64; SHARDS],
}

impl Ledger {
    fn decode(bytes: &[u8]) -> Ledger {
        let mut l = Ledger {
            cursor: 0,
            sizes: [UNKNOWN; SHARDS],
        };
        if bytes.len() == STATS_LEN && &bytes[..8] == STATS_MAGIC {
            let word = |i: usize| {
                u64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().expect("8 bytes"))
            };
            l.cursor = word(0);
            for (i, s) in l.sizes.iter_mut().enumerate() {
                *s = word(i + 1);
            }
        }
        l
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(STATS_LEN);
        out.extend_from_slice(STATS_MAGIC);
        out.extend_from_slice(&self.cursor.to_le_bytes());
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
        if !is_real_dir(root) {
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
        f(&mut l);
        let out = l.encode();
        if file.seek(SeekFrom::Start(0)).is_ok() && file.write_all(&out).is_ok() {
            let _ = file.set_len(out.len() as u64);
        }
        Some(l)
    }
}

/// Immutable data packs. The index is the only publication point; a pack's
/// exclusive file lock leases it until publication, or until its writer dies.
mod packed {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::{File, OpenOptions, TryLockError};
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant, SystemTime};

    use hd_base::{Hash128, StableHasher};

    use super::{Collected, EntryKind, USE_MARK_AGE, is_real_dir, parse_key};

    const MAGIC: &[u8; 8] = b"HDPACK01";
    const HEADER: usize = 32;
    const ROW: usize = 58;
    const MAX_INDEX: usize = 64 * 1024 * 1024;
    const MAX_ENTRY: u64 = 64 * 1024 * 1024;
    const MAX_HANDLES: usize = 64;
    type Key = (u16, u128);
    type Entries = BTreeMap<Key, Location>;
    type Batch = [(EntryKind, Hash128, Arc<[u8]>)];

    #[derive(Clone, Debug)]
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

    #[derive(Default)]
    struct Snapshot {
        entries: Entries,
        files: BTreeMap<u128, Opened>,
        touched: BTreeMap<Key, u64>,
    }

    pub(super) struct Store {
        root: PathBuf,
        snapshot: Mutex<Option<Snapshot>>,
        used: AtomicU64,
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

    fn index_len(count: usize) -> io::Result<usize> {
        count
            .checked_mul(ROW)
            .and_then(|n| n.checked_add(HEADER))
            .filter(|&n| n <= MAX_INDEX)
            .ok_or_else(|| invalid("cache index exceeds its size limit"))
    }

    fn encode(entries: &Entries) -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(index_len(entries.len())?);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(
            &u32::try_from(entries.len())
                .map_err(io::Error::other)?
                .to_le_bytes(),
        );
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u128.to_le_bytes());
        for (&(kind, key), loc) in entries {
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&loc.pack.to_le_bytes());
            out.extend_from_slice(&loc.offset.to_le_bytes());
            out.extend_from_slice(&loc.len.to_le_bytes());
            out.extend_from_slice(&loc.used.to_le_bytes());
        }
        let sum = checksum(&out[HEADER..]);
        out[16..HEADER].copy_from_slice(&sum.to_le_bytes());
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> io::Result<Entries> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC || bytes[12..16] != [0; 4] {
            return Err(invalid("invalid cache index header"));
        }
        let count = usize::try_from(u32::from_le_bytes(bytes[8..12].try_into().expect("header")))
            .map_err(io::Error::other)?;
        if index_len(count)? != bytes.len()
            || checksum(&bytes[HEADER..])
                != u128::from_le_bytes(bytes[16..HEADER].try_into().expect("header"))
        {
            return Err(invalid("invalid cache index length or checksum"));
        }
        let mut entries = Entries::new();
        let mut previous = None;
        for row in bytes[HEADER..].chunks_exact(ROW) {
            let kind = u16::from_le_bytes(row[..2].try_into().expect("row"));
            let key = u128::from_le_bytes(row[2..18].try_into().expect("row"));
            if !EntryKind::ALL
                .iter()
                .any(|&k| k as u16 == kind && k != EntryKind::Clpack)
                || previous.is_some_and(|p| p >= (kind, key))
            {
                return Err(invalid("invalid or unordered cache index key"));
            }
            let loc = Location {
                pack: u128::from_le_bytes(row[18..34].try_into().expect("row")),
                offset: u64::from_le_bytes(row[34..42].try_into().expect("row")),
                len: u64::from_le_bytes(row[42..50].try_into().expect("row")),
                used: u64::from_le_bytes(row[50..58].try_into().expect("row")),
            };
            if loc.len > MAX_ENTRY || loc.offset.checked_add(loc.len).is_none() {
                return Err(invalid("invalid cache index range"));
            }
            previous = Some((kind, key));
            entries.insert((kind, key), loc);
        }
        Ok(entries)
    }

    impl Store {
        pub(super) fn new(root: &Path) -> Self {
            Self {
                root: root.join("pack-v1"),
                snapshot: Mutex::new(None),
                used: AtomicU64::new(0),
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

        fn load(&self) -> io::Result<Entries> {
            if !is_real_dir(&self.root) {
                return Ok(Entries::new());
            }
            let path = self.root.join("index");
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Entries::new()),
                Err(e) => return Err(e),
                Ok(meta)
                    if !meta.file_type().is_file()
                        || meta.len() > u64::try_from(MAX_INDEX).expect("limit") =>
                {
                    return Err(invalid("cache index is not a bounded regular file"));
                }
                Ok(_) => {}
            }
            let mut bytes = Vec::new();
            File::open(path)?
                .take(u64::try_from(MAX_INDEX + 1).expect("limit"))
                .read_to_end(&mut bytes)?;
            decode(&bytes)
        }

        /// Called only under the publication lock. A corrupt index is
        /// derived data: start a new one, leaving active leased packs alone.
        fn recover(&self) -> io::Result<(Entries, bool)> {
            match self.load() {
                Ok(entries) => Ok((entries, false)),
                Err(e) if e.kind() == io::ErrorKind::InvalidData => Ok((Entries::new(), true)),
                Err(e) => Err(e),
            }
        }

        fn save(&self, entries: &Entries) -> io::Result<()> {
            let path = self.root.join("index");
            if entries.is_empty() {
                return match std::fs::remove_file(path) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                    result => result,
                };
            }
            let bytes = encode(entries)?;
            let tmp = self.root.join(format!("index.{:032x}.tmp", unique()));
            let result = (|| {
                let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
                file.write_all(&bytes)?;
                drop(file);
                std::fs::rename(&tmp, path)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(tmp);
            }
            result
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
            Ok(())
        }

        fn open(&self, id: u128) -> io::Result<Opened> {
            let path = self.path(id);
            if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
                return Err(invalid("cache pack is not a regular file"));
            }
            let file = File::open(path)?;
            let len = file.metadata()?.len();
            Ok(Opened { file, len })
        }

        pub(super) fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = guard.get_or_insert_with(|| Snapshot {
                entries: self.load().unwrap_or_default(),
                ..Snapshot::default()
            });
            let k = (kind as u16, key.0);
            let loc = state.entries.get(&k)?.clone();
            if !state.files.contains_key(&loc.pack) {
                let file = self.open(loc.pack).ok()?;
                if state.files.len() >= MAX_HANDLES {
                    state.files.pop_first();
                }
                state.files.insert(loc.pack, file);
            }
            let opened = state.files.get_mut(&loc.pack)?;
            if loc.len > MAX_ENTRY || loc.offset.checked_add(loc.len)? > opened.len {
                return None;
            }
            let mut bytes = vec![0; usize::try_from(loc.len).ok()?];
            opened.file.seek(SeekFrom::Start(loc.offset)).ok()?;
            opened.file.read_exact(&mut bytes).ok()?;
            let at = now();
            if at.saturating_sub(loc.used) > USE_MARK_AGE.as_secs() {
                state.touched.insert(k, at);
            }
            Some(Arc::from(bytes))
        }

        pub(super) fn touch(&self, kind: EntryKind, key: &Hash128) {
            self.used.store(1, Ordering::Relaxed);
            let mut guard = self.state();
            let state = guard.get_or_insert_with(|| Snapshot {
                entries: self.load().unwrap_or_default(),
                ..Snapshot::default()
            });
            let k = (kind as u16, key.0);
            let at = now();
            if state
                .entries
                .get(&k)
                .is_some_and(|l| at.saturating_sub(l.used) > USE_MARK_AGE.as_secs())
            {
                state.touched.insert(k, at);
            }
        }

        fn merge_touches(&self, entries: &mut Entries) -> bool {
            let mut guard = self.state();
            let Some(state) = guard.as_mut() else {
                return false;
            };
            let mut changed = false;
            for (key, used) in std::mem::take(&mut state.touched) {
                if let Some(loc) = entries.get_mut(&key)
                    && used > loc.used
                {
                    loc.used = used;
                    changed = true;
                }
            }
            changed
        }

        fn replace(&self, entries: Entries) {
            let mut guard = self.state();
            let touched = guard
                .as_mut()
                .map(|s| std::mem::take(&mut s.touched))
                .unwrap_or_default();
            *guard = Some(Snapshot {
                entries,
                touched,
                files: BTreeMap::new(),
            });
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
            index_len(batch.len())?;
            let mut lease = {
                let _lock = self.lock()?;
                self.lease()?
            };
            let mut offset = 0_u64;
            let mut pending = Entries::new();
            let at = now();
            for (kind, key, bytes) in batch {
                lease.file.write_all(bytes)?;
                let len = u64::try_from(bytes.len()).map_err(io::Error::other)?;
                pending.insert(
                    (*kind as u16, key.0),
                    Location {
                        pack: lease.id,
                        offset,
                        len,
                        used: at,
                    },
                );
                offset = offset
                    .checked_add(len)
                    .ok_or_else(|| invalid("cache pack length overflow"))?;
            }
            let _lock = self.lock()?;
            let (mut entries, _) = self.recover()?;
            self.merge_touches(&mut entries);
            entries.extend(pending);
            // The writer lock still excludes GC; unlock before readers
            // can see the new index (Windows byte-range locks block reads).
            self.finish(&mut lease)?;
            self.save(&entries)?;
            lease.committed = true;
            self.replace(entries);
            Ok(())
        }

        fn pack_id(name: &str) -> Option<u128> {
            name.strip_suffix(".pack").and_then(parse_key).map(|k| k.0)
        }

        /// Scans managed files with bounded memory, including orphan bytes.
        /// It never follows a link or removes a foreign filename.
        fn scan(&self) -> io::Result<(u64, BTreeMap<u128, u64>)> {
            let mut total = 0_u64;
            let mut sizes = BTreeMap::new();
            // Only indexed IDs need memory; orphan enumeration is streamed.
            let wanted: BTreeSet<_> = self
                .state()
                .as_ref()
                .map(|s| s.entries.values().map(|loc| loc.pack).collect())
                .unwrap_or_default();
            for e in std::fs::read_dir(&self.root)? {
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
                } else if name == "index" {
                    total = total.saturating_add(e.metadata()?.len());
                }
            }
            Ok((total, sizes))
        }

        fn prune_orphans(
            &self,
            live: &BTreeSet<u128>,
            deadline: Option<Instant>,
        ) -> io::Result<()> {
            let mut attempted = false;
            for e in std::fs::read_dir(&self.root)? {
                if attempted && deadline.is_some_and(|d| Instant::now() >= d) {
                    break;
                }
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
            Ok(())
        }

        fn compact(&self, entries: &mut Entries, id: u128) -> io::Result<()> {
            let mut source = self.open(id)?.file;
            let mut lease = self.lease()?;
            let mut offset = 0_u64;
            for loc in entries.values_mut().filter(|loc| loc.pack == id) {
                source.seek(SeekFrom::Start(loc.offset))?;
                let got = io::copy(&mut (&mut source).take(loc.len), &mut lease.file)?;
                if got != loc.len {
                    return Err(invalid("truncated cache pack during compaction"));
                }
                loc.pack = lease.id;
                loc.offset = offset;
                offset += loc.len;
            }
            self.finish(&mut lease)?;
            // Saving here makes every newly unlocked pack indexed before
            // its lease guard is dropped, including if later GC work fails.
            self.save(entries)?;
            lease.committed = true;
            Ok(())
        }

        pub(super) fn maintain(&self, cap: u64, bound: Option<Duration>) -> io::Result<Collected> {
            if !is_real_dir(&self.root) {
                return Ok(Collected {
                    before: 0,
                    after: 0,
                });
            }
            let _lock = self.lock()?;
            let deadline = bound.map(|b| Instant::now() + b);
            let (mut entries, mut dirty) = self.recover()?;
            dirty |= self.merge_touches(&mut entries);
            // Refresh the scan's bounded set from the latest committed index.
            self.replace(entries.clone());
            let (before, sizes) = self.scan()?;
            entries.retain(|_, loc| {
                let valid = sizes.get(&loc.pack).is_some_and(|len| {
                    loc.offset
                        .checked_add(loc.len)
                        .is_some_and(|end| end <= *len)
                });
                dirty |= !valid;
                valid
            });
            let mut live_bytes = entries.values().map(|loc| loc.len).sum::<u64>();
            let live_index = if entries.is_empty() {
                0
            } else {
                u64::try_from(index_len(entries.len())?).expect("limit")
            };
            // Dead records and orphans must be reclaimed before they cost
            // live entries: only evict when the live set itself exceeds cap.
            if live_bytes.saturating_add(live_index) > cap {
                let mut order = entries
                    .iter()
                    .map(|(&key, loc)| (loc.used, key))
                    .collect::<Vec<_>>();
                order.sort_unstable();
                for (_, key) in order {
                    let overhead = if entries.is_empty() {
                        0
                    } else {
                        u64::try_from(index_len(entries.len())?).expect("limit")
                    };
                    if live_bytes.saturating_add(overhead) <= cap.saturating_mul(9) / 10 {
                        break;
                    }
                    if let Some(loc) = entries.remove(&key) {
                        live_bytes -= loc.len;
                        dirty = true;
                    }
                }
            }
            let mut live_per_pack = BTreeMap::<u128, u64>::new();
            for loc in entries.values() {
                *live_per_pack.entry(loc.pack).or_default() += loc.len;
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
                    self.compact(&mut entries, id)?;
                    compacted = true;
                    dirty = false; // compact saved the entire current index.
                }
            }
            if dirty || entries.is_empty() {
                self.save(&entries)?;
            }
            let live = entries.values().map(|loc| loc.pack).collect();
            self.prune_orphans(&live, deadline)?;
            self.replace(entries);
            let (after, _) = self.scan()?;
            Ok(Collected { before, after })
        }

        /// Best-effort physical accounting also works when the writer
        /// lock cannot be opened; a failed GC must not report zero data.
        pub(super) fn bytes(&self) -> u64 {
            if !is_real_dir(&self.root) {
                return 0;
            }
            self.scan().map_or(0, |(bytes, _)| bytes)
        }

        pub(super) fn was_used(&self) -> bool {
            self.used.load(Ordering::Relaxed) != 0
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
            let _lock = store.lock().unwrap();
            let mut entries = store.load().unwrap();
            for &(key, at) in times {
                entries
                    .get_mut(&(EntryKind::Check as u16, key))
                    .unwrap()
                    .used = at;
            }
            store.save(&entries).unwrap();
            store.replace(entries);
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
            assert_eq!(store.load().unwrap().len(), 2);
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
            assert_eq!(reader.load().unwrap().len(), 16);
            for key in 0..16_u8 {
                assert_eq!(
                    reader
                        .get(EntryKind::Check, &Hash128(u128::from(key)))
                        .as_deref(),
                    Some([key, 3].as_slice())
                );
            }
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
            let id = store.load().unwrap().values().next().unwrap().pack;
            let path = store.path(id);
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
            let good = encode(&store.load().unwrap()).unwrap();
            for at in [0, 8, 12, 16, HEADER, good.len() - 1] {
                let mut bad = good.clone();
                bad[at] ^= 0xff;
                assert!(decode(&bad).is_err());
                std::fs::write(store.root.join("index"), bad).unwrap();
                assert!(
                    Store::new(&root)
                        .get(EntryKind::Check, &Hash128(1))
                        .is_none()
                );
            }
            let mut entries = decode(&good).unwrap();
            entries.values_mut().next().unwrap().len = MAX_ENTRY + 1;
            assert!(decode(&encode(&entries).unwrap()).is_err());
            entries.values_mut().next().unwrap().len = 2;
            entries.values_mut().next().unwrap().offset = u64::MAX;
            assert!(decode(&encode(&entries).unwrap()).is_err());
            let file = OpenOptions::new()
                .write(true)
                .open(store.root.join("index"))
                .unwrap();
            file.set_len(u64::try_from(MAX_INDEX + 1).unwrap()).unwrap();
            assert!(store.load().is_err());
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
            assert_eq!(store.load().unwrap().len(), 1);
        }

        #[test]
        fn entry_recency_and_compaction_preserve_the_survivors_ages() {
            let root = scratch("lru");
            let store = Store::new(&root);
            let data = [7; 400];
            store
                .publish(&[item(1, &data), item(2, &data), item(3, &data)])
                .unwrap();
            ages(&store, &[(1, 1), (2, 2), (3, 3)]);
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            let result = store.maintain(1200, None).unwrap();
            assert_eq!(result.before, 1406);
            assert_eq!(result.after, 948);
            let entries = store.load().unwrap();
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
            // One live record + index fits in 160 bytes. Three old packs
            // exceed the cap but must not evict that still useful record.
            let mut after = u64::MAX;
            for _ in 0..4 {
                after = store.maintain(160, Some(Duration::ZERO)).unwrap().after;
                assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
                if after <= 160 {
                    break;
                }
            }
            assert!(after <= 160, "bounded runs make physical progress");
        }

        #[test]
        fn recent_hits_do_not_rewrite_the_index() {
            let root = scratch("recent");
            let store = Store::new(&root);
            store.publish(&[item(1, b"value")]).unwrap();
            ages(&store, &[(1, now() - 60)]);
            let path = store.root.join("index");
            let before = std::fs::read(&path).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert!(store.get(EntryKind::Check, &Hash128(1)).is_some());
            store.touch(EntryKind::Check, &Hash128(1));
            store.maintain(u64::MAX, None).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(
                std::fs::metadata(path).unwrap().modified().unwrap(),
                modified
            );
        }

        #[test]
        fn open_readers_survive_compaction_and_pending_deletes_are_retried() {
            let root = scratch("open-reader");
            let store = Store::new(&root);
            let data = [5; 400];
            store
                .publish(&[item(1, &data), item(2, &data), item(3, &data)])
                .unwrap();
            ages(&store, &[(1, 1), (2, 2), (3, 3)]);
            let path = store.path(store.load().unwrap().values().next().unwrap().pack);
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // READ|WRITE, deliberately withholding FILE_SHARE_DELETE.
                options.share_mode(3);
            }
            let mut pinned = options.open(&path).unwrap();
            let result = store.maintain(1200, None).unwrap();
            #[cfg(windows)]
            {
                assert!(path.exists(), "a reader denied deletion");
                assert!(result.after > 1200);
            }
            #[cfg(not(windows))]
            assert!(result.after <= 1200);
            let mut old = Vec::new();
            pinned.read_to_end(&mut old).unwrap();
            assert_eq!(
                old,
                [data.as_slice(), data.as_slice(), data.as_slice()].concat()
            );
            assert_eq!(
                Store::new(&root)
                    .get(EntryKind::Check, &Hash128(3))
                    .as_deref(),
                Some(data.as_slice())
            );
            drop(pinned);
            assert!(store.maintain(1200, None).unwrap().after <= 1200);
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
            let entries = store.load().unwrap();
            let load = at.elapsed();
            assert_eq!(entries.len(), 1000);
            let at = Instant::now();
            store.publish(&[item(500, b"new")]).unwrap();
            let merge = at.elapsed();
            assert_eq!(store.load().unwrap().len(), 1000);
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
            assert!(collected.after <= 16 * 1024);
            eprintln!(
                "1000 packs: load {load:?}, publication including merge {merge:?}, bytes {} -> {}",
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
        fn failed_collection_reports_the_data_it_could_not_remove() {
            let root = scratch("gc-write-failure");
            let store = DiskStore::new(root.clone()).with_cap(0);
            store.put(EntryKind::Check, &Hash128(1), b"data");
            let before = store.packed.bytes();
            assert!(before > 0);
            let lock = root.join("pack-v1/lock");
            std::fs::remove_file(&lock).unwrap();
            std::fs::create_dir(&lock).unwrap();
            let collected = store.collect();
            assert_eq!(collected.before, before);
            assert_eq!(collected.after, before);
        }

        #[test]
        fn collection_shares_the_cap_with_private_entries_and_retires_old_data() {
            let root = scratch("legacy-cap");
            let store = DiskStore::new(root).with_cap(256 * 1000);
            store.put(EntryKind::Clpack, &Hash128(1), &[2; 800]);
            store.publish(EntryKind::Check, &Hash128(2), b"obsolete", |path| {
                File::create(path)
            });
            store.put_many(&[item(3, &vec![3; 128 * 1024]), item(4, &vec![4; 128 * 1024])]);
            let result = store.collect();
            assert!(result.after <= 256 * 1000);
            assert!(!store.path(EntryKind::Check, &Hash128(2)).exists());
            assert!(store.get(EntryKind::Clpack, &Hash128(1)).is_some());
            assert!(store.get(EntryKind::Check, &Hash128(4)).is_some());
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
        (0..=u8::MAX).map(|s| store.scan(&kinds, s).0).sum()
    }

    #[test]
    fn a_hit_marks_use_so_the_entry_survives_eviction() {
        let root = scratch("lru");
        let store = DiskStore::new(root.clone()).with_cap(256 * 1000);
        let (a, b, c) = (key(7, 1), key(7, 2), key(7, 3));
        for (k, days) in [(a, 3), (b, 2), (c, 1)] {
            store.put_loose(EntryKind::Check, &k, &[0; 400]);
            age(&store, EntryKind::Check, k, days);
        }
        // `a` is the oldest, but a hit makes it the newest.
        assert!(store.get_loose(EntryKind::Check, &a).is_some());
        let got = store.collect();
        assert_eq!((got.before, got.after), (1200, 800));
        assert!(store.path(EntryKind::Check, &a).exists());
        assert!(!store.path(EntryKind::Check, &b).exists());
        assert!(store.path(EntryKind::Check, &c).exists());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
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
    fn eviction_deletes_only_entry_files_of_the_obj_tree() {
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
        let got = store.collect();
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
        assert_eq!(read(&root).cursor, 1);
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
    fn the_ledger_keeps_bytes_another_process_added() {
        let root = scratch("ledger");
        let store = DiskStore::new(root.clone())
            .with_cap(256 * 1000)
            .with_evict_bound(Duration::ZERO);
        let k = key(5, 1);
        // Start from exact sizes, so the bounded drops below need no
        // rescan: the test is about byte merging, not eviction timing.
        assert_eq!(store.collect().after, 0);
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
                    let _ = store.collect();
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
    fn threads_writing_one_key_publish_whole_entries() {
        let root = scratch("same-key");
        let store = DiskStore::new(root.clone());
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
        let dir = store.path(EntryKind::Check, &k);
        let left = std::fs::read_dir(dir.parent().expect("shard"))
            .expect("shard")
            .count();
        assert_eq!(left, 1, "no temporary file is left behind");
        drop(store);
        let _ = std::fs::remove_dir_all(root);
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
