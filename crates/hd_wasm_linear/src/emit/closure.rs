//! Closures and function values (`crates/hd_wasm_linear/DESIGN.md` §23.8, §23.19,
//! job 11). A function value is `(code, environment)`: the function-table
//! slot of its code instance and the address of a counted environment
//! that holds the captures, 0 when there are none. A code instance takes
//! the environment first, then its parameters, then the context its
//! caller passes, which holds the providers of its type's row (`ctx.rs`). A local that a closure shares (`Shared`, one
//! that is assigned after it is captured) lives in a cell, a counted
//! object holding its value, and every holder keeps the cell's address.

use crate::op::Op as I;
use hd_base::HashSet;
use hd_base::SliceSort as _;
use hd_base::{Hash128, HashMap, NodeIdx, StageResult};
use hd_mono::Target;
use hd_tir::ir::{
    Body, CaptureMode, NONE, PlaceRoot, ReadMode, Role, Tag, Topology, Word, local_flags, ref_word,
    slots, sub_flags,
};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{Mem, Part, Walk, env_mem, mem as mem_of, strip};
use crate::module::{Sym, Val};
use crate::unsupported;

/// Each local a closure shares, or that a reference refers to (`roots`,
/// `refs.rs`), mapped to the instruction of its first store outside any
/// closure that captures it: that store declares it and makes its cell
/// during RC lowering. A parameter moves into its cell
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

/// The locals of `roots` that live in a slot of the function's frame on
/// the local stack, not in a counted cell (`crates/hd_wasm_linear/DESIGN.md`
/// §23.38): those whose every `Ref` makes a local reference
/// (`ref_word::LOCAL`, so no counted `*T` is made of it) that is an
/// argument of one call, `Call`, `CallValue` or `CallDyn` (never a bang
/// call's `Await`), and whose `Drop`, if any, follows in the same block,
/// with nothing between that opens a block or ends, replaces or moves the
/// whole local. Such a reference ends before the local does, so nothing
/// needs the slot after the local's own release. A reference that takes
/// the local at its last access (`ref_word::TAKES`, the move pass's rule
/// 4, which deletes the local's `Drop`) needs its `Drop` in that block:
/// it releases the slot's value there ([`Em::slot_takes`]). A parameter,
/// a local a closure captures and one in a closure's body keep their
/// cells.
pub(super) fn slot_locals(b: &Body, roots: &[usize], topo: &Topology) -> HashSet<usize> {
    let mut out: HashSet<usize> = roots
        .iter()
        .copied()
        .filter(|&l| {
            b.local_flags[l] & local_flags::PARAM == 0 && !b.cap_local.iter().any(|c| c.idx() == l)
        })
        .collect();
    if out.is_empty() {
        return out;
    }
    let mut refs: HashMap<u32, usize> = HashMap::default();
    for i in 0..b.len() {
        let Some(l) = super::shape::ref_local(b, i).filter(|l| out.contains(l)) else {
            continue;
        };
        let w = b.data[i][1];
        if w & ref_word::LOCAL == 0 || topo.sub[i] != 0 {
            out.remove(&l);
        }
        refs.insert(u32::try_from(i).expect("instructions"), l);
    }
    // Each reference's calls and drops, and the references used otherwise.
    let mut calls: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut drops: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut other: HashSet<u32> = HashSet::default();
    for j in 0..b.len() {
        let j32 = u32::try_from(j).expect("instructions");
        slots(b, j, |_, v, w| {
            let Word::Value(role) = w else {
                return;
            };
            if !refs.contains_key(&v) {
                return;
            }
            match (b.tags[j], role) {
                (Tag::Call | Tag::CallValue | Tag::CallDyn, Role::Arg(_)) => {
                    calls.entry(v).or_default().push(j32);
                }
                (Tag::Drop, Role::Dropped) => drops.entry(v).or_default().push(j32),
                _ => {
                    other.insert(v);
                }
            }
        });
    }
    let ends_local = |m: usize, l: usize| {
        let whole = |word: u32| {
            b.decode_place(word).is_some_and(|p| {
                matches!(p.root, PlaceRoot::Local(x) if x.idx() == l) && p.proj.is_empty()
            })
        };
        match b.tags[m] {
            Tag::Drop | Tag::Store => whole(b.data[m][0]),
            Tag::Read => whole(b.data[m][0]) && b.data[m][1] == ReadMode::Move as u32,
            Tag::Ref => whole(b.data[m][0]) && b.data[m][1] & ref_word::TAKES != 0,
            _ => false,
        }
    };
    for (&r, &l) in &refs {
        if !out.contains(&l) {
            continue;
        }
        let fits = (|| {
            if other.contains(&r) {
                return false;
            }
            let ([j], d) = (calls.get(&r).map_or(&[][..], Vec::as_slice), drops.get(&r)) else {
                return false;
            };
            let takes = b.data[r as usize][1] & ref_word::TAKES != 0;
            let end = match d.map(Vec::as_slice) {
                None | Some([]) if !takes => *j,
                Some([d]) => *d,
                _ => return false,
            };
            let blk = topo.blk[r as usize];
            if blk == NONE || topo.blk[*j as usize] != blk || topo.blk[end as usize] != blk {
                return false;
            }
            let (from, call, to) = (
                topo.pos[r as usize],
                topo.pos[*j as usize],
                topo.pos[end as usize],
            );
            if !(from < call && call <= to) {
                return false;
            }
            let members = b.record(b.data[blk as usize][0]);
            members[from as usize + 1..=to as usize].iter().all(|&m| {
                let m = m as usize;
                topo.first[m] as usize == m && !ends_local(m, l)
            })
        })();
        if !fits {
            out.remove(&l);
        }
    }
    out
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

/// The immutable name introduced by this closure itself. Its declaration
/// and closure share a syntax node; other captures do not. Address-taking
/// keeps the independently addressable cell and its existing ownership.
pub(super) fn self_local(b: &Body, ci: usize) -> Option<usize> {
    if b.syn[ci] == NodeIdx::NONE {
        return None;
    }
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    b.cap_local[start..start + n].iter().find_map(|l| {
        let l = l.idx();
        (b.local_flags[l] & local_flags::SHORT != 0
            && b.local_syn[l] == b.syn[ci]
            && !(0..b.len()).any(|i| super::shape::ref_local(b, i) == Some(l)))
        .then_some(l)
    })
}

/// A nested closure copies an immutable recursive function's value,
/// rather than sharing the cell required by the checker's initialization.
fn recursive_binding(b: &Body, l: usize) -> bool {
    if b.local_flags[l] & local_flags::SHORT == 0 {
        return false;
    }
    (0..b.len()).any(|ci| {
        b.tags[ci] == Tag::Closure && b.syn[ci] == b.local_syn[l] && self_local(b, ci) == Some(l)
    })
}

/// The captures of closure instruction `ci`: each local, and whether it
/// is shared through a cell.
pub(super) fn captures(b: &Body, ci: usize) -> Vec<(usize, bool)> {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    let own = self_local(b, ci);
    (start..start + n)
        .filter(|&c| Some(b.cap_local[c].idx()) != own)
        .map(|c| {
            (
                b.cap_local[c].raw() as usize,
                b.cap_mode.get(c) == Some(&CaptureMode::Shared)
                    && !recursive_binding(b, b.cap_local[c].idx()),
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
        let at = self.raw_cell(l, &z)?;
        self.f.get(at);
        self.f.set(c);
        self.f.op(I::End);
        Ok(c)
    }

    /// Whether local `l`'s cell certainly exists at the instruction being
    /// emitted, so its local holds a nonzero address:
    ///
    /// - the prologue made or received it ([`Em::entry_cells`]): a
    ///   parameter moved into its cell, or a shared capture, whose holder
    ///   made the cell before passing it; or
    /// - known-null found the local holding its value there on every path
    ///   (`Body::full_cells`, rule 5 of `docs/compiler/tir-v2.md`,
    ///   "Known-null"): a store into it ran since its last `Drop`, move or
    ///   taking `Ref`, and every store makes the cell (its declaration, or
    ///   [`Em::cell_of`] for any other).
    ///
    /// Anything else keeps the test, and so does every use in a suspending
    /// body, whose resume reloads its locals from the frame and enters
    /// blocks in the middle (`susp.rs`).
    fn cell_made(&self, l: usize) -> bool {
        if self.susp.is_some() {
            return false;
        }
        if self.entry_cells.contains(&l) {
            return true;
        }
        let (Some(&j), Ok(l)) = (self.emitting.last(), u32::try_from(l)) else {
            return false;
        };
        self.b.full_cells.binary_search(&(j, l)).is_ok()
    }

    /// Gives each local [`slot_locals`] picks a slot in the function's
    /// frame on the local stack (§23.38): a zero count word, as a static
    /// object's, then the value in its cell's layout, so a count update
    /// through the slot's address does nothing.
    pub(super) fn plan_slots(&mut self, roots: &[usize]) -> StageResult<()> {
        let mut ls: Vec<usize> = slot_locals(self.b, roots, &self.topo).into_iter().collect();
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
        let t = self.cell_ty(l);
        if self.counted(t).is_none() {
            return Ok(());
        }
        let (m, _) = self.cell_mem(l)?;
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        for (leaf, x) in m.leaves.iter().zip(&ls) {
            self.f.get(c);
            self.f.op(leaf.load(0));
            self.f.set(*x);
        }
        self.release_locals(&ls, t)?;
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
    fn raw_cell(&mut self, l: usize, src: &Src) -> StageResult<u32> {
        if let Some(&off) = self.slots.get(&l)
            && let Some(base) = self.frame_base()
        {
            let (m, _) = self.cell_mem(l)?;
            let at = self.f.local(Val::I32);
            self.f.get(base);
            self.f.i32(0);
            self.f.op(I::I32Store(crate::module::mem(
                crate::module::HEAP,
                u64::from(off - crate::module::HEADER),
                2,
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
            return Ok(at);
        }
        if let Some(key) = self.static_cells {
            return self.static_cell(key, l, src);
        }
        let (m, walk) = self.cell_mem(l)?;
        let ty = self.cell_ty(l);
        let cyclic = self.cyclic_holding(&[ty]);
        let recursive = self.recursive_object(ty);
        let at = self.object(&m, walk.as_ref(), cyclic, recursive);
        #[cfg(feature = "checked-report")]
        if self.input.runtime.is_some_and(|r| r.checked.is_some())
            && cell_locals(self.b, &[]).contains_key(&l)
        {
            crate::rt::checked_kind(&mut self.f, self.m, 2);
        }
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, src)?;
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
        let t = self.cell_ty(l);
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
        if self.cell_converts(l) {
            let t = self.sub(self.b.local_ty[l]);
            return self.unerase(ls[0], self.cell_ty(l), t, false);
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
            let s = self.consumed(v)?;
            self.cell_src(l, s)?
        };
        if self.cells.get(&l) == Some(&i) {
            let c = self.local(u32::try_from(l).expect("locals"))?[0];
            // A slot is the same memory each time, so the last value in
            // it ends before the new one is stored.
            let slot = self.slots.contains_key(&l);
            if slot && self.counting {
                self.release_slot(l, c)?;
            }
            let at = self.raw_cell(l, &src)?;
            if self.counting && !slot {
                self.f.get(c);
                self.release_cell();
            }
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
        self.replace_late(d, self.cell_ty(l), src, &old)
    }

    /// One count less on the cell address on the stack.
    pub(super) fn release_cell(&mut self) {
        if self.input.runtime.is_some() {
            self.f.helper(crate::rt::Helper::ReleaseKnown);
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
        self.f.table_slot(Sym::Inst(key));
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
        let tys: Vec<Ty> = caps
            .iter()
            .map(|(l, _)| self.sub(self.b.local_ty[*l]))
            .collect();
        // The literal knows whether it captures a shared cell. Its
        // value type alone cannot describe that mutable reference edge.
        let cyclic = caps.iter().any(|(_, shared)| *shared) || self.cyclic_holding(&tys);
        let at = self.object(&m, walk.as_ref(), cyclic, false);
        #[cfg(feature = "checked-report")]
        crate::rt::checked_kind(&mut self.f, self.m, 1);
        for (s, p) in srcs.iter().zip(&parts) {
            self.store_part(at, &m, *p, s)?;
        }
        // A shared capture's slot holds its cell.
        let copied: Vec<Ty> = caps
            .iter()
            .zip(&tys)
            .map(|((_, shared), t)| if *shared { Ty::VOID } else { *t })
            .collect();
        let colls = self.part_colls(&copied, &parts);
        self.retain_fields(at, &m, walk.as_ref(), &colls);
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
        }
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
        self.push_args(args, (!suspends).then_some(&tys[..]), 0)?;
        let c = self.push_fn_ctx(ft)?;
        ps.push(Val::I32);
        let results = self.vals(self.ty(i))?;
        if let Some(&Target::KnownClosure(key)) = self.calls.get(&i) {
            self.f.call(Sym::Inst(key));
        } else {
            self.comp(callee, 0)?;
            self.f.call_indirect(ps, results);
        }
        let ty = self.ty(i);
        self.call_results(i, ty)?;
        self.release_ctx(c);
        Ok(())
    }
}
