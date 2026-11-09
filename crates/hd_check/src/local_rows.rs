//! Local omitted rows are equations, not nested body-checking loops.
//!
//! Synthetic row parameters carry the dependencies during discovery. They
//! never escape the group: a delta worklist solves them before a final
//! traversal checks the bodies with concrete rows. Graph depth does not
//! consume the native stack; each fact crosses each affected edge once.

use std::collections::VecDeque;

use hd_base::{DefId, HashMap, HashSet, LocalId, NodeIdx, SliceSort as _, StageResult, Symbol};
use hd_syntax::NodeRef;
use hd_types::{RowData, RowId, RowParamRef, Ty, TyData, TyList};

use crate::body::{Ck, RowFrame};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Transform {
    Identity,
    Subst {
        owner: DefId,
        first: u16,
        args: TyList,
    },
    Mask(RowId),
}

struct Node {
    param: RowParamRef,
    at: NodeIdx,
    source: Option<(usize, Transform)>,
}

struct CaptureNode {
    sub: u32,
    at: NodeIdx,
    locals: usize,
    own: Option<LocalId>,
}

#[derive(Clone, Copy)]
pub(crate) struct CaptureSource {
    at: NodeIdx,
    name: Symbol,
    original: LocalId,
}

pub(crate) struct Discovery {
    root: NodeIdx,
    owner: DefId,
    nodes: Vec<Node>,
    indices: HashMap<RowParamRef, usize>,
    proxies: HashMap<(usize, Transform), usize>,
    equations: Vec<(usize, RowId)>,
    bodies: Vec<(NodeIdx, usize)>,
    captures: Vec<CaptureNode>,
    capture_indices: HashMap<u32, usize>,
    capture_facts: Vec<(usize, LocalId)>,
    capture_edges: Vec<(usize, usize)>,
}

pub(crate) enum LocalRows {
    Discover(Discovery),
    Final {
        rows: HashMap<NodeIdx, RowId>,
        errors: Vec<NodeIdx>,
        diag_base: usize,
        captures: HashMap<NodeIdx, Vec<CaptureSource>>,
        replay: CaptureReplay,
    },
}

pub(crate) struct Solution {
    pub rows: Vec<(NodeIdx, RowId)>,
    pub errors: Vec<NodeIdx>,
    pub captures: HashMap<NodeIdx, Vec<CaptureSource>>,
}

pub(crate) struct CaptureReplay {
    persistent: usize,
    indexed: usize,
    locals: HashMap<(NodeIdx, Symbol), LocalId>,
    undo: Vec<((NodeIdx, Symbol), Option<LocalId>)>,
}

impl CaptureReplay {
    pub(crate) fn new(persistent: usize) -> Self {
        Self {
            persistent,
            indexed: persistent,
            locals: HashMap::default(),
            undo: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Mark {
    Discover {
        nodes: usize,
        equations: usize,
        bodies: usize,
        captures: usize,
        capture_facts: usize,
        capture_edges: usize,
    },
    Final {
        indexed: usize,
        undo: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fact {
    Key(Ty),
    Param(RowParamRef),
}

impl LocalRows {
    pub(crate) fn mark(&self) -> Mark {
        match self {
            Self::Discover(d) => Mark::Discover {
                nodes: d.nodes.len(),
                equations: d.equations.len(),
                bodies: d.bodies.len(),
                captures: d.captures.len(),
                capture_facts: d.capture_facts.len(),
                capture_edges: d.capture_edges.len(),
            },
            Self::Final { replay, .. } => Mark::Final {
                indexed: replay.indexed,
                undo: replay.undo.len(),
            },
        }
    }

    pub(crate) fn rollback(&mut self, mark: Mark) {
        // ruled out: a phase-changing rollback takes the group out of Ck
        // first. Every checkpoint reaching here belongs to this phase.
        if let (
            Self::Discover(d),
            Mark::Discover {
                nodes,
                equations,
                bodies,
                captures,
                capture_facts,
                capture_edges,
            },
        ) = (&mut *self, mark)
        {
            for node in d.nodes.drain(nodes..) {
                d.indices.remove(&node.param);
                if let Some(source) = node.source {
                    d.proxies.remove(&source);
                }
            }
            d.equations.truncate(equations);
            d.bodies.truncate(bodies);
            for node in d.captures.drain(captures..) {
                d.capture_indices.remove(&node.sub);
            }
            d.capture_facts.truncate(capture_facts);
            d.capture_edges.truncate(capture_edges);
        }
        if let (Self::Final { replay, .. }, Mark::Final { indexed, undo }) = (self, mark) {
            for (key, old) in replay.undo.drain(undo..).rev() {
                if let Some(old) = old {
                    replay.locals.insert(key, old);
                } else {
                    replay.locals.remove(&key);
                }
            }
            replay.indexed = indexed;
        }
    }
}

impl Discovery {
    pub(crate) fn new(root: NodeIdx, owner: DefId) -> Self {
        Self {
            root,
            owner,
            nodes: Vec::new(),
            indices: HashMap::default(),
            proxies: HashMap::default(),
            equations: Vec::new(),
            bodies: Vec::new(),
            captures: Vec::new(),
            capture_indices: HashMap::default(),
            capture_facts: Vec::new(),
            capture_edges: Vec::new(),
        }
    }

    fn node(
        &mut self,
        cx: &crate::body::BodyCx<'_>,
        at: NodeIdx,
        source: Option<(usize, Transform)>,
    ) -> usize {
        let i = self.nodes.len();
        // More than 65536 equations use another owner, rather than
        // truncating an index or imposing a limit on source programs.
        let owner = cx.names.member(
            self.owner,
            hd_intern::PathKind::Hidden,
            &format!("$local-row-{}-{}", self.root.raw(), i >> 16),
        );
        let param = RowParamRef {
            owner,
            index: self
                .nodes
                .last()
                .map_or(0, |n| n.param.index.wrapping_add(1)),
        };
        self.nodes.push(Node { param, at, source });
        self.indices.insert(param, i);
        i
    }

    fn proxy(
        &mut self,
        cx: &crate::body::BodyCx<'_>,
        source: usize,
        transform: Transform,
        at: NodeIdx,
    ) -> RowParamRef {
        if let Some(&i) = self.proxies.get(&(source, transform)) {
            return self.nodes[i].param;
        }
        let i = self.node(cx, at, Some((source, transform)));
        self.proxies.insert((source, transform), i);
        self.nodes[i].param
    }

    /// Captures are another finite union problem. A local predating the
    /// reader's parameters is outside it; locals declared in its body
    /// cannot become its captures. This uses declaration order, so no
    /// search of scopes or previously completed bodies is needed.
    fn solve_captures(
        &self,
        ck: &mut Ck<'_, '_>,
    ) -> StageResult<HashMap<NodeIdx, Vec<CaptureSource>>> {
        let mut facts = vec![HashSet::default(); self.captures.len()];
        let mut readers = vec![HashSet::default(); self.captures.len()];
        let mut queue = VecDeque::new();
        for &(source, target) in &self.capture_edges {
            readers[source].insert(target);
        }
        for &(i, l) in &self.capture_facts {
            if facts[i].insert(l) {
                queue.push_back((i, l));
            }
        }
        while let Some((source, l)) = queue.pop_front() {
            for &target in &readers[source] {
                ck.charge()?;
                let node = &self.captures[target];
                if (l.idx() < node.locals || node.own == Some(l)) && facts[target].insert(l) {
                    queue.push_back((target, l));
                }
            }
        }
        let body = ck.b.body();
        Ok(self
            .captures
            .iter()
            .zip(facts)
            .map(|(node, facts)| {
                let mut captures: Vec<_> = facts.into_iter().collect();
                // Keep declaration order, independent of the worklist's order.
                captures.sort_stable_by_key(|l| l.raw());
                (
                    node.at,
                    captures
                        .into_iter()
                        .map(|l| CaptureSource {
                            at: body.local_syn[l.idx()],
                            name: body.local_name[l.idx()],
                            original: l,
                        })
                        .collect(),
                )
            })
            .collect())
    }

    /// Finite-height union propagation. Each distinct fact is queued once
    /// per node, and only that node's readers see it. K is bounded by the
    /// produced row keys (and the existing generic-key depth limit), not
    /// by a retry count. Cost O((nodes + edges) * K), space O(nodes * K).
    pub(crate) fn solve(self, ck: &mut Ck<'_, '_>) -> StageResult<Solution> {
        let captures = self.solve_captures(ck)?;
        let pool = ck.pool();
        let mut facts = vec![HashSet::default(); self.nodes.len()];
        let mut edges = vec![HashSet::default(); self.nodes.len()];
        let mut queue = VecDeque::new();
        let mut errors = HashSet::default();
        let push = |i: usize,
                    fact: Fact,
                    facts: &mut Vec<HashSet<Fact>>,
                    queue: &mut VecDeque<(usize, Fact)>| {
            if facts[i].insert(fact) {
                queue.push_back((i, fact));
            }
        };
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some((source, transform)) = node.source {
                edges[source].insert((i, transform));
            }
        }
        for &(i, row) in &self.equations {
            let row = pool.row_data(ck.infer.resolve_row(pool, row));
            for k in row.keys {
                push(i, Fact::Key(k), &mut facts, &mut queue);
            }
            for p in row.params {
                if let Some(&source) = self.indices.get(&p) {
                    edges[source].insert((i, Transform::Identity));
                } else {
                    push(i, Fact::Param(p), &mut facts, &mut queue);
                }
            }
        }
        while let Some((source, fact)) = queue.pop_front() {
            // Substitution of a row argument can add an edge while
            // propagating. Copy this node's adjacency, never the graph.
            let readers: Vec<_> = edges[source].iter().copied().collect();
            for (target, transform) in readers {
                ck.charge()?;
                let row = match fact {
                    Fact::Key(k) => RowData {
                        keys: vec![k],
                        params: Vec::new(),
                    },
                    Fact::Param(p) => RowData {
                        keys: Vec::new(),
                        params: vec![p],
                    },
                };
                let mut row = match transform {
                    Transform::Subst { owner, first, args } => {
                        pool.row_data(pool.subst_row(pool.row(&row), &|p| {
                            if p.owner != owner {
                                return None;
                            }
                            pool.list_items(args)
                                .get(usize::from(p.index.checked_sub(first)?))
                                .copied()
                        }))
                    }
                    _ => row,
                };
                if let Transform::Mask(mask) = transform {
                    let mask = pool.row_data(ck.infer.resolve_row(pool, mask));
                    row.keys.retain(|&k| {
                        !mask
                            .keys
                            .iter()
                            .any(|&h| crate::rows::key_supplies(ck.cx.lookup, pool, h, k))
                    });
                    row.params.retain(|p| !mask.params.contains(p));
                }
                for k in row.keys {
                    let k = ck.infer.resolve(pool, k);
                    if crate::results::depth(pool, k, 0) > crate::results::MAX_KEY_DEPTH {
                        errors.insert(self.nodes[target].at);
                    } else {
                        push(target, Fact::Key(k), &mut facts, &mut queue);
                    }
                }
                for p in row.params {
                    if let Some(&dependency) = self.indices.get(&p) {
                        if edges[dependency].insert((target, Transform::Identity)) {
                            for fact in facts[dependency].clone() {
                                push(target, fact, &mut facts, &mut queue);
                            }
                        }
                    } else {
                        push(target, Fact::Param(p), &mut facts, &mut queue);
                    }
                }
            }
        }
        let rows = self
            .bodies
            .into_iter()
            .map(|(at, i)| {
                let mut row = RowData::default();
                for fact in &facts[i] {
                    match fact {
                        Fact::Key(k) => row.keys.push(*k),
                        Fact::Param(p) => row.params.push(*p),
                    }
                }
                (at, pool.row(&row))
            })
            .collect();
        let mut errors: Vec<_> = errors.into_iter().collect();
        errors.sort_stable_by_key(|n| n.raw());
        Ok(Solution {
            rows,
            errors,
            captures,
        })
    }
}

impl Ck<'_, '_> {
    pub(crate) fn local_capture_open(
        &mut self,
        sub: u32,
        at: NodeIdx,
        locals: usize,
        own: Option<LocalId>,
    ) {
        if let Some(LocalRows::Discover(d)) = &mut self.local_rows {
            let i = d.captures.len();
            if let Some(parent) = self
                .subs
                .last()
                .and_then(|s| d.capture_indices.get(&s.mark.sub().raw()))
            {
                d.capture_edges.push((i, *parent));
            }
            d.captures.push(CaptureNode {
                sub,
                at,
                locals,
                own,
            });
            d.capture_indices.insert(sub, i);
        }
    }

    pub(crate) fn local_capture_close(&mut self, sub: u32, caps: &[LocalId]) {
        if let Some(LocalRows::Discover(d)) = &mut self.local_rows
            && let Some(&i) = d.capture_indices.get(&sub)
        {
            d.capture_facts.extend(caps.iter().map(|&l| (i, l)));
        }
    }

    /// Record a generic closure's capture dependency, even when its row
    /// is pure. Known direct captures are still marked by ordinary reads.
    pub(crate) fn local_capture_use(&mut self, sub: u32) {
        if let Some(LocalRows::Discover(d)) = &mut self.local_rows
            && let Some(&source) = d.capture_indices.get(&sub)
            && let Some(&target) = self
                .subs
                .last()
                .and_then(|s| d.capture_indices.get(&s.mark.sub().raw()))
        {
            d.capture_edges.push((source, target));
        }
    }

    /// Index each newly created local once. The prefix surviving the
    /// group rollback already has stable IDs; no scan of that prefix is
    /// needed. Trials undo only the index entries they introduced.
    pub(crate) fn local_capture_header(&mut self, at: NodeIdx) -> Option<Vec<LocalId>> {
        let Some(LocalRows::Final {
            captures, replay, ..
        }) = &mut self.local_rows
        else {
            return None;
        };
        let body = self.b.body();
        for raw in replay.indexed..body.local_ty.len() {
            // ruled out: TirBuilder already checked every allocated
            // LocalId fits u32, including this body-local slice.
            let l = LocalId::from_raw(u32::try_from(raw).ok()?);
            let key = (body.local_syn[raw], body.local_name[raw]);
            let old = replay.locals.insert(key, l);
            replay.undo.push((key, old));
        }
        replay.indexed = body.local_ty.len();
        Some(
            captures
                .get(&at)?
                .iter()
                .filter_map(|origin| {
                    if origin.original.idx() < replay.persistent {
                        Some(origin.original)
                    } else {
                        replay.locals.get(&(origin.at, origin.name)).copied()
                    }
                })
                .collect(),
        )
    }

    /// The header row during discovery, or the settled row on the final
    /// traversal. A lexical group includes every nested omitted row.
    pub(crate) fn local_row_header(&mut self, at: NodeIdx) -> Option<RowId> {
        let pool = self.pool();
        match self.local_rows.as_mut()? {
            LocalRows::Discover(d) => {
                let i = d.node(self.cx, at, None);
                d.bodies.push((at, i));
                Some(pool.row(&RowData {
                    keys: Vec::new(),
                    params: vec![d.nodes[i].param],
                }))
            }
            LocalRows::Final { rows, .. } => rows.get(&at).copied(),
        }
    }

    pub(crate) fn local_row_equation(&mut self, header: RowId, row: RowId) {
        let params = self.pool().row_data(header).params;
        if let Some(LocalRows::Discover(d)) = &mut self.local_rows {
            for p in params {
                if let Some(&i) = d.indices.get(&p) {
                    d.equations.push((i, row));
                }
            }
        }
    }

    pub(crate) fn local_row_open(&self, row: RowId) -> bool {
        let Some(LocalRows::Discover(d)) = &self.local_rows else {
            return false;
        };
        self.pool()
            .row_data(row)
            .params
            .iter()
            .any(|p| d.indices.contains_key(p))
    }

    /// A symbolic requirement under providers is a dependency with a
    /// mask. Declared frames are validated on the final traversal.
    pub(crate) fn local_row_requirement(&mut self, p: RowParamRef, n: NodeRef<'_>) -> bool {
        let Some(LocalRows::Discover(d)) = &self.local_rows else {
            return false;
        };
        let Some(&source) = d.indices.get(&p) else {
            return false;
        };
        let pool = self.pool();
        let mut mask = RowData::default();
        for i in (0..self.rows.len()).rev() {
            match &self.rows[i] {
                RowFrame::With(row) => {
                    let row = pool.row_data(*row);
                    mask.keys.extend(row.keys);
                    mask.params.extend(row.params);
                }
                RowFrame::Closure { written: None, .. } | RowFrame::Inferred { .. } => {
                    let param = if mask.keys.is_empty() && mask.params.is_empty() {
                        p
                    } else if let Some(LocalRows::Discover(d)) = &mut self.local_rows {
                        d.proxy(self.cx, source, Transform::Mask(pool.row(&mask)), n.index())
                    } else {
                        p
                    };
                    if let RowFrame::Closure { used, .. } | RowFrame::Inferred { used, .. } =
                        &mut self.rows[i]
                    {
                        used.params.push(param);
                        self.log_undo(crate::trial::BodyUndo::RowParam(i));
                    }
                    return true;
                }
                _ => return true,
            }
        }
        true
    }

    /// The solver caps growing keys to remain finite. Report those sites
    /// after final checking, which ordinarily diagnoses the same use.
    /// Examine only this group's diagnostics, once, to avoid duplicates.
    pub(crate) fn finish_local_rows(&mut self) {
        let Some(LocalRows::Final {
            errors, diag_base, ..
        }) = self.local_rows.take()
        else {
            return;
        };
        if errors.is_empty() {
            return;
        }
        let mut reported: HashSet<_> = (diag_base..self.diags.code.len())
            .filter(|&i| self.diags.code[i] == hd_diag::Code::InstantiationTooDeep)
            .map(|i| {
                let s = self.diags.primary[i];
                (s.file, s.lo, s.hi)
            })
            .collect();
        for at in errors {
            let n = self.cx.src.parse.tree.node(at);
            let span = self.cx.src.span(n);
            if reported.insert((span.file, span.lo, span.hi)) {
                self.err(hd_diag::Code::InstantiationTooDeep, n,
                    "this recursive call grows an inferred requirement key past the instantiation depth limit; write the function's `$` clause");
            }
        }
    }

    /// Instantiate unknown rows lazily: substituting a generic type now
    /// must also substitute facts discovered for its row later.
    pub(crate) fn local_row_inst(
        &mut self,
        ty: Ty,
        owner: DefId,
        first: u16,
        args: &[Ty],
        at: NodeIdx,
    ) -> Ty {
        let pool = self.pool();
        let args = pool.list(args);
        let discovery = std::cell::RefCell::new(&mut self.local_rows);
        pool.subst(ty, &|p| {
            if let Some(LocalRows::Discover(d)) = &mut **discovery.borrow_mut()
                && let Some(&source) = d.indices.get(&RowParamRef {
                    owner: p.owner,
                    index: p.index,
                })
            {
                let param = d.proxy(self.cx, source, Transform::Subst { owner, first, args }, at);
                return Some(pool.intern_ty(&TyData::Row(pool.row(&RowData {
                    keys: Vec::new(),
                    params: vec![param],
                }))));
            }
            if p.owner != owner {
                return None;
            }
            pool.list_items(args)
                .get(usize::from(p.index.checked_sub(first)?))
                .copied()
        })
    }
}
