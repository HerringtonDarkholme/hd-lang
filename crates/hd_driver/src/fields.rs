//! Single-owner fields (#1399 step 15d1), facts only: per struct instance
//! `S` and collection field `f`, over every body the program reaches in a
//! release build, whether the buffer `S.f` holds ever has a second owner.
//!
//! `S.f` has one owner when:
//! 1. no statement copies with a count a value of `S`, of a type that holds
//!    an `S` in place (a tuple, struct, option or collection of it, not
//!    behind a reference), or `S.f` itself: a `copy` read, or a closure's
//!    `Copy` capture, or a borrow a use takes a count of (other moves and
//!    borrows are fine). A read of a local bound to a borrow reads what that
//!    borrow names, and a copy-update copies each field it keeps;
//! 2. every construction of an `S`, including a copy-update's replacements,
//!    and every store of all of `S.f` puts a fresh buffer there: a new
//!    collection that is no top-level binding's
//!    initializer (only such a literal of constants is static data the
//!    backend shares), or the move of a local whose every store is one;
//! 3. no body copies or stores, through a reference it can't place, a
//!    header of `f`'s type that is not fresh (`*r = xs` with `r: &mut
//!    List[T]`, or `xs := *r`): such a place may be any field of that type.
//!
//! A pointer alias (`*mut S`) shares the storage, not the buffer, so it
//! adds no owner. The fact is computed after collection, over the bodies
//! the link emits; the link marks each check of a one-owner field that
//! stands at its change `unique_word::OWNED` (step 15d2), and the backend
//! runs no check for it.
//!
//! A collection local has one owner by the same rules, per instance
//! ([`owned_locals`]), and only until something shares it: a share after its
//! last check can't reach one.

// The browser compiler runs the dev pipeline only, which never asks.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use hd_base::{HashMap, HashSet, InstId};
use hd_mono::ProgramEnv;
use hd_mono::layout::StdKind;
use hd_tir::ir::{
    Body, Callee, CaptureMode, Proj, ReadMode, Ref, Role, Root, Tag, local_flags, ref_word,
};
use hd_types::{InternPool, Ty, TyData, TyList};

/// An instance body the link emits: its id, body, item, type arguments.
pub(crate) type Instance<'b> = (InstId, &'b Body, hd_base::DefId, TyList);

/// Why each field has a second owner, or none for one owner, and the
/// collection types that rule 3 may share behind any reference.
#[derive(Default)]
pub(crate) struct Verdicts {
    fields: HashMap<(Ty, u32), Option<&'static str>>,
    blind: HashSet<Ty>,
}

impl Verdicts {
    pub(crate) fn one_owner(&self, field: (Ty, u32)) -> bool {
        self.fields.get(&field).is_some_and(Option::is_none)
    }
}

struct Walk<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    /// Fields a check or a store names: the candidates.
    fields: HashSet<(Ty, u32)>,
    bad: HashMap<(Ty, u32), &'static str>,
    /// Types copied with a count, and collection types copied or stored
    /// unfresh through a place the walk can't name a field of.
    copied: HashSet<Ty>,
    blind: HashSet<Ty>,
}

impl<'a> Walk<'a> {
    fn new(pool: &'a InternPool, env: &'a dyn ProgramEnv) -> Self {
        Walk {
            pool,
            env,
            fields: HashSet::default(),
            bad: HashMap::default(),
            copied: HashSet::default(),
            blind: HashSet::default(),
        }
    }

    fn strip(&self, mut t: Ty) -> Ty {
        while let TyData::Mut(inner) = self.pool.get(t) {
            t = inner;
        }
        t
    }

    fn coll(&self, t: Ty) -> bool {
        matches!(self.pool.get(self.strip(t)), TyData::Adt { def, .. }
            if matches!(self.env.std_kind(def), StdKind::List | StdKind::Map))
    }

    fn fields_of(&self, t: Ty) -> Option<Vec<Ty>> {
        let TyData::Adt { def, args } = self.pool.get(self.strip(t)) else {
            return None;
        };
        if !self.env.is_struct(def) {
            return None;
        }
        let fs = self.env.data_fields(def)?;
        Some(
            fs.iter()
                .map(|&f| hd_mono::subst(self.pool, self.env, def, args, f))
                .collect(),
        )
    }

    /// The type of place `p`'s prefix before its last step, and that step.
    fn prefix(&self, b: &Body, sub: &dyn Fn(Ty) -> Ty, word: u32) -> Option<(Ty, Proj)> {
        let p = b.view(Self::resolve(b, word));
        let mut t = match p.root {
            Root::Local(l) => sub(b.local_ty[l as usize]),
            Root::Value(v) => sub(b.ty[v as usize]),
            Root::Global(_, t) => t,
            _ => return None,
        };
        let steps: Vec<Proj> = p.steps(b).collect();
        let (&last, init) = steps.split_last()?;
        for s in init {
            t = self.step(t, *s)?;
        }
        Some((self.strip(t), last))
    }

    /// The place a whole read of a read's value names: that read's.
    fn resolve(b: &Body, mut word: u32) -> u32 {
        for _ in 0..8 {
            let p = b.view(word);
            match p.root {
                Root::Value(v) if p.whole() && b.tags[v as usize] == Tag::Read => {
                    word = b.data[v as usize][0];
                }
                _ => break,
            }
        }
        word
    }

    /// Whether place `word` may be any field of its type: one behind a
    /// reference, or one the walk can't type. All of a local is none.
    fn blind(&self, b: &Body, sub: &dyn Fn(Ty) -> Ty, word: u32) -> bool {
        let word = Self::resolve(b, word);
        !b.view(word).whole() && matches!(self.prefix(b, sub, word), None | Some((_, Proj::Deref)))
    }

    fn step(&self, t: Ty, s: Proj) -> Option<Ty> {
        let t = self.strip(t);
        match s {
            // A reference is `TyData::Ref`, or a reference cell type
            // (`Ref[T]`, `Counted[T]`) whose one argument is its target.
            Proj::Deref => match self.pool.get(t) {
                TyData::Ref { inner, .. } => Some(inner),
                TyData::Adt { def, args }
                    if !self.env.is_struct(def)
                        && matches!(self.env.std_kind(def), StdKind::Other) =>
                {
                    self.pool.list_items(args).first().copied()
                }
                _ => None,
            },
            Proj::Field(k) => match self.pool.get(t) {
                TyData::Tuple { elems, .. } => self.pool.list_items(elems).get(k as usize).copied(),
                _ => self.fields_of(t)?.get(k as usize).copied(),
            },
            Proj::Elem(_) => match self.pool.get(t) {
                TyData::Adt { def, args } => {
                    let items = self.pool.list_items(args);
                    match self.env.std_kind(def) {
                        StdKind::List => items.first().copied(),
                        StdKind::Map => items.get(1).copied(),
                        _ => None,
                    }
                }
                _ => None,
            },
            Proj::Variant(_) => None,
        }
    }

    /// The field place `word` names: its struct and field index.
    fn field(&self, b: &Body, sub: &dyn Fn(Ty) -> Ty, word: u32) -> Option<(Ty, u32)> {
        match self.prefix(b, sub, word)? {
            (s, Proj::Field(k)) if self.fields_of(s).is_some() => Some((s, k)),
            _ => None,
        }
    }

    /// Whether value `v` is a fresh buffer: a new collection that is no
    /// top-level binding's list literal, which may be static data the
    /// backend shares (`static_scalar_list`).
    fn fresh_new(b: &Body, v: u32) -> bool {
        let Some(i) = Ref(v).as_inst().map(|r| r.0 as usize) else {
            return false;
        };
        match b.tags[i] {
            Tag::NewMap => true,
            Tag::NewList => !b.top_level_initializer(i),
            _ => false,
        }
    }

    fn body(&mut self, b: &Body, sub: &dyn Fn(Ty) -> Ty) {
        let n = b.len();
        // What a binding of a borrow names: a read of the binding reads that.
        let stores = whole_stores(b);
        let mut bound: Vec<Option<u32>> = vec![None; b.local_ty.len()];
        for i in 0..n {
            if binds(b, &stores, i)
                && let Some(l) = b.view(b.data[i][0]).local()
                && let Some(v) = Ref(b.data[i][1]).as_inst().map(|v| v.0 as usize)
                && b.tags[v] == Tag::Read
                && b.data[v][1] == ReadMode::Borrow as u32
            {
                bound[l as usize] = Some(b.data[v][0]);
            }
        }
        let through = |mut word: u32| {
            for _ in 0..8 {
                word = Self::resolve(b, word);
                let p = b.view(word);
                match p.local().filter(|_| p.whole()) {
                    Some(l) if bound[l as usize].is_some() => {
                        word = bound[l as usize].unwrap_or(word);
                    }
                    _ => break,
                }
            }
            word
        };
        // Locals every whole store of which is a fresh new collection.
        let mut fresh_local = vec![true; b.local_ty.len()];
        for i in 0..n {
            if b.tags[i] == Tag::Store {
                let p = b.view(b.data[i][0]);
                if p.whole()
                    && let Some(l) = p.local()
                {
                    fresh_local[l as usize] &= Self::fresh_new(b, b.data[i][1]);
                }
            }
        }
        let fresh = |v: u32| {
            Self::fresh_new(b, v)
                || Ref(v).as_inst().is_some_and(|r| {
                    let i = r.0 as usize;
                    b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Move as u32 && {
                        let p = b.view(b.data[i][0]);
                        p.whole() && p.local().is_some_and(|l| fresh_local[l as usize])
                    }
                })
        };
        for i in 0..n {
            match b.tags[i] {
                Tag::IsUnique => {
                    if let Some(f) = self.field(b, sub, b.data[i][0]) {
                        self.fields.insert(f);
                    }
                }
                Tag::Read if b.data[i][1] == ReadMode::Copy as u32 => {
                    let t = sub(b.ty[i]);
                    self.copied.insert(self.strip(t));
                    if self.coll(t) {
                        let place = through(b.data[i][0]);
                        match self.field(b, sub, place) {
                            Some(f) => {
                                self.bad.entry(f).or_insert("a copy of the field");
                            }
                            // A place the walk can't type, or one behind a
                            // reference, may be any field of this type.
                            None if self.blind(b, sub, place) => {
                                self.blind.insert(self.strip(t));
                            }
                            None => {}
                        }
                    }
                }
                Tag::Store => {
                    let v = b.data[i][1];
                    let t = sub(b
                        .ty
                        .get(Ref(v).as_inst().map_or(usize::MAX, |r| r.0 as usize))
                        .copied()
                        .unwrap_or(Ty::NEVER));
                    match self.field(b, sub, b.data[i][0]) {
                        Some(f) if self.coll(t) => {
                            self.fields.insert(f);
                            if !fresh(v) {
                                self.bad.entry(f).or_insert("a store of a shared header");
                            }
                        }
                        None if self.coll(t) && !fresh(v) && self.blind(b, sub, b.data[i][0]) => {
                            self.blind.insert(self.strip(t));
                        }
                        _ => {}
                    }
                }
                // Every field a copy-update keeps takes a count from its base
                // (`copy_data`); each replacement is a store of that field.
                Tag::CopyStruct => {
                    let s = self.strip(sub(b.ty[i]));
                    let Some(fs) = self.fields_of(s) else {
                        continue;
                    };
                    let shared = Ref(b.data[i][0]).as_inst().is_some_and(|v| {
                        let v = v.0 as usize;
                        b.tags[v] == Tag::Read && b.data[v][1] != ReadMode::Move as u32
                    });
                    let pairs = b.record(b.data[i][1]);
                    for (k, &ft) in fs.iter().enumerate() {
                        let f = (s, u32::try_from(k).unwrap_or(u32::MAX));
                        let value = pairs.chunks(2).find(|w| w[0] as usize == k).map(|w| w[1]);
                        match value {
                            Some(v) if self.coll(ft) => {
                                self.fields.insert(f);
                                if !fresh(v) {
                                    self.bad.entry(f).or_insert("a store of a shared header");
                                }
                            }
                            None if shared => {
                                self.copied.insert(self.strip(ft));
                                if self.coll(ft) {
                                    self.fields.insert(f);
                                    self.bad.entry(f).or_insert("a copy-update keeps the field");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Tag::NewStruct => {
                    let s = self.strip(sub(b.ty[i]));
                    let Some(fs) = self.fields_of(s) else {
                        continue;
                    };
                    for (k, &v) in b.record(b.data[i][1]).iter().enumerate() {
                        if fs.get(k).is_some_and(|&ft| self.coll(ft)) {
                            let f = (s, u32::try_from(k).unwrap_or(u32::MAX));
                            self.fields.insert(f);
                            if !fresh(v) {
                                self.bad
                                    .entry(f)
                                    .or_insert("a construction with a shared header");
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for v in shared_borrows(b) {
            let t = sub(b.ty[v]);
            self.copied.insert(self.strip(t));
            if self.coll(t) {
                let place = through(b.data[v][0]);
                match self.field(b, sub, place) {
                    Some(f) => {
                        self.bad
                            .entry(f)
                            .or_insert("a consumed borrow of the field");
                    }
                    None if self.blind(b, sub, place) => {
                        self.blind.insert(self.strip(t));
                    }
                    None => {}
                }
            }
        }
        for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
            if *m == CaptureMode::Copy {
                let t = sub(b.local_ty[l.idx()]);
                self.copied.insert(self.strip(t));
            }
        }
    }

    /// Whether a value of `t` holds an `s` in place.
    fn holds(&self, t: Ty, s: Ty, depth: u32) -> bool {
        let t = self.strip(t);
        if t == s {
            return true;
        }
        if depth > 8 {
            return true;
        }
        match self.pool.get(t) {
            TyData::Adt { def, args } => match self.env.std_kind(def) {
                StdKind::List | StdKind::Map => self
                    .pool
                    .list_items(args)
                    .iter()
                    .any(|&a| self.holds(a, s, depth + 1)),
                _ => self
                    .fields_of(t)
                    .is_some_and(|fs| fs.iter().any(|&f| self.holds(f, s, depth + 1))),
            },
            TyData::Tuple { elems, .. } => self
                .pool
                .list_items(elems)
                .iter()
                .any(|&a| self.holds(a, s, depth + 1)),
            TyData::Option(i) => self.holds(i, s, depth + 1),
            TyData::Union(ms) => self
                .pool
                .list_items(ms)
                .iter()
                .any(|&a| self.holds(a, s, depth + 1)),
            _ => false,
        }
    }
}

/// Whether statement `i` is a binding: a local's one whole store. Of a borrow,
/// it holds no count; its own reads answer for it.
fn binds(b: &Body, stores: &[u32], i: usize) -> bool {
    let p = b.view(b.data[i][0]);
    b.tags[i] == Tag::Store && p.whole() && p.local().is_some_and(|l| stores[l as usize] == 1)
}

/// How many whole stores each local has.
fn whole_stores(b: &Body) -> Vec<u32> {
    let mut stores = vec![0; b.local_ty.len()];
    for i in 0..b.len() {
        let p = b.view(b.data[i][0]);
        if b.tags[i] == Tag::Store
            && p.whole()
            && let Some(l) = p.local()
        {
            stores[l as usize] += 1;
        }
    }
    stores
}

/// The borrowed reads that a use consumes, or pass as an argument the callee
/// may own. Each takes a count in the backend's glue (`stored`, `conservative:
/// Intrinsic`, `Call`, `CopyStruct`), so it is a copy too. A binding's is not.
fn shared_borrows(b: &Body) -> Vec<usize> {
    let stores = whole_stores(b);
    let mut ops = Vec::new();
    let mut out = Vec::new();
    for i in 0..b.len() {
        if binds(b, &stores, i) {
            continue;
        }
        hd_tir::own::operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if v < b.len()
                && b.tags[v] == Tag::Read
                && b.data[v][1] == ReadMode::Borrow as u32
                && (b.tags[i] == Tag::Retain
                    || matches!(op.role, Role::Consume | Role::Arg(_) | Role::Tail))
            {
                out.push(v);
            }
        }
    }
    out
}

/// Whether `IsUnique` word `w` runs at or before its change.
const fn check_runs(w: u32) -> bool {
    matches!(w, 0 | hd_tir::ir::unique_word::HOISTED)
}

/// The single-owner verdict of every collection field a check, store or
/// construction names, over `bodies` (each with its item and arguments).
pub(crate) fn single_owner<'b>(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    bodies: impl Iterator<Item = (InstId, &'b Body, hd_base::DefId, TyList)>,
) -> Verdicts {
    let mut w = Walk::new(pool, env);
    for (_, b, item, args) in bodies {
        let sub = |t: Ty| hd_mono::subst(pool, env, item, args, t);
        w.body(b, &sub);
    }
    let mut out = Verdicts::default();
    for &(s, k) in &w.fields {
        let ft = w.fields_of(s).and_then(|fs| fs.get(k as usize).copied());
        let why = if let Some(r) = w.bad.get(&(s, k)) {
            Some(*r)
        } else if w.copied.iter().any(|&t| w.holds(t, s, 0)) {
            Some("a copy of a value that holds the struct")
        } else if ft.is_some_and(|ft| w.blind.contains(&w.strip(ft))) {
            Some("a copy or store through a reference of the field's type")
        } else {
            None
        };
        out.fields.insert((s, k), why);
    }
    out.blind = w.blind;
    out
}

/// The uniqueness checks of `b` that stand at their change (data word 0)
/// and are of a field with one owner: the checks step 15d2 marks
/// `unique_word::OWNED`.
pub(crate) fn owned_checks(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    verdicts: &Verdicts,
) -> Vec<usize> {
    let w = Walk::new(pool, env);
    (0..b.len())
        .filter(|&i| b.tags[i] == Tag::IsUnique && b.data[i][1] == 0)
        .filter(|&i| {
            w.field(b, sub, b.data[i][0])
                .is_some_and(|f| verdicts.one_owner(f))
        })
        .collect()
}

/// Per local of `b`: a collection whose buffer has one owner at each of its
/// checks. Every whole store is a fresh collection, no closure captures it,
/// rule 3 shares no value of its type, and no check may run after a statement
/// that shares it: a copy, or a borrow that a use consumes. A check is an
/// `IsUnique` that runs (word 0, or `HOISTED`) of all of it, or a call taking
/// `&mut` of all of it.
pub(crate) fn owned_locals(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    verdicts: &Verdicts,
) -> Vec<bool> {
    let w = Walk::new(pool, env);
    let n = b.len();
    let mut good: Vec<bool> = (0..b.local_ty.len())
        .map(|l| {
            let t = sub(b.local_ty[l]);
            w.coll(t)
                && !verdicts.blind.contains(&w.strip(t))
                && b.local_flags[l] & local_flags::PARAM == 0
                && !b.cap_local.iter().any(|c| c.idx() == l)
        })
        .collect();
    if !good.iter().any(|&g| g) {
        return good;
    }
    // The local all of which a place names, past reads of a read's value.
    let named = |word: u32| {
        let p = b.view(Walk::resolve(b, word));
        if p.whole() {
            p.local().map(|l| l as usize)
        } else {
            None
        }
    };
    // What shares a binding of a borrow shares the local it borrows.
    let stores = whole_stores(b);
    let mut lent: Vec<usize> = (0..b.local_ty.len()).collect();
    for i in 0..n {
        if binds(b, &stores, i)
            && let Some(v) = Ref(b.data[i][1]).as_inst().map(|v| v.0 as usize)
            && b.tags[v] == Tag::Read
            && b.data[v][1] == ReadMode::Borrow as u32
            && let (Some(m), Some(l)) = (named(b.data[i][0]), named(b.data[v][0]))
        {
            lent[m] = lent[l];
        }
    }
    let whole = |word: u32| named(word).map(|l| lent[l]);
    let mut shares: Vec<(usize, usize)> = shared_borrows(b)
        .into_iter()
        .filter_map(|v| Some((whole(b.data[v][0])?, v)))
        .collect();
    let mut checks = Vec::new();
    for i in 0..n {
        match b.tags[i] {
            Tag::Store => {
                let p = b.view(b.data[i][0]);
                if p.whole()
                    && let Some(l) = p.local()
                    && !Walk::fresh_new(b, b.data[i][1])
                {
                    good[l as usize] = false;
                }
            }
            Tag::Read if b.data[i][1] == ReadMode::Copy as u32 => {
                if let Some(l) = whole(b.data[i][0]) {
                    shares.push((l, i));
                }
            }
            Tag::IsUnique if check_runs(b.data[i][1]) => {
                if let Some(l) = whole(b.data[i][0]) {
                    checks.push((l, i));
                }
            }
            Tag::Call => {
                let r = b.record(b.data[i][1]);
                for &a in &r[..r.len().saturating_sub(3)] {
                    let Some(v) = Ref(a).as_inst().map(|v| v.0 as usize) else {
                        continue;
                    };
                    if b.tags[v] == Tag::Ref
                        && b.data[v][1] & ref_word::MUT != 0
                        && let Some(l) = whole(b.data[v][0])
                    {
                        checks.push((l, i));
                    }
                }
            }
            _ => {}
        }
    }
    let topo = hd_tir::ir::Topology::new(b);
    for &(l, s) in &shares {
        if good[l]
            && checks
                .iter()
                .any(|&(m, c)| m == l && hd_tir::own::values::may_follow(b, &topo, s, c))
        {
            good[l] = false;
        }
    }
    good
}

/// The uniqueness checks of all of a one-owner local of `b`
/// ([`owned_locals`]): those the link marks `OWNED`.
pub(crate) fn owned_local_checks(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    verdicts: &Verdicts,
) -> Vec<usize> {
    if !(0..b.len()).any(|i| b.tags[i] == Tag::IsUnique) {
        return Vec::new();
    }
    let good = owned_locals(pool, env, b, sub, verdicts);
    (0..b.len())
        .filter(|&i| b.tags[i] == Tag::IsUnique && check_runs(b.data[i][1]))
        .filter(|&i| {
            let p = b.view(b.data[i][0]);
            p.whole() && p.local().is_some_and(|l| good[l as usize])
        })
        .collect()
}

/// Step 15d3: the uniqueness checks of all of a `&mut` parameter's target
/// (`[*$k]`) in the instances of a function that every call passes a
/// `&mut` of a one-owner field at that parameter, by instance: those the
/// link marks `OWNED`. The function must be a free one (no impl or data
/// member, which a trait value may call) that no function value names, so
/// its direct calls are all its calls.
pub(crate) fn entry_owned_checks(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    all: &[Instance<'_>],
    verdicts: &Verdicts,
) -> HashMap<InstId, Vec<usize>> {
    let w = Walk::new(pool, env);
    // Per callee: whether each parameter got a one-owner field's `&mut`
    // at every call so far; `None` once anything else names it.
    let mut params: HashMap<hd_base::DefId, Option<Vec<bool>>> = HashMap::default();
    for &(_, b, item, args) in all {
        let sub = |t: Ty| hd_mono::subst(pool, env, item, args, t);
        let locals =
            if (0..b.len()).any(|i| b.tags[i] == Tag::Ref && b.data[i][1] & ref_word::MUT != 0) {
                owned_locals(pool, env, b, &sub, verdicts)
            } else {
                Vec::new()
            };
        for i in 0..b.len() {
            match b.tags[i] {
                Tag::Call => {
                    let Some(Callee::Item { def, .. }) = Callee::from_words(b.record(b.data[i][0]))
                    else {
                        continue;
                    };
                    let r = b.record(b.data[i][1]);
                    let argv = &r[..r.len().saturating_sub(3)];
                    let good: Vec<bool> = argv
                        .iter()
                        .map(|&a| {
                            Ref(a).as_inst().is_some_and(|v| {
                                let v = v.0 as usize;
                                let p = b.view(b.data[v][0]);
                                b.tags[v] == Tag::Ref
                                    && b.data[v][1] & ref_word::MUT != 0
                                    && (w
                                        .field(b, &sub, b.data[v][0])
                                        .is_some_and(|f| verdicts.one_owner(f))
                                        || (p.whole()
                                            && p.local().is_some_and(|l| {
                                                locals.get(l as usize).copied().unwrap_or(false)
                                            })))
                            })
                        })
                        .collect();
                    let e = params.entry(def).or_insert_with(|| Some(good.clone()));
                    if let Some(p) = e {
                        if p.len() == good.len() {
                            for (x, g) in p.iter_mut().zip(&good) {
                                *x &= *g;
                            }
                        } else {
                            *e = None;
                        }
                    }
                }
                // A function value or a default call names it: calls the
                // walk can't see may follow.
                Tag::ItemRef | Tag::DefaultCall => {
                    let r = b.record(b.data[i][usize::from(b.tags[i] == Tag::ItemRef)]);
                    for &x in r {
                        params.insert(hd_base::DefId::from_raw(x), None);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = HashMap::default();
    for &(id, b, item, _) in all {
        let Some(Some(p)) = params.get(&item) else {
            continue;
        };
        if env.parent(item).is_some() || !p.iter().any(|&g| g) {
            continue;
        }
        // A parameter the body stores over or takes a reference to may
        // stop naming the field's storage.
        let mut kept = p.clone();
        for i in 0..b.len() {
            if matches!(b.tags[i], Tag::Store | Tag::Ref) {
                let v = b.view(b.data[i][0]);
                if let (Some(l), true) = (v.local(), v.whole())
                    && let Some(k) = kept.get_mut(l as usize)
                {
                    *k = false;
                }
            }
        }
        let p = &kept;
        let marks: Vec<usize> = (0..b.len())
            .filter(|&i| b.tags[i] == Tag::IsUnique && check_runs(b.data[i][1]))
            .filter(|&i| {
                let v = b.view(b.data[i][0]);
                v.local().is_some_and(|l| {
                    p.get(l as usize).copied().unwrap_or(false)
                        && b.local_flags[l as usize] & local_flags::PARAM != 0
                        && matches!(v.steps(b).collect::<Vec<_>>()[..], [Proj::Deref])
                })
            })
            .collect();
        if !marks.is_empty() {
            out.insert(id, marks);
        }
    }
    out
}

/// The own view's line for each uniqueness check of a field in `b`:
/// `one owner`, or `shared: why`.
pub(crate) fn check_lines(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    verdicts: &Verdicts,
) -> Vec<(usize, String)> {
    let w = Walk::new(pool, env);
    (0..b.len())
        .filter(|&i| b.tags[i] == Tag::IsUnique)
        .filter_map(|i| {
            let v = verdicts.fields.get(&w.field(b, sub, b.data[i][0])?)?;
            let line = match v {
                None if b.data[i][1] == 0 => "owned: single-owner field".to_owned(),
                None => "one owner".to_owned(),
                Some(why) => format!("shared: {why}"),
            };
            Some((i, line))
        })
        .collect()
}
