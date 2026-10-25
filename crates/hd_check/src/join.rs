//! The least common type (spec 04 "Least Common Type", `types.lct.*`): the
//! one join every least-common-type site uses, and the narrower join of a
//! generic call's arguments (`types.generic.infer.join`).
//!
//! A site checks its values in order against a join variable. The first
//! value solves the variable; each later value meets the type so far by
//! the conversions `types.lct.conversions` lists, and only those:
//! permission weakening, declared readonly variance, and optional
//! injection of one layer. A value of `never` is dropped
//! (`types.lct.never.dropped`). Function values take the union of their
//! rows (`types.lct.row-union-every-site`). When no unique least type
//! exists, the value that does not meet the others reports
//! `no-common-type`, or `no-least-common-type` when the values do have a
//! common type that these rules cannot reach.

use hd_base::HashMap;

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
    /// A value did not join and was reported: the site reports once.
    failed: bool,
}

/// A value joined at a site: its type, unresolved, and where its
/// conversion to the site's final type goes (§4.3 step 6).
#[derive(Clone, Copy)]
struct Joined {
    ty: Ty,
    to: Conv,
}

/// Where a joined value's conversion goes.
#[derive(Clone, Copy)]
enum Conv {
    /// A slot reserved after the value, filled when the site closes.
    Slot(Slot, Ref, NodeIdx),
    /// A list spread's elements: the list passes as it is, so only a
    /// conversion with no run-time meaning is supported.
    Spread,
}

enum Undo {
    Open(Ty),
    Value(Ty),
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

    fn push(&mut self, var: Ty, ty: Ty, to: Conv) {
        if let Some(site) = self.sites.get_mut(&var) {
            site.values.push(Joined { ty, to });
            self.log.push(Undo::Value(var));
        }
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

/// The direction of a meet: the least type both convert to, or, in a
/// contravariant position, the greatest type that converts to both.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Least,
    Greatest,
}

impl Dir {
    fn flip(self) -> Self {
        match self {
            Self::Least => Self::Greatest,
            Self::Greatest => Self::Least,
        }
    }
}

/// How deep the meet and conversion walks go before giving up.
const DEPTH: u32 = 32;

impl Ck<'_, '_> {
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
        self.fill_join(result)?;
        self.joins.close(result);
        Ok(out)
    }

    /// Fills the slots of `var`'s site with each value's conversion to the
    /// site's final type: `WrapSome` for a value one optional layer short,
    /// else the static conversion the value takes, or `Identity`.
    fn fill_join(&mut self, var: Ty) -> StageResult<()> {
        let pool = self.pool();
        let site = self.infer.resolve(pool, var);
        let values = self.joins.values(var).to_vec();
        for v in values {
            let g = self.infer.resolve(pool, v.ty);
            let kind = if self.option_layers(self.strip_mut(g)) < self.option_layers(site) {
                Coercion::WrapSome
            } else if g == site || pool.has_poison(g) || pool.has_poison(site) {
                Coercion::Identity
            } else {
                match (pool.get(g), pool.get(site)) {
                    (TyData::Mut(i), _) if i == site => Coercion::Weaken,
                    (TyData::Fn { .. }, TyData::Fn { .. }) => Coercion::RowSubsume,
                    _ => Coercion::Variance,
                }
            };
            match v.to {
                Conv::Slot(slot, r, syn) => {
                    let rec = self.b.words_record(&[kind as u32, NONE]);
                    self.b.fill(slot, Tag::Coerce, r.0, rec, var, syn);
                }
                Conv::Spread if kind == Coercion::WrapSome => {
                    return Err(hd_base::NotImplemented::new(
                        hd_base::Stage::Body,
                        "a list spread that converts its elements",
                    ));
                }
                Conv::Spread => {}
            }
        }
        Ok(())
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
            self.joins.push(var, got, Conv::Slot(slot, r, n.index()));
            return s;
        }
        r
    }

    /// A list spread's elements of type `got` at the site of `var`: the
    /// list passes as it is (`expr.list.spread.expected`).
    pub(crate) fn join_spread(&mut self, got: Ty, var: Ty, n: NodeRef<'_>, what: &str) {
        if self.join_type(got, var, n, what) {
            self.joins.push(var, got, Conv::Spread);
        }
    }

    /// Joins a value's type into the site of `var`, or reports why it
    /// does not join. True when the value joined and takes a conversion
    /// when the site closes.
    fn join_type(&mut self, got: Ty, var: Ty, n: NodeRef<'_>, what: &str) -> bool {
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
        if matches!(pool.get(cur), TyData::Infer(_))
            && self.infer.kind_of(pool, cur) == Some(VarKind::General)
        {
            // A value whose type is still a variable, such as a literal's,
            // is aliased rather than joined, so a later widening of the
            // site leaves the value's own type alone.
            if matches!(pool.get(self.infer.shallow(pool, got)), TyData::Infer(_)) {
                self.infer.alias(pool, var, got);
                return true;
            }
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, var, got).is_ok() {
                return true;
            }
            self.infer.rollback(snap);
            self.no_common_type(var, got, false, n, what);
            return false;
        }
        let cur = self.infer.resolve(pool, var);
        if pool.has_poison(cur) {
            return false;
        }
        let open = |t: Ty| matches!(pool.get(t), TyData::Infer(_));
        let gs = self.strip_mut(g);
        // `types.lct.optional`: a plain value joins an optional site by
        // injection, one layer only (`types.lct.optional.one-layer`).
        if let TyData::Option(inner) = pool.get(cur)
            && self.option_layers(gs) < self.option_layers(cur)
            && !(open(gs) && self.infer.kind_of(pool, gs) == Some(VarKind::General))
        {
            let snap = self.infer.snapshot();
            if self.unify_join(inner, got) && self.lct_converts(got, inner, true, 0) {
                return true;
            }
            self.infer.rollback(snap);
            self.no_common_type(var, got, false, n, what);
            return false;
        }
        // An optional value joins a plain site of its inner type: the
        // site's type widens to the optional one, and the values joined
        // before it are wrapped when the site closes (`join_site`).
        if let TyData::Option(inner) = pool.get(gs)
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
            self.no_common_type(var, got, false, n, what);
            return false;
        }
        let snap = self.infer.snapshot();
        if !self.unify_join(var, got) {
            self.infer.rollback(snap);
            self.no_common_type(var, got, false, n, what);
            return false;
        }
        let (cur, g) = (self.infer.resolve(pool, var), self.infer.resolve(pool, got));
        // `types.lct.no-combine`: each value reaches the least type by one
        // admitted conversion. The meet builds the type so that this value
        // does, and the earlier values reach the current type, so they are
        // compared again only when the type changes.
        let met = self.lct_meet(cur, g, Dir::Least, true, 0).filter(|t| {
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
            self.no_common_type(var, got, true, n, what);
            false
        }
    }

    /// The types of the values joined so far at `var`'s site, resolved.
    /// Reference permissions meet by weakening; ordinary unification
    /// keeps them distinct. Only the invariant inner type unifies here.
    fn unify_join(&mut self, a: Ty, b: Ty) -> bool {
        let pool = self.pool();
        let (a, b) = match (
            pool.get(self.infer.shallow(pool, a)),
            pool.get(self.infer.shallow(pool, b)),
        ) {
            (TyData::Ref { inner: x, .. }, TyData::Ref { inner: y, .. }) => (x, y),
            _ => (a, b),
        };
        self.infer.unify(pool, a, b).is_ok()
    }

    fn joined_types(&self, var: Ty) -> Vec<Ty> {
        let pool = self.pool();
        self.joins
            .values(var)
            .iter()
            .map(|v| self.infer.resolve(pool, v.ty))
            .collect()
    }

    /// Reports a value that does not join its site (`types.lct.fail`).
    /// `shaped` is true when the types agree apart from permissions and
    /// function rows: then they may have common types of which these rules
    /// admit no least one, `no-least-common-type`
    /// (`types.lct.no-least`). That holds when, with each value's outer
    /// `mut` weakened first, the values meet by variance, which an
    /// ordinary conversion allows and the least common type does not
    /// (`types.lct.no-combine`).
    fn no_common_type(&mut self, var: Ty, got: Ty, shaped: bool, n: NodeRef<'_>, what: &str) {
        let pool = self.pool();
        let cur = self.infer.resolve(pool, var);
        let g = self.infer.resolve(pool, got);
        let least = shaped && {
            let mut all = self.joined_types(var);
            all.push(g);
            let mut acc = Some(self.strip_mut(all[0]));
            for t in &all[1..] {
                acc = acc.and_then(|a| self.lct_meet(a, self.strip_mut(*t), Dir::Least, true, 0));
            }
            acc.is_some()
        };
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
        if least {
            let msg = format!(
                "in {what}: {shown_cur} and {shown_got} have no least common type; write the type you mean, as in `let x: T = ...`"
            );
            self.err(Code::NoLeastCommonType, n, &msg);
        } else {
            let msg = format!(
                "in {what}: {shown_cur} and {shown_got} have no common type; write the type you mean, as in `let x: T = ...`"
            );
            self.err(Code::NoCommonType, n, &msg);
        }
        self.joins.fail(var);
    }

    /// `t` with a `mut` that grants nothing removed: permissions matter on
    /// composite types only.
    fn perm_canon(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Mut(i) if !self.is_composite(i) => i,
            _ => t,
        }
    }

    /// The least type (or, `Greatest`, the greatest type) that two types
    /// meet at by the least common type's conversions, if there is one.
    /// The two types already unify, so they differ only in permissions and
    /// function rows. `top` is the site's level, where function values
    /// take the union of their rows (`types.lct.row-union-every-site`);
    /// below it rows are invariant (`types.variance.function`,
    /// `req.row.union.literal.direct`).
    fn lct_meet(&self, a: Ty, b: Ty, dir: Dir, top: bool, depth: u32) -> Option<Ty> {
        let pool = self.pool();
        let (a, b) = (self.perm_canon(a), self.perm_canon(b));
        if a == b {
            return Some(a);
        }
        if depth > DEPTH {
            return None;
        }
        let d = depth + 1;
        match (pool.get(a), pool.get(b)) {
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
            ) if self.perm_canon(x) == self.perm_canon(y) => Some(pool.intern_ty(&TyData::Ref {
                inner: x,
                mutable: match dir {
                    Dir::Least => xm && ym,
                    Dir::Greatest => xm || ym,
                },
                counted: match dir {
                    Dir::Least => xc && yc,
                    Dir::Greatest => xc || yc,
                },
            })),
            // `types.variance.mut-invariant`: two mutable views meet only
            // when they are one type.
            (TyData::Mut(_), TyData::Mut(_)) => None,
            // Weakening the mutable view alone; the other value reaches its
            // readonly view by variance or not at all.
            (TyData::Mut(x), _) => match dir {
                Dir::Least => self.lct_converts(b, x, false, d).then_some(x),
                Dir::Greatest => self.lct_converts(a, b, false, d).then_some(a),
            },
            (_, TyData::Mut(y)) => match dir {
                Dir::Least => self.lct_converts(a, y, false, d).then_some(y),
                Dir::Greatest => self.lct_converts(b, a, false, d).then_some(b),
            },
            (TyData::Adt { def, args: xs }, TyData::Adt { def: d2, args: ys }) if def == d2 => {
                let vs = self.declared_variances(def);
                let (xs, ys) = (pool.list_items(xs), pool.list_items(ys));
                let mut out = Vec::with_capacity(xs.len());
                for (j, (x, y)) in xs.iter().zip(ys.iter()).enumerate() {
                    let t = match vs.get(j).copied().unwrap_or(0) {
                        1 => self.lct_meet(*x, *y, dir, false, d)?,
                        -1 => self.lct_meet(*x, *y, dir.flip(), false, d)?,
                        _ => (self.perm_canon(*x) == self.perm_canon(*y)).then_some(*x)?,
                    };
                    out.push(t);
                }
                Some(pool.intern_ty(&TyData::Adt {
                    def,
                    args: pool.list(&out),
                }))
            }
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
                let mut out = Vec::with_capacity(xs.len());
                for (x, y) in xs.iter().zip(ys.iter()) {
                    out.push(self.lct_meet(*x, *y, dir, false, d)?);
                }
                let rest = match (xr, yr) {
                    (Some(x), Some(y)) => Some(self.lct_meet(x, y, dir, false, d)?),
                    (None, None) => None,
                    _ => return None,
                };
                Some(pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&out),
                    rest,
                }))
            }
            (
                TyData::Fn {
                    params: xp,
                    result: xr,
                    suspends,
                    inputs,
                    row: xw,
                },
                TyData::Fn {
                    params: yp,
                    result: yr,
                    suspends: ys,
                    inputs: yi,
                    row: yw,
                },
            ) if suspends == ys && inputs == yi => {
                let (xp, yp) = (pool.list_items(xp), pool.list_items(yp));
                if xp.len() != yp.len() {
                    return None;
                }
                let mut params = Vec::with_capacity(xp.len());
                for (x, y) in xp.iter().zip(yp.iter()) {
                    params.push(self.lct_meet(*x, *y, dir.flip(), false, d)?);
                }
                let result = self.lct_meet(xr, yr, dir, false, d)?;
                let (xw, yw) = (
                    self.infer.resolve_row(pool, xw),
                    self.infer.resolve_row(pool, yw),
                );
                let row = if xw == yw {
                    xw
                } else if top {
                    let mut union = pool.row_data(xw);
                    let more = pool.row_data(yw);
                    union.keys.extend(more.keys);
                    union.params.extend(more.params);
                    pool.row(&union)
                } else {
                    return None;
                };
                Some(pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    suspends,
                    inputs,
                    row,
                }))
            }
            // `types.option.invariant`, and a trait value's arguments are
            // invariant: they meet only as one type.
            _ => None,
        }
    }

    /// Whether `s` converts to `t` by one representation-preserving step
    /// the least common type admits: the same type, weakening the outer
    /// `mut` alone, or declared readonly variance, whose arguments convert
    /// the same way (`types.lct.conversions`, `types.lct.no-combine`,
    /// `types.variance.repr.weakening`). `top` lets a function value's row
    /// widen to the site's union row.
    fn lct_converts(&self, s: Ty, t: Ty, top: bool, depth: u32) -> bool {
        let pool = self.pool();
        let (s, t) = (self.perm_canon(s), self.perm_canon(t));
        if s == t {
            return true;
        }
        if depth > DEPTH {
            return false;
        }
        let d = depth + 1;
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
            ) => self.perm_canon(x) == self.perm_canon(y) && (xm || !ym) && (xc || !yc),
            (TyData::Mut(x), _) => self.perm_canon(x) == t,
            (_, TyData::Mut(_)) => false,
            (TyData::Adt { def, args: xs }, TyData::Adt { def: d2, args: ys }) if def == d2 => {
                let vs = self.declared_variances(def);
                pool.list_items(xs)
                    .iter()
                    .zip(pool.list_items(ys).iter())
                    .enumerate()
                    .all(|(j, (x, y))| match vs.get(j).copied().unwrap_or(0) {
                        1 => self.lct_converts(*x, *y, false, d),
                        -1 => self.lct_converts(*y, *x, false, d),
                        _ => self.perm_canon(*x) == self.perm_canon(*y),
                    })
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
                xs.len() == ys.len()
                    && xs
                        .iter()
                        .zip(ys.iter())
                        .all(|(x, y)| self.lct_converts(*x, *y, false, d))
                    && match (xr, yr) {
                        (Some(x), Some(y)) => self.lct_converts(x, y, false, d),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (
                TyData::Fn {
                    params: xp,
                    result: xr,
                    suspends,
                    inputs,
                    row: xw,
                },
                TyData::Fn {
                    params: yp,
                    result: yr,
                    suspends: ys,
                    inputs: yi,
                    row: yw,
                },
            ) if suspends == ys && inputs == yi => {
                let (xp, yp) = (pool.list_items(xp), pool.list_items(yp));
                xp.len() == yp.len()
                    && xp
                        .iter()
                        .zip(yp.iter())
                        .all(|(x, y)| self.lct_converts(*y, *x, false, d))
                    && self.lct_converts(xr, yr, false, d)
                    && (top || self.infer.resolve_row(pool, xw) == self.infer.resolve_row(pool, yw))
            }
            _ => false,
        }
    }

    /// Whether an earlier value of type `v` reaches the site type `t`: by
    /// one admitted conversion, or by injection into `t`'s optional after
    /// one (`types.lct.optional`).
    fn lct_reaches(&self, v: Ty, t: Ty) -> bool {
        let pool = self.pool();
        if v == Ty::NEVER || self.lct_converts(v, t, true, 0) {
            return true;
        }
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Option(inner) => {
                self.option_layers(self.strip_mut(v)) < self.option_layers(t)
                    && self.lct_converts(v, inner, true, 0)
            }
            _ => false,
        }
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
        let s = self.strip_mut(solved);
        let g = self.strip_mut(got);
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
        let strip = |t: Ty| match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
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
