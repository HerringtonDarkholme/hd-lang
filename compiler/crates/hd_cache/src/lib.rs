//! `hd_cache`: the in-memory `CacheStore` (cache.md §5) and the key rules of
//! §5.3. Entries are bytes by `(kind, key)`; every key is `H(tag, fields...)`
//! over content, never a run ID.

use std::collections::HashMap;
use std::sync::Arc;

use hd_base::Hash128;
use hd_iface::KeyHasher;

pub mod store;
pub use store::{CacheStore, DiskStore, EntryKind, MemoryStore};

/// The in-memory store: entries by (kind, key), bytes only.
#[derive(Default)]
pub struct MemStore {
    entries: HashMap<(&'static str, u128), Arc<[u8]>>,
}

impl MemStore {
    #[must_use]
    pub fn get(&self, kind: &'static str, key: Hash128) -> Option<Arc<[u8]>> {
        self.entries.get(&(kind, key.0)).cloned()
    }
    pub fn put(&mut self, kind: &'static str, key: Hash128, bytes: Vec<u8>) -> Arc<[u8]> {
        let bytes: Arc<[u8]> = bytes.into();
        self.entries.insert((kind, key.0), Arc::clone(&bytes));
        bytes
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One file's part of a folder key: `(module, role, api_text_hash)`.
pub struct FileApi<'a> {
    pub module: &'a str,
    pub role: &'a str,
    pub api_text_hash: Hash128,
}

/// `toolchain_key`: the compiler, the target and any option that changes
/// what a stage writes.
#[must_use]
pub fn toolchain_key(compiler: &str, target: &str, options: &str) -> Hash128 {
    KeyHasher::new("tc")
        .str(compiler)
        .str(target)
        .str(options)
        .finish()
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
    let mut k = KeyHasher::new("iface")
        .hash(toolchain)
        .hash(package)
        .str(folder);
    for f in files {
        k = k.str(f.module).str(f.role).hash(f.api_text_hash);
    }
    for (d, h) in reach {
        k = k.str(d).hash(*h);
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
    let mut k = KeyHasher::new("check")
        .hash(toolchain)
        .hash(package)
        .str(module)
        .str(role)
        .hash(source_hash);
    for (c, h) in closure {
        k = k.str(c).hash(*h);
    }
    k.finish()
}

/// `prog_key = H("prog", toolchain, pipeline, entry, [(module, TIR content
/// hash)])` (codegen.md §11.3).
#[must_use]
pub fn prog_key(
    toolchain: Hash128,
    pipeline: &str,
    entry: &str,
    modules: &[(&str, Hash128)],
) -> Hash128 {
    let mut k = KeyHasher::new("prog")
        .hash(toolchain)
        .str(pipeline)
        .str(entry);
    for (m, h) in modules {
        k = k.str(m).hash(*h);
    }
    k.finish()
}

/// `code_key = H("code", pipeline, instance key, TIR hash, callee
/// representation summaries)` (codegen.md §13.8; walking skeleton, SK-3).
#[must_use]
pub fn code_key(pipeline: &str, instance: Hash128, tir: Hash128, callee_reps: Hash128) -> Hash128 {
    KeyHasher::new("code")
        .str(pipeline)
        .hash(instance)
        .hash(tir)
        .hash(callee_reps)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_change_with_each_field() {
        let tc = toolchain_key("hd 0", "wasm32-gc", "");
        let a = code_key("dev", Hash128(1), Hash128(2), Hash128(3));
        assert_ne!(a, code_key("dev", Hash128(1), Hash128(2), Hash128(4)));
        assert_ne!(a, code_key("opt", Hash128(1), Hash128(2), Hash128(3)));
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
    }

    #[test]
    fn store_round_trip() {
        let mut s = MemStore::default();
        s.put("check", Hash128(1), vec![1, 2, 3]);
        assert_eq!(
            s.get("check", Hash128(1)).as_deref(),
            Some(&[1u8, 2, 3][..])
        );
        assert!(s.get("iface", Hash128(1)).is_none());
    }
}
