//! Omitted result types and rows (`crates/hd_tir/DESIGN.md` §4.13.1 "Omitted
//! result types (M1)" and "M3: inferred rows"). A non-public function or
//! inherent method written without `-> T` (spec 07
//! `fn.decl.result-omitted-private`) takes its result from its body: the
//! least common type of the final value and every `return` operand, `void`
//! when the body produces no value (`fn.decl.result-inferred`,
//! `fn.decl.result-inferred.common`, `fn.decl.result-inferred.void`). One
//! written without a `$` clause takes the least row its body needs
//! (`req.row.omitted.inferred-private`).
//!
//! Such a body is checked before its callers: those that reach no cycle
//! callees first, then the rest in source order. A body about to be
//! checked for a caller first has the pending bodies it reaches checked,
//! callees first. A body never checks another inside its own check: one
//! that reads a pending callable waits for it, and an explicit stack
//! checks the callable and then the body again ([`drive`]), so no chain
//! or cycle of them nests body checks. A callee whose omitted result is
//! being checked, or waits, closes a cycle (`fn.decl.omitted-cycle`),
//! reported once at the cycle's member that comes first in source order
//! (`fn.decl.omitted-cycle.report`). A callee
//! whose omitted row is still open is no error: the call records an edge,
//! and the rows of the recursive group are the least solution of its edges
//! once no member is being checked (`req.row.omitted.cycle`).

use hd_base::HashMap;
use hd_base::SliceSort as _;
use hd_types::AdtId;
use std::collections::VecDeque;

use hd_base::{DefId, NodeIdx, StageResult};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{FnSig, ImplKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_types::{ParamRef, RowData, RowId, Ty, TyData, Types};

use crate::body::{BodyCx, Ck, Infer, check_fn_body};

/// The instantiation depth limit (spec 04
/// `types.generic.instantiation-depth`; `crates/hd_tir/DESIGN.md` §4.15): a row
/// key nested deeper comes from polymorphic recursion and stops the solve.
pub(crate) const MAX_KEY_DEPTH: u32 = 32;

#[derive(Clone, Copy)]
enum State {
    /// Not checked yet: its declaration.
    Pending(NodeIdx),
    /// Its body is being checked.
    Active(NodeIdx),
    /// Its check read a pending callable, so the check was dropped; it
    /// runs again once [`drive`] has checked what it read.
    Waiting(NodeIdx),
    /// Checked: its result. Its body, with the body's diagnostics, is in
    /// [`Results::bodies`] until `Body(m)` takes it.
    Done { ret: Ty },
}

/// What a registered callable's declaration leaves out.
#[derive(Clone, Copy)]
struct Omits {
    ret: bool,
    row: bool,
    /// The callable's declaration.
    at: NodeIdx,
    /// Source order, indexed once when the declaration is registered.
    position: usize,
}

/// A call of a private callable whose row is still open, from the body of
/// a member of its recursive group (`crates/hd_tir/DESIGN.md` "M3: inferred
/// rows"): the callee, the call's type arguments by owner, and the keys of
/// the `$.with` blocks around the call, which the caller need not supply.
#[derive(Clone)]
pub(crate) struct RowEdge {
    pub callee: DefId,
    pub args: Vec<(DefId, Vec<Ty>)>,
    pub minus: Vec<Ty>,
    pub at: NodeIdx,
}

/// The row facts of one finished body with an omitted row: the keys and
/// row parameters it takes from the body, and its edges to open rows.
#[derive(Default)]
pub(crate) struct RowFacts {
    pub used: RowData,
    pub edges: Vec<RowEdge>,
    pub fuel_left: Option<u64>,
}

/// An omitted row after its body is checked.
#[derive(Clone)]
enum RowState {
    /// It still depends on a callable being checked.
    Waiting {
        used: RowData,
        edges: Vec<RowEdge>,
    },
    Solved(RowId),
}

/// A module's private callables with an omitted result type or row.
#[derive(Default)]
pub struct Results {
    state: HashMap<DefId, State>,
    /// The body of each done callable, with its diagnostics, until
    /// `Body(m)` takes it; a callable checked again loses it.
    bodies: HashMap<DefId, Box<(StageResult<Body>, DiagBuf)>>,
    omits: HashMap<DefId, Omits>,
    rows: HashMap<DefId, RowState>,
    /// Signature dependencies recorded by checking, including reads that
    /// materialize a function value. A solved row can still be a current
    /// approximation when its body read another unfinished signature.
    readers: HashMap<DefId, hd_base::HashSet<DefId>>,
    dirty: VecDeque<DefId>,
    queued: hd_base::HashSet<DefId>,
    rechecking: bool,
    /// Pending row signatures discovered by an active body. Forcing them
    /// runs on this queue after that body returns, never on its stack.
    deferred_rows: VecDeque<DefId>,
    deferred_queued: hd_base::HashSet<DefId>,
    draining_rows: bool,
    returns: HashMap<DefId, Ty>,
    /// Rechecking a body spends the remainder of its existing work budget.
    remaining: HashMap<DefId, u64>,
    /// Written rows' calls of open rows: the caller, the edge, its row.
    waits: Vec<(DefId, RowEdge, RowId)>,
    /// The callables in source order.
    order: Vec<DefId>,
    /// The callable being checked: at most one, since a body never
    /// checks another inside its own check.
    active: Vec<DefId>,
    /// The pending callables the active body read, which it waits for.
    blocked: Vec<DefId>,
    /// The waiting callables of the running [`drive`], outermost first:
    /// each read the next, and the last read the active one.
    chain: Vec<DefId>,
    /// The `recursive-function-needs-result-type` reports, and those of
    /// the module's top-level statements' calls of open rows.
    module_diags: DiagBuf,
    reported: hd_base::HashSet<DefId>,
    /// The callables a template named without a declared result type,
    /// each reported once (`module.package.template-private.result`).
    template_named: Vec<DefId>,
    /// The templates with a method that calls `walk`, `describe` or
    /// `build` of `Structure` (`annot.template.body`).
    structure_reads: hd_base::HashSet<DefId>,
    /// A result-type cycle or a poisoned module initialization read.
    errors: bool,
    /// Inferred-result diagnostics belong to the current body, not a
    /// discarded row approximation. Rechecking can remove these errors.
    body_errors: hd_base::HashSet<DefId>,
    /// The unresolved rows. Solving visits this set, without sweeping
    /// unrelated callables that have already finished.
    waiting: hd_base::HashSet<DefId>,
    /// The waiting rows with an edge to each callable, appended as a row
    /// starts waiting; an entry whose row stopped waiting is skipped.
    waiters: HashMap<DefId, Vec<DefId>>,
    /// The callables whose check finished since the last settle. Every
    /// waiting row was blocked after that settle, so only a row that
    /// reaches one of them can be ready now ([`Results::solve_ready`]).
    settled_since: Vec<DefId>,
    /// The callees of each waiting row's edges, in edge order.
    wait_callees: HashMap<DefId, Vec<DefId>>,
    /// Per waiting row, the edge that last reached a callable not done:
    /// the next search for one starts there.
    undone_hint: HashMap<DefId, usize>,
    /// In a forked context: how many of `template_named` it was forked
    /// with ([`crate::fork`]).
    named_base: usize,
    /// The reference graph of the callables, built on first use.
    plan: Option<std::sync::Arc<Plan>>,
}

/// What a caller sees of a registered callable's omitted parts.
struct Seen {
    /// The inferred result, when the result is omitted.
    ret: Option<Ty>,
    row: RowSeen,
}

enum RowSeen {
    Written,
    Solved(RowId),
    /// Still being inferred: calls record edges instead.
    Open,
}

/// An unfinished row's approximation is empty. Solving it to empty adds
/// no signature information and must not make its readers start over.
pub(crate) fn row_changed(before: Option<RowId>, after: Option<RowId>) -> bool {
    before.unwrap_or(RowId::EMPTY) != after.unwrap_or(RowId::EMPTY)
}

impl Results {
    pub(crate) fn inferring(&self) -> bool {
        !self.active.is_empty()
    }

    /// The native body-check depth, observed only by opt-in work tests.
    pub(crate) fn active_depth(&self) -> usize {
        self.active.len()
    }

    /// A copy for a forked context, taken after M1: every callable is
    /// done, so no body is copied, and nothing waits.
    pub(crate) fn fork(&self) -> Results {
        debug_assert!(
            self.waits.is_empty()
                && self.active.is_empty()
                && self.chain.is_empty()
                && self.dirty.is_empty()
        );
        Results {
            state: self.state.clone(),
            bodies: HashMap::default(),
            omits: self.omits.clone(),
            rows: self.rows.clone(),
            readers: HashMap::default(),
            dirty: VecDeque::new(),
            queued: hd_base::HashSet::default(),
            rechecking: false,
            deferred_rows: VecDeque::new(),
            deferred_queued: hd_base::HashSet::default(),
            draining_rows: false,
            returns: HashMap::default(),
            remaining: HashMap::default(),
            waits: Vec::new(),
            order: self.order.clone(),
            active: Vec::new(),
            blocked: Vec::new(),
            chain: Vec::new(),
            module_diags: DiagBuf::default(),
            reported: hd_base::HashSet::default(),
            template_named: self.template_named.clone(),
            structure_reads: hd_base::HashSet::default(),
            errors: self.has_errors(),
            body_errors: hd_base::HashSet::default(),
            waiting: self.waiting.clone(),
            waiters: self.waiters.clone(),
            settled_since: Vec::new(),
            wait_callees: self.wait_callees.clone(),
            undone_hint: HashMap::default(),
            named_base: self.template_named.len(),
            plan: self.plan.clone(),
        }
    }

    /// What the bodies checked in a forked context since the last call
    /// added: the callables a template named, in order, and the
    /// templates that read `Structure`. Their reports are made again when
    /// they join ([`Results::join`]), so the forked ones are dropped.
    pub(crate) fn drain(&mut self) -> (Vec<DefId>, Vec<DefId>) {
        let named = self.template_named.split_off(self.named_base);
        self.module_diags = DiagBuf::default();
        let reads = self.structure_reads.drain().collect();
        (named, reads)
    }

    /// Adds what a forked body added, as if it had been checked here.
    pub(crate) fn join(&mut self, cx: &BodyCx<'_>, named: Vec<DefId>, reads: Vec<DefId>) {
        for def in named {
            self.template_names(cx, def);
        }
        self.structure_reads.extend(reads);
    }

    /// Whether an inferred result's body or a cycle reported an error, so
    /// a body whose types hold poison from it adds no `cannot-infer-type`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        let own = self
            .active
            .last()
            .is_some_and(|def| self.body_errors.contains(def));
        self.errors || self.body_errors.len() > usize::from(own)
    }

    /// A body read a binding no statement had declared yet. The
    /// initialization check reports it, and the poison it left in an
    /// inferred result adds no `cannot-infer-type` to a caller.
    pub(crate) fn note_poisoned_read(&mut self) {
        self.errors = true;
    }

    /// A template names `def`: a private callable with an omitted result
    /// type follows the public signature rules, so it is an error, once at
    /// its declaration (`module.package.template-private.result`).
    fn template_names(&mut self, cx: &BodyCx<'_>, def: DefId) {
        let Some(omits) = self.omits.get(&def).copied().filter(|o| o.ret) else {
            return;
        };
        if self.template_named.contains(&def) {
            return;
        }
        self.template_named.push(def);
        let node = cx.src.parse.tree.node(omits.at);
        let (span, name) = match node
            .name(&cx.src.parse.tokens)
            .or_else(|| cx.src.name_after(node, TokenKind::KwFn))
        {
            Some(t) => (cx.src.token_span(t), cx.src.text(t)),
            None => (cx.src.span(node), "this function"),
        };
        let msg = format!(
            "`{name}` is named by a template, so it follows the public signature rules: write its result type"
        );
        self.module_diags.error(Code::MissingResultType, span, &msg);
    }

    /// One report per cycle, at its member first in source order. The
    /// cycle runs from `def` along the waiting chain to the active body.
    fn cycle(&mut self, cx: &BodyCx<'_>, def: DefId) {
        let path = || self.chain.iter().chain(&self.active);
        let Some(at) = path().position(|d| *d == def) else {
            return;
        };
        let Some(first) = path()
            .skip(at)
            .copied()
            .min_by_key(|d| self.omits[d].position)
        else {
            return;
        };
        self.errors = true;
        if !self.reported.insert(first) {
            return;
        }
        let Some(State::Active(n) | State::Waiting(n)) = self.state.get(&first) else {
            return;
        };
        let node = cx.src.parse.tree.node(*n);
        let (span, name) = match node
            .name(&cx.src.parse.tokens)
            .or_else(|| cx.src.name_after(node, TokenKind::KwFn))
        {
            Some(t) => (cx.src.token_span(t), cx.src.text(t)),
            None => (cx.src.span(node), "this function"),
        };
        let msg = format!(
            "`{name}` calls itself through functions without a result type; write its result type"
        );
        self.module_diags
            .error(Code::RecursiveFunctionNeedsResultType, span, &msg);
    }

    /// Whether `def`'s row is omitted and not solved yet: its body is not
    /// checked, is being checked, or waits on one that is.
    fn row_open(&self, def: DefId) -> bool {
        self.omits.get(&def).is_some_and(|o| o.row)
            && !matches!(self.rows.get(&def), Some(RowState::Solved(_)))
    }

    /// The callables not checked yet that `def`'s waiting row reaches
    /// through its edges and the waiting rows they lead to.
    fn pending_reach(&self, def: DefId) -> Vec<DefId> {
        let mut seen = vec![def];
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(&d) = seen.get(i) {
            i += 1;
            let Some(RowState::Waiting { edges, .. }) = self.rows.get(&d) else {
                continue;
            };
            for e in edges {
                if seen.contains(&e.callee) {
                    continue;
                }
                seen.push(e.callee);
                if matches!(self.state.get(&e.callee), Some(State::Pending(_))) {
                    out.push(e.callee);
                }
            }
        }
        out
    }

    /// Where a diagnostic about `owner`'s body goes once a solve finds
    /// it: the body's own diagnostics until `Body(m)` takes them, else
    /// (the top-level statements) the module's.
    fn diags_of(&mut self, owner: DefId) -> &mut DiagBuf {
        match self.bodies.get_mut(&owner) {
            Some(b) => &mut b.1,
            None => &mut self.module_diags,
        }
    }

    /// After a body is checked: solves the rows that no longer wait on a
    /// body, then checks the written rows that waited on them.
    fn settle(&mut self, cx: &BodyCx<'_>) {
        self.solve_ready(cx);
        self.check_waits(cx);
    }

    /// Queue a signature only when its body has not been checked. Active
    /// cycle members use their approximation; completed members already
    /// have indexed readers for subsequent changes.
    fn defer_row(&mut self, def: DefId) {
        if matches!(self.state.get(&def), Some(State::Pending(_)))
            && self.deferred_queued.insert(def)
        {
            self.deferred_rows.push_back(def);
        }
    }

    /// Only bodies that read this signature may need checking again.
    fn changed(&mut self, def: DefId) -> usize {
        if let Some(readers) = self.readers.get(&def) {
            let mut readers: Vec<_> = readers.iter().copied().collect();
            readers.sort_stable_by_key(|def| self.omits[def].position);
            let visited = readers.len();
            for reader in readers {
                if self.queued.insert(reader) {
                    self.dirty.push_back(reader);
                }
            }
            return visited;
        }
        0
    }

    /// Solves every waiting row whose edges reach only checked callables:
    /// the least rows that satisfy each member of the group
    /// (`req.row.omitted.cycle`), by a monotone pass over the edges in
    /// source order until nothing grows.
    ///
    /// Every row still waiting after a settle is blocked, so a row can be
    /// ready now only if it reaches a callable checked since then. The
    /// search starts at those callables' rows and the rows calling them,
    /// then at the rows calling each group it solves. Cost: the rows and
    /// edges it visits, each once per settle; a row with an edge to a
    /// callable not done stops it, found from that row's last such edge.
    /// So a settle costs what changed, not every waiting row (#1839).
    fn solve_ready(&mut self, cx: &BodyCx<'_>) {
        let settled = std::mem::take(&mut self.settled_since);
        if self.waiting.is_empty() {
            return;
        }
        let mut seeds: Vec<DefId> = Vec::new();
        for c in settled {
            seeds.push(c);
            if let Some(ws) = self.waiters.get(&c) {
                seeds.extend(ws.iter().copied());
            }
        }
        let mut search = ReadySearch::default();
        loop {
            let ready = self.find_ready(&seeds, &mut search);
            if ready.is_empty() {
                return;
            }
            seeds = ready
                .iter()
                .filter_map(|d| self.waiters.get(d))
                .flatten()
                .copied()
                .collect();
            self.solve_group(cx, ready);
        }
    }

    /// Whether waiting row `d` has an edge to a callable not done: the
    /// scan starts at the edge that last had one and wraps around.
    fn reaches_undone(&mut self, d: DefId) -> bool {
        let callees = &self.wait_callees[&d];
        let from = self.undone_hint.get(&d).copied().unwrap_or(0);
        let n = callees.len();
        for k in 0..n {
            crate::work::solve_work(1);
            let i = (from + k) % n;
            if !matches!(self.state.get(&callees[i]), Some(State::Done { .. })) {
                self.undone_hint.insert(d, i);
                return true;
            }
        }
        false
    }

    /// The waiting rows reachable from `seeds` through waiting rows'
    /// edges that reach no callable not done, by Tarjan's strongly
    /// connected components: a component is blocked when a member has an
    /// edge to a callable not done or to a blocked component. `search`
    /// keeps each row's verdict for the rest of the settle.
    fn find_ready(&mut self, seeds: &[DefId], search: &mut ReadySearch) -> Vec<DefId> {
        let mut ready = Vec::new();
        for &seed in seeds {
            search.walk.visit(seed, |event| {
                match event {
                    hd_base::scc::Event::Enter(v, out) => {
                        if !self.waiting.contains(&v) {
                            return false;
                        }
                        crate::work::solve_work(1);
                        let blocked = self.reaches_undone(v);
                        search.blocked.insert(v, blocked);
                        if !blocked {
                            out.extend_from_slice(&self.wait_callees[&v]);
                        }
                    }
                    hd_base::scc::Event::Edge(v, w) => {
                        crate::work::solve_work(1);
                        if search.blocked.get(&w) == Some(&true) {
                            search.blocked.insert(v, true);
                        }
                    }
                    hd_base::scc::Event::Component(members, _) => {
                        let blocked = members.iter().any(|m| search.blocked[m]);
                        for &m in members {
                            search.blocked.insert(m, blocked);
                        }
                        if !blocked {
                            ready.extend_from_slice(members);
                        }
                    }
                    hd_base::scc::Event::Finish(_) => {}
                }
                true
            });
        }
        ready
    }

    /// Solves the rows of `group`, which reach only callables done and
    /// rows solved or in `group`.
    fn solve_group(&mut self, cx: &BodyCx<'_>, mut group: Vec<DefId>) {
        group.sort_stable_by_key(|def| self.omits[def].position);
        let pool = cx.names.pool.types();
        let mut cur: HashMap<DefId, RowData> = HashMap::default();
        let mut edges: Vec<(DefId, RowEdge)> = Vec::new();
        for d in &group {
            if let Some(RowState::Waiting { used, edges: es }) = self.rows.get(d) {
                cur.insert(*d, used.clone());
                edges.extend(es.iter().map(|e| (*d, e.clone())));
            }
        }
        let mut deep: Vec<(DefId, NodeIdx, Ty)> = Vec::new();
        loop {
            let mut grew = false;
            for (f, e) in &edges {
                let callee = match (cur.get(&e.callee), self.rows.get(&e.callee)) {
                    (Some(r), _) => r.clone(),
                    (None, Some(RowState::Solved(r))) => pool.row_data(*r),
                    _ => RowData::default(),
                };
                let add = edge_row(cx, pool, &callee, e);
                let row = cur.entry(*f).or_default();
                for k in add.keys {
                    if row.keys.contains(&k) {
                        continue;
                    }
                    if depth(pool, k, 0) > MAX_KEY_DEPTH {
                        if !deep.iter().any(|x| x.0 == *f && x.1 == e.at) {
                            deep.push((*f, e.at, k));
                        }
                        continue;
                    }
                    row.keys.push(k);
                    grew = true;
                }
                for p in add.params {
                    if !row.params.contains(&p) {
                        row.params.push(p);
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        // Interned in source order, so the run's ids do not depend on the
        // map's.
        for d in group {
            let row = cx.names.pool.row(&cur.remove(&d).unwrap_or_default());
            self.rows.insert(d, RowState::Solved(row));
            self.waiting.remove(&d);
            self.wait_callees.remove(&d);
            self.undone_hint.remove(&d);
            if row != RowId::EMPTY {
                self.changed(d);
            }
        }
        for (f, at, k) in deep {
            let node = cx.src.parse.tree.node(at);
            let span = cx.src.span(node);
            let shown = hd_resolve::show_ty_in(&cx.names, pool, k);
            let msg = format!(
                "this call adds `$ {shown}` to the inferred row, whose type arguments nest past the limit of {MAX_KEY_DEPTH}; write the function's `$` clause"
            );
            self.diags_of(f)
                .error(Code::InstantiationTooDeep, span, &msg);
        }
    }

    /// Each written row that called an open row entails its solved keys
    /// and row parameters (`req.row.set.call`): `missing-requirement` at
    /// the call otherwise.
    fn check_waits(&mut self, cx: &BodyCx<'_>) {
        let pool = cx.names.pool.types();
        let waits = std::mem::take(&mut self.waits);
        for (owner, e, written) in waits {
            let Some(RowState::Solved(r)) = self.rows.get(&e.callee) else {
                self.waits.push((owner, e, written));
                continue;
            };
            let need = edge_row(cx, pool, &pool.row_data(*r), &e);
            let have = pool.row_data(written);
            let held = crate::rows::KeyIndex::new(cx, pool, &have.keys);
            let mut missing: Vec<String> = need
                .keys
                .iter()
                .filter(|k| !held.supplies(pool, **k))
                .map(|k| format!("`$ {}`", hd_resolve::show_ty_in(&cx.names, pool, *k)))
                .collect();
            missing.sort_stable();
            let mut params: Vec<String> = need
                .params
                .iter()
                .filter(|p| !have.params.contains(p))
                .map(|p| format!("the row `$ {}`", row_param_name(cx, *p)))
                .collect();
            params.sort_stable();
            missing.extend(params);
            let node = cx.src.parse.tree.node(e.at);
            let span = cx.src.span(node);
            for shown in missing {
                let msg =
                    format!("this needs {shown}, which the enclosing function's row does not name");
                self.diags_of(owner)
                    .error(Code::MissingRequirement, span, &msg);
            }
        }
    }
}

/// One settle's persistent search and per-component blocked verdicts.
#[derive(Default)]
struct ReadySearch {
    walk: hd_base::scc::Online<DefId>,
    blocked: HashMap<DefId, bool>,
}

/// A row parameter's name as its owner declares it.
fn row_param_name(cx: &BodyCx<'_>, p: hd_types::RowParamRef) -> String {
    cx.lookup
        .item(p.owner)
        .and_then(|i| match i.sig() {
            Some(s) => s.generics.get(usize::from(p.index)).map(|g| g.name),
            None => i.generics.get(usize::from(p.index)).map(|g| g.name),
        })
        .map_or_else(|| "R".to_owned(), |n| cx.names.text(n).to_owned())
}

/// What an edge adds to its caller's row: the callee's row with the call's
/// type arguments, less the keys of the `$.with` blocks around the call.
fn edge_row(cx: &BodyCx<'_>, pool: Types<'_>, callee: &RowData, e: &RowEdge) -> RowData {
    let f = |p: ParamRef| {
        e.args
            .iter()
            .find(|(o, _)| *o == p.owner)
            .and_then(|(_, a)| a.get(p.index as usize).copied())
    };
    let row = pool.subst_row(pool.row(callee), &f);
    let mut d = pool.row_data(row);
    let minus = crate::rows::KeyIndex::new(cx, pool, &e.minus);
    d.keys
        .retain(|k| !pool.has_poison(*k) && !minus.supplies(pool, *k));
    d
}

/// The nesting depth of a row key's type, counted to just past
/// `MAX_KEY_DEPTH`. A type reached at a shallower depth adds no larger
/// answer. Each type can be revisited at most `MAX_KEY_DEPTH + 1` times;
/// the first depth past the existing limit finishes the query.
pub(crate) fn depth(pool: Types<'_>, t: Ty, at: u32) -> u32 {
    if at > MAX_KEY_DEPTH {
        return at;
    }
    let mut deepest = HashMap::default();
    let mut parts = Vec::new();
    let mut max = at;
    hd_base::stack::all((t, at), |(t, at), stack| {
        if deepest.get(&t).is_some_and(|&seen| seen >= at) {
            return true;
        }
        deepest.insert(t, at);
        hd_types::work::type_node();
        max = max.max(at + 1);
        if max > MAX_KEY_DEPTH {
            return false;
        }
        parts.clear();
        match pool.get(t) {
            TyData::Fn { params, result, .. } => {
                parts.extend_from_slice(pool.list_items(params));
                parts.push(result);
            }
            // The row limit counts declaration arguments, not structural sum members.
            TyData::Adt(AdtId::Declared(def), _) if def.get().is_some() => {
                pool.children(t, &mut parts);
            }
            TyData::Adt(AdtId::Product { .. }, _)
            | TyData::Ref { .. }
            | TyData::TraitValue { .. } => {
                pool.children(t, &mut parts);
            }
            _ => return true,
        }
        stack.extend(parts.iter().rev().map(|&t| (t, at + 1)));
        true
    });
    max
}

/// Registers the module's private callables whose result type or row is
/// omitted, from its bodies in source order: non-public functions and
/// non-public methods of inherent implementations.
pub fn omitted_results(cx: &BodyCx<'_>, bodies: &[(DefId, NodeRef<'_>)]) {
    let named = template_named(cx, bodies);
    let mut r = cx.results.borrow_mut();
    for &(def, node) in bodies {
        if Src::child(node, SyntaxKind::Block).is_none() {
            continue;
        }
        // A callable a template names has the empty row when it writes
        // none, as a public one does (`module.package.template-private.row`):
        // its row is not inferred.
        let omits = Omits {
            ret: Src::type_child(node).is_none(),
            row: Src::child(node, SyntaxKind::RequirementRow).is_none() && !named.contains(&def),
            at: node.index(),
            position: r.order.len(),
        };
        if !omits.ret && !omits.row {
            continue;
        }
        let Some(item) = cx.lookup.item(def) else {
            continue;
        };
        let private = !item.public
            && match &item.data {
                ItemData::Fn(_, _) => true,
                ItemData::Method { owner, .. } => matches!(
                    cx.lookup.item(*owner).map(|o| &o.data),
                    Some(ItemData::Impl {
                        trait_,
                        kind: ImplKind::Written,
                        ..
                    }) if *trait_ == DefId::NONE
                ),
                _ => false,
            };
        if private {
            r.state.insert(def, State::Pending(node.index()));
            r.omits.insert(def, omits);
            r.order.push(def);
        }
    }
}

/// The module's items that a template body names
/// (`module.package.template-private`): each name in the body of a
/// method of a `by` template that the module scope binds to an item,
/// unless the body binds that name itself.
fn template_named(cx: &BodyCx<'_>, bodies: &[(DefId, NodeRef<'_>)]) -> hd_base::HashSet<DefId> {
    let mut out = hd_base::HashSet::default();
    for &(def, node) in bodies {
        let Some(ItemData::Method { owner, .. }) = cx.lookup.item(def).map(|i| &i.data) else {
            continue;
        };
        let template = matches!(
            cx.lookup.item(*owner).map(|o| &o.data),
            Some(ItemData::Impl {
                kind: ImplKind::Template | ImplKind::TupleTemplate | ImplKind::UnionTemplate,
                ..
            })
        );
        if !template {
            continue;
        }
        let name_of = |n: NodeRef<'_>| {
            n.direct_tokens()
                .find(|t| {
                    matches!(
                        cx.src.parse.tokens.kind(*t),
                        TokenKind::Ident | TokenKind::RawIdent
                    )
                })
                .map(|t| cx.src.text(t))
        };
        let local: hd_base::HashSet<&str> = node
            .descendants()
            .filter(|d| matches!(d.kind(), SyntaxKind::BindingPattern | SyntaxKind::Parameter))
            .filter_map(name_of)
            .collect();
        for n in node
            .descendants()
            .filter(|d| d.kind() == SyntaxKind::NameExpr)
        {
            let Some(name) = name_of(n).filter(|x| !local.contains(x)) else {
                continue;
            };
            if let Some(b) = cx.scope.lookup(cx.names.syms.intern(name))
                && b.kind == hd_resolve::BindingKind::Item
            {
                out.insert(DefId::from_raw(b.value));
            }
        }
    }
    out
}

/// M1's sweep: checks every registered callable in source order. The
/// callables whose omitted results reach no cycle go first, callees
/// before callers, so each call finds its callee done
/// (`crates/hd_check/DESIGN.md` (type checking) §9.2).
pub fn infer_results(cx: &BodyCx<'_>) {
    for def in acyclic_callees_first(cx) {
        seen(cx, def, true);
    }
    let order = cx.results.borrow().order.clone();
    for def in order {
        seen(cx, def, true);
    }
}

/// The module's registered callables as a reference graph, built once on
/// first use. A body refers to each registered callable whose name is one
/// of its identifier tokens. That over-approximates its calls and its
/// reads, so an order that respects these edges respects the real ones.
///
/// A name that several callables share is a node of its own, after the
/// callables, that refers to each of them: a body naming it refers to
/// that one node. So the graph has one edge per name a body holds plus
/// one per callable, however many callables share a name, where an edge
/// from each body to each callable of each name it holds made n bodies
/// that each call one of n same-named methods n² edges (#1889). A path
/// through a name node is a path between the callables it joins, so the
/// callables' reach and cycles are as with the direct edges.
struct Plan {
    /// The callables in source order: a callable's index is its
    /// `Omits::position`. The name nodes follow them.
    defs: Vec<DefId>,
    /// Each node's references: a body's in node order, a name node's the
    /// callables of its name.
    refs: Vec<Vec<u32>>,
    /// The component of each callable with an omitted result, and of each
    /// name node, in the graph of references between them, numbered callees
    /// first; `u32::MAX` for a callable whose result is written.
    comp: Vec<u32>,
    /// The module's top-level binding names each body names: it can be
    /// checked early only once each is declared.
    tops: Vec<Vec<hd_base::Symbol>>,
}

impl Plan {
    /// Whether node `i` is a name node, not a callable.
    fn is_name(&self, i: u32) -> bool {
        i as usize >= self.defs.len()
    }

    fn build(cx: &BodyCx<'_>, r: &Results) -> Plan {
        let defs: Vec<DefId> = r.order.clone();
        let mut by_name: HashMap<&str, Vec<u32>> = HashMap::default();
        let mut texts = Vec::with_capacity(defs.len());
        for (i, d) in defs.iter().enumerate() {
            let text = cx.lookup.item(*d).map(|it| cx.names.text(it.name));
            if let Some(text) = text {
                by_name
                    .entry(text)
                    .or_default()
                    .push(u32::try_from(i).unwrap_or(u32::MAX));
            }
            texts.push(text);
        }
        // What a name refers to: its one callable, or the name node of
        // the callables that share it, numbered in order of their first.
        let mut name_refs: Vec<Vec<u32>> = Vec::new();
        let mut node_of: HashMap<&str, u32> = HashMap::default();
        for text in texts.into_iter().flatten() {
            // Taken at the name's first callable.
            let Some(callees) = by_name.remove(text) else {
                continue;
            };
            let node = if let [one] = callees[..] {
                one
            } else {
                name_refs.push(callees);
                u32::try_from(defs.len() + name_refs.len() - 1).unwrap_or(u32::MAX)
            };
            node_of.insert(text, node);
        }
        let tops_by_name: HashMap<&str, hd_base::Symbol> = {
            let init = cx.init.borrow();
            init.pending
                .keys()
                .map(|s| (cx.names.text(*s), *s))
                .collect()
        };
        let mut refs = Vec::with_capacity(defs.len());
        let mut tops = Vec::with_capacity(defs.len());
        for d in &defs {
            let mut out: Vec<u32> = Vec::new();
            let mut named: Vec<hd_base::Symbol> = Vec::new();
            // A registered callable has a body (`omitted_results`).
            let body = r
                .omits
                .get(d)
                .and_then(|o| Src::child(cx.src.parse.tree.node(o.at), SyntaxKind::Block));
            for t in body.into_iter().flat_map(|b| cx.src.tokens(b)) {
                if !matches!(
                    cx.src.tkind(t),
                    Some(TokenKind::Ident | TokenKind::RawIdent)
                ) {
                    continue;
                }
                let text = cx.src.text(t);
                if let Some(&node) = node_of.get(text) {
                    out.push(node);
                }
                if let Some(sym) = tops_by_name.get(text) {
                    named.push(*sym);
                }
            }
            out.sort_stable();
            out.dedup();
            named.sort_stable_by_key(|s| s.raw());
            named.dedup();
            crate::work::plan_edges(out.len());
            refs.push(out);
            tops.push(named);
        }
        let mut ret: Vec<bool> = defs.iter().map(|d| r.omits[d].ret).collect();
        ret.resize(defs.len() + name_refs.len(), true);
        crate::work::plan_edges(name_refs.iter().map(Vec::len).sum());
        refs.extend(name_refs);
        let mut comp = vec![u32::MAX; refs.len()];
        for (c, members) in components(&refs, &ret).iter().enumerate() {
            for &m in members {
                comp[m as usize] = u32::try_from(c).unwrap_or(u32::MAX);
            }
        }
        Plan {
            defs,
            refs,
            comp,
            tops,
        }
    }
}

/// The strongly connected components of `edges` restricted to the nodes
/// `include` keeps, each after every component it reaches
/// ([`hd_base::scc`]); a component's members in the order Tarjan pops them.
fn components(edges: &[Vec<u32>], include: &[bool]) -> Vec<Vec<u32>> {
    let roots = (0..edges.len())
        .filter(|&v| include[v])
        .map(hd_base::scc::node);
    let sccs = hd_base::scc::sccs_from(edges.len(), roots, |v, out| {
        out.extend(edges[v as usize].iter().filter(|&&w| include[w as usize]));
    });
    sccs.iter()
        .map(|m| m.iter().rev().copied().collect())
        .collect()
}

fn plan(cx: &BodyCx<'_>) -> std::sync::Arc<Plan> {
    let mut r = cx.results.borrow_mut();
    if let Some(p) = &r.plan {
        return std::sync::Arc::clone(p);
    }
    let p = std::sync::Arc::new(Plan::build(cx, &r));
    r.plan = Some(std::sync::Arc::clone(&p));
    p
}

/// The pending callables whose bodies reach no
/// cycle, callees before callers. A callable left out may still reach no
/// cycle, since references over-approximate calls; it keeps its place in
/// the source-order sweep, and so does every cycle's entry.
fn acyclic_callees_first(cx: &BodyCx<'_>) -> Vec<DefId> {
    let plan = plan(cx);
    // A name node passes on to its callables.
    let pending: Vec<bool> = {
        let r = cx.results.borrow();
        let mut pending: Vec<bool> = plan
            .defs
            .iter()
            .map(|d| matches!(r.state.get(d), Some(State::Pending(_))))
            .collect();
        pending.resize(plan.refs.len(), true);
        pending
    };
    let mut cyclic = vec![false; plan.refs.len()];
    let mut out = Vec::new();
    for component in components(&plan.refs, &pending) {
        // A component reaches a cycle when it is one, or when a
        // component it calls does (those came out before it). A name node
        // in a component of two or more lies on a cycle through a
        // callable: one that names itself, if no other.
        let reaches = component.len() > 1
            || component.iter().any(|&c| {
                plan.refs[c as usize]
                    .iter()
                    .any(|&w| pending[w as usize] && (w == c || cyclic[w as usize]))
            });
        for &c in &component {
            cyclic[c as usize] = reaches;
        }
        if !reaches {
            out.extend(
                (component.iter())
                    .filter(|&&c| !plan.is_name(c))
                    .map(|&c| plan.defs[c as usize]),
            );
        }
    }
    out
}

/// Before the omitted result of the callable at `root` (its position) is
/// checked for a caller: checks the pending callables with omitted
/// results that its body reaches, callees
/// first, each from this depth, so a chain of them does not nest native
/// body checks (`crates/hd_check/DESIGN.md` (type checking) §9.2). A
/// callable that names a top-level binding not declared yet, or reaches
/// one that does, waits for its caller: its check depends on the moment
/// of the call. So do the members of its own cycle. Both are checked by
/// the caller's [`drive`] when its check reads them.
/// Returns whether it checked any.
fn callees_first(cx: &BodyCx<'_>, root: usize) -> bool {
    let plan = plan(cx);
    let own = plan.comp[root];
    // A name node outside the root's component counts as a pending
    // callable that is ready once its callables are. One inside it is
    // named only by the root, since whatever names it is on the root's
    // cycle: the walk passes through it to its callables.
    let is_pending_ret = |r: &Results, i: u32| {
        plan.comp[i as usize] != u32::MAX
            && plan.comp[i as usize] != own
            && (plan.is_name(i)
                || matches!(r.state.get(&plan.defs[i as usize]), Some(State::Pending(_))))
    };
    let follows = |r: &Results, i: u32| plan.is_name(i) || is_pending_ret(r, i);
    // The pending callables with omitted results that `def` reaches, and
    // the name nodes on the way.
    let mut reach: Vec<u32> = Vec::new();
    {
        let r = cx.results.borrow();
        let mut visited: hd_base::HashSet<u32> = hd_base::HashSet::default();
        let mut stack: Vec<u32> = plan.refs[root]
            .iter()
            .copied()
            .filter(|&i| follows(&r, i))
            .collect();
        while let Some(i) = stack.pop() {
            if !visited.insert(i) {
                continue;
            }
            if is_pending_ret(&r, i) {
                reach.push(i);
            }
            stack.extend(
                plan.refs[i as usize]
                    .iter()
                    .copied()
                    .filter(|&w| !visited.contains(&w) && follows(&r, w)),
            );
        }
    }
    if reach.is_empty() {
        return false;
    }
    reach.sort_stable_by_key(|&i| (plan.comp[i as usize], i));
    // A component is ready when each member's top-level names are
    // declared and each component it reaches is ready; those come first.
    let mut ready: HashMap<u32, bool> = HashMap::default();
    {
        let init = cx.init.borrow();
        let r = cx.results.borrow();
        let declared =
            |s: &hd_base::Symbol| init.globals.contains_key(s) || init.earlier.contains_key(s);
        let mut at = 0;
        while at < reach.len() {
            let c = plan.comp[reach[at] as usize];
            let end = reach[at..]
                .iter()
                .position(|&i| plan.comp[i as usize] != c)
                .map_or(reach.len(), |k| at + k);
            let ok = reach[at..end].iter().all(|&i| {
                (plan.is_name(i) || plan.tops[i as usize].iter().all(declared))
                    && plan.refs[i as usize].iter().all(|&w| {
                        let cw = plan.comp[w as usize];
                        cw == c
                            || !is_pending_ret(&r, w)
                            || ready.get(&cw).copied().unwrap_or(false)
                    })
            });
            ready.insert(c, ok);
            at = end;
        }
    }
    let mut checked = false;
    for i in reach {
        if plan.is_name(i) || !ready.get(&plan.comp[i as usize]).copied().unwrap_or(false) {
            continue;
        }
        drive(cx, plan.defs[i as usize]);
        checked = true;
    }
    checked
}

/// What callers see of `def` when it is a registered callable. Its body
/// is checked first when it is not done and the caller needs it: for an
/// omitted result, or with `force`. Rows alone never order checking: a
/// call of an open row records an edge. A callee whose omitted result is
/// being checked, or waits, closes a cycle: the error is reported and the
/// call is poison. A body being checked never checks a pending callee: it
/// waits for it ([`drive`]), and the call is poison until then.
fn seen(cx: &BodyCx<'_>, def: DefId, force: bool) -> Option<Seen> {
    let mut prepared = false;
    loop {
        let mut r = cx.results.borrow_mut();
        let omits = *r.omits.get(&def)?;
        if let Some(&reader) = r.active.last() {
            r.readers.entry(def).or_default().insert(reader);
        }
        let row = match r.rows.get(&def) {
            _ if !omits.row => RowSeen::Written,
            Some(RowState::Solved(row)) => RowSeen::Solved(*row),
            _ => RowSeen::Open,
        };
        match r.state.get(&def)? {
            State::Done { ret, .. } => {
                let ret = omits.ret.then_some(*ret);
                return Some(Seen { ret, row });
            }
            State::Active(_) | State::Waiting(_) => {
                let ret = if omits.ret {
                    r.cycle(cx, def);
                    Some(Ty::POISON)
                } else {
                    None
                };
                return Some(Seen { ret, row });
            }
            State::Pending(_) if !omits.ret && !force => {
                return Some(Seen { ret: None, row });
            }
            State::Pending(_) if !r.active.is_empty() => {
                r.blocked.push(def);
                return Some(Seen {
                    ret: omits.ret.then_some(Ty::POISON),
                    row,
                });
            }
            State::Pending(_) if omits.ret && !prepared => {
                drop(r);
                prepared = true;
                callees_first(cx, omits.position);
            }
            State::Pending(_) => {
                drop(r);
                drive(cx, def);
            }
        }
    }
}

/// Checks the pending callable `root` from an explicit stack, never by
/// native recursion (AGENTS.md): a check that read pending callables
/// waits, they are checked above it, callees first in the order it read
/// them, and then it is checked again. Each callable above a waiting one
/// is one it read, directly or through those between, so the waiting
/// ones form the chain from `root` to the active body ([`Results::cycle`]).
/// Cost: a body is checked once, plus once per check that read a
/// callable not checked yet; each such read leaves that callable done
/// before the body runs again, so a body runs at most once more than the
/// distinct pending callables it reads (twice in a chain or a cycle).
fn drive(cx: &BodyCx<'_>, root: DefId) {
    debug_assert!(cx.results.borrow().chain.is_empty(), "one drive at a time");
    let mut stack = vec![root];
    while let Some(&def) = stack.last() {
        let (n, omits) = {
            let mut r = cx.results.borrow_mut();
            let n = match r.state.get(&def) {
                Some(State::Pending(n)) => *n,
                Some(State::Waiting(n)) => {
                    let n = *n;
                    debug_assert_eq!(r.chain.last(), Some(&def), "waits on the chain's end");
                    r.chain.pop();
                    n
                }
                // Done: checked through another reader since it was pushed.
                _ => {
                    stack.pop();
                    continue;
                }
            };
            r.state.insert(def, State::Active(n));
            r.active.push(def);
            // `omitted_results` registers a callable's state together
            // with its omits, and only a registered callable has a state;
            // ruled out: a missing entry.
            (n, r.omits[&def])
        };
        let blocked = check(cx, def, n, omits);
        if blocked.is_empty() {
            stack.pop();
            continue;
        }
        let mut first = hd_base::HashSet::default();
        let fresh: Vec<DefId> = blocked.into_iter().filter(|d| first.insert(*d)).collect();
        stack.extend(fresh.into_iter().rev());
    }
}

/// Checks the body of `def`, which the caller made active, with what its
/// declaration `omits`, and settles what it changed. A check that read
/// pending callables is dropped: `def` waits, and they are returned for
/// [`drive`] to check first.
fn check(cx: &BodyCx<'_>, def: DefId, node: NodeIdx, omits: Omits) -> Vec<DefId> {
    let at = node;
    // Its own diagnostics: a caller's trial cannot roll them back, and a
    // later row pass replaces them together with this provisional body.
    let mut diags = DiagBuf::default();
    let node = cx.src.parse.tree.node(node);
    let infer = Infer {
        ret: omits.ret,
        row: omits.row,
        budget: cx.results.borrow().remaining.get(&def).copied(),
    };
    // A body that stops with "not implemented" has the empty row.
    let (checked, ret, facts) = match check_fn_body(cx, def, node, &mut diags, (None, None), infer)
    {
        Ok((b, t, f)) => (Ok(b), t, f),
        Err(e) => (Err(e), Ty::POISON, RowFacts::default()),
    };
    let mut r = cx.results.borrow_mut();
    r.active.pop();
    let blocked = std::mem::take(&mut r.blocked);
    if !blocked.is_empty() {
        r.state.insert(def, State::Waiting(at));
        r.chain.push(def);
        return blocked;
    }
    if let Some(left) = facts.fuel_left {
        r.remaining.insert(def, left);
    }
    let previous_error = r.body_errors.contains(&def);
    let has_errors = omits.ret && diags.has_errors();
    if has_errors {
        r.body_errors.insert(def);
    } else {
        r.body_errors.remove(&def);
    }
    let old_ret = r.returns.insert(def, ret);
    r.state.insert(def, State::Done { ret });
    r.settled_since.push(def);
    r.bodies.insert(def, Box::new((checked, diags)));
    if omits.row {
        let old_row = match r.rows.get(&def) {
            Some(RowState::Solved(row)) => Some(*row),
            _ => None,
        };
        let state = if facts.edges.is_empty() {
            RowState::Solved(cx.names.pool.row(&facts.used))
        } else {
            RowState::Waiting {
                used: facts.used,
                edges: facts.edges,
            }
        };
        if let RowState::Waiting { edges, .. } = &state {
            let callees: Vec<DefId> = edges.iter().map(|e| e.callee).collect();
            for &c in &callees {
                r.waiters.entry(c).or_default().push(def);
            }
            r.wait_callees.insert(def, callees);
            r.undone_hint.remove(&def);
            r.waiting.insert(def);
        } else {
            r.waiting.remove(&def);
        }
        r.rows.insert(def, state);
        let new_row = match r.rows.get(&def) {
            Some(RowState::Solved(row)) => Some(*row),
            _ => None,
        };
        if row_changed(old_row, new_row) {
            r.changed(def);
        }
    }
    if omits.ret && old_ret.is_some_and(|old| old != ret) {
        r.changed(def);
    }
    if previous_error != has_errors {
        r.changed(def);
    }
    // Rows settle once the drive's chain is done: a row on it waits for
    // the chain's first member, so settling after each check would visit
    // every waiting row once per check.
    if r.chain.is_empty() {
        r.settle(cx);
    }
    drop(r);
    drain_rows(cx);
    recheck_readers(cx);
    Vec::new()
}

/// Force the row signatures requested by a body without nesting body
/// checks. Requests made while draining append to the same queue. Only
/// after it empties may the indexed reader worklist publish final bodies.
/// Each pending declaration is forced once, independent of ring length.
fn drain_rows(cx: &BodyCx<'_>) {
    {
        let mut r = cx.results.borrow_mut();
        if r.draining_rows
            || !r.active.is_empty()
            || !r.chain.is_empty()
            || r.deferred_rows.is_empty()
        {
            return;
        }
        r.draining_rows = true;
    }
    loop {
        let def = {
            let mut r = cx.results.borrow_mut();
            let Some(def) = r.deferred_rows.pop_front() else {
                r.draining_rows = false;
                return;
            };
            r.deferred_queued.remove(&def);
            def
        };
        seen(cx, def, true);
    }
}

/// Drain the affected signature readers after the active check unwinds.
/// Keeping the last solved row while rechecking gives recursive function
/// values the same approximation as direct calls. Discard the old body;
/// only a body checked against stable signatures is published by M1.
fn recheck_readers(cx: &BodyCx<'_>) {
    {
        let mut r = cx.results.borrow_mut();
        if r.rechecking
            || r.draining_rows
            || !r.active.is_empty()
            || !r.chain.is_empty()
            || r.dirty.is_empty()
        {
            return;
        }
        r.rechecking = true;
    }
    loop {
        let def = {
            let mut r = cx.results.borrow_mut();
            let Some(def) = r.dirty.pop_front() else {
                r.rechecking = false;
                return;
            };
            r.queued.remove(&def);
            if r.remaining.get(&def) == Some(&0) {
                continue;
            }
            let at = r.omits[&def].at;
            r.state.insert(def, State::Pending(at));
            r.bodies.remove(&def);
            def
        };
        seen(cx, def, true);
    }
}

/// Whether `def` is a registered callable whose row is still open: a call
/// of it records an edge (`crates/hd_tir/DESIGN.md` "M3: inferred rows").
pub(crate) fn row_open(cx: &BodyCx<'_>, def: DefId) -> bool {
    cx.results.borrow().row_open(def)
}

pub(crate) fn rechecking_rows(cx: &BodyCx<'_>) -> bool {
    cx.results.borrow().rechecking
}

/// The current row for a closure call or function value: check its body
/// first when needed. An unfinished recursive group starts at empty;
/// signature readers are rechecked when its rows grow. A callable outside
/// inference keeps its written row.
pub(crate) fn row_now(cx: &BodyCx<'_>, def: DefId, written: RowId) -> RowId {
    loop {
        let active = !cx.results.borrow().active.is_empty();
        let Some(view) = seen(cx, def, !active) else {
            return written;
        };
        match view.row {
            RowSeen::Solved(row) => return row,
            RowSeen::Written => return written,
            RowSeen::Open if active => {
                cx.results.borrow_mut().defer_row(def);
                return RowId::EMPTY;
            }
            RowSeen::Open => {}
        }
        // Its row waits on bodies not checked yet: check them too.
        let pending = cx.results.borrow().pending_reach(def);
        if pending.is_empty() {
            // The active recursive group has no complete row yet. Its
            // signature readers will be rechecked when these rows grow.
            return RowId::EMPTY;
        }
        for p in pending {
            seen(cx, p, true);
        }
    }
}

/// A body's calls of open rows under its written row (`owner`'s, or the
/// top-level statements'), each checked once its callee's row is solved.
pub(crate) fn add_waits(cx: &BodyCx<'_>, owner: DefId, waits: Vec<(RowEdge, RowId)>) {
    let mut r = cx.results.borrow_mut();
    // A check that waits for a callable is dropped, and its next check
    // adds them.
    if !r.blocked.is_empty() {
        return;
    }
    r.waits
        .extend(waits.into_iter().map(|(e, row)| (owner, e, row)));
    r.check_waits(cx);
}

/// The body M1 checked for `def` and its diagnostics, once.
pub fn take(cx: &BodyCx<'_>, def: DefId) -> Option<(StageResult<Body>, DiagBuf)> {
    let (checked, diags) = *cx.results.borrow_mut().bodies.remove(&def)?;
    let checked = match checked {
        Ok(mut body) if !diags.has_errors() => {
            crate::providers::finalize(cx, &mut body).map(|()| body)
        }
        other => other,
    };
    Some((checked, diags))
}

/// The module-level reports, once: the cycles, and the top-level
/// statements' calls whose solved rows they do not entail.
pub fn module_diags(cx: &BodyCx<'_>) -> DiagBuf {
    std::mem::take(&mut cx.results.borrow_mut().module_diags)
}

/// One callable's inferred parts: what the module's items carry as its
/// result type and row (`crates/hd_tir/DESIGN.md` §4.13.1).
#[derive(Clone, Copy)]
pub struct Inferred {
    pub def: DefId,
    pub ret: Option<Ty>,
    pub row: Option<RowId>,
}

/// Every inferred result and row, in source order.
pub fn inferred(cx: &BodyCx<'_>) -> Vec<Inferred> {
    let r = cx.results.borrow();
    r.order
        .iter()
        .filter_map(|d| {
            let omits = r.omits.get(d)?;
            let Some(State::Done { ret, .. }) = r.state.get(d) else {
                return None;
            };
            let row = match r.rows.get(d) {
                Some(RowState::Solved(row)) if omits.row => Some(*row),
                _ => None,
            };
            Some(Inferred {
                def: *d,
                ret: omits.ret.then_some(*ret),
                row,
            })
        })
        .collect()
}

impl Ck<'_, '_> {
    /// A callee's signature as its callers see it: an omitted result type
    /// is the inferred one, and an omitted row the solved one. An open row
    /// reads as empty here; a call of it records an edge
    /// (`Ck::callee_row`).
    pub(crate) fn with_result(&self, def: DefId, mut sig: FnSig) -> FnSig {
        (sig.ret, sig.row) = self.result_row(def, sig.ret, sig.row);
        sig
    }

    /// A callee's result and row after applying their inferred values,
    /// without cloning the rest of its signature.
    pub(crate) fn result_row(&self, def: DefId, mut ret: Ty, mut row: RowId) -> (Ty, RowId) {
        if self.in_template.is_some() {
            self.cx.results.borrow_mut().template_names(self.cx, def);
        }
        if let Some(s) = seen(self.cx, def, false) {
            if let Some(solved) = s.ret {
                ret = solved;
            }
            match s.row {
                RowSeen::Written => {}
                RowSeen::Solved(solved) => row = solved,
                RowSeen::Open => row = RowId::EMPTY,
            }
        }
        (ret, row)
    }
}

/// Notes that the template `imp` calls `Structure`'s `walk`, `describe` or
/// `build` (`annot.template.body`).
pub(crate) fn note_structure_read(cx: &BodyCx<'_>, imp: DefId) {
    cx.results.borrow_mut().structure_reads.insert(imp);
}

/// Whether a method of the template `imp` calls `Structure`'s `walk`,
/// `describe` or `build`.
#[must_use]
pub fn reads_structure(cx: &BodyCx<'_>, imp: DefId) -> bool {
    cx.results.borrow().structure_reads.contains(&imp)
}

/// Signature-change bookkeeping for `tests/unit` (feature `testing`).
#[cfg(feature = "testing")]
impl Results {
    /// Registers `def`, whose row is omitted and solved empty, declared at
    /// `at` in source position `position`.
    pub fn solved_row(&mut self, def: DefId, at: NodeIdx, position: usize) {
        let omits = Omits {
            ret: false,
            row: true,
            at,
            position,
        };
        self.omits.insert(def, omits);
        self.rows.insert(def, RowState::Solved(RowId::EMPTY));
    }

    /// Records that `reader`'s body read `def`'s signature.
    pub fn read_signature(&mut self, def: DefId, reader: DefId) {
        self.readers.entry(def).or_default().insert(reader);
    }

    /// [`Results::changed`]: the readers visited.
    pub fn signature_changed(&mut self, def: DefId) -> usize {
        self.changed(def)
    }

    /// The bodies queued to check again, in order.
    #[must_use]
    pub fn dirty(&self) -> Vec<DefId> {
        self.dirty.iter().copied().collect()
    }
}
