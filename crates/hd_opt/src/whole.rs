//! Whole-object references (issue #704, `crates/hd_wasm_linear/DESIGN.md`
//! §23.30): a release-only, whole-program fact per reference target. A
//! `*T` whose program never makes a counted address inside an object for
//! `T` holds null, an object's payload start, or a slot or static cell
//! with a zero count word before it, so its counts read the header at
//! `address - 8` (`retain_known`/`release_known`) and never search the
//! page table.
//!
//! The scan reads every instance's TIR before emission, at its type
//! arguments. Only a `Ref` makes a new address; every other value of a
//! reference type copies one. A `Ref`'s address is:
//!
//! - **whole** when it is a local's or a value's storage, a cell, a slot,
//!   a new object or a view, or a data object's payload, at offset 0;
//! - **the source's** when it dereferences a reference with no offset
//!   after it: the target then inherits the source target's fact;
//! - **interior** otherwise: an element (`&xs[i]`, `&map[k]`), a field at
//!   a nonzero offset, a top-level binding's static slot (no header), a
//!   frame or pointer root, or anything the scan cannot name.
//!
//! A `Ref` the borrow pass marks lent (`ref_word::LENT`) makes no counted
//! address, so a body instance's lent `Ref` is skipped (issue #909).
//!
//! An interior reference to an open target, which an erased body makes
//! for any type, turns the fact off for the whole program. A coercion
//! between reference types carries the fact as a dereference does. Dev
//! builds, REPL sessions and open-world builds never compute it. The scan
//! reads the backend's layout through [`ObjectLayout`], and the backend
//! consumes its result.

use hd_base::{DefId, HashSet, InstId};
use hd_mono::{InstanceTable, ProgramEnv, Sub, is_open, subst};
use hd_tir::ir::{Body, Place, PlaceRoot, Proj, Ref, Tag, ref_word};
use hd_types::{InternPool, Ty, TyData, TyList};

/// What a concrete type is to the backend's layout, as far as the scan
/// reads it.
pub enum Shape {
    /// A reference to a place of type `target`, once stripped: the
    /// place's address, or, when `fat`, a trait value.
    Ref { target: Ty, fat: bool },
    /// A data value: the address of an object whose payload holds these
    /// fields.
    Data(Vec<Ty>),
    /// Any other type.
    Other,
}

/// What the scan reads of the backend's type layout.
pub trait ObjectLayout {
    /// What `t` is ([`Shape`]).
    fn shape(&self, t: Ty) -> Shape;
    /// `t` with newtypes erased and `mut` dropped: what its layout reads.
    fn strip(&self, t: Ty) -> Ty;
    /// The offset of field `f` in a data object's payload of `fields`.
    fn payload_offset(&self, fields: &[Ty], f: usize) -> Option<u32>;
    /// Field `f` of struct, tuple or newtype `t`: its type and offset.
    fn field(&self, t: Ty, f: u32) -> Option<(Ty, u32)>;
    /// The variants' payload types of an enum or optional type.
    fn variants(&self, t: Ty) -> Option<Vec<Vec<Ty>>>;
}

/// The program's instances as the backend emits them: collection's table,
/// the content order, and each instance's TIR body.
pub struct Instances<'a> {
    pub table: &'a InstanceTable,
    pub order: &'a [InstId],
    pub body: &'a dyn Fn(InstId) -> Option<&'a Body>,
}

/// The reference targets some counted reference may point inside an
/// object of, or `None` when the program may point inside any.
pub fn interior_targets(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    layout: &dyn ObjectLayout,
    instances: &Instances<'_>,
) -> Option<HashSet<Ty>> {
    let table = instances.table;
    let mut interior = HashSet::default();
    let mut edges = Vec::new();
    let mut seen = HashSet::default();
    for id in instances.order {
        // An adapter forwards to one call and makes no address.
        if matches!(table.sub[id.idx()], Sub::Adapter) {
            continue;
        }
        let Some(b) = (instances.body)(*id) else {
            continue;
        };
        let (item, args) = (table.item[id.idx()], table.args[id.idx()]);
        // A body's sub-bodies and thunks share its instructions.
        if !seen.insert((std::ptr::from_ref(b) as usize, item, args)) {
            continue;
        }
        let s = Scan {
            pool,
            env,
            layout,
            item,
            args,
            b,
            lent: matches!(table.sub[id.idx()], Sub::Body(_)),
        };
        for i in 0..b.len() {
            s.inst(i, &mut interior, &mut edges)?;
        }
    }
    // A target reached from an interior one by a dereference or a
    // coercion is interior too.
    let mut changed = true;
    while changed {
        changed = false;
        for (from, to) in &edges {
            if interior.contains(from) && interior.insert(*to) {
                changed = true;
            }
        }
    }
    Some(interior)
}

/// Where a `Ref`'s address lies.
enum Start {
    Whole,
    Interior,
    /// The address of the reference to this target that it dereferences.
    Source(Ty),
}

struct Scan<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    layout: &'a dyn ObjectLayout,
    item: DefId,
    args: TyList,
    b: &'a Body,
    /// Whether a lent `Ref` here is a loan: in a body instance, where no
    /// type is open and no thunk makes the address.
    lent: bool,
}

impl Scan<'_> {
    fn sub(&self, t: Ty) -> Ty {
        subst(self.pool, self.env, self.item, self.args, t)
    }

    /// The type of value `r`: an instruction's or a constant's.
    fn value_ty(&self, r: Ref) -> Ty {
        let t = match r.as_inst() {
            Some(i) => self.b.ty.get(i.0 as usize).copied(),
            None => self
                .b
                .consts
                .get((r.0 & !Ref::CONST_BIT) as usize)
                .map(|c| c.0),
        };
        self.sub(t.unwrap_or(Ty::POISON))
    }

    /// A thin reference type's stripped target.
    fn thin_target(&self, t: Ty) -> Option<Ty> {
        match self.layout.shape(t) {
            Shape::Ref { target, fat: false } => Some(target),
            _ => None,
        }
    }

    /// Records what instruction `i` says of reference targets; `None`
    /// when it may make an interior address of any type.
    fn inst(&self, i: usize, interior: &mut HashSet<Ty>, edges: &mut Vec<(Ty, Ty)>) -> Option<()> {
        let b = self.b;
        let Some(target) = self.thin_target(self.sub(b.ty[i])) else {
            return Some(());
        };
        let open = is_open(self.pool, target);
        let start = match b.tags[i] {
            // A `Ref` into what a lent parameter refers to takes no count,
            // and its value's only release is of the new object a view's
            // projection makes (`Em::prepare_own_loans`), so no count
            // reads its address.
            Tag::Ref if self.lent && b.data[i][1] & ref_word::LENT != 0 => return Some(()),
            Tag::Ref => b
                .decode_place(b.data[i][0])
                .map_or(Start::Interior, |p| self.start(&p)),
            Tag::Coerce => {
                let from = self.value_ty(Ref(b.data[i][0]));
                match self.thin_target(from) {
                    Some(from) => Start::Source(from),
                    // `never` makes no value.
                    None if self.pool.get(from) == TyData::Never => Start::Whole,
                    None => Start::Interior,
                }
            }
            // The host gives no address of hd's heap.
            Tag::CallHost => Start::Interior,
            _ => return Some(()),
        };
        match start {
            Start::Whole => {}
            Start::Interior if open => return None,
            Start::Interior => {
                interior.insert(target);
            }
            Start::Source(from) if from == target || open => {}
            // An open source may be any type's interior address.
            Start::Source(from) if is_open(self.pool, from) => {
                interior.insert(target);
            }
            Start::Source(from) => edges.push((from, target)),
        }
        Some(())
    }

    /// Where the address of a `Ref` of place `p` lies, as `Em::counted_ref`
    /// computes it: from the last element or dereference, or from the
    /// root, then the struct fields after it.
    fn start(&self, p: &Place) -> Start {
        let root = match p.root {
            PlaceRoot::Local(l) => self.sub(self.b.local_ty[l.idx()]),
            PlaceRoot::Value(r) => self.value_ty(r),
            // A binding's static slot has no header before it.
            PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => {
                return Start::Interior;
            }
        };
        let Some(d) = p
            .proj
            .iter()
            .rposition(|s| matches!(s, Proj::Elem(_) | Proj::Deref))
        else {
            return self.at_zero(root, &p.proj, Start::Whole);
        };
        if p.proj[d] != Proj::Deref {
            return Start::Interior;
        }
        let Some(before) = self.walk_ty(root, &p.proj[..d]) else {
            return Start::Interior;
        };
        match self.layout.shape(before) {
            Shape::Ref { target, fat: false } => {
                self.at_zero(target, &p.proj[d + 1..], Start::Source(target))
            }
            Shape::Data(tys) => {
                let Some(&Proj::Field(f)) = p.proj.get(d + 1) else {
                    return Start::Interior;
                };
                match (
                    self.layout.payload_offset(&tys, f as usize),
                    tys.get(f as usize),
                ) {
                    (Some(0), Some(&ft)) => self.at_zero(ft, &p.proj[d + 2..], Start::Whole),
                    _ => Start::Interior,
                }
            }
            _ => Start::Interior,
        }
    }

    /// `then` when the struct fields `steps` lie at offset 0 of type `t`.
    fn at_zero(&self, mut t: Ty, steps: &[Proj], then: Start) -> Start {
        for s in steps {
            let Proj::Field(f) = *s else {
                return Start::Interior;
            };
            match self.layout.field(t, f) {
                Some((ft, 0)) => t = ft,
                _ => return Start::Interior,
            }
        }
        then
    }

    /// The type `steps` reach from `t`, as `Em::walk_ty` reads it.
    fn walk_ty(&self, mut t: Ty, steps: &[Proj]) -> Option<Ty> {
        let (pool, env, layout) = (self.pool, self.env, self.layout);
        let mut k = 0;
        while k < steps.len() {
            match steps[k] {
                Proj::Field(f) => t = layout.field(t, f)?.0,
                Proj::Elem(_) => {
                    let TyData::Adt { def, args } = pool.get(layout.strip(t)) else {
                        return None;
                    };
                    let items = pool.list_items(args);
                    t = match env.std_kind(def) {
                        hd_mono::layout::StdKind::Map => *items.get(1)?,
                        hd_mono::layout::StdKind::List => *items.first()?,
                        _ => return None,
                    };
                }
                Proj::Deref => match layout.shape(t) {
                    Shape::Ref { target, .. } => t = target,
                    Shape::Data(tys) => {
                        let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                            return None;
                        };
                        t = *tys.get(f as usize)?;
                        k += 1;
                    }
                    Shape::Other => return None,
                },
                Proj::Variant(v) => {
                    let Some(&Proj::Field(f)) = steps.get(k + 1) else {
                        return None;
                    };
                    t = *layout.variants(t)?.get(v as usize)?.get(f as usize)?;
                    k += 1;
                }
            }
            k += 1;
        }
        Some(t)
    }
}
