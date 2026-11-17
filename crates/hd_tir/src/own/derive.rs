//! One derivation of which reads borrow
//! (`docs/compiler/ownership.md#ownership-per-instance`, "The One Place Each Count
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
//! A **binding** is a local whose one `Store` is of a read that borrows: it
//! owns nothing, so its `Drop`s go and it leaves the exit set, and its reads
//! lead on to what it holds. A read through a binding that owns its value has
//! the binding as its source. A parameter its caller keeps stays unowned while
//! no read of it keeps a count. A read whose reader needs a count, and that
//! could move, moves rather than borrow: the count then costs nothing.
//!
//! Nothing is spliced here: the `Drop`s that go are edits for the pipeline's one
//! rebuild, and the move pass after this reads the body with them still in place.

use hd_base::SliceSort as _;

use super::flow::{Backward, BitArena, Flow, backward, backward_blocks};
use super::{Loc, OwnCx, Pass, Role, Tri, consumes, fill, operands, splice};
use crate::ir::{
    Body, BodyKind, Callee, DEREF, ELEM, IntrinsicOp, NONE, PlaceRoot, Proj, ReadMode, Ref, Root,
    Tag, Topology, inside, is_suspension, local_flags, ref_word, sub_flags, u32_of,
};

/// The longest chain of roots a read's source is followed through.
const DEPTH: usize = 8;

/// One place a read's source goes through: a local (`key` < the local
/// count) or a value (`key` = local count + value), the read's own steps
/// there (`own` codes of the path from `start`), then the steps below them
/// (`len` codes in all), and whether the steps there go behind a reference
/// or into a buffer.
#[derive(Clone, Copy)]
struct Link {
    key: u32,
    start: u32,
    own: u32,
    len: u32,
    behind: bool,
}

/// What a read's source is ([`Tables::source`]).
#[derive(Clone, Copy, Default)]
struct Source {
    /// Ends at a root the walk can reason about.
    known: bool,
    /// Calls and writes through references may reach it.
    heap: bool,
    /// In exclusive mode, the one reference it is reached by is parameter k
    /// declared `&T` or `&mut T`: (its local, k).
    excl: Option<(u32, u32)>,
    /// All or a field path of a parameter the body never stores, whose
    /// count the frame holds across a suspension point.
    frame: bool,
}

/// What a `Drop` empties, which is no write if that owns nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dropped {
    Nothing,
    Value(u32),
    Local(u32),
}

/// One write at one place: its steps below that place, the last step that
/// goes behind a reference or into a buffer (`cut`), and whether the place is
/// the write's own.
#[derive(Clone, Copy)]
struct Write {
    key: u32,
    start: u32,
    len: u32,
    cut: u32,
    direct: bool,
    dropped: Dropped,
}

/// What an instruction may change besides its own place: whether a reference
/// can reach what it writes, whether it is a call that may write, and the value
/// its place is rooted at.
#[derive(Clone, Copy)]
struct Effect {
    reach: bool,
    may: bool,
    through: u32,
}

/// The facts of one value.
#[derive(Clone, Copy)]
struct Value {
    uses: u32,
    /// Every use only reads through this value or ends its loan.
    read_through: bool,
    user: u32,
    stored: u32,
    alias: u32,
    /// Its read's first bit, also for a coercion that keeps its values.
    bit: u32,
    keeps: bool,
    dropped: bool,
    /// A cleanup suite it is not inside of uses it.
    in_suite: bool,
    /// A reader needs a count, and this read may give it by moving: it stays
    /// a copy, which the move pass moves at its last use.
    moves: bool,
    borrows: bool,
    /// The alias changes aggregate layout while preserving RC identity.
    rewrapped: bool,
}

/// The facts of one local.
#[derive(Clone, Copy)]
struct Local {
    stores: u32,
    /// The read it may hold as a binding, before that is decided, and its bit.
    holds: u32,
    bit: u32,
    /// Exclusive mode: its index, as a `&T` or `&mut T` parameter whose
    /// reference only calls take.
    excl: u32,
    /// The reads of it that keep a count, once solved.
    kept: u32,
    reached: bool,
    /// Every address is immutable and dies with one direct call.
    shared_lends_only: bool,
    /// Every address is a frame-local immutable view, read through or
    /// lent to calls whose results carry no local reference.
    binding_lends_only: bool,
    escapes: bool,
    changed: bool,
    captured: bool,
    fresh: bool,
    dropped: bool,
    /// The body owns none of it, once solved.
    unowned: bool,
    /// Its binding's projections do not describe the held read's layout.
    rewrapped: bool,
}

/// One candidate's source: the read, the binding it stores into, the bit
/// that follows one more binding to what it holds, and the binding this bit's
/// source stops at. A binding that owns its value is the source itself; only
/// one that borrows leads on.
#[derive(Clone, Copy)]
struct Bit {
    read: u32,
    bind: u32,
    far: u32,
    stop: u32,
    seen: bool,
    relied: u64,
}

/// The tables of one derivation.
#[derive(Default)]
pub(crate) struct Tables {
    v: Vec<Value>,
    l: Vec<Local>,
    bits: Vec<Bit>,
    // Masks over the bits: what a write has changed, whose source calls that
    // write reach, whose source a write through a reference can reach, and
    // whose source a suspension point can change.
    changed: Vec<u64>,
    heap: Vec<u64>,
    exposed: Vec<u64>,
    unframed: Vec<u64>,
    mask: Vec<u64>,
    /// (bit, what a `Drop` it crossed empties): a value, or the local count
    /// past the values plus a local.
    crossed: Vec<(u32, u32)>,
    excl_reads: Vec<(u32, u32, u32)>,
    receives: Vec<(u32, u32)>,
    // The links of each bit's source by key, in rows.
    key_start: Vec<u32>,
    key_rows: Vec<(u32, Link)>,
    paths: Vec<u32>,
    links: Vec<Link>,
    chain: Vec<u32>,
    stop: u32,
    // What each instruction writes, in rows.
    write_start: Vec<u32>,
    writes: Vec<Write>,
    capture_drops: super::capture_env::ReferenceDrops,
    wpaths: Vec<u32>,
    effects: Vec<Effect>,
    // Per `Scope` and `Defer`: the instructions of its suites, what it uses,
    // and what suites around the walk's place keep live.
    suites: Vec<(u32, u32, u32)>,
    suite_insts: Vec<u32>,
    suite_roots: Vec<u32>,
    suite_bits: Vec<(u32, u32)>,
    held: Vec<u32>,
    // Each use of what may point into a frame local a mutable reference can
    // write, as (instruction, local): only there can it change that local.
    frame_uses: Vec<(u32, u32)>,
    // Each use of what may point into what a binding holds, as (instruction,
    // bit): a reference into it holds no count, so that use keeps it live.
    root_uses: Vec<(u32, u32)>,
    reference_roots: Vec<u32>,
    // The counted reads that may keep a count.
    takers: Vec<u32>,
    // Per label: its wrapper `Scope` ([`wrappers`]), or `NONE`.
    wrapper: Vec<u32>,
    scope_of: Vec<u32>,
    back: Flow,
    ops: Vec<super::Operand>,
    stack: Vec<u32>,
    excl: bool,
    probe: bool,
    pub(crate) relies: u64,
}

fn get(mask: &[u64], bit: u32) -> bool {
    mask[bit as usize / 64] & (1 << (bit % 64)) != 0
}

fn put(mask: &mut [u64], bit: u32) {
    mask[bit as usize / 64] |= 1 << (bit % 64);
}

/// Everything live in `row` and in `mask` has changed, but bit `except`.
fn mark(changed: &mut [u64], row: &[u64], mask: &[u64], except: u32) {
    for (k, c) in changed.iter_mut().enumerate() {
        let spared = if except != NONE && except as usize / 64 == k {
            1u64 << (except % 64)
        } else {
            0
        };
        *c |= row[k] & mask[k] & !spared;
    }
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        let value = Value {
            uses: 0,
            read_through: true,
            user: NONE,
            stored: NONE,
            alias: NONE,
            bit: NONE,
            keeps: false,
            dropped: false,
            in_suite: false,
            moves: false,
            borrows: false,
            rewrapped: false,
        };
        let local = Local {
            stores: 0,
            holds: NONE,
            bit: NONE,
            excl: NONE,
            kept: 0,
            reached: false,
            shared_lends_only: true,
            binding_lends_only: true,
            escapes: false,
            changed: false,
            captured: false,
            fresh: true,
            dropped: false,
            unowned: false,
            rewrapped: false,
        };
        let effect = Effect {
            reach: false,
            may: false,
            through: NONE,
        };
        fill(&mut self.v, n, value);
        fill(&mut self.l, locals, local);
        fill(&mut self.effects, n, effect);
        self.bits.clear();
        self.crossed.clear();
        self.excl_reads.clear();
        self.receives.clear();
        self.key_rows.clear();
        self.paths.clear();
        self.writes.clear();
        self.wpaths.clear();
        self.suites.clear();
        self.suite_insts.clear();
        self.suite_roots.clear();
        self.suite_bits.clear();
        self.held.clear();
        self.frame_uses.clear();
        self.root_uses.clear();
        self.relies = 0;
    }

    /// Follows the place at `word` into `self.links`, down to what it finally
    /// reads: through the reads its root values made, and through `levels`
    /// bindings to the reads they hold.
    fn source(&mut self, b: &Body, word: u32, mut levels: u32, mut handle: bool) -> Source {
        self.links.clear();
        self.chain.clear();
        self.stop = NONE;
        let locals = u32_of(b.local_ty.len());
        let mut out = Source::default();
        let (mut word, mut below_start, mut below) = (word, 0, 0);
        let (mut derefs, mut elem, mut first_deref) = (0, false, false);
        let (mut seen_deref, mut above, mut pinned) = (false, false, false);
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
            above |= seen_deref && own > 0;
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
            };
            (below_start, below) = (start, own + below);
            match p.root {
                Root::Local(l) => {
                    self.links.push(Link { key: l, ..link });
                    let f = self.l[l as usize];
                    if f.holds != NONE {
                        if levels > 0 {
                            levels -= 1;
                            if f.rewrapped {
                                // RC identity is not field layout identity.
                                // Protect the complete source, conservatively,
                                // instead of interpreting wrapper projections
                                // as projections of the source's type.
                                below = 0;
                            }
                            word = b.data[f.holds as usize][0];
                            continue;
                        }
                        self.stop = l;
                    }
                    let param = b.local_flags[l as usize] & local_flags::PARAM != 0;
                    out.known = !f.escapes;
                    out.heap = derefs > 0 || (elem && (param || !f.fresh || f.reached));
                    let one = derefs == 1 && first_deref && !above;
                    out.excl =
                        (one && f.excl != NONE && (!param || f.stores == 0)).then_some((l, f.excl));
                    out.frame = param && f.stores == 0 && derefs == 0 && !elem;
                    return out;
                }
                Root::Value(v) => {
                    if first
                        && let Some(&local) = self.reference_roots.get(v as usize)
                        && local != NONE
                    {
                        // Loading a value through this address depends on
                        // its backing frame owner, not on the address
                        // loan's end. Keep heap/write checks conservative.
                        below = 0;
                        word = local | crate::ir::place::BARE_LOCAL;
                        continue;
                    }
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
                    if let Some(read) = b.forwarded_read(v) {
                        // A projected borrow may escape its constructor.
                        // Keep the complete RC source alive, without using
                        // the constructor's field paths in its layout.
                        below = 0;
                        word = b.data[read as usize][0];
                        continue;
                    }
                    if let Some(&local) = self.reference_roots.get(v as usize)
                        && local != NONE
                    {
                        below = 0;
                        word = local | crate::ir::place::BARE_LOCAL;
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
                    self.links.push(link);
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
    fn write_links(&mut self, dropped: Dropped) {
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
                direct: k == 0,
                dropped,
            });
        }
    }

    /// The `Store` that takes read `r`'s value: its own, or that of the one
    /// coercion keeping its values that is its only use.
    fn store_of(&self, r: usize) -> u32 {
        let v = self.v[r];
        if v.alias == NONE {
            return v.stored;
        }
        let c = self.v[v.alias as usize];
        // A lending constructor (implicit Some) ends its operand copy
        // separately. Borrowing forwards that end to the aggregate binding.
        if v.uses == 1
            && c.uses == 1
            && c.alias == NONE
            && (!v.dropped || v.rewrapped)
            && !c.dropped
        {
            c.stored
        } else {
            NONE
        }
    }

    /// The local `Store` `s` makes a binding of: all of a local that is no
    /// parameter, that no reference or closure reaches, stored once.
    fn binding(&self, b: &Body, s: u32) -> Option<u32> {
        let p = b.view(b.data[s as usize][0]);
        let l = p.local()?;
        let f = self.l[l as usize];
        (p.whole()
            && b.local_flags[l as usize] & local_flags::PARAM == 0
            && (!f.reached || f.binding_lends_only)
            && !f.changed
            && !f.captured
            && f.stores == 1
            && f.dropped)
            .then_some(l)
    }

    fn set(&self, bit: u32, s: &mut BitArena, live: usize, on: bool) {
        let mut at = bit;
        while at != NONE {
            s.set(live, at, on);
            at = self.bits[at as usize].far;
        }
    }

    fn use_value(&self, v: u32, s: &mut BitArena, live: usize) {
        let Some(f) = self.v.get(v as usize) else {
            return;
        };
        if f.bit != NONE {
            self.set(f.bit, s, live, true);
        }
    }

    /// What instruction `x` may change, against every value `live`.
    fn change_at(&mut self, x: u32, tag: Tag, s: &BitArena, live: usize) {
        let i = x as usize;
        let capture_drop = self.capture_drops.at(i);
        for k in self.write_start[i]..self.write_start[i + 1] {
            let w = self.writes[k as usize];
            let (lo, hi) = (
                self.key_start[w.key as usize] as usize,
                self.key_start[w.key as usize + 1] as usize,
            );
            for row in lo..hi {
                let (bit, link) = self.key_rows[row];
                if !s.get(live, bit) {
                    continue;
                }
                let at = link.start as usize;
                let own = &self.paths[at..at + link.own as usize];
                let below = &self.paths[at..at + link.len as usize];
                let path = &self.wpaths[w.start as usize..(w.start + w.len) as usize];
                let hit = (w.direct && overlap(own, path))
                    || (overlap(below, path) && (w.cut == NONE || link.len > w.cut));
                if !hit {
                    continue;
                }
                match w.dropped {
                    Dropped::Nothing => put(&mut self.changed, bit),
                    Dropped::Value(v) => self.crossed.push((bit, v)),
                    Dropped::Local(l) => {
                        if !(capture_drop && own.first() == Some(&DEREF)) {
                            self.crossed.push((bit, u32_of(self.v.len()) + l));
                        }
                    }
                }
            }
        }
        let e = self.effects[i];
        let except = self.v.get(e.through as usize).map_or(NONE, |f| f.bit);
        let row = s.row(live);
        if e.reach {
            mark(&mut self.changed, row, &self.exposed, except);
        }
        if e.may {
            // Exclusive mode: the call reaches the place only through the
            // reference, so only when it receives the reference itself.
            self.mask.clone_from(&self.heap);
            for &(bit, l, param) in &self.excl_reads {
                let k = self.receives.partition_point(|&(c, _)| c < x);
                let takes = self.receives[k..]
                    .iter()
                    .take_while(|&&(c, _)| c == x)
                    .any(|&(_, m)| m == l);
                if !takes {
                    self.mask[bit as usize / 64] &= !(1u64 << (bit % 64));
                    if s.get(live, bit) {
                        self.bits[bit as usize].relied |= 1 << param;
                    }
                }
            }
            mark(&mut self.changed, row, &self.mask, NONE);
        }
        if is_suspension(tag) {
            mark(&mut self.changed, row, &self.unframed, NONE);
        }
    }
}

/// The facts of each value and local: uses, drops, stores.
fn scan(b: &Body, cx: &dyn OwnCx, t: &mut Tables, constructors: &[u32]) {
    let n = b.len();
    wrappers(b, t, constructors);
    let recursive = super::RecursiveBindings::new(b);
    let mut ops = std::mem::take(&mut t.ops);
    let mut through = super::promote::ReadThroughUses::default();
    for &(_, capture) in &b.inlined_shared {
        let local = &mut t.l[b.cap_local[capture as usize].idx()];
        local.captured = true;
        local.reached = true;
        local.escapes = true;
    }
    for (i, &constructor) in constructors.iter().enumerate() {
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        let local = b.local_in_place(a).map(hd_base::LocalId::idx);
        match (tag, local) {
            (Tag::Drop, _) => {
                let p = b.view(a);
                match p.root {
                    Root::Value(v) if p.whole() => t.v[v as usize].dropped = true,
                    Root::Local(l) if p.whole() => t.l[l as usize].dropped = true,
                    _ => {}
                }
            }
            (Tag::Store, Some(l)) => {
                t.l[l].stores += 1;
                // A fresh value shares its header with no place.
                t.l[l].fresh &= Ref(w).as_inst().is_none_or(|_| {
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
            }
            (Tag::Ref, Some(l)) => t.l[l].reached = true,
            (Tag::IsUnique, Some(l)) => t.l[l].changed = true,
            (Tag::Closure, _) => {
                let rec = b.record(w);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    let l = b.cap_local[c].idx();
                    t.l[l].captured = true;
                    if b.cap_mode[c].shares()
                        && recursive.self_local(i).is_none_or(|s| s.idx() != l)
                    {
                        t.l[l].reached = true;
                        t.l[l].escapes = true;
                    }
                }
            }
            _ => {}
        }
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role == Role::Dropped {
                continue;
            }
            let v = op.value as usize;
            t.v[v].read_through &= through.permits(b, i, op.value);
            t.v[v].uses += 1;
            t.v[v].user = u32_of(i);
            let keeps = match op.role {
                Role::Consume | Role::Lend if constructor == op.value && t.v[v].alias == NONE => {
                    t.v[v].alias = u32_of(i);
                    t.v[v].rewrapped = true;
                    false
                }
                Role::Consume if b.same_value_coercion(i) && t.v[v].alias == NONE => {
                    t.v[v].alias = u32_of(i);
                    false
                }
                Role::Consume if tag == Tag::Break && t.v[v].alias == NONE => {
                    let s = t.wrapper[b.data[i][0] as usize];
                    if s == NONE {
                        true
                    } else {
                        t.v[v].alias = s;
                        false
                    }
                }
                Role::Consume
                    if tag == Tag::Store && op.at == Loc::Data(1) && t.v[v].stored == NONE =>
                {
                    t.v[v].stored = u32_of(i);
                    false
                }
                // A trait value a `&dyn` is made of lends what it reads: the
                // `&dyn` counts nothing, and the checker keeps its referent
                // unchanged while it lives (`ref.count.local`).
                Role::Consume if tag == Tag::Coerce && super::dyn_view(b, cx, i).is_some() => false,
                Role::Consume => true,
                Role::Tail => consumes(b, cx, i, op, NONE),
                Role::Arg(_) => consumes(b, cx, i, op, NONE),
                Role::Dropped => false,
                // A reference into the value's own storage outlives the read;
                // one to a referent does not, nor does a loop's element
                // reference into the buffer the read pins.
                Role::Lend if tag == Tag::Ref => {
                    let p = b.view(a);
                    (matches!(p.root, Root::Const(_)) || p.first(b) != Some(Proj::Deref))
                        && !b.pinned_element_ref(i)
                }
                Role::Lend => !matches!(
                    tag,
                    Tag::Read
                        | Tag::Return
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
                        | Tag::ContextNew
                        | Tag::CopyStruct
                ),
            };
            t.v[v].keeps |= keeps;
        }
    }
    t.ops = ops;
    let (counts, last): (Vec<u32>, Vec<u32>) = t.v.iter().map(|f| (f.uses, f.user)).unzip();
    let calls = local_lending_calls(b, cx, &counts, &last);
    t.reference_roots = frame_reference_roots(b, constructors, || {});
    let frame_lends = frame_lending_roots(b, cx, &t.reference_roots);
    for (i, &call) in calls.iter().enumerate() {
        if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(b.data[i][0])
        {
            // Frame planning already proved that every reference derived
            // from this address stays inside the invocation, including
            // references held in local aggregates and dependent results.
            let lends = call != NONE || t.v[i].read_through || b.data[i][1] & ref_word::FRAME != 0;
            t.l[l.idx()].shared_lends_only &= b.data[i][1] & ref_word::MUT == 0 && lends;
            t.l[l.idx()].binding_lends_only &= t.reference_roots[i] != NONE && frame_lends[l.idx()];
            if b.data[i][1] & ref_word::FRAME == 0 && !lends {
                t.l[l.idx()].escapes = true;
            }
        }
    }
    // A value a coercion consumes is kept where the coercion's value is.
    for v in (0..n).rev() {
        let c = t.v[v].alias;
        if c != NONE {
            let f = t.v[c as usize];
            let bound =
                f.stored != NONE && t.store_of(v) != NONE && t.binding(b, f.stored).is_some();
            t.v[v].keeps |= f.keeps || (f.stored != NONE && !bound);
        }
    }
    collapse_aliases(&mut t.v, || {});
    // A value no instance may copy needs a count wherever a use keeps it or a
    // store takes a part of it, which only a move gives; so does the read the
    // part leaves.
    for i in (0..n).rev() {
        if b.tags[i] != Tag::Read
            || b.data[i][1] != ReadMode::Copy as u32
            || !cx.counted(b.ty[i])
            || cx.copies(b.ty[i]) == Tri::Yes
        {
            continue;
        }
        let p = b.view(b.data[i][0]);
        let part = !p.whole() && p.in_place(b);
        if t.v[i].keeps || (part && t.store_of(i) != NONE) {
            t.v[i].keeps = true;
            if let Root::Value(v) = p.root
                && p.in_place(b)
                && b.tags[v as usize] == Tag::Read
            {
                t.v[v as usize].keeps = true;
            }
        }
    }
}

/// Unary forwarding scopes/coercions preserve RC identity (Swift SIL ARC).
/// Collapse single-use, undropped intermediates once, in reverse SSA order.
fn collapse_aliases(values: &mut [Value], mut visit: impl FnMut()) {
    for v in (0..values.len()).rev() {
        visit();
        let c = values[v].alias;
        if c != NONE {
            let f = values[c as usize];
            if f.uses == 1 && (!f.dropped || f.rewrapped) && f.stored == NONE && f.alias != NONE {
                values[v].alias = f.alias;
                values[v].rewrapped |= f.rewrapped;
            }
        }
    }
}

/// Each label's single-producing-exit `Scope`, `NONE` for none. The shared
/// RC source index proves its result even when loan cleanup follows the
/// producing `Break`. A read there is used where the `Scope` is.
fn wrappers(b: &Body, t: &mut Tables, sources: &[u32]) {
    wrappers_with(b, t, sources, || {});
}

fn wrappers_with(b: &Body, t: &mut Tables, sources: &[u32], mut visit: impl FnMut()) {
    let labels = b.label_inst.len();
    fill(&mut t.wrapper, labels, NONE);
    fill(&mut t.scope_of, b.len(), NONE);
    for (i, &source) in sources.iter().enumerate() {
        visit();
        if b.tags[i] == Tag::Scope && source != NONE {
            t.scope_of[b.data[i][0] as usize] = u32_of(i);
        }
    }
    for l in 0..labels {
        visit();
        let blk = b.label_inst[l] as usize;
        t.wrapper[l] = t.scope_of.get(blk).copied().unwrap_or(NONE);
    }
}

/// The cleanup suites: which values one reaches from outside it, and the
/// instructions each holds. A suite runs at its scope's exits, which the walk
/// does not follow.
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
            operands(b, t.suite_insts[k as usize] as usize, &mut ops);
            for op in &ops {
                if mine[op.value as usize] != u32_of(x) {
                    t.v[op.value as usize].in_suite = true;
                }
            }
        }
    }
    t.stack = stack;
    t.ops = ops;
}

/// Exclusive mode: the calls that receive a parameter's reference, and the
/// parameters whose reference something else takes; then which parameters
/// are read through such a reference.
fn receivers(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let whole = |v: usize| {
        let p = (b.tags[v] == Tag::Read).then(|| b.view(b.data[v][0]))?;
        match p.root {
            Root::Local(l) if p.whole() => Some(l),
            _ => None,
        }
    };
    let mut other = vec![false; b.local_ty.len()];
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..b.len() {
        let call = match b.tags[i] {
            Tag::Call | Tag::CallValue | Tag::CallHost | Tag::DefaultCall => true,
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
                other[l as usize] = true;
            }
        }
    }
    t.ops = ops;
    let list = super::params(b);
    for (at, &l) in list.iter().enumerate() {
        if at < 64 && !other[l as usize] && cx.local_ref_param(b.item, at, list.len()) {
            t.l[l as usize].excl = u32_of(at);
        }
    }
}

/// Numbers the reads that may borrow: counted, whose every use lends or
/// stores them into a binding. Their aliases and bindings share their bit, and
/// a read has one more bit per binding its source may go through.
fn candidates(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    for r in 0..b.len() {
        if b.tags[r] != Tag::Read
            || b.data[r][1] != ReadMode::Copy as u32
            || t.v[r].keeps
            || !cx.counted(b.ty[r])
        {
            continue;
        }
        let f = t.v[r];
        let store = t.store_of(r);
        let alias = if store == NONE { f.alias } else { NONE };
        if alias != NONE && !t.v[alias as usize].dropped {
            continue;
        }
        let bound = (store != NONE).then(|| t.binding(b, store)).flatten();
        let plain = alias != NONE || (store == NONE && f.dropped && f.stored == NONE);
        if !plain && (f.dropped && !f.rewrapped || bound.is_none() || f.uses != 1) {
            continue;
        }
        let bit = u32_of(t.bits.len());
        t.bits.push(Bit {
            read: u32_of(r),
            bind: bound.unwrap_or(NONE),
            far: NONE,
            stop: NONE,
            seen: false,
            relied: 0,
        });
        t.v[r].bit = bit;
        if f.alias != NONE {
            t.v[f.alias as usize].bit = bit;
        }
        if let (false, Some(l)) = (plain, bound) {
            t.l[l as usize].bit = bit;
            t.l[l as usize].holds = u32_of(r);
            t.l[l as usize].rewrapped = f.rewrapped;
        }
    }
    for bit in 0..t.bits.len() {
        let r = t.bits[bit].read;
        let mut word = b.data[r as usize][0];
        let mut at = bit;
        for _ in 0..DEPTH {
            word = match b.view(word).root {
                Root::Local(l) if t.l[l as usize].holds != NONE => {
                    let next = u32_of(t.bits.len());
                    t.bits[at].far = next;
                    at = next as usize;
                    t.bits.push(Bit {
                        read: r,
                        bind: NONE,
                        far: NONE,
                        stop: NONE,
                        seen: false,
                        relied: 0,
                    });
                    b.data[t.l[l as usize].holds as usize][0]
                }
                Root::Value(v) => {
                    if b.tags[v as usize] == Tag::Read {
                        b.data[v as usize][0]
                    } else if let Some(read) = b.forwarded_read(v) {
                        b.data[read as usize][0]
                    } else if t.reference_roots[v as usize] != NONE {
                        t.reference_roots[v as usize] | crate::ir::place::BARE_LOCAL
                    } else {
                        break;
                    }
                }
                _ => break,
            };
        }
    }
    // What each suite uses, by bit: read through a binding, or an operand.
    let mut ops = std::mem::take(&mut t.ops);
    for k in 0..t.suites.len() {
        let (x, from, to) = t.suites[k];
        for j in from..to {
            let i = t.suite_insts[j as usize] as usize;
            if matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Store)
                && let Some(l) = b.view(b.data[i][0]).local()
                && t.l[l as usize].bit != NONE
            {
                t.suite_bits.push((x, t.l[l as usize].bit));
            }
            operands(b, i, &mut ops);
            for op in &ops {
                let f = t.v[op.value as usize];
                if f.in_suite && f.bit != NONE {
                    t.suite_bits.push((x, f.bit));
                }
            }
        }
    }
    t.ops = ops;
    let words = t.bits.len().div_ceil(64).max(1);
    for mask in [&mut t.changed, &mut t.heap, &mut t.exposed, &mut t.unframed] {
        fill(mask, words, 0);
    }
    t.back.arena.words = words;
}

/// Follows each bit's source: its links by key, its masks.
fn sources(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let locals = u32_of(b.local_ty.len());
    for bit in 0..u32_of(t.bits.len()) {
        let r = t.bits[bit as usize].read as usize;
        let (mut before, mut last) = (None, None);
        for step in b.view(b.data[r][0]).steps(b) {
            (before, last) = (last, Some(step));
        }
        // A trait value read through a reference or a counted pointer is
        // that reference's words, in a local or a part: its object is held
        // by the reference's count.
        let handle = last == Some(Proj::Deref)
            && matches!(
                before,
                None | Some(Proj::Field(_) | Proj::Elem(_) | Proj::Variant(_))
            )
            && cx.trait_value(b.ty[r]);
        // This bit's place in its read's list says how many bindings it
        // goes through.
        let (mut levels, mut at) = (0, t.v[r].bit);
        while at != bit {
            at = t.bits[at as usize].far;
            levels += 1;
        }
        let source = t.source(b, b.data[r][0], levels, handle);
        t.bits[bit as usize].stop = t.stop;
        if !source.known {
            put(&mut t.changed, bit);
            continue;
        }
        if source.heap {
            put(&mut t.heap, bit);
        }
        if !source.frame {
            put(&mut t.unframed, bit);
        }
        if source.heap || t.links.iter().any(|l| l.behind) {
            put(&mut t.exposed, bit);
        }
        if let Some((l, k)) = source.excl {
            t.excl_reads.push((bit, l, k));
        }
        let shift = u32_of(t.paths.len());
        t.paths.extend_from_slice(&t.chain);
        for k in 0..t.links.len() {
            let link = t.links[k];
            if link.key != NONE {
                let start = link.start + shift;
                t.key_rows.push((bit, Link { start, ..link }));
            }
        }
    }
    // Rows by key, keeping each key's bits in order.
    let keys = locals as usize + b.len();
    fill(&mut t.key_start, keys + 2, 0);
    for &(_, l) in &t.key_rows {
        t.key_start[l.key as usize + 2] += 1;
    }
    for k in 2..keys + 2 {
        t.key_start[k] += t.key_start[k - 1];
    }
    let rows = std::mem::take(&mut t.key_rows);
    t.key_rows.clone_from(&rows);
    for &row in &rows {
        let at = &mut t.key_start[row.1.key as usize + 1];
        t.key_rows[*at as usize] = row;
        *at += 1;
    }
}

/// What each instruction writes.
fn index_writes(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    fill(&mut t.write_start, n + 1, 0);
    for i in 0..n {
        t.write_start[i] = u32_of(t.writes.len());
        let k = t.frame_uses.partition_point(|u| (u.0 as usize) < i);
        for u in k..t.frame_uses.len() {
            let (at, key) = t.frame_uses[u];
            if at as usize != i {
                break;
            }
            t.writes.push(Write {
                key,
                start: 0,
                len: 0,
                cut: NONE,
                direct: true,
                dropped: Dropped::Nothing,
            });
        }
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        let mutable_ref = tag == Tag::Ref && w & ref_word::MUT != 0;
        let moved = tag == Tag::Read && w == ReadMode::Move as u32;
        if mutable_ref || moved || matches!(tag, Tag::Store | Tag::Drop | Tag::IsUnique) {
            let p = b.view(a);
            let (derefs, elem) = match p.root {
                Root::Const(_) => (0, false),
                _ => p.kinds(b),
            };
            t.effects[i].reach = derefs > 0
                || elem
                || match p.root {
                    Root::Local(l) => t.l[l as usize].escapes,
                    Root::Value(_) => false,
                    _ => true,
                };
            t.effects[i].through = p.value().unwrap_or(NONE);
            t.source(b, a, 0, false);
            if mutable_ref || tag == Tag::Store {
                t.write_links(Dropped::Nothing);
            } else {
                t.links.truncate(1);
                t.write_links(match p.root {
                    Root::Value(v) if tag == Tag::Drop && p.whole() => Dropped::Value(v),
                    Root::Local(l) if tag == Tag::Drop && p.whole() => Dropped::Local(l),
                    _ => Dropped::Nothing,
                });
            }
        }
        t.effects[i].may = match tag {
            Tag::Call => Callee::from_words(b.record(a)).is_none_or(|c| cx.may_write(&c)),
            Tag::Intrinsic => IntrinsicOp::from_u32(a) == Some(IntrinsicOp::Item),
            Tag::CallValue | Tag::CallHost | Tag::DefaultCall | Tag::Hook | Tag::Await => true,
            _ => false,
        };
    }
    t.write_start[n] = u32_of(t.writes.len());
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
                    self.change_at(inst, b.tags[inst as usize], s, live);
                }
                self.held.push(NONE);
                for k in 0..self.suite_bits.len() {
                    if self.suite_bits[k].0 == x {
                        let bit = self.suite_bits[k].1;
                        self.held.push(bit);
                        self.set(bit, s, live, true);
                    }
                }
            }
        }
        // Its value is made here: dead before. A coercion's value is still
        // its read's.
        let bit = self.v[i].bit;
        if bit != NONE && self.bits[bit as usize].read == x {
            self.bits[bit as usize].seen = true;
            self.set(bit, s, live, false);
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
        if let Some(&local) = self.reference_roots.get(v as usize)
            && local != NONE
        {
            let bit = self.l[local as usize].bit;
            if bit != NONE {
                self.set(bit, s, live, true);
            }
        }
    }

    fn reads(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let tag = b.tags[x as usize];
        // A read through a binding local uses what it holds.
        if matches!(tag, Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Store)
            && let Some(l) = b.view(b.data[x as usize][0]).local()
            && self.l[l as usize].bit != NONE
        {
            self.set(self.l[l as usize].bit, s, live, true);
        }
        for k in 0..self.held.len() {
            if self.held[k] != NONE {
                self.set(self.held[k], s, live, true);
            }
        }
        let k = self.root_uses.partition_point(|u| u.0 < x);
        for &(_, bit) in self.root_uses[k..].iter().take_while(|u| u.0 == x) {
            self.set(bit, s, live, true);
        }
        self.change_at(x, tag, s, live);
    }
}

/// Whether every `Drop` bit `bit` crossed empties what owns nothing.
fn crossed_ok(b: &Body, t: &Tables, bit: u32) -> bool {
    let lo = t.crossed.partition_point(|c| c.0 < bit);
    t.crossed[lo..]
        .iter()
        .take_while(|c| c.0 == bit)
        .all(|&(_, d)| match d.checked_sub(u32_of(b.len())) {
            None => {
                let f = t.v[d as usize];
                b.provider_ref(d as usize)
                    || (f.bit != NONE && t.v[t.bits[f.bit as usize].read as usize].borrows)
            }
            Some(l) => t.l[l as usize].unowned,
        })
}

/// Solves what depends on another decision, from everything that may borrow
/// down to what holds: a read's source crossed the `Drop` of a place that
/// must own nothing, a binding stays one only if no read takes a count from
/// it for nothing, and a parameter stays lent only if nothing keeps it.
fn solve(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) {
    for bit in 0..t.bits.len() {
        let r = t.bits[bit].read as usize;
        // Either source may be the one that holds, until the solve knows.
        let seen = t.bits[t.v[r].bit as usize].seen;
        t.v[r].borrows |= seen && !get(&t.changed, u32_of(bit));
    }
    let list = super::params(b);
    let n = list.len();
    let suspends = b.kind != BodyKind::Default && cx.suspending(b.item);
    let lendable = matches!(
        b.kind,
        BodyKind::Fn | BodyKind::DeriveInstance | BodyKind::Default
    );
    // Reuse the write index: borrowing an addressed parameter must not
    // lose the value snapshot while another alias can change its storage.
    let no_external_write = t.effects.iter().all(|e| !e.may && !e.reach);
    let mut taken = vec![false; b.len()];
    t.crossed.sort_stable();
    t.crossed.dedup();
    // Bindings settle first: a read through one depends on how it ended.
    let mut settled = false;
    loop {
        let mut changed = false;
        // The reads that keep a count: per local how many read it, and per
        // value whether one reads it.
        t.l.iter_mut().for_each(|f| f.kept = 0);
        taken.fill(false);
        for k in 0..t.takers.len() {
            let i = t.takers[k] as usize;
            let f = t.v[i];
            if (f.keeps || f.moves || t.store_of(i) != NONE) && !f.borrows {
                match b.view(b.data[i][0]).root {
                    Root::Local(l) => t.l[l as usize].kept += 1,
                    Root::Value(v) => taken[v as usize] = true,
                    _ => {}
                }
            }
        }
        for (at, &l) in list.iter().enumerate() {
            let f = t.l[l as usize];
            let lent = lendable
                && f.stores == 0
                && !f.captured
                && (!f.reached || (f.shared_lends_only && no_external_write))
                && !f.changed
                && cx.counted(b.local_ty[l as usize])
                && (f.kept == 0 && !suspends || cx.received(b.item, at, n) == Pass::Lent);
            changed |= f.unowned != lent;
            t.l[l as usize].unowned = lent;
        }
        for l in 0..t.l.len() {
            let bit = t.l[l].bit;
            if bit != NONE {
                t.l[l].unowned = t.v[t.bits[bit as usize].read as usize].borrows;
            }
        }
        for k in 0..u32_of(t.bits.len()) {
            let Bit { read, bind, .. } = t.bits[k as usize];
            let r = read as usize;
            let f = t.v[r];
            if !f.borrows || f.bit != k || (!settled && bind == NONE) {
                continue;
            }
            // What reads this value for a consumer needs a count. Where this
            // read could move, that count costs nothing here, and borrowing
            // would make it a retain there.
            let needed = taken[r]
                || (f.alias != NONE && taken[f.alias as usize])
                || (bind != NONE && t.l[bind as usize].kept > 0);
            // It could move only out of what can own, by a place a move
            // takes: not a value that borrows, nor a parameter its caller
            // keeps, nor what lies behind a reference or in a buffer.
            let root_owned = match b.view(b.data[r][0]).root {
                Root::Local(m) => {
                    !t.l[m as usize].unowned
                        || list.iter().position(|&p| p == m).is_some_and(|at| {
                            !suspends && cx.received(b.item, at, n) == Pass::Owned
                        })
                }
                Root::Value(v) => !t.v[v as usize].borrows,
                _ => true,
            };
            // A binding that borrows leads on to what it holds; one that owns
            // its value is the source.
            let mut bit = k;
            let mut at = t.bits[bit as usize];
            while at.stop != NONE && t.l[at.stop as usize].unowned && at.far != NONE {
                bit = at.far;
                at = t.bits[bit as usize];
            }
            let moves = needed && root_owned && b.view(b.data[r][0]).in_place(b);
            if moves
                || get(&t.changed, bit)
                || !crossed_ok(b, t, bit)
                || (t.probe && at.relied != 0)
            {
                t.v[r].moves = moves;
                t.v[r].borrows = false;
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
    for bit in 0..t.bits.len() {
        if !get(&t.changed, u32_of(bit)) {
            t.relies |= t.bits[bit].relied;
        }
    }
    // The trait value a `&dyn` is made of, read through a reference, is
    // that reference's referent, which the checker keeps unchanged while
    // the `&dyn` lives and whose root lives as long (`frame`, rule 5): a
    // borrow, as the `&dyn` counts nothing (`ref.count.local`).
    for r in 0..b.len() {
        let f = t.v[r];
        if b.tags[r] == Tag::Read
            && b.data[r][1] == ReadMode::Copy as u32
            && f.uses == 1
            && b.view(b.data[r][0]).first(b) == Some(Proj::Deref)
            && super::dyn_view(b, cx, f.user as usize).is_some()
        {
            t.v[r].borrows = true;
        }
    }
    // A value no instance copies is never copied: a match decides through
    // its scrutinee by borrowing it. There is no count to end or drop.
    for r in 0..b.len() {
        if b.tags[r] == Tag::Read
            && b.data[r][1] == ReadMode::Copy as u32
            && !t.v[r].keeps
            && !cx.counted(b.ty[r])
            && cx.copies(b.ty[r]) != Tri::Yes
            && match_reference_read(b, topo, r, false)
        {
            t.v[r].borrows = true;
        }
    }
}

struct ParameterJoin {
    local: u32,
    reads: Vec<u32>,
    sources: Vec<(u32, bool)>,
}

/// Build each incidence once, enqueue it once, and remove each group once.
/// A bad parameter invalidates its consumers once; depth never retries the body.
fn retain_join_groups(
    kept: &[u32],
    sources: &[Vec<(u32, bool)>],
    mut visit: impl FnMut(),
) -> Vec<bool> {
    let mut by_param = vec![Vec::new(); kept.len()];
    let mut credited = vec![0u32; kept.len()];
    for (group, inputs) in sources.iter().enumerate() {
        for &(p, owned) in inputs {
            visit();
            by_param[p as usize].push(group);
            credited[p as usize] += u32::from(owned);
        }
    }
    let mut active = vec![true; sources.len()];
    let mut rejected = vec![false; kept.len()];
    let mut queue = Vec::new();
    for (p, &credit) in credited.iter().enumerate() {
        visit();
        if credit < kept[p] {
            rejected[p] = true;
            queue.extend_from_slice(&by_param[p]);
        }
    }
    while let Some(group) = queue.pop() {
        visit();
        if !std::mem::replace(&mut active[group], false) {
            continue;
        }
        for &(p, owned) in &sources[group] {
            visit();
            let p = p as usize;
            credited[p] -= u32::from(owned);
            if !rejected[p] && credited[p] < kept[p] {
                rejected[p] = true;
                queue.extend_from_slice(&by_param[p]);
            }
        }
    }
    active
}

/// A conditional selection that only lends its stored value can borrow
/// unchanged whole parameters. These sources live for the entire call;
/// unlike a general place loan they need no per-alternative source walk.
/// Reject connected candidates with another owned use by a worklist: each
/// candidate and parameter edge is removed once, with no retry by depth.
fn parameter_joins(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) {
    if !cx.summarizes() || cx.suspending(b.item) {
        return;
    }
    let mut groups = Vec::new();
    for i in 0..b.len() {
        let f = t.v[i];
        // A closure's suspension and parameter ABI are separate from the
        // enclosing function's. Its own later body can prove its loans.
        if topo.sub[i] != 0 || f.stored == NONE || f.uses != 1 || f.keeps {
            continue;
        }
        let Some(local) = t.binding(b, f.stored) else {
            continue;
        };
        if t.l[local as usize].kept != 0 {
            continue;
        }
        let Some(reads) = b.parameter_join_with(u32_of(i), |v| t.v[v as usize].uses <= 1) else {
            continue;
        };
        // parameter_join_with already admits only whole PARAM reads.
        let eligible = reads.iter().all(|&r| {
            b.view(b.data[r as usize][0]).local().is_some_and(|p| {
                let p = t.l[p as usize];
                p.stores == 0
                    && !p.captured
                    && !p.reached
                    && !p.changed
                    && cx.counted(b.ty[r as usize])
                    && cx.copies(b.ty[r as usize]) == Tri::Yes
                    && cx.holds_drop(b.ty[r as usize]) == Tri::No
            })
        });
        if !eligible {
            continue;
        }
        let sources = reads
            .iter()
            .filter_map(|&r| {
                let f = t.v[r as usize];
                let p = b.view(b.data[r as usize][0]).local()?;
                let owned = !f.borrows && (f.keeps || f.moves || t.store_of(r as usize) != NONE);
                Some((p, owned))
            })
            .collect();
        groups.push(ParameterJoin {
            local,
            reads,
            sources,
        });
    }
    if groups.is_empty() {
        return;
    }
    let kept: Vec<_> = t.l.iter().map(|p| p.kept).collect();
    let sources: Vec<_> = groups
        .iter_mut()
        .map(|g| std::mem::take(&mut g.sources))
        .collect();
    let active = retain_join_groups(&kept, &sources, || {});
    for ((group, sources), yes) in groups.into_iter().zip(sources).zip(active) {
        if !yes {
            continue;
        }
        t.l[group.local as usize].unowned = true;
        for r in group.reads {
            t.v[r as usize].borrows = true;
            t.v[r as usize].moves = false;
        }
        for (p, owned) in sources {
            t.l[p as usize].kept -= u32::from(owned);
            if t.l[p as usize].kept == 0 {
                t.l[p as usize].unowned = true;
            }
        }
    }
}

/// A needed copy has its own count; it does not also need an owner for
/// its unchanged source parameter. Run after move decisions so an input
/// count actually donated to a consumer keeps the consuming ABI. In
/// particular, a temporary can still move straight into a mutable local.
pub(super) fn lend_copied_parameters(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> bool {
    lend_copied_parameters_with(b, cx, topo, t, edits, || {})
}

fn lend_copied_parameters_with(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    t: &mut Tables,
    edits: &mut splice::Tables,
    mut visit: impl FnMut(),
) -> bool {
    if !matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance) || cx.suspending(b.item) {
        return false;
    }
    let params = super::params(b);
    let mut eligible = vec![false; b.local_ty.len()];
    for &p in params.iter().take(64) {
        visit();
        let local = t.l[p as usize];
        let ty = b.local_ty[p as usize];
        eligible[p as usize] = !local.unowned
            && local.stores == 0
            && !local.captured
            && !local.reached
            && !local.changed
            && cx.counted(ty)
            && cx.copies(ty) == Tri::Yes
            && cx.holds_drop(ty) == Tri::No;
    }
    if !eligible.iter().any(|&yes| yes) {
        return false;
    }
    if b.tags.iter().enumerate().any(|(i, &tag)| {
        visit();
        is_suspension(tag)
            || tag == Tag::Defer
            || tag == Tag::Scope && !b.record(b.data[i][1]).is_empty()
    }) {
        return false;
    }
    // A cleanup after the input ends may still use its copies. Retain the
    // consuming convention when an implicit local/resource cleanup could
    // make a temporary input's earlier end useful.
    if b.exit_drops.iter().any(|&(sub, l)| {
        sub.raw() == 0
            && (b.local_flags[l.idx()] & local_flags::PARAM == 0
                || cx.holds_drop(b.local_ty[l.idx()]) != Tri::No)
    }) {
        return false;
    }
    // One operand census, including large calls. A move used only for
    // reading and then dropped donates no count; a consuming use does.
    // Do not rescan each call's argument list for default-argument reads:
    // an owned formal conservatively counts as a possible donation.
    let mut needed = vec![false; b.len()];
    let mut last_mutation = None;
    let mut ops = Vec::new();
    for i in 0..b.len() {
        visit();
        let tag = b.tags[i];
        let mut mutation = matches!(
            tag,
            Tag::Drop
                | Tag::IsUnique
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Hook
                | Tag::NewStruct
                | Tag::NewTuple
                | Tag::NewVariant
                | Tag::NewList
                | Tag::NewMap
                | Tag::Closure
        ) || tag == Tag::Intrinsic
            && (super::changed(b, i).next().is_some()
                || matches!(
                    IntrinsicOp::from_u32(b.data[i][0]),
                    None | Some(
                        IntrinsicOp::Item
                            | IntrinsicOp::StrConcat
                            | IntrinsicOp::MapGet
                            | IntrinsicOp::MapEntry
                    )
                ))
            || tag == Tag::Call
                && Callee::from_words(b.record(b.data[i][0]))
                    .is_none_or(|c| !cx.call_reads_storage_only(&c));
        operands(b, i, &mut ops);
        for op in &ops {
            visit();
            let consumes = match op.role {
                Role::Consume | Role::Tail => true,
                // A place read, reference or match may move a field through
                // a borrowed alias or an implicit pattern binding. Only an
                // independent Copy read can supply that owner's count.
                Role::Lend => matches!(tag, Tag::Read | Tag::Ref | Tag::Match | Tag::SwitchTag),
                Role::Dropped => false,
                Role::Arg(k) => match b.tags[i] {
                    Tag::Call => Callee::from_words(b.record(b.data[i][0])).is_none_or(|c| {
                        cx.pass(&c, k as usize, b.call_args(i).len()) == Pass::Owned
                            || cx.retains_result(&c)
                    }),
                    _ => true,
                },
            };
            needed[op.value as usize] |= consumes;
            mutation |= tag == Tag::Call && consumes && cx.counted(b.ty[op.value as usize]);
        }
        if mutation {
            last_mutation = Some(i);
        }
    }
    let mut copied = vec![false; b.local_ty.len()];
    let mut moved_from = vec![NONE; b.len()];
    for i in 0..b.len() {
        visit();
        if b.tags[i] == Tag::Read
            && b.data[i][1] == ReadMode::Copy as u32
            && needed[i]
            && let Some(local) = b.view(b.data[i][0]).local()
        {
            copied[local as usize] = true;
        }
        if b.tags[i] == Tag::Read && b.data[i][1] != ReadMode::Copy as u32 {
            let p = b.view(b.data[i][0]);
            if let Some(local) = p.local().filter(|&l| eligible[l as usize]) {
                if needed[i] || !p.whole() && b.data[i][1] == ReadMode::Move as u32 {
                    eligible[local as usize] = false;
                } else if b.data[i][1] == ReadMode::Move as u32 {
                    moved_from[i] = local;
                }
            }
        }
        if b.tags[i] == Tag::Drop {
            let p = b.view(b.data[i][0]);
            let local = match p.root {
                Root::Local(l) if p.whole() => Some(l),
                Root::Value(v) if p.whole() && moved_from[v as usize] != NONE => {
                    Some(moved_from[v as usize])
                }
                _ => None,
            };
            // Releasing an owned temporary input before a later COW
            // operation can make its copy unique. A guaranteed input lives
            // for the call, so preserve the consuming ABI in that case.
            // A loop can run an earlier mutation again after this end.
            if let Some(local) = local
                && (topo.loop_of[i] != NONE || last_mutation.is_some_and(|last| i < last))
            {
                eligible[local as usize] = false;
            }
        }
    }
    // Change only owners duplicated by an independently needed copy.
    // Borrowed-result calls may instead take over the input's count in
    // the later retains pass, so the operand census preserves that donor.
    for (yes, copied) in eligible.iter_mut().zip(copied) {
        visit();
        *yes &= copied;
    }
    if !eligible.iter().any(|&yes| yes) {
        return false;
    }
    // Clear only the original parameter's owner. Each Read copy remains
    // a copy and each owned local keeps its existing drop and COW check.
    for i in 0..b.len() {
        visit();
        if moved_from[i] != NONE && eligible[moved_from[i] as usize] {
            b.data[i][1] = ReadMode::Borrow as u32;
        }
        if b.tags[i] == Tag::Drop {
            let p = b.view(b.data[i][0]);
            // Insertion emits whole drops only for locals or SSA values.
            let source = p
                .local()
                .or_else(|| p.value().map(|v| moved_from[v as usize]));
            if p.whole() && source.is_some_and(|l| l != NONE && eligible[l as usize]) {
                // Keep the original parameter words intact: a caller may
                // still use the owned ABI, whose common-exit descriptor
                // releases the incoming count. A LoanEnd would zero them.
                edits.remove(u32_of(i));
            }
        }
    }
    b.exit_drops
        .retain(|&(sub, l)| sub.raw() != 0 || !eligible[l.idx()]);
    for p in params {
        if eligible[p as usize] {
            t.l[p as usize].unowned = true;
            t.l[p as usize].kept = 0;
        }
    }
    true
}

/// The counted reads that may keep a count, into `t.takers`.
fn takers(b: &Body, cx: &dyn OwnCx, t: &mut Tables) {
    t.takers.clear();
    for i in 0..b.len() {
        let f = t.v[i];
        if b.tags[i] == Tag::Read
            && (f.keeps || f.bit != NONE || t.store_of(i) != NONE)
            && cx.counted(b.ty[i])
        {
            t.takers.push(u32_of(i));
        }
    }
}

/// Swift RC identity through single-producing-exit scopes and inline Copy
/// aggregates without Drop or reference carriers and with one nontrivial
/// operand. Classify each concrete shape once; constructor records supply
/// their operands in total input-sized work.
pub(crate) fn rc_sources(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    rc_sources_with(b, cx, |_| {})
}

fn rc_sources_with(b: &Body, cx: &dyn OwnCx, mut visit: impl FnMut(usize)) -> Vec<u32> {
    let mut sources = b.scope_forwarders(|| visit(1));
    let mut shapes = hd_base::HashMap::default();
    let mut fields = Vec::new();
    for (i, source) in sources.iter_mut().enumerate() {
        visit(1);
        if b.tags[i] == Tag::Scope {
            continue;
        }
        let some = b.tags[i] == Tag::Coerce
            && b.record(b.data[i][1]).first() == Some(&(crate::ir::Coercion::WrapSome as u32));
        let variant = match b.tags[i] {
            Tag::NewStruct | Tag::NewTuple => None,
            Tag::NewVariant => Some(b.data[i][0]),
            Tag::Coerce if some => Some(1),
            _ => continue,
        };
        let shape = *shapes.entry((b.ty[i], variant)).or_insert_with(|| {
            if cx.copies(b.ty[i]) != Tri::Yes
                || cx.holds_drop(b.ty[i]) != Tri::No
                || cx.holds_ref(b.ty[i])
                || !cx.parts(b.ty[i], variant, &mut fields)
            {
                return None;
            }
            let mut nontrivial = None;
            for (field, &ty) in fields.iter().enumerate() {
                visit(1);
                if cx.counted(ty) {
                    if nontrivial.is_some() {
                        return None;
                    }
                    nontrivial = Some(field);
                }
            }
            nontrivial.map(|field| (field, fields.len()))
        });
        let Some((field, width)) = shape else {
            continue;
        };
        if some {
            if field == 0 && width == 1 {
                *source = b.data[i][0];
            }
            continue;
        }
        let values = b.record(b.data[i][1]);
        let rest = b.tags[i] == Tag::NewTuple && b.data[i][0] != NONE;
        if values.len() + usize::from(rest) != width {
            continue;
        }
        *source = values
            .get(field)
            .copied()
            .or_else(|| rest.then_some(b.data[i][0]))
            .unwrap_or(NONE);
    }
    sources
}

/// Derives which reads of `b`, as insertion left it, borrow and which
/// parameters and bindings the body owns none of.
pub(crate) fn run(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) {
    let constructors = rc_sources(b, cx);
    b.index_forwarded_reads(&constructors);
    t.reset(b.len(), b.local_ty.len());
    {
        let b = &*b;
        suites(b, t);
        scan(b, cx, t, &constructors);
        let flagged = b
            .sub_flags
            .first()
            .is_some_and(|f| f & sub_flags::EXCLUSIVE != 0);
        t.probe = !flagged && cx.summarizes();
        t.excl = (flagged || t.probe)
            && matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance)
            && !b.tags.iter().any(|&tag| is_suspension(tag));
        if t.excl {
            receivers(b, cx, t);
        }
        candidates(b, cx, t);
    }
    takers(b, cx, t);
    let b = &*b;
    super::frame_reference_uses(b, topo, cx, &mut t.frame_uses);
    if t.l.iter().any(|f| f.bit != NONE) {
        let mut roots = std::mem::take(&mut t.root_uses);
        super::frame::roots(b, cx, |i, l| {
            let bit = t.l[l as usize].bit;
            if bit != NONE && b.tags[i as usize] != Tag::Drop {
                roots.push((i, bit));
            }
        });
        roots.sort_stable();
        roots.dedup();
        t.root_uses = roots;
    }
    sources(b, cx, t);
    t.capture_drops.build(b, cx, topo);
    index_writes(b, cx, t);
    let mut back = std::mem::take(&mut t.back);
    backward(b, topo, &mut back, t);
    let roots = std::mem::take(&mut t.suite_roots);
    backward_blocks(b, topo, &mut back, t, &roots);
    t.suite_roots = roots;
    t.back = back;
    solve(b, topo, cx, t);
    parameter_joins(b, topo, cx, t);
}

/// Applies a derivation to `b`: the modes, the rooted reads, the exit rows,
/// and the `Drop`s of what owns nothing now, as edits.
pub(crate) fn apply(b: &mut Body, topo: &Topology, t: &Tables, edits: &mut splice::Tables) {
    edits.clear();
    for r in 0..b.len() {
        if t.v[r].borrows {
            b.data[r][1] = ReadMode::Borrow as u32;
        }
    }
    // The index describes possible RC identities. Only values sharing the
    // read's chosen forwarding endpoint actually borrow its count. A
    // stored, multiply used or independently ended value is a boundary.
    let mut forwarded = std::mem::take(&mut b.forwarded_reads);
    forwarded.retain(|&(value, read)| {
        let end = t.v[read as usize].alias;
        let f = t.v[value as usize];
        let value_end =
            if f.uses == 1 && (!f.dropped || f.rewrapped) && f.stored == NONE && f.alias != NONE {
                f.alias
            } else {
                value
            };
        b.is_borrow(read) && end != NONE && end == value_end
    });
    b.forwarded_reads = forwarded;
    for i in 0..b.len() {
        if b.tags[i] == Tag::Drop {
            let p = b.view(b.data[i][0]);
            let gone = p.whole()
                && match p.root {
                    Root::Value(v) => {
                        let bit = t.v[v as usize].bit;
                        t.v[v as usize].borrows
                            || b.borrowed_alias(v).is_some()
                            || bit != NONE && t.v[t.bits[bit as usize].read as usize].borrows
                            || b.parameter_join_reads(v).is_some_and(|reads| {
                                reads.into_iter().all(|r| t.v[r as usize].borrows)
                            })
                    }
                    Root::Local(l) => t.l[l as usize].unowned,
                    _ => false,
                };
            if gone {
                edits.remove(u32_of(i));
            }
        }
    }
    flatten_match_reference_reads(b, topo);
    let l = &t.l;
    b.exit_drops.retain(|(_, x)| !l[x.idx()].unowned);
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
    for (k, l) in params.into_iter().enumerate().take(64) {
        let f = t.l[l as usize];
        // A value with a `Drop` hook is kept: its hooks run at the
        // callee's exit, as a dev build runs them. A lent argument would
        // run them at the caller's drop instead, and a field moved into
        // the call would stay in place, so the value that held it would
        // run its own hook too (`ref.move.field`).
        if cx.holds_drop(b.local_ty[l as usize]) == Tri::Yes {
            kept |= 1 << k;
            continue;
        }
        if !cx.counted(b.local_ty[l as usize]) {
            continue;
        }
        if !f.unowned || f.kept > 0 {
            kept |= 1 << k;
        } else if cx.received(b.item, k, n) == Pass::Owned {
            borrows |= 1 << k;
        }
    }
    Some((kept, borrows))
}

/// For each local reference, the one direct call it reaches through
/// same-value coercions, or NONE. Its result holds no local reference,
/// so the address dies with the call; every other use is a Drop.
/// Counted-reference results own their referents independently.
pub(crate) fn local_lending_calls(
    b: &Body,
    cx: &dyn OwnCx,
    counts: &[u32],
    last: &[u32],
) -> Vec<u32> {
    local_lending_calls_with(b, cx, counts, last, || {
        #[cfg(test)]
        super::effect_places::work(1);
    })
}

/// One shared def-use proof, not one bounded alias walk per reference.
/// Seed values used by one call, then propagate backwards through same-value
/// coercions. Each value enters the worklist at most once, so width and
/// coercion depth cost O(instructions + operands), without native recursion.
fn local_lending_calls_with(
    b: &Body,
    cx: &dyn OwnCx,
    counts: &[u32],
    last: &[u32],
    mut visit: impl FnMut(),
) -> Vec<u32> {
    let mut calls = vec![NONE; b.len()];
    let mut work = Vec::new();
    for v in 0..b.len() {
        visit();
        let u = last[v] as usize;
        if counts[v] == 1
            && b.tags.get(u) == Some(&Tag::Call)
            && (!cx.holds_ref(b.ty[u]) || cx.counted_refs_only(b.ty[u]))
        {
            calls[v] = last[v];
            work.push(v);
        }
    }
    while let Some(v) = work.pop() {
        visit();
        if b.same_value_coercion(v)
            && let Some(source) = Ref(b.data[v][0]).as_inst()
        {
            let source = source.0 as usize;
            if counts[source] == 1 && last[source] as usize == v && calls[source] == NONE {
                calls[source] = calls[v];
                work.push(source);
            }
        }
    }
    for (v, call) in calls.iter_mut().enumerate() {
        visit();
        if b.tags[v] != Tag::Ref || b.data[v][1] & ref_word::LOCAL == 0 {
            *call = NONE;
        }
    }
    calls
}

/// Per local: whether a reference to it outlives the one call it lends to,
/// or a closure shares it ([`local_lending_calls`]). A reference used only for
/// read-through projections does not escape either; an inlined reader
/// has those uses instead of its original call.
pub(crate) struct ReferenceUses {
    pub escaping: Vec<bool>,
    pub calls: Vec<u32>,
}

pub(crate) fn reference_uses(b: &Body, cx: &dyn OwnCx) -> ReferenceUses {
    let n = b.len();
    let (mut counts, mut last) = (vec![0; n], vec![NONE; n]);
    let mut read_through = vec![true; n];
    let mut through = super::promote::ReadThroughUses::default();
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            let value = op.value as usize;
            read_through[value] &= through.permits(b, i, op.value);
            if op.role != Role::Dropped {
                counts[value] += 1;
                last[value] = u32_of(i);
            }
        }
    }
    let calls = local_lending_calls(b, cx, &counts, &last);
    let mut out = vec![false; b.local_ty.len()];
    for (i, &read_through) in read_through.iter().enumerate() {
        if b.tags[i] == Tag::Ref
            && b.data[i][1] & crate::ir::ref_word::FRAME == 0
            && let Some(l) = b.local_in_place(b.data[i][0])
            && calls[i] == NONE
            && !read_through
        {
            out[l.idx()] = true;
        }
    }
    for (l, m) in b.cap_local.iter().zip(&b.cap_mode) {
        if m.shares() {
            out[l.idx()] = true;
        }
    }
    ReferenceUses {
        escaping: out,
        calls,
    }
}

/// Immutable frame dependencies through unary values and single-definition
/// locals. Local definitions may follow their uses in block layout; resolve
/// the functional graph once, with path compression and no native recursion.
/// Each node and edge settles once: O(instructions + locals + captures +
/// distinct place words) work and O(instructions + locals) table space.
pub(crate) fn frame_reference_roots(
    b: &Body,
    forwarding: &[u32],
    mut visit: impl FnMut(),
) -> Vec<u32> {
    let n = b.len();
    let locals = b.local_ty.len();
    let mut parents = vec![usize::MAX; n + locals];
    let mut roots = vec![NONE; n + locals];
    let mut state = vec![0u8; n + locals];
    // Zero, one, or multiple definitions; arbitrarily many stores remain
    // ambiguous rather than being truncated to a chosen definition.
    let mut stores = vec![0u8; locals];
    let mut value = vec![NONE; locals];
    let mut addressed: Vec<_> = b
        .local_flags
        .iter()
        .map(|f| f & local_flags::PARAM != 0)
        .collect();
    let mut scopes = vec![NONE; n];
    let mut places = hd_base::HashMap::default();
    let mut local_in_place = |word| {
        *places
            .entry(word)
            .or_insert_with(|| b.local_in_place(word).map_or(NONE, hd_base::LocalId::raw))
    };
    for (i, &forwarded) in forwarding.iter().enumerate() {
        visit();
        let [a, w] = b.data[i];
        if b.tags[i] == Tag::Store {
            let local = local_in_place(a);
            if local != NONE {
                let l = local as usize;
                stores[l] = stores[l].saturating_add(1).min(2);
                if b.view(a).whole() {
                    value[l] = w;
                }
            }
        } else if b.tags[i] == Tag::Ref {
            let local = local_in_place(a);
            if local != NONE {
                addressed[local as usize] = true;
            }
        } else if b.tags[i] == Tag::Scope && forwarded != NONE {
            scopes[a as usize] = u32_of(i);
        }
    }
    for local in &b.cap_local {
        visit();
        addressed[local.idx()] = true;
    }
    for l in 0..locals {
        visit();
        if stores[l] == 1
            && !addressed[l]
            && let Some(v) = Ref(value[l]).as_inst()
            && (v.0 as usize) < n
        {
            parents[n + l] = v.0 as usize;
        }
    }
    for (i, &tag) in b.tags.iter().enumerate() {
        visit();
        if tag == Tag::Ref
            && b.data[i][1] & (ref_word::LOCAL | ref_word::FRAME | ref_word::MUT)
                == ref_word::LOCAL | ref_word::FRAME
            && local_in_place(b.data[i][0]) != NONE
        {
            roots[i] = local_in_place(b.data[i][0]);
            state[i] = 2;
            continue;
        }
        let source = if b.same_value_coercion(i) {
            Some(b.data[i][0])
        } else if tag == Tag::Scope {
            forwarding.get(i).copied().filter(|&v| v != NONE)
        } else {
            None
        };
        if let Some(v) = source
            .and_then(|v| Ref(v).as_inst())
            .filter(|v| (v.0 as usize) < n)
        {
            parents[i] = v.0 as usize;
        } else if matches!(tag, Tag::Ref | Tag::Read) {
            parents[i] = match b.view(b.data[i][0]).root {
                Root::Local(l) if parents[n + l as usize] != usize::MAX => n + l as usize,
                Root::Value(v) => v as usize,
                _ => usize::MAX,
            };
        } else if tag == Tag::Store {
            if let Some(local) = b.view(b.data[i][0]).local() {
                parents[i] = parents[n + local as usize];
            }
        } else if tag == Tag::Break
            && let Some(&block) = b.label_inst.get(b.data[i][0] as usize)
            && scopes[block as usize] != NONE
            && let Some(value) = Ref(b.data[i][1]).as_inst()
        {
            parents[i] = value.0 as usize;
        }
    }
    let mut path = Vec::new();
    for start in 0..parents.len() {
        visit();
        let mut at = start;
        let root = loop {
            if state[at] == 2 {
                break roots[at];
            }
            if state[at] == 1 || parents[at] == usize::MAX {
                break NONE;
            }
            visit();
            state[at] = 1;
            path.push(at);
            at = parents[at];
        };
        // An unknown terminal also resolves once; cycles conservatively
        // produce no origin and every node on their incoming path settles.
        if state[at] == 0 {
            state[at] = 2;
        }
        while let Some(at) = path.pop() {
            visit();
            roots[at] = root;
            state[at] = 2;
        }
    }
    roots.truncate(n);
    roots
}

/// A frame descriptor lends only when every transitive use is another
/// indexed alias, an immutable projection, or a nonescaping direct call.
/// The origin index turns every def-use edge into one constant-time check.
/// Reference facts are queried once per distinct type, not once per use.
pub(crate) fn frame_lending_roots(b: &Body, cx: &dyn OwnCx, roots: &[u32]) -> Vec<bool> {
    frame_lending_roots_with(b, cx, roots, || {})
}

fn frame_lending_roots_with(
    b: &Body,
    cx: &dyn OwnCx,
    roots: &[u32],
    mut queried: impl FnMut(),
) -> Vec<bool> {
    let mut lends = vec![true; b.local_ty.len()];
    let mut ops = Vec::new();
    let mut through = super::promote::ReadThroughUses::default();
    let mut types = hd_base::HashMap::default();
    let mut references = |ty| {
        *types.entry(ty).or_insert_with(|| {
            queried();
            let holds = cx.holds_ref(ty);
            (holds, !holds || cx.counted_refs_only(ty))
        })
    };
    for i in 0..b.len() {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role == Role::Dropped {
                continue;
            }
            let Some(&root) = roots.get(op.value as usize).filter(|&&root| root != NONE) else {
                continue;
            };
            // An independently owned value read through the address ends
            // that address use at the read. If the read later borrows,
            // its indexed source is protected by the lifetime solver.
            if Ref(op.value).as_inst().is_some()
                && b.tags[op.value as usize] == Tag::Read
                && !references(b.ty[op.value as usize]).0
            {
                continue;
            }
            let aliases = roots[i] == root;
            let allowed = match b.tags[i] {
                Tag::Store | Tag::Break | Tag::Read => aliases,
                Tag::Ref => aliases && b.data[i][1] & ref_word::MUT == 0,
                Tag::Coerce => aliases && b.same_value_coercion(i),
                Tag::Call => references(b.ty[i]).1,
                _ => through.permits(b, i, op.value),
            };
            lends[root as usize] &= allowed;
        }
    }
    lends
}

/// Whether read `r` is in a match's decision block and reads through that
/// match's reference scrutinee. With `nested`, follows borrowed decision reads
/// to recognize their payload reads too.
pub(super) fn match_reference_read(b: &Body, topo: &Topology, r: usize, nested: bool) -> bool {
    let Some(&first) = topo.blk.get(r).filter(|&&block| block != NONE) else {
        return false;
    };
    let mut block = first;
    let mut subject = NONE;
    for _ in 0..b.len() {
        let owner = topo.owner[block as usize];
        if owner != NONE && b.tags.get(owner as usize) == Some(&Tag::Match) {
            let [candidate, blocks] = b.data[owner as usize];
            if b.record(blocks).first() == Some(&block) {
                subject = candidate;
                break;
            }
        }
        let Some((parent, _)) = topo.up(block) else {
            return false;
        };
        block = parent;
    }
    if subject == NONE {
        return false;
    }
    let mut word = b.data[r][0];
    let mut behind = false;
    for _ in 0..DEPTH {
        let place = b.view(word);
        behind |= place.kinds(b).0 > 0;
        match place.root {
            Root::Value(value) if value == subject => return behind,
            Root::Value(value)
                if nested
                    && b.tags.get(value as usize) == Some(&Tag::Read)
                    && b.data[value as usize][1] == ReadMode::Borrow as u32 =>
            {
                word = b.data[value as usize][0];
            }
            _ => return false,
        }
    }
    false
}

/// Re-roots borrowed match payload reads at the place the borrowed scrutinee
/// reads. The lowering initially spells a payload as a part of the decision
/// read (`[%decision as .variant .field]`). That decision read may keep a
/// `Drop` for an erased instance that makes its value fresh; the payload loan
/// instead lives on the referenced scrutinee, which the match keeps alive.
pub(super) fn flatten_match_reference_reads(b: &mut Body, topo: &Topology) {
    let mut changed = Vec::new();
    for r in 0..b.len() {
        if b.tags[r] != Tag::Read
            || b.data[r][1] != ReadMode::Borrow as u32
            || !match_reference_read(b, topo, r, true)
        {
            continue;
        }
        let Some(mut p) = b.decode_place(b.data[r][0]) else {
            continue;
        };
        let mut flattened = false;
        for _ in 0..DEPTH {
            let PlaceRoot::Value(base) = p.root else {
                break;
            };
            let i = base.0 as usize;
            if b.tags.get(i) != Some(&Tag::Read) || b.data[i][1] != ReadMode::Borrow as u32 {
                break;
            }
            let Some(mut outer) = b.decode_place(b.data[i][0]) else {
                break;
            };
            outer.proj.extend(p.proj);
            p = outer;
            flattened = true;
        }
        if flattened {
            changed.push((r, p));
        }
    }
    for (r, p) in changed {
        let words = p.words().expect("a non-bare match place");
        let at = u32_of(b.extra.len());
        b.extra.push(u32_of(words.len()));
        b.extra.extend_from_slice(&words);
        b.data[r][0] = at;
    }
}

#[cfg(test)]
mod lending_tests {
    use super::*;
    use crate::ir::{Coercion, Providers, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    const LOCAL_REF: Ty = Ty(1_000_004);

    #[test]
    fn forwarding_aliases_collapse_once_at_width_and_depth() {
        for width in [1_000, 10_000] {
            for depth in [1, 20] {
                let mut t = super::Tables::default();
                t.reset(width * (depth + 1), 0);
                for root in (0..t.v.len()).step_by(depth + 1) {
                    for j in root..root + depth {
                        t.v[j].uses = 1;
                        t.v[j].alias = u32::try_from(j + 1).expect("generated value");
                    }
                    t.v[root + depth].uses = 1;
                    t.v[root + depth].stored = 42;
                }
                let mut visits = 0;
                super::collapse_aliases(&mut t.v, || visits += 1);
                assert_eq!(visits, t.v.len());
                for root in (0..t.v.len()).step_by(depth + 1) {
                    assert_eq!(t.v[root].alias as usize, root + depth);
                    assert_eq!(t.store_of(root), 42);
                }
            }
        }
    }

    #[test]
    fn forwarding_aliases_keep_drop_store_and_shared_use_boundaries() {
        for barrier in 0..3 {
            let mut t = super::Tables::default();
            t.reset(4, 0);
            for j in 0..3 {
                t.v[j].uses = 1;
                t.v[j].alias = u32::try_from(j + 1).expect("generated value");
            }
            t.v[3].uses = 1;
            t.v[3].stored = 42;
            match barrier {
                0 => t.v[1].dropped = true,
                1 => t.v[1].stored = 41,
                _ => t.v[1].uses = 2,
            }
            super::collapse_aliases(&mut t.v, || {});
            assert_eq!(t.v[0].alias, 1);
        }
    }

    #[test]
    fn wrapper_work_is_linear_with_trailing_loan_cleanup() {
        for (width, depth) in [
            (1, 1),
            (1000, 1),
            (10_000, 1),
            (1, 20),
            (1000, 20),
            (10_000, 20),
        ] {
            let mut b = Body::new(DefId::from_raw(4), BodyKind::Fn);
            // Only the instruction and label columns are queried by this
            // index. Build nested single-exit scopes without a native walk.
            let mut expected = Vec::new();
            for _ in 0..width {
                let mut value = u32_of(b.len());
                b.tags.push(Tag::Read);
                b.data.push([0, ReadMode::Borrow as u32]);
                for _ in 0..depth {
                    let label = u32_of(b.label_inst.len());
                    let exit = u32_of(b.len());
                    b.tags.push(Tag::Break);
                    b.data.push([label, value]);
                    let cleanup = u32_of(b.len());
                    b.tags.push(Tag::LoanEnd);
                    b.data.push([0, NONE]);
                    let record = u32_of(b.extra.len());
                    b.extra.extend([2, exit, cleanup]);
                    let block = u32_of(b.len());
                    b.tags.push(Tag::Block);
                    b.data.push([record, NONE]);
                    b.label_inst.push(block);
                    let empty = u32_of(b.extra.len());
                    b.extra.push(0);
                    value = u32_of(b.len());
                    b.tags.push(Tag::Scope);
                    b.data.push([block, empty]);
                    expected.push(value);
                }
            }
            let mut t = Tables::default();
            let mut visits = 0;
            let sources = b.scope_forwarders(|| visits += 1);
            assert_eq!(visits, 2 * b.len());
            wrappers_with(&b, &mut t, &sources, || visits += 1);
            assert_eq!(t.wrapper, expected);
            assert_eq!(visits, 3 * b.len() + b.label_inst.len());
            let mut source_visits = 0;
            let roots = b.forwarding_reads(&sources, || source_visits += 1);
            assert_eq!(source_visits, b.len());
            b.index_forwarded_reads(&sources);
            for &scope in &expected {
                let root = roots[scope as usize];
                assert_eq!(b.tags[root as usize], Tag::Read);
                assert_eq!(b.borrowed_alias(scope), Some(root));
                // A later ownership decision changes the result without
                // rebuilding the identity index.
                b.data[root as usize][1] = ReadMode::Copy as u32;
                assert_eq!(b.borrowed_alias(scope), None);
                b.data[root as usize][1] = ReadMode::Borrow as u32;
            }

            // A second exit to a block invalidates that wrapper only.
            b.tags.push(Tag::Break);
            b.data.push([0, 0]);
            wrappers(&b, &mut t, &b.scope_forwarders(|| {}));
            assert_eq!(t.wrapper[0], NONE);
            assert_eq!(&t.wrapper[1..], &expected[1..]);
        }
    }

    #[test]
    fn parameter_join_rejection_work_is_linear_at_width_1000_depth_20() {
        for width in [1usize, 1000] {
            for depth in [1usize, 20] {
                let mut inputs = Vec::new();
                let mut kept = vec![0; width * (depth + 1) + 1];
                for chain in 0..width {
                    let base = chain * (depth + 1);
                    for edge in 0..depth {
                        let a = base + edge;
                        let b = a + 1;
                        inputs.push(vec![(u32_of(a), true), (u32_of(b), true)]);
                        kept[a] += 1;
                        kept[b] += 1;
                    }
                    // One extra owned use rejects the entire connected chain.
                    kept[base + depth] += 1;
                }
                let control = kept.len() - 1;
                kept[control] = 1;
                inputs.push(vec![(u32_of(control), true)]);
                let mut work = 0;
                let active = super::retain_join_groups(&kept, &inputs, || work += 1);
                assert!(active[..width * depth].iter().all(|active| !active));
                assert_eq!(active.last(), Some(&true));
                assert_eq!(work, width * (7 * depth + 1) + 2);
            }
        }
    }

    #[test]
    fn parameter_join_queries_bound_width_and_depth_without_recursion() {
        let mut per_chain = 0;
        for width in [1, 1000] {
            let mut b = Body::new(DefId::from_raw(4), BodyKind::Fn);
            b.local_flags = vec![local_flags::PARAM; 2];
            let mut work = 0;
            for _ in 0..width {
                let mut value = u32_of(b.len());
                b.tags.push(Tag::Read);
                b.data
                    .push([crate::ir::place::BARE_LOCAL, ReadMode::Copy as u32]);
                b.ty.push(Ty::STRING);
                for _ in 0..20 {
                    let left = u32_of(b.len());
                    b.tags.push(Tag::Block);
                    b.data.push([0, value]);
                    b.ty.push(Ty::STRING);
                    let read = u32_of(b.len());
                    b.tags.push(Tag::Read);
                    b.data
                        .push([crate::ir::place::BARE_LOCAL | 1, ReadMode::Copy as u32]);
                    b.ty.push(Ty::STRING);
                    let right = u32_of(b.len());
                    b.tags.push(Tag::Block);
                    b.data.push([0, read]);
                    b.ty.push(Ty::STRING);
                    let arms = u32_of(b.extra.len());
                    b.extra.extend([2, left, right]);
                    value = u32_of(b.len());
                    b.tags.push(Tag::If);
                    b.data.push([0, arms]);
                    b.ty.push(Ty::STRING);
                    let mut visits = 0;
                    let _ = b.parameter_join_with(value, |_| {
                        visits += 1;
                        true
                    });
                    assert!(visits <= 16);
                    work += visits;
                }
                for _ in 0..20 {
                    let coercion = u32_of(b.extra.len());
                    b.extra
                        .extend([2, crate::ir::Coercion::Identity as u32, NONE]);
                    let next = u32_of(b.len());
                    b.tags.push(Tag::Coerce);
                    b.data.push([value, coercion]);
                    b.ty.push(Ty::STRING);
                    value = next;
                    let mut visits = 0;
                    let _ = b.parameter_join_with(value, |_| {
                        visits += 1;
                        true
                    });
                    assert!(visits <= 32);
                    work += visits;
                }
            }
            if width == 1 {
                per_chain = work;
            }
            assert_eq!(work, width * per_chain);
        }
    }

    struct CopyFacts {
        copies: Tri,
        drop: Tri,
    }
    impl OwnCx for CopyFacts {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn copies(&self, _: Ty) -> Tri {
            self.copies
        }
        fn holds_drop(&self, _: Ty) -> Tri {
            self.drop
        }
        fn pass(&self, c: &Callee, _: usize, _: usize) -> Pass {
            if matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(88)) {
                Pass::Owned
            } else {
                Pass::Lent
            }
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn call_reads_storage_only(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn parameter_count(&self, _: DefId) -> Option<usize> {
            Some(1)
        }
    }

    fn copied_body(width: usize, depth: usize, read_again: bool) -> Result<(Body, u32), String> {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let block = b.open_block();
        let p = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
        for _ in 0..width {
            let mut value = b.get(p, Ty::STRING, n);
            for _ in 0..depth {
                value = b.coerce(Coercion::Identity, NONE, value, Ty::STRING, n);
            }
            call(&mut b, 88, &[value], Ty::VOID);
        }
        if read_again {
            let value = b.get(p, Ty::STRING, n);
            call(&mut b, 7, &[value], Ty::VOID);
        }
        let root = b.close_block(block, None, Ty::VOID, n);
        Ok((b.finish(root, &[]).map_err(|e| format!("{e:?}"))?, p.raw()))
    }

    #[test]
    fn a_needed_copy_does_not_also_own_its_source_parameter() -> Result<(), String> {
        let facts = CopyFacts {
            copies: Tri::Yes,
            drop: Tri::No,
        };
        for read_again in [false, true] {
            let (mut b, p) = copied_body(1, 0, read_again)?;
            let mut scratch = super::super::OwnScratch::default();
            super::super::run(&mut b, &facts, &mut scratch)?;
            let errors = crate::ir::verify_owned(&b, &facts);
            assert!(errors.is_empty(), "{errors:?}");
            let source = b
                .tags
                .iter()
                .enumerate()
                .find(|(i, tag)| **tag == Tag::Read && b.view(b.data[*i][0]).local() == Some(p))
                .map(|(i, _)| i)
                .ok_or("source read")?;
            assert_eq!(
                b.data[source][1],
                if read_again {
                    ReadMode::Copy
                } else {
                    ReadMode::Move
                } as u32
            );
            let summary = super::super::summary(&b, &facts, &scratch, None).ok_or("summary")?;
            assert_eq!(
                (summary.kept, summary.borrows),
                if read_again { (0, 1) } else { (1, 0) }
            );
        }
        Ok(())
    }

    #[test]
    fn ending_the_input_before_a_later_owned_call_preserves_count_donation() -> Result<(), String> {
        let facts = CopyFacts {
            copies: Tri::Yes,
            drop: Tri::No,
        };
        let mut build = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let block = build.open_block();
        let input = build.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
        let local = build.local(Ty::STRING, Symbol::NONE, 0, n);
        let value = build.get(input, Ty::STRING, n);
        build.set(local, value, n);
        let value = build.get(input, Ty::STRING, n);
        call(&mut build, 7, &[value], Ty::VOID);
        let value = build.get(local, Ty::STRING, n);
        call(&mut build, 88, &[value], Ty::VOID);
        let root = build.close_block(block, None, Ty::VOID, n);
        let mut b = build.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        let mut scratch = super::super::OwnScratch::default();
        super::super::run(&mut b, &facts, &mut scratch)?;
        assert_eq!(
            super::super::summary(&b, &facts, &scratch, None)
                .ok_or("summary")?
                .kept,
            1
        );
        let errors = crate::ir::verify_owned(&b, &facts);
        assert!(errors.is_empty(), "{errors:?}");
        Ok(())
    }

    #[test]
    fn copied_parameter_proof_rejects_mutation_capture_address_drop_and_donation()
    -> Result<(), String> {
        for control in 0..8 {
            let (mut b, p) = copied_body(1, 0, true)?;
            for i in 0..b.len() {
                if b.tags[i] == Tag::Read {
                    b.data[i][1] = ReadMode::Copy as u32;
                }
            }
            let mut t = Tables::default();
            t.reset(b.len(), b.local_ty.len());
            t.l[p as usize].kept = 1;
            let mut facts = CopyFacts {
                copies: Tri::Yes,
                drop: Tri::No,
            };
            match control {
                0 => t.l[p as usize].captured = true,
                1 => t.l[p as usize].reached = true,
                2 => t.l[p as usize].stores = 1,
                3 => t.l[p as usize].changed = true,
                4 => facts.drop = Tri::Yes,
                5 => facts.copies = Tri::No,
                6 => b.data[0][1] = ReadMode::Move as u32,
                7 => {}
                _ => return Err("bad test control".into()),
            }
            let topo = Topology::new(&b);
            assert_eq!(
                lend_copied_parameters(
                    &mut b,
                    &facts,
                    &topo,
                    &mut t,
                    &mut splice::Tables::default()
                ),
                control == 7
            );
        }
        Ok(())
    }

    #[test]
    fn copied_parameter_work_is_linear_at_width_1000_depth_20() -> Result<(), String> {
        for width in [1, 1000] {
            for depth in [1, 20] {
                let (mut b, p) = copied_body(width, depth, true)?;
                for i in 0..b.len() {
                    if b.tags[i] == Tag::Read {
                        b.data[i][1] = ReadMode::Copy as u32;
                    }
                }
                let mut t = Tables::default();
                t.reset(b.len(), b.local_ty.len());
                t.l[p as usize].kept = u32_of(width);
                let facts = CopyFacts {
                    copies: Tri::Yes,
                    drop: Tri::No,
                };
                let mut work = 0;
                let topo = Topology::new(&b);
                assert!(lend_copied_parameters_with(
                    &mut b,
                    &facts,
                    &topo,
                    &mut t,
                    &mut splice::Tables::default(),
                    || work += 1
                ));
                // Four instruction visits, one parameter and each operand
                // edge once. No body or coercion-chain retry.
                assert_eq!(
                    work,
                    4 * b.len() + width * (depth + 1) + 2 + b.local_ty.len()
                );
            }
        }
        Ok(())
    }

    struct ReadOnly;
    impl OwnCx for ReadOnly {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
        fn may_write(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(9))
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
        fn holds_ref(&self, ty: Ty) -> bool {
            ty == LOCAL_REF
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
    }

    fn call(b: &mut TirBuilder, def: u32, args: &[Ref], ty: Ty) -> Ref {
        b.call(
            &Callee::Item {
                def: DefId::from_raw(def),
                targs: TyList::EMPTY,
            },
            args,
            Providers::None,
            ty,
            NodeIdx::from_raw(1),
        )
    }

    #[test]
    fn addressed_parameters_lend_for_immutable_frame_references() -> Result<(), String> {
        // A mutable or escaping address, or a possibly writing call, must
        // retain the snapshot. Multiple reads inside this frame need none.
        for control in 0..8 {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let local = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
            let place = b.place(
                &format!("[${}]", local.raw())
                    .parse()
                    .map_err(|e| format!("{e:?}"))?,
            );
            let flags = ref_word::LOCAL | if control == 1 { ref_word::MUT } else { 0 };
            let r = b.emit(Tag::Ref, place, flags, LOCAL_REF, n);
            let result = if control == 3 || control == 7 {
                LOCAL_REF
            } else {
                Ty::VOID
            };
            let returned = if control == 5 || control == 6 {
                let through = b.place(
                    &format!("[*%{}]", r.0)
                        .parse()
                        .map_err(|e| format!("{e:?}"))?,
                );
                let value = b.emit(Tag::Read, through, ReadMode::Use as u32, Ty::STRING, n);
                call(&mut b, 8, &[value], Ty::VOID)
            } else {
                call(&mut b, 8, &[r], result)
            };
            if control == 2 {
                call(&mut b, 8, &[r], Ty::VOID);
            }
            if control == 4 || control == 6 {
                call(&mut b, 9, &[], Ty::VOID);
            }
            let root = b.close_block(
                block,
                (control == 7).then_some(returned),
                if control == 7 { LOCAL_REF } else { Ty::VOID },
                n,
            );
            let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            super::super::run(
                &mut body,
                &ReadOnly,
                &mut super::super::OwnScratch::default(),
            )
            .map_err(|e| format!("{e:?}"))?;
            let errors = crate::ir::verify_owned(&body, &ReadOnly);
            assert!(
                errors.is_empty(),
                "control {control}: {errors:?}\n{}",
                crate::ir::print(&body)
            );
            assert_eq!(
                body.exit_drops.is_empty(),
                matches!(control, 0 | 2 | 3 | 5),
                "control {control}"
            );
        }
        Ok(())
    }

    #[test]
    fn addressed_bindings_keep_the_source_live_through_the_call() -> Result<(), String> {
        for mutate in [false, true] {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let source = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
            let temporary = b.local(Ty::STRING, Symbol::NONE, 0, n);
            let value = b.get(source, Ty::STRING, n);
            b.set(temporary, value, n);
            let place = b.place(&"[$1]".parse().expect("temporary's place"));
            let address = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL_REF, n);
            let address = b.coerce(Coercion::Identity, NONE, address, LOCAL_REF, n);
            if mutate {
                let fresh = call(&mut b, 8, &[], Ty::STRING);
                b.set(source, fresh, n);
            }
            call(&mut b, 8, &[address], Ty::VOID);
            let root = b.close_block(block, None, Ty::VOID, n);
            let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            super::super::run(
                &mut body,
                &ReadOnly,
                &mut super::super::OwnScratch::default(),
            )?;
            let errors = crate::ir::verify_owned(&body, &ReadOnly);
            assert!(errors.is_empty(), "{errors:?}\n{}", crate::ir::print(&body));
            let read = (0..body.len())
                .find(|&i| {
                    body.tags[i] == Tag::Read
                        && body.view(body.data[i][0]).local() == Some(source.raw())
                })
                .unwrap_or_else(|| panic!("the source copy\n{}", crate::ir::print(&body)));
            assert_eq!(body.data[read][1] == ReadMode::Borrow as u32, !mutate);
            if mutate {
                body.data[read][1] = ReadMode::Borrow as u32;
                let errors = crate::ir::verify_owned(&body, &ReadOnly);
                assert!(errors.iter().any(|e| e.what.contains("live")), "{errors:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn lending_call_work_is_linear_in_width_and_coercion_depth() -> Result<(), String> {
        for (width, depth) in [
            (1, 1),
            (1000, 1),
            (10_000, 1),
            (1, 20),
            (1000, 20),
            (10_000, 20),
        ] {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let mut references = Vec::new();
            for _ in 0..width {
                let local = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
                let place = b.place(
                    &format!("[${}]", local.raw())
                        .parse()
                        .map_err(|e| format!("{e:?}"))?,
                );
                let mut r = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL_REF, n);
                references.push(r.0 as usize);
                for _ in 0..depth {
                    r = b.coerce(Coercion::Identity, NONE, r, LOCAL_REF, n);
                }
                call(&mut b, 8, &[r], Ty::VOID);
            }
            let root = b.close_block(block, None, Ty::VOID, n);
            let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let (mut counts, mut last) = (vec![0; body.len()], vec![NONE; body.len()]);
            let mut ops = Vec::new();
            for i in 0..body.len() {
                operands(&body, i, &mut ops);
                for op in &ops {
                    if op.role != Role::Dropped {
                        counts[op.value as usize] += 1;
                        last[op.value as usize] = u32_of(i);
                    }
                }
            }
            let mut visits = 0;
            let calls = local_lending_calls_with(&body, &ReadOnly, &counts, &last, || visits += 1);
            assert!(references.iter().all(|&r| calls[r] != NONE));
            assert!(visits <= 3 * body.len(), "{width}x{depth}: {visits} visits");
            // The same immutable address, after frame planning, keeps its
            // source live at the final call through every coercion.
            for &r in &references {
                body.data[r][1] |= ref_word::FRAME;
            }
            let mut visits = 0;
            let roots = frame_reference_roots(&body, &body.scope_forwarders(|| {}), || visits += 1);
            assert!(visits <= 5 * body.len() + 4 * body.local_ty.len());
            for &r in &references {
                let local = body.local_in_place(body.data[r][0]).unwrap().raw();
                assert!(roots[r..=r + depth].iter().all(|&root| root == local));
            }
            for &r in &references {
                body.data[r][1] |= ref_word::MUT;
            }
            assert!(
                frame_reference_roots(&body, &body.scope_forwarders(|| {}), || {})
                    .iter()
                    .all(|&root| root == NONE)
            );
        }
        Ok(())
    }

    #[test]
    fn frame_origin_work_is_linear_through_single_definition_locals() -> Result<(), String> {
        for width in [1_000, 10_000] {
            for depth in [1, 20] {
                let n = NodeIdx::from_raw(1);
                let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
                let block = b.open_block();
                let source = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
                let place = b.place(&"[$0]".parse().expect("source place"));
                let mut expected = Vec::new();
                for _ in 0..width {
                    let mut value = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL_REF, n);
                    for _ in 0..depth {
                        let alias = b.local(LOCAL_REF, Symbol::NONE, 0, n);
                        b.set(alias, value, n);
                        value = b.get(alias, LOCAL_REF, n);
                        expected.push(value.0 as usize);
                    }
                    call(&mut b, 8, &[value], Ty::VOID);
                }
                let root = b.close_block(block, None, Ty::VOID, n);
                let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
                for i in 0..body.len() {
                    if body.tags[i] == Tag::Ref {
                        body.data[i][1] |= ref_word::FRAME;
                    }
                }
                let mut work = 0;
                let roots =
                    frame_reference_roots(&body, &body.scope_forwarders(|| {}), || work += 1);
                assert!(work <= 5 * body.len() + 4 * body.local_ty.len());
                assert!(expected.iter().all(|&value| roots[value] == source.raw()));
                let mut queries = 0;
                assert!(
                    frame_lending_roots_with(&body, &ReadOnly, &roots, || queries += 1)
                        [source.idx()]
                );
                assert_eq!(
                    queries, 2,
                    "one query each for the reference and void types"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_reassigned_reference_cannot_lend_its_first_origin() -> Result<(), String> {
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        let left = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
        let right = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, n);
        let alias = b.local(LOCAL_REF, Symbol::NONE, 0, n);
        for source in [left, right] {
            let place = b.place(
                &format!("[${}]", source.raw())
                    .parse()
                    .map_err(|e| format!("{e:?}"))?,
            );
            let reference = b.emit(Tag::Ref, place, ref_word::LOCAL, LOCAL_REF, n);
            b.set(alias, reference, n);
        }
        let read = b.get(alias, LOCAL_REF, n);
        call(&mut b, 8, &[read], Ty::VOID);
        let root = b.close_block(block, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        for i in 0..body.len() {
            if body.tags[i] == Tag::Ref {
                body.data[i][1] |= ref_word::FRAME;
            }
        }
        let roots = frame_reference_roots(&body, &body.scope_forwarders(|| {}), || {});
        assert_eq!(roots[read.0 as usize], NONE);
        let lends = frame_lending_roots(&body, &ReadOnly, &roots);
        assert!(!lends[left.idx()] && !lends[right.idx()]);
        Ok(())
    }
}

#[cfg(test)]
mod aggregate_identity_tests {
    use super::*;
    use crate::ir::{TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;
    use std::cell::Cell;

    const BASE: u32 = 2_000_000;
    const TAIL: Ty = Ty(BASE + 1000);
    const NEWTYPE: Ty = Ty(BASE + 1001);
    struct Facts {
        queries: Cell<usize>,
        reject: u8,
    }
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty.0 >= BASE
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
        fn holds_ref(&self, _: Ty) -> bool {
            self.reject == 2
        }
        fn holds_drop(&self, _: Ty) -> Tri {
            if self.reject == 1 { Tri::Yes } else { Tri::No }
        }
        fn copies(&self, _: Ty) -> Tri {
            if self.reject == 3 { Tri::No } else { Tri::Yes }
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            true
        }
        fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool {
            self.queries.set(self.queries.get() + 1);
            out.clear();
            if self.reject == 5 {
                return false;
            }
            if ty == TAIL {
                out.extend([Ty::I32, Ty::STRING]);
                return true;
            }
            if ty == NEWTYPE || variant == Some(1) {
                out.push(Ty::STRING);
                return true;
            }
            let source = if ty.0 == BASE {
                Ty::STRING
            } else {
                Ty(ty.0 - 1)
            };
            out.extend([
                source,
                if self.reject == 4 {
                    Ty::STRING
                } else {
                    Ty::I32
                },
            ]);
            true
        }
    }

    fn generated(width: usize, depth: usize, second_string: bool) -> (Body, Vec<(u32, u32)>) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(0);
        let block = b.open_block();
        let param = b.local(Ty::STRING, Symbol::NONE, local_flags::PARAM, syn);
        let zero = b.const_value(Ty::I32, 0);
        let mut roots = vec![];
        for _ in 0..width {
            let source = b.get(param, Ty::STRING, syn);
            let mut value = source;
            for d in 0..depth {
                let field = if second_string { source } else { zero };
                let fields = b.refs_record(&[value, field]);
                value = b.emit(Tag::NewStruct, NONE, fields, Ty(BASE + u32_of(d)), syn);
                roots.push((value.0, source.0));
            }
        }
        let block = b.close_block(block, None, Ty::VOID, syn);
        let mut b = b.finish(block, &[]).expect("typed constructor graph");
        for &(_, root) in &roots {
            b.data[root as usize][1] = ReadMode::Borrow as u32;
        }
        (b, roots)
    }

    #[test]
    fn aggregate_identity_work_is_linear_in_width_and_depth() {
        for width in [1_000, 10_000] {
            for depth in [1, 20] {
                let (mut b, roots) = generated(width, depth, false);
                let cx = Facts {
                    queries: Cell::new(0),
                    reject: 0,
                };
                let mut work = 0;
                let sources = rc_sources_with(&b, &cx, |n| work += n);
                assert_eq!(work, 3 * b.len() + 2 * depth);
                assert_eq!(
                    cx.queries.get(),
                    depth,
                    "one layout query per concrete shape"
                );
                let mut edges = 0;
                b.forwarding_reads(&sources, || edges += 1);
                assert_eq!(edges, b.len());
                b.index_forwarded_reads(&sources);
                assert_eq!(b.forwarded_reads.len(), roots.len());
                for (value, source) in roots {
                    assert_eq!(b.borrowed_alias(value), Some(source));
                }
            }
        }
    }

    #[test]
    fn aggregate_identity_covers_struct_tuple_rest_variant_newtype_and_optional_lift() {
        for form in 0..6 {
            let (mut b, roots) = generated(1, 1, false);
            let (value, read) = roots[0];
            let i = value as usize;
            let scalar = b.record(b.data[i][1])[1];
            match form {
                0 => {}
                1 => {
                    b.tags[i] = Tag::NewTuple;
                }
                2 => {
                    b.tags[i] = Tag::NewVariant;
                    b.data[i][0] = 7;
                }
                3 => {
                    b.tags[i] = Tag::NewTuple;
                    b.ty[i] = TAIL;
                    b.data[i] = [read, u32_of(b.extra.len())];
                    b.extra.extend([1, scalar]);
                }
                4 => {
                    b.ty[i] = NEWTYPE;
                    b.data[i][1] = u32_of(b.extra.len());
                    b.extra.extend([1, read]);
                }
                _ => {
                    b.tags[i] = Tag::Coerce;
                    b.data[i] = [read, u32_of(b.extra.len())];
                    b.extra
                        .extend([2, crate::ir::Coercion::WrapSome as u32, NONE]);
                }
            }
            let cx = Facts {
                queries: Cell::new(0),
                reject: 0,
            };
            let sources = rc_sources(&b, &cx);
            assert_eq!(sources[i], read, "form {form}");
            b.index_forwarded_reads(&sources);
            assert_eq!(b.borrowed_alias(value), Some(read), "form {form}");
        }
    }

    #[test]
    fn aggregate_identity_requires_copy_layout_and_one_nontrivial_operand() {
        for reject in 1..=5 {
            let (b, _) = generated(2, 1, reject == 4);
            let cx = Facts {
                queries: Cell::new(0),
                reject,
            };
            assert!(rc_sources(&b, &cx).iter().all(|&v| v == NONE));
        }
    }
}
