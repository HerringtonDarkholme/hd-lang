//! The declarations a stored unit's key holds (#2423,
//! `crates/hd_wasm/DESIGN.md` §23.41): a unit's emission reads the
//! program's declarations its code names (a callee's signature, a type's
//! fields, an impl's head), so its key holds those, each by its
//! fingerprint, not one hash of every declaration. Adding a function no
//! unit names then re-emits no unit, as rustc keys a query on its inputs'
//! fingerprints rather than on the whole crate.
//!
//! The graph: one node per non-std item of the program, its fingerprint
//! the hash of its record ([`hd_resolve::item_fingerprint`]), and one per
//! distinct type a record names, its fingerprint the type's content hash.
//! A record names each of its types by that hash, and a type's row names
//! its parts by theirs, so each distinct type is written once per link
//! however many records share it: a chain of n aliases, each naming the
//! one before, costs O(n) rows, not O(n²) (#2423). An edge goes to every
//! declaration and type a record or a row names. A declaration an impl
//! names also reaches that impl, since a type's drop hook, its associated
//! types and its trait impls live in impls that name it: an edge from it
//! to the impl when the record names it, and through reverse nodes, one
//! per type, when a type does (from each declaration a row names to the
//! row's reverse node, from a part's to its whole's, from an impl's
//! types' to the impl). A reverse node holds no content, so one that
//! reaches no impl adds nothing to a key. Std declarations are the
//! toolchain's, in every key already. A node's deep hash covers
//! everything it reaches: its strongly connected component's members and
//! the deep hashes of the components it reaches, one pass of Tarjan's
//! algorithm over the graph (each component closes after every component
//! it reaches), as Go's and Bazel's action keys hash their inputs'
//! transitive digests. A unit's part is then one probe per declaration it
//! names. Cost: one record encoding per non-std item, one row per
//! distinct type, one visit per node and edge, once per link.

use std::cell::{Cell, RefCell};

use hd_base::wire::Writer;
use hd_base::{DefId, Hash128, HashMap, PathId, SliceSort as _, StableHasher};
use hd_resolve::{ItemData, Names};
use hd_types::wire::TableWriter;
use hd_types::{InternPool, Ty, TyData};

/// The deep hash of each of the program's non-std declarations.
pub(crate) struct Decls {
    /// By declaration: its component's deep hash; `None` for one that
    /// reaches a record with no wire form, whose units are not stored.
    deep: HashMap<DefId, Option<Hash128>>,
    /// Whether the graph holds every item: false past `u32::MAX` nodes,
    /// which have no number; then no unit is stored.
    whole: bool,
    /// The type rows the fingerprints wrote: the graph's encoding work.
    pub(crate) rows: usize,
}

/// One node of the graph while it is built: `key` names it among its
/// component's members (an item's path hash; none for a type, whose
/// fingerprint is its content). A reverse node carries no content.
struct Node {
    key: Hash128,
    fingerprint: Option<Hash128>,
    edges: Vec<u32>,
    reverse: bool,
}

/// The deep hash of a component of reverse nodes that reaches no impl:
/// it adds nothing to a key, so a type no impl names leaves the keys of
/// the declarations it names as they were.
const NOTHING: Hash128 = Hash128(0);

/// One distinct type a record names: its content hash, the paths its row
/// names, and its parts, which its row names by their hashes.
struct TyRow {
    hash: Option<Hash128>,
    paths: Vec<PathId>,
    parts: Vec<Ty>,
}

/// Each distinct type's content hash, written once: its row, with its
/// parts by their hashes. Interned types share their parts, so this is a
/// hash over the pool's DAG, one row per node.
struct TyHashes<'n, 'p> {
    names: &'n Names<'p>,
    at: RefCell<HashMap<Ty, u32>>,
    rows: RefCell<Vec<TyRow>>,
    /// The type rows written: one per type, plus any a part with no hash
    /// writes again.
    work: Cell<usize>,
}

impl TyHashes<'_, '_> {
    fn seen(&self, t: Ty) -> bool {
        self.at.borrow().contains_key(&t)
    }

    /// The hash of `t` when its row is written already.
    fn known(&self, t: Ty) -> Option<Hash128> {
        let at = *self.at.borrow().get(&t)?;
        self.rows.borrow().get(at as usize)?.hash
    }

    /// The content hash of `t`, its parts' first, each once, with an
    /// explicit stack: a type nested 20,000 deep hashes without native
    /// recursion. `None` for a type with no wire form.
    fn hash(&self, t: Ty) -> Option<Hash128> {
        if self.seen(t) {
            return self.known(t);
        }
        let pool = self.names.pool;
        let (mut defs, mut parts) = (Vec::new(), Vec::new());
        let mut stack = vec![(t, false)];
        while let Some((u, expanded)) = stack.pop() {
            if self.seen(u) {
                continue;
            }
            if !expanded {
                stack.push((u, true));
                parts.clear();
                hd_resolve::ty_parts(pool, u, &mut defs, &mut parts);
                stack.extend(parts.iter().map(|&p| (p, false)));
                continue;
            }
            let row = self.row(u);
            let n = self.rows.borrow().len();
            // ruled out: a failure. The rows are one per distinct type,
            // which `Decls::new` counts against `u32::MAX` nodes.
            let Ok(n) = u32::try_from(n) else { return None };
            self.rows.borrow_mut().push(row);
            self.at.borrow_mut().insert(u, n);
        }
        self.known(t)
    }

    /// The row of `u`, whose parts are hashed already.
    fn row(&self, u: Ty) -> TyRow {
        let memo = |p: Ty| self.known(p);
        let names = self.names;
        let mut t = TableWriter::hashing(names.pool, names.paths, names.syms, &memo);
        let at = t.ty(u);
        self.work.set(self.work.get() + t.ty_rows());
        let hash = at.ok().map(|at| {
            let mut w = Writer::default();
            t.write(&mut w);
            w.u32(at);
            let mut h = StableHasher::new("decl-type-v1");
            h.bytes(&w.bytes);
            h.finish()
        });
        TyRow {
            hash,
            paths: t.paths().collect(),
            parts: t.hashed_tys().to_vec(),
        }
    }
}

/// The node numbers the graph has room for: `None` past `u32::MAX`.
fn number(n: usize) -> Option<u32> {
    u32::try_from(n).ok()
}

impl Decls {
    /// The graph of `items`, each a non-std item of the program.
    /// Cost: one record encoding per item, one row per distinct type its
    /// records name, one visit per node and edge.
    pub(crate) fn new<'i>(
        names: &Names<'_>,
        items: impl Iterator<Item = &'i hd_resolve::Item>,
    ) -> Self {
        Self::graph(names, items).unwrap_or(Decls {
            deep: HashMap::default(),
            whole: false,
            rows: 0,
        })
    }

    fn graph<'i>(
        names: &Names<'_>,
        items: impl Iterator<Item = &'i hd_resolve::Item>,
    ) -> Option<Self> {
        let tys = TyHashes {
            names,
            at: RefCell::default(),
            rows: RefCell::default(),
            work: Cell::new(0),
        };
        let memo = |t: Ty| tys.hash(t);
        let mut at: HashMap<DefId, u32> = HashMap::default();
        let mut nodes = Vec::new();
        let mut mentions = Vec::new();
        let mut rows = 0;
        for item in items {
            at.insert(item.def, number(nodes.len())?);
            // A record with no wire form has no fingerprint and names nothing.
            let f = hd_resolve::item_fingerprint(names, item, &memo).ok();
            rows += f.as_ref().map_or(0, |f| f.rows);
            nodes.push(Node {
                key: names.path_hash(item.def),
                fingerprint: f.as_ref().map(|f| f.hash),
                edges: Vec::new(),
                reverse: false,
            });
            let (paths, types) = f.map_or_else(Default::default, |f| (f.paths, f.tys));
            mentions.push((paths, types, matches!(item.data, ItemData::Impl { .. })));
        }
        let ty_at = tys.at.into_inner();
        let ty_rows = tys.rows.into_inner();
        rows += tys.work.into_inner();
        // Type `i` is node `base + i`, its reverse node `back + i`.
        let base = number(nodes.len())?;
        let back = number(nodes.len() + ty_rows.len())?;
        number(nodes.len() + 2 * ty_rows.len())?;
        let ty_node = |t: &Ty| ty_at.get(t).copied();
        let def_node = |p: &PathId| at.get(&DefId::from_raw(p.raw())).copied();
        for row in &ty_rows {
            let parts = row.parts.iter().filter_map(ty_node).map(|p| base + p);
            let mut edges: Vec<u32> = parts.collect();
            edges.extend(row.paths.iter().filter_map(def_node));
            nodes.push(Node {
                key: Hash128::default(),
                fingerprint: row.hash,
                edges,
                reverse: false,
            });
        }
        nodes.extend(ty_rows.iter().map(|_| Node {
            key: Hash128::default(),
            fingerprint: None,
            edges: Vec::new(),
            reverse: true,
        }));
        for (row, i) in ty_rows.iter().zip(back..) {
            for m in row.paths.iter().filter_map(def_node) {
                nodes[m as usize].edges.push(i);
            }
            for p in row.parts.iter().filter_map(ty_node) {
                nodes[(back + p) as usize].edges.push(i);
            }
        }
        for (n, (paths, types, is_impl)) in (0u32..).zip(mentions) {
            for m in paths.iter().filter_map(def_node) {
                if m == n {
                    continue;
                }
                nodes[n as usize].edges.push(m);
                if is_impl {
                    nodes[m as usize].edges.push(n);
                }
            }
            for t in types.iter().filter_map(ty_node) {
                nodes[n as usize].edges.push(base + t);
                if is_impl {
                    nodes[(back + t) as usize].edges.push(n);
                }
            }
        }
        let deep = deep_hashes(&mut nodes);
        Some(Decls {
            deep: at
                .into_iter()
                .map(|(d, n)| (d, deep.get(n as usize).copied().flatten()))
                .collect(),
            whole: true,
            rows,
        })
    }
    /// The hash of the declarations `defs` and `tys` name, with every
    /// ancestor of each (a method's impl, a variant's enum), by their deep
    /// hashes. `None` when one reaches a record with no wire form.
    /// Cost: one probe per path named and per ancestor.
    pub(crate) fn of(&self, names: &Names<'_>, defs: &[DefId], tys: &[Ty]) -> Option<Hash128> {
        if !self.whole {
            return None;
        }
        let mut deep = Vec::new();
        let mut seen = hd_base::HashSet::default();
        let starts = defs
            .iter()
            .copied()
            .chain(type_defs(names.pool, tys))
            .map(|d| PathId::from_raw(d.raw()));
        for start in starts {
            let mut p = start;
            while p.get().is_some() && seen.insert(p) {
                if let Some(&h) = self.deep.get(&DefId::from_raw(p.raw())) {
                    deep.push(h?);
                }
                p = names.paths.parent(p);
            }
        }
        deep.sort_stable();
        deep.dedup();
        let mut h = StableHasher::new("unit-decls-v1");
        h.len(deep.len());
        for d in deep {
            h.hash(d);
        }
        Some(h.finish())
    }
}

/// Each node's deep hash: its component's members (key and
/// fingerprint, sorted) and the deep hashes of the components it reaches.
/// Tarjan's algorithm with an explicit stack, so a long chain of
/// declarations cannot overflow the thread's stack; a component closes
/// after every component it reaches, so their hashes are ready.
fn deep_hashes(nodes: &mut [Node]) -> Vec<Option<Hash128>> {
    const UNSEEN: u32 = u32::MAX;
    let n = nodes.len();
    for node in nodes.iter_mut() {
        node.edges.sort_stable();
        node.edges.dedup();
    }
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNSEEN; n];
    let mut comp_hash: Vec<Option<Hash128>> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut next = 0u32;
    // (node, position in its edges)
    let mut work: Vec<(u32, usize)> = Vec::new();
    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        // ruled out: a failure. `Decls::new` numbers at most `u32::MAX` nodes.
        let Ok(root) = u32::try_from(root) else { break };
        work.push((root, 0));
        while let Some(&(v, e)) = work.last() {
            let vi = v as usize;
            if e == 0 && index[vi] == UNSEEN {
                index[vi] = next;
                low[vi] = next;
                next += 1;
                stack.push(v);
                on_stack[vi] = true;
            }
            if let Some(&w) = nodes[vi].edges.get(e) {
                if let Some(top) = work.last_mut() {
                    top.1 = e + 1;
                }
                let wi = w as usize;
                if index[wi] == UNSEEN {
                    work.push((w, 0));
                } else if on_stack[wi] {
                    low[vi] = low[vi].min(index[wi]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                let pi = parent as usize;
                low[pi] = low[pi].min(low[vi]);
            }
            if low[vi] != index[vi] {
                continue;
            }
            // `v` roots a component: its members are on the stack above it.
            // ruled out: UNSEEN. There are no more components than nodes.
            let c = u32::try_from(comp_hash.len()).unwrap_or(UNSEEN);
            let mut members = Vec::new();
            while let Some(w) = stack.pop() {
                on_stack[w as usize] = false;
                comp[w as usize] = c;
                members.push(w);
                if w == v {
                    break;
                }
            }
            comp_hash.push(component_hash(nodes, &members, &comp, &comp_hash, c));
        }
    }
    comp.iter()
        .map(|&c| comp_hash.get(c as usize).copied().flatten())
        .collect()
}

/// The deep hash of component `c`, whose members are `members`; every
/// component they reach other than `c` has its hash in `done`. Reverse
/// members add no content, and a component that holds and reaches none
/// is [`NOTHING`].
fn component_hash(
    nodes: &[Node],
    members: &[u32],
    comp: &[u32],
    done: &[Option<Hash128>],
    c: u32,
) -> Option<Hash128> {
    let mut own = Vec::with_capacity(members.len());
    let mut reached = Vec::new();
    for &m in members {
        let node = &nodes[m as usize];
        if !node.reverse {
            own.push((node.key, node.fingerprint?));
        }
        for &w in &node.edges {
            let wc = comp[w as usize];
            if wc != c {
                reached.push((*done.get(wc as usize)?)?);
            }
        }
    }
    reached.retain(|&r| r != NOTHING);
    if own.is_empty() && reached.is_empty() {
        return Some(NOTHING);
    }
    own.sort_stable();
    reached.sort_stable();
    reached.dedup();
    let mut h = StableHasher::new("decl-component-v1");
    h.len(own.len());
    for (path, fingerprint) in own {
        h.hash(path);
        h.hash(fingerprint);
    }
    h.len(reached.len());
    for r in reached {
        h.hash(r);
    }
    Some(h.finish())
}

/// Every declaration `tys` name, at any depth: each type's own, its
/// arguments', an opaque result's function, an associated type and its
/// trait, a trait value's bindings. Each distinct type is visited once.
fn type_defs(pool: &InternPool, tys: &[Ty]) -> Vec<DefId> {
    let mut defs = Vec::new();
    let mut stack = tys.to_vec();
    let mut seen = hd_base::HashSet::default();
    while let Some(t) = stack.pop() {
        if !seen.insert(t) {
            continue;
        }
        match pool.get(t) {
            TyData::Opaque { owner, .. } => defs.push(owner),
            TyData::Assoc { assoc, .. } => defs.push(assoc),
            TyData::TraitValue { bindings, .. } => defs.extend(bindings.iter().map(|b| b.0)),
            // `ty_parts` reaches each of these one's declarations.
            TyData::Adt(..)
            | TyData::Ref { .. }
            | TyData::Fn { .. }
            | TyData::Row(_)
            | TyData::Context(_)
            | TyData::Param(_)
            | TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => {}
        }
        hd_resolve::ty_parts(pool, t, &mut defs, &mut stack);
    }
    defs
}
