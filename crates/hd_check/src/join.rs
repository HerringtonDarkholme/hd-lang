//! The least common type (spec 04 "Least Common Type", `types.lct.*`): the
//! one join every least-common-type site uses, and the narrower join of a
//! generic call's arguments (`types.generic.infer.join`).
//!
//! A site checks its values in order against a join variable. The first
//! value solves the variable; each later value meets the type so far by
//! the conversions `types.lct.conversions` lists, and only those:
//! permission weakening, at the top and in tuple elements, and optional
//! injection of one layer. A generic type's arguments meet only as one
//! type, since weakening under a variance step is the combination
//! `types.lct.no-combine` rules out. A value of `never` is dropped
//! (`types.lct.never.dropped`). Function values take the union of their
//! rows (`types.lct.row-union-every-site`). When no unique least type
//! exists, the value that does not meet the others reports
//! `no-common-type`.

use hd_base::{HashMap, HashSet};

use hd_base::{DefId, NodeIdx, StageResult};
use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_tir::ir::{Coercion, NONE, Ref, Slot, Tag, TirSink};
use hd_types::{Ty, TyData, VarKind};

use crate::body::Ck;

/// The least-common-type sites of a body, by join variable. A site keeps
/// the values joined so far, each reaching the site's type by one admitted
/// conversion; they are compared again only when that type changes, which
/// happens a bounded number of times per site, so a value that leaves it
/// unchanged costs a constant. An undo log lets a trial roll back.
#[derive(Default)]
pub(crate) struct Joins {
    sites: HashMap<Ty, Site>,
    log: Vec<Undo>,
}

#[derive(Default)]
struct Site {
    /// The values joined so far, in order.
    values: Vec<Joined>,
    /// Inferred return sites keep each `?`'s required outer result kind.
    /// Later optional widening must not turn a Result target into an optional.
    propagation: Vec<(bool, NodeIdx)>,
    /// Unknown-kind operands, lowered at the end of this function's body.
    deferred: Vec<DeferredTry>,
    /// A value did not join and was reported: the site reports once.
    failed: bool,
}

/// A value joined at a site: its type, unresolved, and where its
/// conversion to the site's final type goes (§4.3 step 6).
#[derive(Clone, Copy)]
struct Joined {
    ty: Ty,
    to: (Slot, Ref, NodeIdx),
}

#[derive(Clone, Copy)]
struct DeferredTry {
    operand: (Ref, Ty),
    payload: Ty,
    slot: Slot,
    at: (NodeIdx, NodeIdx),
}

enum Undo {
    Open(Ty),
    Value(Ty),
    Propagation(Ty),
    Deferred(Ty),
    Failed(Ty),
    Closed(Ty, Site),
}

impl Joins {
    /// Whether `var` is a site's join variable.
    pub(crate) fn is_site(&self, var: Ty) -> bool {
        self.sites.contains_key(&var)
    }

    fn open(&mut self, var: Ty) {
        self.sites.insert(var, Site::default());
        self.log.push(Undo::Open(var));
    }

    /// Closes `var`'s site once its slots are filled: its variable is an
    /// ordinary type from here on, so a later value of that type, such as
    /// one assigned to a binding the site initialized, converts as into
    /// any other type instead of reserving a slot nothing fills.
    fn close(&mut self, var: Ty) {
        if let Some(site) = self.sites.remove(&var) {
            self.log.push(Undo::Closed(var, site));
        }
    }

    fn push(&mut self, var: Ty, ty: Ty, to: (Slot, Ref, NodeIdx)) {
        if let Some(site) = self.sites.get_mut(&var) {
            site.values.push(Joined { ty, to });
            self.log.push(Undo::Value(var));
        }
    }

    /// A propagated return at this inferred site's original source node.
    /// `optional` distinguishes an Option operand from a Result operand.
    pub(crate) fn propagate(&mut self, var: Ty, optional: bool, at: NodeIdx) {
        if let Some(site) = self.sites.get_mut(&var) {
            site.propagation.push((optional, at));
            self.log.push(Undo::Propagation(var));
        }
    }

    fn defer(&mut self, var: Ty, deferred: DeferredTry) {
        if let Some(site) = self.sites.get_mut(&var) {
            site.deferred.push(deferred);
            self.log.push(Undo::Deferred(var));
        }
    }

    fn deferred(&self, var: Ty) -> &[DeferredTry] {
        self.sites.get(&var).map_or(&[], |site| &site.deferred)
    }

    fn fail(&mut self, var: Ty) {
        if let Some(site) = self.sites.get_mut(&var)
            && !site.failed
        {
            site.failed = true;
            self.log.push(Undo::Failed(var));
        }
    }

    fn failed(&self, var: Ty) -> bool {
        self.sites.get(&var).is_some_and(|s| s.failed)
    }

    fn values(&self, var: Ty) -> &[Joined] {
        self.sites.get(&var).map_or(&[], |s| &s.values)
    }

    /// A position in the log to roll back to.
    pub(crate) fn mark(&self) -> usize {
        self.log.len()
    }

    /// Undoes everything logged after `mark`.
    pub(crate) fn rollback(&mut self, mark: usize) {
        while self.log.len() > mark {
            match self.log.pop() {
                Some(Undo::Open(v)) => {
                    self.sites.remove(&v);
                }
                Some(Undo::Value(v)) => {
                    if let Some(site) = self.sites.get_mut(&v) {
                        site.values.pop();
                    }
                }
                Some(Undo::Propagation(v)) => {
                    if let Some(site) = self.sites.get_mut(&v) {
                        site.propagation.pop();
                    }
                }
                Some(Undo::Deferred(v)) => {
                    if let Some(site) = self.sites.get_mut(&v) {
                        site.deferred.pop();
                    }
                }
                Some(Undo::Failed(v)) => {
                    if let Some(site) = self.sites.get_mut(&v) {
                        site.failed = false;
                    }
                }
                Some(Undo::Closed(v, site)) => {
                    self.sites.insert(v, site);
                }
                None => break,
            }
        }
    }
}

/// How deep shape unification walks before ordinary unification takes over.
const DEPTH: u32 = 32;

impl Ck<'_, '_> {
    /// A `?` whose operand kind awaits this function's inferred result.
    /// Its payload still participates in ordinary checking immediately.
    pub(crate) fn defer_try(
        &mut self,
        target: Ty,
        operand: (Ref, Ty),
        payload: Ty,
        n: NodeRef<'_>,
        e: NodeRef<'_>,
    ) -> Ref {
        let slot = self.b.reserve();
        let r = slot.as_ref();
        self.b.body_mut().ty[r.0 as usize] = payload;
        self.joins.defer(
            target,
            DeferredTry {
                operand,
                payload,
                slot,
                at: (n.index(), e.index()),
            },
        );
        r
    }

    /// Finish only this function's deferred operands, before its root
    /// block closes, so their child blocks stay in that function.
    pub(crate) fn finish_tries(&mut self, target: Ty) -> StageResult<()> {
        let pool = self.pool();
        let deferred = self.joins.deferred(target).to_vec();
        for d in deferred {
            let ret = self.infer.resolve(pool, target);
            self.charge()?;
            let n = self.cx.src.parse.tree.node(d.at.0);
            let e = self.cx.src.parse.tree.node(d.at.1);
            let known = self.cx.names.known;
            let hint = match pool.get(ret) {
                TyData::Adt { def, .. } if def == known.option => Some(self.optional(d.payload)),
                TyData::Adt { def, args } if def == known.result => {
                    Some(pool.intern_ty(&TyData::Adt {
                        def,
                        args: pool.list(&[d.payload, pool.list_items(args)[1]]),
                    }))
                }
                _ => None,
            };
            if let Some(hint) = hint {
                // A hint solves only open parts, as at the original
                // operand. Later context may have fixed a different error
                // type; that error still joins without From conversion.
                if pool.has_infer(self.infer.resolve(pool, d.operand.1)) {
                    let snap = self.infer.snapshot();
                    if self.infer.unify(pool, d.operand.1, hint).is_err() {
                        self.infer.rollback(snap);
                    }
                }
                let t = self.infer.resolve(pool, d.operand.1);
                let ret = self.try_return(target, t, ret, n);
                // A Scope's child block may be forward, unlike value
                // operands. Keep lowering and any payload conversion in
                // that block; the original slot runs it at the `?`.
                let mark = self.b.open_block();
                let (value, ty) = self.lower_try(n, e, (d.operand.0, t), (target, ret))?;
                let value = self.coerce(
                    value,
                    ty,
                    d.payload,
                    n,
                    "propagation value",
                    crate::body::ExpectPosition::General,
                );
                let block = self.b.close_block(mark, Some(value), d.payload, n.index());
                let cleanups = self.b.refs_record(&[]);
                self.b
                    .fill(d.slot, Tag::Scope, block.0, cleanups, d.payload, n.index());
            } else {
                if !pool.has_poison(ret) {
                    self.err(
                        Code::InvalidResultPropagation,
                        n,
                        "the inferred function result is not a target for this `?`",
                    );
                }
                self.infer.rebind(pool, d.operand.1, Ty::POISON);
                self.infer.rebind(pool, d.payload, Ty::POISON);
                self.b
                    .fill(d.slot, Tag::Poison, NONE, NONE, Ty::POISON, n.index());
            }
        }
        Ok(())
    }

    /// The type a least-common-type site (`types.lct.sites`) joins its
    /// values into: the expected type, or else a fresh join variable. A
    /// variable not yet solved, such as a generic argument's, is no
    /// expected type: the site joins on its own and the caller solves the
    /// variable from the result.
    pub(crate) fn join_target(&mut self, want: Option<Ty>) -> Ty {
        let pool = self.pool();
        if let Some(w) = want
            && (self.strict_opaque_slot(w)
                || !matches!(pool.get(self.infer.shallow(pool, w)), TyData::Infer(_))
                || self.joins.is_site(w))
        {
            return w;
        }
        let v = self.infer.fresh(pool, VarKind::General);
        self.joins.open(v);
        v
    }

    /// Checks a least-common-type site's values with `run`, which gets the
    /// site's type, once each (§4.3). An optional value after plain ones
    /// widens the site's type to the optional one (`types.lct.optional`);
    /// when the site closes, each value's reserved slot gets its
    /// conversion to the final type, so the values checked before the
    /// widening are wrapped there.
    pub(crate) fn join_site<T>(
        &mut self,
        want: Option<Ty>,
        run: impl FnOnce(&mut Self, Ty) -> StageResult<T>,
    ) -> StageResult<T> {
        let result = self.join_target(want);
        // A site the caller's own site passed on is filled by that site.
        if !self.joins.is_site(result) || want == Some(result) {
            return run(self, result);
        }
        let out = run(self, result)?;
        self.fill_join(result);
        self.joins.close(result);
        Ok(out)
    }

    /// Fills the slots of `var`'s site with each value's conversion to the
    /// site's final type: `WrapSome` for a value one optional layer short,
    /// else the static conversion the value takes, or `Identity`.
    fn fill_join(&mut self, var: Ty) {
        let pool = self.pool();
        let site = self.infer.resolve(pool, var);
        let propagation = self
            .joins
            .sites
            .get(&var)
            .filter(|_| !self.joins.failed(var))
            .map_or_else(Vec::new, |s| s.propagation.clone());
        for (optional, at) in propagation {
            let valid = match pool.get(site) {
                TyData::Poison => true,
                TyData::Adt { def, .. } if def == self.cx.names.known.option => optional,
                TyData::Adt { def, .. } if def == self.cx.names.known.result => !optional,
                _ => false,
            };
            if !valid {
                let node = self.cx.src.parse.tree.node(at);
                self.err(
                    Code::InvalidResultPropagation,
                    node,
                    "the inferred function result is not a compatible target for this `?`",
                );
            }
        }
        let values = self.joins.values(var).to_vec();
        for v in values {
            let g = self.infer.resolve(pool, v.ty);
            let kind = if self.option_layers(self.resolved(g)) < self.option_layers(site) {
                Coercion::WrapSome
            } else if g == site || pool.has_poison(g) || pool.has_poison(site) {
                Coercion::Identity
            } else {
                match (pool.get(g), pool.get(site)) {
                    (TyData::Fn { .. }, TyData::Fn { .. }) => Coercion::RowSubsume,
                    _ => Coercion::Variance,
                }
            };
            let (slot, r, syn) = v.to;
            let rec = self.b.words_record(&[kind as u32, NONE]);
            self.b.fill(slot, Tag::Coerce, r.0, rec, var, syn);
        }
    }

    /// The expected type a value at a site passes on to a literal it holds:
    /// none when `want` is the site's join variable. A nested list, map or
    /// tuple literal then takes its own type, which joins the site's, so
    /// `[[health], [orders]]` compares the two lists' types
    /// (`req.row.union.literal.direct`) rather than checking `orders`
    /// against the first list's element type.
    pub(crate) fn literal_want(&self, want: Option<Ty>) -> Option<Ty> {
        want.filter(|w| !self.joins.is_site(*w))
    }

    /// A value of type `got` at the site whose join variable is `var`.
    /// Returns the value, wrapped in `.Some` when optional injection
    /// joins it. A value that does not join reports the error here, at
    /// the value, and leaves the site's type as it was, so nothing after
    /// it reports again.
    pub(crate) fn join_value(
        &mut self,
        r: Ref,
        got: Ty,
        var: Ty,
        n: NodeRef<'_>,
        what: &str,
    ) -> Ref {
        if self.join_type(got, var, n, what) {
            // The value's conversion, known when the site closes.
            let slot = self.b.reserve();
            let s = slot.as_ref();
            self.b.body_mut().ty[s.0 as usize] = got;
            self.joins.push(var, got, (slot, r, n.index()));
            return s;
        }
        r
    }

    /// A spread contributes its element type to the join. Once the site
    /// closes, list assembly converts its captured list value's elements
    /// (`expr.list.spread.convert`).
    pub(crate) fn join_spread(&mut self, got: Ty, var: Ty, n: NodeRef<'_>, what: &str) {
        self.join_type(got, var, n, what);
    }

    /// Joins a value's type into the site of `var`, or reports why it
    /// does not join. True when the value joined and takes a conversion
    /// when the site closes.
    fn join_type(&mut self, got: Ty, var: Ty, n: NodeRef<'_>, what: &str) -> bool {
        crate::work::join_work(1);
        let pool = self.pool();
        let g = self.infer.resolve(pool, got);
        // `types.lct.never.dropped`: a `never` value does not take part.
        if g == Ty::NEVER || pool.has_poison(g) {
            return false;
        }
        // A site reports once: after a value that does not join, the
        // others are not compared.
        if self.joins.failed(var) {
            return false;
        }
        // The first value solves the site; an open literal class has solved
        // it as "an integer literal", which an optional value still widens.
        let cur = self.infer.shallow(pool, var);
        if let TyData::Infer(site) = pool.get(cur)
            && self.infer.kind_of(pool, cur) == Some(VarKind::General)
        {
            // A value whose type is still a variable, such as a literal's,
            // is aliased rather than joined, so a later widening of the
            // site leaves the value's own type alone. A value of the site's
            // own class unifies below, which binds nothing.
            let value = self.infer.shallow(pool, got);
            if matches!(pool.get(value), TyData::Infer(_)) && value != cur {
                self.infer.alias(pool, site, got);
                return true;
            }
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, var, got).is_ok() {
                return true;
            }
            self.infer.rollback(snap);
            self.no_common_type(var, got, n, what);
            return false;
        }
        let cur = self.infer.resolve(pool, var);
        if pool.has_poison(cur) {
            return false;
        }
        let open = |t: Ty| matches!(pool.get(t), TyData::Infer(_));
        let gs = self.resolved(g);
        // `types.lct.optional`: a plain value joins an optional site by
        // injection, one layer only (`types.lct.optional.one-layer`).
        if let Some(inner) = self.option_arg(cur)
            && self.option_layers(gs) < self.option_layers(cur)
            && !(open(gs) && self.infer.kind_of(pool, gs) == Some(VarKind::General))
        {
            let snap = self.infer.snapshot();
            if self.unify_join(inner, got) && self.lct_converts(got, inner, true) {
                return true;
            }
            self.infer.rollback(snap);
            self.no_common_type(var, got, n, what);
            return false;
        }
        // An optional value joins a plain site of its inner type: the
        // site's type widens to the optional one, and the values joined
        // before it are wrapped when the site closes (`join_site`).
        if let Some(inner) = self.option_arg(gs)
            && self.option_layers(cur) < self.option_layers(gs)
        {
            let snap = self.infer.snapshot();
            if self.unify_join(inner, cur) {
                let full = self.infer.resolve(pool, gs);
                let earlier = self.joined_types(var);
                if !pool.has_infer(full) && earlier.iter().all(|v| self.lct_reaches(*v, full)) {
                    self.infer.rebind(pool, var, full);
                    return true;
                }
            }
            self.infer.rollback(snap);
            self.no_common_type(var, got, n, what);
            return false;
        }
        let snap = self.infer.snapshot();
        if !self.unify_join(var, got) {
            self.infer.rollback(snap);
            self.no_common_type(var, got, n, what);
            return false;
        }
        let (cur, g) = (self.infer.resolve(pool, var), self.infer.resolve(pool, got));
        // `types.lct.no-combine`: each value reaches the least type by one
        // admitted conversion. The meet builds the type so that this value
        // does, and the earlier values reach the current type, so they are
        // compared again only when the type changes.
        let met = self.lct_meet(cur, g, true).filter(|t| {
            *t == cur
                || self
                    .joined_types(var)
                    .iter()
                    .all(|v| self.lct_reaches(*v, *t))
        });
        if let Some(t) = met {
            if t != cur {
                self.infer.rebind(pool, var, t);
            }
            true
        } else {
            self.no_common_type(var, got, n, what);
            false
        }
    }

    /// Whether two values' types have the same shape for the join, binding
    /// variables as unification does. Reference permissions meet by
    /// weakening, and ordinary unification keeps them distinct, so a
    /// reference's permissions are left to [`Self::lct_meet`] where the
    /// least common type may weaken them: at the top and in each tuple
    /// element (`types.lct.tuple-elements`). A generic type's arguments
    /// unify whatever their declared variance: a variance step that
    /// weakens a permission inside them is the combination
    /// `types.lct.no-combine` rules out. A reference's own referent unifies
    /// too. Cost: one walk of the tuple spine, bounded by `DEPTH`.
    fn unify_join(&mut self, a: Ty, b: Ty) -> bool {
        self.unify_join_at(a, b, 0)
    }

    fn unify_join_at(&mut self, a: Ty, b: Ty, depth: u32) -> bool {
        let pool = self.pool();
        let (sa, sb) = (self.infer.shallow(pool, a), self.infer.shallow(pool, b));
        if depth > DEPTH {
            return self.infer.unify(pool, sa, sb).is_ok();
        }
        let d = depth + 1;
        match (pool.get(sa), pool.get(sb)) {
            (TyData::Ref { inner: x, .. }, TyData::Ref { inner: y, .. }) => {
                self.infer.unify(pool, x, y).is_ok()
            }
            (
                TyData::Tuple {
                    elems: xs,
                    rest: xr,
                },
                TyData::Tuple {
                    elems: ys,
                    rest: yr,
                },
            ) => {
                let (xs, ys) = (pool.list_items(xs), pool.list_items(ys));
                if xs.len() != ys.len() {
                    return false;
                }
                let elems = xs
                    .iter()
                    .zip(ys.iter())
                    .all(|(x, y)| self.unify_join_at(*x, *y, d));
                elems
                    && match (xr, yr) {
                        (Some(x), Some(y)) => self.unify_join_at(x, y, d),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => self.infer.unify(pool, sa, sb).is_ok(),
        }
    }

    fn joined_types(&self, var: Ty) -> Vec<Ty> {
        let pool = self.pool();
        let values = self.joins.values(var);
        crate::work::join_work(values.len());
        values
            .iter()
            .map(|v| self.infer.resolve(pool, v.ty))
            .collect()
    }

    /// Reports a value that does not join its site (`types.lct.fail`,
    /// `types.lct.no-common`). The meet weakens permissions only at the
    /// top and in tuple elements, and unions rows only at the top, so each
    /// value reaches any type the meet builds and two values that meet
    /// have a least type. A variance step that would weaken a permission,
    /// as `List[*mut Cell]` to `List[*Cell]`, is `no-common-type`
    /// (`types.lct.no-combine`).
    fn no_common_type(&mut self, var: Ty, got: Ty, n: NodeRef<'_>, what: &str) {
        let pool = self.pool();
        let cur = self.infer.resolve(pool, var);
        let g = self.infer.resolve(pool, got);
        let (shown_cur, shown_got) = (self.show(cur), self.show(g));
        // A row is not part of the shown type, so name what differs.
        let (shown_cur, shown_got) = if shown_cur == shown_got {
            (
                format!("{shown_cur} with one row"),
                format!("{shown_got} with another"),
            )
        } else {
            (shown_cur, shown_got)
        };
        let msg = format!(
            "in {what}: {shown_cur} and {shown_got} have no common type; write the type you mean, as in `let x: T = ...`"
        );
        self.err(Code::NoCommonType, n, &msg);
        self.joins.fail(var);
    }

    /// The least type that two types meet at by the least common type's
    /// conversions, if there is one. The two types already unify, so they
    /// differ only in permissions and function rows. A permission weakens
    /// at the top and in a tuple element (`types.lct.tuple-elements`),
    /// never under a generic type's argument or a function's parameter or
    /// result, where weakening would ride on a variance step
    /// (`types.lct.no-combine`). `top` is the site's level, where function
    /// values take the union of their rows
    /// (`types.lct.row-union-every-site`); below it rows are invariant
    /// (`types.variance.function`, `req.row.union.literal.direct`).
    fn lct_meet(&self, a: Ty, b: Ty, top: bool) -> Option<Ty> {
        enum Work {
            Enter(Ty, Ty, bool, usize),
            Tuple((Ty, Ty, bool), std::ops::Range<usize>, bool, usize),
        }
        let pool = self.pool();
        let (a, b) = (self.resolved(a), self.resolved(b));
        if a == b {
            return Some(a);
        }
        let mut memo = HashMap::default();
        let mut values = vec![Ty::POISON];
        let done = hd_base::stack::all(Work::Enter(a, b, top, 0), |work, stack| {
            let (a, b, top, out) = match work {
                Work::Enter(a, b, top, out) => (a, b, top, out),
                Work::Tuple(key, slots, rest, out) => {
                    let parts = &values[slots];
                    let n = parts.len() - usize::from(rest);
                    let ty = pool.intern_ty(&TyData::Tuple {
                        elems: pool.list(&parts[..n]),
                        rest: rest.then(|| parts[n]),
                    });
                    memo.insert(key, ty);
                    values[out] = ty;
                    return true;
                }
            };
            let (a, b) = (self.resolved(a), self.resolved(b));
            if a == b {
                values[out] = a;
                return true;
            }
            if let Some(&ty) = memo.get(&(a, b, top)) {
                values[out] = ty;
                return true;
            }
            hd_types::work::type_node();
            let ty = match (pool.get(a), pool.get(b)) {
                // Reference permissions weaken at the site, while the
                // referred-to type stays invariant (`ref.convert.weaken`).
                // A counted reference lends itself as a local one
                // (`ref.convert.lend`), so the kinds join as the access does.
                (
                    TyData::Ref {
                        inner: x,
                        mutable: xm,
                        counted: xc,
                    },
                    TyData::Ref {
                        inner: y,
                        mutable: ym,
                        counted: yc,
                    },
                ) if self.resolved(x) == self.resolved(y) => Some(pool.intern_ty(&TyData::Ref {
                    inner: x,
                    mutable: xm && ym,
                    counted: xc && yc,
                })),
                // A tuple element keeps its permission and converts as the
                // whole tuple does, as assignment allows (`perm_fits`).
                (
                    TyData::Tuple {
                        elems: xs,
                        rest: xr,
                    },
                    TyData::Tuple {
                        elems: ys,
                        rest: yr,
                    },
                ) => {
                    let (xs, ys) = (pool.list_items(xs), pool.list_items(ys));
                    if xs.len() != ys.len() {
                        return false;
                    }
                    let rest = match (xr, yr) {
                        (Some(x), Some(y)) => Some((x, y)),
                        (None, None) => None,
                        _ => return false,
                    };
                    let first = values.len();
                    values.resize(first + xs.len() + usize::from(rest.is_some()), Ty::POISON);
                    stack.push(Work::Tuple(
                        (a, b, top),
                        first..values.len(),
                        rest.is_some(),
                        out,
                    ));
                    if let Some((x, y)) = rest {
                        stack.push(Work::Enter(x, y, false, first + xs.len()));
                    }
                    stack.extend(
                        xs.iter()
                            .zip(ys)
                            .enumerate()
                            .rev()
                            .map(|(i, (&x, &y))| Work::Enter(x, y, false, first + i)),
                    );
                    return true;
                }
                (
                    TyData::Fn {
                        params: xp,
                        result: xr,
                        suspends,
                        inputs,
                        row: xw,
                        deps,
                    },
                    TyData::Fn {
                        params: yp,
                        result: yr,
                        suspends: ys,
                        inputs: yi,
                        row: yw,
                        deps: yd,
                    },
                ) if top
                    && suspends == ys
                    && inputs == yi
                    && deps == yd
                    && self.same_signature(xp, xr, yp, yr) =>
                {
                    let (xw, yw) = (
                        self.infer.resolve_row(pool, xw),
                        self.infer.resolve_row(pool, yw),
                    );
                    let row = if xw == yw {
                        xw
                    } else {
                        let mut union = pool.row_data(xw);
                        let more = pool.row_data(yw);
                        union.keys.extend(more.keys);
                        union.params.extend(more.params);
                        pool.row(&union)
                    };
                    Some(pool.intern_ty(&TyData::Fn {
                        params: xp,
                        result: xr,
                        suspends,
                        inputs,
                        row,
                        deps,
                    }))
                }
                // A generic type's arguments meet only as one type: by
                // `types.lct.no-combine` for a covariant one, by
                // `types.option.invariant` and invariance for the rest. Two
                // functions that differ below the top differ in a row there.
                _ => None,
            };
            let Some(ty) = ty else {
                return false;
            };
            memo.insert((a, b, top), ty);
            values[out] = ty;
            true
        });
        done.then(|| values[0])
    }

    /// Whether two function types have the same parameters and result,
    /// so they differ at most in their rows. The two types already unify,
    /// so this compares resolved types and never recurses.
    fn same_signature(&self, xp: hd_types::TyList, xr: Ty, yp: hd_types::TyList, yr: Ty) -> bool {
        let pool = self.pool();
        let (xp, yp) = (pool.list_items(xp), pool.list_items(yp));
        xp.len() == yp.len()
            && self.resolved(xr) == self.resolved(yr)
            && xp
                .iter()
                .zip(yp.iter())
                .all(|(x, y)| self.resolved(*x) == self.resolved(*y))
    }

    /// Whether `s` converts to `t` by one step the least common type
    /// admits: the same type, weakening a reference's permissions, or the
    /// same per tuple element (`types.lct.conversions`,
    /// `types.lct.tuple-elements`). A generic type's arguments and a
    /// function's parameters and result must already agree: a variance
    /// step never carries a weakening (`types.lct.no-combine`). `top` lets
    /// a function value's row widen to the site's union row.
    fn lct_converts(&self, s: Ty, t: Ty, top: bool) -> bool {
        let pool = self.pool();
        let (s, t) = (self.resolved(s), self.resolved(t));
        if s == t {
            return true;
        }
        let mut seen = HashSet::default();
        hd_base::stack::all((s, t, top), |(s, t, top), stack| {
            let (s, t) = (self.resolved(s), self.resolved(t));
            if s == t || !seen.insert((s, t, top)) {
                return true;
            }
            hd_types::work::type_node();
            match (pool.get(s), pool.get(t)) {
                (
                    TyData::Ref {
                        inner: x,
                        mutable: xm,
                        counted: xc,
                    },
                    TyData::Ref {
                        inner: y,
                        mutable: ym,
                        counted: yc,
                    },
                ) => self.resolved(x) == self.resolved(y) && (xm || !ym) && (xc || !yc),
                (
                    TyData::Tuple {
                        elems: xs,
                        rest: xr,
                    },
                    TyData::Tuple {
                        elems: ys,
                        rest: yr,
                    },
                ) => {
                    let (xs, ys) = (pool.list_items(xs), pool.list_items(ys));
                    if xs.len() != ys.len() {
                        return false;
                    }
                    match (xr, yr) {
                        (Some(x), Some(y)) => stack.push((x, y, false)),
                        (None, None) => {}
                        _ => return false,
                    }
                    stack.extend(xs.iter().zip(ys).rev().map(|(&x, &y)| (x, y, false)));
                    true
                }
                (
                    TyData::Fn {
                        params: xp,
                        result: xr,
                        suspends,
                        inputs,
                        deps,
                        ..
                    },
                    TyData::Fn {
                        params: yp,
                        result: yr,
                        suspends: ys,
                        inputs: yi,
                        deps: yd,
                        ..
                    },
                ) if suspends == ys && inputs == yi && deps == yd => {
                    top && self.same_signature(xp, xr, yp, yr)
                }
                _ => false,
            }
        })
    }

    /// Whether an earlier value of type `v` reaches the site type `t`: by
    /// one admitted conversion, or by injection into `t`'s optional after
    /// one (`types.lct.optional`).
    fn lct_reaches(&self, v: Ty, t: Ty) -> bool {
        let pool = self.pool();
        if v == Ty::NEVER || self.lct_converts(v, t, true) {
            return true;
        }
        let t = self.infer.resolve(pool, t);
        self.option_arg(t).is_some_and(|inner| {
            self.option_layers(self.resolved(v)) < self.option_layers(t)
                && self.lct_converts(v, inner, true)
        })
    }

    /// The declared variance of each of `def`'s parameters: `1` covariant,
    /// `-1` contravariant, `0` invariant (`types.variance.markers`,
    /// `types.variance.list`, `types.variance.map`).
    pub(crate) fn declared_variances(&self, def: DefId) -> Vec<i8> {
        let known = self.cx.names.known;
        if def == known.list {
            return vec![1];
        }
        if def == known.map {
            return vec![0, 1];
        }
        self.cx
            .lookup
            .item(def)
            .map(|i| i.generics.iter().map(|g| g.variance).collect())
            .unwrap_or_default()
    }

    /// The open variables of a call's formal type that stand directly
    /// behind a reference or pointer, as `T` in `&T`: the only place an
    /// argument can solve one to a bare `dyn Tr` descriptor. The formal is
    /// resolved (each node once per body, by the table's memo), then a
    /// work stack walks only the parts that hold both a reference and a
    /// variable, each distinct part once: a formal with no reference, as
    /// the `W^i[?int]` a generic nest passes at level `i`, costs one probe,
    /// not its depth (#1912).
    pub(crate) fn open_referents(&self, formal: Ty) -> Vec<Ty> {
        let pool = self.pool();
        let mut out = Vec::new();
        let walked = |t: Ty| pool.has_infer(t) && pool.has_ref(t);
        let formal = self.infer.resolve(pool, formal);
        if !walked(formal) {
            return out;
        }
        let mut seen: HashSet<Ty> = HashSet::default();
        let mut stack = vec![formal];
        let mut children = Vec::new();
        while let Some(t) = stack.pop() {
            if !seen.insert(t) {
                continue;
            }
            hd_types::work::type_node();
            if let TyData::Ref { inner, .. } = pool.get(t)
                && matches!(pool.get(inner), TyData::Infer(_))
            {
                out.push(inner);
                continue;
            }
            children.clear();
            pool.children(t, &mut children);
            stack.extend(children.iter().rev().copied().filter(|&c| walked(c)));
        }
        out
    }

    /// Call inference never solves a type parameter to a bare `dyn Tr`
    /// (`types.generic.infer.no-bare-dyn`): an argument that would, as
    /// `&dyn Tr` for `&T`, is `cannot-infer-type`. Each such variable is
    /// rebound to poison, so the rest of the call reports nothing more
    /// about it.
    pub(crate) fn reject_bare_dyn_solve(&mut self, referents: &[Ty], n: NodeRef<'_>) {
        let pool = self.pool();
        let mut descriptor = None;
        for v in referents {
            let t = self.infer.resolve(pool, *v);
            if matches!(pool.get(t), TyData::TraitValue { .. }) {
                descriptor.get_or_insert(t);
                self.infer.rebind(pool, *v, Ty::POISON);
            }
        }
        let Some(descriptor) = descriptor else {
            return;
        };
        let shown = self.show(descriptor);
        let msg = format!(
            "this argument would solve a type parameter to the bare trait descriptor `dyn {shown}`, which is never a type argument; pass a concrete type's reference, or write the type argument as a reference type, as in `f::[&dyn {shown}](...)`"
        );
        self.err(Code::CannotInferType, n, &msg);
    }

    /// A generic call's argument of type `got` for a bare type parameter an
    /// earlier argument solved as `solved`. The two meet only by
    /// permission weakening (`types.generic.infer.join`): when they would
    /// need a trait-value conversion or a supertrait widening, that is
    /// `no-common-type` (`types.generic.infer.join.no-trait-value`,
    /// `types.generic.infer.join.no-supertrait-widening`), reported here.
    /// Returns whether it was. Any other conflict is the caller's
    /// `type-mismatch` (`types.generic.infer.join.other-conflict`).
    pub(crate) fn infer_join_conflict(&mut self, solved: Ty, got: Ty, n: NodeRef<'_>) -> bool {
        let pool = self.pool();
        let s = self.resolved(solved);
        let g = self.resolved(got);
        let trait_value = |t: Ty| self.dyn_ref_of(t).is_some();
        if g == Ty::NEVER
            || pool.has_poison(g)
            || pool.has_poison(s)
            || !(trait_value(s) || trait_value(g))
            || self.can_unify(s, g)
        {
            return false;
        }
        let msg = format!(
            "the arguments for one type parameter have no common type: {} and {}; write the type argument, as in `f::[T](...)`",
            self.show(s),
            self.show(g)
        );
        self.err(Code::NoCommonType, n, &msg);
        true
    }

    /// Whether two argument types meet by the inference join: outer
    /// permissions may weaken, but everything inside must already match in
    /// both directions. A `mut` inside the type, an option layer, or two
    /// different constructors conflict, even where unification or one
    /// direction's permission fit would pass.
    fn infer_join_compatible(&mut self, a: Ty, b: Ty) -> bool {
        let pool = self.pool();
        let (a, b) = (self.infer.resolve(pool, a), self.infer.resolve(pool, b));
        // Anything still generic takes whatever the rest of checking gives
        // it: only fully ground pairs meet here. In particular this keeps
        // existentials, placeholders, and projections on today's behavior.
        if pool.has_infer(a)
            || pool.has_infer(b)
            || pool.has_param(a)
            || pool.has_param(b)
            || pool.has_canon(a)
            || pool.has_canon(b)
            || pool.has_assoc(a)
            || pool.has_assoc(b)
        {
            return true;
        }
        // Poison and divergence are reported elsewhere, if at all.
        if matches!(pool.get(a), TyData::Poison | TyData::Never)
            || matches!(pool.get(b), TyData::Poison | TyData::Never)
        {
            return true;
        }
        let strip = |t: Ty| t;
        let (a, b) = (strip(a), strip(b));
        // Function types carry invariant rows and their own subtyping: the
        // join never meets them.
        if matches!(pool.get(a), TyData::Fn { .. }) || matches!(pool.get(b), TyData::Fn { .. }) {
            return true;
        }
        self.can_unify(a, b) && self.perm_fits(a, b, true, 0) && self.perm_fits(b, a, true, 0)
    }

    /// `type-mismatch` (`types.generic.infer.join.other-conflict`): any
    /// other conflict between the arguments' types for one inferred type
    /// parameter. Reports at the later argument and returns whether it did;
    /// the caller then skips its own coercion. Trait-value pairs belong to
    /// `infer_join_conflict`, which runs first.
    pub(crate) fn infer_other_conflict(
        &mut self,
        want: Ty,
        got: Ty,
        n: NodeRef<'_>,
        what: &str,
    ) -> bool {
        if self.infer_join_compatible(want, got) {
            return false;
        }
        let msg = format!(
            "in {what}: expected {}, found {}; the arguments for one type parameter may differ only by outer permission: write a conversion, as in `max(i64(small), large)`",
            self.show(want),
            self.show(got)
        );
        self.err(Code::TypeMismatch, n, &msg);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_tir::ir::{BodyKind, TirBuilder};

    #[test]
    fn tuple_meets_and_conversions_at_depth_1000_share_pair_answers() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for both generated depth tests.
            .spawn(|| {
                crate::body::with_checker(|ck| {
                    let p = ck.pool();
                    for am in [false, true] {
                        for ac in [false, true] {
                            for bm in [false, true] {
                                for bc in [false, true] {
                                    let reference = |mutable, counted| {
                                        p.intern_ty(&TyData::Ref {
                                            inner: Ty::I32,
                                            mutable,
                                            counted,
                                        })
                                    };
                                    for depth in [100, 1000] {
                                        let (mut a, mut b, mut wanted) = (
                                            reference(am, ac),
                                            reference(bm, bc),
                                            reference(am && bm, ac && bc),
                                        );
                                        for i in 0..depth {
                                            let tuple = |t| {
                                                p.intern_ty(&TyData::Tuple {
                                                    elems: p.list(&[t, t]),
                                                    rest: (i % 2 == 0).then_some(t),
                                                })
                                            };
                                            a = tuple(a);
                                            b = tuple(b);
                                            wanted = tuple(wanted);
                                        }
                                        let work = if a == b { 0 } else { depth + 1 };
                                        assert_eq!(
                                            hd_types::work::count_type_nodes(
                                                || ck.lct_meet(a, b, true)
                                            ),
                                            (Some(wanted), work)
                                        );
                                        assert_eq!(
                                            hd_types::work::count_type_nodes(
                                                || ck.lct_converts(a, b, true)
                                            ),
                                            ((am || !bm) && (ac || !bc), work)
                                        );
                                    }
                                }
                            }
                        }
                    }
                });
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn lct_rows_join_only_at_the_top_and_arguments_stay_invariant() {
        crate::body::with_checker(|ck| {
            let p = ck.pool();
            let key = |name| {
                p.intern_ty(&TyData::TraitValue {
                    def: ck.cx.names.item("app.main", name),
                    args: hd_types::TyList::EMPTY,
                    bindings: Vec::new(),
                })
            };
            let row = |keys| {
                p.row(&hd_types::RowData {
                    keys,
                    params: Vec::new(),
                })
            };
            let left = row(vec![key("Left")]);
            let right = row(vec![key("Right")]);
            let union = row(vec![key("Left"), key("Right")]);
            let function = |row| {
                p.intern_ty(&TyData::Fn {
                    params: p.list(&[Ty::I32]),
                    result: Ty::I32,
                    row,
                    suspends: false,
                    inputs: hd_types::Inputs::Fixed,
                    deps: hd_types::FnDeps::NONE,
                })
            };
            let (a, b) = (function(left), function(right));
            assert_eq!(ck.lct_meet(a, b, true), Some(function(union)));
            assert!(ck.lct_converts(a, b, true));
            assert_eq!(ck.lct_meet(a, b, false), None);
            assert!(!ck.lct_converts(a, b, false));
            let tuple = |t, rest| {
                p.intern_ty(&TyData::Tuple {
                    elems: p.list(&[t]),
                    rest,
                })
            };
            let adt = |t| {
                p.intern_ty(&TyData::Adt {
                    def: ck.cx.names.known.list,
                    args: p.list(&[t]),
                })
            };
            for (a, b) in [
                (tuple(a, None), tuple(b, None)),
                (adt(a), adt(b)),
                (tuple(Ty::I32, Some(Ty::I32)), tuple(Ty::I32, None)),
                (
                    tuple(Ty::I32, None),
                    p.intern_ty(&TyData::Tuple {
                        elems: hd_types::TyList::EMPTY,
                        rest: None,
                    }),
                ),
            ] {
                assert_eq!(ck.lct_meet(a, b, true), None);
                assert!(!ck.lct_converts(a, b, true));
            }
        });
    }

    #[test]
    fn deferred_tries_visit_only_their_return_site_and_roll_back() {
        for unrelated in [100, 1000] {
            let mut joins = Joins::default();
            let mut builder = TirBuilder::new(DefId::NONE, BodyKind::Fn);
            let deferred = DeferredTry {
                operand: (Ref(NONE), Ty::VOID),
                payload: Ty::VOID,
                slot: builder.reserve(),
                at: (NodeIdx::NONE, NodeIdx::NONE),
            };
            for i in 0..unrelated {
                let var = Ty(i + 128);
                joins.open(var);
                joins.defer(var, deferred);
            }
            let target = Ty(5000);
            joins.open(target);
            joins.defer(target, deferred);
            let mark = joins.mark();
            joins.defer(target, deferred);
            assert_eq!(joins.deferred(target).len(), 2);
            joins.rollback(mark);
            assert_eq!(joins.deferred(target).len(), 1);
            let mut visited = 0;
            for _ in joins.deferred(target) {
                visited += 1;
            }
            assert_eq!(visited, 1, "unrelated state: {unrelated}");
        }
    }
}
