//! The compiled cache (Compiled Cache, `cli.cache.obj.*`, `cli.cache.gc.*`):
//! the cap from `HD_CACHE_MAX_SIZE`, automatic eviction over later
//! commands, least-recently-used order, `hd cache gc` inside and outside a
//! package, fetched versions left alone, and evicted entries read as misses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use hd_base::{Hash128, StableHasher};
use hd_cache::{CacheStore, DiskStore, EntryKind};
use hd_testkit::Scratch;

fn scratch(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

const CART: &str = "use pkg.tax\n\npub fn total(price: i32) -> i32:\n    price + tax.of(price)\n";
const TAX: &str = "pub fn of(price: i32) -> i32:\n    price / 10\n";
const MAIN: &str =
    "use pkg.cart\n\npub fn main() -> void $ Console:\n    println(\"{cart.total(+100)}\")\n";

/// A package `shop` of three modules in `src`.
fn package(name: &str) -> Scratch {
    let dir = scratch(name);
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    for (file, text) in [("main.hd", MAIN), ("cart.hd", CART), ("tax.hd", TAX)] {
        std::fs::write(dir.join("src").join(file), text).expect("write");
    }
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// `hd ARGS` in `cwd` over the cache directory `cache`, with the cap `cap`.
fn hd(cwd: &Path, cache: &Path, cap: Option<&str>, args: &[&str]) -> Ran {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", cache).env_remove("HD_CACHE_MAX_SIZE");
    if let Some(cap) = cap {
        c.env("HD_CACHE_MAX_SIZE", cap);
    }
    let output = c.current_dir(cwd).args(args).output().expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// One logical compiled entry. Its identity survives pack compaction.
struct Entry {
    kind: EntryKind,
    key: Hash128,
    pack: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

impl Entry {
    fn identity(&self) -> (EntryKind, Hash128) {
        (self.kind, self.key)
    }
}

const INDEX_HEADER: usize = 32;
const INDEX_ROW: usize = 58;

fn checksum_index(bytes: &mut [u8]) {
    let mut hash = StableHasher::new("cache-storage-index-v1");
    hash.bytes(&bytes[INDEX_HEADER..]);
    bytes[16..INDEX_HEADER].copy_from_slice(&hash.finish().0.to_le_bytes());
}

fn read_index(obj: &Path) -> Option<Vec<u8>> {
    let path = obj.join("pack-v1/index");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("{}: {error}", path.display()),
    };
    assert!(bytes.len() >= INDEX_HEADER);
    assert_eq!(&bytes[..8], b"HDPACK01");
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(
        bytes.len(),
        INDEX_HEADER + usize::try_from(count).unwrap() * INDEX_ROW
    );
    let mut checked = bytes.clone();
    checksum_index(&mut checked);
    assert_eq!(
        &checked[16..INDEX_HEADER],
        &bytes[16..INDEX_HEADER],
        "index checksum"
    );
    Some(bytes)
}

/// Read the logical directory and verify every indexed range exists in a
/// physical pack. Merely enumerating loose files would silently test none.
fn entries(obj: &Path) -> Vec<Entry> {
    let Some(index) = read_index(obj) else {
        return Vec::new();
    };
    index[INDEX_HEADER..]
        .chunks_exact(INDEX_ROW)
        .map(|row| {
            let kind = u16::from_le_bytes(row[..2].try_into().unwrap());
            let kind = EntryKind::ALL
                .into_iter()
                .find(|&k| k as u16 == kind)
                .unwrap();
            let key = Hash128(u128::from_le_bytes(row[2..18].try_into().unwrap()));
            let pack = u128::from_le_bytes(row[18..34].try_into().unwrap());
            let offset = u64::from_le_bytes(row[34..42].try_into().unwrap());
            let bytes = u64::from_le_bytes(row[42..50].try_into().unwrap());
            let used = u64::from_le_bytes(row[50..58].try_into().unwrap());
            let pack = obj.join(format!("pack-v1/{pack:032x}.pack"));
            assert!(
                offset.checked_add(bytes).unwrap() <= pack.metadata().expect("indexed pack").len()
            );
            Entry {
                kind,
                key,
                pack,
                bytes,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(used),
            }
        })
        .collect()
}

/// Physical compiled bytes, including index and obsolete pack records;
/// leases, the publication lock and the legacy stats ledger are not entries.
fn total(obj: &Path) -> u64 {
    std::fs::read_dir(obj.join("pack-v1"))
        .into_iter()
        .flatten()
        .map(|entry| {
            let entry = entry.expect("pack directory entry");
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type().expect("pack type").is_file()
                && (name == "index"
                    || name.strip_suffix(".pack").is_some_and(|id| {
                        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
                    }))
            {
                entry.metadata().expect("pack size").len()
            } else {
                0
            }
        })
        .sum()
}

fn set_age(obj: &Path, entry: &Entry, days: u64) {
    let time = SystemTime::now() - Duration::from_secs(days * 86_400);
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut index = read_index(obj).expect("index");
    let row = index[INDEX_HEADER..]
        .chunks_exact_mut(INDEX_ROW)
        .find(|row| {
            u16::from_le_bytes(row[..2].try_into().unwrap()) == entry.kind as u16
                && u128::from_le_bytes(row[2..18].try_into().unwrap()) == entry.key.0
        })
        .expect("logical entry");
    row[50..58].copy_from_slice(&seconds.to_le_bytes());
    checksum_index(&mut index);
    std::fs::write(obj.join("pack-v1/index"), index).expect("age index entry");
}

fn recent(e: &Entry) -> bool {
    e.modified
        .elapsed()
        .is_ok_and(|age| age < Duration::from_mins(10))
}

/// The two size lines of `hd cache gc`.
fn gc_lines(r: &Ran) -> (String, String) {
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", r.out);
    assert!(lines[0].starts_with("compiled cache before: "), "{}", r.out);
    assert!(lines[1].starts_with("compiled cache after: "), "{}", r.out);
    (lines[0].to_owned(), lines[1].to_owned())
}

#[test]
fn gc_outside_a_package_prints_both_sizes_and_creates_nothing() {
    let dir = scratch("gc-outside");
    let cache = dir.join("cache");
    let r = hd(&dir, &cache, None, &["cache", "gc"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let (before, after) = gc_lines(&r);
    assert_eq!(before, "compiled cache before: 0 B");
    assert_eq!(after, "compiled cache after: 0 B (cap 10.0 GiB)");
    assert!(!cache.exists());
    // `cli.cache.gc.anywhere`: it reads no manifest, so a broken one is fine.
    std::fs::write(dir.join("hd.toml"), "[package\nname = ").expect("write");
    let r = hd(&dir, &cache, Some("2G"), &["cache", "gc"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(gc_lines(&r).1, "compiled cache after: 0 B (cap 2.0 GiB)");
}

#[test]
fn gc_rejects_an_invalid_cap_and_extra_words() {
    let dir = scratch("gc-usage");
    let cache = dir.join("cache");
    for (cap, args) in [
        (Some("10X"), &["cache", "gc"][..]),
        (Some(""), &["cache", "gc"][..]),
        (None, &["cache"][..]),
        (None, &["cache", "gc", "now"][..]),
        (None, &["cache", "trim"][..]),
    ] {
        let r = hd(&dir, &cache, cap, args);
        assert_eq!(r.code, Some(101), "{cap:?} {args:?}: {}", r.out);
        assert!(r.out.is_empty());
    }
    // A command that compiles rejects the invalid cap too.
    let pkg = package("gc-usage-pkg");
    let r = hd(&pkg, &pkg.cache(), Some("lots"), &["check"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("HD_CACHE_MAX_SIZE"), "{}", r.err);
}

#[test]
fn gc_evicts_down_to_the_cap_and_leaves_fetched_versions() {
    let pkg = package("gc-cap");
    let cache = pkg.cache();
    let obj = cache.join("obj");
    let r = hd(&pkg, &cache, None, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let full = total(&obj);
    assert!(full > 64 * 1024, "{full}");
    // A fetched version and the other cache entries hd owns.
    let version = cache.join("pkg/github.com/acme/json@1.0.0");
    std::fs::create_dir_all(&version).expect("pkg");
    std::fs::write(version.join("hd.toml"), "[package]\nname = \"json\"\n").expect("write");
    std::fs::create_dir_all(cache.join("hash")).expect("hash");
    std::fs::write(cache.join("hash/github.com"), "h1").expect("write");
    let r = hd(&pkg, &cache, Some("16K"), &["cache", "gc"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let left = total(&obj);
    assert!(left <= 16 * 1024, "{left} bytes left");
    assert!(left < full);
    let (before, after) = gc_lines(&r);
    assert!(
        before.ends_with("KiB") || before.ends_with("MiB"),
        "{before}"
    );
    assert!(after.ends_with("(cap 16.0 KiB)"), "{after}");
    assert!(version.join("hd.toml").exists());
    assert!(cache.join("hash/github.com").exists());
    // A second run finds the cache within the cap and removes nothing.
    let r = hd(&pkg, &cache, Some("16K"), &["cache", "gc"]);
    assert_eq!(total(&obj), left);
    let (before, after) = gc_lines(&r);
    let before = before.strip_prefix("compiled cache before: ");
    let after = after.strip_prefix("compiled cache after: ");
    assert_eq!(before, after.and_then(|a| a.split(" (cap").next()));
    // The evicted entries are misses: the build still succeeds.
    let r = hd(&pkg, &cache, None, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

#[test]
fn commands_evict_over_the_cap_from_hd_cache_max_size() {
    let pkg = package("gc-auto");
    let cache = pkg.cache();
    let obj = cache.join("obj");
    let r = hd(&pkg, &cache, None, &["build"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(total(&obj) > 64 * 1024);
    // Each command cleans for a bounded time and later ones carry on, so
    // warm runs that write nothing bring the cache under the cap.
    for _ in 0..20 {
        let r = hd(&pkg, &cache, Some("16K"), &["check"]);
        assert_eq!(r.code, Some(0), "{}", r.err);
        if total(&obj) <= 16 * 1024 {
            break;
        }
    }
    assert!(total(&obj) <= 16 * 1024, "{} bytes left", total(&obj));
}

#[test]
fn a_recently_hit_entry_outlives_older_ones() {
    let pkg = package("gc-lru");
    let cache = pkg.cache();
    let obj = cache.join("obj");
    let r = hd(&pkg, &cache, None, &["check"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let initial = entries(&obj);
    let first: Vec<_> = initial.iter().map(Entry::identity).collect();
    for entry in &initial {
        set_age(&obj, entry, 10);
    }
    // An edit leaves the old entries of `tax` unused; the rest are hits,
    // which mark their entries used.
    std::fs::write(
        pkg.join("src/tax.hd"),
        "pub fn of(price: i32) -> i32:\n    price / 5\n",
    )
    .expect("write");
    let r = hd(&pkg, &cache, None, &["check"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let all = entries(&obj);
    let (used, unused): (Vec<&Entry>, Vec<&Entry>) = all.iter().partition(|e| recent(e));
    assert!(!unused.is_empty());
    // The hits are entries of the first run that are recent again.
    assert!(used.iter().any(|e| first.contains(&e.identity())));
    // The cap fits every recently used logical entry and its index rows,
    // but neither the ten-day-old entries nor a three-day-old filler.
    let kept_bytes = used.iter().map(|e| e.bytes).sum::<u64>()
        + u64::try_from(INDEX_HEADER + used.len() * INDEX_ROW).unwrap();
    let filler = (EntryKind::Check, Hash128(u128::MAX));
    assert!(!all.iter().any(|entry| entry.identity() == filler));
    {
        let store = DiskStore::new(obj.clone());
        store.put(
            filler.0,
            &filler.1,
            &vec![0; usize::try_from(kept_bytes + 1024).unwrap()],
        );
    }
    let fillers = entries(&obj)
        .into_iter()
        .filter(|e| e.identity() == filler)
        .collect::<Vec<_>>();
    assert_eq!(fillers.len(), 1);
    for entry in &fillers {
        set_age(&obj, entry, 3);
    }
    let cap = (kept_bytes.div_ceil(9) * 10).to_string();
    let r = hd(&pkg, &cache, Some(&cap), &["cache", "gc"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let survivors = entries(&obj)
        .into_iter()
        .map(|e| e.identity())
        .collect::<Vec<_>>();
    for entry in &used {
        assert!(
            survivors.contains(&entry.identity()),
            "a used entry was evicted: {:?}",
            entry.identity()
        );
    }
    for entry in &unused {
        assert!(
            !survivors.contains(&entry.identity()),
            "an unused entry stayed: {:?}",
            entry.identity()
        );
    }
    for entry in &fillers {
        assert!(
            !survivors.contains(&entry.identity()),
            "a filler stayed: {:?}",
            entry.identity()
        );
    }
    // Everything the check reads survived, so it checks no module again.
    let r = hd(&pkg, &cache, None, &["check", "--format", "json"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(r.out.contains("\"modules_checked\":0}"), "{}", r.out);
}

#[test]
fn evicted_and_vanished_entries_read_as_misses() {
    let pkg = package("gc-vanish");
    let cache = pkg.cache();
    let obj = cache.join("obj");
    let first = hd(&pkg, &cache, None, &["check", "--format", "json"]);
    assert_eq!(first.code, Some(0), "{}", first.err);
    // Alternate packs disappear, as if another process evicted their
    // entries. Multiple index rows can address the same immutable file.
    let packs = entries(&obj)
        .into_iter()
        .map(|e| e.pack)
        .collect::<BTreeSet<_>>();
    for (i, pack) in packs.iter().enumerate() {
        if i % 2 == 0 {
            std::fs::remove_file(pack).expect("remove");
        }
    }
    let second = hd(&pkg, &cache, None, &["check", "--format", "json"]);
    assert_eq!(second.code, Some(0), "{}", second.err);
    // The output, but for how many modules were checked again.
    let summary = |r: &Ran| {
        r.out
            .split(",\"modules_checked\":")
            .next()
            .unwrap_or("")
            .to_owned()
    };
    assert_eq!(summary(&first), summary(&second));
    // An entry cut short is a miss too.
    let packs = entries(&obj)
        .into_iter()
        .map(|e| e.pack)
        .collect::<BTreeSet<_>>();
    for pack in packs {
        let bytes = std::fs::read(&pack).expect("entry");
        std::fs::write(pack, &bytes[..bytes.len() / 2]).expect("truncate");
    }
    let third = hd(&pkg, &cache, None, &["check", "--format", "json"]);
    assert_eq!(third.code, Some(0), "{}", third.err);
    assert_eq!(summary(&first), summary(&third));
}

#[test]
fn eviction_running_beside_a_check_never_breaks_it() {
    let pkg = package("gc-concurrent");
    let cache = pkg.cache();
    let warm = hd(&pkg, &cache, None, &["build"]);
    assert_eq!(warm.code, Some(0), "{}", warm.err);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (builds, gcs) = std::thread::scope(|s| {
        let gc = s.spawn(|| {
            let mut runs = Vec::new();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                runs.push(hd(&pkg, &cache, Some("0"), &["cache", "gc"]));
            }
            runs
        });
        let builds: Vec<Ran> = (0..6).map(|_| hd(&pkg, &cache, None, &["build"])).collect();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        (builds, gc.join().expect("gc thread"))
    });
    assert!(!gcs.is_empty());
    for r in gcs {
        assert_eq!(r.code, Some(0), "{}", r.err);
    }
    for r in builds {
        assert_eq!((r.code, &r.out), (warm.code, &warm.out), "{}", r.err);
    }
}
