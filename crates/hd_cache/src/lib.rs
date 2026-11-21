#![forbid(unsafe_code)]
//! `hd_cache`: the `CacheStore` interface with its memory and disk stores,
//! entry framing (`crates/hd_cache/DESIGN.md` §5) and the key rules of §5.3. Entries are
//! bytes by `(kind, key)`; every key is `H(tag, fields...)` over content,
//! never a run ID.

use hd_base::{BuildProfile, Hash128, StableHasher};

#[cfg(feature = "disk")]
pub mod disk;
pub mod flight;
pub mod store;
#[cfg(feature = "disk")]
pub use disk::{DiskStore, IndexedEntry, LockFile};
pub use flight::{FlightRun, SingleFlight};
pub use store::{
    CacheStore, DeferredStore, EntryKind, MemoryStore, OverlayStore, decode_entry, encode_entry,
};

/// The entry layout version (`docs/compiler/data-structures.md` §3.20): part of every
/// toolchain key, so a layout change misses instead of misreading.
pub const LAYOUT_VERSION: u16 = 42;

/// One file's part of a folder key: `(module, role, api_text_hash)`.
pub struct FileApi<'a> {
    pub module: &'a str,
    pub role: &'a str,
    pub api_text_hash: Hash128,
}

/// `toolchain_key`: the compiler, the target, the entry layout, the hash
/// function and any option that changes what a stage writes.
#[must_use]
pub fn toolchain_key(compiler: &str, target: &str, options: &str) -> Hash128 {
    let mut k = StableHasher::new("tc");
    k.str(compiler);
    k.str(target);
    k.u16(LAYOUT_VERSION);
    k.str("xxh3-128");
    k.str(options);
    k.finish()
}

/// The package key: the package's name from its manifest.
#[must_use]
pub fn package_key(name: &str) -> Hash128 {
    let mut k = StableHasher::new("pkg");
    k.str(name);
    k.finish()
}

/// `reads_key(F) = H("iface-reads", toolchain, package, F, [(module, role,
/// api_text_hash)], roots)`: what F's own source decides, `roots` being
/// the hash of its modules' use roots. It keys the entry that lists what
/// F's header lowering last read, and starts [`iface_key`].
#[must_use]
pub fn reads_key(
    toolchain: Hash128,
    package: Hash128,
    folder: &str,
    files: &[FileApi<'_>],
    roots: Hash128,
) -> Hash128 {
    let mut k = StableHasher::new("iface-reads");
    k.hash(toolchain);
    k.hash(package);
    k.str(folder);
    for f in files {
        k.str(f.module);
        k.str(f.role);
        k.hash(f.api_text_hash);
    }
    k.hash(roots);
    k.finish()
}

/// `iface_key(F) = H("iface", reads_key(F), [(read, answer) for each
/// read of F's header lowering])`: each module path it asked about, with
/// what the run answers for it now, the deep hash of its folder's
/// interface included when the lowering opened that interface.
#[must_use]
pub fn iface_key(reads_key: Hash128, reads: &[(&str, Hash128)]) -> Hash128 {
    let mut k = StableHasher::new("iface");
    k.hash(reads_key);
    for (d, h) in reads {
        k.str(d);
        k.hash(*h);
    }
    k.finish()
}

/// `check_key(m) = H("check", toolchain, package, m, role, token_hash(m),
/// [(folder, deep_hash) for closure(folder(m))])`. The token hash leaves
/// out plain comments and blank lines, so the entry stores its positions
/// by code line, and `parse_key` proves that the file's current bytes
/// parse.
#[must_use]
pub fn check_key(
    toolchain: Hash128,
    package: Hash128,
    module: &str,
    role: &str,
    token_hash: Hash128,
    closure: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("check");
    k.hash(toolchain);
    k.hash(package);
    k.str(module);
    k.str(role);
    k.hash(token_hash);
    for (c, h) in closure {
        k.str(c);
        k.hash(*h);
    }
    k.finish()
}

/// `parse_key(f) = H("parse", toolchain, source_hash(f))`: an empty entry
/// whose presence says that these bytes parse without a diagnostic. A
/// `check` hit needs it, since its key ignores comments and blank lines,
/// which can still detach a documentation comment.
#[must_use]
pub fn parse_key(toolchain: Hash128, source_hash: Hash128) -> Hash128 {
    let mut k = StableHasher::new("parse");
    k.hash(toolchain);
    k.hash(source_hash);
    k.finish()
}

/// `depfiles_key(p) = H("depfiles", toolchain, identity(p))`: the skims of
/// a package whose files cannot change under one key, read once per
/// machine instead of skimming its files on every run (§5.5). The
/// embedded std is one: its texts are part of the toolchain key.
#[must_use]
pub fn depfiles_key(toolchain: Hash128, identity: &str) -> Hash128 {
    let mut k = StableHasher::new("depfiles");
    k.hash(toolchain);
    k.str(identity);
    k.finish()
}

/// `hdr_key(F) = H("hdr", toolchain, package, F, own,
/// [(folder, deep_hash) for every folder of closure(F)])`: a part key of
/// the `graph` entry (stage-B header findings and F's use diagnostics,
/// one entry per folder here). `own` is the driver's hash of F's deep and
/// heads hashes, its `reads_key` and its closure's private names.
/// The deep hashes cover the interfaces, impl heads and `arg_impls`, which
/// fix the impl universe of the solving context. The driver adds the
/// source hash of each user tuple-template module as a closure entry, as
/// the template's body fixes the member steps its goals read.
#[must_use]
pub fn hdr_key(
    toolchain: Hash128,
    package: Hash128,
    folder: &str,
    own: Hash128,
    closure: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("hdr");
    k.hash(toolchain);
    k.hash(package);
    k.str(folder);
    k.hash(own);
    for (c, h) in closure {
        k.str(c);
        k.hash(*h);
    }
    k.finish()
}

/// `init_key(F) = H("init", toolchain, F, [(module, init summary hash)])`,
/// the modules sorted by path: a part key of the `graph` entry
/// (`InitOrder`'s statement order, one entry per folder here).
#[must_use]
pub fn init_key(toolchain: Hash128, folder: &str, modules: &[(&str, Hash128)]) -> Hash128 {
    let mut k = StableHasher::new("init");
    k.hash(toolchain);
    k.str(folder);
    for (m, h) in modules {
        k.str(m);
        k.hash(*h);
    }
    k.finish()
}

/// `prog_key = H("prog", toolchain, pipeline, profile, entry, [(module,
/// content hash)])` (`crates/hd_mono/DESIGN.md` §11.3).
#[must_use]
pub fn prog_key(
    toolchain: Hash128,
    pipeline: Hash128,
    profile: BuildProfile,
    entry: &str,
    modules: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("prog");
    k.hash(toolchain);
    k.hash(pipeline);
    k.str(profile.name());
    k.str(entry);
    for (m, h) in modules {
        k.str(m);
        k.hash(*h);
    }
    k.finish()
}

thread_local! {
    /// How many [`task`] frames this thread is inside.
    static TASK_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `f` as one scheduler task on this thread. A work-stealing pool
/// runs a task inside another one's join on the same thread, while the
/// outer task is suspended; a claim records the depth it was taken at, so
/// the disk store tells a suspended task's claim from one this task left
/// behind (DESIGN.md §5.4, "Why claims cannot deadlock"). Cost: two
/// thread-local writes per task.
pub fn task<R>(f: impl FnOnce() -> R) -> R {
    struct Leave;
    impl Drop for Leave {
        fn drop(&mut self) {
            TASK_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }
    TASK_DEPTH.with(|d| d.set(d.get() + 1));
    let _leave = Leave;
    f()
}

/// The number of [`task`] frames on this thread.
#[must_use]
pub fn task_depth() -> u32 {
    TASK_DEPTH.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_depth_counts_nested_tasks_and_unwinds() {
        assert_eq!(task_depth(), 0);
        task(|| {
            assert_eq!(task_depth(), 1);
            task(|| assert_eq!(task_depth(), 2));
            assert_eq!(task_depth(), 1);
            let unwound = std::panic::catch_unwind(|| task(|| panic!("task panics")));
            assert!(unwound.is_err());
            assert_eq!(task_depth(), 1);
        });
        assert_eq!(task_depth(), 0);
    }

    #[test]
    fn keys_change_with_each_field() {
        let tc = toolchain_key("hd 0", "wasm32", "");
        let dev = Hash128(10);
        let (checked, release) = (BuildProfile::Checked, BuildProfile::Release);
        let modules = [("pkg.main", Hash128(1))];
        let p = prog_key(tc, dev, checked, "pkg.main", &modules);
        assert_ne!(p, prog_key(tc, dev, release, "pkg.main", &modules));
        let c = check_key(
            tc,
            Hash128(9),
            "pkg.main",
            "lib",
            Hash128(5),
            &[("pkg", Hash128(6))],
        );
        assert_ne!(
            c,
            check_key(
                tc,
                Hash128(9),
                "pkg.main",
                "lib",
                Hash128(5),
                &[("pkg", Hash128(7))]
            )
        );
        assert_ne!(package_key("a"), package_key("b"));
        let files = [FileApi {
            module: "pkg.main",
            role: "lib",
            api_text_hash: Hash128(1),
        }];
        let r = reads_key(tc, Hash128(9), "pkg", &files, Hash128(2));
        assert_ne!(r, reads_key(tc, Hash128(9), "pkg", &files, Hash128(3)));
        assert_ne!(r, reads_key(tc, Hash128(9), "app", &files, Hash128(2)));
        let f = iface_key(r, &[("pkg.util", Hash128(4))]);
        assert_ne!(f, iface_key(r, &[("pkg.util", Hash128(5))]));
        assert_ne!(f, iface_key(r, &[]));
        assert_ne!(f, r);
        let i = init_key(tc, "pkg", &[("pkg.a", Hash128(1))]);
        assert_ne!(i, init_key(tc, "pkg", &[("pkg.a", Hash128(2))]));
        assert_ne!(i, init_key(tc, "pkg", &[("pkg.b", Hash128(1))]));
        assert_ne!(i, init_key(tc, "app", &[("pkg.a", Hash128(1))]));
    }
}
