//! The stable-path trie (`docs/compiler/data-structures.md` §3.2) and the sharded interner
//! (§3.3).
//!
//! Every package, module segment and item segment is one trie node, and a
//! `DefId` is a `PathId` whose node is an item. Equal paths are the same
//! node. Each node's 128-bit stable hash is computed once, at intern time:
//! `hash(node) = H(hash(parent) ‖ kind ‖ segment bytes)`.

use hd_base::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;

use hd_base::{AppendVec, DefId, Hash128, PathId, StableHasher, Symbol};

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
    /// The text of a string const generic argument, as a root path: its
    /// interned path gives the value a stable hash and wire form.
    Text,
}

impl PathKind {
    pub const ALL: [PathKind; 8] = [
        PathKind::Package,
        PathKind::Module,
        PathKind::Item,
        PathKind::Member,
        PathKind::Variant,
        PathKind::Impl,
        PathKind::Hidden,
        PathKind::Text,
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

type PathIndex = Mutex<HashMap<(PathId, PathKind, Symbol), PathId>>;

/// The trie. Columns are append-only, so a reader indexes without a lock;
/// the shards guard only the dedup index.
pub struct PathTable {
    parent: AppendVec<PathId>,
    seg: AppendVec<Symbol>,
    symbols: ShardedInterner,
    kind: AppendVec<PathKind>,
    hash: AppendVec<Hash128>,
    def_module: AppendVec<PathId>,
    append: Mutex<()>,
    /// The names declared type and row parameters are written with, by
    /// owner and position, for messages (the trie holds no parameter nodes).
    param_names: Mutex<HashMap<(u32, u16), Box<str>>>,
    index: [PathIndex; SHARDS],
}

impl Default for PathTable {
    fn default() -> Self {
        Self {
            parent: AppendVec::new(),
            seg: AppendVec::new(),
            symbols: ShardedInterner::default(),
            kind: AppendVec::new(),
            hash: AppendVec::new(),
            def_module: AppendVec::new(),
            append: Mutex::new(()),
            param_names: Mutex::new(HashMap::default()),
            index: std::array::from_fn(|_| Mutex::new(HashMap::default())),
        }
    }
}

impl PathTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The shard depends only on the compact key, not the path's stable hash.
    fn shard(parent: PathId, kind: PathKind, symbol: Symbol) -> usize {
        (parent.raw() as usize)
            .wrapping_mul(31)
            .wrapping_add((kind as usize).wrapping_mul(17))
            .wrapping_add(symbol.raw() as usize)
            % SHARDS
    }

    /// Finds an existing node without adding a path or a segment symbol.
    #[must_use]
    pub fn find(&self, parent: PathId, kind: PathKind, seg: &str) -> Option<PathId> {
        let symbol = self.symbols.find(seg)?;
        self.index[Self::shard(parent, kind, symbol)]
            .lock()
            .expect("path shard")
            .get(&(parent, kind, symbol))
            .copied()
    }

    /// Interns one node under `parent` (`PathId::NONE` for a package root).
    pub fn intern(&self, parent: PathId, kind: PathKind, seg: &str) -> PathId {
        // No symbol lock remains held when acquiring a path shard.
        let symbol = self.symbols.intern(seg);
        let key = (parent, kind, symbol);
        let mut idx = self.index[Self::shard(parent, kind, symbol)]
            .lock()
            .expect("path shard");
        if let Some(&id) = idx.get(&key) {
            return id;
        }
        let mut h = StableHasher::new("path");
        h.hash(parent.get().map_or(Hash128(0), |p| self.hash(p)));
        h.u8(kind as u8);
        h.str(seg);
        let hash = h.finish();
        let _g = self.append.lock().expect("path append");
        let row = self.parent.push(parent);
        let id = PathId::from_raw(row);
        self.seg.push(symbol);
        self.kind.push(kind);
        self.hash.push(hash);
        self.def_module
            .push(if matches!(kind, PathKind::Package | PathKind::Module) {
                id
            } else {
                self.def_module(parent)
            });
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

    /// Records that parameter `index` of `owner` is written `name`.
    pub fn set_param_name(&self, owner: PathId, index: u16, name: &str) {
        self.param_names
            .lock()
            .expect("param names")
            .insert((owner.raw(), index), name.into());
    }

    /// The name parameter `index` of `owner` is written with, if recorded.
    #[must_use]
    pub fn param_name(&self, owner: PathId, index: u16) -> Option<String> {
        let names = self.param_names.lock().expect("param names");
        names.get(&(owner.raw(), index)).map(ToString::to_string)
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
        self.symbols.resolve(self.seg[id.raw()])
    }
    #[must_use]
    pub fn def_module(&self, id: PathId) -> PathId {
        id.get()
            .map_or(PathId::NONE, |id| self.def_module[id.raw()])
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
            index: std::array::from_fn(|_| Mutex::new(HashMap::default())),
        }
    }
}

impl ShardedInterner {
    fn shard(s: &str) -> usize {
        usize::try_from(hd_base::hash128(s.as_bytes()).0 % SHARDS as u128).expect("shard")
    }

    /// Finds an existing symbol without inserting text on a miss.
    #[must_use]
    pub fn find(&self, s: &str) -> Option<Symbol> {
        self.index[Self::shard(s)]
            .lock()
            .expect("symbol shard")
            .get(s)
            .copied()
            .map(Symbol::from_raw)
    }

    pub fn intern(&self, s: &str) -> hd_base::Symbol {
        let shard = Self::shard(s);
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
    fn finding_paths_never_inserts_paths_or_symbols() {
        let t = PathTable::new();
        let package = t.intern(PathId::NONE, PathKind::Package, "app");
        let module = t.intern(package, PathKind::Module, "data");
        let item = t.intern(module, PathKind::Item, "Record");
        let count = (t.len(), t.symbols.len());
        assert_eq!(t.find(module, PathKind::Item, "Record"), Some(item));
        assert_eq!(t.find(module, PathKind::Item, "unknown"), None);
        assert_eq!(t.find(module, PathKind::Member, "Record"), None);
        assert_eq!(t.find(package, PathKind::Item, "Record"), None);
        assert_eq!((t.len(), t.symbols.len()), count);
        assert_eq!(t.intern(module, PathKind::Item, "Record"), item);
        assert_eq!((t.len(), t.symbols.len()), count);
    }

    #[test]
    fn declaring_module_is_the_nearest_module_or_package_node() {
        let t = PathTable::new();
        let package = t.intern(PathId::NONE, PathKind::Package, "app");
        let module = t.intern(package, PathKind::Module, "data");
        assert_eq!(t.def_module(PathId::NONE), PathId::NONE);
        assert_eq!(t.def_module(package), package);
        assert_eq!(t.def_module(module), module);
        let root_item = t.intern(package, PathKind::Item, "root");
        assert_eq!(t.def_module(root_item), package);
        let mut parent = module;
        for kind in [
            PathKind::Item,
            PathKind::Impl,
            PathKind::Hidden,
            PathKind::Variant,
            PathKind::Member,
        ] {
            parent = t.intern(parent, kind, "nested");
            assert_eq!(t.def_module(parent), module);
        }
        let nested_module = t.intern(module, PathKind::Module, "child");
        let nested_item = t.intern(nested_module, PathKind::Item, "Record");
        assert_eq!(t.def_module(nested_item), nested_module);
    }

    #[test]
    fn symbols_dedup() {
        let i = ShardedInterner::default();
        let a = i.intern("x");
        assert_eq!(a, i.intern("x"));
        assert_eq!(i.resolve(a), "x");
    }
}
