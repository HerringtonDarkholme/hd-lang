//! Closures and function values (`crates/hd_wasm/DESIGN.md` §23.8, §23.19,
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
use hd_base::SliceSort as _;
use hd_base::{Hash128, StageResult};
use hd_mono::Target;
use hd_tir::ir::{Body, CaptureMode, NONE, Storage, Tag, box_word, local_flags};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{Mem, Part, env_mem, mem as mem_of, strip_newtype};
use crate::module::{Sym, Val};

/// The defining `Closure` of sub-body `sub`, not an instantiation of a
/// generic local function, at type arguments `args`: the instruction,
/// whether its function suspends, and its result type. A sub-body past
/// the root is a closure's (`crates/hd_tir/DESIGN.md`, "sub-body").
pub(super) fn closure_of(input: &crate::Input<'_>, b: &Body, ci: usize) -> (usize, bool, Ty) {
    let (pool, env) = (input.pool, input.env);
    let t = b.ty[ci];
    let TyData::Fn {
        suspends, result, ..
    } = pool.get(strip_newtype(pool, env, t))
    else {
        unreachable!("a closure's type is a function")
    };
    (ci, suspends, result)
}

/// The immutable name introduced by this closure itself. Its declaration
/// and closure share a syntax node; other captures do not. Address-taking
/// keeps the independently addressable cell and its existing ownership.
pub(super) fn self_local(recursive: &hd_tir::own::RecursiveBindings, ci: usize) -> Option<usize> {
    recursive.self_local(ci).map(hd_base::LocalId::idx)
}

/// The captures of closure instruction `ci`: each local, and whether it
/// is shared through a cell (`hd_tir::own::closure_captures`).
pub(super) fn captures(
    b: &Body,
    ci: usize,
    recursive: &hd_tir::own::RecursiveBindings,
) -> Vec<(usize, bool)> {
    hd_tir::own::closure_captures(b, ci, recursive).collect()
}

/// Whether closure `ci`'s body runs once (`sub_flags::ONCE`): it takes
/// its captures with their counts, and its environment gives them up. A
/// crossing closure shares no cell (`fn.crossing.mode`), so every capture
/// is a value of the environment's.
pub(super) fn takes_captures(b: &Body, ci: usize) -> bool {
    b.sub_flags[b.data[ci][0] as usize] & hd_tir::ir::sub_flags::ONCE != 0
}

/// Whether each capture [`captures`] lists for closure `ci` moves into
/// the environment (`CaptureMode::Move`, the move pass's rule 5): the
/// environment takes the local's count, so the closure retains nothing.
fn moved_captures(b: &Body, ci: usize, recursive: &hd_tir::own::RecursiveBindings) -> Vec<bool> {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    let own = self_local(recursive, ci);
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
    pub(super) fn cell_mem(&self, l: usize) -> StageResult<Mem> {
        let t = self.b.local_ty[l];
        let mut m = mem_of(self.pool(), self.env(), t)?;
        let flags = self.cell_layout(t).map_or(0, |cell| cell.paths.len());
        if flags != 0 {
            m.align = m.align.max(4);
            m.size = (m.size.next_multiple_of(4) + u32::try_from(flags).expect("flags") * 4)
                .next_multiple_of(m.align);
        }
        Ok(m)
    }

    /// The layout of closure `ci`'s environment, whose captures are
    /// `caps`: a cell's address for a shared capture, the value for any
    /// other.
    pub(super) fn env_layout(
        &self,
        _ci: usize,
        caps: &[(usize, bool)],
    ) -> StageResult<(Mem, Vec<Part>)> {
        let fields: Vec<(Ty, bool)> = caps
            .iter()
            .map(|(l, shared)| (self.b.local_ty[*l], *shared))
            .collect();
        env_mem(self.pool(), self.env(), &fields)
    }

    /// A new cell for local `l`, holding the values `src` pushes, which
    /// the cell's count of them already covers: a local holding its
    /// address.
    pub(super) fn new_cell(&mut self, l: usize, src: &Src) -> StageResult<u32> {
        let kind = if self.slots.contains_key(&l) {
            Storage::Slot
        } else if self.static_cells.contains(&l) {
            Storage::Static
        } else {
            Storage::Cell
        };
        let at = self.make_cell(l, kind)?;
        self.fill_cell(l, at, src, Some(true))?;
        Ok(at)
    }

    /// `Box`: local `l`'s cell, made where its scope starts
    /// (`own::storage::boxes`) in the kind the statement names; a
    /// parameter's in the prologue, which moves the parameter into it.
    /// The last round's box, or the value in a slot, ends here when its
    /// declaration overwrites it (`overwrites.rs`), as the declaration
    /// store did when it made the cell. With `box_word::ZERO`, a path may
    /// leave before the declaration fills it: a payload a release reads
    /// starts clear, flags and all, which the release skips.
    pub(super) fn box_statement(&mut self, i: u32) -> StageResult<()> {
        let [l, w] = self.b.data[i as usize];
        let l = l as usize;
        let Some(kind) = Storage::from_word(w) else {
            return Ok(());
        };
        if self.b.local_flags[l] & local_flags::PARAM != 0 {
            return Ok(());
        }
        // ruled out: TIR numbers its locals in u32 (`Box`'s data word).
        let c = self.local(u32::try_from(l).expect("locals"))?[0];
        let was = self.f.reason.replace(hd_tir::own::Reason::Overwritten);
        if self.store_releases(self.cells[&l]) {
            if kind == Storage::Slot {
                self.release_slot(l, c)?;
            } else {
                self.f.get(c);
                self.release_cell();
            }
        }
        self.f.reason = was;
        let at = self.make_cell(l, kind)?;
        // A payload nothing releases needs no clearing.
        if w & box_word::ZERO != 0 && self.needs_drop(self.b.local_ty[l]) {
            ledger!("cell/made-at-entry-unused");
            let zero = self.zero_src(l)?;
            self.fill_cell(l, at, &zero, Some(false))?;
        }
        self.f.get(at);
        self.f.set(c);
        Ok(())
    }

    /// Pushes the zero of each value of local `l`'s type, for a cell that
    /// a capture reaches before its declaration stores into it.
    pub(super) fn zero_src(&mut self, l: usize) -> StageResult<Src> {
        let m = self.cell_mem(l)?;
        let ls: Vec<u32> = m.leaves.iter().map(|x| self.f.local(x.val)).collect();
        Ok(Src::Locals(ls))
    }

    /// The local holding local `l`'s cell, which its `Box` made.
    pub(super) fn cell_of(&mut self, l: usize) -> StageResult<u32> {
        // ruled out: TIR numbers its locals in u32.
        Ok(self.local(u32::try_from(l).expect("locals"))?[0])
    }

    /// Gives each local of `slots` (its `Storage::Slot` rows) a slot in the function's
    /// frame on the local stack (§23.38): a zero count word, as a static
    /// object's, then the value in its cell's layout, so a count update
    /// through the slot's address does nothing.
    /// With `framed`, the slots are a suspending root's, in its frame
    /// object from `F_SAVED` on, before its save area (#1707).
    pub(super) fn plan_slots(&mut self, slots: &[usize], framed: bool) -> StageResult<()> {
        let mut ls = slots.to_vec();
        if ls.is_empty() {
            return Ok(());
        }
        ls.sort_stable();
        let mut size = if framed { crate::task::F_SAVED } else { 0 };
        for l in ls {
            let m = self.cell_mem(l)?;
            let at = size + crate::module::HEADER;
            size = (at + m.size).next_multiple_of(8);
            self.slots.insert(l, at);
        }
        if framed {
            self.susp_slots = size;
            return Ok(());
        }
        // The frame's local is made at its first use, after the
        // parameters' ([`Em::frame_base`]).
        self.frame = Some((NONE, size));
        Ok(())
    }

    /// The local holding the address of the function's frame of slots.
    pub(super) fn frame_base(&mut self) -> Option<u32> {
        if self.susp_slots != 0 {
            self.own_frame();
            return Some(super::susp::FRAME);
        }
        let (base, size) = self.frame?;
        if base != NONE {
            return Some(base);
        }
        let base = self.f.local(Val::I32);
        self.frame = Some((base, size));
        Some(base)
    }

    /// Releases the value in local `l`'s slot, whose address local `c`
    /// holds, or 0 before it is made, when its flag says it holds one: the
    /// slot's end, as the last count of a cell is the cell's.
    fn release_slot(&mut self, l: usize, c: u32) -> StageResult<()> {
        let t = self.b.local_ty[l];
        if !self.needs_drop(t) {
            return Ok(());
        }
        let m = self.cell_mem(l)?;
        // Its address says it is made and not yet released: an exit after
        // the slot's own end finds it cleared. Its whole flag guards this
        // in TIR (`hd_tir::own::elaborate`).
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        if !matches!(self.kind(t), crate::layout::Kind::Value) || self.cell_layout(t).is_none() {
            let ls = self.f.load_locals(c, &m.leaves);
            self.release_locals(&ls, t)?;
        } else if let Some(glue) = self.cell_glue(t, &m)? {
            // The slot's value with its drop slots, as a cell's glue
            // releases it.
            self.f.get(c);
            self.f.call(glue);
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

    /// A new cell of kind `kind` for local `l`, in a fresh local: a heap
    /// cell, its frame slot, or its static data. Its payload is unwritten.
    fn make_cell(&mut self, l: usize, kind: Storage) -> StageResult<u32> {
        let at = self.f.local(Val::I32);
        if kind == Storage::Slot
            && let Some(&off) = self.slots.get(&l)
            && let Some(base) = self.frame_base()
        {
            ledger!(if self.susp_slots != 0 => "cell-made/susp-frame-slot" else "cell-made/stack-slot");
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
            return Ok(at);
        }
        if kind == Storage::Static {
            ledger!("cell-made/static");
            self.static_cell(l, at)?;
            return Ok(at);
        }
        ledger!("cell-made/heap");
        let m = self.cell_mem(l)?;
        let ty = self.b.local_ty[l];
        let cyclic = self.cyclic_holding(&[ty]);
        let recursive = self.recursive_object(ty);
        let glue = self.cell_glue(ty, &m)?;
        let obj = self.new_object(Some(m.size), glue, cyclic, recursive);
        #[cfg(feature = "checked-report")]
        if self.input.runtime.report().is_some() && self.cells.contains_key(&l) {
            crate::rt::checked_kind(&mut self.f, self.m, 2);
        }
        self.f.get(obj);
        self.f.set(at);
        Ok(at)
    }

    /// Stores `src`, in local `l`'s cell layout, into the cell at `at`; its
    /// flag slots start `init`, or are left to the `FlagSet` that follows
    /// a store.
    fn fill_cell(&mut self, l: usize, at: u32, src: &Src, init: Option<bool>) -> StageResult<()> {
        let m = self.cell_mem(l)?;
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, src)?;
        self.init_cell_drop(at, self.b.local_ty[l], init);
        Ok(())
    }

    /// Local `l`'s cell in static data (`Storage::Static`): a static
    /// object, a zero count word and then the value, which lives as long
    /// as the program, so no count changes it and every reference to it is
    /// one address no count reaches (`annot.handle.constants`). A checked
    /// run's teardown releases the value, as the once-cell's.
    fn static_cell(&mut self, l: usize, at: u32) -> StageResult<()> {
        let m = self.cell_mem(l)?;
        let header = crate::module::HEADER;
        let base = self.m.static_slot(header + m.size, 8);
        let addr = base + header;
        let mut bytes = self.key.0.to_le_bytes().to_vec();
        bytes.extend_from_slice(&u32::try_from(l).expect("locals").to_le_bytes());
        let t = self.b.local_ty[l];
        self.note_stored(
            hd_base::hash128(&bytes),
            t,
            Some((crate::module::Address::Static(addr), m.leaves.clone())),
        )?;
        self.f.static_addr(addr);
        self.f.set(at);
        Ok(())
    }

    /// Fresh locals holding local `l`'s value, read from its cell with no
    /// count of its own: a view that lives as long as the cell's value.
    pub(super) fn cell_read(&mut self, l: usize) -> StageResult<Vec<u32>> {
        let m = self.cell_mem(l)?;
        let c = self.cell_of(l)?;
        let ls = self.f.load_locals(c, &m.leaves);
        Ok(ls)
    }

    /// A plain read of a cell's local, `Read [$l]`: its values, a copy with its own count.
    pub(super) fn cell_get(&mut self, i: u32, l: usize) -> StageResult<()> {
        for x in self.cell_read(l)? {
            self.f.get(x);
        }
        self.store(i)
    }

    /// Moves local `l`'s value out of its cell into instruction `i`. The
    /// cell stays, as Swift's box after a `load [take]`: a heap cell's drop
    /// or exit row ends it, a slot keeps its address and clears its flag
    /// ([`Em::slot_flag`]), and a later store fills it again. Move proves no use
    /// reads the value before such a store; shared captures are not
    /// candidates.
    pub(super) fn cell_take(&mut self, i: u32, l: usize) -> StageResult<()> {
        let (m, c) = (self.cell_mem(l)?, self.cell_of(l)?);
        // Its `Empty` clears the payload (`Em::empty_place`).
        for x in self.f.load_locals(c, &m.leaves) {
            self.f.get(x);
        }
        self.store(i)
    }

    /// A `Store` into the whole of a cell's local. Its declaration fills
    /// the cell its `Box` made; any other store writes the cell, counting
    /// the new value before it releases the old. `NONE` stores zeros: a recursive local function's
    /// name, declared before its closure exists.
    pub(super) fn cell_set(&mut self, i: u32, l: usize, v: u32) -> StageResult<()> {
        // The one-use initializer plan already wrote this destination;
        // its Box still controls the slot's lifetime.
        if self.placed.contains_key(&v) && self.written.contains(&v) {
            let at = self.cell_of(l)?;
            let init = (!self.flagged(l)).then_some(true);
            self.init_cell_drop(at, self.b.local_ty[l], init);
            return Ok(());
        }
        // The value in the cell's layout, with the count the cell takes.
        let src = if v == NONE {
            self.zero_src(l)?
        } else if self.loan_locals.contains_key(&l) {
            // An immutable frame binding stores a borrowed descriptor;
            // the ownership proof keeps its source live through every use.
            Src::Ref(v)
        } else {
            self.consumed(v)?
        };
        // The declaration fills the cell its `Box` made.
        if self.cells.get(&l) == Some(&i) {
            let c = self.local(u32::try_from(l).expect("locals"))?[0];
            // A store the TIR flags track sets the slots after it.
            let flagged = self.flagged(l);
            return self.fill_cell(l, c, &src, (v == NONE || !flagged).then_some(v != NONE));
        }
        self.cell_put(l, &src)
    }

    /// Replaces the value in local `l`'s cell with the values `src`
    /// pushes, in the cell's layout and with the count the cell takes,
    /// then releases the old one.
    pub(super) fn cell_put(&mut self, l: usize, src: &Src) -> StageResult<()> {
        let at = self.cell_of(l)?;
        let m = self.cell_mem(l)?;
        let d = Dst::Mem(at, 0, &m.leaves);
        let old = self.old_locals(d);
        self.replace_late(d, self.b.local_ty[l], src, &old)
    }

    /// One count less on the cell address on the stack.
    pub(super) fn release_cell(&mut self) {
        self.f.helper(crate::rt::Helper::ReleaseKnown);
    }

    /// Moves parameter `l`, whose values are in `ls`, into a new cell, or
    /// its slot (`slot_locals`): the local's locals become its address.
    pub(super) fn param_cell(&mut self, l: usize, ls: &[u32]) -> StageResult<()> {
        // A parameter kept by address is copied from where it is.
        let t = self.b.local_ty[l];
        let src = match ls {
            &[at] if crate::layout::by_address(self.pool(), self.env(), t)? => Src::Ref(at),
            _ => Src::Locals(ls.to_vec()),
        };
        let at = self.new_cell(l, &src)?;
        self.locals.insert(l, vec![at]);
        self.param_vals.insert(l, ls.to_vec());
        Ok(())
    }

    /// `Closure`: the code instance's table slot and a new environment
    /// holding the captures, each counted once more, or 0 for none.
    pub(super) fn closure(&mut self, i: u32) -> StageResult<()> {
        let Some(Target::Closure(key)) = self.calls.get(&i).cloned() else {
            unreachable!("collection selects every emitted closure's code instance");
        };
        let caps = captures(self.b, i as usize, &self.recursive_bindings);
        ledger!(if caps.iter().any(|c| c.1) => "closure/with-shared-capture");
        // A lent parameter holds no count to move: the environment
        // counts its own copy.
        let moved: Vec<bool> = moved_captures(self.b, i as usize, &self.recursive_bindings)
            .into_iter()
            .zip(&caps)
            .map(|(mv, (l, _))| mv && !self.borrowed.contains(l))
            .collect();
        self.f.table_slot(Sym::Inst(key));
        if caps.is_empty() {
            self.f.i32(0);
            return self.store(i);
        }
        let (m, parts) = self.env_layout(i as usize, &caps)?;
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
        let tys: Vec<Ty> = caps.iter().map(|(l, _)| self.b.local_ty[*l]).collect();
        // The literal knows whether it captures a shared cell. Its
        // value type alone cannot describe that mutable reference edge.
        let cyclic = caps.iter().any(|(_, shared)| *shared) || self.cyclic_holding(&tys);
        let fields: Vec<Option<Ty>> = caps
            .iter()
            .zip(&tys)
            .map(|((_, shared), t)| (!*shared).then_some(*t))
            .collect();
        let counted = self.payload_parts(&fields, &parts)?;
        let at = self.object_parts(&m, counted.clone(), cyclic, false);
        #[cfg(feature = "checked-report")]
        crate::rt::checked_kind(&mut self.f, self.m, 1);
        for (s, p) in srcs.iter().zip(&parts) {
            self.store_part(at, &m, *p, s)?;
        }
        // A shared capture's slot holds its cell; a moved one, the
        // local's own count.
        // A moved part takes no count: the move put its count there.
        let moved_at: Vec<u32> = parts
            .iter()
            .zip(&moved)
            .filter(|(_, mv)| **mv)
            .map(|(p, _)| p.first_leaf())
            .collect();
        let retained: Vec<_> = counted
            .into_iter()
            .filter(|(first, _)| !moved_at.contains(first))
            .collect();
        if let Some(reason) =
            self.operation_count(i, Tag::Retain, hd_tir::ir::count::Part::Captures)
        {
            self.because(reason, |em| {
                em.retain_fields(at, &m, retained.clone());
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
            if self.needs_drop(t) {
                for x in self.local(u32::try_from(l).expect("locals"))? {
                    self.f.zero(x);
                }
            }
        }
        self.f.get(at);
        self.store(i)
    }

    /// The prologue of closure `ci`'s code instance: each capture loaded
    /// from the environment, parameter `env`, into its local, as a value or
    /// a cell's address.
    pub(super) fn load_captures(&mut self, ci: usize, env: u32, key: Hash128) -> StageResult<()> {
        if let Some(l) = self_local(&self.recursive_bindings, ci) {
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
        let caps = captures(self.b, ci, &self.recursive_bindings);
        if caps.is_empty() {
            return Ok(());
        }
        let (m, parts) = self.env_layout(ci, &caps)?;
        for ((l, shared), p) in caps.iter().zip(&parts) {
            // The environment outlives every call of the code, so a
            // reference to a copy points into it (a suspending code's
            // environment is read once, into its frame).
            if !shared && env == 0 {
                self.env_caps.insert(*l, (env, p.offset));
            }
            let ls = self.local(u32::try_from(*l).expect("locals"))?;
            self.f
                .load_into(env, &m.leaves[p.first..p.first + p.count], &ls);
            if *shared && env == 0 {
                // A shared capture holds its cell's address. The
                // environment owns the cell; writes still replace only
                // its contents. Escaping references count themselves.
                self.borrowed.insert(*l);
            }
        }
        if env == 0 && takes_captures(self.b, ci) {
            self.give_up_captures(env);
        }
        Ok(())
    }

    /// After a body that runs once took the captures of its environment
    /// in local `env` (`sub_flags::ONCE`): the environment's descriptor
    /// loses its release glue, so its last release frees its block alone,
    /// whoever holds it then. One load and one store.
    pub(super) fn give_up_captures(&mut self, env: u32) {
        let desc = self.f.local(Val::I32);
        self.f.get(env);
        self.f.u32(4);
        self.f.op(I::I32Sub);
        self.f.op(I::LocalTee(desc));
        self.f.get(desc);
        self.f
            .op(I::I32Load(crate::module::mem(crate::module::HEAP, 0, 2)));
        self.f.u32(!crate::module::GLUE);
        self.f.op(I::I32And);
        self.f
            .op(I::I32Store(crate::module::mem(crate::module::HEAP, 0, 2)));
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
        } = self.pool().get(strip_newtype(self.pool(), self.env(), ft))
        else {
            unreachable!("checking calls only a function value");
        };
        let mut ps = vec![Val::I32];
        let mut tys = Vec::new();
        for p in self.pool().list_items(params).to_vec() {
            let t = p;
            ps.extend(self.vals(t)?);
            tys.push(t);
        }
        // ruled out: check_args/tuple_inputs_args already coerced each TIR
        // argument to its parameter type, so their flattened ABIs agree.
        self.comp(callee, 1)?;
        // Copy inputs and references use the common borrowed convention;
        // noncopy values retain their ownership transfer.
        let copy = self.operation_count(i, Tag::Retain, hd_tir::ir::count::Part::Arguments);
        self.push_args(i, args, Some(&tys), None, copy, None)?;
        let c = self.push_fn_ctx(i, ft)?;
        ps.push(Val::I32);
        let results = self.vals(self.ty(i))?;
        if let Some(&Target::KnownClosure(key)) = self.calls.get(&i) {
            self.f.call(Sym::Inst(key));
        } else {
            self.comp(callee, 0)?;
            self.f.call_indirect(ps, results);
        }
        let ty = match self.pool().get(strip_newtype(self.pool(), self.env(), ft)) {
            TyData::Fn { result, .. } if !suspends => result,
            _ => self.ty(i),
        };
        self.call_results(i, ty)?;
        // The explicit ownership packet takes native components, while the
        // original value remains the carrier of lent thin-reference counts.
        // Keep its original discriminants: conditional borrowed leaves must
        // be released according to the value that was actually passed.
        if self
            .operation_count(i, Tag::Release, hd_tir::ir::count::Part::Arguments)
            .is_some()
        {
            for arg in args {
                if let Some(&(flag, _)) = self.input_transferred.get(arg) {
                    self.f.i32(1);
                    self.f.set(flag);
                }
            }
        }
        self.release_call_ctx(i, c);
        Ok(())
    }
}
