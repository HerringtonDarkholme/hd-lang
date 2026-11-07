//! One derivation of which reads borrow
//! (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`, "The One Place Each Count
//! Is Decided"), from the four facts of a value: its source is owned or
//! lent, each use consumes or lends, its last use on each path, and whether
//! its source may change while it lives.
//!
//! A read borrows when its value only lends and nothing changes its source
//! while it lives. A value lives from its read to its last use on each path,
//! which one backward walk finds, so a write on a path the value is dead on
//! stops nothing. The walk records what each live value crosses. A `Drop` of
//! a place the read's source goes through is not yet a change: it goes when
//! that place turns out to own nothing. Those conditions are solved after
//! the walk, from everything borrowing down to what really holds, so no
//! decision depends on the order the reads are met in.
//!
//! For now this decides nothing: [`agreement`] compares it with the passes
//! it will replace, and the driver reports the differences (`HD_OWN_AGREE`).

use hd_base::SliceSort as _;
use std::fmt::Write as _;

use super::flow::{Backward, BitArena, Flow, backward, backward_blocks};
use super::{Loc, OwnCx, Pass, Role, Tri, consumes, fill, moves, operands, splice};
use crate::ir::{
    Body, BodyKind, Callee, DEREF, ELEM, IntrinsicOp, NONE, Proj, ReadMode, Ref, Root, Tag,
    Topology, inside, is_suspension, local_flags, ref_word, sub_flags, u32_of,
};

/// The longest chain of roots a read's source is followed through.
const DEPTH: usize = 8;

/// Why a live value's source may change, for the report: the first thing
/// the walk met.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Change {
    #[default]
    None,
    /// A write of a place the source goes through.
    Write,
    /// A write through a reference, which may reach the source.
    Reached,
    /// A write of what calls and references reach.
    Heap,
    /// A call that may write what calls reach.
    Call,
    /// A suspension point.
    Suspension,
}

/// One place a read's source goes through: a local (`key` < the local
/// count) or a value (`key` = local count + value), the read's own steps
/// there (`own` codes of `Tables::paths` from `start`), then the steps below
/// them (`len` codes in all), and whether the steps there go behind a
/// reference or into a buffer, or the place is a top-level binding's.
#[derive(Clone, Copy)]
struct Link {
    key: u32,
    start: u32,
    own: u32,
    len: u32,
    behind: bool,
    global: bool,
}

/// What a read's source is ([`Tables::source`]).
#[derive(Clone, Copy, Default)]
struct Source {
    /// Ends at a root the walk can reason about.
    known: bool,
    /// Calls and writes through references may reach it.
    heap: bool,
    /// A local holds the one reference or the list it is reached by, so a
    /// call that writes nothing leaves it alone.
    held: bool,
    /// That reference is parameter k declared `&T` or `&mut T`, in
    /// exclusive mode: (its local, k).
    excl: Option<(u32, u32)>,
    /// All or a field path of a parameter the body never stores, whose
    /// count the frame holds across a suspension point.
    frame: bool,
}

/// One write at one place: where, its steps below that place, the last step
/// that goes behind a reference or into a buffer (`cut`), whether it changes
/// nothing a value holds (`keep`), and whether the place is the write's own.
/// `dropped` is what a `Drop` empties, which is no write if that owns
/// nothing.
#[derive(Clone, Copy)]
struct Write {
    key: u32,
    start: u32,
    len: u32,
    cut: u32,
    keep: bool,
    direct: bool,
    dropped: Dropped,
}

/// What a `Drop` empties.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dropped {
    Nothing,
    Value(u32),
    Local(u32),
}

/// The tables of one derivation.
#[derive(Default)]
pub(crate) struct Tables {
    // Per value: its uses, the last one, whether a use keeps it, the
    // `Store` that takes it, the coercion that keeps its values, whether a
    // `Drop` of all of it stands anywhere.
    uses: Vec<u32>,
    user: Vec<u32>,
    keeps: Vec<bool>,
    stored: Vec<u32>,
    alias: Vec<u32>,
    dropped: Vec<bool>,
    // Per value: a cleanup suite outside it uses it.
    in_suite: Vec<bool>,
    // Per local.
    stores: Vec<u32>,
    reached: Vec<bool>,
    escapes: Vec<bool>,
    changed: Vec<bool>,
    captured: Vec<bool>,
    fresh: Vec<bool>,
    ldropped: Vec<bool>,
    suite_local: Vec<bool>,
    first_read: Vec<u32>,
    last_read: Vec<u32>,
    inner: Vec<bool>,
    /// The read a local may hold as a binding, before it is decided.
    holds: Vec<u32>,
    excl_param: Vec<u32>,
    whole_other: Vec<bool>,
    receives: Vec<(u32, u32)>,
    excl: bool,
    probe: bool,
    // The candidates: per value and per local (a binding) its bit, and per
    // bit its read, whether it reached a walk, and what stops it.
    vbit: Vec<u32>,
    lbit: Vec<u32>,
    read: Vec<u32>,
    /// Per bit: the bit of the same read that follows one more binding local
    /// to what it holds, or `NONE`, and the binding this bit's source stops
    /// at. A binding that owns its value is the source itself; only one that
    /// borrows leads on to the next bit.
    far: Vec<u32>,
    stops: Vec<u32>,
    stop: u32,
    // Suites around the instruction the walk is at: what they use stays
    // live for the whole scope, on every way out of it.
    held: Vec<u32>,
    suite_bits: Vec<(u32, u32)>,
    seen: Vec<bool>,
    changes: Vec<Change>,
    relied: Vec<u64>,
    crossed: Vec<(u32, Dropped)>,
    // What the bits' sources are, as masks.
    heap: Vec<u64>,
    behind: Vec<u64>,
    reachable: Vec<u64>,
    unframed: Vec<u64>,
    excl_reads: Vec<(u32, u32, u32)>,
    // The links of each bit's source by key, in rows.
    key_start: Vec<u32>,
    key_rows: Vec<(u32, Link)>,
    paths: Vec<u32>,
    links: Vec<Link>,
    chain: Vec<u32>,
    // The writes of each instruction, in rows.
    write_start: Vec<u32>,
    writes: Vec<Write>,
    wpaths: Vec<u32>,
    // Per instruction: writes that reach the heap (with the value they are
    // rooted at), that a reference can reach (behind, value), calls (may
    // write), and suspension points.
    heap_write: Vec<u32>,
    reach_write: Vec<(u32, bool, u32)>,
    call: Vec<u8>,
    // Per `Scope` and `Defer`: the instructions of its suites.
    suites: Vec<(u32, u32, u32)>,
    suite_insts: Vec<u32>,
    suite_roots: Vec<u32>,
    // Values a use of another value keeps live ([`super::lent_values`]).
    carries: Vec<(u32, u32)>,
    carried: Vec<bool>,
    back: Flow,
    ops: Vec<super::Operand>,
    stack: Vec<u32>,
    pub(crate) relies: u64,
    /// Per value: the read is the last use of its place.
    last_use: Vec<bool>,
    /// Per local: the reads of it that keep a count, once derived.
    kept: Vec<u32>,
    /// Per local: the body owns none of it, once derived.
    pub(crate) unowned: Vec<bool>,
    /// Per instruction, once derived: the read borrows.
    pub(crate) borrows: Vec<bool>,
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.uses, n, 0);
        fill(&mut self.user, n, NONE);
        fill(&mut self.keeps, n, false);
        fill(&mut self.stored, n, NONE);
        fill(&mut self.alias, n, NONE);
        fill(&mut self.dropped, n, false);
        fill(&mut self.in_suite, n, false);
        fill(&mut self.stores, locals, 0);
        fill(&mut self.reached, locals, false);
        fill(&mut self.escapes, locals, false);
        fill(&mut self.changed, locals, false);
        fill(&mut self.captured, locals, false);
        fill(&mut self.fresh, locals, true);
        fill(&mut self.ldropped, locals, false);
        fill(&mut self.suite_local, locals, false);
        fill(&mut self.first_read, locals, NONE);
        fill(&mut self.last_read, locals, 0);
        fill(&mut self.inner, locals, false);
        fill(&mut self.holds, locals, NONE);
        fill(&mut self.excl_param, locals, NONE);
        fill(&mut self.whole_other, locals, false);
        self.receives.clear();
        fill(&mut self.vbit, n, NONE);
        fill(&mut self.lbit, locals, NONE);
        self.read.clear();
        self.far.clear();
        self.stops.clear();
        self.held.clear();
        self.suite_bits.clear();
        self.crossed.clear();
        self.excl_reads.clear();
        self.key_rows.clear();
        self.paths.clear();
        self.writes.clear();
        self.wpaths.clear();
        fill(&mut self.heap_write, n, NONE);
        self.reach_write.clear();
        fill(&mut self.call, n, 0);
        self.suites.clear();
        self.suite_insts.clear();
        self.suite_roots.clear();
        self.carries.clear();
        fill(&mut self.carried, n, false);
        self.relies = 0;
        fill(&mut self.unowned, locals, false);
        fill(&mut self.borrows, n, false);
    }

    /// Follows the place at `word` into `self.links`, down to what it
    /// finally reads: through the reads its root values made and, with
    /// `follow`, through the read a binding local holds.
    fn source(&mut self, b: &Body, word: u32, levels: u32, handle: bool) -> Source {
        self.links.clear();
        self.chain.clear();
        self.stop = NONE;
        self.source_below(b, word, levels, handle, 0, 0)
    }

    fn source_below(
        &mut self,
        b: &Body,
        word: u32,
        mut levels: u32,
        mut handle: bool,
        below_start: u32,
        below: u32,
    ) -> Source {
        let locals = u32_of(b.local_ty.len());
        let mut out = Source::default();
        let (mut word, mut below_start, mut below) = (word, below_start, below);
        let (mut derefs, mut elem, mut first_deref) = (0, false, false);
        let (mut seen_deref, mut above) = (false, false);
        let mut pinned = false;
        for _ in 0..DEPTH {
            let p = b.view(word);
            let start = u32_of(self.chain.len());
            let first = p.first(b) == Some(Proj::Deref);
            let (mut d, e) = p.push_steps(b, &mut self.chain);
            if std::mem::take(&mut handle) && self.chain.pop_if(|c| *c == DEREF).is_some() {
                d -= 1;
            }
            let own = u32_of(self.chain.len()) - start;
            self.chain
                .extend_from_within(below_start as usize..(below_start + below) as usize);
            if seen_deref && own > 0 {
                above = true;
            }
            if d > 0 {
                seen_deref = true;
                first_deref = first;
            }
            derefs += d;
            elem |= e;
            let link = Link {
                key: NONE,
                start,
                own,
                len: own + below,
                behind: d > 0 || e,
                global: false,
            };
            (below_start, below) = (start, own + below);
            match p.root {
                Root::Local(l) => {
                    self.links.push(Link { key: l, ..link });
                    let held = self.holds[l as usize];
                    if held != NONE {
                        if levels > 0 {
                            levels -= 1;
                            word = b.data[held as usize][0];
                            continue;
                        }
                        self.stop = l;
                    }
                    let li = l as usize;
                    let param = b.local_flags[li] & local_flags::PARAM != 0;
                    out.known = !self.escapes[li];
                    out.heap =
                        derefs > 0 || (elem && (param || !self.fresh[li] || self.reached[li]));
                    let one = (derefs == 1 && first_deref && !above) || (derefs == 0 && elem);
                    out.held = one && (!param || self.stores[li] == 0);
                    out.excl = (out.held && derefs == 1 && self.excl_param[li] != NONE)
                        .then(|| (l, self.excl_param[li]));
                    out.frame = param && self.stores[li] == 0 && derefs == 0 && !elem;
                    return out;
                }
                Root::Value(v) => {
                    self.links.push(Link {
                        key: locals + v,
                        ..link
                    });
                    if pinned {
                        out.known = true;
                        out.heap = derefs > 0;
                        return out;
                    }
                    if b.tags[v as usize] == Tag::Read {
                        word = b.data[v as usize][0];
                        continue;
                    }
                    if own == 0
                        && below > 0
                        && self.chain.get(start as usize) == Some(&DEREF)
                        && b.pinned_element_ref(v as usize)
                    {
                        word = b.data[v as usize][0];
                        (below_start, below) = (start + 1, below - 1);
                        derefs -= 1;
                        pinned = true;
                        continue;
                    }
                    out.known = true;
                    out.heap = (derefs > 0 || elem)
                        && !(self.chain[..] == [DEREF] && b.provider_ref(v as usize));
                    return out;
                }
                Root::Global(..) => {
                    self.links.push(Link {
                        global: true,
                        ..link
                    });
                    out.known = true;
                    out.heap = true;
                    return out;
                }
                _ => return out,
            }
        }
        out
    }

    /// Records a write at every link just followed.
    fn write_links(&mut self, keep: bool, dropped: Dropped) {
        for k in 0..self.links.len() {
            let link = self.links[k];
            if link.key == NONE {
                continue;
            }
            let path = &self.chain[link.start as usize..(link.start + link.len) as usize];
            let cut = path
                .iter()
                .rposition(|&c| c == ELEM || c == DEREF)
                .map_or(NONE, u32_of);
            let start = u32_of(self.wpaths.len());
            self.wpaths.extend_from_slice(path);
            self.writes.push(Write {
                key: link.key,
                start,
                len: link.len,
                cut,
                keep,
                direct: k == 0,
                dropped,
            });
        }
    }

    fn receives(&self, x: u32, l: u32) -> bool {
        let k = self.receives.partition_point(|&(c, _)| c < x);
        self.receives[k..]
            .iter()
            .take_while(|&&(c, _)| c == x)
            .any(|&(_, m)| m == l)
    }

    /// The `Store` that takes read `r`'s value: its own, or that of the one
    /// coercion keeping its values that is its only use.
    fn store_of(&self, r: usize) -> u32 {
        let c = self.alias[r];
        if c == NONE {
            return self.stored[r];
        }
        let c = c as usize;
        if self.uses[r] == 1
            && self.uses[c] == 1
            && self.alias[c] == NONE
            && !self.dropped[r]
            && !self.dropped[c]
        {
            self.stored[c]
        } else {
            NONE
        }
    }

    /// The local `Store` `s` makes a binding of: all of a local that is no
    /// parameter, that no reference or closure reaches, stored once.
    fn binding(&self, b: &Body, s: u32) -> Option<u32> {
        let p = b.view(b.data[s as usize][0]);
        let l = p.local()?;
        let li = l as usize;
        (p.whole()
            && b.local_flags[li] & local_flags::PARAM == 0
            && !self.reached[li]
            && !self.changed[li]
            && !self.captured[li]
            && self.stores[li] == 1
            && self.ldropped[li])
            .then_some(l)
    }

    /// Whether the walk may mark what `x` reads or writes: the state is
    /// `live` after `x` and its operands.
    fn mark(&mut self, x: u32, s: &BitArena, live: usize, mask: &[u64], except: u32, why: Change) {
        let row = s.row(live);
        for (k, (&l, &m)) in row.iter().zip(mask).enumerate() {
            let mut hits = l & m;
            while hits != 0 {
                let bit = u32_of(k * 64) + hits.trailing_zeros();
                hits &= hits - 1;
                if bit != except && self.read[bit as usize] != x {
                    let c = &mut self.changes[bit as usize];
                    if *c == Change::None {
                        *c = why;
                    }
                }
            }
        }
    }
}

/// Whether value `v` is lent from, with a use of `v` also a use of what
/// lends it: the values `i` lends that its own value may point through,
/// and the arguments of a suspending call its frame holds until it is
/// awaited.
fn carries(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let mut through = Vec::new();
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..b.len() {
        super::lent_values(b, cx, i, &mut through);
        for &v in &through {
            if Ref(v).as_inst().is_some() && (v as usize) < b.len() {
                t.carries.push((u32_of(i), v));
            }
        }
        if matches!(b.tags[i], Tag::Call | Tag::CallValue | Tag::CallDyn) {
            let awaited = t.user[i] != NONE && is_suspension(b.tags[t.user[i] as usize]);
            if awaited {
                operands(b, i, &mut ops);
                for op in &ops {
                    t.carries.push((u32_of(i), op.value));
                }
            }
        }
    }
    t.ops = ops;
    for k in 0..t.carries.len() {
        let u = t.carries[k].0 as usize;
        t.carried[u] = true;
    }
}

/// The facts of each value and local: uses, drops, stores, reads.
fn scan(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    let mut recursive = vec![false; b.local_ty.len()];
    for ci in 0..n {
        if let Some(l) = b.closure_self(ci) {
            recursive[l.idx()] = true;
        }
    }
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        match tag {
            Tag::Drop => {
                let p = b.view(a);
                if p.whole() {
                    match p.root {
                        Root::Value(v) => t.dropped[v as usize] = true,
                        Root::Local(l) => t.ldropped[l as usize] = true,
                        _ => {}
                    }
                }
            }
            Tag::Store => {
                if let Some(l) = b.local_in_place(a) {
                    let li = l.idx();
                    t.stores[li] += 1;
                    let fresh = Ref(w).as_inst().is_none_or(|_| {
                        matches!(
                            b.tags.get(w as usize),
                            Some(
                                Tag::Call
                                    | Tag::CallValue
                                    | Tag::CallHost
                                    | Tag::DefaultCall
                                    | Tag::NewList
                                    | Tag::NewMap
                                    | Tag::Read
                            )
                        )
                    });
                    t.fresh[li] &= fresh;
                }
            }
            Tag::Ref => {
                if let Some(l) = b.local_in_place(a) {
                    t.reached[l.idx()] = true;
                }
            }
            Tag::IsUnique => {
                if let Some(l) = b.local_in_place(a) {
                    t.changed[l.idx()] = true;
                }
            }
            Tag::Read => {
                if let Some(l) = b.view(a).local() {
                    let li = l as usize;
                    t.first_read[li] = t.first_read[li].min(u32_of(i));
                    t.last_read[li] = u32_of(i);
                }
            }
            Tag::Closure => {
                let rec = b.record(w);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    let l = b.cap_local[c].idx();
                    t.captured[l] = true;
                    if b.cap_mode[c].shares() && !recursive[l] {
                        t.reached[l] = true;
                        t.escapes[l] = true;
                    }
                }
            }
            _ => {}
        }
        operands(b, i, &mut ops);
        for op in &ops {
            let v = op.value as usize;
            if op.role == Role::Dropped {
                continue;
            }
            t.uses[v] += 1;
            t.user[v] = u32_of(i);
            let keeps = match op.role {
                Role::Consume if b.same_value_coercion(i) && t.alias[v] == NONE => {
                    t.alias[v] = u32_of(i);
                    false
                }
                Role::Consume if tag == Tag::Store && op.at == Loc::Data(1) => {
                    if t.stored[v] == NONE {
                        t.stored[v] = u32_of(i);
                        false
                    } else {
                        true
                    }
                }
                Role::Consume | Role::Tail => true,
                Role::Arg(_) => consumes(b, cx, i, op, NONE),
                Role::Dropped => false,
                Role::Lend => match tag {
                    Tag::Read
                    | Tag::Store
                    | Tag::IsUnique
                    | Tag::Prim
                    | Tag::Intrinsic
                    | Tag::Call
                    | Tag::CallValue
                    | Tag::CallHost
                    | Tag::DefaultCall
                    | Tag::Await
                    | Tag::Match
                    | Tag::SwitchTag
                    | Tag::SwitchInt
                    | Tag::SwitchChar
                    | Tag::SwitchStr
                    | Tag::If
                    | Tag::Is
                    | Tag::With
                    | Tag::ContextNew => false,
                    // A reference into the value's own storage outlives the
                    // read; one to a referent does not, nor does a loop's
                    // element reference into the buffer the read pins.
                    Tag::Ref => {
                        let p = b.view(a);
                        (matches!(p.root, Root::Const(_)) || p.first(b) != Some(Proj::Deref))
                            && !b.pinned_element_ref(i)
                    }
                    _ => true,
                },
            };
            t.keeps[v] |= keeps;
        }
    }
    t.ops = ops;
    for i in 0..n {
        if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(b.data[i][0])
            && !super::borrow::lends_once(b, cx, &t.uses, &t.user, i)
        {
            t.escapes[l.idx()] = true;
        }
    }
    // A value a coercion consumes is kept where the coercion's value is.
    for v in (0..n).rev() {
        let c = t.alias[v];
        if c != NONE {
            let c = c as usize;
            let bound =
                t.stored[c] != NONE && t.store_of(v) != NONE && t.binding(b, t.stored[c]).is_some();
            t.keeps[v] |= t.keeps[c]
                || (t.stored[c] != NONE && !bound)
                || t.alias[c] != NONE
                || t.uses[v] > 1;
        }
    }
}

/// The cleanup suites: which values and locals one reaches from outside it,
/// and the instructions each holds. A suite runs at its scope's exits, which
/// the walk does not follow.
fn suites(b: &Body, t: &mut Tables) {
    let mut stack = std::mem::take(&mut t.stack);
    let mut ops = std::mem::take(&mut t.ops);
    let mut mine = vec![NONE; b.len()];
    for x in 0..b.len() {
        let [a, w] = b.data[x];
        let roots: Vec<u32> = match b.tags[x] {
            Tag::Scope if w != NONE => b.record(w).to_vec(),
            Tag::Defer => vec![a],
            _ => continue,
        };
        let from = u32_of(t.suite_insts.len());
        inside(b, &roots, &mut stack, |i| {
            t.suite_insts.push(u32_of(i));
            mine[i] = u32_of(x);
            true
        });
        let to = u32_of(t.suite_insts.len());
        t.suites.push((u32_of(x), from, to));
        t.suite_roots.extend_from_slice(&roots);
        for k in from..to {
            let i = t.suite_insts[k as usize] as usize;
            if matches!(
                b.tags[i],
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique
            ) && let Some(l) = b.view(b.data[i][0]).local()
            {
                t.suite_local[l as usize] = true;
            }
            operands(b, i, &mut ops);
            for op in &ops {
                if mine[op.value as usize] != u32_of(x) {
                    t.in_suite[op.value as usize] = true;
                }
            }
        }
    }
    t.stack = stack;
    t.ops = ops;
}

/// A value no instance may copy needs a count wherever a use keeps it or a
/// store takes a part of it, which only a move gives; so does the read the
/// part leaves.
fn uncopyable(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let mut last = (hd_types::Ty::VOID, false);
    for i in (0..b.len()).rev() {
        if b.tags[i] != Tag::Read || b.data[i][1] != ReadMode::Copy as u32 {
            continue;
        }
        let ty = b.ty[i];
        if last.0 != ty {
            last = (ty, cx.copies(ty) != Tri::Yes && cx.counted(ty));
        }
        let p = b.view(b.data[i][0]);
        let part = !p.whole() && p.in_place(b);
        if !last.1 || !(t.keeps[i] || (part && t.store_of(i) != NONE)) {
            continue;
        }
        t.keeps[i] = true;
        if let Root::Value(v) = p.root
            && p.in_place(b)
            && b.tags[v as usize] == Tag::Read
        {
            t.keeps[v as usize] = true;
        }
    }
}

/// Exclusive mode: the calls that receive a parameter's reference, and the
/// locals whose reference something else takes.
fn receivers(b: &Body, t: &mut Tables) {
    let whole = |v: usize| {
        let p = (b.tags[v] == Tag::Read).then(|| b.view(b.data[v][0]))?;
        match p.root {
            Root::Local(l) if p.whole() => Some(l),
            _ => None,
        }
    };
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..b.len() {
        let call = match b.tags[i] {
            Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall => true,
            Tag::Hook | Tag::Await => true,
            Tag::Intrinsic => IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::Item),
            _ => false,
        };
        let harmless = matches!(
            b.tags[i],
            Tag::Read | Tag::Ref | Tag::Store | Tag::Drop | Tag::IsUnique | Tag::Prim
        );
        operands(b, i, &mut ops);
        for o in &ops {
            let v = o.value as usize;
            let l = match b.tags[v] {
                Tag::Read => whole(v),
                Tag::Ref if b.data[v][1] & ref_word::MUT != 0 => match b.view(b.data[v][0]).root {
                    Root::Local(l) => Some(l),
                    Root::Value(u) => whole(u as usize),
                    _ => None,
                },
                _ => None,
            };
            let Some(l) = l else { continue };
            if call {
                t.receives.push((u32_of(i), l));
            } else if !(harmless && matches!(o.role, Role::Lend | Role::Dropped)) {
                t.whole_other[l as usize] = true;
            }
        }
    }
    t.ops = ops;
}

/// Each parameter's position in the signature, and exclusive mode's
/// reference parameters.
fn params(b: &Body, cx: &dyn OwnCx, t: &mut Tables) -> Vec<(u32, usize)> {
    for r in b.sub_params.iter().skip(1) {
        for k in 0..r.len {
            t.inner[b.extra[(r.start + 1 + k) as usize] as usize] = true;
        }
    }
    let list: Vec<u32> = (0..u32_of(b.local_ty.len()))
        .filter(|&l| b.local_flags[l as usize] & local_flags::PARAM != 0 && !t.inner[l as usize])
        .collect();
    let n = list.len();
    let mut out = Vec::with_capacity(n);
    for (at, &l) in list.iter().enumerate() {
        let li = l as usize;
        if t.excl && at < 64 && !t.whole_other[li] && cx.local_ref_param(b.item, at, n) {
            t.excl_param[li] = u32_of(at);
        }
        out.push((l, at));
    }
    out
}

/// Numbers the reads that may borrow: counted, whose every use lends or
/// stores them into a binding. Their aliases and bindings share their bit.
fn candidates(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    for r in 0..b.len() {
        if b.tags[r] != Tag::Read
            || b.data[r][1] != ReadMode::Copy as u32
            || t.keeps[r]
            || !cx.counted(b.ty[r])
        {
            continue;
        }
        let store = t.store_of(r);
        let alias = if store == NONE { t.alias[r] } else { NONE };
        if alias != NONE && !t.dropped[alias as usize] {
            continue;
        }
        let plain = alias != NONE || (t.dropped[r] && t.stored[r] == NONE);
        let bind = if plain {
            None
        } else if !t.dropped[r] && store != NONE && t.uses[r] == 1 {
            match t.binding(b, store) {
                Some(l) => Some(l),
                None => continue,
            }
        } else {
            continue;
        };
        let bit = u32_of(t.read.len());
        t.read.push(u32_of(r));
        t.far.push(NONE);
        t.vbit[r] = bit;
        if t.alias[r] != NONE {
            t.vbit[t.alias[r] as usize] = bit;
        }
        if let Some(l) = bind {
            t.lbit[l as usize] = bit;
            t.holds[l as usize] = u32_of(r);
        }
    }
    // A read has one more bit per binding its source may go through.
    for bit in 0..t.read.len() {
        let r = t.read[bit] as usize;
        let mut word = b.data[r][0];
        let mut at = bit;
        for _ in 0..DEPTH {
            word = match b.view(word).root {
                Root::Local(l) if t.holds[l as usize] != NONE => {
                    let next = u32_of(t.read.len());
                    t.far[at] = next;
                    at = next as usize;
                    t.read.push(u32_of(r));
                    t.far.push(NONE);
                    b.data[t.holds[l as usize] as usize][0]
                }
                Root::Value(v) if b.tags[v as usize] == Tag::Read => b.data[v as usize][0],
                _ => break,
            };
        }
    }
    // What each suite uses, by bit.
    let mut ops = std::mem::take(&mut t.ops);
    for k in 0..t.suites.len() {
        let (x, from, to) = t.suites[k];
        for j in from..to {
            let i = t.suite_insts[j as usize] as usize;
            if matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Store)
                && let Some(l) = b.view(b.data[i][0]).local()
                && t.lbit[l as usize] != NONE
            {
                t.suite_bits.push((x, t.lbit[l as usize]));
            }
            operands(b, i, &mut ops);
            for op in &ops {
                let v = op.value as usize;
                if t.in_suite[v] && t.vbit[v] != NONE {
                    t.suite_bits.push((x, t.vbit[v]));
                }
            }
        }
    }
    t.ops = ops;
    let words = t.read.len().div_ceil(64).max(1);
    fill(&mut t.seen, t.read.len(), false);
    fill(&mut t.changes, t.read.len(), Change::None);
    fill(&mut t.relied, t.read.len(), 0);
    fill(&mut t.heap, words, 0);
    fill(&mut t.behind, words, 0);
    fill(&mut t.reachable, words, 0);
    fill(&mut t.unframed, words, 0);
    t.back.arena.words = words;
}

/// Follows each candidate's source: its links by key, its masks.
fn sources(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let locals = u32_of(b.local_ty.len());
    for bit in 0..t.read.len() {
        let r = t.read[bit] as usize;
        let mut before = None;
        let mut last = None;
        for step in b.view(b.data[r][0]).steps(b) {
            (before, last) = (last, Some(step));
        }
        // A trait value read through the counted pointer a part holds is
        // that pointer's words: its object is held by the pointer's count.
        let handle = last == Some(Proj::Deref)
            && matches!(
                before,
                Some(Proj::Field(_) | Proj::Elem(_) | Proj::Variant(_))
            )
            && cx.trait_value(b.ty[r]);
        // How many bindings this bit goes through: its place in its read's
        // list of bits.
        let mut levels = 0;
        let mut at = t.vbit[r];
        while at != u32_of(bit) {
            at = t.far[at as usize];
            levels += 1;
        }
        let source = t.source(b, b.data[r][0], levels, handle);
        t.stops.push(t.stop);
        let at = bit / 64;
        let one = 1u64 << (bit % 64);
        if !source.known {
            t.changes[bit] = Change::Write;
            continue;
        }
        if source.heap {
            t.heap[at] |= one;
        }
        if !source.frame {
            t.unframed[at] |= one;
        }
        if t.links.iter().any(|l| l.behind) {
            t.behind[at] |= one;
        }
        if t.links
            .iter()
            .any(|l| l.global || (l.key < locals && t.reached[l.key as usize]))
        {
            t.reachable[at] |= one;
        }
        if let Some((l, k)) = source.excl {
            t.excl_reads.push((u32_of(bit), l, k));
        }
        let shift = u32_of(t.paths.len());
        t.paths.extend_from_slice(&t.chain);
        for k in 0..t.links.len() {
            let link = t.links[k];
            if link.key != NONE {
                let moved = Link {
                    start: link.start + shift,
                    ..link
                };
                t.key_rows.push((u32_of(bit), moved));
            }
        }
    }
    t.key_rows.sort_stable_by_key(|&(bit, l)| (l.key, bit));
    let keys = locals as usize + b.len();
    fill(&mut t.key_start, keys + 2, 0);
    for &(_, l) in &t.key_rows {
        t.key_start[l.key as usize + 1] += 1;
    }
    for k in 0..=keys {
        t.key_start[k + 1] += t.key_start[k];
    }
}

/// What each instruction writes.
fn index_writes(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    let locals = u32_of(b.local_ty.len());
    fill(&mut t.write_start, n + 1, 0);
    for i in 0..n {
        t.write_start[i] = u32_of(t.writes.len());
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        let verifier = match tag {
            Tag::Store | Tag::Drop | Tag::IsUnique => true,
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Ref => w & ref_word::MUT != 0,
            _ => false,
        };
        let place = matches!(
            tag,
            Tag::Store | Tag::Drop | Tag::IsUnique | Tag::Read | Tag::Ref
        );
        let through = match place.then(|| b.view(a).root) {
            Some(Root::Value(v)) => v,
            _ => NONE,
        };
        if verifier {
            let p = b.view(a);
            let (derefs, elem) = match p.root {
                Root::Const(_) => (0, false),
                _ => p.kinds(b),
            };
            let behind = derefs > 0 || elem;
            let reachable = behind
                || match p.root {
                    Root::Local(l) => t.escapes[l as usize],
                    Root::Value(_) => false,
                    _ => true,
                };
            if reachable {
                let global = matches!(p.root, Root::Global(..));
                t.reach_write.push((u32_of(i), behind || global, through));
            }
        }
        match tag {
            Tag::Store | Tag::Ref if tag == Tag::Store || w & ref_word::MUT != 0 => {
                let source = t.source(b, a, 0, false);
                t.write_links(false, Dropped::Nothing);
                let reached = matches!(t.links.last(), Some(l) if l.key < locals && t.escapes[l.key as usize]);
                if !source.known || source.heap || reached {
                    t.heap_write[i] = through;
                    t.call[i] |= 4;
                }
            }
            Tag::Read if w == ReadMode::Move as u32 => {
                t.source(b, a, 0, false);
                t.links.truncate(1);
                t.write_links(false, Dropped::Nothing);
            }
            Tag::Drop => {
                t.source(b, a, 0, false);
                t.links.truncate(1);
                let p = b.view(a);
                // A provider reference's `Drop` releases nothing.
                let dropped = match p.root {
                    Root::Value(v) if p.whole() => Dropped::Value(v),
                    Root::Local(l) if p.whole() => Dropped::Local(l),
                    _ => Dropped::Nothing,
                };
                t.write_links(false, dropped);
            }
            Tag::IsUnique => {
                t.source(b, a, 0, false);
                t.links.truncate(1);
                t.write_links(true, Dropped::Nothing);
            }
            Tag::Intrinsic => match IntrinsicOp::from_u32(a) {
                Some(IntrinsicOp::MapSet | IntrinsicOp::MapRemove) => {
                    if let Some(&c) = b.record(w).first()
                        && Ref(c).as_inst().is_some()
                        && (c as usize) < n
                    {
                        let heap = collection(b, t, c);
                        t.write_links(false, Dropped::Nothing);
                        if heap {
                            t.heap_write[i] = c;
                            t.call[i] |= 4;
                        }
                    }
                }
                Some(IntrinsicOp::Item) => t.call[i] |= 1,
                _ => {}
            },
            Tag::Call => {
                let may = Callee::from_words(b.record(a)).is_none_or(|c| cx.may_write(&c));
                t.call[i] |= u8::from(may);
                let args = b.record(w);
                for &v in args.get(..args.len().saturating_sub(3)).unwrap_or_default() {
                    if let Some(r) = Ref(v).as_inst().map(|r| uncoerced(b, r.0 as usize))
                        && b.tags[r] == Tag::Ref
                        && b.data[r][1] & ref_word::MUT != 0
                        && ((may && cx.summarizes()) || b.local_in_place(b.data[r][0]).is_some())
                    {
                        t.source(b, b.data[r][0], 0, false);
                        t.write_links(false, Dropped::Nothing);
                    }
                }
            }
            Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall | Tag::Hook => {
                t.call[i] |= 1;
            }
            Tag::Await => t.call[i] |= 1,
            _ => {}
        }
    }
    t.write_start[n] = u32_of(t.writes.len());
}

/// The links of the collection value `c` a map's change writes in; whether
/// it may reach the heap.
fn collection(b: &Body, t: &mut Tables, c: u32) -> bool {
    let locals = u32_of(b.local_ty.len());
    t.links.clear();
    t.chain.clear();
    t.chain.push(ELEM);
    t.links.push(Link {
        key: locals + c,
        start: 0,
        own: 1,
        len: 1,
        behind: true,
        global: false,
    });
    if b.tags[c as usize] == Tag::Read {
        let source = t.source_below(b, b.data[c as usize][0], 0, false, 0, 1);
        return !source.known || source.heap;
    }
    true
}

/// Value `v`, or what the coercions that keep its value coerce.
fn uncoerced(b: &Body, mut v: usize) -> usize {
    for _ in 0..DEPTH {
        if !b.same_value_coercion(v) {
            break;
        }
        v = b.data[v][0] as usize;
    }
    v
}

fn overlap(a: &[u32], c: &[u32]) -> bool {
    a.iter().zip(c).all(|(x, y)| x == y)
}

impl Backward for Tables {
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        // A suite runs after the statement it guards, on every way out, so
        // what it uses lives through the scope.
        if matches!(b.tags[i], Tag::Scope | Tag::Defer) {
            let k = self.suites.partition_point(|e| e.0 < x);
            if let Some(&(y, from, to)) = self.suites.get(k)
                && y == x
            {
                for j in from..to {
                    let inst = self.suite_insts[j as usize];
                    self.change_at(b, inst, s, live);
                }
                self.held.push(NONE);
                for k in 0..self.suite_bits.len() {
                    if self.suite_bits[k].0 == x {
                        let bit = self.suite_bits[k].1;
                        self.held.push(bit);
                        self.set(bit, s, live);
                    }
                }
            }
        }
        // Its value is made here: dead before. A coercion's value is still
        // its read's.
        let bit = self.vbit[i];
        if bit != NONE && self.read[bit as usize] == x {
            self.seen[bit as usize] = true;
            let mut at = bit;
            while at != NONE {
                s.set(live, at, false);
                at = self.far[at as usize];
            }
        }
    }

    fn ran(&mut self, b: &Body, x: u32, _: &mut BitArena, _: usize) {
        if matches!(b.tags[x as usize], Tag::Scope | Tag::Defer)
            && self.suites.binary_search_by_key(&x, |e| e.0).is_ok()
        {
            while self.held.pop().is_some_and(|h| h != NONE) {}
        }
    }

    fn uses(&mut self, _: &Body, v: u32, s: &mut BitArena, live: usize) {
        self.use_value(v, s, live);
    }

    fn reads(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        // A read through a binding local uses what it holds.
        if matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Store)
            && let Some(l) = b.view(b.data[i][0]).local()
            && self.lbit[l as usize] != NONE
        {
            self.set(self.lbit[l as usize], s, live);
        }
        for k in 0..self.held.len() {
            if self.held[k] != NONE {
                self.set(self.held[k], s, live);
            }
        }
        self.change_at(b, x, s, live);
    }
}

impl Tables {
    fn set(&self, bit: u32, s: &mut BitArena, live: usize) {
        let mut at = bit;
        while at != NONE {
            s.set(live, at, true);
            at = self.far[at as usize];
        }
    }

    fn use_value(&self, v: u32, s: &mut BitArena, live: usize) {
        let i = v as usize;
        if let Some(&bit) = self.vbit.get(i)
            && bit != NONE
        {
            self.set(bit, s, live);
        }
        if self.carried.get(i) == Some(&true) {
            let k = self.carries.partition_point(|c| c.0 < v);
            for &(_, u) in self.carries[k..].iter().take_while(|c| c.0 == v) {
                self.use_value(u, s, live);
            }
        }
    }

    /// What instruction `x` may change, against every value `live`.
    fn change_at(&mut self, b: &Body, x: u32, s: &BitArena, live: usize) {
        let i = x as usize;
        for k in self.write_start[i]..self.write_start[i + 1] {
            let w = self.writes[k as usize];
            let (lo, hi) = (
                self.key_start[w.key as usize] as usize,
                self.key_start[w.key as usize + 1] as usize,
            );
            for row in lo..hi {
                let (bit, link) = self.key_rows[row];
                if !s.get(live, bit) || self.read[bit as usize] == x {
                    continue;
                }
                let at = link.start as usize;
                let own = &self.paths[at..at + link.own as usize];
                let below = &self.paths[at..at + link.len as usize];
                let path = &self.wpaths[w.start as usize..(w.start + w.len) as usize];
                let hit = (w.direct && overlap(own, path))
                    || (!w.keep && overlap(below, path) && (w.cut == NONE || link.len > w.cut));
                if !hit {
                    continue;
                }
                if w.dropped == Dropped::Nothing {
                    if self.changes[bit as usize] == Change::None {
                        self.changes[bit as usize] = Change::Write;
                    }
                } else {
                    self.crossed.push((bit, w.dropped));
                }
            }
        }
        if self.reach_write.is_empty() && self.call[i] == 0 && !is_suspension(b.tags[i]) {
            return;
        }
        let k = self.reach_write.partition_point(|e| e.0 < x);
        if let Some(&(y, behind, through)) = self.reach_write.get(k)
            && y == x
        {
            let except = self.vbit.get(through as usize).copied().unwrap_or(NONE);
            let mask = std::mem::take(&mut self.behind);
            self.mark(x, s, live, &mask, except, Change::Reached);
            if behind {
                let reach = std::mem::take(&mut self.reachable);
                self.mark(x, s, live, &reach, except, Change::Reached);
                self.reachable = reach;
            }
            self.behind = mask;
        }
        if self.call[i] & 4 != 0 {
            let through = self.heap_write[i];
            let except = self.vbit.get(through as usize).copied().unwrap_or(NONE);
            let mask = std::mem::take(&mut self.heap);
            self.mark(x, s, live, &mask, except, Change::Heap);
            self.heap = mask;
        }
        if self.call[i] & 1 != 0 {
            self.call_at(x, s, live);
        }
        if is_suspension(b.tags[i]) {
            let mask = std::mem::take(&mut self.unframed);
            self.mark(x, s, live, &mask, NONE, Change::Suspension);
            self.unframed = mask;
        }
    }

    fn call_at(&mut self, x: u32, s: &BitArena, live: usize) {
        // Exclusive mode: the call reaches the place only through the
        // reference, so only when it receives the reference itself.
        let mut mask = std::mem::take(&mut self.heap);
        let saved = mask.clone();
        for k in 0..self.excl_reads.len() {
            let (bit, l, param) = self.excl_reads[k];
            if self.receives(x, l) {
                continue;
            }
            mask[bit as usize / 64] &= !(1u64 << (bit % 64));
            if s.get(live, bit) {
                self.relied[bit as usize] |= 1 << param;
            }
        }
        self.mark(x, s, live, &mask, NONE, Change::Call);
        self.heap = saved;
    }
}

/// The reads that keep a count, with what borrows now: per local how many
/// read it, and per value whether one reads it.
fn kept_reads(b: &Body, cx: &dyn OwnCx, t: &Tables, kept: &mut [u32], taken: &mut [bool]) {
    kept.fill(0);
    taken.fill(false);
    for i in 0..b.len() {
        if b.tags[i] != Tag::Read
            || !(t.keeps[i] || t.store_of(i) != NONE)
            || t.borrows[i]
            || !cx.counted(b.ty[i])
        {
            continue;
        }
        match b.view(b.data[i][0]).root {
            Root::Local(l) => kept[l as usize] += 1,
            Root::Value(v) => taken[v as usize] = true,
            _ => {}
        }
    }
}

/// Whether every `Drop` bit `bit` crossed empties what owns nothing.
fn crossed_ok(b: &Body, t: &Tables, bit: u32) -> bool {
    let lo = t.crossed.partition_point(|c| c.0 < bit);
    t.crossed[lo..]
        .iter()
        .take_while(|c| c.0 == bit)
        .all(|&(_, d)| match d {
            Dropped::Nothing => true,
            Dropped::Value(v) => {
                let v = v as usize;
                b.provider_ref(v)
                    || (t.vbit[v] != NONE && t.borrows[t.read[t.vbit[v] as usize] as usize])
            }
            Dropped::Local(l) => t.unowned[l as usize],
        })
}

/// Solves what depends on another decision, from everything that may borrow
/// down to what holds: a read's source crossed the `Drop` of a place that
/// must own nothing, a binding stays one only if no read takes a count from
/// it, and a parameter stays lent only if nothing keeps it.
fn solve(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables, list: &[(u32, usize)]) {
    for bit in 0..t.read.len() {
        let r = t.read[bit] as usize;
        // Either source may be the one that holds, until the solve knows.
        t.borrows[r] |= t.seen[t.vbit[r] as usize] && t.changes[bit] == Change::None;
    }
    let locals = b.local_ty.len();
    let lendable = matches!(
        b.kind,
        BodyKind::Fn | BodyKind::DeriveInstance | BodyKind::Default
    );
    let suspends = b.kind != BodyKind::Default && cx.suspending(b.item);
    let n = list.len();
    let mut open = vec![false; locals];
    if lendable {
        for &(l, _) in list {
            let li = l as usize;
            open[li] = t.stores[li] == 0
                && !t.captured[li]
                && !t.reached[li]
                && !t.changed[li]
                && cx.counted(b.local_ty[li]);
        }
    }
    let mut kept = vec![0u32; locals];
    let mut taken = vec![false; b.len()];
    t.crossed.sort_stable_by_key(|&(bit, d)| {
        (
            bit,
            match d {
                Dropped::Nothing => 0,
                Dropped::Value(v) => v + 1,
                Dropped::Local(l) => l + 1 + u32_of(b.len()),
            },
        )
    });
    t.crossed.dedup();
    // Bindings settle first: a read through one depends on how it ended.
    let mut settled = false;
    loop {
        let mut changed = false;
        kept_reads(b, cx, t, &mut kept, &mut taken);
        for &(l, at) in list {
            let li = l as usize;
            let lent = open[li]
                && (kept[li] == 0 && !suspends || cx.received(b.item, at, n) == Pass::Lent);
            if t.unowned[li] != lent {
                t.unowned[li] = lent;
                changed = true;
            }
        }
        for l in 0..locals {
            let bit = t.lbit[l];
            if bit != NONE && list.iter().all(|&(p, _)| p as usize != l) {
                t.unowned[l] = t.borrows[t.read[bit as usize] as usize];
            }
        }
        for k in 0..t.read.len() {
            let r = t.read[k] as usize;
            if !t.borrows[r] || t.vbit[r] != u32_of(k) {
                continue;
            }
            // What reads this value for a consumer needs a count. Where this
            // read could move, that count costs nothing here, and borrowing
            // would make it a retain there.
            let store = t.store_of(r);
            let bind = if store == NONE {
                None
            } else {
                t.binding(b, store)
            };
            if !settled && bind.is_none() {
                continue;
            }
            let needed = taken[r]
                || (t.alias[r] != NONE && taken[t.alias[r] as usize])
                || bind.is_some_and(|l| kept[l as usize] > 0);
            let root_owned = b
                .view(b.data[r][0])
                .local()
                .is_none_or(|m| !t.unowned[m as usize]);
            let mut blocked = needed && t.last_use[r] && root_owned;
            // A binding that borrows leads on to what it holds; one that owns
            // its value is the source.
            let mut bit = u32_of(k);
            while t.stops[bit as usize] != NONE
                && t.unowned[t.stops[bit as usize] as usize]
                && t.far[bit as usize] != NONE
            {
                bit = t.far[bit as usize];
            }
            blocked |= t.changes[bit as usize] != Change::None
                || !crossed_ok(b, t, bit)
                || (t.probe && t.relied[bit as usize] != 0);
            if blocked {
                t.borrows[r] = false;
                changed = true;
            }
        }
        if !changed {
            if settled {
                break;
            }
            settled = true;
        }
    }
    t.kept = kept;
    t.relies = 0;
    for bit in 0..t.read.len() {
        if t.changes[bit] == Change::None {
            t.relies |= t.relied[bit];
        }
    }
    // A value no instance copies is never copied: a match decides through
    // its scrutinee by borrowing it. There is no count to end or drop.
    for r in 0..b.len() {
        if b.tags[r] == Tag::Read
            && b.data[r][1] == ReadMode::Copy as u32
            && !t.keeps[r]
            && !cx.counted(b.ty[r])
            && cx.copies(b.ty[r]) != Tri::Yes
            && super::borrow::match_reference_read(b, topo, r, false)
        {
            t.borrows[r] = true;
        }
    }
}

/// Derives which reads of `b`, as insertion left it, borrow and which
/// parameters the body owns none of, into `t` ([`Tables::borrows`],
/// [`Tables::unowned`]). Reads only `b`.
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    last: &mut moves::Tables,
) {
    let (n, locals) = (b.len(), b.local_ty.len());
    t.reset(n, locals);
    fill(&mut t.last_use, n, false);
    if moves::last_uses(b, topo, cx, last) {
        for r in 0..n {
            t.last_use[r] = moves::last_use(last, r);
        }
    }
    let b = &*b;
    suites(b, t);
    scan(b, cx, t);
    uncopyable(b, cx, t);
    let flagged = b
        .sub_flags
        .first()
        .is_some_and(|f| f & sub_flags::EXCLUSIVE != 0);
    t.probe = !flagged && cx.summarizes();
    t.excl = (flagged || t.probe)
        && matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance)
        && !b.tags.iter().any(|&tag| is_suspension(tag));
    if t.excl {
        receivers(b, t);
    }
    let inputs = params(b, cx, t);
    candidates(b, cx, t);
    sources(b, cx, t);
    index_writes(b, cx, t);
    carries(b, cx, t);
    let mut back = std::mem::take(&mut t.back);
    backward(b, topo, &mut back, t);
    let roots = std::mem::take(&mut t.suite_roots);
    backward_blocks(b, topo, &mut back, t, &roots);
    t.suite_roots = roots;
    t.back = back;
    solve(b, topo, cx, t, &inputs);
}

/// One read the derivation and the old passes answer differently.
#[derive(Clone, Debug)]
pub struct Disagreement {
    pub inst: u32,
    pub old: &'static str,
    pub new: &'static str,
    /// The shape of the read and what stopped the derivation, for the table.
    pub why: String,
    pub place: String,
}

fn mode_name(mode: u32) -> &'static str {
    match mode {
        m if m == ReadMode::Copy as u32 => "copy",
        m if m == ReadMode::Move as u32 => "move",
        m if m == ReadMode::Borrow as u32 => "borrow",
        _ => "use",
    }
}

/// Applies a derivation to `b`: the modes, the rooted reads, the exit rows,
/// and the `Drop`s of what owns nothing now, as edits.
pub(crate) fn apply(b: &mut Body, topo: &Topology, t: &Tables, edits: &mut splice::Tables) {
    edits.clear();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Drop {
            let p = b.view(b.data[i][0]);
            let gone = p.whole()
                && match p.root {
                    Root::Value(v) => {
                        let bit = t.vbit[v as usize];
                        bit != NONE && t.borrows[t.read[bit as usize] as usize]
                    }
                    Root::Local(l) => t.unowned[l as usize],
                    _ => false,
                };
            if gone {
                edits.remove(u32_of(i));
            }
        }
    }
    for r in 0..b.len() {
        if t.borrows[r] {
            b.data[r][1] = ReadMode::Borrow as u32;
        }
    }
    super::borrow::flatten_match_reference_reads(b, topo);
    let unowned = &t.unowned;
    b.exit_drops.retain(|(_, l)| !unowned[l.idx()]);
}

/// The kept bits of `b`'s summary, and its borrows bits
/// ([`crate::own::Summary`]). `None` for a body that is no function's or that
/// suspends.
pub(crate) fn kept(b: &Body, cx: &dyn OwnCx, t: &Tables) -> Option<(u64, u64)> {
    let callable = match b.kind {
        BodyKind::Fn | BodyKind::DeriveInstance => !cx.suspending(b.item),
        BodyKind::Default => true,
        _ => false,
    };
    if !callable {
        return None;
    }
    let (mut kept, mut borrows) = (0, 0);
    let params = super::params(b);
    let n = params.len();
    for (k, l) in params.into_iter().enumerate() {
        let l = l as usize;
        let counted = cx.counted(b.local_ty[l]);
        if k < 64 {
            if counted && (!t.unowned[l] || t.kept[l] > 0) {
                kept |= 1 << k;
            } else if counted && cx.received(b.item, k, n) == Pass::Owned {
                borrows |= 1 << k;
            }
        }
    }
    Some((kept, borrows))
}

/// Compares the body `old` the old passes left with the body `new` the
/// derivation and the move walk left, into `out`: how many counted reads
/// were compared.
pub(crate) fn agreement(
    old: &Body,
    new: &Body,
    cx: &dyn OwnCx,
    t: &Tables,
    out: &mut Vec<Disagreement>,
) -> u32 {
    out.clear();
    let reads = (0..old.len())
        .filter(|&i| old.tags[i] == Tag::Read && cx.counted(old.ty[i]))
        .count();
    for i in 0..old.len() {
        let (o, m) = (old.data[i][1], new.data[i][1]);
        match old.tags[i] {
            Tag::Read if o != m => {
                let p = new.view(new.data[i][0]);
                let (derefs, elem) = match p.root {
                    Root::Const(_) => (0, false),
                    _ => p.kinds(new),
                };
                let root = match p.root {
                    Root::Local(l) if new.local_flags[l as usize] & local_flags::PARAM != 0 => {
                        "param"
                    }
                    Root::Local(_) => "local",
                    Root::Value(_) => "value",
                    Root::Global(..) => "global",
                    Root::Ptr(_) => "ptr",
                    _ => "other",
                };
                let path = if p.whole() {
                    "whole"
                } else if derefs > 0 {
                    "deref"
                } else if elem {
                    "elem"
                } else {
                    "part"
                };
                let bit = t.vbit[i];
                let stop = if bit == NONE {
                    if !cx.counted(new.ty[i]) {
                        "uncounted"
                    } else if t.keeps[i] {
                        "kept"
                    } else if t.in_suite[i] {
                        "suite"
                    } else {
                        "shape"
                    }
                } else if !t.seen[bit as usize] {
                    "unseen"
                } else {
                    match t.changes[bit as usize] {
                        Change::None if t.borrows[i] => "free",
                        Change::None => "solved",
                        Change::Write => "write",
                        Change::Reached => "reached",
                        Change::Heap => "heap",
                        Change::Call => "call",
                        Change::Suspension => "suspension",
                    }
                };
                // What a read the solve refused crossed.
                let mut place = new
                    .decode_place(new.data[i][0])
                    .map(|p| p.to_string())
                    .unwrap_or_default();
                if stop == "solved" {
                    for &(c, d) in t.crossed.iter().filter(|c| c.0 == bit) {
                        let _ = c;
                        match d {
                            Dropped::Value(v) => {
                                let _ = write!(place, " crossed %{v}");
                            }
                            Dropped::Local(l) => {
                                let _ = write!(place, " crossed ${l}");
                            }
                            Dropped::Nothing => {}
                        }
                    }
                }
                out.push(Disagreement {
                    inst: u32_of(i),
                    old: mode_name(o),
                    new: mode_name(m),
                    why: format!("{root}-{path} {stop}"),
                    place,
                });
            }
            Tag::Ref if (o ^ m) & ref_word::TAKES != 0 => out.push(Disagreement {
                inst: u32_of(i),
                old: if o & ref_word::TAKES != 0 {
                    "take"
                } else {
                    "keep"
                },
                new: if m & ref_word::TAKES != 0 {
                    "take"
                } else {
                    "keep"
                },
                why: "ref-take".into(),
                place: String::new(),
            }),
            _ => {}
        }
    }
    let (mut a, mut c): (Vec<u32>, Vec<u32>) = (
        old.exit_drops.iter().map(|e| e.1.raw()).collect(),
        new.exit_drops.iter().map(|e| e.1.raw()).collect(),
    );
    a.sort_stable();
    c.sort_stable();
    for &l in a.iter().filter(|l| !c.contains(l)) {
        out.push(Disagreement {
            inst: l,
            old: "owned",
            new: "unowned",
            why: "exit-row".into(),
            place: format!("${l}"),
        });
    }
    for &l in c.iter().filter(|l| !a.contains(l)) {
        out.push(Disagreement {
            inst: l,
            old: "unowned",
            new: "owned",
            why: "exit-row".into(),
            place: format!("${l}"),
        });
    }
    u32_of(reads)
}
