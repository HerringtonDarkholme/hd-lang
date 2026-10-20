//! Closures and function values (`crates/hd_wasm/DESIGN.md` §23.8, §23.19,
//! job 11). A function value is `(code, environment)`: the function-table
//! slot of its code instance and the address of a counted environment
//! that holds the captures, 0 when there are none. A code instance takes
//! the environment first, then its parameters, then the context its
//! caller passes, which holds the providers of its type's row (`ctx.rs`). A local that a closure shares (`Shared`, one
//! that is assigned after it is captured) lives in a cell, a counted
//! object holding its value, and every holder keeps the cell's address.

use hd_base::{HashMap, StageResult};
use hd_mono::Target;
use hd_tir::ir::{Body, CaptureMode, NONE, Tag, local_flags, sub_flags};
use hd_types::{Ty, TyData};
use wasm_encoder::{BlockType, Instruction as I};

use super::Em;
use super::value::Src;
use crate::layout::{Mem, Part, Walk, env_mem, mem as mem_of, strip};
use crate::module::{Ins, Sym, Val};
use crate::unsupported;

/// Each local a closure shares, or that a reference refers to (`roots`,
/// `refs.rs`), mapped to the instruction of its first store outside any
/// closure that captures it: that store declares it and makes its cell
/// (the Wasm GC backend's `cell_locals`). A parameter moves into its cell
/// on entry, so it has none (`u32::MAX`). Only the closures the body still
/// makes count: a reference's closure form is gone.
pub(super) fn cell_locals(b: &Body, roots: &[usize]) -> HashMap<usize, u32> {
    let mut cells: HashMap<usize, u32> = HashMap::default();
    for ci in 0..b.len() {
        if b.tags[ci] == Tag::Closure {
            for (l, shared) in captures(b, ci) {
                if shared {
                    cells.insert(l, u32::MAX);
                }
            }
        }
    }
    for l in roots {
        cells.insert(*l, u32::MAX);
    }
    if cells.is_empty() {
        return cells;
    }
    let subs = b.inst_subs();
    let mut through_capture = hd_base::HashSet::default();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            let rec = b.record(b.data[i][1]);
            let (start, len) = (rec[0] as usize, rec[1] as usize);
            for l in &b.cap_local[start..start + len] {
                through_capture.insert((b.data[i][0], l.raw()));
            }
        }
    }
    for (i, &sub) in subs.iter().enumerate() {
        let l = b.data[i][0];
        if b.tags[i] == Tag::LocalSet
            && b.local_flags.get(l as usize).copied().unwrap_or(0) & local_flags::PARAM == 0
            && !through_capture.contains(&(sub, l))
            && let Some(first) = cells.get_mut(&(l as usize))
            && *first == u32::MAX
        {
            *first = u32::try_from(i).expect("instructions");
        }
    }
    cells
}

/// The defining `Closure` of sub-body `sub`, not an instantiation of a
/// generic local function.
pub(super) fn closure_of(b: &Body, sub: u16) -> StageResult<usize> {
    let defining = |i: usize| {
        b.tags[i] == Tag::Closure && b.data[i][0] == u32::from(sub) && !b.is_instantiation(i)
    };
    match (0..b.len()).find(|&i| defining(i)) {
        Some(ci) => Ok(ci),
        None => unsupported("a closure body without its closure"),
    }
}

/// Whether closure body `sub` of an instance with type arguments `args`
/// suspends: its closure's function type says so.
pub(super) fn closure_suspends(
    input: &crate::Input<'_>,
    b: &Body,
    args: hd_types::TyList,
    sub: u16,
) -> StageResult<bool> {
    let (pool, env) = (input.pool, input.env);
    let t = hd_mono::subst(pool, env, b.item, args, b.ty[closure_of(b, sub)?]);
    Ok(matches!(
        pool.get(strip(pool, env, t)),
        TyData::Fn { suspends: true, .. }
    ))
}

/// The captures of closure instruction `ci`: each local, and whether it
/// is shared through a cell.
pub(super) fn captures(b: &Body, ci: usize) -> Vec<(usize, bool)> {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    (start..start + n)
        .map(|c| {
            (
                b.cap_local[c].raw() as usize,
                b.cap_mode.get(c) == Some(&CaptureMode::Shared),
            )
        })
        .collect()
}

impl Em<'_, '_, '_> {
    /// Whether TIR local `l` lives in a cell.
    pub(super) fn is_cell(&self, l: usize) -> bool {
        self.cells.contains_key(&l)
    }

    /// The layout of the value in local `l`'s cell, and its counted
    /// references: its erased view's in a generic method (`erased.rs`).
    fn cell_mem(&self, l: usize) -> StageResult<(Mem, Option<Walk>)> {
        let t = self.cell_ty(l);
        Ok((mem_of(self.pool(), self.env(), t)?, self.counted(t)))
    }

    /// The layout of closure `ci`'s environment: a cell's address for a
    /// shared capture, the value for any other.
    pub(super) fn env_layout(
        &self,
        caps: &[(usize, bool)],
    ) -> StageResult<(Mem, Vec<Part>, Option<Walk>)> {
        let fields: Vec<(Ty, bool)> = caps
            .iter()
            .map(|(l, shared)| (self.sub(self.b.local_ty[*l]), *shared))
            .collect();
        let (m, parts, walk) = env_mem(self.pool(), self.env(), &fields)?;
        Ok((m, parts, walk.filter(|_| self.input.runtime.is_some())))
    }

    /// A new cell for local `l`, holding the values `src` pushes, which
    /// the cell's count of them already covers: a local holding its
    /// address.
    fn new_cell(&mut self, l: usize, src: Src) -> StageResult<u32> {
        let src = self.cell_src(l, src)?;
        self.raw_cell(l, &src)
    }

    /// Pushes the zero of each value of local `l`'s type, for a cell that
    /// a capture reaches before its declaration stores into it.
    fn zero_src(&mut self, l: usize) -> StageResult<Src> {
        let (m, _) = self.cell_mem(l)?;
        let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        Ok(Src::Locals(ls))
    }

    /// The local holding local `l`'s cell, made now when no store has made
    /// it yet.
    pub(super) fn cell_of(&mut self, l: usize) -> StageResult<u32> {
        let c = self.local(u32::try_from(l).expect("locals"))?[0];
        self.f.get(c);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        let z = self.zero_src(l)?;
        let at = self.raw_cell(l, &z)?;
        self.f.get(at);
        self.f.set(c);
        self.f.op(I::End);
        Ok(c)
    }

    /// A new cell for local `l` holding `src`, already in the cell's
    /// layout.
    fn raw_cell(&mut self, l: usize, src: &Src) -> StageResult<u32> {
        let (m, walk) = self.cell_mem(l)?;
        let at = self.object(&m, walk.as_ref());
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, src)?;
        Ok(at)
    }

    /// Fresh locals holding local `l`'s value, read from its cell with no
    /// count of its own: a view that lives as long as the cell's value.
    pub(super) fn cell_read(&mut self, l: usize) -> StageResult<Vec<u32>> {
        let (m, _) = self.cell_mem(l)?;
        let c = self.cell_of(l)?;
        let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        for (leaf, x) in m.leaves.iter().zip(&ls) {
            self.f.get(c);
            self.f.op(leaf.load(0));
            self.f.set(*x);
        }
        if self.cell_converts(l) {
            let t = self.sub(self.b.local_ty[l]);
            return self.unerase(ls[0], self.cell_ty(l), t, false);
        }
        Ok(ls)
    }

    /// `LocalGet` of a cell's local: its values, a copy with its own count.
    pub(super) fn cell_get(&mut self, i: u32, l: usize) -> StageResult<()> {
        for x in self.cell_read(l)? {
            self.f.get(x);
        }
        self.store(i)
    }

    /// `LocalSet` of a cell's local. Its declaration makes a new cell, the
    /// last one's (from an earlier loop iteration) released; any other
    /// store writes the cell, counting the new value before it releases
    /// the old. `NONE` stores zeros: a recursive local function's name,
    /// declared before its closure exists.
    pub(super) fn cell_set(&mut self, i: u32, l: usize, v: u32) -> StageResult<()> {
        let t = self.cell_ty(l);
        // The value in the cell's layout, with the count the cell takes.
        let src = if v == NONE {
            self.zero_src(l)?
        } else {
            if !self.take(v) {
                self.retain_ref(v)?;
            }
            self.cell_src(l, Src::Ref(v))?
        };
        if self.cells.get(&l) == Some(&i) {
            let c = self.local(u32::try_from(l).expect("locals"))?[0];
            let at = self.raw_cell(l, &src)?;
            if self.counting {
                self.f.get(c);
                self.release_cell();
            }
            self.f.get(at);
            self.f.set(c);
            return Ok(());
        }
        let c = self.cell_of(l)?;
        let (m, walk) = self.cell_mem(l)?;
        let old: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        if walk.is_some() {
            for (leaf, o) in m.leaves.iter().zip(&old) {
                self.f.get(c);
                self.f.op(leaf.load(0));
                self.f.set(*o);
            }
        }
        self.store_part(
            c,
            &m,
            Part {
                first: 0,
                count: m.leaves.len(),
                offset: 0,
            },
            &src,
        )?;
        if walk.is_some() {
            self.release_locals(&old, t)?;
        }
        Ok(())
    }

    /// One count less on the cell address on the stack.
    pub(super) fn release_cell(&mut self) {
        if self.input.runtime.is_some() {
            self.f.helper(crate::rt::Helper::ReleaseRef);
        } else {
            self.f.op(I::Drop);
        }
    }

    /// Moves parameter `l`, whose values are in `ls`, into a new cell:
    /// the local's locals become the cell's address.
    pub(super) fn param_cell(&mut self, l: usize, ls: &[u32]) -> StageResult<()> {
        let at = self.new_cell(l, Src::Locals(ls.to_vec()))?;
        self.locals.insert(l, vec![at]);
        Ok(())
    }

    /// `Closure`: the code instance's table slot and a new environment
    /// holding the captures, each counted once more, or 0 for none. The
    /// defining `Closure` of a generic local function makes no value.
    pub(super) fn closure(&mut self, i: u32) -> StageResult<()> {
        let sub = self.b.data[i as usize][0];
        let generic = self
            .b
            .sub_flags
            .get(sub as usize)
            .is_some_and(|f| f & sub_flags::GENERIC != 0);
        if generic && !self.b.is_instantiation(i as usize) {
            return Ok(());
        }
        let Some(Target::Closure(key)) = self.calls.get(&i).cloned() else {
            return unsupported("a closure without its code instance");
        };
        let caps = captures(self.b, i as usize);
        self.f.body.push(Ins::TableSlot(Sym::Inst(key)));
        if caps.is_empty() {
            self.f.i32(0);
            return self.store_owned(i);
        }
        let (m, parts, walk) = self.env_layout(&caps)?;
        let mut srcs = Vec::new();
        for (l, shared) in &caps {
            let l32 = u32::try_from(*l).expect("locals");
            srcs.push(Src::Locals(if *shared {
                vec![self.cell_of(*l)?]
            } else if self.is_cell(*l) {
                // A copy of a local that lives in a cell only because a
                // reference refers to it.
                self.local_value(l32)?
            } else {
                self.local(l32)?
            }));
        }
        let at = self.object(&m, walk.as_ref());
        for (s, p) in srcs.iter().zip(&parts) {
            self.store_part(at, &m, *p, s)?;
        }
        self.retain_fields(at, &m, walk.as_ref());
        self.f.get(at);
        self.store_owned(i)
    }

    /// The prologue of closure `ci`'s code instance: each capture loaded
    /// from the environment, parameter `env`, into its local, as a value or
    /// a cell's address.
    pub(super) fn load_captures(&mut self, ci: usize, env: u32) -> StageResult<()> {
        let caps = captures(self.b, ci);
        if caps.is_empty() {
            return Ok(());
        }
        let (m, parts, _) = self.env_layout(&caps)?;
        for ((l, _), p) in caps.iter().zip(&parts) {
            let ls = self.local(u32::try_from(*l).expect("locals"))?;
            for (leaf, x) in m.leaves[p.first..p.first + p.count].iter().zip(&ls) {
                self.f.get(env);
                self.f.op(leaf.load(0));
                self.f.set(*x);
            }
        }
        Ok(())
    }

    /// `CallValue`: the environment, the arguments and the context of the
    /// callee type's row (`ctx.rs`), then an indirect call of the code, or
    /// a direct one of a closure collection resolved.
    pub(super) fn call_value(&mut self, i: u32, callee: u32, args: &[u32]) -> StageResult<()> {
        let ft = self.ty_of(callee);
        // A suspending value's code is its cold constructor, which
        // returns the frame that the call's `mut Suspend[T]` holds.
        let TyData::Fn {
            params, suspends, ..
        } = self.pool().get(strip(self.pool(), self.env(), ft))
        else {
            return unsupported("a call of a value that is not a function");
        };
        let mut ps = vec![Val::I32];
        let mut tys = Vec::new();
        for p in self.pool().list_items(params).to_vec() {
            let t = self.sub(p);
            ps.extend(self.vals(t)?);
            tys.push(t);
        }
        let mut have = vec![Val::I32];
        for r in args {
            have.extend(self.vals(self.ty_of(*r))?);
        }
        if have != ps {
            return unsupported("a call whose arguments lay out unlike the callee's parameters");
        }
        self.comp(callee, 1)?;
        // A suspending value's code counts each argument into its frame.
        self.push_args(args, (!suspends).then_some(&tys[..]))?;
        let c = self.push_fn_ctx(ft)?;
        ps.push(Val::I32);
        let results = self.vals(self.ty(i))?;
        if let Some(&Target::KnownClosure(key)) = self.calls.get(&i) {
            self.f.call(Sym::Inst(key));
        } else {
            self.comp(callee, 0)?;
            self.f.body.push(Ins::CallIndirect(ps, results));
        }
        let ty = self.ty(i);
        self.call_results(i, ty)?;
        self.release_ctx(c);
        Ok(())
    }
}
