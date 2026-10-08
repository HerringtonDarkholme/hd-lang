#![forbid(unsafe_code)]
//! `hd_cache`: the `CacheStore` interface with its memory and disk stores,
//! entry framing (cache.md §5) and the key rules of §5.3. Entries are
//! bytes by `(kind, key)`; every key is `H(tag, fields...)` over content,
//! never a run ID.

use hd_base::{Hash128, StableHasher};

pub mod store;
pub use store::{CacheStore, DiskStore, EntryKind, MemoryStore, decode_entry, encode_entry};

/// The entry layout version (data-structures.md §3.20): part of every
/// toolchain key, so a layout change misses instead of misreading.
pub const LAYOUT_VERSION: u16 = 3;

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

/// `iface_key(F) = H("iface", toolchain, package, F, [(module, role,
/// api_text_hash)], [(folder, deep_hash) for every folder F's uses reach])`.
#[must_use]
pub fn iface_key(
    toolchain: Hash128,
    package: Hash128,
    folder: &str,
    files: &[FileApi<'_>],
    reach: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("iface");
    k.hash(toolchain);
    k.hash(package);
    k.str(folder);
    for f in files {
        k.str(f.module);
        k.str(f.role);
        k.hash(f.api_text_hash);
    }
    for (d, h) in reach {
        k.str(d);
        k.hash(*h);
    }
    k.finish()
}

/// `check_key(m) = H("check", toolchain, package, m, role, source_hash(m),
/// [(folder, deep_hash) for closure(folder(m))])`.
#[must_use]
pub fn check_key(
    toolchain: Hash128,
    package: Hash128,
    module: &str,
    role: &str,
    source_hash: Hash128,
    closure: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("check");
    k.hash(toolchain);
    k.hash(package);
    k.str(module);
    k.str(role);
    k.hash(source_hash);
    for (c, h) in closure {
        k.str(c);
        k.hash(*h);
    }
    k.finish()
}

/// `prog_key = H("prog", toolchain, pipeline, entry, [(module, TIR content
/// hash)])` (codegen.md §11.3).
#[must_use]
pub fn prog_key(
    toolchain: Hash128,
    pipeline: Hash128,
    entry: &str,
    modules: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = StableHasher::new("prog");
    k.hash(toolchain);
    k.hash(pipeline);
    k.str(entry);
    for (m, h) in modules {
        k.str(m);
        k.hash(*h);
    }
    k.finish()
}

/// `code_key = H("code", pipeline, instance key, TIR hash, callee
/// representation summaries)` (codegen.md §13.8; walking skeleton, SK-3).
#[must_use]
pub fn code_key(
    pipeline: Hash128,
    instance: Hash128,
    tir: Hash128,
    callee_reps: Hash128,
) -> Hash128 {
    let mut k = StableHasher::new("code");
    k.hash(pipeline);
    k.hash(instance);
    k.hash(tir);
    k.hash(callee_reps);
    k.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_change_with_each_field() {
        let tc = toolchain_key("hd 0", "wasm32-gc", "");
        let (dev, opt) = (Hash128(10), Hash128(11));
        let a = code_key(dev, Hash128(1), Hash128(2), Hash128(3));
        assert_ne!(a, code_key(dev, Hash128(1), Hash128(2), Hash128(4)));
        assert_ne!(a, code_key(opt, Hash128(1), Hash128(2), Hash128(3)));
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
    }
}
