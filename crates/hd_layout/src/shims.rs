//! Per-type count shims (step 4b of `docs/future-work/BACKEND_NEUTRAL_TIR.md`):
//! the retain or the release of a value of one type, as a TIR body that a
//! backend lowers as it lowers any body. rustc's `drop_glue::<T>`
//! (`-Zdump-mir`) is the model: one body per type, a `switchInt` on the
//! discriminant, one `drop` per counted field, and a field of another
//! aggregate calling that type's glue. Swift's outlined copy and consume
//! (`$s..WOy`, `WOe`) flatten a loadable payload's references the same way
//! this flattens a product kept in values.
//!
//! The field rule: a part that is one counted address is a `Retain` or
//! `Release` statement; a product kept in values (at most [`INDIRECT`]
//! scalars) is counted in place, field by field; a sum or a value kept in
//! memory is a statement on the whole part, which the backend lowers as a
//! call of that type's own shim. So a shim is O(its own fields): a nested
//! enum is called, never copied into its owner.
//!
//! [`INDIRECT`]: crate::INDIRECT

use hd_base::{DefId, Hash128, HashMap, LocalId, NodeIdx, StableHasher, SubId, Symbol};
use hd_tir::ir::count::{At, Info, Part};
use hd_tir::ir::{
    Body, BodyKind, Callee, Phase, Place, PlaceRoot, Proj, Providers, ReadMode, Ref, Storage, Tag,
    TirBuilder, TirSink, VerifyError, local_flags, ref_word,
};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

use crate::{
    Kind, ProgramEnv, StdKind, by_address, fat, kind, strip_newtype, tuple_members, value_parts,
    variants,
};
use hd_base::{Stage, StageResult};

/// Which count a shim states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shim {
    Retain,
    Release,
}

/// Which of a value's counted parts a shim counts. A function value's
/// input whose `Drop` ownership the callee takes (`OwnCx::function_input`,
/// `transfer`) splits them: the thin counted references, lent (`Thin`),
/// and every other part, hooks included, transferred (`Rest`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Portion {
    #[default]
    All,
    Thin,
    Rest,
}

impl Portion {
    /// The portion a count statement of a shim body states in its reason
    /// word (`Info::reason`).
    #[must_use]
    pub fn from_reason(w: u32) -> Self {
        match w {
            1 => Portion::Thin,
            2 => Portion::Rest,
            _ => Portion::All,
        }
    }
}

/// The portions of a function value's input `t`: the one its caller keeps
/// counting (lent) and the one the callee takes, `None` for nothing.
#[must_use]
pub fn input_portions(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
) -> (Option<Portion>, Option<Portion>) {
    if !env.function_input(t).transfer {
        (Some(Portion::All), None)
    } else if !crate::holds_thin_ref(pool, env, strip_newtype(pool, env, t)) {
        (None, Some(Portion::All))
    } else {
        (Some(Portion::Thin), Some(Portion::Rest))
    }
}

/// What counting a value of a type takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// Nothing: no counted part.
    None,
    /// One counted address, value `index` of the value's, `known` when it
    /// is a payload start: a statement, no shim. A null one counts nothing.
    Address { index: u32, known: bool },
    /// A product kept in values: its parts, in place.
    Parts,
    /// A sum, or a value kept in memory: its own shim.
    Call,
}

/// Per run and [`Portion`], each type's [`Count`] and shape, so
/// classifying a type reads each of its parts' answers once: O(own parts)
/// per type.
#[derive(Default)]
pub struct Counts {
    portion: Portion,
    of: HashMap<Ty, Count>,
    shapes: HashMap<Ty, Hash128>,
}

impl Counts {
    /// Counts of `portion` of each type's counted parts.
    #[must_use]
    pub fn new(portion: Portion) -> Self {
        Counts {
            portion,
            ..Counts::default()
        }
    }

    /// One address leaf as this portion sees it: `thin` when a thin
    /// counted reference.
    fn leaf(&self, c: Count, thin: bool) -> Count {
        match (self.portion, thin) {
            (Portion::All, _) | (Portion::Thin, true) | (Portion::Rest, false) => c,
            _ => Count::None,
        }
    }

    /// What counting a value of `t` takes. A local reference counts
    /// nothing (`ref.count.local`).
    ///
    /// # Errors
    /// A type whose layout cannot be made.
    pub fn of(&mut self, pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Count> {
        if let Some(&c) = self.of.get(&t) {
            return Ok(c);
        }
        let s = strip_newtype(pool, env, t);
        let at = |index, known| self.leaf(Count::Address { index, known }, false);
        let c = match pool.get(s) {
            TyData::Prim(Prim::String) | TyData::TraitValue { .. } => at(0, false),
            // A function value's environment, after its code.
            TyData::Fn { .. } => at(1, true),
            TyData::Context(_) => at(0, true),
            TyData::Canon(_) if hd_mono::is_class_ref(pool, s) => at(0, false),
            TyData::Tuple { .. } => self.product(pool, env, s, &tuple_members(pool, s))?,
            TyData::Union(_) => self.sum(pool, env, s)?,
            TyData::Ref { inner, counted, .. } => {
                let fat = fat(pool, env, inner);
                self.leaf(reference(fat, counted), !fat && counted)
            }
            TyData::Adt { def, .. } if env.std_kind(def) != StdKind::Other => at(0, true),
            TyData::Adt { def, args } => match kind(pool, env, s) {
                Kind::Boxed => at(0, true),
                Kind::Ref { fat, whole, .. } => self.leaf(reference(fat, whole), !fat && whole),
                Kind::Value if env.enum_variants(def, args).is_some() => self.sum(pool, env, s)?,
                Kind::Value => self.product(pool, env, s, &value_parts(pool, env, s))?,
            },
            _ => Count::None,
        };
        // A value that holds a `Drop` hook has its own release shim, which
        // calls the hook before its fields' counts (`module.rt.drop.glue`).
        // A newtype's layers are values whatever its base is.
        let c = if self.portion != Portion::Thin
            && (s != t || value_kind(pool, env, s))
            && crate::holds_drop(pool, env, t)
        {
            Count::Call
        } else {
            c
        };
        self.of.insert(t, c);
        Ok(c)
    }

    /// `t`'s shape: its values, its [`Count`], and each counted part's
    /// position and shape (a sum's, with its discriminant). Types of one
    /// shape have one shim body, so they share one shim, as equal walks
    /// shared one helper. O(own parts) per type, each once a run.
    ///
    /// # Errors
    /// A type whose layout cannot be made.
    pub fn shape(
        &mut self,
        pool: &InternPool,
        env: &dyn ProgramEnv,
        t: Ty,
    ) -> StageResult<Hash128> {
        if let Some(&h) = self.shapes.get(&t) {
            return Ok(h);
        }
        let std_hash = |v: &dyn Fn(&mut std::collections::hash_map::DefaultHasher)| {
            use std::hash::Hasher as _;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            v(&mut h);
            h.finish()
        };
        let vals = crate::vals(pool, env, t)?;
        let mut h = StableHasher::new("count-shape");
        h.u32(self.portion as u32);
        // A type with a hook has its own: its hook is its type's.
        if crate::holds_drop(pool, env, t) {
            h.u32(t.0);
        }
        h.u64(std_hash(&|s| std::hash::Hash::hash(&vals, s)));
        match self.of(pool, env, t)? {
            Count::None => h.u32(0),
            Count::Address { index, known } => {
                h.u32(1);
                h.u32(index);
                h.u32(u32::from(known));
            }
            c @ (Count::Parts | Count::Call) => {
                h.u32(if c == Count::Parts { 2 } else { 3 });
                let s = strip_newtype(pool, env, t);
                if let Some(vs) = variants(pool, env, s) {
                    let flat = crate::flat(pool, env, s)?;
                    let disc = flat.as_ref().map(|f| f.disc);
                    h.u64(std_hash(&|x| std::hash::Hash::hash(&disc, x)));
                    for (v, fields) in (0u32..).zip(vs.iter()) {
                        for (k, &p) in (0u32..).zip(fields) {
                            if self.of(pool, env, p)? == Count::None {
                                continue;
                            }
                            let at = flat.as_ref().and_then(|f| {
                                f.positions.get(v as usize)?.get(k as usize).cloned()
                            });
                            h.u32(v);
                            h.u32(k);
                            h.u64(std_hash(&|x| std::hash::Hash::hash(&at, x)));
                            h.hash(self.shape(pool, env, p)?);
                        }
                    }
                } else {
                    let parts = match pool.get(s) {
                        TyData::Tuple { .. } => tuple_members(pool, s),
                        _ => value_parts(pool, env, s),
                    };
                    let (_, at) = crate::payload_mem(pool, env, &parts)?;
                    for (k, &p) in parts.iter().enumerate() {
                        if self.of(pool, env, p)? == Count::None {
                            continue;
                        }
                        h.u64(u64::try_from(at[k].first).unwrap_or(u64::MAX));
                        h.u32(at[k].offset);
                        h.hash(self.shape(pool, env, p)?);
                    }
                }
            }
        }
        let shape = h.finish();
        self.shapes.insert(t, shape);
        Ok(shape)
    }

    fn product(
        &mut self,
        pool: &InternPool,
        env: &dyn ProgramEnv,
        t: Ty,
        parts: &[Ty],
    ) -> StageResult<Count> {
        let mut counted = Vec::new();
        for (k, &p) in parts.iter().enumerate() {
            let c = self.of(pool, env, p)?;
            if c != Count::None {
                counted.push((k, c));
            }
        }
        if counted.is_empty() {
            return Ok(Count::None);
        }
        // One counted address is that address, at its field's first value,
        // as the walk's single leaf was, wherever the value is kept.
        if let &[(k, Count::Address { index, known })] = counted.as_slice() {
            let (_, at) = crate::payload_mem(pool, env, parts)?;
            let first = u32::try_from(at[k].first).unwrap_or(u32::MAX);
            return Ok(Count::Address {
                index: first + index,
                known,
            });
        }
        Ok(if by_address(pool, env, t)? {
            Count::Call
        } else {
            Count::Parts
        })
    }

    /// A sum with a counted variant has its own shim, but for an optional
    /// address whose empty variant is that address null (`*T?`,
    /// `string?`), which the address's count tests: it is that address.
    fn sum(&mut self, pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Count> {
        let vs = variants(pool, env, t).unwrap_or_default();
        let mut counted = Vec::new();
        for (v, fields) in (0u32..).zip(vs.iter()) {
            for &p in fields {
                let c = self.of(pool, env, p)?;
                if c != Count::None {
                    counted.push((v, p, c));
                }
            }
        }
        Ok(match counted.as_slice() {
            [] => Count::None,
            &[(v, p, Count::Address { index, known })] => {
                match one_variant(pool, env, t, v, p, index)? {
                    Some(index) => Count::Address { index, known },
                    None if vs.len() == 2 && null_niche(pool, env, t, v, p, index)? => {
                        Count::Address { index, known }
                    }
                    None => Count::Call,
                }
            }
            _ => Count::Call,
        })
    }
}

/// When `t` has one variant, `v`, whose values say it (`Disc::One`, as
/// `Result[T, never]`): the value of `t` that its field `p`'s value
/// `index` is.
fn one_variant(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    v: u32,
    p: Ty,
    index: u32,
) -> StageResult<Option<u32>> {
    let Some(flat) = crate::flat(pool, env, t)? else {
        return Ok(None);
    };
    if flat.disc != crate::Disc::One(v) {
        return Ok(None);
    }
    let field =
        variants(pool, env, t).and_then(|vs| vs.get(v as usize)?.iter().position(|&f| f == p));
    Ok(field.and_then(|f| {
        flat.positions
            .get(v as usize)?
            .get(f)?
            .get(index as usize)
            .copied()
    }))
}

/// Whether two-variant `t`'s variant `v`, of the one field `p`, holds `p`'s
/// values as `t`'s own, and the other variant is `p`'s value `index` null:
/// a niche at that value, untagged for `v`, writing 0 for the other.
fn null_niche(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    v: u32,
    p: Ty,
    index: u32,
) -> StageResult<bool> {
    let Some(flat) = crate::flat(pool, env, t)? else {
        return Ok(false);
    };
    let crate::Disc::Cell(d) = flat.disc else {
        return Ok(false);
    };
    let width = crate::vals(pool, env, p)?.len();
    let same = flat.positions.get(v as usize).is_some_and(|fields| {
        fields.len() == 1 && fields[0].iter().map(|&k| k as usize).eq(0..width)
    }) && crate::vals(pool, env, t)?.len() == width;
    Ok(same
        && d.value == index
        && matches!(d.enc, crate::Enc::Niche { untagged, .. } if untagged == v)
        && d.enc.is(1 - v) == crate::Is::Eq(0))
}

/// Whether `t`, stripped, is kept by value: a tuple, a union, or a struct
/// or enum that is not boxed.
fn value_kind(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Tuple { .. } | TyData::Union(_) => true,
        TyData::Adt { def, .. } => {
            env.std_kind(def) == StdKind::Other && matches!(kind(pool, env, t), Kind::Value)
        }
        _ => false,
    }
}

/// A counted reference (`whole`) counts its target, a known payload start
/// when thin; a local one counts nothing (`ref.count.local`).
fn reference(fat: bool, whole: bool) -> Count {
    match (fat, whole) {
        (false, true) => Count::Address {
            index: 0,
            known: true,
        },
        (true, true) => Count::Address {
            index: 0,
            known: false,
        },
        (_, false) => Count::None,
    }
}

/// The `kind` shim of `t`: a function of one lent parameter of type `t`
/// whose body states each count. `None` when `t` needs none: it counts
/// nothing, or one address, which the caller states itself. `item`, the
/// shim's lang item, names the body. O(own fields of `t`).
///
/// # Errors
/// A type whose layout cannot be made.
pub fn shim(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    counts: &mut Counts,
    hooks: &Hooks<'_>,
    item: DefId,
    t: Ty,
    kind: Shim,
) -> StageResult<Option<Body>> {
    if !matches!(counts.of(pool, env, t)?, Count::Parts | Count::Call) {
        return Ok(None);
    }
    let n = NodeIdx::from_raw(0);
    let mut b = TirBuilder::new(item, BodyKind::Fn);
    let x = b.local(t, Symbol::from_raw(0), local_flags::PARAM, n);
    let root = b.open_block();
    // The hooks take the parameter's address, in heap form: it moves into
    // a slot of the frame, a value kept in memory copied from where it is.
    if kind == Shim::Release
        && counts.portion != Portion::Thin
        && hook_calls(pool, env, hooks, &mut b, x, t)
    {
        b.body_mut()
            .storage
            .push((SubId::from_raw(0), x, Storage::Slot));
    }
    let v = b.get(x, t, n);
    let mut w = Writer {
        pool,
        env,
        counts,
        b,
        kind,
    };
    w.borrow(v);
    w.whole(t, v)?;
    let root = w.b.close_block(root, None, Ty::VOID, n);
    w.b.body_mut().phase = Phase::Lowered;
    let body = (w.b.finish(root, &[]))
        .map_err(|e| VerifyError::bug(Stage::Collect, "count shim TIR verifier", &e))?;
    Ok(Some(body))
}

/// The `std.rt.Drop.drop` instance of a type that implements it: the impl
/// method and its type arguments (`Collected::drop_glue`).
pub type Hooks<'a> = dyn Fn(Ty) -> Option<(DefId, TyList)> + 'a;

/// Calls the hooks of `t` on local `x`, each newtype layer's outermost
/// first, then its base's (`module.rt.drop.glue`, as `newtype_layers`
/// walks them), each on `&mut` of its layer: `x` behind as many `.0`
/// projections, in its frame slot. Whether it called any.
fn hook_calls(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    hooks: &Hooks<'_>,
    b: &mut TirBuilder,
    x: LocalId,
    t: Ty,
) -> bool {
    let n = NodeIdx::from_raw(0);
    let mut layers = Vec::new();
    crate::newtype_layers(pool, env, t, &mut |layer| layers.push(layer));
    layers.push(strip_newtype(pool, env, t));
    let mut called = false;
    for (depth, layer) in layers.into_iter().enumerate() {
        let Some((def, targs)) = hooks(layer) else {
            continue;
        };
        if !called {
            // The parameter moves into its frame slot at entry: the
            // prologue does it for a parameter its root's `Box` names.
            b.emit(Tag::Box, x.raw(), Storage::Slot as u32, Ty::VOID, n);
        }
        let place = Place {
            root: PlaceRoot::Local(x),
            proj: vec![Proj::Field(0); depth],
        };
        let word = b.place(&place);
        let to = pool.intern_ty(&TyData::Ref {
            inner: layer,
            mutable: true,
            counted: false,
        });
        let r = b.emit(
            Tag::Ref,
            word,
            ref_word::MUT | ref_word::LOCAL | ref_word::FRAME,
            to,
            n,
        );
        b.call(
            &Callee::Item { def, targs },
            &[r],
            Providers::None,
            Ty::VOID,
            n,
        );
        called = true;
    }
    called
}

struct Writer<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    counts: &'a mut Counts,
    b: TirBuilder,
    kind: Shim,
}

impl Writer<'_> {
    /// The shim's own type: a sum switches on its variants; a newtype
    /// counts its one field; any other type, its parts in place.
    fn whole(&mut self, t: Ty, v: Ref) -> StageResult<()> {
        let s = strip_newtype(self.pool, self.env, t);
        if s != t {
            // A newtype has one field (`strip_newtype`).
            if let &[inner] = value_parts(self.pool, self.env, t).as_slice() {
                let f = self.field(v, 0, inner);
                return self.whole(inner, f);
            }
        }
        // A base that is one address (a newtype's string), or that has
        // its own shim, is a statement; this shim's own type, or a
        // product, opens.
        let own = self.counts.of(self.pool, self.env, t)?;
        if matches!(own, Count::Address { .. }) {
            return self.part(t, v);
        }
        match variants(self.pool, self.env, t) {
            Some(vs) => self.switch(v, &vs),
            None => self.parts(t, v),
        }
    }

    /// A product's counted parts, in place.
    fn parts(&mut self, t: Ty, v: Ref) -> StageResult<()> {
        let parts = match self.pool.get(t) {
            TyData::Tuple { .. } => tuple_members(self.pool, t),
            _ => value_parts(self.pool, self.env, t),
        };
        for (k, &p) in (0u32..).zip(&parts) {
            if self.counts.of(self.pool, self.env, p)? != Count::None {
                let f = self.field(v, k, p);
                self.part(p, f)?;
            }
        }
        Ok(())
    }

    /// One part: a statement, or its own parts when a product in values.
    fn part(&mut self, t: Ty, v: Ref) -> StageResult<()> {
        match self.counts.of(self.pool, self.env, t)? {
            Count::None => {}
            Count::Parts => self.parts(strip_newtype(self.pool, self.env, t), v)?,
            Count::Address { .. } | Count::Call => {
                let n = NodeIdx::from_raw(0);
                // The part's own shim counts the same portion.
                let portion = self.counts.portion as u32;
                let info = Info::new(At::Value, Part::Result, portion).write(self.b.body_mut());
                let tag = match self.kind {
                    Shim::Retain => Tag::Retain,
                    Shim::Release => Tag::Release,
                };
                self.b.emit(tag, v.0, info, Ty::VOID, n);
            }
        }
        Ok(())
    }

    /// One test per variant with a counted payload, each the next one's
    /// `else`: as many tests as counted variants, nested once each.
    fn switch(&mut self, v: Ref, variants: &[Vec<Ty>]) -> StageResult<()> {
        let n = NodeIdx::from_raw(0);
        let mut open = Vec::new();
        for (k, fields) in (0u32..).zip(variants) {
            let mut counted = false;
            for &f in fields {
                counted |= self.counts.of(self.pool, self.env, f)? != Count::None;
            }
            if !counted {
                continue;
            }
            let arm = self.b.open_block();
            for (j, &f) in (0u32..).zip(fields) {
                if self.counts.of(self.pool, self.env, f)? != Count::None {
                    let p = self.b.payload(v, k, j, f, n);
                    self.borrow(p);
                    self.part(f, p)?;
                }
            }
            let arm = self.b.close_block(arm, None, Ty::VOID, n);
            open.push((k, arm, self.b.open_block()));
        }
        while let Some((k, arm, rest)) = open.pop() {
            let rest = self.b.close_block(rest, None, Ty::VOID, n);
            let cases = self.b.refs_record(&[Ref(k), arm, rest]);
            self.b.emit(Tag::SwitchTag, v.0, cases, Ty::VOID, n);
        }
        Ok(())
    }

    fn field(&mut self, v: Ref, k: u32, t: Ty) -> Ref {
        let f = self.b.field(v, k, t, NodeIdx::from_raw(0));
        self.borrow(f);
        f
    }

    /// The read takes no count: the shim's statements are its counts.
    fn borrow(&mut self, r: Ref) {
        if let Some(i) = r.as_inst() {
            self.b.body_mut().data[i.0 as usize][1] = ReadMode::Borrow as u32;
        }
    }
}
