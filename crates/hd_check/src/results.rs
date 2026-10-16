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
//! callees first, then the rest in source order, depth first when a body
//! reads the signature of one that is not done. A callee
//! whose omitted result is still being checked closes a cycle
//! (`fn.decl.omitted-cycle`), reported once at the cycle's member that
//! comes first in source order (`fn.decl.omitted-cycle.report`). A callee
//! whose omitted row is still open is no error: the call records an edge,
//! and the rows of the recursive group are the least solution of its edges
//! once no member is being checked (`req.row.omitted.cycle`).

use hd_base::HashMap;

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
const MAX_KEY_DEPTH: u32 = 32;

enum State {
    /// Not checked yet: its declaration.
    Pending(NodeIdx),
    /// Its body is being checked.
    Active(NodeIdx),
    /// Checked: its result, and its body with the body's diagnostics until
    /// `Body(m)` takes them.
    Done {
        ret: Ty,
        body: Option<Box<(StageResult<Body>, DiagBuf)>>,
    },
}

/// What a registered callable's declaration leaves out.
#[derive(Clone, Copy)]
struct Omits {
    ret: bool,
    row: bool,
    /// The callable's declaration.
    at: NodeIdx,
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
}

/// An omitted row after its body is checked.
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
    omits: HashMap<DefId, Omits>,
    rows: HashMap<DefId, RowState>,
    /// Written rows' calls of open rows: the caller, the edge, its row.
    waits: Vec<(DefId, RowEdge, RowId)>,
    /// The callables in source order.
    order: Vec<DefId>,
    /// The callables being checked, innermost last.
    active: Vec<DefId>,
    /// The `recursive-function-needs-result-type` reports, and those of
    /// the module's top-level statements' calls of open rows.
    module_diags: DiagBuf,
    reported: Vec<DefId>,
    /// The callables a template named without a declared result type,
    /// each reported once (`module.package.template-private.result`).
    template_named: Vec<DefId>,
    /// The templates with a method that calls `walk`, `describe` or
    /// `build` of `Structure` (`annot.template.body`).
    structure_reads: hd_base::HashSet<DefId>,
    /// Whether a body with an inferred result or a cycle reported an error.
    errors: bool,
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

impl Results {
    /// Whether an inferred result's body or a cycle reported an error, so
    /// a body whose types hold poison from it adds no `cannot-infer-type`.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.errors
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

    /// One report per cycle, at its member first in source order.
    fn cycle(&mut self, cx: &BodyCx<'_>, def: DefId) {
        let Some(at) = self.active.iter().position(|d| *d == def) else {
            return;
        };
        let Some(first) = self.active[at..]
            .iter()
            .copied()
            .min_by_key(|d| self.order.iter().position(|o| o == d))
        else {
            return;
        };
        self.errors = true;
        if self.reported.contains(&first) {
            return;
        }
        self.reported.push(first);
        let Some(State::Active(n)) = self.state.get(&first) else {
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
        match self.state.get_mut(&owner) {
            Some(State::Done { body: Some(b), .. }) => &mut b.1,
            _ => &mut self.module_diags,
        }
    }

    /// After a body is checked: solves the rows that no longer wait on a
    /// body, then checks the written rows that waited on them.
    fn settle(&mut self, cx: &BodyCx<'_>) {
        self.solve_ready(cx);
        self.check_waits(cx);
    }

    /// Solves every waiting row whose edges reach only checked callables:
    /// the least rows that satisfy each member of the group
    /// (`req.row.omitted.cycle`), by a monotone pass over the edges in
    /// source order until nothing grows.
    fn solve_ready(&mut self, cx: &BodyCx<'_>) {
        let waiting: Vec<DefId> = self
            .order
            .iter()
            .copied()
            .filter(|d| matches!(self.rows.get(d), Some(RowState::Waiting { .. })))
            .collect();
        if waiting.is_empty() {
            return;
        }
        // A waiting row is blocked when an edge reaches a callable not
        // checked yet, directly or through another blocked row.
        let mut blocked: Vec<DefId> = Vec::new();
        loop {
            let before = blocked.len();
            for d in &waiting {
                if blocked.contains(d) {
                    continue;
                }
                let Some(RowState::Waiting { edges, .. }) = self.rows.get(d) else {
                    continue;
                };
                if edges.iter().any(|e| {
                    !matches!(self.state.get(&e.callee), Some(State::Done { .. }))
                        || blocked.contains(&e.callee)
                }) {
                    blocked.push(*d);
                }
            }
            if blocked.len() == before {
                break;
            }
        }
        let group: Vec<DefId> = waiting
            .into_iter()
            .filter(|d| !blocked.contains(d))
            .collect();
        if group.is_empty() {
            return;
        }
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
            let mut missing: Vec<String> = need
                .keys
                .iter()
                .filter(|k| {
                    !have
                        .keys
                        .iter()
                        .any(|h| crate::rows::key_supplies(cx.lookup, pool, *h, **k))
                })
                .map(|k| format!("`$ {}`", hd_resolve::show_ty_in(&cx.names, pool, *k)))
                .collect();
            missing.sort();
            let mut params: Vec<String> = need
                .params
                .iter()
                .filter(|p| !have.params.contains(p))
                .map(|p| format!("the row `$ {}`", row_param_name(cx, *p)))
                .collect();
            params.sort();
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
    d.keys.retain(|k| {
        !pool.has_poison(*k)
            && !e
                .minus
                .iter()
                .any(|m| crate::rows::key_supplies(cx.lookup, pool, *m, *k))
    });
    d
}

/// The nesting depth of a row key's type, counted to just past `limit`.
fn depth(pool: Types<'_>, t: Ty, at: u32) -> u32 {
    if at > MAX_KEY_DEPTH {
        return at;
    }
    let parts: Vec<Ty> = match pool.get(t) {
        TyData::Adt { args, .. } => pool.list_items(args).to_vec(),
        TyData::Tuple { elems, rest } => {
            let mut p = pool.list_items(elems).to_vec();
            p.extend(rest);
            p
        }
        TyData::Option(i) | TyData::Mut(i) => vec![i],
        TyData::Fn { params, result, .. } => {
            let mut p = pool.list_items(params).to_vec();
            p.push(result);
            p
        }
        TyData::TraitValue { args, bindings, .. } => {
            let mut p = pool.list_items(args).to_vec();
            p.extend(bindings.into_iter().map(|(_, b)| b));
            p
        }
        _ => Vec::new(),
    };
    parts
        .into_iter()
        .map(|p| depth(pool, p, at + 1))
        .max()
        .unwrap_or(at + 1)
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
        };
        if !omits.ret && !omits.row {
            continue;
        }
        let Some(item) = cx.lookup.item(def) else {
            continue;
        };
        let private = !item.public
            && match &item.data {
                ItemData::Fn(_) => true,
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
                kind: ImplKind::Template | ImplKind::TupleTemplate,
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
/// before callers, so each call finds its callee done and a long chain
/// checks without native recursion (`crates/hd_check/DESIGN.md` (type
/// checking) §9.2).
pub fn infer_results(cx: &BodyCx<'_>) {
    for def in acyclic_callees_first(cx) {
        seen(cx, def, true);
    }
    let order = cx.results.borrow().order.clone();
    for def in order {
        seen(cx, def, true);
    }
}

/// The pending callables with an omitted result whose bodies reach no
/// cycle, callees before callers. A body refers to each such callable
/// whose name is one of its identifier tokens. That over-approximates its
/// calls, so a callable left out may still reach no cycle; it keeps its
/// place in the source-order sweep, and so does every cycle's entry.
fn acyclic_callees_first(cx: &BodyCx<'_>) -> Vec<DefId> {
    let defs: Vec<(DefId, NodeIdx)> = {
        let r = cx.results.borrow();
        r.order
            .iter()
            .filter_map(|d| {
                let o = r.omits.get(d)?;
                let pending = matches!(r.state.get(d), Some(State::Pending(_)));
                (o.ret && pending).then_some((*d, o.at))
            })
            .collect()
    };
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::default();
    for (i, (d, _)) in defs.iter().enumerate() {
        if let Some(it) = cx.lookup.item(*d) {
            by_name.entry(cx.names.text(it.name)).or_default().push(i);
        }
    }
    let edges: Vec<Vec<usize>> = defs
        .iter()
        .map(|&(_, at)| {
            let node = cx.src.parse.tree.node(at);
            let Some(body) = Src::child(node, SyntaxKind::Block) else {
                return Vec::new();
            };
            let mut out: Vec<usize> = cx
                .src
                .tokens(body)
                .filter(|&t| {
                    matches!(
                        cx.src.tkind(t),
                        Some(TokenKind::Ident | TokenKind::RawIdent)
                    )
                })
                .filter_map(|t| by_name.get(cx.src.text(t)))
                .flatten()
                .copied()
                .collect();
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect();
    // Tarjan's strongly connected components with an explicit stack: each
    // component comes out after every component it reaches.
    let n = defs.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut cyclic = vec![false; n];
    let mut next = 0;
    let mut out = Vec::new();
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut work = vec![(root, 0)];
        while let Some(&(v, i)) = work.last() {
            if let Some(&w) = edges[v].get(i) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] != index[v] {
                continue;
            }
            let mut component = Vec::new();
            while let Some(w) = stack.pop() {
                on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            // A component reaches a cycle when it is one, or when a
            // component it calls does (those came out before it).
            let reaches = component.len() > 1
                || component
                    .iter()
                    .any(|&c| edges[c].iter().any(|&w| w == c || cyclic[w]));
            for &c in &component {
                cyclic[c] = reaches;
            }
            if !reaches {
                out.push(defs[v].0);
            }
        }
    }
    out
}

/// What callers see of `def` when it is a registered callable. Its body
/// is checked first when it is not done and the caller needs it: for an
/// omitted result, or with `force`. Rows alone never order checking: a
/// call of an open row records an edge. A callee whose omitted result is
/// being checked closes a cycle: the error is reported and the call is
/// poison.
fn seen(cx: &BodyCx<'_>, def: DefId, force: bool) -> Option<Seen> {
    let (node, omits) = {
        let mut r = cx.results.borrow_mut();
        let omits = *r.omits.get(&def)?;
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
            State::Active(_) => {
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
            State::Pending(n) => {
                let n = *n;
                r.state.insert(def, State::Active(n));
                r.active.push(def);
                (n, omits)
            }
        }
    };
    // Its own diagnostics: a trial that reads this callable rolls its own
    // back, and the body is checked only here.
    let mut diags = DiagBuf::default();
    let node = cx.src.parse.tree.node(node);
    let infer = Infer {
        ret: omits.ret,
        row: omits.row,
    };
    // A body that stops with "not implemented" has the empty row.
    let (checked, ret, facts) = match check_fn_body(cx, def, node, &mut diags, (None, None), infer)
    {
        Ok((b, t, f)) => (Ok(b), t, f),
        Err(e) => (Err(e), Ty::POISON, RowFacts::default()),
    };
    let mut r = cx.results.borrow_mut();
    r.active.pop();
    r.errors |= omits.ret && diags.has_errors();
    r.state.insert(
        def,
        State::Done {
            ret,
            body: Some(Box::new((checked, diags))),
        },
    );
    if omits.row {
        let state = if facts.edges.is_empty() {
            RowState::Solved(cx.names.pool.row(&facts.used))
        } else {
            RowState::Waiting {
                used: facts.used,
                edges: facts.edges,
            }
        };
        r.rows.insert(def, state);
    }
    r.settle(cx);
    drop(r);
    seen(cx, def, false)
}

/// Whether `def` is a registered callable whose row is still open: a call
/// of it records an edge (`crates/hd_tir/DESIGN.md` "M3: inferred rows").
pub(crate) fn row_open(cx: &BodyCx<'_>, def: DefId) -> bool {
    cx.results.borrow().row_open(def)
}

/// `def`'s solved row for a use that needs it now, such as a closure's
/// call or a function value: its body is checked first if it is not
/// done. `None` while its recursive group is being checked.
pub(crate) fn row_now(cx: &BodyCx<'_>, def: DefId) -> Option<RowId> {
    loop {
        match seen(cx, def, true)?.row {
            RowSeen::Solved(row) => return Some(row),
            RowSeen::Written => return None,
            RowSeen::Open => {}
        }
        // Its row waits on bodies not checked yet: check them too.
        let pending = cx.results.borrow().pending_reach(def);
        if pending.is_empty() {
            return None;
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
    r.waits
        .extend(waits.into_iter().map(|(e, row)| (owner, e, row)));
    r.check_waits(cx);
}

/// The body M1 checked for `def` and its diagnostics, once.
pub fn take(cx: &BodyCx<'_>, def: DefId) -> Option<(StageResult<Body>, DiagBuf)> {
    match cx.results.borrow_mut().state.get_mut(&def) {
        Some(State::Done { body, .. }) => body.take().map(|b| *b),
        _ => None,
    }
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
        if self.in_template.is_some() {
            self.cx.results.borrow_mut().template_names(self.cx, def);
        }
        if let Some(s) = seen(self.cx, def, false) {
            if let Some(ret) = s.ret {
                sig.ret = ret;
            }
            match s.row {
                RowSeen::Written => {}
                RowSeen::Solved(row) => sig.row = row,
                RowSeen::Open => sig.row = RowId::EMPTY,
            }
        }
        sig
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
