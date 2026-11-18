//! One interned [`Layout`] per type in a run (rustc's `layout_of`
//! returning an interned `LayoutData`, `rustc_abi/src/layout.rs`): its
//! size, alignment, saturating scalar count, representation, field
//! offsets and layouts, variants and niche. A type is laid out once, from
//! its parts' ids, on [`crate::by_parts`]'s explicit stack: O(its own
//! fields) per distinct type, with no native recursion and no list of a
//! value's scalars past [`INDIRECT`]. Equal layouts share one id, as
//! rustc's `mk_layout` interns `LayoutData` by value; a layout's parts are
//! ids, so hashing one is O(its own fields).

use std::cell::RefCell;
use std::rc::Rc;

use hd_base::StageResult;
use hd_mono::ProgramEnv;
use hd_types::{InternPool, Ty};

use crate::{
    Enc, INDIRECT, Leaf, Mem, Niche, by_parts, enum_mem_by, inline, mem_one, payload_mem_by,
    too_large, value_parts, variants,
};

/// A layout's index in its run's [`Layouts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayoutId(usize);

/// A type's layout (rustc's `LayoutData`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    pub size: u32,
    pub align: u32,
    /// How many scalar values the type holds, saturating at
    /// `INDIRECT + 1`: never a list.
    pub scalars: u32,
    pub repr: Repr,
    pub fields: Fields,
    pub variants: Variants,
    pub niche: Option<Niche>,
    pub uninhabited: bool,
}

/// How a value is held (rustc's `BackendRepr`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Repr {
    /// At most [`INDIRECT`] scalar values, in the order locals hold them,
    /// each at its offset ([`Mem::leaves`]).
    Scalars(Box<[Leaf]>),
    /// An address: past [`INDIRECT`] values (`Memory`).
    Memory,
}

/// Where a value's parts lie (rustc's `FieldsShape`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Fields {
    /// A scalar, an address, or a pair of them: no part of its own.
    Primitive,
    /// A struct's or a tuple's fields, or one variant's payload: each
    /// field's offset and layout (`FieldsShape::Arbitrary`).
    Record {
        offsets: Box<[u32]>,
        layouts: Box<[LayoutId]>,
    },
}

impl Fields {
    /// Each field's offset; none for a primitive.
    #[must_use]
    pub fn offsets(&self) -> &[u32] {
        match self {
            Fields::Primitive => &[],
            Fields::Record { offsets, .. } => offsets,
        }
    }
}

/// What says a value's variant (rustc's `Variants`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Variants {
    /// Not an enum.
    Single,
    /// An enum: what says its variant, and each variant's payload as a
    /// [`Fields::Record`].
    Enum { tag: Tag, payloads: Box<[Fields]> },
}

/// An enum's discriminant (rustc's `TagEncoding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    /// Only variant `k` is present: no bytes say it.
    One(u32),
    /// The scalar that holds the tag or niche, and how it says the variant.
    Cell(Leaf, Enc),
}

impl Layout {
    /// The layout as a part of an enclosing one: its leaves when it has at
    /// most [`INDIRECT`], else none, which only a `Memory` parent reads.
    pub(crate) fn part(&self) -> Mem {
        Mem {
            size: self.size,
            align: self.align,
            leaves: match &self.repr {
                Repr::Scalars(leaves) => leaves.to_vec(),
                Repr::Memory => Vec::new(),
            },
            niche: self.niche,
            uninhabited: self.uninhabited,
        }
    }

    /// The layout's leaves, each `offset` bytes further on: its part of an
    /// enclosing value's leaves. None for a `Memory` layout.
    #[must_use]
    pub fn leaves_at(&self, offset: u32) -> Vec<Leaf> {
        match &self.repr {
            Repr::Scalars(leaves) => leaves
                .iter()
                .map(|l| Leaf {
                    offset: l.offset + offset,
                    ..*l
                })
                .collect(),
            Repr::Memory => Vec::new(),
        }
    }

    /// The layout of `m`, whose values are `scalars`, saturated.
    fn new(m: Mem, scalars: usize, fields: Fields, variants: Variants) -> Layout {
        let scalars = scalars.min(INDIRECT + 1);
        Layout {
            size: m.size,
            align: m.align,
            scalars: u32::try_from(scalars).unwrap_or(u32::MAX),
            repr: if scalars <= INDIRECT {
                Repr::Scalars(m.leaves.into())
            } else {
                Repr::Memory
            },
            fields,
            variants,
            niche: m.niche,
            uninhabited: m.uninhabited,
        }
    }
}

/// One run's layouts, by stripped type, interned by value.
#[derive(Default)]
pub struct Layouts {
    by_ty: RefCell<hd_base::HashMap<Ty, StageResult<LayoutId>>>,
    data: RefCell<Vec<Rc<Layout>>>,
    ids: RefCell<hd_base::HashMap<Rc<Layout>, LayoutId>>,
}

impl Layouts {
    /// `t`'s layout: one probe once laid out, else O(the fields of each
    /// type it holds by value that is not laid out yet).
    pub fn of(&self, pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<LayoutId> {
        let u = crate::strip_newtype(pool, env, t);
        if let Some(got) = self.by_ty.borrow().get(&u) {
            return got.clone();
        }
        let mut done = self.by_ty.take();
        let got = by_parts(pool, env, u, &mut done, &|u, part| {
            self.build(pool, env, u, part)
        });
        *self.by_ty.borrow_mut() = done;
        got
    }

    /// The layout `id` names.
    #[must_use]
    pub fn get(&self, id: LayoutId) -> Rc<Layout> {
        Rc::clone(&self.data.borrow()[id.0])
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.data.borrow().len()
    }

    /// `layout`'s id: an equal layout's, or a new one.
    pub fn intern(&self, layout: Layout) -> LayoutId {
        if let Some(&id) = self.ids.borrow().get(&layout) {
            return id;
        }
        let mut data = self.data.borrow_mut();
        let id = LayoutId(data.len());
        let layout = Rc::new(layout);
        data.push(Rc::clone(&layout));
        self.ids.borrow_mut().insert(layout, id);
        id
    }

    /// Stripped `t`'s layout from its parts' ids, which `part` reads:
    /// rustc's `layout_of_uncached`. O(its own fields), and O(at most
    /// [`INDIRECT`] leaves) per field or variant for its scalar list.
    fn build(
        &self,
        pool: &InternPool,
        env: &dyn ProgramEnv,
        t: Ty,
        part: &dyn Fn(Ty) -> StageResult<LayoutId>,
    ) -> StageResult<LayoutId> {
        let named = || too_large(&hd_mono::shown_type(pool, env, t));
        let mem_of = |f: Ty| part(f).map(|id| self.get(id).part());
        let scalars_of = |f: Ty| part(f).map(|id| self.get(id).scalars as usize);
        let record = |tys: &[Ty], offsets: Vec<u32>| -> StageResult<Fields> {
            Ok(Fields::Record {
                offsets: offsets.into(),
                layouts: tys.iter().map(|&f| part(f)).collect::<StageResult<_>>()?,
            })
        };
        let sum = |tys: &[Ty]| -> StageResult<usize> {
            let mut n = 0;
            for &f in tys {
                n = (n + scalars_of(f)?).min(INDIRECT + 1);
            }
            Ok(n)
        };
        let layout = if !inline(pool, env, t) {
            let m = mem_one(pool, env, t, &mem_of, false)?;
            let n = m.leaves.len();
            Layout::new(m, n, Fields::Primitive, Variants::Single)
        } else if let Some(vs) = variants(pool, env, t) {
            let flat = enum_mem_by(&vs, &mem_of, &named)?;
            let mut n = flat.mem.leaves.len();
            for fields in vs.iter() {
                n = n.max(sum(fields)?);
            }
            let payloads = vs
                .iter()
                .zip(&flat.offsets)
                .map(|(tys, offsets)| record(tys, offsets.clone()))
                .collect::<StageResult<_>>()?;
            let tag = match flat.disc {
                crate::Disc::One(k) => Tag::One(k),
                crate::Disc::Cell(d) => Tag::Cell(d.leaf, d.enc),
            };
            Layout::new(
                flat.mem.clone(),
                n,
                Fields::Primitive,
                Variants::Enum { tag, payloads },
            )
        } else {
            let tys = value_parts(pool, env, t);
            let (m, parts) = payload_mem_by(&tys, &mem_of, &named)?;
            let n = sum(&tys)?;
            let offsets = parts.iter().map(|p| p.offset).collect();
            Layout::new(m, n, record(&tys, offsets)?, Variants::Single)
        };
        Ok(self.intern(layout))
    }
}

thread_local! {
    /// The layouts of the run or link in place ([`crate::cached`]).
    pub(crate) static LAYOUTS: RefCell<Option<Rc<Layouts>>> = const { RefCell::new(None) };
}

/// The layouts of the run or link in place, or, outside one, a table of
/// the caller's own.
#[must_use]
pub fn layouts() -> Rc<Layouts> {
    LAYOUTS.with(|l| l.borrow().clone()).unwrap_or_default()
}

/// `t`'s layout ([`Layouts::of`]).
pub fn layout(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> StageResult<Rc<Layout>> {
    let table = layouts();
    table.of(pool, env, t).map(|id| table.get(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Val;

    fn scalar(n: u32) -> Layout {
        let leaf = Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 4,
            signed: false,
        };
        Layout::new(
            Mem {
                size: 4 * n,
                align: 4,
                leaves: vec![leaf; n as usize],
                niche: None,
                uninhabited: false,
            },
            n as usize,
            Fields::Primitive,
            Variants::Single,
        )
    }

    #[test]
    fn equal_layouts_share_one_id() {
        let table = Layouts::default();
        let a = table.intern(scalar(1));
        let b = table.intern(scalar(2));
        assert_ne!(a, b);
        assert_eq!(table.intern(scalar(1)), a);
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(b).scalars, 2);
    }

    #[test]
    fn a_layout_past_indirect_is_memory_and_lists_no_scalars() {
        let at = scalar(u32::try_from(INDIRECT).expect("16"));
        assert!(matches!(&at.repr, Repr::Scalars(ls) if ls.len() == INDIRECT));
        assert_eq!(at.part().leaves.len(), INDIRECT);
        let past = scalar(u32::try_from(INDIRECT).expect("16") + 5);
        assert_eq!(past.repr, Repr::Memory);
        assert_eq!(past.scalars as usize, INDIRECT + 1, "saturated");
        assert!(past.part().leaves.is_empty());
        assert_eq!(past.part().size, 4 * 21);
        assert!(past.leaves_at(8).is_empty());
        assert_eq!(scalar(1).leaves_at(8)[0].offset, 8);
    }

    #[test]
    fn a_primitive_has_no_field_offsets() {
        assert!(Fields::Primitive.offsets().is_empty());
        let record = Fields::Record {
            offsets: vec![0, 8].into(),
            layouts: vec![LayoutId(0), LayoutId(1)].into(),
        };
        assert_eq!(record.offsets(), [0, 8]);
    }
}
