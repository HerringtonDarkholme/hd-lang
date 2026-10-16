//! The disk store (feature `disk`): `<root>/<kind>/<shard>/<key>`,
//! published by write-then-rename (`crates/hd_cache/DESIGN.md` §5.4), with use marks, the
//! shard-size ledger `<root>/stats` and least-recently-used eviction down
//! to the size cap (§5.7; `cli.cache.obj.cap`, `cli.cache.obj.evict`).
//! Only this module touches the file system (`docs/compiler/design-overview.md` §2.2
//! rule 1).
//!
//! - **Use marks.** A hit sets the entry's mtime to now when it is more
//!   than an hour old, so the mtime is the last use to within an hour, at
//!   most one write per entry per hour.
//! - **Size accounting.** `stats` holds 256 approximate shard sizes and the
//!   next shard to rescan. A store adds the bytes it published per shard
//!   when it is dropped, under a short lock of that 2 KB file. No command
//!   walks the whole tree to learn the total.
//! - **Eviction.** On drop, a store that was used rescans one shard in
//!   round-robin order (drift), then cleans the shards over their budget
//!   (the cap divided by 256) and the shards of unknown size, oldest
//!   first, each down to 90% of its budget, until [`EVICT_BOUND`] has
//!   passed. What is left over is cleaned by later commands.
//! - **Safety.** Eviction deletes only regular files at
//!   `<root>/<kind>/<shard>/<32 hex>` whose name parses as a key of that
//!   shard, and follows no symbolic link. A reader that finds an entry gone
//!   reads a miss (`cli.cache.obj.derived`).

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
        self.clean(&(0..=u8::MAX).collect::<Vec<_>>(), None, snapshot)
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
        for e in plan_eviction(&entries, budget) {
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
        self.settle();
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
    /// A missing, vanished or unreadable entry is a miss.
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        self.read(kind, key, |_, _| true)
    }

    /// A failed publish is reported once and turns later puts off, so the
    /// run continues uncached (§5.4).
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.publish(kind, key, bytes, |tmp| File::create(tmp));
    }

    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.used.store(true, Ordering::Relaxed);
        let path = self.path(kind, key);
        if std::fs::metadata(&path).is_ok_and(|m| stale(&m)) {
            mark_used(&path);
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
        file.read_to_end(&mut bytes).ok()?;
        let mut l = Ledger::decode(&bytes);
        f(&mut l);
        let out = l.encode();
        if file.seek(SeekFrom::Start(0)).is_ok() && file.write_all(&out).is_ok() {
            let _ = file.set_len(out.len() as u64);
        }
        Some(l)
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskStore, Ledger, SHARDS, UNKNOWN, parse_key};
    use crate::store::{CacheStore, EntryKind};
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
            store.put(EntryKind::Check, &k, &[0; 400]);
            age(&store, EntryKind::Check, k, days);
        }
        // `a` is the oldest, but a hit makes it the newest.
        assert!(store.get(EntryKind::Check, &a).is_some());
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
        store.put(EntryKind::Iface, &k, b"x");
        let path = store.path(EntryKind::Iface, &k);
        let set = SystemTime::now() - Duration::from_mins(1);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("entry")
            .set_modified(set)
            .expect("mtime");
        store.touch(EntryKind::Iface, &k);
        assert!(store.get(EntryKind::Iface, &k).is_some());
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
        store.put(EntryKind::Link, &k, b"bytes");
        std::fs::remove_file(store.path(EntryKind::Link, &k)).expect("remove");
        assert!(store.get(EntryKind::Link, &k).is_none());
        store.touch(EntryKind::Link, &k);
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
        store.put(EntryKind::Codepack, &k, &[1; 64]);
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
                .touch(EntryKind::Check, &key(0, 1));
        };
        // A cap of 256 bytes is a budget of one byte per shard.
        let store = DiskStore::new(root.clone()).with_cap(256);
        for n in 0..8 {
            store.put(EntryKind::Check, &key(n * 30, 1), &[0; 100]);
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
            store.put(EntryKind::Check, &key(9, n), &[0; 600]);
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
        store.put(EntryKind::Iface, &k, &[0; 300]);
        drop(store);
        let read = |r: &PathBuf| Ledger::decode(&std::fs::read(r.join("stats")).expect("ledger"));
        assert_eq!(read(&root).sizes[5], 300);
        assert_eq!(read(&root).sizes[6], 0);
        // A later store over the same tree adds its own bytes.
        let other = DiskStore::new(root.clone())
            .with_cap(256 * 1000)
            .with_evict_bound(Duration::ZERO);
        other.put(EntryKind::Iface, &key(5, 2), &[0; 200]);
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
                        store.put(EntryKind::Clpack, k, &body);
                    }
                    let _ = store.collect();
                }
            });
            s.spawn(|| {
                for _ in 0..200 {
                    for k in &keys {
                        if let Some(got) = store.get(EntryKind::Clpack, k) {
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
                        store.put(EntryKind::Check, &k, &body);
                    }
                });
            }
        });
        assert_eq!(store.get(EntryKind::Check, &k).as_deref(), Some(&body[..]));
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
        store.put(EntryKind::Clpack, &k3, b"code");
        let loose = std::fs::metadata(store.path(EntryKind::Clpack, &k3)).expect("entry");
        if loose.mode() & 0o777 != 0o600 {
            assert!(store.get_private(EntryKind::Clpack, &k3, me).is_none());
        }
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }
}
