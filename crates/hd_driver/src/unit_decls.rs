//! The declarations a stored unit's key holds (#2423,
//! `crates/hd_wasm/DESIGN.md` §23.41): a unit's emission reads the
//! program's declarations its code names (a callee's signature, a type's
//! fields, an impl's head), so its key holds those, each by its
//! fingerprint, not one hash of every declaration. Adding a function no
//! unit names then re-emits no unit, as rustc keys a query on its inputs'
//! fingerprints rather than on the whole crate.
//!
//! The graph: one node per non-std item of the program, its fingerprint
//! the hash of its record ([`hd_resolve::item_fingerprint`]); an edge to
//! every declaration its record names, and from every declaration an
//! impl's record names to that impl, since a type's drop hook, its
//! associated types and its trait impls live in impls that name it. Std
//! declarations are the toolchain's, in every key already. A node's deep
//! hash covers everything it reaches: its strongly connected component's
//! members and the deep hashes of the components it reaches, one pass of
//! Tarjan's algorithm over the graph (each component closes after every
//! component it reaches), as Go's and Bazel's action keys hash their
//! inputs' transitive digests. A unit's part is then one probe per
//! declaration it names. Cost: one record encoding per non-std item and
//! one visit per node and edge, once per link.

use hd_base::{DefId, Hash128, HashMap, PathId, SliceSort as _, StableHasher};
use hd_resolve::{ItemData, Names};
use hd_types::{InternPool, Ty, TyData};

/// The deep hash of each of the program's non-std declarations.
pub(crate) struct Decls {
    /// By declaration: its component's deep hash; `None` for one that
    /// reaches a record with no wire form, whose units are not stored.
    deep: HashMap<DefId, Option<Hash128>>,
    /// Whether the graph holds every item: false past `u32::MAX` items,
    /// whose nodes have no number; then no unit is stored.
    whole: bool,
}

/// One node of the graph while it is built.
struct Node {
    def: DefId,
    fingerprint: Option<Hash128>,
    edges: Vec<u32>,
}

impl Decls {
    /// The graph of `items`, each a non-std item of the program.
    /// Cost: one record encoding per item, one visit per node and edge.
    pub(crate) fn new<'i>(
        names: &Names<'_>,
        items: impl Iterator<Item = &'i hd_resolve::Item>,
    ) -> Self {
        let mut at: HashMap<DefId, u32> = HashMap::default();
        let mut nodes = Vec::new();
        let mut mentions = Vec::new();
        for item in items {
            let Ok(n) = u32::try_from(nodes.len()) else {
                return Decls {
                    deep: HashMap::default(),
                    whole: false,
                };
            };
            at.insert(item.def, n);
            let (fingerprint, paths) = match hd_resolve::item_fingerprint(names, item) {
                Ok((h, paths)) => (Some(h), paths),
                Err(_) => (None, Vec::new()),
            };
            let is_impl = matches!(item.data, ItemData::Impl { .. });
            nodes.push(Node {
                def: item.def,
                fingerprint,
                edges: Vec::new(),
            });
            mentions.push((paths, is_impl));
        }
        for (n, (paths, is_impl)) in (0u32..).zip(mentions) {
            for p in paths {
                let Some(&m) = at.get(&DefId::from_raw(p.raw())) else {
                    continue;
                };
                if m == n {
                    continue;
                }
                nodes[n as usize].edges.push(m);
                if is_impl {
                    nodes[m as usize].edges.push(n);
                }
            }
        }
        let deep = deep_hashes(names, &mut nodes);
        Decls {
            deep: nodes.iter().map(|n| n.def).zip(deep).collect(),
            whole: true,
        }
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

/// Each node's deep hash: its component's members (path hash and
/// fingerprint, sorted) and the deep hashes of the components it reaches.
/// Tarjan's algorithm with an explicit stack, so a long chain of
/// declarations cannot overflow the thread's stack; a component closes
/// after every component it reaches, so their hashes are ready.
fn deep_hashes(names: &Names<'_>, nodes: &mut [Node]) -> Vec<Option<Hash128>> {
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
            comp_hash.push(component_hash(names, nodes, &members, &comp, &comp_hash, c));
        }
    }
    comp.iter()
        .map(|&c| comp_hash.get(c as usize).copied().flatten())
        .collect()
}

/// The deep hash of component `c`, whose members are `members`; every
/// component they reach other than `c` has its hash in `done`.
fn component_hash(
    names: &Names<'_>,
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
        own.push((names.path_hash(node.def), node.fingerprint?));
        for &w in &node.edges {
            let wc = comp[w as usize];
            if wc != c {
                reached.push((*done.get(wc as usize)?)?);
            }
        }
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
