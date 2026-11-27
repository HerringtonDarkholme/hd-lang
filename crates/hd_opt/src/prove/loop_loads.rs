//! Loads hoisted out of read-only loops, and bounds guards a dominating
//! loop test decides (#2368). Both run in release, after the list proofs.
//!
//! A load through a reference, `[*$xs .buffer .ptr]`, gives the same value
//! on every iteration of a loop that writes no memory: the loop's stores go
//! only to unaddressed locals, and it calls nothing but pure intrinsics,
//! panics, which leave the loop, and functions whose whole-program write
//! summary says they write nothing their caller reaches
//! ([`super::Cx::call_writes`], #2374). Writes from elsewhere are what alias
//! analysis must rule out in LLVM's LICM (`promoteLoopAccessesToScalars`)
//! and Cranelift's alias-analysis load elimination. Here the place a live
//! reference refers to has no writer outside the loop either: while a
//! `&p` is live, a write to `p` or an overlapping place is an error
//! (`ref.excl.shared`), and through a counted pointer a run-time loan
//! panics instead (`ref.excl.alias.conflict`); a `&mut p` excludes every
//! other use (`ref.excl.mut`). So no other task or thread writes it while
//! the loop runs, and the only possible writers are the loop's own
//! instructions. Raw writes (`Ptr.write`, a `mem_store_*`, a `Store` into a
//! `ptr` place) take no loan, so any of them, any other call (a `dyn` or
//! function value call included), any count or drop that may free, and
//! any suspension point make the loop not read-only, and nothing in it is
//! hoisted.
//!
//! A load's key names its place from a local that keeps its value: the
//! root its definitions reach ([`super::places`]) is not stored from where
//! the chain read it to the loop's end, nor is memory the chain read
//! through a reference written there. A copy `k = r` of a reference
//! followed by `r = ys` keeps `k`'s loads in the loop.
//!
//! The load is read once before the loop into a new local. That read runs
//! even when the loop runs no iteration, which is safe: each step of its
//! place is a field or a dereference of a live reference, never an
//! element or a payload, so it reads valid memory. It goes before the
//! loop's waiting hoisted checks, which must stand right before the loop.
//! A check may replace its place's buffer, so a load whose place it may
//! reach stays in the loop: only one that parts from every check's place
//! at a field, through the same local, leaves.
//!
//! A guard `if i >= n: panic(..)` in such a loop whose test is `i < m`,
//! with `i` the exact induction from 0 by 1 ([`Lists::induction`]), is
//! decided when `n` and `m` are the same value or loads of one place that
//! nothing writes between `m`'s load and the loop's end, as LLVM's
//! `ConstraintElimination` and Go's `prove` drop a check that a dominating
//! condition implies. Its condition becomes the constant `false`, which the
//! backend folds as any literal condition. `i <= m` and any other test keep
//! the guard.
//!
//! Cost: one census of the body's instructions, then per loop one walk of
//! its body, which nesting repeats per enclosing loop: O(n × depth). Each
//! definition a key follows is computed once ([`super::places`]).

use super::places::{Def, Defs, Key};
use super::{Induction, Lists, Nest, Sets, Topology, scans};
use hd_base::LocalId;
use hd_tir::ir::{
    Body, Callee, IntrinsicOp, NONE, PlaceRoot, Proj, ReadMode, Ref, Tag, local_flags, place,
    u32_of,
};
use hd_types::Ty;

/// The intrinsic calls that write no memory and always return.
const PURE: &[&str] = &[
    "ptr_offset",
    "ptr_ref",
    "ptr_mut",
    "ptr_size_of",
    "ptr_align_of",
    "ptr_needs_drop",
    "int_count_ones",
    "int_leading_zeros",
    "int_trailing_zeros",
    "int_max",
    "int_min",
    "int_wrapping_add",
    "int_wrapping_sub",
    "int_wrapping_mul",
    "bytes_len",
    "char_scalar",
    "f32_to_bits",
    "f32_from_bits",
    "f64_to_bits",
    "f64_from_bits",
    "mem_load_u8",
    "mem_load_u16",
    "mem_load_u32",
    "mem_load_u64",
];

/// A load group leaving its loop: the loop, the block and position its
/// read goes to, its place's root local and steps, its type and its loads.
type Hoist = (u32, (u32, u32), (u32, Vec<Proj>), Ty, Vec<u32>);

/// One loop's facts.
struct LoopFacts<'a, 'b> {
    lists: &'a Lists<'b>,
    defs: &'a Defs<'b>,
    /// Instructions that may write memory, before each index.
    kills: &'a [u32],
    /// The loop.
    l: u32,
    /// The loop's first instruction.
    first: u32,
}

/// Hoists `b`'s invariant loads and decides its guards.
///
/// # Errors
/// The splice of the hoisted loads failing, a malformed body.
pub(super) fn run(b: &mut Body, cx: &dyn super::Cx) -> Result<(), String> {
    if !b.tags.contains(&Tag::Loop) {
        return Ok(());
    }
    let topo = Topology::new(b);
    let nest = Nest::new(b, &topo);
    let sets = Sets::new(b);
    scans(2 * b.len());
    let mut awaits = vec![false; b.sub_root.len()];
    for i in 0..b.len() {
        if matches!(
            b.tags[i],
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
        ) {
            awaits[topo.sub[i] as usize] = true;
        }
    }
    let stored = |sub: u32, local: usize| {
        b.storage
            .binary_search_by(|&(s, x, _)| (s.idx(), x.idx()).cmp(&(sub as usize, local)))
            .is_ok()
    };
    let defs = Defs::new(b, &topo, &nest, &sets, &stored);
    // `kills[i]`: how many instructions before `i` may write memory.
    let mut kills = vec![0u32; b.len() + 1];
    let mut decided = Vec::new();
    let mut hoists: Vec<Hoist> = Vec::new();
    {
        let plain = |i: usize| -> bool {
            let sub = topo.sub[i];
            let cell = |local: usize| stored(sub, local);
            let lists = Lists {
                b,
                topo: &topo,
                nest: &nest,
                cx,
                cell: &cell,
                sets: &sets,
            };
            !lists.writes_memory(i)
        };
        for i in 0..b.len() {
            kills[i + 1] = kills[i] + u32::from(!plain(i));
        }
        let mut claimed = vec![false; b.len()];
        // Outer loops first (a loop follows its body), so a load leaves
        // the outermost read-only loop it is invariant in.
        for l in (0..b.len()).rev().filter(|&l| b.tags[l] == Tag::Loop) {
            let sub = topo.sub[l];
            if awaits[sub as usize] {
                continue;
            }
            let cell = |local: usize| stored(sub, local);
            let lists = Lists {
                b,
                topo: &topo,
                nest: &nest,
                cx,
                cell: &cell,
                sets: &sets,
            };
            let body = lists.loop_body(u32_of(l));
            let Some(&first) = body.first() else { continue };
            if body
                .iter()
                .any(|&x| kills[x as usize + 1] != kills[x as usize])
            {
                continue;
            }
            let facts = LoopFacts {
                lists: &lists,
                defs: &defs,
                kills: &kills,
                l: u32_of(l),
                first,
            };
            decided.extend(facts.guards(&body));
            // The loop's waiting checks run between its hoisted reads and
            // it, and one may replace its place's buffer: a load moves
            // above them only where its place is apart from theirs.
            let (at, checks) = lists.waiting(u32_of(l));
            let from = first.saturating_sub(1);
            let written: Option<Vec<Key>> = checks
                .iter()
                .map(|&u| facts.place_key(b.data[u as usize][0], u, from))
                .collect();
            for (key, ty, loads) in facts.invariant_loads(&body, &mut claimed) {
                if written
                    .as_ref()
                    .is_some_and(|w| w.iter().all(|&c| defs.apart(c, key)))
                {
                    let place = (defs.local(key), defs.proj(key));
                    hoists.push((u32_of(l), at, place, ty, loads));
                } else {
                    // An inner loop's own position may still take it.
                    for x in loads {
                        claimed[x as usize] = false;
                    }
                }
            }
        }
    }
    if !decided.is_empty() {
        let no = u32_of(b.consts.len());
        b.consts.push((Ty::BOOL, 0));
        for g in decided {
            b.data[g as usize][0] = Ref::konst(no).0;
        }
    }
    if hoists.is_empty() {
        return Ok(());
    }
    let n = b.len();
    let mut inserts = Vec::new();
    for (l, (block, pos), (root, proj), ty, loads) in hoists {
        let local = u32_of(b.local_ty.len());
        b.local_ty.push(ty);
        b.local_name.push(cx.load_name());
        b.local_syn.push(b.local_syn[root as usize]);
        b.local_flags
            .push(local_flags::ASSIGNED | local_flags::READ);
        let words = hd_tir::ir::Place {
            root: PlaceRoot::Local(LocalId::from_raw(root)),
            proj,
        }
        .words()
        .ok_or("a hoisted load's place is a bare local")?;
        let word = u32_of(b.extra.len());
        b.extra.push(u32_of(words.len()));
        b.extra.extend(words);
        // Insertion `k` is value `n + k` (`insert_batch`).
        let read = u32_of(n + inserts.len());
        let syn = b.syn[l as usize];
        let mode = b.data[loads[0] as usize][1];
        for (tag, data, ty) in [
            (Tag::Read, [word, mode], ty),
            (Tag::Store, [local | place::BARE_LOCAL, read], Ty::VOID),
        ] {
            inserts.push(hd_tir::own::BatchInsert {
                block,
                pos,
                tag,
                data,
                ty,
                syn,
            });
        }
        for x in loads {
            b.tags[x as usize] = Tag::Read;
            b.data[x as usize] = [local | place::BARE_LOCAL, mode];
        }
    }
    hd_tir::own::insert_batch(b, &topo, &inserts).map(|_| ())
}

/// The first instruction of the outermost loop around `at` that loop
/// `l` is outside of, or `at`: where a value read at `at` may last have
/// been read before `l` ends.
fn reach(topo: &Topology, at: u32, l: u32) -> u32 {
    let mut out = at;
    let mut lp = topo.loop_of[at as usize];
    while lp != NONE && !(topo.first[lp as usize] <= l && l <= lp) {
        out = topo.first[lp as usize];
        lp = topo.loop_of[lp as usize];
    }
    out
}

impl Lists<'_> {
    /// Whether instruction `i` may write memory a reference reaches, or
    /// run code that does: anything but a value, a control instruction, a
    /// loan marker, a store to an unaddressed local, a pure intrinsic, a
    /// panic, or a call whose callee writes nothing its caller reaches.
    pub(super) fn writes_memory(&self, i: usize) -> bool {
        let b = self.b;
        match b.tags[i] {
            Tag::Read
            | Tag::Ref
            | Tag::Prim
            | Tag::And
            | Tag::Or
            | Tag::Coerce
            | Tag::Is
            | Tag::Block
            | Tag::Scope
            | Tag::If
            | Tag::Loop
            | Tag::Break
            | Tag::Continue
            | Tag::Return
            | Tag::Unreachable
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::Guard
            | Tag::ToArm
            | Tag::NewStruct
            | Tag::NewTuple
            | Tag::NewVariant
            | Tag::CopyStruct
            | Tag::RefIdentity
            | Tag::ItemRef
            | Tag::FlagSet
            | Tag::FlagClear
            | Tag::LoanBegin
            | Tag::LoanClose
            | Tag::LoanEnd
            | Tag::OwnLoans => false,
            Tag::Store => !b.stored_local(i).is_some_and(|(l, _)| {
                !self.sets.referenced[l.idx()]
                    && !(self.cell)(l.idx())
                    && b.local_flags[l.idx()] & local_flags::CAPTURED_ASSIGNED == 0
            }),
            // A callee whose write summary says it writes nothing its
            // caller reaches (#2374). A `dyn` or function value call is
            // `CallDyn` or `CallValue`: any code may run.
            Tag::Call => {
                let s = u32_of(i);
                !(self.calls_panic(s) || self.intrinsic(s).is_some_and(|n| PURE.contains(&&*n)))
                    && Callee::from_words(b.record(b.data[i][0]))
                        .is_none_or(|c| self.cx.call_writes(&c))
            }
            Tag::Intrinsic => !matches!(
                IntrinsicOp::from_u32(b.data[i][0]),
                Some(
                    IntrinsicOp::ListLen
                        | IntrinsicOp::MapLen
                        | IntrinsicOp::StrIndex
                        | IntrinsicOp::StrEq
                        | IntrinsicOp::ParamOwned
                        | IntrinsicOp::CellFlag
                )
            ),
            _ => true,
        }
    }
}

impl LoopFacts<'_, '_> {
    /// Definition `d`'s key, where its root local keeps the value it had
    /// where the chain read it, from there or `from`, whichever is
    /// earlier, to the loop's end, and nothing writes the memory read on
    /// its way in that range.
    fn holds(&self, d: Def, from: u32) -> Option<Key> {
        let lists = self.lists;
        let r = self.defs.local(d.key) as usize;
        let lo = |at: u32| reach(lists.topo, at, self.l).min(from);
        let stores = &lists.sets.stores[r];
        let next = stores.partition_point(|&(s, _)| s <= lo(d.root_at));
        let steady = !(lists.cell)(r)
            && lists.b.local_flags[r] & local_flags::CAPTURED_ASSIGNED == 0
            && !lists.sets.referenced[r]
            && stores.get(next).is_none_or(|&(s, _)| s > self.l);
        let read = d.mem_at == NONE
            || self.kills[self.l as usize + 1] == self.kills[lo(d.mem_at) as usize + 1];
        (steady && read).then_some(d.key)
    }

    /// The key of the place `word` names at instruction `at`, where its
    /// definition holds from `from` to the loop's end.
    fn place_key(&self, word: u32, at: u32, from: u32) -> Option<Key> {
        let p = self.lists.b.decode_place(word)?;
        self.holds(self.defs.place(&p, at)?, from)
    }

    /// The key of copy load `x` through a reference, read where no local
    /// on its way changes after `from`.
    fn load_key(&self, x: u32, from: u32) -> Option<Key> {
        let b = self.lists.b;
        let i = Ref(x).as_inst()?.0 as usize;
        // A borrow counts nothing, and its place does not change in the
        // loop, so a counted pointer's borrow leaves it as a copy does.
        let copy = b.data[i][1] == ReadMode::Copy as u32 && !self.lists.cx.tracked(b.ty[i]);
        if b.tags[i] != Tag::Read
            || b.data[i][0] & place::BARE_LOCAL != 0
            || !(copy || b.data[i][1] == ReadMode::Borrow as u32)
        {
            return None;
        }
        self.place_key(b.data[i][0], x, from)
            .filter(|&k| self.defs.derefs(k) != 0)
    }

    /// The loop's copy loads through a reference whose place nothing in
    /// the loop changes, grouped by place, with each group's type. A
    /// group's base local holds its value where the loop starts.
    fn invariant_loads(&self, body: &[u32], claimed: &mut [bool]) -> Vec<(Key, Ty, Vec<u32>)> {
        let lists = self.lists;
        let b = lists.b;
        let mut out: Vec<(Key, Ty, Vec<u32>)> = Vec::new();
        for &x in body {
            if claimed[x as usize] {
                continue;
            }
            let Some(key) = self.load_key(x, self.first.saturating_sub(1)) else {
                continue;
            };
            let k = self.defs.local(key) as usize;
            let ready = b.local_flags[k] & local_flags::PARAM != 0
                || lists.sets.stores[k]
                    .iter()
                    .any(|&(s, _)| lists.dominates(s, self.l));
            if !ready {
                continue;
            }
            claimed[x as usize] = true;
            let ty = b.ty[x as usize];
            match out.iter_mut().find(|(k, t, l)| {
                *k == key && *t == ty && b.data[l[0] as usize][1] == b.data[x as usize][1]
            }) {
                Some((_, _, loads)) => loads.push(x),
                None => out.push((key, ty, vec![x])),
            }
        }
        out
    }

    /// The loop's guards `if i >= n: panic` that its test `i < m` decides,
    /// `n` and `m` the same value or loads of one unchanged place: each
    /// guard's `If`.
    fn guards(&self, body: &[u32]) -> Vec<u32> {
        let lists = self.lists;
        let Some((test, false)) = lists.loop_guard(self.l) else {
            return Vec::new();
        };
        let Some(ind) = lists.induction(self.l, test) else {
            return Vec::new();
        };
        // The bound's place, unchanged from its load through the loop.
        let bound = Ref(ind.len).as_inst().and_then(|len| {
            let len = len.0;
            let (lo, hi) = (len.min(self.first), len.max(self.first));
            (self.kills[lo as usize] == self.kills[hi as usize + 1])
                .then(|| self.load_key(ind.len, lo))
                .flatten()
        });
        body.iter()
            .copied()
            .filter(|&x| lists.after_test(test, x) && self.decides(&ind, bound.as_ref(), x))
            .collect()
    }

    /// Whether `If` `x` is a guard `if i >= n: panic` that `ind` and the
    /// bound's key decide.
    fn decides(&self, ind: &Induction, bound: Option<&Key>, x: u32) -> bool {
        let lists = self.lists;
        let b = lists.b;
        if b.tags[x as usize] != Tag::If {
            return false;
        }
        let [cond, arms] = b.data[x as usize];
        let [then, NONE] = *b.record(arms) else {
            return false;
        };
        if !lists.is_op(cond, Tag::Prim, hd_tir::ir::PrimOp::Ge as u32) {
            return false;
        }
        let [i, n] = *b.record(b.data[cond as usize][1]) else {
            return false;
        };
        let panics = b
            .record(b.data[then as usize][0])
            .iter()
            .any(|&s| lists.calls_panic(s));
        if !panics || !lists.induction_index(ind, i, x) {
            return false;
        }
        let n = lists.source(n);
        n == ind.len
            || bound
                .is_some_and(|k| self.load_key(n, self.first.saturating_sub(1)).as_ref() == Some(k))
    }
}
