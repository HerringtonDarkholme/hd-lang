//! The compiled cache (Compiled Cache, `cli.cache.obj.*`, `cli.cache.gc.*`):
//! the cap from `HD_CACHE_MAX_SIZE`, automatic eviction over later
//! commands, least-recently-used order, `hd cache gc` inside and outside a
//! package, fetched versions left alone, and evicted entries read as misses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use hd_base::Hash128;
use hd_cache::{CacheStore, DiskStore, EntryKind};
use hd_testkit::Scratch;

fn scratch(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

const CART: &str = include_str!("programs/cache_gc/cart.hd");
const TAX: &str = include_str!("programs/cache_gc/tax.hd");
const MAIN: &str = include_str!("programs/cache_gc/main.hd");

/// A package `shop` of three modules in `src`.
fn package(name: &str) -> Scratch {
    let dir = scratch(name);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
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
    let mut c = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
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
pub(super) struct Entry {
    pub(super) kind: EntryKind,
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

/// Read the logical directory and verify every indexed range exists in a
/// physical pack. Merely enumerating loose files would silently test none.
/// The pack lengths are read with the index, under its lock: another
/// test's hd may merge packs of a shared store right after.
pub(super) fn entries(obj: &Path) -> Vec<Entry> {
    DiskStore::new(obj.to_path_buf())
        .index_entries()
        .into_iter()
        .map(|e| {
            assert!(
                e.offset.checked_add(e.len).unwrap() <= e.pack_len.expect("indexed pack"),
                "{e:?} past its pack"
            );
            Entry {
                kind: e.kind,
                key: e.key,
                pack: e.pack,
                bytes: e.len,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(e.used),
            }
        })
        .collect()
}

/// The managed files of `dir` that `keep` accepts, by name, in bytes.
fn sizes(dir: &Path, keep: impl Fn(&str) -> bool) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .map(|entry| {
            let entry = entry.expect("pack directory entry");
            let name = entry.file_name();
            if entry.file_type().expect("pack type").is_file() && keep(&name.to_string_lossy()) {
                entry.metadata().expect("pack size").len()
            } else {
                0
            }
        })
        .sum()
}

/// The index's bytes: each shard's head, pages and pack table.
fn index_bytes(obj: &Path) -> u64 {
    (0..16)
        .map(|shard| {
            sizes(&obj.join(format!("pack-v3/index/{shard:x}")), |name| {
                !name.contains('.') && name != "lock"
            })
        })
        .sum()
}

/// Physical compiled bytes, including index and obsolete pack records;
/// leases, the publication lock and the legacy stats ledger are not entries.
fn total(obj: &Path) -> u64 {
    let packs = sizes(&obj.join("pack-v3"), |name| {
        name.strip_suffix(".pack")
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    });
    packs + index_bytes(obj)
}

fn set_age(obj: &Path, entry: &Entry, days: u64) {
    let time = SystemTime::now() - Duration::from_secs(days * 86_400);
    let seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        DiskStore::new(obj.to_path_buf()).set_last_use(entry.kind, &entry.key, seconds),
        "logical entry"
    );
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
fn gc_reports_an_unreadable_inventory_without_printing_partial_sizes() -> std::io::Result<()> {
    let dir = scratch("gc-incomplete");
    let cache = dir.join("cache");
    std::fs::create_dir_all(cache.join("obj/stats"))?;
    let r = hd(&dir, &cache, None, &["cache", "gc"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.out.is_empty(), "{}", r.out);
    assert!(r.err.contains("cannot collect compiled cache"), "{}", r.err);
    Ok(())
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
    std::fs::write(
        version.join("hd.toml"),
        "[package]\nname = \"json\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
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
    let store = DiskStore::new(obj.clone());
    assert_eq!(store.last_collection_cap(), None, "the build fits its cap");
    // One command over a smaller cap asks for a collection, which a
    // detached `hd cache gc` runs to the low-water mark (#1932); the
    // command does not wait for it. The test does, on the collector's own
    // signals: it records its cap under `obj/gc`'s lock before it
    // collects, and holds that lock until it ends. No later command runs
    // first: one would recompute the evicted entries, which the spec
    // allows within the hour (`cli.cache.obj.evict`).
    let r = hd(&pkg, &cache, Some("16K"), &["check"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    // A hang guard for a collector that never starts, not a time budget:
    // the verdict below does not depend on how fast it runs.
    let started = std::time::Instant::now();
    while store.last_collection_cap() != Some(16 * 1024) {
        assert!(
            started.elapsed() < Duration::from_mins(2),
            "no collection under the 16K cap started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store
        .wait_for_collection()
        .expect("wait for the collection");
    let left = total(&obj);
    assert!(left <= 16 * 1024, "{left} bytes left");
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
        include_str!("programs/cache_gc/a_recently_hit_entry_outlives_older_ones.hd"),
    )
    .expect("write");
    let r = hd(&pkg, &cache, None, &["check"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    // A collection under the default cap folds the index logs into pages,
    // so the index's bytes below are the ones eviction keeps.
    let r = hd(&pkg, &cache, None, &["cache", "gc"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let all = entries(&obj);
    let (used, unused): (Vec<&Entry>, Vec<&Entry>) = all.iter().partition(|e| recent(e));
    assert!(!unused.is_empty());
    // The hits are entries of the first run that are recent again.
    assert!(used.iter().any(|e| first.contains(&e.identity())));
    // The cap fits every recently used logical entry and the index, but
    // neither the ten-day-old entries nor a three-day-old filler.
    let used_bytes = used.iter().map(|e| e.bytes).sum::<u64>();
    let filler = (EntryKind::Check, Hash128(u128::MAX));
    assert!(!all.iter().any(|entry| entry.identity() == filler));
    {
        let store = DiskStore::new(obj.clone());
        store.put(
            filler.0,
            &filler.1,
            &vec![0; usize::try_from(used_bytes + index_bytes(&obj) + 8192).unwrap()],
        );
    }
    // Each kept frame also keeps its 34-byte footer row, and each pack
    // its 32-byte footer trailer.
    let packs = used.iter().map(|e| &e.pack).collect::<BTreeSet<_>>().len();
    let footers = u64::try_from(used.len() * 34 + packs * 32).unwrap();
    let kept_bytes = used_bytes + index_bytes(&obj) + footers;
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

/// One `hd check` of `file` over the cache `cache`, its work counted by
/// `HD_WORK` on one thread (`HD_JOBS=1`, as counted runs need).
fn counted_check(dir: &Path, cache: &Path, file: &str, n: usize) -> Command {
    let mut c = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", cache)
        .env_remove("HD_CACHE_MAX_SIZE")
        .env("HD_JOBS", "1")
        .env("HD_WORK", dir.join(format!("work-{n}.json")))
        .current_dir(dir)
        .args(["check", file])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c
}

/// Counter `name` of run `n`'s work report.
fn counter(dir: &Path, n: usize, name: &str) -> u64 {
    let text = std::fs::read_to_string(dir.join(format!("work-{n}.json"))).expect("work report");
    let at = text.find(&format!("\"{name}\":")).expect("counter") + name.len() + 3;
    text[at..]
        .split([',', '}'])
        .next()
        .and_then(|n| n.parse().ok())
        .expect("count")
}

/// Runs `files` together over one empty cache, each `hd check`ing its
/// file, and returns each run's `item_body_checks` and `header_types`.
fn together(dir: &Path, cache: &Path, files: &[String]) -> Vec<(u64, u64)> {
    // cost: one process per file, run together.
    let runs: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(i, f)| counted_check(dir, cache, f, i).spawn().expect("spawn hd"))
        .collect();
    // Every run must exit: one that does not within the harness timeout
    // is a deadlock, and the test fails rather than hang.
    let deadline = std::time::Instant::now() + Duration::from_mins(1);
    for mut run in runs {
        let status = loop {
            if let Some(status) = run.try_wait().expect("wait hd") {
                break status;
            }
            if std::time::Instant::now() > deadline {
                let _ = run.kill();
                panic!("an `hd check` run did not exit: a deadlock");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
    }
    (0..files.len())
        .map(|i| {
            (
                counter(dir, i, "item_body_checks"),
                counter(dir, i, "header_types"),
            )
        })
        .collect()
}

/// Each entry is computed once (#1932): N `hd check` processes of one
/// file start together on one empty cache. One checks the file and its
/// std; the others wait for those entries and check nothing, by count.
#[test]
fn processes_checking_one_program_together_compute_each_entry_once() {
    for n in [4, 16] {
        let dir = scratch(&format!("compute-once-{n}"));
        std::fs::write(dir.join("a.hd"), include_str!("programs/cache_gc/processes_checking_one_program_together_compute_each_entry_once.hd")).expect("write");
        let alone = together(&dir, &dir.join("alone"), &["a.hd".to_owned()]);
        let runs = together(&dir, &dir.join("cache"), &vec!["a.hd".to_owned(); n]);
        let sum = |pick: fn(&(u64, u64)) -> u64| runs.iter().map(pick).sum::<u64>();
        assert_eq!(sum(|r| r.0), alone[0].0, "N = {n}: {runs:?}");
        assert_eq!(sum(|r| r.1), alone[0].1, "N = {n}: {runs:?}");
    }
}

/// Runs of different programs wait for each other only on what they all
/// need (#1932): four programs that share only std check together over
/// one empty cache. Each checks its own file; std is checked once, as when
/// they run one after another.
#[test]
fn processes_checking_different_programs_together_check_std_once() {
    let dir = scratch("compute-once-programs");
    let files: Vec<String> = (0..4)
        .map(|i| {
            let file = format!("p{i}.hd");
            std::fs::write(dir.join(&file), format!("fn f{i}() -> i32:\n    {i}\n"))
                .expect("write");
            file
        })
        .collect();
    let mut apart = Vec::new();
    for file in &files {
        apart.extend(together(
            &dir,
            &dir.join("apart"),
            std::slice::from_ref(file),
        ));
    }
    let runs = together(&dir, &dir.join("together"), &files);
    assert!(runs.iter().all(|r| r.0 == 1), "{runs:?}");
    let types = |runs: &[(u64, u64)]| runs.iter().map(|r| r.1).sum::<u64>();
    assert_eq!(types(&runs), types(&apart), "{runs:?} {apart:?}");
}

/// A rejected parse must not retain its claim across other queries, the
/// next command, or a source edit (#1981). Use the reported CLI fixture,
/// four actual processes and eight cold/warm/repaired batches.
#[test]
fn rejected_parses_then_warm_checks_release_claims_across_processes() {
    const BAD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/check-fix-elif-continuations/src/lib.hd"
    ));
    const GOOD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/check-fix-elif-continuations/want/lib.input"
    ));
    rejected_checks("parse", &[("lib.hd", BAD)], GOOD, 2);
}

#[test]
fn rejected_type_checks_then_warm_checks_release_claims_across_processes() {
    const LIB: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/trait-reference-messages/src/lib.hd"
    ));
    const LEFT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/trait-reference-messages/src/left.hd"
    ));
    const RIGHT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/trait-reference-messages/src/right.hd"
    ));
    const GOOD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/conformance/cli/trait-reference-messages/valid.hd"
    ));
    rejected_checks(
        "type",
        &[("lib.hd", LIB), ("left.hd", LEFT), ("right.hd", RIGHT)],
        GOOD,
        5,
    );
}

/// Exercise claim liveness across actual cold, warm and repaired commands.
/// Diagnostics themselves remain covered by the included CLI fixtures.
fn rejected_checks(label: &str, files: &[(&str, &str)], good: &str, errors: u64) {
    for round in 0..8 {
        let dir = scratch(&format!("rejected-{label}-claim-{round}"));
        let cache = dir.join("cache");
        std::fs::write(dir.join("hd.toml"), "[package]\nname = \"rejectclaim\"\n").unwrap();
        std::fs::create_dir(dir.join("src")).unwrap();
        for &(name, source) in files {
            std::fs::write(dir.join("src").join(name), source).unwrap();
        }
        for (batch, expected) in [(0, 101), (1, 101), (2, 0)] {
            if batch == 2 {
                std::fs::write(dir.join("src/lib.hd"), good).unwrap();
            }
            // cost: four subprocesses per batch, all on one shared cache;
            // one deadline kills every unfinished child before failing.
            let mut children: Vec<_> = (0..4)
                .map(|n| {
                    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
                        .current_dir(&dir)
                        .env("HD_CACHE", &cache)
                        .env("HD_JOBS", if round < 4 { "1" } else { "2" })
                        .env_remove("HD_CACHE_MAX_SIZE")
                        .args(["check", "--format", "json"])
                        .stdout(
                            std::fs::File::create(dir.join(format!("out-{batch}-{n}"))).unwrap(),
                        )
                        .stderr(
                            std::fs::File::create(dir.join(format!("err-{batch}-{n}"))).unwrap(),
                        )
                        .spawn()
                        .unwrap()
                })
                .collect();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut statuses = [None; 4];
            while statuses.iter().any(Option::is_none) {
                for (n, child) in children.iter_mut().enumerate() {
                    if statuses[n].is_none() {
                        statuses[n] = child.try_wait().unwrap();
                    }
                }
                if std::time::Instant::now() > deadline {
                    for child in &mut children {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    panic!("rejected/warm check deadlocked in round{round}, batch{batch}");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            for (n, status) in statuses.into_iter().enumerate() {
                let out = std::fs::read_to_string(dir.join(format!("out-{batch}-{n}"))).unwrap();
                let err = std::fs::read_to_string(dir.join(format!("err-{batch}-{n}"))).unwrap();
                assert_eq!(status.unwrap().code(), Some(expected), "{out} {err}");
                assert!(!err.contains("panicked"), "{err}");
                let rows: Vec<serde_json::Value> = out
                    .lines()
                    .map(|line| serde_json::from_str(line).expect("diagnostics JSON"))
                    .collect();
                let summary = rows
                    .iter()
                    .find(|row| row["kind"] == "summary")
                    .expect("summary");
                assert_eq!(summary["errors"], if expected == 0 { 0 } else { errors });
            }
        }
    }
}
