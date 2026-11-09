//! Closures and function values (`crates/hd_wasm_linear/DESIGN.md` §23.8, §23.19,
//! job 11). A function value is `(code, environment)`: the function-table
//! slot of its code instance and the address of a counted environment
//! that holds the captures, 0 when there are none. A code instance takes
//! the environment first, then its parameters, then the context its
//! caller passes, which holds the providers of its type's row (`ctx.rs`). A local that a closure shares (`Shared`, one
//! that is assigned after it is captured) lives in a cell, a counted
//! object holding its value, and every holder keeps the cell's address.
//! A frame cell's local (`CaptureMode::Frame`) keeps it in a frame slot
//! instead, whose zero count word makes the holders' counts do nothing.

use crate::op::Op as I;
use hd_base::HashSet;
use hd_base::SliceSort as _;
use hd_base::{Hash128, HashMap, StageResult};
use hd_mono::Target;
use hd_tir::ir::{Body, CaptureMode, NONE, Tag, local_flags, ref_word, sub_flags};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{Mem, Part, Walk, env_mem, mem as mem_of, strip};
use crate::module::{Sym, Val};
use crate::rt::Helper;

/// Each local of `stored` (a cell or a slot, [`stored_locals`]) mapped
/// to the instruction of its first store outside any closure that
/// captures it: that store declares it and makes its cell during RC
/// lowering. A parameter moves into its cell on entry, so it has none
/// (`u32::MAX`).
pub(super) fn cell_locals(b: &Body, stored: &[usize]) -> HashMap<usize, u32> {
    let mut cells: HashMap<usize, u32> = stored.iter().map(|&l| (l, u32::MAX)).collect();
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
        let Some((l, _)) = super::shape::local_store(b, i) else {
            continue;
        };
        if b.local_flags.get(l).copied().unwrap_or(0) & local_flags::PARAM == 0
            && !through_capture.contains(&(sub, u32::try_from(l).expect("locals")))
            && let Some(first) = cells.get_mut(&l)
            && *first == u32::MAX
        {
            *first = u32::try_from(i).expect("instructions");
        }
    }
    cells
}

/// The locals some closure of `b` shares through a cell. A cell is an
/// object a closure reaches as a `*mut` does its pointee, so a reference
/// into the variable takes a loan on it (`POINTER_LOANS.md`, question 6).
pub(super) fn shared_locals(b: &Body) -> hd_base::HashSet<usize> {
    (0..b.len())
        .filter(|&ci| b.tags[ci] == Tag::Closure)
        .flat_map(|ci| captures(b, ci))
        .filter_map(|(l, shared)| shared.then_some(l))
        .collect()
}

/// The locals of sub-body `sub` that the passes put in a cell or a frame
/// slot (`Body::storage`), and those of them in a slot, once promoted
/// arguments apply: one reads its local's value (§23.40), so a local that
/// only promoted references reach and no closure shares stays in Wasm
/// locals.
pub(super) fn stored_locals(
    input: &crate::Input<'_>,
    b: &Body,
    (item, args): (hd_base::DefId, hd_types::TyList),
    sub: u16,
    promoted: &HashSet<u32>,
) -> (Vec<usize>, Vec<usize>) {
    let (pool, env) = (input.pool, input.env);
    let shared = if promoted.is_empty() {
        HashSet::default()
    } else {
        shared_locals(b)
    };
    let reached = |l: usize| {
        promoted.is_empty()
            || shared.contains(&l)
            || (0..b.len()).any(|i| {
                super::shape::ref_local(b, i) == Some(l)
                    && !promoted.contains(&u32::try_from(i).expect("insts"))
                    && matches!(
                        crate::layout::kind(
                            pool,
                            env,
                            hd_mono::subst(pool, env, item, args, b.ty[i])
                        ),
                        crate::layout::Kind::Ref { fat: false, .. }
                    )
            })
    };
    let (mut cells, mut slots) = (Vec::new(), Vec::new());
    for &(s, l, k) in &b.storage {
        if s.idx() == usize::from(sub) && reached(l.idx()) {
            cells.push(l.idx());
            if k == hd_tir::ir::Storage::Slot {
                slots.push(l.idx());
            }
        }
    }
    (cells, slots)
}

/// The defining `Closure` of sub-body `sub`, not an instantiation of a
/// generic local function, at type arguments `args`: the instruction,
/// whether its function suspends, and its result type. A sub-body past
/// the root is a closure's (`crates/hd_tir/DESIGN.md`, "sub-body").
pub(super) fn closure_of(
    input: &crate::Input<'_>,
    b: &Body,
    args: hd_types::TyList,
    sub: u16,
) -> (usize, bool, Ty) {
    let defining = |i: usize| {
        b.tags[i] == Tag::Closure && b.data[i][0] == u32::from(sub) && !b.is_instantiation(i)
    };
    let ci = (0..b.len())
        .find(|&i| defining(i))
        .expect("a sub-body's closure");
    let (pool, env) = (input.pool, input.env);
    let t = hd_mono::subst(pool, env, b.item, args, b.ty[ci]);
    let TyData::Fn {
        suspends, result, ..
    } = pool.get(strip(pool, env, t))
    else {
        unreachable!("a closure's type is a function")
    };
    (ci, suspends, result)
}

/// The immutable name introduced by this closure itself. Its declaration
/// and closure share a syntax node; other captures do not. Address-taking
/// keeps the independently addressable cell and its existing ownership.
pub(super) fn self_local(b: &Body, ci: usize) -> Option<usize> {
    b.closure_self(ci).map(hd_base::LocalId::idx)
}

/// The captures of closure instruction `ci`: each local, and whether it
/// is shared through a cell (`hd_tir::own::closure_captures`).
pub(super) fn captures(b: &Body, ci: usize) -> Vec<(usize, bool)> {
    hd_tir::own::closure_captures(b, ci).collect()
}

/// Whether walk `w` runs drop glue anywhere.
fn has_glue(w: &Walk) -> bool {
    match w {
        Walk::Glue => true,
        Walk::Seq(ws) => ws.iter().any(has_glue),
        Walk::Switch(_, arms) => arms.iter().flatten().any(has_glue),
        Walk::Ref(_) | Walk::Known(_) => false,
    }
}

/// Whether each capture [`captures`] lists for closure `ci` moves into
/// the environment (`CaptureMode::Move`, the move pass's rule 5): the
/// environment takes the local's count, so the closure retains nothing.
fn moved_captures(b: &Body, ci: usize) -> Vec<bool> {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    let own = self_local(b, ci);
    (start..start + n)
        .filter(|&c| Some(b.cap_local[c].idx()) != own)
        .map(|c| b.cap_mode.get(c) == Some(&CaptureMode::Move))
        .collect()
}

impl Em<'_, '_, '_> {
    /// Whether TIR local `l` lives in a cell.
    pub(super) fn is_cell(&self, l: usize) -> bool {
        self.cells.contains_key(&l)
    }

    /// The layout of the value in local `l`'s cell, and its counted
    /// references.
    pub(super) fn cell_mem(&self, l: usize) -> StageResult<(Mem, Option<Walk>)> {
        let t = self.sub(self.b.local_ty[l]);
        let mut m = mem_of(self.pool(), self.env(), t)?;
        let flags = self.drop_node_paths(t).len();
        if flags != 0 {
            m.align = m.align.max(4);
            m.size = (m.size.next_multiple_of(4) + u32::try_from(flags).expect("flags") * 4)
                .next_multiple_of(m.align);
        }
        Ok((m, self.counted(t)))
    }

    /// The layout of closure `ci`'s environment, whose captures are
    /// `caps`: a cell's address for a shared capture, the value for any
    /// other, and after them the address of the cell of its creator's own
    /// loans through each pointer of [`Em::own_caps`].
    pub(super) fn env_layout(
        &self,
        ci: usize,
        caps: &[(usize, bool)],
    ) -> StageResult<(Mem, Vec<Part>, Option<Walk>)> {
        let own = self.own_caps(ci).into_iter().map(|_| (Ty::I32, true));
        let fields: Vec<(Ty, bool)> = caps
            .iter()
            .map(|(l, shared)| (self.sub(self.b.local_ty[*l]), *shared))
            .chain(own)
            .collect();
        let (m, parts, walk) = env_mem(self.pool(), self.env(), &fields)?;
        Ok((m, parts, walk.filter(|_| self.input.runtime.is_some())))
    }

    /// A new cell for local `l`, holding the values `src` pushes, which
    /// the cell's count of them already covers: a local holding its
    /// address.
    pub(super) fn new_cell(&mut self, l: usize, src: &Src) -> StageResult<u32> {
        self.raw_cell(l, src, true)
    }

    /// Pushes the zero of each value of local `l`'s type, for a cell that
    /// a capture reaches before its declaration stores into it.
    fn zero_src(&mut self, l: usize) -> StageResult<Src> {
        let (m, _) = self.cell_mem(l)?;
        let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        Ok(Src::Locals(ls))
    }

    /// The local holding local `l`'s cell, made now when no store has made
    /// it yet. Where the cell certainly exists ([`Em::cell_made`]) the
    /// local is read as it is, with no test.
    pub(super) fn cell_of(&mut self, l: usize) -> StageResult<u32> {
        let c = self.local(u32::try_from(l).expect("locals"))?[0];
        if self.cell_made(l) {
            return Ok(c);
        }
        self.f.get(c);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        let z = self.zero_src(l)?;
        let at = self.raw_cell(l, &z, false)?;
        self.f.get(at);
        self.f.set(c);
        self.f.op(I::End);
        Ok(c)
    }

    /// Whether local `l`'s cell certainly exists at the instruction being
    /// emitted, so its local holds a nonzero address:
    ///
    /// - the prologue made or received it ([`Em::entry_cells`]): a
    ///   parameter moved into its cell that nothing stores into again, or
    ///   a shared capture, whose holder made the cell before passing it; or
    /// - known-null found the local holding its value there on every path
    ///   (`Body::full_cells`, rule 5 of `docs/compiler/tir-v2.md`,
    ///   "Known-null"): a store into it ran since its last `Drop`, move or
    ///   taking `Ref`, and every store makes the cell (its declaration, or
    ///   [`Em::cell_of`] for any other).
    ///
    /// A use in a suspending body, whose resume reloads its locals from the
    /// frame and enters blocks in the middle (`susp.rs`), takes only the
    /// first. Anything else keeps the test.
    fn cell_made(&self, l: usize) -> bool {
        if self.entry_cells.contains(&l) {
            return true;
        }
        if self.susp.is_some() {
            return false;
        }
        let (Some(&j), Ok(l)) = (self.emitting.last(), u32::try_from(l)) else {
            return false;
        };
        self.b.full_cells.binary_search(&(j, l)).is_ok()
    }

    /// Gives each local of `slots` ([`stored_locals`]) a slot in the function's
    /// frame on the local stack (§23.38): a zero count word, as a static
    /// object's, then the value in its cell's layout, so a count update
    /// through the slot's address does nothing.
    pub(super) fn plan_slots(&mut self, slots: &[usize]) -> StageResult<()> {
        let mut ls = slots.to_vec();
        if ls.is_empty() {
            return Ok(());
        }
        ls.sort_stable();
        let mut size = 0;
        for l in ls {
            let (m, _) = self.cell_mem(l)?;
            let at = size + crate::module::HEADER;
            size = (at + m.size).next_multiple_of(8);
            self.slots.insert(l, at);
        }
        // The frame's local is made at its first use, after the
        // parameters' ([`Em::frame_base`]).
        self.frame = Some((NONE, size));
        Ok(())
    }

    /// The local holding the address of the function's frame of slots.
    fn frame_base(&mut self) -> Option<u32> {
        let (base, size) = self.frame?;
        if base != NONE {
            return Some(base);
        }
        let base = self.f.local(Val::I32);
        self.frame = Some((base, size));
        Some(base)
    }

    /// Releases the value in local `l`'s slot, whose address local `c`
    /// holds, or 0 when it holds none: the slot's end, as the last count
    /// of a cell is the cell's.
    fn release_slot(&mut self, l: usize, c: u32) -> StageResult<()> {
        let t = self.sub(self.b.local_ty[l]);
        if !self.needs_drop(t) {
            return Ok(());
        }
        let (m, _) = self.cell_mem(l)?;
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        if !matches!(self.kind(t), crate::layout::Kind::Value) || self.drop_calls(t).is_empty() {
            let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
            for (leaf, x) in m.leaves.iter().zip(&ls) {
                self.f.get(c);
                self.f.op(leaf.load(0));
                self.f.set(*x);
            }
            self.release_locals(&ls, t)?;
        } else {
            self.f.get(c);
            self.f.helper(Helper::DropFields {
                leaves: m.leaves,
                walk: crate::layout::walk(self.pool(), self.env(), t)
                    .unwrap_or_else(|| Walk::Seq(Vec::new())),
                drops: self.cell_drop_calls(t),
            });
            self.mark_drop_place(&Self::local_place(l), false);
        }
        self.f.op(I::End);
        Ok(())
    }

    /// One count less on local `l`'s cell, whose address local `c` holds:
    /// for a slot, the release of the value in it.
    pub(super) fn release_cell_of(&mut self, l: usize, c: u32) -> StageResult<()> {
        if self.slots.contains_key(&l) {
            return self.release_slot(l, c);
        }
        self.f.get(c);
        self.release_cell();
        Ok(())
    }

    /// A new cell for local `l` holding `src`, already in the cell's
    /// layout; for a slot local, its slot.
    fn raw_cell(&mut self, l: usize, src: &Src, live: bool) -> StageResult<u32> {
        if let Some(&off) = self.slots.get(&l)
            && let Some(base) = self.frame_base()
        {
            let (m, _) = self.cell_mem(l)?;
            let at = self.f.local(Val::I32);
            // A zero count word and a zero descriptor word: a `*T` that
            // reaches the slot reads its borrow state there (§23.4).
            self.f.get(base);
            self.f.op(I::I64Const(0));
            self.f.op(I::I64Store(crate::module::mem(
                crate::module::HEAP,
                u64::from(off - crate::module::HEADER),
                3,
            )));
            self.f.get(base);
            self.f.u32(off);
            self.f.op(I::I32Add);
            self.f.set(at);
            let part = Part {
                first: 0,
                count: m.leaves.len(),
                offset: 0,
            };
            self.store_part(at, &m, part, src)?;
            self.init_cell_drop(at, self.sub(self.b.local_ty[l]), live);
            return Ok(at);
        }
        // A local a top-level binding's initializer takes a local
        // reference to (its `&e`, `ref.expr.allocate`): the binding may
        // keep the new object for the whole program, so it is static.
        let key = self.static_cells.or_else(|| {
            self.init_key.filter(|_| {
                (0..self.b.len()).any(|i| {
                    self.b.tags[i] == Tag::Ref
                        && self.b.data[i][1] & ref_word::LOCAL != 0
                        && self
                            .b
                            .local_in_place(self.b.data[i][0])
                            .map(hd_base::LocalId::idx)
                            == Some(l)
                })
            })
        });
        if let Some(key) = key {
            let at = self.static_cell(key, l, src)?;
            self.init_cell_drop(at, self.sub(self.b.local_ty[l]), live);
            return Ok(at);
        }
        let (m, walk) = self.cell_mem(l)?;
        let ty = self.sub(self.b.local_ty[l]);
        let cyclic = self.cyclic_holding(&[ty]);
        let recursive = self.recursive_object(ty);
        let drops = self.cell_drop_calls(ty);
        let at = self.object_drops(&m, walk.as_ref(), cyclic, recursive, Some(ty), drops);
        #[cfg(feature = "checked-report")]
        if self.input.runtime.is_some_and(|r| r.checked.is_some())
            && shared_locals(self.b).contains(&l)
        {
            crate::rt::checked_kind(&mut self.f, self.m, 2);
        }
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, src)?;
        self.init_cell_drop(at, ty, live);
        Ok(at)
    }

    /// Local `l`'s cell in a handle body, whose once-cell has key `key`:
    /// a static object, a zero count word and then the value, which lives
    /// as long as the program, so no count changes it and every reference
    /// to it is one address no count reaches (`annot.handle.constants`).
    /// A checked run's teardown releases the value, as the once-cell's.
    fn static_cell(&mut self, key: Hash128, l: usize, src: &Src) -> StageResult<u32> {
        let (m, _) = self.cell_mem(l)?;
        let header = crate::module::HEADER;
        let base = self.m.static_slot(header + m.size, 8);
        let addr = base + header;
        let mut bytes = key.0.to_le_bytes().to_vec();
        bytes.extend_from_slice(&u32::try_from(l).expect("locals").to_le_bytes());
        let t = self.sub(self.b.local_ty[l]);
        self.note_stored(
            hd_base::hash128(&bytes),
            t,
            Some((crate::module::Address::Static(addr), m.leaves.clone())),
        )?;
        let at = self.f.local(Val::I32);
        self.f.static_addr(addr);
        self.f.set(at);
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
        Ok(ls)
    }

    /// A plain read of a cell's local, `Read [$l]`: its values, a copy with its own count.
    pub(super) fn cell_get(&mut self, i: u32, l: usize) -> StageResult<()> {
        for x in self.cell_read(l)? {
            self.f.get(x);
        }
        self.store(i)
    }

    /// Moves local `l`'s value out of its cell into instruction `i`,
    /// leaving no payload for the cell's release and no cell for a later
    /// exit drop. A frame slot is emptied in place; an ordinary cell is
    /// released. Move proves this is the local's last access; shared
    /// captures are not candidates.
    pub(super) fn cell_take(&mut self, i: u32, l: usize) -> StageResult<()> {
        let slot = self.slots.contains_key(&l);
        let (m, walk) = self.cell_mem(l)?;
        let c = self.cell_of(l)?;
        let held: Vec<u32> = m.leaves.iter().map(|leaf| self.f.local(leaf.val)).collect();
        for (leaf, x) in m.leaves.iter().zip(&held) {
            self.f.get(c);
            self.f.op(leaf.load(0));
            self.f.set(*x);
        }
        // Only a counted payload needs its old copy cleared: an uncounted
        // one is never released from the cell.
        if walk.is_some() {
            let zero = self.zero_src(l)?;
            let all = Part {
                first: 0,
                count: m.leaves.len(),
                offset: 0,
            };
            self.store_part(c, &m, all, &zero)?;
        }
        if !slot {
            self.init_cell_drop(c, self.sub(self.b.local_ty[l]), false);
            self.f.get(c);
            // ruled out: own::cell_move states the release for this moved local's storage row.
            let Some(reason) = self.operation_count(
                i,
                Tag::Release,
                super::CountPart::Local(u32::try_from(l).expect("local")),
            ) else {
                return crate::unsupported("a moved cell without its stated storage cleanup");
            };
            self.because(reason, Self::release_cell);
        }
        self.f.zero(c);
        for x in held {
            self.f.get(x);
        }
        self.store_owned(i)
    }

    /// A `Store` into the whole of a cell's local. Its declaration makes a
    /// new cell, the last one's (from an earlier loop iteration) released;
    /// any other store writes the cell, counting the new value before it
    /// releases the old. `NONE` stores zeros: a recursive local function's
    /// name, declared before its closure exists.
    pub(super) fn cell_set(&mut self, i: u32, l: usize, v: u32) -> StageResult<()> {
        // The value in the cell's layout, with the count the cell takes.
        let src = if v == NONE {
            self.zero_src(l)?
        } else {
            self.consumed(v)?
        };
        if self.cells.get(&l) == Some(&i) {
            let c = self.local(u32::try_from(l).expect("locals"))?[0];
            // The last value or cell (an earlier round's) is overwritten.
            let was = self.f.reason.replace(hd_tir::own::Reason::Overwritten);
            // A slot is the same memory each time, so the last value in
            // it ends before the new one is stored.
            let slot = self.slots.contains_key(&l);
            let releases = self.store_releases(i);
            if slot && releases {
                self.release_slot(l, c)?;
            }
            let at = self.raw_cell(l, &src, v != NONE)?;
            if releases && !slot {
                self.f.get(c);
                self.release_cell();
            }
            self.f.reason = was;
            self.f.get(at);
            self.f.set(c);
            return Ok(());
        }
        self.cell_put(l, &src)
    }

    /// Replaces the value in local `l`'s cell with the values `src`
    /// pushes, in the cell's layout and with the count the cell takes,
    /// then releases the old one.
    pub(super) fn cell_put(&mut self, l: usize, src: &Src) -> StageResult<()> {
        let at = self.cell_of(l)?;
        let (m, _) = self.cell_mem(l)?;
        let d = Dst::Mem(at, 0, &m.leaves);
        let old = self.old_locals(d);
        self.replace_late(d, self.sub(self.b.local_ty[l]), src, &old)
    }

    /// One count less on the cell address on the stack.
    pub(super) fn release_cell(&mut self) {
        if self.input.runtime.is_some() {
            self.f.helper(crate::rt::Helper::ReleaseKnown);
        } else {
            self.f.op(I::Drop);
        }
    }

    /// Moves parameter `l`, whose values are in `ls`, into a new cell, or
    /// its slot (`slot_locals`): the local's locals become its address.
    pub(super) fn param_cell(&mut self, l: usize, ls: &[u32]) -> StageResult<()> {
        let at = self.new_cell(l, &Src::Locals(ls.to_vec()))?;
        self.locals.insert(l, vec![at]);
        self.param_vals.insert(l, ls.to_vec());
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
            unreachable!("collection selects every emitted closure's code instance");
        };
        let caps = captures(self.b, i as usize);
        // A lent parameter holds no count to move: the environment
        // counts its own copy.
        let moved: Vec<bool> = moved_captures(self.b, i as usize)
            .into_iter()
            .zip(&caps)
            .map(|(mv, (l, _))| mv && !self.borrowed.contains(l))
            .collect();
        self.f.table_slot(Sym::Inst(key));
        if caps.is_empty() {
            self.f.i32(0);
            return self.store_owned(i);
        }
        let (m, parts, walk) = self.env_layout(i as usize, &caps)?;
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
        // The cells of the creator's own loans (`POINTER_LOANS.md` E.2).
        for l in self.own_caps(i as usize) {
            let c = self.own_cell_for(l)?;
            srcs.push(Src::Locals(vec![c]));
        }
        let tys: Vec<Ty> = caps
            .iter()
            .map(|(l, _)| self.sub(self.b.local_ty[*l]))
            .collect();
        // The literal knows whether it captures a shared cell. Its
        // value type alone cannot describe that mutable reference edge.
        let cyclic = caps.iter().any(|(_, shared)| *shared) || self.cyclic_holding(&tys);
        let at = self.object(&m, walk.as_ref(), cyclic, false, None);
        #[cfg(feature = "checked-report")]
        crate::rt::checked_kind(&mut self.f, self.m, 1);
        for (s, p) in srcs.iter().zip(&parts) {
            self.store_part(at, &m, *p, s)?;
        }
        // A shared capture's slot holds its cell; a moved one, the
        // local's own count.
        let copied: Vec<Ty> = caps
            .iter()
            .zip(&tys)
            .zip(&moved)
            .map(|(((_, shared), t), mv)| if *shared || *mv { Ty::VOID } else { *t })
            .collect();
        let mut skip = self.part_colls(&copied, &parts);
        let colls = skip.len();
        for (p, _) in parts.iter().zip(&moved).filter(|(_, mv)| **mv) {
            skip.extend((p.first..p.first + p.count).map(|k| u32::try_from(k).expect("leaves")));
        }
        // Drop glue releases a value whole: a moved part's local then
        // releases its count, as the `Drop` the move took would have.
        let glue = walk.as_ref().is_some_and(has_glue);
        if let Some(reason) =
            self.operation_count(i, Tag::Retain, hd_tir::ir::count::Part::Captures)
        {
            self.because(reason, |em| {
                em.retain_fields(at, &m, walk.as_ref(), &skip, colls);
            });
        }
        // A moved capture leaves its local empty, as a move read does.
        for (l, t) in caps
            .iter()
            .zip(&tys)
            .zip(&moved)
            .filter(|(_, mv)| **mv)
            .map(|((c, t), _)| (c.0, *t))
        {
            if glue {
                self.release_local(l)?;
                continue;
            }
            self.mark_drop_root(super::drop_flags::Root::Local(l), t, false);
            if self.needs_drop(t) {
                for x in self.local(u32::try_from(l).expect("locals"))? {
                    self.f.zero(x);
                }
            }
        }
        self.f.get(at);
        self.store_owned(i)
    }

    /// The prologue of closure `ci`'s code instance: each capture loaded
    /// from the environment, parameter `env`, into its local, as a value or
    /// a cell's address.
    pub(super) fn load_captures(&mut self, ci: usize, env: u32, key: Hash128) -> StageResult<()> {
        if let Some(l) = self_local(self.b, ci) {
            let ls = self.local(u32::try_from(l).expect("locals"))?;
            self.f.table_slot(Sym::Inst(key));
            self.f.set(ls[0]);
            self.f.get(env);
            self.f.set(ls[1]);
            if env == 0 {
                // The caller holds this environment throughout the call.
                self.borrowed.insert(l);
            }
        }
        let caps = captures(self.b, ci);
        if caps.is_empty() {
            return Ok(());
        }
        let (m, parts, _) = self.env_layout(ci, &caps)?;
        for ((l, shared), p) in caps.iter().zip(&parts) {
            // The environment outlives every call of the code, so a
            // reference to a copy points into it (a suspending code's
            // environment is read once, into its frame).
            if !shared && env == 0 {
                self.env_caps.insert(*l, (env, p.offset));
            }
            let ls = self.local(u32::try_from(*l).expect("locals"))?;
            for (leaf, x) in m.leaves[p.first..p.first + p.count].iter().zip(&ls) {
                self.f.get(env);
                self.f.op(leaf.load(0));
                self.f.set(*x);
            }
            if *shared && env == 0 {
                // A shared capture holds its cell's address. The
                // environment owns the cell; writes still replace only
                // its contents. Escaping references count themselves.
                self.borrowed.insert(*l);
            }
        }
        // The cells of the creator's own loans. A suspending closure's
        // frame holds a count of each (`susp::finish`), which its end or
        // abandonment releases.
        let cells: Vec<u32> = (parts[caps.len()..].iter())
            .map(|p| {
                let c = self.f.local(Val::I32);
                self.f.get(env);
                self.f.op(m.leaves[p.first].load(0));
                self.f.set(c);
                c
            })
            .collect();
        self.own_cells_load(ci, &cells, env != 0);
        Ok(())
    }

    /// `CallValue`: the environment, the arguments and the context of the
    /// callee type's row (`ctx.rs`), then an indirect call of the code, or
    /// a direct one of a closure collection resolved.
    pub(super) fn call_value(&mut self, i: u32, callee: u32, args: &[u32]) -> StageResult<()> {
        let ft = self.ty_of(callee);
        // A suspending value's code is its cold constructor, which
        // returns the frame that the call's `Suspend[T]` holds.
        let TyData::Fn {
            params, suspends, ..
        } = self.pool().get(strip(self.pool(), self.env(), ft))
        else {
            unreachable!("checking calls only a function value");
        };
        let mut ps = vec![Val::I32];
        let mut tys = Vec::new();
        for p in self.pool().list_items(params).to_vec() {
            let t = self.sub(p);
            ps.extend(self.vals(t)?);
            tys.push(t);
        }
        // ruled out: check_args/tuple_inputs_args already coerced each TIR
        // argument to its parameter type, so their flattened ABIs agree.
        self.comp(callee, 1)?;
        // A suspending value's code counts each argument into its frame.
        let copy = self.operation_count(i, Tag::Retain, hd_tir::ir::count::Part::Arguments);
        self.push_args(i, args, (!suspends).then_some(&tys[..]), 0, copy)?;
        let c = self.push_fn_ctx(i, ft)?;
        ps.push(Val::I32);
        let results = self.vals(self.ty(i))?;
        if let Some(&Target::KnownClosure(key)) = self.calls.get(&i) {
            self.f.call(Sym::Inst(key));
        } else {
            self.comp(callee, 0)?;
            self.f.call_indirect(ps, results);
        }
        let ty = match self.pool().get(strip(self.pool(), self.env(), ft)) {
            TyData::Fn { result, .. } if !suspends => self.sub(result),
            _ => self.ty(i),
        };
        self.call_results(i, ty)?;
        self.release_call_ctx(i, c)
    }
}
