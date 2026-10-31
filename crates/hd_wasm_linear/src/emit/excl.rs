//! Run-time exclusivity through unseen `*mut` aliases (`ref.excl.alias`,
//! stage 7e of `docs/future-work/RETURNED_REFERENCES_PATH.md`). A `&` or
//! `&mut` binding of a place inside a counted object registers the
//! place's address range in the runtime's loan table
//! ([`Helper::LoanBegin`]) until the binding's last use, the `Drop` the
//! ownership passes put there; a copy of the binding shares the entry.
//! An access through a counted reference that a registered loan of the
//! program could overlap checks the table ([`Helper::LoanCheck`]) and
//! panics `borrow-conflict` on a conflicting overlap. A program with no
//! such binding emits none of this.

use hd_base::StageResult;
use hd_mono::{ProgramEnv, Sub, subst};
use hd_tir::ir::{Body, Place, PlaceRoot, Proj, ReadMode, Tag, Word, slots};
use hd_types::{InternPool, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use crate::Input;
use crate::layout::{Kind, kind, mem as mem_of, ref_mutable};
use crate::module::{Global, Module};
use crate::op::Op as I;
use crate::rt::Helper;

/// A loan the program may register: the type of the counted object its
/// place lies in, the struct field path to the place, and whether it is
/// exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
    pub object: Ty,
    pub path: Vec<u32>,
    pub excl: bool,
}

/// How a `Store` into a holder binding registers its loan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Bind {
    /// A new loan of the stored reference's place.
    Begin,
    /// A copy of another holder: one more count of its entry.
    Share,
    /// A move from another holder: the entry passes over.
    Move,
}

/// A binding that holds a registered loan: its referent's size and
/// whether it is `&mut`, which with its value key the loan's entry.
#[derive(Clone, Copy)]
pub(super) struct Holder {
    size: u32,
    excl: bool,
}

/// The type at the end of the struct field `f` of type `t`.
fn field_ty(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, f: u32) -> Option<Ty> {
    let t = match pool.get(t) {
        TyData::Mut(inner) => inner,
        _ => t,
    };
    match pool.get(t) {
        TyData::Adt { def, args } => {
            let fs = env.data_fields(def)?;
            Some(subst(pool, env, def, args, *fs.get(f as usize)?))
        }
        TyData::Tuple { .. } => crate::layout::tuple_members(pool, t)
            .get(f as usize)
            .copied(),
        _ => None,
    }
}

/// The place of loan-taking `Ref` `word`, rooted at a value of type
/// `root`, as a counted object's field: the object's type and the field
/// path, when the place's last dereference reads a `*mut`,
/// only fields follow it, and no element or variant step is on the path
/// (an element reference's loan is its collection's pin).
fn loan_place(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    root: Ty,
    p: &Place,
) -> Option<(Ty, Vec<u32>)> {
    let d = p.proj.iter().rposition(|s| *s == Proj::Deref)?;
    let mut ty = root;
    let mut k = 0;
    while k < d {
        ty = match p.proj[k] {
            Proj::Field(f) => field_ty(pool, env, ty, f)?,
            Proj::Deref => match kind(pool, env, ty) {
                Kind::Ref { target, .. } => target,
                Kind::Data(tys) => {
                    k += 1;
                    let Some(Proj::Field(f)) = p.proj.get(k) else {
                        return None;
                    };
                    *tys.get(*f as usize)?
                }
                _ => return None,
            },
            Proj::Elem(_) | Proj::Variant(_) => return None,
        };
        k += 1;
    }
    let path = fields(&p.proj[d + 1..])?;
    match kind(pool, env, ty) {
        Kind::Ref {
            target,
            fat: false,
            whole: true,
        } if ref_mutable(pool, env, ty) => Some((target, path)),
        _ => None,
    }
}

/// The struct field indices `steps` name, when they are all fields.
fn fields(steps: &[Proj]) -> Option<Vec<u32>> {
    steps
        .iter()
        .map(|s| match s {
            Proj::Field(f) => Some(*f),
            _ => None,
        })
        .collect()
}

/// Whether two field paths from one object overlap: one is a prefix of
/// the other (`ref.excl.place`).
fn overlap(a: &[u32], b: &[u32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x == y)
}

/// Each `Store` of a body into a bare local binding that registers a
/// loan, by the binding: a loan-taking `Ref` of a place inside a counted
/// object, or a copy or a move of another such binding. `ty` gives a
/// local's or an instruction's type at this instance.
fn binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
) -> Vec<(usize, usize, Bind, Option<Loan>)> {
    let bare = |l: usize| {
        matches!(
            kind(pool, env, sub(b.local_ty[l])),
            Kind::Ref {
                fat: false,
                whole: false,
                ..
            }
        )
    };
    let mut out: Vec<(usize, usize, Bind, Option<Loan>)> = Vec::new();
    let mut grew = true;
    while grew {
        grew = false;
        for i in 0..b.tags.len() {
            let Some((l, true)) = b.stored_local(i) else {
                continue;
            };
            let (l, v) = (l.idx(), b.data[i][1] as usize);
            if out.iter().any(|x| x.0 == i) || !bare(l) || v >= b.tags.len() {
                continue;
            }
            let found = match b.tags[v] {
                Tag::Ref => b.decode_place(b.data[v][0]).and_then(|p| {
                    let root = match p.root {
                        PlaceRoot::Local(r) => sub(b.local_ty[r.idx()]),
                        PlaceRoot::Global(_, t) => sub(t),
                        _ => return None,
                    };
                    let (object, path) = loan_place(pool, env, root, &p)?;
                    let excl = ref_mutable(pool, env, sub(b.ty[v]));
                    Some((Bind::Begin, Some(Loan { object, path, excl })))
                }),
                Tag::Read => b.local_read(v).and_then(|m| {
                    out.iter().any(|x| x.1 == m.idx()).then(|| {
                        let copy = b.data[v][1] == ReadMode::Copy as u32;
                        (if copy { Bind::Share } else { Bind::Move }, None)
                    })
                }),
                _ => None,
            };
            if let Some((bind, loan)) = found {
                out.push((i, l, bind, loan));
                grew = true;
            }
        }
    }
    out
}

/// The loans the program may register ([`Module::loans`]): every
/// instance body's loan-taking bindings. Erased bodies and thunks
/// register none.
pub(crate) fn collect_loans(input: &Input<'_>, m: &mut Module) {
    let t = &input.collected.table;
    if input.runtime.is_none() {
        return;
    }
    for id in input.order {
        let (Sub::Body(_), Some(b)) = (t.sub[id.idx()], (input.body)(*id)) else {
            continue;
        };
        let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
        let sub = |ty| subst(input.pool, input.env, item, args, ty);
        for (_, _, _, loan) in binds(input.pool, input.env, b, &sub) {
            if let Some(loan) = loan
                && !m.loans.contains(&loan)
            {
                m.loans.push(loan);
            }
        }
    }
}

impl Em<'_, '_, '_> {
    /// Finds the body's holder bindings and the stores that bind them,
    /// when the program registers loans at all.
    pub(super) fn find_holders(&mut self) {
        if self.m.loans.is_empty() {
            return;
        }
        let sub = |t| self.sub(t);
        let found = binds(self.pool(), self.env(), self.b, &sub);
        for (i, l, bind, _) in found {
            let ty = self.sub(self.b.local_ty[l]);
            let Kind::Ref { target, .. } = self.kind(ty) else {
                continue;
            };
            let Ok(m) = mem_of(self.pool(), self.env(), target) else {
                continue;
            };
            let excl = ref_mutable(self.pool(), self.env(), ty);
            self.holders.insert(l, Holder { size: m.size, excl });
            self.binds.insert(u32::try_from(i).expect("insts"), bind);
        }
    }

    /// Whether local `l` holds a registered loan: a holder binding with a
    /// count of its own, in no cell.
    fn holder(&self, l: usize) -> Option<Holder> {
        let h = *self.holders.get(&l)?;
        (!self.is_cell(l) && !self.loan_locals.contains_key(&l)).then_some(h)
    }

    /// Pushes the key of holder `h`'s entry after its value, then calls
    /// [`Helper::LoanCount`] with `delta`.
    fn loan_count(&mut self, h: Holder, delta: i32) {
        self.f.u32(h.size);
        self.f.i32(i32::from(h.excl));
        self.f.i32(delta);
        self.f.helper(Helper::LoanCount);
    }

    /// The end of the loan holder local `l` (Wasm locals `ls`) holds, at
    /// its release: one count less of its entry.
    pub(super) fn loan_release(&mut self, l: usize, ls: &[u32]) {
        if let (Some(h), Some(&x)) = (self.holder(l), ls.first()) {
            self.f.get(x);
            self.loan_count(h, -1);
        }
    }

    /// Before `Store` `i`: the loan of the value a holder binding still
    /// holds ends, since the store replaces it.
    pub(super) fn loan_before_store(&mut self, i: u32) {
        if !self.binds.contains_key(&i) {
            return;
        }
        let Some((l, true)) = self.b.stored_local(i as usize) else {
            return;
        };
        if let Some(ls) = self.locals.get(&l.idx()).cloned() {
            self.loan_release(l.idx(), &ls);
        }
    }

    /// After `Store` `i` into a holder binding: its new loan's entry, or
    /// one more count of the entry of the binding it copied.
    pub(super) fn loan_after_store(&mut self, i: u32) {
        let Some(&bind) = self.binds.get(&i) else {
            return;
        };
        let Some((l, true)) = self.b.stored_local(i as usize) else {
            return;
        };
        let (Some(h), Some(&x)) = (
            self.holder(l.idx()),
            self.locals.get(&l.idx()).and_then(|ls| ls.first()),
        ) else {
            return;
        };
        self.f.get(x);
        match bind {
            Bind::Begin => {
                self.f.u32(h.size);
                self.f.i32(i32::from(h.excl));
                self.f.helper(Helper::LoanBegin);
            }
            Bind::Share => self.loan_count(h, 1),
            Bind::Move => self.f.op(I::Drop),
        }
    }

    /// Whether an access of `path` in a `object`, a write or a read, may
    /// conflict with a loan the program registers.
    fn may_conflict(&self, object: Ty, path: &[u32], write: bool) -> bool {
        (self.m.loans.iter())
            .any(|l| l.object == object && overlap(&l.path, path) && (write || l.excl))
    }

    /// Before instruction `i`: the loan check of the access it makes
    /// through a counted reference, where a registered loan may conflict
    /// with it. A `Store` or an `IsUnique` writes its place, a `Ref`
    /// borrows it, and a `Read` reads it; a call that takes a counted
    /// reference accesses its whole object, a write for a `*mut`.
    pub(super) fn loan_access(&mut self, i: u32) -> StageResult<()> {
        if self.m.loans.is_empty() || !self.owned {
            return Ok(());
        }
        let tag = self.b.tags[i as usize];
        if matches!(tag, Tag::Call | Tag::CallDyn | Tag::CallValue) {
            let mut args = Vec::new();
            slots(self.b, i as usize, |_, v, word| {
                if matches!(word, Word::Value(_)) && v < i {
                    args.push(v);
                }
            });
            for v in args {
                let ty = self.ty_of(v);
                let Kind::Ref {
                    target,
                    fat: false,
                    whole: true,
                } = self.kind(ty)
                else {
                    continue;
                };
                let write = ref_mutable(self.pool(), self.env(), ty);
                if !self.vals.contains_key(&v) || !self.may_conflict(target, &[], write) {
                    continue;
                }
                let size = mem_of(self.pool(), self.env(), target)?.size;
                self.loan_check(write, size, |em| em.comp(v, 0))?;
            }
            return Ok(());
        }
        let write = match tag {
            Tag::Store | Tag::IsUnique => true,
            Tag::Ref => ref_mutable(self.pool(), self.env(), self.ty(i)),
            Tag::Read => false,
            _ => return Ok(()),
        };
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(());
        };
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return Ok(());
        }
        let Some((d, Some((target, false)))) = self.last_through(&p)? else {
            return Ok(());
        };
        let before = self.walk_ty(self.at_root_ty(p.root), &p.proj[..d])?;
        let (Kind::Ref { whole: true, .. }, Some(path)) =
            (self.kind(before), fields(&p.proj[d + 1..]))
        else {
            return Ok(());
        };
        if !self.may_conflict(target, &path, write) {
            return Ok(());
        }
        let (ft, part) = self.fold_path(target, &path)?;
        let size = mem_of(self.pool(), self.env(), ft)?.size;
        self.loan_check(write, size, |em| {
            let mut fresh = Vec::new();
            let (at, _) = em.walk_place(i, &p, d, None, &mut fresh)?;
            let r = em.at_i32(&at)?;
            em.release_fresh(&fresh)?;
            em.f.get(r);
            if part.offset != 0 {
                em.f.u32(part.offset);
                em.f.op(I::I32Add);
            }
            Ok(())
        })
    }

    /// While any loan is registered: `addr` pushes the accessed place's
    /// address, of `size` bytes, and [`Helper::LoanCheck`] checks it.
    fn loan_check(
        &mut self,
        write: bool,
        size: u32,
        addr: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        self.f.global_get(Global::LoanTop);
        self.f.op(I::If(BlockType::Empty));
        addr(self)?;
        self.f.u32(size);
        self.f.i32(i32::from(write));
        self.f.helper(Helper::LoanCheck);
        self.f.op(I::End);
        Ok(())
    }
}
