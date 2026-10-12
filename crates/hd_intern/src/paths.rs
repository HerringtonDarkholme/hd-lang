//! The stable-path trie (data-structures.md §3.2) and the sharded interner
//! (§3.3).
//!
//! Every package, module segment and item segment is one trie node, and a
//! `DefId` is a `PathId` whose node is an item. Equal paths are the same
//! node. Each node's 128-bit stable hash is computed once, at intern time:
//! `hash(node) = H(hash(parent) ‖ kind ‖ segment bytes)`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;

use hd_base::{AppendVec, DefId, Hash128, ModuleId, PathId, StableHasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PathKind {
    Package,
    Module,
    Item,
    Member,
    Variant,
    Impl,
    Hidden,
}

impl PathKind {
    pub const ALL: [PathKind; 7] = [
        PathKind::Package,
        PathKind::Module,
        PathKind::Item,
        PathKind::Member,
        PathKind::Variant,
        PathKind::Impl,
        PathKind::Hidden,
    ];
    #[must_use]
    pub fn from_u8(v: u8) -> Option<PathKind> {
        Self::ALL.get(v as usize).copied()
    }
}

/// An impl segment: its head's normalized text hash plus an ordinal among
/// identical heads in the module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImplSeg {
    pub head: Hash128,
    pub ordinal: u16,
}

const SHARDS: usize = 16;

type PathIndex = Mutex<HashMap<(u32, PathKind, Box<str>), PathId>>;

/// The trie. Columns are append-only, so a reader indexes without a lock;
/// the shards guard only the dedup index.
pub struct PathTable {
    parent: AppendVec<PathId>,
    seg: AppendVec<Box<str>>,
    kind: AppendVec<PathKind>,
    hash: AppendVec<Hash128>,
    def_module: AppendVec<ModuleId>,
    append: Mutex<()>,
    index: [PathIndex; SHARDS],
}

impl Default for PathTable {
    fn default() -> Self {
        Self {
            parent: AppendVec::new(),
            seg: AppendVec::new(),
            kind: AppendVec::new(),
            hash: AppendVec::new(),
            def_module: AppendVec::new(),
            append: Mutex::new(()),
            index: std::array::from_fn(|_| Mutex::new(HashMap::new())),
        }
    }
}

impl PathTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns one node under `parent` (`PathId::NONE` for a package root).
    pub fn intern(&self, parent: PathId, kind: PathKind, seg: &str) -> PathId {
        let mut h = StableHasher::new("path");
        h.hash(parent.get().map_or(Hash128(0), |p| self.hash(p)));
        h.u8(kind as u8);
        h.str(seg);
        let hash = h.finish();
        let shard = usize::try_from(hash.0 % SHARDS as u128).expect("shard");
        let key = (parent.raw(), kind, Box::<str>::from(seg));
        let mut idx = self.index[shard].lock().expect("path shard");
        if let Some(&id) = idx.get(&key) {
            return id;
        }
        let _g = self.append.lock().expect("path append");
        let row = self.parent.push(parent);
        self.seg.push(seg.into());
        self.kind.push(kind);
        self.hash.push(hash);
        self.def_module.push(ModuleId::NONE);
        let id = PathId::from_raw(row);
        idx.insert(key, id);
        id
    }

    /// Interns a dotted module path under a package root, then an item.
    pub fn item(&self, package: &str, module: &str, item: &str) -> DefId {
        let mut p = self.intern(PathId::NONE, PathKind::Package, package);
        for seg in module.split('.') {
            p = self.intern(p, PathKind::Module, seg);
        }
        DefId::from_raw(self.intern(p, PathKind::Item, item).raw())
    }

    #[must_use]
    pub fn parent(&self, id: PathId) -> PathId {
        self.parent[id.raw()]
    }
    #[must_use]
    pub fn kind(&self, id: PathId) -> PathKind {
        self.kind[id.raw()]
    }
    #[must_use]
    pub fn hash(&self, id: PathId) -> Hash128 {
        self.hash[id.raw()]
    }
    #[must_use]
    pub fn segment(&self, id: PathId) -> &str {
        &self.seg[id.raw()]
    }
    #[must_use]
    pub fn def_module(&self, id: PathId) -> ModuleId {
        self.def_module[id.raw()]
    }
    #[must_use]
    pub fn len(&self) -> u32 {
        self.parent.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// Prints the path: `pkg/mod.sub/Item.member`.
    pub fn print(&self, id: PathId, out: &mut String) {
        let mut chain = Vec::new();
        let mut p = id;
        while let Some(q) = p.get() {
            chain.push(q);
            p = self.parent(q);
        }
        let mut prev: Option<PathKind> = None;
        for q in chain.into_iter().rev() {
            let k = self.kind(q);
            match (prev, k) {
                (None, _) => {}
                (Some(PathKind::Package), _) => out.push('/'),
                (Some(PathKind::Module), PathKind::Item | PathKind::Impl | PathKind::Hidden) => {
                    out.push('/');
                }
                _ => out.push('.'),
            }
            let _ = write!(out, "{}", self.segment(q));
            prev = Some(k);
        }
    }

    #[must_use]
    pub fn display(&self, id: PathId) -> StablePath<'_> {
        StablePath { table: self, id }
    }
}

/// A view that prints a stable path.
pub struct StablePath<'a> {
    pub table: &'a PathTable,
    pub id: PathId,
}

impl core::fmt::Display for StablePath<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut s = String::new();
        self.table.print(self.id, &mut s);
        f.write_str(&s)
    }
}

/// The sharded string interner of §3.3: lookups lock one of 16 shards,
/// reads of an existing symbol take no lock.
pub struct ShardedInterner {
    text: AppendVec<Box<str>>,
    append: Mutex<()>,
    index: [Mutex<HashMap<Box<str>, u32>>; SHARDS],
}

impl Default for ShardedInterner {
    fn default() -> Self {
        Self {
            text: AppendVec::new(),
            append: Mutex::new(()),
            index: std::array::from_fn(|_| Mutex::new(HashMap::new())),
        }
    }
}

impl ShardedInterner {
    pub fn intern(&self, s: &str) -> hd_base::Symbol {
        let shard =
            usize::try_from(hd_base::hash128(s.as_bytes()).0 % SHARDS as u128).expect("shard");
        let mut idx = self.index[shard].lock().expect("symbol shard");
        if let Some(&id) = idx.get(s) {
            return hd_base::Symbol::from_raw(id);
        }
        let _g = self.append.lock().expect("symbol append");
        let id = self.text.push(s.into());
        idx.insert(s.into(), id);
        hd_base::Symbol::from_raw(id)
    }
    #[must_use]
    pub fn resolve(&self, s: hd_base::Symbol) -> &str {
        &self.text[s.raw()]
    }
    #[must_use]
    pub fn len(&self) -> u32 {
        self.text.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{PathKind, PathTable, ShardedInterner};
    use hd_base::PathId;

    #[test]
    fn equal_paths_are_one_node_with_a_stable_hash() {
        let t = PathTable::new();
        let a = t.item("std", "collections.list", "List");
        let b = t.item("std", "collections.list", "List");
        assert_eq!(a, b);
        let c = t.item("std", "collections.map", "List");
        assert_ne!(a, c);
        let p = PathId::from_raw(a.raw());
        assert_eq!(t.display(p).to_string(), "std/collections.list/List");
        let t2 = PathTable::new();
        t2.intern(PathId::NONE, PathKind::Package, "other");
        let a2 = t2.item("std", "collections.list", "List");
        assert_eq!(
            t.hash(p),
            t2.hash(PathId::from_raw(a2.raw())),
            "the hash is content, not ID"
        );
    }

    #[test]
    fn symbols_dedup() {
        let i = ShardedInterner::default();
        let a = i.intern("x");
        assert_eq!(a, i.intern("x"));
        assert_eq!(i.resolve(a), "x");
    }
}
