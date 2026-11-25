//! Scalar replacement of aggregate locals after inlining (#2365, #2366),
//! on TIR, before the ownership passes. A local lives in memory only when
//! a `Ref` takes its storage or a closure shares it
//! (`hd_tir::own::storage`); every other local is Wasm locals, one per
//! scalar leaf. Inlining a `&mut self` method or an iterator's `next`
//! leaves such a `Ref` behind: the copy binds its reference parameter to
//! `Ref [$x]` and reaches the fields through `[*$self .f]`. This pass
//! removes those addresses:
//!
//! 1. **Forwarding.** A local reference bound once, to `Ref [$x .path]`
//!    (fields only) or to a read of another such binding, names the same
//!    storage at every use: the checker's definite initialization puts
//!    the one binding before every read, and a field path of a local has
//!    one address for the local's life. So `[*$self rest]` becomes
//!    `[$x .path rest]`, and so does a place rooted at such a `Ref`, read
//!    or same-value coercion. A reference-typed field of `x` is a plain
//!    address value here and blocks nothing. Kept through the reference:
//!    a counted reference (its count owns a cell), a local a closure
//!    captures (its cell's loans are checked at run time), a read of a
//!    value no instance copies (through a reference it borrows), a `Ref`
//!    of a list element (a local list's would ask for a unique buffer),
//!    and a write into a payload (written in memory).
//! 2. **Spills around cold calls.** A `Ref` still passed whole to a call
//!    (`reserve_for` behind a capacity test) is replaced by a temporary:
//!    the argument becomes `Ref [$tmp]`, with `$tmp = $x .path` before the
//!    call and `$x .path = $tmp` after it. Only a call under a condition
//!    inside its innermost loop qualifies, so the copies stay off the hot
//!    path, and straight-line code keeps its one address. Only a call
//!    whose other arguments are primitives and whose result is `void` or
//!    `bool` qualifies, so no place the caller can reach may keep the
//!    address past the call (a local reference parameter outlives nothing
//!    else, `ref.local`). A wider result could be the address itself:
//!    `address_of` returns it as a `usize`, and a store through it after
//!    the call would land in the copy. A
//!    local is spilled at every remaining address or at none: if one
//!    address must stay, copies would only add work. The pipeline's
//!    `cold_spill` knob turns spills off where the loop's back edge
//!    already holds a call (wasmtime's epoch check): there the spilled
//!    fields and the copy's frame are all live across that call, past
//!    the callee-saved registers (`hd_opt::passes::OPTIMIZED_WASMTIME`).
//! 3. **Cleanup.** A binding no read uses any more is deleted with the
//!    `Ref`s and reads that fed only it, so the local has no address.
//!
//! Cost: linear in the body's instruction and place words: one scan for
//! stores and addresses, one pass that rewrites each place and resolves
//! each binding, one use count, a per-block memo for coldness, and a
//! worklist that deletes each instruction at most once; the splice is
//! one renumbering ([`hd_tir::own::edit_batch`]).

use hd_base::LocalId;
use hd_tir::ir::{
    Body, Loc, NONE, Op, Place, PlaceRoot, Proj, ReadMode, Ref, Role, Tag, Topology, Word,
    local_flags, place, ref_word, slots, u32_of,
};
use hd_tir::own::BatchInsert;
use hd_types::{InternPool, Ty, TyData};

#[cfg(test)]
thread_local! {
    /// Work units of [`run`] on this thread, for the counted test.
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn charge(n: usize) {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + n));
    #[cfg(not(test))]
    let _ = n;
}

/// A place that names fixed storage of a local: its root and its field
/// steps.
#[derive(Clone)]
struct Fixed {
    local: LocalId,
    proj: Vec<Proj>,
}

/// The local storage `Ref` `i` takes, when its steps are fields only and
/// it makes a local reference. A counted one (`box(x)`, `*mut T`) holds a
/// count of a cell, which its moves and drops decide: it stays.
fn fixed_ref(b: &Body, pool: &InternPool, i: usize) -> Option<Fixed> {
    if b.tags[i] != Tag::Ref || !matches!(pool.get(b.ty[i]), TyData::Ref { counted: false, .. }) {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    let PlaceRoot::Local(local) = p.root else {
        return None;
    };
    p.proj
        .iter()
        .all(|s| matches!(s, Proj::Field(_)))
        .then_some(Fixed {
            local,
            proj: p.proj,
        })
}

fn place_word(b: &mut Body, local: LocalId, proj: Vec<Proj>) -> u32 {
    let p = Place {
        root: PlaceRoot::Local(local),
        proj,
    };
    match p.words() {
        Some(w) => b.push_record(&w),
        None => place::BARE_LOCAL | local.raw(),
    }
}

fn bare_local(word: u32) -> Option<LocalId> {
    (word & place::BARE_LOCAL != 0).then(|| LocalId::from_raw(word & !place::BARE_LOCAL))
}

fn value(word: u32, n: usize) -> Option<usize> {
    (word & Ref::CONST_BIT == 0 && (word as usize) < n).then_some(word as usize)
}

/// Per instruction: whether it is a whole-argument use of the value by a
/// `Call`, as (call, `extra` word).
type ArgUse = (u32, u32);

/// Runs the pass over `b`; whether it changed anything. `spill` allows
/// step 2's spills (the pipeline's `cold_spill` knob).
///
/// # Errors
/// A malformed body the splice cannot renumber.
pub fn run(
    b: &mut Body,
    pool: &InternPool,
    copies: &dyn Fn(Ty) -> bool,
    spill: bool,
) -> Result<bool, String> {
    let n = b.len();
    let locals = b.local_ty.len();
    charge(n + locals);
    // Each local's whole stores, and whether a `Ref` takes it or a
    // closure captures it.
    let mut stores = vec![0u32; locals];
    let mut pinned = vec![false; locals];
    for c in &b.cap_local {
        pinned[c.idx()] = true;
    }
    // A local a closure captures lives in a cell, whose loans the run
    // checks (`ref.excl.alias.shared-variable`): a reference to it keeps
    // its loan, and nothing would split it.
    let mut shared = pinned.clone();
    for &(_, c) in &b.inlined_shared {
        shared[b.cap_local[c as usize].idx()] = true;
    }
    for i in 0..n {
        charge(1);
        if b.tags[i] == Tag::Store
            && let Some(l) = bare_local(b.data[i][0])
        {
            stores[l.idx()] += 1;
        } else if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            pinned[l.idx()] = true;
        }
    }
    let topo = Topology::new(b);
    // Each value under the coercions that keep it (`&mut T` weakened to
    // `&T`): operands precede their users, so one forward pass.
    let mut base: Vec<usize> = (0..n).collect();
    for v in 0..n {
        if b.same_value_coercion(v)
            && let Some(x) = value(b.data[v][0], n)
        {
            base[v] = base[x];
        }
    }
    // 1. In evaluation order: forward each place, then resolve the
    //    binding a `Store` makes. A binding's reads follow its one store.
    let mut alias: Vec<Option<(Fixed, u32)>> = vec![None; locals];
    let mut store_at = vec![NONE; locals];
    let mut changed = false;
    for i in 0..n {
        let (oa, ob) = b.tags[i].operands();
        for (x, op) in [oa, ob].into_iter().enumerate() {
            if op != Op::Place || b.data[i][x] & place::BARE_LOCAL != 0 {
                continue;
            }
            let Some(p) = b.decode_place(b.data[i][x]) else {
                continue;
            };
            charge(1 + p.proj.len());
            if p.proj.first() != Some(&Proj::Deref) {
                continue;
            }
            let target = match p.root {
                PlaceRoot::Local(l) => alias[l.idx()].as_ref().map(|(f, _)| f.clone()),
                PlaceRoot::Value(r) => value(r.0, n).map(|v| base[v]).and_then(|v| {
                    fixed_ref(b, pool, v)
                        .filter(|_| topo.sub[v] == topo.sub[i])
                        .or_else(|| {
                            let l = b.local_read(v)?;
                            alias[l.idx()].as_ref().map(|(f, _)| f.clone())
                        })
                }),
                _ => None,
            };
            let Some(f) = target.filter(|f| !shared[f.local.idx()]) else {
                continue;
            };
            // A write into a payload needs its enum in memory
            // (`hd_wasm` `payload_ref`): such a place keeps its reference.
            let writes = matches!(b.tags[i], Tag::Store | Tag::Ref);
            if writes && p.proj.iter().any(|s| matches!(s, Proj::Variant(_))) {
                continue;
            }
            // Through a reference a read of a value no instance copies
            // borrows it; out of the local itself the passes would copy it.
            if b.tags[i] == Tag::Read && !copies(b.ty[i]) {
                continue;
            }
            // A reference into a local list's element asks for its buffer
            // unique (insertion's `IsUnique`); through a shared reference
            // it borrows. Such a `Ref` keeps the reference it goes through.
            if b.tags[i] == Tag::Ref && p.proj.iter().any(|s| matches!(s, Proj::Elem(_))) {
                continue;
            }
            let mut proj = f.proj;
            proj.extend_from_slice(&p.proj[1..]);
            b.data[i][x] = place_word(b, f.local, proj);
            changed = true;
        }
        if b.tags[i] != Tag::Store {
            continue;
        }
        let Some(l) = bare_local(b.data[i][0]).map(LocalId::idx) else {
            continue;
        };
        if stores[l] != 1 || pinned[l] || b.local_flags[l] & local_flags::PARAM != 0 {
            continue;
        }
        store_at[l] = u32_of(i);
        let Some(v) = value(b.data[i][1], n).map(|v| base[v]) else {
            continue;
        };
        let found = match fixed_ref(b, pool, v) {
            Some(f) => Some((f, u32_of(v))),
            None => b.local_read(v).and_then(|src| alias[src.idx()].clone()),
        };
        alias[l] =
            found.filter(|(f, r)| topo.sub[*r as usize] == topo.sub[i] && !shared[f.local.idx()]);
    }
    if alias.iter().all(Option::is_none) && !changed {
        return Ok(false);
    }
    // 2. Uses after forwarding: each value's, each binding's reads, and
    //    each value's whole-argument use by a call.
    let mut uses = vec![0u32; n];
    // Uses by anything but a coercion that keeps the value.
    let mut direct = vec![0u32; n];
    let mut reads = vec![0u32; locals];
    let mut arg_use: Vec<ArgUse> = vec![(NONE, NONE); n];
    for i in 0..n {
        if let Some(l) = b.local_read(i) {
            reads[l.idx()] += 1;
        }
        // A place forwarding left on a binding (`[*$self [%i]]`) reads it.
        let (oa, ob) = b.tags[i].operands();
        for (x, op) in [oa, ob].into_iter().enumerate() {
            let word = b.data[i][x];
            if op == Op::Place
                && word & place::BARE_LOCAL == 0
                && let Some(l) = b.view(word).local()
            {
                reads[l as usize] += 1;
            }
        }
        let call = b.tags[i] == Tag::Call;
        let coercion = b.same_value_coercion(i);
        slots(b, i, |at, w, word| {
            charge(1);
            if !matches!(word, Word::Value(_) | Word::Provider | Word::ProviderRef) {
                return;
            }
            let Some(v) = value(w, n) else {
                return;
            };
            uses[v] += 1;
            direct[v] += u32::from(!coercion);
            if let (true, Word::Value(Role::Arg(_)), Loc::Extra(e)) = (call, word, at) {
                arg_use[v] = (u32_of(i), e);
            }
        });
    }
    // 3. Spills.
    let spills = if spill {
        spills(b, pool, &topo, &alias, &base, &direct, &arg_use)
    } else {
        Vec::new()
    };
    let mut inserts = Vec::new();
    let mut values: Vec<u32> = Vec::new();
    for s in spills {
        let (blk, pos, syn) = (topo.blk[s.call], topo.pos[s.call], b.syn[s.call]);
        let copy = LocalId::from_raw(u32_of(b.local_ty.len()));
        b.local_ty.push(s.inner);
        b.local_name.push(b.local_name[s.at.local.idx()]);
        b.local_syn.push(b.local_syn[s.at.local.idx()]);
        b.local_flags
            .push(local_flags::READ | local_flags::ASSIGNED | local_flags::MUTATED);
        let src = place_word(b, s.at.local, s.at.proj);
        let held = place::BARE_LOCAL | copy.raw();
        let first = u32_of(n + inserts.len());
        let mode = ReadMode::Use as u32;
        for (tag, data, ty, at) in [
            (Tag::Read, [src, mode], s.inner, pos),
            (Tag::Store, [held, first], Ty::VOID, pos),
            (Tag::Ref, [held, s.word], s.ty, pos),
            (Tag::Read, [held, mode], s.inner, pos + 1),
            (Tag::Store, [src, first + 3], Ty::VOID, pos + 1),
        ] {
            inserts.push(BatchInsert {
                block: blk,
                pos: at,
                tag,
                data,
                ty,
                syn,
            });
        }
        b.extra[s.extra as usize] = first + 2;
        uses[s.value] -= 1;
        values.push(u32_of(s.value));
        changed = true;
    }
    // 4. Delete what only fed dead bindings: bindings no read uses, and
    //    the `Ref`s and binding reads forwarding left unused.
    let mut work: Vec<usize> = (0..locals)
        .filter(|&l| alias[l].is_some() && reads[l] == 0)
        .collect();
    values.extend(
        (0..n)
            .filter(|&v| uses[v] == 0 && deletable(b, pool, v, &alias))
            .map(u32_of),
    );
    let mut removed = Vec::new();
    let mut gone = vec![false; n];
    loop {
        charge(1);
        if let Some(l) = work.pop() {
            let s = store_at[l] as usize;
            if gone[s] {
                continue;
            }
            gone[s] = true;
            removed.push(u32_of(s));
            if let Some(v) = value(b.data[s][1], n) {
                uses[v] -= 1;
                values.push(u32_of(v));
            }
        } else if let Some(v) = values.pop() {
            let v = v as usize;
            if uses[v] != 0 || gone[v] || !deletable(b, pool, v, &alias) {
                continue;
            }
            gone[v] = true;
            removed.push(u32_of(v));
            if b.same_value_coercion(v)
                && let Some(x) = value(b.data[v][0], n)
            {
                uses[x] -= 1;
                values.push(u32_of(x));
            } else if let Some(l) = b.local_read(v) {
                reads[l.idx()] -= 1;
                if reads[l.idx()] == 0 {
                    work.push(l.idx());
                }
            }
        } else {
            break;
        }
    }
    if inserts.is_empty() && removed.is_empty() {
        return Ok(changed);
    }
    hd_tir::own::edit_batch(b, &topo, &inserts, &removed)?;
    Ok(true)
}

/// Whether value `v` is pure and may go once unused: a `Ref` of fixed
/// local storage, or a read of a binding.
fn deletable(b: &Body, pool: &InternPool, v: usize, alias: &[Option<(Fixed, u32)>]) -> bool {
    fixed_ref(b, pool, v).is_some()
        || b.local_read(v).is_some_and(|l| alias[l.idx()].is_some())
        || (b.same_value_coercion(v)
            && matches!(pool.get(b.ty[v]), TyData::Ref { counted: false, .. }))
}

/// One spill: the call, its argument word in `extra`, the argument value,
/// the storage, the `Ref`'s word, its type and its referent type.
struct Spill {
    call: usize,
    extra: u32,
    value: usize,
    at: Fixed,
    word: u32,
    ty: Ty,
    inner: Ty,
}

/// The spills of step 3, all or none per referent local.
fn spills(
    b: &Body,
    pool: &InternPool,
    topo: &Topology,
    alias: &[Option<(Fixed, u32)>],
    base: &[usize],
    uses: &[u32],
    arg_use: &[ArgUse],
) -> Vec<Spill> {
    let n = b.len();
    let locals = b.local_ty.len();
    // Per referent local: whether some address of it must stay.
    let mut kept = vec![false; locals];
    for c in &b.cap_local {
        kept[c.idx()] = true;
    }
    // The `Ref`s that only make one binding: their binding's reads speak
    // for them.
    let mut binds = vec![false; n];
    for (_, r) in alias.iter().flatten() {
        binds[*r as usize] = true;
    }
    let mut out = Vec::new();
    let mut cold = Cold::new(b, topo);
    for v in 0..n {
        charge(1);
        if uses[v] == 0 {
            continue;
        }
        let at = base[v];
        let target = if at != v {
            // A kept coercion of an address carries it as its base does.
            let found = fixed_ref(b, pool, at).map(|f| (f, at)).or_else(|| {
                let l = b.local_read(at)?;
                alias[l.idx()]
                    .as_ref()
                    .map(|(f, r)| (f.clone(), *r as usize))
            });
            match found {
                Some(t) => t,
                None => continue,
            }
        } else if b.tags[v] == Tag::Ref {
            let Some(l) = b.local_in_place(b.data[v][0]) else {
                continue;
            };
            match fixed_ref(b, pool, v) {
                None => {
                    kept[l.idx()] = true;
                    continue;
                }
                Some(_) if binds[v] && uses[v] == 1 => continue,
                Some(f) => (f, v),
            }
        } else if let Some(l) = b.local_read(v) {
            match &alias[l.idx()] {
                Some((f, r)) => (f.clone(), *r as usize),
                None => continue,
            }
        } else {
            continue;
        };
        let (f, r) = target;
        let (call, extra) = arg_use[v];
        match (uses[v] == 1 && call != NONE)
            .then(|| spill_type(b, pool, v, call as usize, &mut cold))
            .flatten()
        {
            Some(inner) => out.push(Spill {
                call: call as usize,
                extra,
                value: v,
                at: f,
                // The argument's own kind: a weakened `&mut` lends `&`.
                word: (b.data[r][1] & !ref_word::MUT)
                    | if matches!(pool.get(b.ty[v]), TyData::Ref { mutable: true, .. }) {
                        ref_word::MUT
                    } else {
                        0
                    },
                ty: b.ty[v],
                inner,
            }),
            None => kept[f.local.idx()] = true,
        }
    }
    out.retain(|s| !kept[s.at.local.idx()]);
    out
}

/// The referent type, when call `c`, which takes value `v` whole as a
/// local reference argument, may take a spilled copy instead: a cold
/// direct call that returns, whose other arguments are primitives and
/// whose result is too narrow to hold an address (`void` or `bool`).
fn spill_type(b: &Body, pool: &InternPool, v: usize, c: usize, cold: &mut Cold) -> Option<Ty> {
    let TyData::Ref {
        inner,
        counted: false,
        ..
    } = pool.get(b.ty[v])
    else {
        return None;
    };
    let prim = |t: Ty| t == Ty::VOID || matches!(pool.get(t), TyData::Prim(_));
    let others = b.call_args(c).iter().all(|&a| {
        charge(1);
        a == u32_of(v) || a & Ref::CONST_BIT != 0 || prim(b.ty[a as usize])
    });
    // `address_of` (`mem_ref_address`) gives the referent's address as a
    // `usize`, which the caller may write through after the call.
    let addressless = b.ty[c] == Ty::VOID || b.ty[c] == Ty::BOOL;
    (others && addressless && cold.is_cold(c)).then_some(inner)
}

/// Whether an instruction runs under a condition inside its innermost
/// loop: memoized per block, so each block is judged once.
struct Cold<'a> {
    b: &'a Body,
    topo: &'a Topology,
    /// Per block: whether it is cold, once judged.
    memo: Vec<Option<bool>>,
}

impl<'a> Cold<'a> {
    fn new(b: &'a Body, topo: &'a Topology) -> Self {
        Cold {
            b,
            topo,
            memo: vec![None; b.len()],
        }
    }

    fn is_cold(&mut self, i: usize) -> bool {
        let blk = self.topo.blk[i];
        blk != NONE && self.block(blk as usize)
    }

    fn block(&mut self, start: usize) -> bool {
        let mut chain = Vec::new();
        let mut at = start;
        let answer = loop {
            charge(1);
            if let Some(cold) = self.memo[at] {
                break cold;
            }
            chain.push(at);
            // No loop around it: a spill would cost as much as the address
            // saves.
            if self.topo.is_root[at] {
                break false;
            }
            let owner = self.topo.owner[at];
            let up = if owner == NONE {
                self.topo.blk[at]
            } else {
                match self.b.tags[owner as usize] {
                    Tag::Loop => break false,
                    Tag::If
                    | Tag::Match
                    | Tag::SwitchTag
                    | Tag::SwitchInt
                    | Tag::SwitchChar
                    | Tag::SwitchStr => break true,
                    _ => self.topo.blk[owner as usize],
                }
            };
            if up == NONE {
                break false;
            }
            at = up as usize;
        };
        for c in chain {
            self.memo[c] = Some(answer);
        }
        answer
    }
}

#[cfg(test)]
mod tests;
