//! Single-owner fields (#1399 step 15d1), facts only: per struct instance
//! `S` and collection field `f`, over every body the program reaches in a
//! release build, whether the buffer `S.f` holds ever has a second owner.
//!
//! `S.f` has one owner when:
//! 1. no statement copies with a count a value of `S`, of a type that holds
//!    an `S` in place (a tuple, struct, option or collection of it, not
//!    behind a reference), or `S.f` itself: a `copy` read, or a closure's
//!    `Copy` capture (moves and borrows are fine);
//! 2. every construction of an `S` and every store of all of `S.f` puts a
//!    fresh buffer there: a new collection that is empty or has an element
//!    that is no constant (a literal of constants is static data the
//!    backend shares), or the move of a local whose every store is one;
//! 3. no body copies or stores, through a reference it can't place, a
//!    header of `f`'s type that is not fresh (`*r = xs` with `r: &mut
//!    List[T]`, or `xs := *r`): such a place may be any field of that type.
//!
//! A pointer alias (`*mut S`) shares the storage, not the buffer, so it
//! adds no owner. An erased body, whose types are open, makes no field
//! single-owner. The fact is computed after collection, so the passes do
//! not read it yet (step 15d2).

use hd_base::{HashMap, HashSet, InstId};
use hd_mono::ProgramEnv;
use hd_mono::layout::StdKind;
use hd_tir::ir::{Body, CaptureMode, Proj, ReadMode, Ref, Role, Root, Tag};
use hd_types::{InternPool, Ty, TyData, TyList};

/// Why a field has a second owner, or none for one owner.
pub(crate) type Verdicts = HashMap<(Ty, u32), Option<&'static str>>;

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
    open: bool,
}

impl Walk<'_> {
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

    /// Whether value `v` is a fresh buffer: a new collection that is empty
    /// or has an element that is no constant.
    fn fresh_new(b: &Body, v: u32) -> bool {
        let Some(i) = Ref(v).as_inst().map(|r| r.0 as usize) else {
            return false;
        };
        matches!(b.tags[i], Tag::NewList | Tag::NewMap) && {
            let items = b.record(b.data[i][1]);
            items.is_empty() || items.iter().any(|&x| Ref(x).as_inst().is_some())
        }
    }

    fn body(&mut self, b: &Body, sub: &dyn Fn(Ty) -> Ty, erased: bool) {
        let n = b.len();
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
                    if erased && matches!(self.pool.get(self.strip(t)), TyData::Param(..)) {
                        self.open = true;
                    }
                    self.copied.insert(self.strip(t));
                    if self.coll(t) {
                        match self.field(b, sub, b.data[i][0]) {
                            Some(f) => {
                                self.bad.entry(f).or_insert("a copy of the field");
                            }
                            // A place the walk can't type, or one behind a
                            // reference, may be any field of this type.
                            None if self.blind(b, sub, b.data[i][0]) => {
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
        // A borrowed read that a use consumes, or an argument the callee
        // may own, takes a count in the backend's glue (`stored`,
        // `conservative: Intrinsic`, `Call`, `CopyStruct`): a copy too.
        let mut ops = Vec::new();
        for i in 0..n {
            hd_tir::own::operands(b, i, &mut ops);
            for op in &ops {
                let v = op.value as usize;
                if v < n
                    && b.tags[v] == Tag::Read
                    && b.data[v][1] == ReadMode::Borrow as u32
                    && matches!(op.role, Role::Consume | Role::Arg(_) | Role::Tail)
                {
                    self.copied.insert(self.strip(sub(b.ty[v])));
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

/// The single-owner verdict of every collection field a check, store or
/// construction names, over `bodies` (each with its item and arguments).
pub(crate) fn single_owner<'b>(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    bodies: impl Iterator<Item = (InstId, &'b Body, hd_base::DefId, TyList, bool)>,
) -> Verdicts {
    let mut w = Walk {
        pool,
        env,
        fields: HashSet::default(),
        bad: HashMap::default(),
        copied: HashSet::default(),
        blind: HashSet::default(),
        open: false,
    };
    for (_, b, item, args, erased) in bodies {
        let sub = |t: Ty| hd_mono::subst(pool, env, item, args, t);
        w.body(b, &sub, erased);
    }
    let mut out = Verdicts::default();
    for &(s, k) in &w.fields {
        let ft = w.fields_of(s).and_then(|fs| fs.get(k as usize).copied());
        let why = if w.open {
            Some("an erased body")
        } else if let Some(r) = w.bad.get(&(s, k)) {
            Some(*r)
        } else if w.copied.iter().any(|&t| w.holds(t, s, 0)) {
            Some("a copy of a value that holds the struct")
        } else if ft.is_some_and(|ft| w.blind.contains(&w.strip(ft))) {
            Some("a copy or store through a reference of the field's type")
        } else {
            None
        };
        out.insert((s, k), why);
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
    let w = Walk {
        pool,
        env,
        fields: HashSet::default(),
        bad: HashMap::default(),
        copied: HashSet::default(),
        blind: HashSet::default(),
        open: false,
    };
    (0..b.len())
        .filter(|&i| b.tags[i] == Tag::IsUnique)
        .filter_map(|i| {
            let v = verdicts.get(&w.field(b, sub, b.data[i][0])?)?;
            Some((
                i,
                v.map_or_else(|| "one owner".to_owned(), |why| format!("shared: {why}")),
            ))
        })
        .collect()
}
