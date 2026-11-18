//! The unifier (`crates/hd_check/DESIGN.md` (type checking) §3): union-find over inference variables
//! with a trail for rollback (§3.5), an occurs check (§3.3) and literal
//! kinds (§3.6). Poison unifies with everything (`docs/compiler/data-structures.md` §3.6).

use hd_base::InferVar;
use hd_base::{HashMap, HashSet};

use crate::pool::{Inputs, Ty, TyData, Types};

/// A variable's kind: general, integer literal or float literal (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarKind {
    General,
    IntLit,
    /// An integer literal class with a signed member (`-1`, `+5`).
    SignedIntLit,
    FloatLit,
}

impl VarKind {
    /// A literal class's default type (`types.literal.local.default`);
    /// `None` for a general variable.
    #[must_use]
    pub const fn literal_default(self) -> Option<Ty> {
        match self {
            Self::General => None,
            Self::IntLit => Some(Ty::prim(crate::Prim::Usize)),
            Self::SignedIntLit => Some(Ty::I32),
            Self::FloatLit => Some(Ty::prim(crate::Prim::F64)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnifyError {
    Mismatch { expected: Ty, found: Ty },
    Occurs { var: InferVar, ty: Ty },
    Kind { var: InferVar, ty: Ty },
}

#[derive(Clone, Copy, Debug)]
enum Undo {
    Bind(u32),
    Parent(u32, u32),
    Rank(u32, u8),
    Kind(u32, VarKind),
    /// A replaced binding, with the flags the old one held.
    Rebind(u32, Option<Ty>, u8),
}

/// What a binding's type may hold that a use must see through its
/// variables: the flags [`InferTable::may_hold`] answers for.
pub mod holds {
    pub const POISON: u8 = 1;
    pub const PROJECTION: u8 = 2;
    pub const OPAQUE: u8 = 4;
}

fn holds_of(pool: Types<'_>, t: Ty) -> u8 {
    let bit = |on: bool, flag: u8| if on { flag } else { 0 };
    bit(pool.has_poison(t), holds::POISON)
        | bit(pool.has_assoc(t), holds::PROJECTION)
        | bit(pool.has_opaque(t), holds::OPAQUE)
}

/// The resolutions kept across calls. A closed one, with no variable
/// left (#1891), depends only on the bindings made before it, all on the
/// trail, so it stays valid until a rollback undoes one of them or a
/// `rebind` replaces one. `log` holds each key with the trail length when
/// it was resolved; stamps never decrease, because a rollback to `s`
/// first drops every entry stamped above `s`.
///
/// `open` keeps the resolutions that still hold variables (#1912, #2005),
/// whatever their number. Such a result also depends on each unbound
/// root variable it reached staying an unbound root: a binding or a
/// union of one changes it. So each entry records its direct
/// dependencies, the unbound roots and the open child types its walk
/// reached, as edges in `users`; a binding or union of a variable kills
/// the entries above it along those edges ([`InferTable::kill`]), and a
/// lookup that finds an entry needs no check. A killed entry goes to
/// `killed` with the trail length of the kill, so a rollback below the
/// kill revives it, as it undoes the binding that killed it.
/// `W[W[...[?int]]]` resolved at each level of a nest is then walked
/// once, not once per level, however many variables each level holds.
///
/// Cost: a walk pays one edge per child of each new node; a lookup is one
/// probe; a kill and its revival pay each entry killed and each edge out
/// of it once. `edges` keeps each edge once, so a key resolved again
/// after a kill adds no edge it already has, and a trial repeated `k`
/// times does not grow a kill's scan `k` times. An edge goes with the
/// entry that added it when a rollback drops that entry (`edge_log`); an
/// edge whose entry was killed stays, and a kill along it finds no entry.
#[derive(Default, Debug, Clone)]
struct Closed {
    map: HashMap<Ty, Ty>,
    /// Each open key's result and the trail length it was resolved at.
    open: HashMap<Ty, (Ty, usize)>,
    /// Each kept key, the trail length it was resolved at, and whether
    /// it went to `open`.
    log: Vec<(usize, Ty, bool)>,
    /// The open entries a dependency's change kills: those whose walk
    /// reached it directly.
    users: HashMap<Dep, Vec<Ty>>,
    edges: HashSet<(Dep, Ty)>,
    /// Each edge in the order added, with the trail length of the entry
    /// that added it.
    edge_log: Vec<(usize, Dep, Ty)>,
    /// Killed open entries in kill order: the trail length at the kill,
    /// the key, its result and the trail length it was resolved at.
    killed: Vec<(usize, Ty, Ty, usize)>,
}

/// What an open resolution read directly: an unbound root variable, or
/// an open child type with its own entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Dep {
    Var(u32),
    Ty(Ty),
}

impl Closed {
    /// Drops what was resolved above trail length `s` and the edges it
    /// added, then revives what was killed above it and resolved at or
    /// below it.
    fn drop_above(&mut self, s: usize) {
        while let Some(&(stamp, d, t)) = self.edge_log.last()
            && stamp > s
        {
            self.edge_log.pop();
            self.edges.remove(&(d, t));
            // Edges leave in the reverse of the order they came, so each
            // is the last of its dependency's users: `depend` adds every
            // edge to `edges`, `users` and `edge_log` together, and only a
            // `rebind` (all three reset) or this loop takes one out.
            let ts = self.users.entry(d).or_default();
            let last = ts.pop();
            debug_assert_eq!(last, Some(t), "an edge leaves as the last of its users");
            if ts.is_empty() {
                self.users.remove(&d);
            }
        }
        while let Some(&(stamp, t, open)) = self.log.last()
            && stamp > s
        {
            self.log.pop();
            if open {
                self.open.remove(&t);
            } else {
                self.map.remove(&t);
            }
        }
        while let Some(&(at, t, r, made)) = self.killed.last()
            && at > s
        {
            self.killed.pop();
            if made <= s {
                self.open.insert(t, (r, made));
            }
        }
    }

    /// Records that the open entry `t`, resolved at trail length
    /// `stamp`, read each of `deps` directly.
    fn depend(&mut self, deps: impl Iterator<Item = Dep>, t: Ty, stamp: usize) {
        for d in deps {
            if self.edges.insert((d, t)) {
                self.users.entry(d).or_default().push(t);
                self.edge_log.push((stamp, d, t));
            }
        }
    }
}

/// The body's inference table: union-find with union by rank and a trail
/// (`docs/compiler/data-structures.md` §3.19's `parent`, `rank`, `kind` and `binding`
/// columns). Union by rank keeps every path at most log2(n) links long
/// without writing during a lookup, so `root` stays a read and a rollback
/// undoes each link and rank change exactly.
#[derive(Default, Debug, Clone)]
pub struct InferTable {
    parent: Vec<u32>,
    rank: Vec<u8>,
    binding: Vec<Option<Ty>>,
    kind: Vec<VarKind>,
    trail: Vec<Undo>,
    /// Per variable, the [`holds`] flags its binding's type holds itself.
    held: Vec<u8>,
    /// The live bindings holding each [`holds`] flag, by bit.
    held_count: [u32; 3],
    closed: std::cell::RefCell<Closed>,
}

/// A rollback point (the one rollback contract, §3.5).
#[derive(Clone, Copy, Debug)]
pub struct Snapshot(usize);

/// A trial's rollback point: the trail and the variable count. Rolling
/// back to it also drops the variables the trial made, so a failed trial
/// leaves no variable behind (`crates/hd_check/DESIGN.md` (type checking) §3.5, §2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrialMark {
    trail: usize,
    vars: usize,
}

impl InferTable {
    pub fn fresh(&mut self, pool: Types<'_>, kind: VarKind) -> Ty {
        let v = crate::word(self.parent.len());
        self.parent.push(v);
        self.rank.push(0);
        self.binding.push(None);
        self.held.push(0);
        self.kind.push(kind);
        pool.intern_ty(&TyData::Infer(InferVar::from_raw(v)))
    }

    /// Kills the open resolutions that read the unbound root `v`, which
    /// a binding or union just changed, and those above them
    /// ([`Closed`]): each entry and each edge out of it once. Called
    /// after the change's trail entry, so a rollback to before the change
    /// revives them.
    fn kill(&mut self, v: u32) {
        let at = self.trail.len();
        let Closed {
            open,
            users,
            killed,
            ..
        } = self.closed.get_mut();
        if open.is_empty() {
            return;
        }
        let mut todo = vec![Dep::Var(v)];
        while let Some(d) = todo.pop() {
            for &t in users.get(&d).map_or(&[][..], Vec::as_slice) {
                crate::work::type_node();
                if let Some((r, made)) = open.remove(&t) {
                    killed.push((at, t, r, made));
                    todo.push(Dep::Ty(t));
                }
            }
        }
    }

    fn root(&self, mut v: u32) -> u32 {
        while self.parent[v as usize] != v {
            v = self.parent[v as usize];
        }
        v
    }

    /// Records the [`holds`] flags `h` of `v`'s new binding for
    /// [`Self::may_hold`]; the binding and its trail entry are the caller's.
    fn hold(&mut self, v: u32, h: u8) {
        self.unhold(v);
        self.held[v as usize] = h;
        for (bit, n) in self.held_count.iter_mut().enumerate() {
            *n += u32::from(h >> bit & 1);
        }
    }

    fn unhold(&mut self, v: u32) {
        let h = std::mem::take(&mut self.held[v as usize]);
        for (bit, n) in self.held_count.iter_mut().enumerate() {
            *n -= u32::from(h >> bit & 1);
        }
    }

    /// Whether `t` resolved may hold one of the [`holds`] flags `mask`:
    /// `t` holds it itself, or `t` has a variable and some live binding's
    /// type holds it. One read of `t`'s flags, no walk; `false` is exact.
    #[must_use]
    pub fn may_hold(&self, pool: Types<'_>, t: Ty, mask: u8) -> bool {
        holds_of(pool, t) & mask != 0
            || (pool.has_infer(t)
                && self
                    .held_count
                    .iter()
                    .enumerate()
                    .any(|(bit, n)| *n > 0 && mask >> bit & 1 != 0))
    }

    /// `t` shallowly resolved, or fully when its full resolution is
    /// already known closed: one lookup, no walk.
    #[must_use]
    pub fn shallow_or_closed(&self, pool: Types<'_>, t: Ty) -> Ty {
        let t = self.shallow(pool, t);
        if !pool.has_infer(t) {
            return t;
        }
        self.closed.borrow().map.get(&t).copied().unwrap_or(t)
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot(self.trail.len())
    }

    pub fn rollback(&mut self, s: Snapshot) {
        let from = s.0.min(self.trail.len());
        while self.trail.len() > from {
            let Some(u) = self.trail.pop() else { break };
            match u {
                Undo::Bind(v) => {
                    self.binding[v as usize] = None;
                    self.unhold(v);
                }
                Undo::Parent(v, old) => self.parent[v as usize] = old,
                Undo::Rank(v, old) => self.rank[v as usize] = old,
                Undo::Kind(v, k) => self.kind[v as usize] = k,
                Undo::Rebind(v, old, h) => {
                    self.binding[v as usize] = old;
                    self.hold(v, h);
                }
            }
        }
        self.closed.get_mut().drop_above(from);
    }

    #[must_use]
    pub fn trial_mark(&self) -> TrialMark {
        TrialMark {
            trail: self.trail.len(),
            vars: self.parent.len(),
        }
    }

    /// Undoes everything since `m` and drops the variables made since.
    /// After the trail is undone no older variable links to or binds a
    /// newer one, so the newer ones are unreachable.
    pub fn rollback_trial(&mut self, m: TrialMark) {
        self.rollback(Snapshot(m.trail));
        self.parent.truncate(m.vars);
        self.rank.truncate(m.vars);
        self.binding.truncate(m.vars);
        debug_assert!(
            self.held[m.vars.min(self.held.len())..]
                .iter()
                .all(|h| *h == 0)
        );
        self.held.truncate(m.vars);
        self.kind.truncate(m.vars);
    }

    /// Replaces a bound variable's solution with `t`, undoably: the
    /// permission join of several arguments lowers `mut X` to `X`
    /// (`crates/hd_check/DESIGN.md` (type checking) §3.4, "Permission join").
    pub fn rebind(&mut self, pool: Types<'_>, var: Ty, t: Ty) {
        let TyData::Infer(v) = pool.get(var) else {
            return;
        };
        let r = self.root(v.raw());
        self.trail.push(Undo::Rebind(
            r,
            self.binding[r as usize],
            self.held[r as usize],
        ));
        self.binding[r as usize] = Some(t);
        self.hold(r, holds_of(pool, t));
        // A replaced binding may change any closed resolution through it.
        *self.closed.get_mut() = Closed::default();
    }

    /// Binds the unbound variable `var` to `t`, a type outside `var`'s
    /// class that may be another variable, without joining their classes:
    /// a later `rebind` of `var` leaves `t`'s class as it is. A
    /// least-common-type site's variable takes its first value's type this
    /// way, so widening the site does not retype that value
    /// (`crates/hd_check/DESIGN.md` §4.3).
    pub fn alias(&mut self, pool: Types<'_>, var: InferVar, t: Ty) {
        let r = self.root(var.raw());
        debug_assert!(
            self.binding[r as usize].is_none(),
            "alias binds a free variable"
        );
        self.trail.push(Undo::Rebind(r, None, 0));
        self.binding[r as usize] = Some(t);
        self.hold(r, holds_of(pool, t));
        self.kill(r);
    }

    /// Replaces bound variables at the top of `t`.
    #[must_use]
    pub fn shallow(&self, pool: Types<'_>, t: Ty) -> Ty {
        let mut t = t;
        // Global types cannot hold inference variables. Check the local
        // bit again after each binding, so its global result is not decoded.
        while t.is_local()
            && let TyData::Infer(v) = pool.get(t)
        {
            let root = self.root(v.raw());
            match self.binding[root as usize] {
                Some(b) => t = b,
                None if root == v.raw() => return t,
                None => return pool.intern_ty(&TyData::Infer(InferVar::from_raw(root))),
            }
        }
        t
    }

    /// The class of the variable `t` itself names, bound or not: what a
    /// literal's variable stays after its class takes a type. `None` for
    /// any other type.
    #[must_use]
    pub fn class_of(&self, pool: Types<'_>, t: Ty) -> Option<u32> {
        match pool.get(t) {
            TyData::Infer(v) => Some(self.root(v.raw())),
            _ => None,
        }
    }

    /// The kind of an unbound variable, if `t` is one.
    #[must_use]
    pub fn kind_of(&self, pool: Types<'_>, t: Ty) -> Option<VarKind> {
        match pool.get(self.shallow(pool, t)) {
            TyData::Infer(v) => Some(self.kind[self.root(v.raw()) as usize]),
            _ => None,
        }
    }

    /// The number of variables made so far.
    #[must_use]
    pub fn var_count(&self) -> usize {
        self.parent.len()
    }

    /// Variables from `start` on whose class is an unbound literal, with
    /// the class kind (the literal classes a statement leaves open).
    #[must_use]
    pub fn open_literals_since(&self, pool: Types<'_>, start: usize) -> Vec<(Ty, VarKind)> {
        (start..self.parent.len())
            .filter_map(|v| {
                let t = pool.intern_ty(&TyData::Infer(InferVar::from_raw(crate::word(v))));
                match self.kind_of(pool, t) {
                    Some(k) if k != VarKind::General => Some((t, k)),
                    _ => None,
                }
            })
            .collect()
    }

    /// Marks an unbound integer literal class as holding a signed literal.
    pub fn mark_signed(&mut self, pool: Types<'_>, t: Ty) {
        if let TyData::Infer(v) = pool.get(self.shallow(pool, t)) {
            let r = self.root(v.raw());
            if self.kind[r as usize] == VarKind::IntLit {
                self.trail.push(Undo::Kind(r, VarKind::IntLit));
                self.kind[r as usize] = VarKind::SignedIntLit;
            }
        }
    }

    /// Resolves every variable it can on an explicit stack: each distinct node of
    /// the type once, memoized by type id, so a type that shares its
    /// parts (an interned DAG) costs its distinct nodes, not its tree.
    #[must_use]
    pub fn resolve(&self, pool: Types<'_>, t: Ty) -> Ty {
        let t = self.shallow(pool, t);
        if !pool.has_infer(t) {
            return t;
        }
        crate::TyMap::ty(&mut Resolve::new(self, pool), t)
    }

    /// Resolves a row's keys; a bound row variable's row joins the keys.
    #[must_use]
    pub fn resolve_row(&self, pool: Types<'_>, row: crate::RowId) -> crate::RowId {
        crate::TyMap::row(&mut Resolve::new(self, pool), row)
    }

    /// Whether variable `v` occurs in `t`: each distinct node once.
    fn occurs(&self, pool: Types<'_>, v: u32, t: Ty) -> bool {
        let mut seen: HashSet<Ty> = HashSet::default();
        let mut todo = vec![t];
        let mut kids = Vec::new();
        while let Some(t) = todo.pop() {
            let t = self.shallow(pool, t);
            if !pool.has_infer(t) || !seen.insert(t) {
                continue;
            }
            crate::work::type_node();
            kids.clear();
            match pool.get(t) {
                TyData::Infer(w) => {
                    if self.root(w.raw()) == v {
                        return true;
                    }
                }
                TyData::Adt { args: l, .. } => kids.extend_from_slice(pool.list_items(l)),
                TyData::Opaque { args, bound, .. } => {
                    kids.push(bound);
                    kids.extend_from_slice(pool.list_items(args));
                }
                TyData::Tuple { elems, rest } => {
                    kids.extend_from_slice(pool.list_items(elems));
                    kids.extend(rest);
                }
                TyData::Union(members) => kids.extend_from_slice(pool.list_items(members)),
                TyData::Ref { inner: i, .. } => kids.push(i),
                TyData::Fn { params, result, .. } => {
                    kids.push(result);
                    kids.extend_from_slice(pool.list_items(params));
                }
                // No variable is looked for inside a trait value, a
                // projection or a row, as before the walk was memoized.
                TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::TraitValue { .. }
                | TyData::Param(_)
                | TyData::Const(_)
                | TyData::Assoc { .. }
                | TyData::Canon(_)
                | TyData::Row(_)
                | TyData::Context(_) => {}
            }
            todo.extend(kids.iter().rev());
        }
        false
    }

    fn bind(&mut self, pool: Types<'_>, v: u32, t: Ty) -> Result<(), UnifyError> {
        let var = InferVar::from_raw(v);
        // The occurs check reads `t` resolved: its parts already known
        // closed cost one lookup each, and a closed `t` needs no walk, so a
        // chain of bindings each naming the last is linear (#1891).
        let r = self.resolve(pool, t);
        if pool.has_infer(r) && self.occurs(pool, v, r) {
            return Err(UnifyError::Occurs { var, ty: t });
        }
        let ok = match (self.kind[v as usize], pool.get(t)) {
            (VarKind::General, _) | (_, TyData::Poison | TyData::Never) => true,
            // `types.literal.int-not-float`: an integer literal never
            // takes a floating-point type.
            (VarKind::IntLit | VarKind::SignedIntLit, TyData::Prim(p)) => p.is_integer(),
            (VarKind::FloatLit, TyData::Prim(p)) => p.is_float(),
            _ => false,
        };
        if !ok {
            return Err(UnifyError::Kind { var, ty: t });
        }
        self.binding[v as usize] = Some(t);
        self.hold(v, holds_of(pool, t));
        self.trail.push(Undo::Bind(v));
        self.kill(v);
        Ok(())
    }

    /// Binds every unbound variable of `t` to poison.
    fn poison_vars(&mut self, pool: Types<'_>, t: Ty) {
        if !pool.has_infer(t) {
            return;
        }
        let mut seen = HashSet::default();
        hd_base::stack::all(t, |t, stack| {
            let t = self.shallow(pool, t);
            if !pool.has_infer(t) || !seen.insert(t) {
                return true;
            }
            crate::work::type_node();
            if let TyData::Infer(v) = pool.get(t) {
                let root = self.root(v.raw());
                self.binding[root as usize] = Some(Ty::POISON);
                self.hold(root, holds::POISON);
                self.trail.push(Undo::Bind(root));
                self.kill(root);
            } else {
                let start = stack.len();
                match pool.get(t) {
                    TyData::Fn { params, result, .. } => {
                        stack.extend_from_slice(pool.list_items(params));
                        stack.push(result);
                    }
                    // Poison propagation has always left row keys alone.
                    TyData::Row(_) | TyData::Context(_) => {}
                    _ => pool.children(t, stack),
                }
                stack[start..].reverse();
            }
            true
        });
    }

    /// Unifies two types; on error the caller rolls back to its snapshot.
    pub fn unify(&mut self, pool: Types<'_>, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.shallow(pool, a), self.shallow(pool, b));
        if a == b {
            return Ok(());
        }
        match (pool.get(a), pool.get(b)) {
            // The variables of the other side take poison: what is
            // inferred from a type that failed is no new error.
            (TyData::Poison, _) => {
                self.poison_vars(pool, b);
                Ok(())
            }
            (_, TyData::Poison) => {
                self.poison_vars(pool, a);
                Ok(())
            }
            // `&T` and `&mut T` are distinct types; the conversion from
            // `&mut T` to `&T` is a coercion (ref.convert.weaken). So are
            // `*T` and `&T` (`ref.type.kind-distinct`), whose conversion
            // is a lend (`ref.convert.lend`).
            (
                TyData::Ref {
                    inner: x,
                    mutable: mx,
                    counted: cx,
                },
                TyData::Ref {
                    inner: y,
                    mutable: my,
                    counted: cy,
                },
            ) if mx == my && cx == cy => self.unify(pool, x, y),
            (TyData::Infer(x), TyData::Infer(y)) => {
                let (x, y) = (self.root(x.raw()), self.root(y.raw()));
                let (kx, ky) = (self.kind[x as usize], self.kind[y as usize]);
                let merged = match (kx, ky) {
                    (VarKind::General, k) | (k, VarKind::General) => k,
                    (VarKind::FloatLit, VarKind::FloatLit) => VarKind::FloatLit,
                    // An integer class and a float class never join
                    // (`types.literal.int-not-float`).
                    (VarKind::FloatLit, _) | (_, VarKind::FloatLit) => {
                        return Err(UnifyError::Mismatch {
                            expected: a,
                            found: b,
                        });
                    }
                    (VarKind::SignedIntLit, _) | (_, VarKind::SignedIntLit) => {
                        VarKind::SignedIntLit
                    }
                    _ => VarKind::IntLit,
                };
                // `shallow` returns one interned root per class, so equal
                // roots were equal types and returned above.
                debug_assert_ne!(x, y, "one class reached the join twice");
                // Union by rank: the lower-ranked root joins the other; on
                // a tie `x` joins `y` and `y`'s rank grows.
                let (rx, ry) = (self.rank[x as usize], self.rank[y as usize]);
                let (child, root) = if rx > ry { (y, x) } else { (x, y) };
                self.trail
                    .push(Undo::Parent(child, self.parent[child as usize]));
                self.parent[child as usize] = root;
                if rx == ry {
                    self.trail.push(Undo::Rank(root, ry));
                    self.rank[root as usize] = ry + 1;
                }
                let kr = self.kind[root as usize];
                if merged != kr {
                    self.trail.push(Undo::Kind(root, kr));
                    self.kind[root as usize] = merged;
                }
                self.kill(child);
                Ok(())
            }
            (TyData::Infer(x), _) => self.bind(pool, self.root(x.raw()), b),
            (_, TyData::Infer(y)) => self.bind(pool, self.root(y.raw()), a),
            (
                TyData::Opaque {
                    owner: d1,
                    site: s1,
                    args: a1,
                    bound: b1,
                    ..
                },
                TyData::Opaque {
                    owner: d2,
                    site: s2,
                    args: a2,
                    bound: b2,
                    ..
                },
            ) if d1 == d2 && s1 == s2 => {
                self.unify_lists(pool, a1, a2, a, b)?;
                self.unify(pool, b1, b2)
            }
            (TyData::Adt { def: d1, args: a1 }, TyData::Adt { def: d2, args: a2 }) if d1 == d2 => {
                self.unify_lists(pool, a1, a2, a, b)
            }
            (
                TyData::Tuple {
                    elems: e1,
                    rest: None,
                },
                TyData::Tuple {
                    elems: e2,
                    rest: None,
                },
            ) => self.unify_lists(pool, e1, e2, a, b),
            // `types.tuple.rest.same`: with rest elements, the fixed
            // elements and the rest elements are each pairwise equal; a
            // tuple with one never meets a tuple without.
            (
                TyData::Tuple {
                    elems: e1,
                    rest: Some(r1),
                },
                TyData::Tuple {
                    elems: e2,
                    rest: Some(r2),
                },
            ) => {
                self.unify_lists(pool, e1, e2, a, b)?;
                self.unify(pool, r1, r2)
            }
            (TyData::Union(x), TyData::Union(y)) => self.unify_lists(pool, x, y, a, b),
            (
                TyData::TraitValue {
                    def: d1, args: a1, ..
                },
                TyData::TraitValue {
                    def: d2, args: a2, ..
                },
            ) if d1 == d2 => self.unify_lists(pool, a1, a2, a, b),
            (
                TyData::Assoc {
                    assoc: x1,
                    self_ty: s1,
                    args: a1,
                    ..
                },
                TyData::Assoc {
                    assoc: x2,
                    self_ty: s2,
                    args: a2,
                    ..
                },
            ) if x1 == x2 => {
                self.unify(pool, s1, s2)?;
                self.unify_lists(pool, a1, a2, a, b)
            }
            (
                TyData::Fn {
                    params: p1,
                    result: r1,
                    suspends: s1,
                    inputs: i1,
                    deps: d1,
                    row: w1,
                },
                TyData::Fn {
                    params: p2,
                    result: r2,
                    suspends: s2,
                    inputs: i2,
                    deps: d2,
                    row: w2,
                },
            ) if s1 == s2 && d1 == d2 => {
                if i1 == i2 {
                    self.unify_lists(pool, p1, p2, a, b)?;
                } else if i1 == Inputs::Tuple || i2 == Inputs::Tuple {
                    // An inputs tuple of unknown arity against the other
                    // side's whole inputs (`fn.type.ctor.inputs`).
                    let (t1, t2) = (pool.inputs_tuple(p1, i1), pool.inputs_tuple(p2, i2));
                    self.unify(pool, t1, t2)?;
                } else {
                    return Err(UnifyError::Mismatch {
                        expected: a,
                        found: b,
                    });
                }
                self.unify(pool, r1, r2)?;
                // A row that is one variable, a generic row parameter's,
                // takes the other row, as a data type's row argument does;
                // otherwise a variable in a row key is decided by the other
                // row's key of the same trait, as in a context row. Whether
                // the rows fit is the caller's question, as a top-level
                // function row may widen (`req.row.subsume`), so keys with
                // no variable are left to it.
                if self.row_variable(pool, w1) || self.row_variable(pool, w2) {
                    return self.unify_data_rows(pool, w1, w2, a, b);
                }
                self.unify_row_keys(pool, w1, w2, true)
            }
            (TyData::Context(r1), TyData::Context(r2)) => {
                self.unify_context_rows(pool, r1, r2, a, b)
            }
            // A data constructor represents its inferred row argument as a
            // row containing one inference variable. Bind that variable to
            // the other argument's whole row so `OnExit { ... }` can infer
            // `OnExit[$ Console]` (`req.row.param.data-infer`).
            (TyData::Row(r1), TyData::Row(r2)) => self.unify_data_rows(pool, r1, r2, a, b),
            _ => Err(UnifyError::Mismatch {
                expected: a,
                found: b,
            }),
        }
    }

    /// Whether `r` is one variable: a row of one variable key and no row
    /// parameter, as a generic row parameter's argument is.
    fn row_variable(&self, pool: Types<'_>, r: crate::RowId) -> bool {
        let d = pool.row_data(self.resolve_row(pool, r));
        matches!((d.keys.as_slice(), d.params.as_slice()),
            ([key], []) if matches!(pool.get(*key), TyData::Infer(_)))
    }

    fn unify_data_rows(
        &mut self,
        pool: Types<'_>,
        r1: crate::RowId,
        r2: crate::RowId,
        a: Ty,
        b: Ty,
    ) -> Result<(), UnifyError> {
        let r1 = self.resolve_row(pool, r1);
        let r2 = self.resolve_row(pool, r2);
        if r1 == r2 {
            return Ok(());
        }
        let d1 = pool.row_data(r1);
        let d2 = pool.row_data(r2);
        let variable = |row: &crate::RowData| match (row.keys.as_slice(), row.params.as_slice()) {
            ([key], []) => match pool.get(*key) {
                TyData::Infer(v) => Some(v),
                _ => None,
            },
            _ => None,
        };
        if let Some(v) = variable(&d1) {
            return self.bind(pool, self.root(v.raw()), pool.intern_ty(&TyData::Row(r2)));
        }
        if let Some(v) = variable(&d2) {
            return self.bind(pool, self.root(v.raw()), pool.intern_ty(&TyData::Row(r1)));
        }
        Err(UnifyError::Mismatch {
            expected: a,
            found: b,
        })
    }

    /// Two context types are equal when their rows are one key set
    /// (`req.context.row`). A key whose trait occurs once in each row
    /// unifies with its counterpart, so `$.Context[$ Repo[T]]` against
    /// `$.Context[$ Repo[User]]` fixes `T`; every other key must already
    /// be in both rows. Cost: one hash probe per key of each row to index
    /// them by trait, then one probe and one unify per key of the first.
    fn unify_context_rows(
        &mut self,
        pool: Types<'_>,
        r1: crate::RowId,
        r2: crate::RowId,
        a: Ty,
        b: Ty,
    ) -> Result<(), UnifyError> {
        let mismatch = UnifyError::Mismatch {
            expected: a,
            found: b,
        };
        let d1 = pool.row_data(self.resolve_row(pool, r1));
        let d2 = pool.row_data(self.resolve_row(pool, r2));
        if d1.keys.len() != d2.keys.len() || d1.params != d2.params {
            return Err(mismatch);
        }
        self.unify_row_keys(pool, r1, r2, false)?;
        if self.resolve_row(pool, r1) == self.resolve_row(pool, r2) {
            Ok(())
        } else {
            Err(mismatch)
        }
    }

    /// Unifies each key of row `r1` with the key of the same trait in
    /// `r2` when the trait occurs once in each row; with `open_only`,
    /// only pairs where a key holds a variable. Cost: one hash probe per
    /// key of each row to index them by trait, then one probe and at most
    /// one unify per key of the first; no work for two rows without keys.
    fn unify_row_keys(
        &mut self,
        pool: Types<'_>,
        r1: crate::RowId,
        r2: crate::RowId,
        open_only: bool,
    ) -> Result<(), UnifyError> {
        let (r1, r2) = (self.resolve_row(pool, r1), self.resolve_row(pool, r2));
        if r1 == r2 {
            return Ok(());
        }
        let d1 = pool.row_data(r1);
        let d2 = pool.row_data(r2);
        if d1.keys.is_empty() || d2.keys.is_empty() {
            return Ok(());
        }
        // Each row's keys by trait, `None` for a trait named twice: one
        // probe per key, so the pairing is linear in the keys.
        let by_trait = |keys: &[Ty]| {
            let mut by: HashMap<hd_base::DefId, Option<Ty>> = HashMap::default();
            for &k in keys {
                crate::work::type_node();
                if let TyData::TraitValue { def, .. } = pool.get(k) {
                    by.entry(def).and_modify(|e| *e = None).or_insert(Some(k));
                }
            }
            by
        };
        let (by1, by2) = (by_trait(&d1.keys), by_trait(&d2.keys));
        for &k1 in &d1.keys {
            let TyData::TraitValue { def, .. } = pool.get(k1) else {
                continue;
            };
            if let (Some(Some(_)), Some(Some(k2))) = (by1.get(&def), by2.get(&def))
                && (!open_only || pool.has_infer(k1) || pool.has_infer(*k2))
            {
                self.unify(pool, k1, *k2)?;
            }
        }
        Ok(())
    }

    fn unify_lists(
        &mut self,
        pool: Types<'_>,
        l1: crate::TyList,
        l2: crate::TyList,
        a: Ty,
        b: Ty,
    ) -> Result<(), UnifyError> {
        let (x, y) = (pool.list_items(l1), pool.list_items(l2));
        if x.len() != y.len() {
            return Err(UnifyError::Mismatch {
                expected: a,
                found: b,
            });
        }
        for (&s, &t) in x.iter().zip(y) {
            self.unify(pool, s, t)?;
        }
        Ok(())
    }
}

/// [`InferTable::resolve`] applied to each child of a type.
struct Resolve<'t, 'p> {
    table: &'t InferTable,
    pool: Types<'p>,
    /// The direct dependencies of the nodes being resolved, each node's
    /// run on top: what each finished child read, for its parent's entry
    /// ([`Closed::depend`]).
    deps: Vec<Dep>,
}

impl<'t, 'p> Resolve<'t, 'p> {
    fn new(table: &'t InferTable, pool: Types<'p>) -> Self {
        Resolve {
            table,
            pool,
            deps: Vec::new(),
        }
    }
}

impl Resolve<'_, '_> {
    // Keep known roots off the heap work stack.
    fn known(&mut self, t: Ty) -> Option<Ty> {
        if !self.pool.has_infer(t) {
            return Some(t);
        }
        if let TyData::Infer(v) = self.pool.get(t) {
            self.deps.push(Dep::Var(v.raw()));
            return Some(t);
        }
        let c = self.table.closed.borrow();
        let r = c
            .map
            .get(&t)
            .or_else(|| c.open.get(&t).map(|(r, _)| r))
            .copied()?;
        if self.pool.has_infer(r) {
            self.deps.push(Dep::Ty(t));
        }
        Some(r)
    }
}

// Rebuild only after every child has been resolved. This mapper reads the
// finished entries without recording a second copy of their dependencies.
struct ResolvedChildren<'t, 'p> {
    table: &'t InferTable,
    pool: Types<'p>,
}

impl crate::TyMap for ResolvedChildren<'_, '_> {
    fn ty(&mut self, t: Ty) -> Ty {
        let t = self.table.shallow(self.pool, t);
        if !self.pool.has_infer(t) || matches!(self.pool.get(t), TyData::Infer(_)) {
            return t;
        }
        let c = self.table.closed.borrow();
        c.map
            .get(&t)
            .or_else(|| c.open.get(&t).map(|(r, _)| r))
            .copied()
            // ruled out: an unfinished child; its finish frame ran before
            // the parent's and resolution cannot change bindings.
            .expect("the child has finished resolving")
    }

    fn row(&mut self, r: crate::RowId) -> crate::RowId {
        let t = self.ty(self.pool.intern_ty(&TyData::Row(r)));
        // ruled out: another constructor, since resolution preserves Row.
        let TyData::Row(r) = self.pool.get(t) else {
            unreachable!("resolved row")
        };
        r
    }
}

impl ResolvedChildren<'_, '_> {
    fn row_keys(&mut self, r: crate::RowId) -> crate::RowId {
        let mut d = self.pool.row_data(r);
        d.keys = d
            .keys
            .iter()
            .map(|&k| {
                crate::work::type_node();
                crate::TyMap::ty(self, k)
            })
            .collect();
        self.pool.row(&d)
    }
}

impl crate::TyMap for Resolve<'_, '_> {
    fn ty(&mut self, t: Ty) -> Ty {
        enum Work {
            Enter(Ty),
            Finish(Ty, usize),
        }
        let t = self.table.shallow(self.pool, t);
        if let Some(known) = self.known(t) {
            return known;
        }

        let mut children = Vec::new();
        hd_base::stack::all(Work::Enter(t), |work, stack| {
            match work {
                Work::Enter(t) => {
                    let t = self.table.shallow(self.pool, t);
                    if self.known(t).is_some() {
                        return true;
                    }
                    crate::work::resolve_node();
                    stack.push(Work::Finish(t, self.deps.len()));
                    children.clear();
                    match self.pool.get(t) {
                        TyData::Fn {
                            params,
                            result,
                            row,
                            ..
                        } => {
                            children.extend_from_slice(self.pool.list_items(params));
                            children.push(result);
                            children.push(self.pool.intern_ty(&TyData::Row(row)));
                        }
                        TyData::Context(row) => {
                            children.push(self.pool.intern_ty(&TyData::Row(row)));
                        }
                        _ => self.pool.children(t, &mut children),
                    }
                    stack.extend(children.iter().rev().map(|&c| Work::Enter(c)));
                }
                Work::Finish(t, mark) => {
                    let mut ready = ResolvedChildren {
                        table: self.table,
                        pool: self.pool,
                    };
                    let r = if let TyData::Row(row) = self.pool.get(t) {
                        self.pool.intern_ty(&TyData::Row(ready.row_keys(row)))
                    } else {
                        self.pool.map_children(t, &mut ready)
                    };
                    let stamp = self.table.trail.len();
                    let mut c = self.table.closed.borrow_mut();
                    if self.pool.has_infer(r) {
                        c.depend(self.deps.drain(mark..), t, stamp);
                        c.open.insert(t, (r, stamp));
                        c.log.push((stamp, t, true));
                        self.deps.push(Dep::Ty(t));
                    } else {
                        self.deps.truncate(mark);
                        c.map.insert(t, r);
                        c.log.push((stamp, t, false));
                    }
                }
            }
            true
        });
        crate::TyMap::ty(
            &mut ResolvedChildren {
                table: self.table,
                pool: self.pool,
            },
            t,
        )
    }

    fn row(&mut self, r: crate::RowId) -> crate::RowId {
        let t = self.ty(self.pool.intern_ty(&TyData::Row(r)));
        // ruled out: another constructor, since resolution preserves Row.
        let TyData::Row(r) = self.pool.get(t) else {
            unreachable!("resolved row")
        };
        r
    }
}

#[cfg(test)]
mod tests {
    use super::{InferTable, UnifyError, VarKind};
    use crate::pool::{Inputs, InternPool, LocalPool, RowData, RowId, Ty, TyData, Types};

    /// The variable of a fresh variable's type.
    fn var(p: Types<'_>, t: Ty) -> hd_base::InferVar {
        let TyData::Infer(v) = p.get(t) else {
            panic!("a fresh variable");
        };
        v
    }

    #[test]
    fn resolving_depth_1000_and_binding_open_types_fit_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for this generated depth test.
            .spawn(|| {
                let (gp, lp) = (InternPool::new(), LocalPool::new());
                let p = Types::with_local(&gp, &lp);
                for depth in [100, 1000] {
                    let mut table = InferTable::default();
                    let x = table.fresh(p, VarKind::General);
                    let tuple = |t, n| {
                        p.intern_ty(&TyData::Tuple {
                            elems: p.list(&vec![t; n]),
                            rest: None,
                        })
                    };
                    let (mut open, mut closed) = (x, Ty::I32);
                    for _ in 0..depth {
                        open = tuple(open, 2);
                        closed = tuple(closed, 2);
                    }
                    let (found, work) = crate::work::count_resolve_nodes(|| table.resolve(p, open));
                    assert_eq!(found, open);
                    assert_eq!(work, depth);
                    for width in [100, 1000] {
                        let root = tuple(open, width);
                        let (found, work) =
                            crate::work::count_resolve_nodes(|| table.resolve(p, root));
                        assert_eq!(found, root);
                        assert_eq!(work, 1, "shared children stay cached at either width");
                    }
                    let snapshot = table.snapshot();
                    table.unify(p, x, Ty::I32).unwrap();
                    let (found, work) = crate::work::count_resolve_nodes(|| table.resolve(p, open));
                    assert_eq!(found, closed);
                    assert_eq!(work, depth);
                    assert_eq!(
                        crate::work::count_resolve_nodes(|| table.resolve(p, open)),
                        (closed, 0)
                    );
                    table.rollback(snapshot);
                    assert_eq!(
                        crate::work::count_resolve_nodes(|| table.resolve(p, open)),
                        (open, 0)
                    );
                    // A bind calls resolve and the occurs check on an open
                    // target, so stack safety must hold on that path too.
                    let target = table.fresh(p, VarKind::General);
                    table.unify(p, target, open).unwrap();
                    assert_eq!(table.resolve(p, target), open);
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn poison_at_depth_1000_fits_a_small_stack_and_preserves_rows() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for this generated depth test.
            .spawn(|| {
                let (gp, lp) = (InternPool::new(), LocalPool::new());
                let p = Types::with_local(&gp, &lp);
                for depth in [100, 1000] {
                    let mut table = InferTable::default();
                    let x = table.fresh(p, VarKind::General);
                    let mut root = x;
                    for _ in 0..depth {
                        root = p.intern_ty(&TyData::Tuple {
                            elems: p.list(&[root, root]),
                            rest: None,
                        });
                    }
                    let snapshot = table.snapshot();
                    let (answer, work) =
                        crate::work::count_type_nodes(|| table.unify(p, Ty::POISON, root));
                    assert_eq!(answer, Ok(()));
                    assert_eq!(work, depth + 1);
                    assert_eq!(table.resolve(p, x), Ty::POISON);
                    table.rollback(snapshot);
                    assert_eq!(table.resolve(p, x), x);
                }
                let mut table = InferTable::default();
                let x = table.fresh(p, VarKind::General);
                let row_var = table.fresh(p, VarKind::General);
                let row = p.row(&RowData {
                    keys: vec![row_var],
                    params: Vec::new(),
                });
                let root = p.intern_ty(&TyData::Fn {
                    params: p.list(&[x]),
                    result: x,
                    row,
                    suspends: false,
                    inputs: Inputs::Fixed,
                    deps: crate::FnDeps::NONE,
                });
                table.unify(p, root, Ty::POISON).unwrap();
                assert_eq!(table.resolve(p, x), Ty::POISON);
                assert_eq!(table.resolve(p, row_var), row_var);
                for kind in [TyData::Row(row), TyData::Context(row)] {
                    table.unify(p, p.intern_ty(&kind), Ty::POISON).unwrap();
                    assert_eq!(table.resolve(p, row_var), row_var);
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn resolving_and_poisoning_keep_every_holder_and_row_boundary() {
        use hd_base::DefId;
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut table = InferTable::default();
        let x = table.fresh(p, VarKind::General);
        let forms = |x| {
            let args = p.list(&[x]);
            let row = p.row(&RowData {
                keys: vec![x],
                params: Vec::new(),
            });
            let def = DefId::from_raw(1);
            [
                TyData::Adt { def, args },
                TyData::Tuple {
                    elems: args,
                    rest: Some(x),
                },
                TyData::Union(args),
                TyData::Fn {
                    params: args,
                    result: x,
                    row,
                    suspends: true,
                    inputs: Inputs::Fixed,
                    deps: crate::FnDeps::NONE,
                },
                TyData::TraitValue {
                    def,
                    args,
                    bindings: vec![(DefId::from_raw(2), x)],
                },
                TyData::Opaque {
                    owner: def,
                    site: 3,
                    borrows: true,
                    args,
                    bound: x,
                },
                TyData::Assoc {
                    assoc: DefId::from_raw(2),
                    trait_: def,
                    self_ty: x,
                    args,
                },
                TyData::Ref {
                    inner: x,
                    mutable: true,
                    counted: false,
                },
                TyData::Row(row),
                TyData::Context(row),
            ]
            .map(|d| p.intern_ty(&d))
        };
        let (open, closed) = (forms(x), forms(Ty::I32));
        for &t in &open {
            assert_eq!(table.resolve(p, t), t);
        }
        let snapshot = table.snapshot();
        table.unify(p, x, Ty::I32).unwrap();
        for (&a, b) in open.iter().zip(closed) {
            assert_eq!(table.resolve(p, a), b);
        }
        table.rollback(snapshot);
        for &t in &open {
            assert_eq!(table.resolve(p, t), t);
        }
        for (index, &t) in open.iter().enumerate() {
            table.unify(p, t, Ty::POISON).unwrap();
            assert_eq!(table.resolve(p, x), if index < 8 { Ty::POISON } else { x });
            table.rollback(snapshot);
        }
    }

    #[test]
    fn resolving_shared_rows_indexes_their_keys_once() {
        use hd_base::DefId;
        for width in [100, 1000] {
            let (gp, lp) = (InternPool::new(), LocalPool::new());
            let p = Types::with_local(&gp, &lp);
            let mut table = InferTable::default();
            let x = table.fresh(p, VarKind::General);
            let keys = (0..width)
                .map(|i| {
                    p.intern_ty(&TyData::TraitValue {
                        def: DefId::from_raw(i + 1),
                        args: p.list(&[x]),
                        bindings: Vec::new(),
                    })
                })
                .collect();
            let row = p.row(&RowData {
                keys,
                params: Vec::new(),
            });
            let functions: Vec<_> = (0..width)
                .map(|i| {
                    p.intern_ty(&TyData::Fn {
                        params: crate::TyList::EMPTY,
                        result: p.intern_ty(&TyData::Adt {
                            def: DefId::from_raw(i + 2000),
                            args: crate::TyList::EMPTY,
                        }),
                        row,
                        suspends: false,
                        inputs: Inputs::Fixed,
                        deps: crate::FnDeps::NONE,
                    })
                })
                .collect();
            let root = p.intern_ty(&TyData::Tuple {
                elems: p.list(&functions),
                rest: None,
            });
            let (found, work) = crate::work::count_type_nodes(|| table.resolve(p, root));
            assert_eq!(found, root);
            assert_eq!(work, 3 * u64::from(width) + 2);
            assert_eq!(
                crate::work::count_type_nodes(|| table.resolve(p, root)),
                (root, 0)
            );
            let snapshot = table.snapshot();
            table.unify(p, x, Ty::I32).unwrap();
            let closed = table.resolve(p, root);
            assert!(!p.has_infer(closed));
            table.rollback(snapshot);
            assert_eq!(
                crate::work::count_type_nodes(|| table.resolve(p, root)),
                (root, 0)
            );
        }
    }

    #[test]
    fn data_row_argument_inference_binds_the_whole_row() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut table = InferTable::default();
        let variable = table.fresh(p, VarKind::General);
        let inferred = p.row(&RowData {
            keys: vec![variable],
            params: Vec::new(),
        });
        let concrete = p.row(&RowData {
            keys: vec![Ty::I32, Ty::STRING],
            params: Vec::new(),
        });
        let a = p.intern_ty(&TyData::Row(inferred));
        let b = p.intern_ty(&TyData::Row(concrete));
        table.unify(p, a, b).expect("infer the data row argument");
        assert_eq!(table.resolve_row(p, inferred), concrete);
    }

    /// `unify_context_rows` pairs keys by trait through one index per
    /// row: `n` keys `Gen_i[?v_i]` against `Gen_i[i32]` bind each variable,
    /// and the counted work at `10n` keys is ten times that at `n`, not a
    /// hundred (each key once scanned the whole row for its trait).
    #[test]
    fn context_rows_pair_their_keys_in_linear_work() {
        let work = |n: u32| {
            let (gp, lp) = (InternPool::new(), LocalPool::new());
            let p = Types::with_local(&gp, &lp);
            let mut table = InferTable::default();
            let key = |i: u32, arg: Ty| {
                p.intern_ty(&TyData::TraitValue {
                    def: hd_base::DefId::from_raw(100 + i),
                    args: p.list(&[arg]),
                    bindings: vec![],
                })
            };
            let vars: Vec<Ty> = (0..n).map(|_| table.fresh(p, VarKind::General)).collect();
            let open = p.row(&RowData {
                keys: (0..n).map(|i| key(i, vars[i as usize])).collect(),
                params: Vec::new(),
            });
            let ground = p.row(&RowData {
                keys: (0..n).map(|i| key(i, Ty::I32)).collect(),
                params: Vec::new(),
            });
            let (a, b) = (
                p.intern_ty(&TyData::Context(open)),
                p.intern_ty(&TyData::Context(ground)),
            );
            let (r, count) = crate::work::count_type_nodes(|| table.unify(p, a, b));
            r.expect("each key binds its variable");
            for &v in &vars {
                assert_eq!(table.resolve(p, v), Ty::I32);
            }
            count
        };
        let (small, large) = (work(40), work(400));
        assert!(
            large <= 10 * small + 64,
            "context unify work for 40 and 400 keys: {small} and {large}"
        );
    }

    #[test]
    fn shallow_preserves_global_types_without_a_local_pool() {
        let gp = InternPool::new();
        let p = gp.types();
        let trait_value = p.intern_ty(&TyData::TraitValue {
            def: hd_base::DefId::from_raw(7),
            args: p.list(&[Ty::I32, Ty::BOOL]),
            bindings: vec![(hd_base::DefId::from_raw(8), Ty::STRING)],
        });
        let function = p.intern_ty(&TyData::Fn {
            params: p.list(&[trait_value]),
            result: Ty::STRING,
            row: RowId::EMPTY,
            suspends: false,
            inputs: Inputs::Fixed,
            deps: crate::pool::FnDeps::NONE,
        });
        let param = p.intern_ty(&TyData::Param(crate::ParamRef {
            owner: hd_base::DefId::from_raw(9),
            index: 0,
        }));
        let table = InferTable::default();
        for ty in [
            Ty::I32,
            Ty::NEVER,
            Ty::POISON,
            Ty::VOID,
            trait_value,
            function,
            param,
            p.intern_ty(&TyData::Canon(0)),
        ] {
            assert!(!ty.is_local());
            assert_eq!(table.shallow(p, ty), ty);
        }
    }

    #[test]
    fn shallow_canonicalizes_unbound_classes_and_follows_global_bindings() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut table = InferTable::default();
        let a = table.fresh(p, VarKind::General);
        let b = table.fresh(p, VarKind::General);
        let alias = table.fresh(p, VarKind::General);
        assert_eq!(table.shallow(p, a), a);
        table.unify(p, a, b).expect("join classes");
        table.alias(p, var(p, alias), a);
        let root = table.shallow(p, a);
        assert_eq!(table.shallow(p, b), root);
        assert_eq!(table.shallow(p, alias), root);
        let before = lp.len();
        assert_eq!(table.shallow(p, root), root);
        assert_eq!(lp.len(), before);
        let snap = table.snapshot();
        let global = p.intern_ty(&TyData::Union(p.list(&[Ty::I32])));
        table.unify(p, b, global).expect("bind class");
        assert!(!global.is_local());
        for ty in [a, b, alias] {
            assert_eq!(table.shallow(p, ty), global);
        }
        table.rebind(p, a, Ty::STRING);
        assert_eq!(table.shallow(p, alias), Ty::STRING);
        table.rollback(snap);
        for ty in [a, b, alias] {
            assert_eq!(table.shallow(p, ty), root);
        }
    }

    #[test]
    fn shallow_stops_at_local_composites_even_after_their_variables_bind() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut table = InferTable::default();
        let a = table.fresh(p, VarKind::General);
        let b = table.fresh(p, VarKind::General);
        let nested = p.intern_ty(&TyData::TraitValue {
            def: hd_base::DefId::from_raw(7),
            args: p.list(&[b]),
            bindings: vec![(hd_base::DefId::from_raw(8), b)],
        });
        assert!(nested.is_local());
        table.alias(p, var(p, a), nested);
        assert_eq!(table.shallow(p, a), nested);
        assert_eq!(table.shallow(p, nested), nested);
        table.unify(p, b, Ty::I32).expect("bind nested variable");
        assert_eq!(table.shallow(p, a), nested);
        let resolved = table.resolve(p, a);
        assert!(!resolved.is_local());
        assert_eq!(
            p.get(resolved),
            TyData::TraitValue {
                def: hd_base::DefId::from_raw(7),
                args: p.list(&[Ty::I32]),
                bindings: vec![(hd_base::DefId::from_raw(8), Ty::I32)],
            }
        );
    }

    /// Poison binds the variables of the other side, nested or not, from
    /// either side; a rollback unbinds them.
    #[test]
    fn poison_binds_the_variables_of_the_other_side() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let lit = t.fresh(p, VarKind::IntLit);
        let func = p.intern_ty(&TyData::Fn {
            params: p.list(&[a, Ty::STRING]),
            result: lit,
            row: RowId::EMPTY,
            suspends: false,
            inputs: Inputs::Fixed,
            deps: crate::pool::FnDeps::NONE,
        });
        let snap = t.snapshot();
        t.unify(p, Ty::POISON, func).expect("poison unifies");
        assert_eq!(t.resolve(p, a), Ty::POISON);
        assert_eq!(t.resolve(p, lit), Ty::POISON);
        t.rollback(snap);
        assert_eq!(t.resolve(p, a), a);
        let pair = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::I32, a]),
            rest: None,
        });
        t.unify(p, pair, Ty::POISON).expect("poison unifies");
        assert_eq!(t.resolve(p, a), Ty::POISON);
        // A type without variables is left as it is.
        t.unify(p, Ty::I32, Ty::POISON).expect("poison unifies");
    }

    /// `Fn[?A, i32, $()]` against a function type of any arity solves
    /// `?A` as that type's inputs tuple; against a different result or a
    /// suspending type it fails.
    #[test]
    fn tuple_inputs_unify_with_any_arity() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let func = |params: &[Ty], inputs: Inputs, result: Ty, suspends: bool| {
            p.intern_ty(&TyData::Fn {
                params: p.list(params),
                result,
                row: RowId::EMPTY,
                suspends,
                inputs,
                deps: crate::pool::FnDeps::NONE,
            })
        };
        let tuple = |elems: &[Ty], rest: Option<Ty>| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(elems),
                rest,
            })
        };
        let cases = [
            (vec![], Inputs::Fixed, tuple(&[], None)),
            (vec![Ty::I32], Inputs::Fixed, tuple(&[Ty::I32], None)),
            (
                vec![Ty::I32, Ty::STRING, Ty::BOOL],
                Inputs::Fixed,
                tuple(&[Ty::I32, Ty::STRING, Ty::BOOL], None),
            ),
            (
                vec![Ty::I32, Ty::STRING],
                Inputs::Rest,
                tuple(&[Ty::I32], Some(Ty::STRING)),
            ),
        ];
        for (params, inputs, want) in cases {
            let mut t = InferTable::default();
            let a = t.fresh(p, VarKind::General);
            let generic = func(&[a], Inputs::Tuple, Ty::I32, false);
            let concrete = func(&params, inputs, Ty::I32, false);
            t.unify(p, generic, concrete).expect("unify");
            assert_eq!(t.resolve(p, a), want);
            assert_eq!(t.resolve(p, generic), concrete);
            let mut t = InferTable::default();
            let b = t.fresh(p, VarKind::General);
            let back = func(&[b], Inputs::Tuple, Ty::I32, false);
            t.unify(p, concrete, back).expect("either side");
            assert_eq!(t.resolve(p, b), want);
        }
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let generic = func(&[a], Inputs::Tuple, Ty::I32, false);
        let wrong_result = func(&[Ty::I32], Inputs::Fixed, Ty::STRING, false);
        assert!(t.unify(p, generic, wrong_result).is_err());
        let suspending = func(&[Ty::I32], Inputs::Fixed, Ty::I32, true);
        assert!(t.unify(p, generic, suspending).is_err());
        let plain = func(&[Ty::I32], Inputs::Fixed, Ty::I32, false);
        let rest = func(&[Ty::I32], Inputs::Rest, Ty::I32, false);
        assert!(t.unify(p, plain, rest).is_err());
    }

    #[test]
    fn unify_resolve_rollback_and_occurs() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let opt_a = p.intern_ty(&TyData::Union(p.list(&[a])));
        let opt_i32 = p.intern_ty(&TyData::Union(p.list(&[Ty::I32])));
        let snap = t.snapshot();
        t.unify(p, opt_a, opt_i32).expect("unify");
        assert_eq!(t.resolve(p, opt_a), opt_i32);
        t.rollback(snap);
        assert_eq!(t.resolve(p, opt_a), opt_a);
        assert!(matches!(
            t.unify(p, a, opt_a),
            Err(UnifyError::Occurs { .. })
        ));
        let lit = t.fresh(p, VarKind::IntLit);
        assert!(matches!(
            t.unify(p, lit, Ty::STRING),
            Err(UnifyError::Kind { .. })
        ));
        assert!(t.unify(p, lit, Ty::POISON).is_ok());
    }

    /// #1891: a closed resolution is kept across calls until a rollback
    /// undoes a binding under it or a `rebind` replaces one; a rollback
    /// keeps the results resolved before its snapshot. `may_hold` counts
    /// live bindings holding poison, so it follows binds, rebinds and
    /// rollbacks.
    #[test]
    fn closed_resolutions_and_held_flags_follow_the_trail() {
        use super::holds;
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let b = t.fresh(p, VarKind::General);
        let opt = |x: Ty| p.intern_ty(&TyData::Union(p.list(&[x])));
        let pair = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[a, b]),
            rest: None,
        });
        t.unify(p, a, Ty::I32).expect("a");
        assert_eq!(t.resolve(p, opt(a)), opt(Ty::I32));
        assert_eq!(t.shallow_or_closed(p, opt(a)), opt(Ty::I32));
        assert!(!t.may_hold(p, pair, holds::POISON));
        let outer = t.snapshot();
        t.unify(p, b, Ty::STRING).expect("b");
        let closed = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::I32, Ty::STRING]),
            rest: None,
        });
        assert_eq!(t.resolve(p, pair), closed);
        assert_eq!(t.shallow_or_closed(p, pair), closed);
        // A trial that binds a variable to poison and is dropped.
        let mark = t.trial_mark();
        let c = t.fresh(p, VarKind::General);
        t.unify(p, c, Ty::POISON).expect("c");
        assert!(t.may_hold(p, pair, holds::POISON));
        assert!(!t.may_hold(p, Ty::I32, holds::POISON));
        t.rollback_trial(mark);
        assert!(!t.may_hold(p, pair, holds::POISON));
        // Undoing `b` drops the pair's result, not `opt(a)`'s.
        t.rollback(outer);
        assert_eq!(t.shallow_or_closed(p, pair), pair);
        assert_eq!(t.shallow_or_closed(p, opt(a)), opt(Ty::I32));
        assert_eq!(
            t.resolve(p, pair),
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[Ty::I32, b]),
                rest: None
            })
        );
        // A rebind replaces `a`'s solution: every kept result goes, and
        // its undo restores the flags the old binding held.
        let snap = t.snapshot();
        t.rebind(p, a, Ty::POISON);
        assert!(t.may_hold(p, opt(a), holds::POISON));
        assert_eq!(t.shallow_or_closed(p, opt(a)), opt(a));
        assert_eq!(t.resolve(p, opt(a)), opt(Ty::POISON));
        t.rollback(snap);
        assert!(!t.may_hold(p, opt(a), holds::POISON));
        assert_eq!(t.resolve(p, opt(a)), opt(Ty::I32));
    }

    /// Union by rank: unions keep classes shallow, kinds merge at the
    /// root, and a rollback restores every link, rank and kind exactly.
    #[test]
    fn union_by_rank_rolls_back_exactly() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let vars: Vec<Ty> = (0..16).map(|_| t.fresh(p, VarKind::General)).collect();
        let lit = t.fresh(p, VarKind::IntLit);
        let before: Vec<Ty> = vars.iter().map(|v| t.shallow(p, *v)).collect();
        let snap = t.snapshot();
        for w in vars.windows(2) {
            t.unify(p, w[0], w[1]).expect("unify vars");
        }
        t.unify(p, vars[7], lit).expect("unify literal");
        let root = t.shallow(p, vars[0]);
        assert!(vars.iter().all(|v| t.shallow(p, *v) == root));
        assert_eq!(t.kind_of(p, vars[3]), Some(VarKind::IntLit));
        assert!(t.rank.iter().all(|r| *r <= 5), "log2(17) bounds the rank");
        t.unify(p, vars[0], Ty::I32).expect("bind");
        assert_eq!(t.resolve(p, vars[15]), Ty::I32);
        t.rollback(snap);
        let after: Vec<Ty> = vars.iter().map(|v| t.shallow(p, *v)).collect();
        assert_eq!(before, after);
        assert!(t.rank.iter().all(|r| *r == 0));
        assert_eq!(t.kind_of(p, vars[3]), Some(VarKind::General));
        assert_eq!(t.kind_of(p, lit), Some(VarKind::IntLit));
    }

    /// A failed trial leaves no inference trace: older variables that the
    /// trial linked to, bound through, or re-kinded with its own new
    /// variables come back exactly, and the new variables are gone.
    #[test]
    fn a_trial_rollback_drops_its_variables_and_undoes_every_link() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let old: Vec<Ty> = (0..4).map(|_| t.fresh(p, VarKind::General)).collect();
        t.unify(p, old[0], old[1]).expect("before the trial");
        let before = format!("{t:?}");
        let mark = t.trial_mark();
        let new = t.fresh(p, VarKind::General);
        let lit = t.fresh(p, VarKind::IntLit);
        t.unify(p, old[2], new)
            .expect("an old variable joins a new one");
        t.unify(p, new, lit).expect("the class becomes a literal");
        t.unify(p, old[0], p.intern_ty(&TyData::Union(p.list(&[new]))))
            .expect("an old class binds through a new variable");
        t.mark_signed(p, lit);
        assert_eq!(t.kind_of(p, old[2]), Some(VarKind::SignedIntLit));
        t.rollback_trial(mark);
        assert_eq!(format!("{t:?}"), before, "every column as before");
        assert_eq!(t.var_count(), 4);
        assert_eq!(t.kind_of(p, old[2]), Some(VarKind::General));
        assert_eq!(t.resolve(p, old[0]), t.shallow(p, old[1]));
    }

    /// `types.tuple.rest.same`: tuples with rest elements unify their
    /// fixed elements and their rest elements pairwise, and a tuple with a
    /// rest element never meets one without.
    #[test]
    fn rest_tuples_unify_elementwise_and_never_with_plain_tuples() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let list_of = |x: Ty| {
            p.intern_ty(&TyData::Adt {
                def: hd_base::DefId::from_raw(7),
                args: p.list(&[x]),
            })
        };
        let rest_tuple = |fixed: Ty, item: Ty| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[fixed]),
                rest: Some(list_of(item)),
            })
        };
        let (a, b) = (t.fresh(p, VarKind::General), t.fresh(p, VarKind::General));
        t.unify(p, rest_tuple(a, b), rest_tuple(Ty::I32, Ty::BOOL))
            .expect("fixed and rest elements unify");
        assert_eq!(t.resolve(p, a), Ty::I32);
        assert_eq!(t.resolve(p, b), Ty::BOOL);
        let plain = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::I32]),
            rest: None,
        });
        assert!(matches!(
            t.unify(p, rest_tuple(Ty::I32, Ty::BOOL), plain),
            Err(UnifyError::Mismatch { .. })
        ));
        assert!(matches!(
            t.unify(
                p,
                rest_tuple(Ty::I32, Ty::BOOL),
                rest_tuple(Ty::I32, Ty::I32)
            ),
            Err(UnifyError::Mismatch { .. })
        ));
        let c = t.fresh(p, VarKind::General);
        assert!(matches!(
            t.unify(p, c, rest_tuple(c, Ty::I32)),
            Err(UnifyError::Occurs { .. })
        ));
    }

    /// `req.context.row`: context types unify as key sets, a key whose
    /// trait occurs once on each side fixing its arguments.
    #[test]
    fn context_types_unify_as_key_sets() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let key = |d: u32, args: &[Ty]| {
            p.intern_ty(&TyData::TraitValue {
                def: hd_base::DefId::from_raw(d),
                args: p.list(args),
                bindings: Vec::new(),
            })
        };
        let ctx = |keys: Vec<Ty>| {
            p.intern_ty(&TyData::Context(p.row(&crate::RowData {
                keys,
                params: Vec::new(),
            })))
        };
        let a = t.fresh(p, VarKind::General);
        let log = key(3, &[]);
        t.unify(
            p,
            ctx(vec![key(2, &[a]), log]),
            ctx(vec![log, key(2, &[Ty::I32])]),
        )
        .expect("one Repo key on each side");
        assert_eq!(t.resolve(p, a), Ty::I32);
        assert!(matches!(
            t.unify(p, ctx(vec![key(2, &[Ty::I32])]), ctx(vec![log])),
            Err(UnifyError::Mismatch { .. })
        ));
        assert!(matches!(
            t.unify(p, ctx(vec![log]), ctx(vec![log, key(2, &[Ty::I32])])),
            Err(UnifyError::Mismatch { .. })
        ));
    }

    /// A type that shares its parts (a DAG of 2^40 leaves): resolve
    /// substitutes through every shared node, the occurs check finds the
    /// variable through each form, and both count each distinct node once.
    #[test]
    fn shared_types_are_walked_once_per_node() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let a = t.fresh(p, VarKind::General);
        let b = t.fresh(p, VarKind::General);
        let pair = |x: Ty| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[x, x]),
                rest: None,
            })
        };
        let mut deep = a;
        for _ in 0..40 {
            deep = pair(deep);
        }
        let ((), nodes) = crate::work::count_type_nodes(|| {
            t.unify(p, a, Ty::I32).expect("bind");
            let mut want = Ty::I32;
            for _ in 0..40 {
                want = pair(want);
            }
            assert_eq!(t.resolve(p, deep), want);
        });
        assert!(nodes <= 41, "one visit per distinct node: {nodes}");
        let mut deep_b = b;
        for _ in 0..40 {
            deep_b = pair(deep_b);
        }
        let forms = [
            deep_b,
            p.intern_ty(&TyData::Adt {
                def: hd_base::DefId::from_raw(7),
                args: p.list(&[deep_b]),
            }),
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[Ty::I32]),
                rest: Some(deep_b),
            }),
            p.intern_ty(&TyData::Union(p.list(&[deep_b]))),
            p.intern_ty(&TyData::Ref {
                inner: deep_b,
                mutable: false,
                counted: false,
            }),
            p.intern_ty(&TyData::Fn {
                params: p.list(&[Ty::STRING]),
                result: deep_b,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps: crate::pool::FnDeps::NONE,
            }),
            p.intern_ty(&TyData::Opaque {
                owner: hd_base::DefId::from_raw(26),
                site: 0,
                borrows: false,
                args: p.list(&[deep_b]),
                bound: Ty::I32,
            }),
            p.intern_ty(&TyData::Opaque {
                owner: hd_base::DefId::from_raw(26),
                site: 0,
                borrows: false,
                args: p.list(&[]),
                bound: deep_b,
            }),
            p.intern_ty(&TyData::Fn {
                params: p.list(&[deep_b]),
                result: Ty::I32,
                row: RowId::EMPTY,
                suspends: false,
                inputs: Inputs::Fixed,
                deps: crate::pool::FnDeps::NONE,
            }),
        ];
        for form in forms {
            let (found, nodes) = crate::work::count_type_nodes(|| t.unify(p, b, form));
            assert!(
                matches!(found, Err(UnifyError::Occurs { .. })),
                "{:?}",
                p.get(form)
            );
            // The occurs check reads the bound type resolved: one resolve
            // and one occurs visit per distinct node.
            assert!(nodes <= 2 * 45, "two visits per distinct node: {nodes}");
        }
    }

    /// An open resolution is kept while its unbound variables stay unbound
    /// (#1912), however many it holds (#2005): the next resolve reuses it,
    /// a binding or union of one kills it and every entry above it, an
    /// entry that does not read the variable stays, and a rollback drops
    /// what a trial kept and revives what it killed.
    #[test]
    fn open_resolutions_hold_while_their_variables_stay_unbound() {
        let (gp, lp) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&gp, &lp);
        let mut t = InferTable::default();
        let tuple = |xs: &[Ty]| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(xs),
                rest: None,
            })
        };
        let (a, b) = (t.fresh(p, VarKind::General), t.fresh(p, VarKind::General));
        let x = t.fresh(p, VarKind::General);
        t.unify(p, x, tuple(&[a, a])).expect("bind");
        let outer = tuple(&[x, Ty::I32]);
        let walk = |t: &InferTable, ty: Ty| crate::work::count_type_nodes(|| t.resolve(p, ty));
        // Binding `x` resolved its type, and kept it: only `outer` is new.
        assert_eq!(walk(&t, outer), (tuple(&[tuple(&[a, a]), Ty::I32]), 1));
        // Kept: one lookup, no walk.
        assert_eq!(walk(&t, outer).1, 0);
        // A trial's binding of `a` makes it stale; the rollback drops the
        // trial's own resolution and the old one holds again.
        let s = t.snapshot();
        t.unify(p, a, Ty::I32).expect("bind");
        assert_eq!(
            walk(&t, outer),
            (tuple(&[tuple(&[Ty::I32, Ty::I32]), Ty::I32]), 2)
        );
        t.rollback(s);
        assert_eq!(walk(&t, outer), (tuple(&[tuple(&[a, a]), Ty::I32]), 0));
        // Nine variables: binding `y` resolved the tuple and kept it, and
        // the type around it is kept too.
        let many: Vec<Ty> = (0..9).map(|_| t.fresh(p, VarKind::General)).collect();
        let y = t.fresh(p, VarKind::General);
        t.unify(p, y, tuple(&many)).expect("bind");
        let around = tuple(&[y, b, y]);
        assert_eq!(
            walk(&t, around),
            (tuple(&[tuple(&many), b, tuple(&many)]), 1)
        );
        assert_eq!(walk(&t, around).1, 0);
        // A trial's union of the tuple's last variable kills the tuple and
        // `around`, not `outer`; the rollback revives both.
        let s = t.snapshot();
        t.unify(p, many[8], b).expect("union");
        assert_eq!(walk(&t, around).1, 2);
        assert_eq!(walk(&t, outer).1, 0);
        t.rollback(s);
        assert_eq!(walk(&t, around).1, 0);
        // A union that leaves the tuple's variable the root kills only
        // the type that reads the other one.
        let (c, d) = (t.fresh(p, VarKind::General), t.fresh(p, VarKind::General));
        let (cd, dc) = (tuple(&[c, Ty::I32]), tuple(&[d, Ty::I32]));
        assert_eq!(walk(&t, cd).1 + walk(&t, dc).1, 2);
        t.unify(p, d, c).expect("union");
        assert_eq!(walk(&t, cd), (cd, 0));
        assert_eq!(walk(&t, dc), (cd, 1));
        // An alias binding and a poisoning kill what read the variable.
        let (e, f) = (t.fresh(p, VarKind::General), t.fresh(p, VarKind::General));
        let ef = tuple(&[e, f]);
        assert_eq!(walk(&t, ef).1, 1);
        t.alias(p, var(p, e), Ty::I32);
        assert_eq!(walk(&t, ef), (tuple(&[Ty::I32, f]), 1));
        t.unify(p, f, Ty::POISON).expect("poison");
        assert_eq!(walk(&t, ef), (tuple(&[Ty::I32, Ty::POISON]), 1));
    }

    /// #2005: a nest whose level `i` holds `i` distinct variables,
    /// `(?i, (?i-1, ... (?1,)))`, resolved at every level, walks each
    /// level once, and a binding of the innermost variable kills each
    /// level once: linear in the depth at `n` and `10n`, where a memo
    /// that keeps only results with few variables walks `n^2 / 2` nodes.
    #[test]
    fn a_nest_of_distinct_variables_resolves_and_kills_each_level_once() {
        let nodes = |n: usize| {
            let (gp, lp) = (InternPool::new(), LocalPool::new());
            let p = Types::with_local(&gp, &lp);
            let mut t = InferTable::default();
            let first = t.fresh(p, VarKind::General);
            let mut level = first;
            let mut levels = Vec::new();
            for _ in 0..n {
                let v = t.fresh(p, VarKind::General);
                level = p.intern_ty(&TyData::Tuple {
                    elems: p.list(&[v, level]),
                    rest: None,
                });
                levels.push(level);
            }
            let ((), count) = crate::work::count_type_nodes(|| {
                for &l in &levels {
                    let _ = t.resolve(p, l);
                }
                t.unify(p, first, Ty::I32).expect("bind");
                let _ = t.resolve(p, level);
            });
            count
        };
        let (small, large) = (nodes(40), nodes(400));
        assert!(
            small > 0 && large <= 11 * small,
            "type nodes at depth 40 and 400: {small} and {large}"
        );
    }
}
