//! Insertion (`docs/compiler/tir-v2.md`, "Insertion"): the naive, correct
//! ownership of `crates/hd_wasm_linear/DESIGN.md` §23.16 with releases at
//! last use (§23.20), made explicit.
//!
//! 1. **Modes.** Every `use` read becomes `copy`, but a read through a
//!    pointer (`[ptr %p]`) is a `move`: it takes the heap's count.
//! 2. **Temporaries.** An owned value whose one use consumes it, as a
//!    statement of the value's own block (or as that block's tail, which
//!    the block's owner takes), is consumed there and gets no `Drop`. Any
//!    other consumer gets a `Read copy` of the value just before it, which
//!    it consumes instead, so a consumed value always has exactly one use;
//!    the value then gets a `Drop` after the statement of its **home**
//!    block that holds its last use. The drops after one statement run
//!    youngest first, so a binding made from a value (a payload's) is
//!    dropped before that value. The home is the innermost block
//!    around the value that holds every use: its own block, or for a value
//!    a match decision makes and an arm reads, the block of the `Match`.
//! 3. **Locals.** An owned local gets a `Drop` after the statement of the
//!    innermost block holding all its accesses that holds the last one
//!    (the emitter's old rule, §23.20). Parameters and captures live in their
//!    sub-body's root block. Every owned local is in its sub-body's exit
//!    set.
//! 4. **Jumps.** A `Break`, `Continue` or `ToArm` gets a `Drop` before it
//!    of each value and local whose home it leaves while it may still hold
//!    a count; a `Return` of each such value (the exit set drops the
//!    locals). A place that a crossed cleanup scope's suite reads is left
//!    for the suite: its reads count at every exit of its scope, so the
//!    drop after the scope covers the normal path and the exit set the
//!    jump's.
//!
//! 5. **Uniqueness.** Every in-place change of a collection place gets an
//!    `IsUnique` of that place just before it: a `Store` through an element
//!    projection (each list on its path, outermost first), and `ListPush`,
//!    `MapSet` and `MapRemove`, of their collection operand. A change goes
//!    through the reads its place or operand was made from (`xs[i].push(v)`
//!    pushes to the element that `Read [$xs [%i]]` read): each list on such
//!    a read's path gets an `IsUnique` before that read, outermost first,
//!    so the read sees the buffer the change will reach; the walk stops at
//!    a read through a dereference, whose change reaches the referent. A
//!    push's list or a map store's or removal's map read from a place
//!    rooted at a local or a top-level binding names that place instead:
//!    the `IsUnique` is of the place, and the change reads the place right
//!    after it, again where an argument may write, looking each map value
//!    step's entry up again with its key, since growth renumbers entries
//!    ([`changes`]). The backend's intrinsics then check nothing
//!    themselves.
//! 6. **Pins.** A `Ref` of an element place gets an `IsUnique` and a `Pin`
//!    of each list on its path, and of each list on the path of a read its
//!    root was made from, outermost first. The reference holds a count of
//!    the buffer, so the pin ends with the reference's drop (rule 2 or 3);
//!    nothing else releases it. A `for` over element references pins its
//!    list this way at each step, from the step's reference to the end of
//!    the step, where its binding is dropped.
//!
//! Why it is sound: on every path an owned value is consumed by its one
//! use or dropped once, after its last use or before a jump that leaves
//! its home; a local is dropped after its last access, before a jump that
//! leaves its block, or at the exit, where a place already dropped holds
//! zeros and drops as nothing.

use hd_base::{LocalId, NodeIdx, SubId};
use hd_types::Ty;

use super::{
    Loc, Operand, OwnCx, OwnScratch, Role, Tri, consumes, fill, operands, splice, suite_touches,
    tracked,
};
use crate::ir::{
    Body, CaptureMode, IntrinsicOp, NONE, PlaceView, ReadMode, Ref, Root, Tag, Topology,
    local_flags, place, u32_of,
};

/// A place a jump may need to drop: a value or a local (bit 31 set),
/// homed in block `blk`, live from statement `first` (a value's
/// definition) to statement `last` (its consumer's or its last use's, a
/// local's last access).
#[derive(Clone, Copy)]
struct Item {
    blk: u32,
    what: u32,
    first: u32,
    last: u32,
}

const LOCAL_BIT: u32 = 1 << 31;

/// Insertion's dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    tracked: Vec<bool>,
    /// The uses of each tracked value, in compressed rows by value.
    pub(super) start: Vec<u32>,
    fill: Vec<u32>,
    pub(super) uses: Vec<(u32, Operand)>,
    ops: Vec<Operand>,
    /// The chain of blocks around a place, innermost first, with the
    /// statement position in each; `mark` is a chain block's level plus 1.
    mark: Vec<u32>,
    pub(super) chain: Vec<(u32, u32)>,
    levels: Vec<(u32, u32)>,
    /// Local accesses as (sub-body, local, instruction), sorted.
    acc: Vec<(u32, u32, u32)>,
    /// Entry locals as (sub-body, local), sorted: parameters and a
    /// closure body's captures.
    entries: Vec<(u32, u32)>,
    /// Per local: a stamp of the sub-body whose entry local it is while
    /// that sub-body's locals are placed, whether a cell may hold it, and
    /// whether it was placed.
    entry: Vec<u32>,
    celled: Vec<bool>,
    placed: Vec<u32>,
    /// The places jumps may drop, in compressed rows by block.
    items: Vec<Item>,
    item_start: Vec<u32>,
    sorted: Vec<Item>,
    /// A crossed suite's reads carry the jump's stamp.
    local_stamp: Vec<u32>,
    value_stamp: Vec<u32>,
    stack: Vec<u32>,
    exits: Vec<(SubId, LocalId)>,
    /// Per read a change goes through (rules 5 and 6): whether the lists
    /// on its path have their `IsUnique` before it (bit 1) and their `Pin`
    /// (bit 2), so a read two changes go through gets them once.
    walked: Vec<u8>,
    /// The collection changes' operands ([`changes`]), and as (key,
    /// change) each key a change looks a map entry up with again.
    changes: Vec<Change>,
    relook: Vec<(u32, u32)>,
    /// Per read: the change that reads its place again, or `NONE`.
    reread: Vec<u32>,
    /// The element projections of one place: the `extra` position of each
    /// element word.
    elems: Vec<u32>,
    /// The drops after last uses (rules 2 and 3), placed once every value
    /// and local is placed, so that the drops after one statement run
    /// youngest first ([`after_drops`]).
    after: Vec<After>,
}

/// A `Drop` after a last use: its block and position, its place's age (the
/// instruction that defines the value, or a local's first access; 0 for
/// an entry local), and its place word and syntax node.
#[derive(Clone, Copy)]
struct After {
    blk: u32,
    pos: u32,
    age: u32,
    word: u32,
    syn: NodeIdx,
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.tracked, n, false);
        fill(&mut self.start, n + 1, 0);
        fill(&mut self.fill, n + 1, 0);
        fill(&mut self.mark, n, 0);
        fill(&mut self.entry, locals, NONE);
        fill(&mut self.celled, locals, false);
        fill(&mut self.placed, locals, NONE);
        fill(&mut self.item_start, n + 1, 0);
        fill(&mut self.local_stamp, locals, NONE);
        fill(&mut self.value_stamp, n, NONE);
        fill(&mut self.walked, n, 0);
        fill(&mut self.reread, n, NONE);
        self.elems.clear();
        self.changes.clear();
        self.relook.clear();
        self.after.clear();
        self.uses.clear();
        self.ops.clear();
        self.chain.clear();
        self.levels.clear();
        self.acc.clear();
        self.entries.clear();
        self.items.clear();
        self.sorted.clear();
        self.stack.clear();
        self.exits.clear();
    }

    /// Walks up from `(blk, pos)` to the first block of the marked chain:
    /// its level and the statement position there.
    pub(super) fn meet(&self, topo: &Topology, mut blk: u32, mut pos: u32) -> Option<(u32, u32)> {
        loop {
            let m = self.mark[blk as usize];
            if m != 0 {
                return Some((m - 1, pos));
            }
            (blk, pos) = topo.up(blk)?;
        }
    }

    /// Marks the chain of blocks from `(blk, pos)` up.
    pub(super) fn mark_chain(&mut self, topo: &Topology, mut blk: u32, mut pos: u32) {
        self.chain.clear();
        loop {
            self.chain.push((blk, pos));
            self.mark[blk as usize] = u32::try_from(self.chain.len()).expect("depth");
            match topo.up(blk) {
                Some(next) => (blk, pos) = next,
                None => break,
            }
        }
    }

    pub(super) fn unmark_chain(&mut self) {
        for &(blk, _) in &self.chain {
            self.mark[blk as usize] = 0;
        }
    }
}

/// Where use `(u, op)` stands: `u`'s block and position, or for a block's
/// tail, the end of that block.
fn at(b: &Body, topo: &Topology, u: u32, op: &Operand) -> Option<(u32, u32)> {
    if op.role == Role::Tail {
        return Some((u, block_len(b, u)));
    }
    let blk = topo.blk[u as usize];
    (blk != NONE).then(|| (blk, topo.pos[u as usize]))
}

/// The owner a use's block tail goes to, for [`consumes`].
fn tail_owner(topo: &Topology, u: u32, op: &Operand) -> u32 {
    if op.role == Role::Tail && !topo.is_root[u as usize] {
        topo.owner[u as usize]
    } else {
        NONE
    }
}

fn block_len(b: &Body, blk: u32) -> u32 {
    b.extra
        .get(b.data[blk as usize][0] as usize)
        .copied()
        .unwrap_or(0)
}

/// The root local of a place word, or `NONE` for a top-level binding.
fn named_root(b: &Body, word: u32) -> Option<u32> {
    match b.view(word).root {
        Root::Local(l) => Some(l),
        Root::Global(..) => Some(NONE),
        _ => None,
    }
}

/// The syntax node of statement `pos` of block `blk`, for a statement the
/// pass places after it, so the dumps show it beside that statement.
fn statement_syn(b: &Body, blk: u32, pos: u32) -> NodeIdx {
    b.record(b.data[blk as usize][0])
        .get(pos as usize)
        .map_or(b.syn[blk as usize], |&x| b.syn[x as usize])
}

/// A new place record `[%v]`.
fn value_place(b: &mut Body, v: u32) -> u32 {
    let at = u32_of(b.extra.len());
    b.extra.extend([2, place::ROOT_VALUE, v]);
    at
}

/// Runs insertion over every sub-body of `b`.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, scratch: &mut OwnScratch) -> Result<(), String> {
    for (i, &tag) in b.tags.iter().enumerate() {
        match tag {
            Tag::CallDyn | Tag::ContextFor | Tag::Hole | Tag::Poison => {
                return Err(format!("`{}` at %{i}", tag.name()));
            }
            Tag::Drop | Tag::IsUnique | Tag::Pin | Tag::FlagSet | Tag::FlagClear => {
                return Err(format!("an ownership statement `{}` at %{i}", tag.name()));
            }
            Tag::Call | Tag::Await => {
                let r = b.record(b.data[i][1]);
                if r.len() < 3 || r[r.len() - 3] > 2 {
                    return Err(format!("pending call providers at %{i}"));
                }
            }
            _ => {}
        }
    }
    if !b.exit_drops.is_empty() {
        return Err("an exit set before insertion".into());
    }
    let n = b.len();
    scratch.topo.build(b);
    let (topo, t, edits) = (&scratch.topo, &mut scratch.insert, &mut scratch.splice);
    edits.clear();
    t.reset(n, b.local_ty.len());
    for i in 0..n {
        t.tracked[i] = tracked(b, cx, i);
    }
    collect_uses(b, t);
    for i in 0..n {
        changes(b, topo, t, i);
    }
    values(b, topo, cx, t, edits)?;
    locals(b, topo, cx, t, edits);
    after_drops(t, edits);
    jumps(b, topo, t, edits);
    uniqueness(b, topo, t, edits);
    for i in 0..n {
        if b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Use as u32 {
            // A read through a pointer moves the value out
            // (`module.rt.ptr.read`).
            let ptr = b.ptr_target(i).is_some();
            b.data[i][1] = if ptr { ReadMode::Move } else { ReadMode::Copy } as u32;
        }
    }
    t.exits.sort_unstable_by_key(|(s, l)| (s.raw(), l.raw()));
    t.exits.dedup();
    b.exit_drops.clone_from(&t.exits);
    splice::apply(b, topo, edits)
}

/// The uses of each tracked value.
fn collect_uses(b: &Body, t: &mut Tables) {
    let n = b.len();
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped && t.tracked[op.value as usize] {
                t.start[op.value as usize + 1] += 1;
            }
        }
    }
    for v in 0..n {
        t.start[v + 1] += t.start[v];
    }
    let blank = (
        NONE,
        Operand {
            value: NONE,
            role: Role::Lend,
            at: Loc::Data(0),
        },
    );
    t.uses.resize(t.start[n] as usize, blank);
    t.fill.copy_from_slice(&t.start);
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped && t.tracked[op.value as usize] {
                let v = op.value as usize;
                t.uses[t.fill[v] as usize] = (u32_of(i), *op);
                t.fill[v] += 1;
            }
        }
    }
    t.ops = ops;
}

/// Rule 2: each tracked value is consumed by its one use, or each consumer
/// gets a copy and the value a `Drop` after its home's last use.
fn values(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    for v in 0..b.len() {
        if !t.tracked[v] || topo.blk[v] == NONE {
            continue;
        }
        let (lo, hi) = (t.start[v] as usize, t.start[v + 1] as usize);
        t.mark_chain(topo, topo.blk[v], topo.pos[v]);
        t.levels.clear();
        let mut home = 0;
        let mut stray = None;
        for k in lo..hi {
            let (u, op) = t.uses[k];
            // A change that reads its place again no longer uses it.
            if u == t.reread[v] {
                continue;
            }
            match at(b, topo, u, &op).and_then(|(blk, pos)| t.meet(topo, blk, pos)) {
                Some(met) => {
                    home = home.max(met.0);
                    t.levels.push(met);
                }
                None => stray = Some(u),
            }
        }
        // A key is used again where its change looks its entry up.
        for x in 0..t.relook.len() {
            let u = t.relook[x].1 as usize;
            if t.relook[x].0 == u32_of(v)
                && let Some(met) = t.meet(topo, topo.blk[u], topo.pos[u])
            {
                home = home.max(met.0);
                t.levels.push(met);
            }
        }
        let (h, first) = t.chain[home as usize];
        let last = t
            .levels
            .iter()
            .filter(|(level, _)| *level == home)
            .map(|(_, p)| *p)
            .fold(first, u32::max);
        let own = t.chain[0].0;
        t.unmark_chain();
        if let Some(u) = stray {
            return Err(format!("%{u} uses %{v} outside the blocks around it"));
        }
        // One use, which consumes it as a statement of its own block or as
        // that block's tail.
        if let [(u, op)] = t.uses[lo..hi] {
            let direct = if op.role == Role::Tail {
                u == own
            } else {
                topo.blk[u as usize] == own
            };
            if direct && consumes(b, cx, u as usize, &op, tail_owner(topo, u, &op)) {
                let last = match op.role {
                    Role::Tail => block_len(b, own).saturating_sub(1),
                    _ => topo.pos[u as usize].saturating_sub(1),
                };
                t.items.push(Item {
                    blk: own,
                    what: u32_of(v),
                    first: topo.pos[v],
                    last,
                });
                continue;
            }
        }
        for k in lo..hi {
            let (u, op) = t.uses[k];
            if u == t.reread[v] {
                continue;
            }
            let consumed = consumes(b, cx, u as usize, &op, tail_owner(topo, u, &op));
            let (blk, pos) = match op.role {
                Role::Tail => (u, block_len(b, u)),
                _ => (topo.blk[u as usize], topo.pos[u as usize]),
            };
            if !consumed {
                continue;
            }
            let word = value_place(b, u32_of(v));
            let copy = edits.insert(
                blk,
                pos,
                Tag::Read,
                [word, ReadMode::Copy as u32],
                b.ty[v],
                b.syn[u as usize],
            );
            edits.rewrite(u, op.at, copy);
        }
        let syn = statement_syn(b, h, last);
        let word = value_place(b, u32_of(v));
        let at = (last + 1).min(block_len(b, h));
        t.after.push(After {
            blk: h,
            pos: at,
            age: u32_of(v),
            word,
            syn,
        });
        t.items.push(Item {
            blk: h,
            what: u32_of(v),
            first,
            last,
        });
    }
    Ok(())
}

/// The drops after last uses, youngest first where several follow one
/// statement: a binding that borrows from a value made before it (a match
/// arm's payload binding, from the scrutinee) is dropped before that value,
/// so the borrow's span ends before the value's drop.
fn after_drops(t: &mut Tables, edits: &mut splice::Tables) {
    t.after
        .sort_by_key(|a| (a.blk, a.pos, std::cmp::Reverse(a.age)));
    for a in &t.after {
        edits.insert(a.blk, a.pos, Tag::Drop, [a.word, NONE], Ty::VOID, a.syn);
    }
}

/// Rule 3: each owned local's `Drop` after its last access, and its exit
/// row.
fn locals(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables) {
    let n = b.len();
    // The entry locals: each closure body's parameters and captures, and
    // the body's own parameters, those in no closure's list.
    let mut in_sub = std::mem::take(&mut t.placed);
    for s in 1..b.sub_params.len() {
        let r = b.sub_params[s];
        for k in 0..r.len {
            let l = b.extra[(r.start + 1 + k) as usize];
            t.entries.push((u32_of(s), l));
            in_sub[l as usize] = u32_of(s);
        }
    }
    for (l, (&flags, sub)) in b.local_flags.iter().zip(in_sub.iter_mut()).enumerate() {
        if flags & local_flags::PARAM != 0 && *sub == NONE {
            t.entries.push((0, u32_of(l)));
        }
        *sub = NONE;
    }
    t.placed = in_sub;
    for i in 0..n {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref => {
                if let Some(l) = b.view(a).local() {
                    t.acc.push((topo.sub[i], l, u32_of(i)));
                    // A reference to the local's own storage puts it in a
                    // cell; one through it, to a referent or an element,
                    // does not.
                    if b.tags[i] == Tag::Ref && b.local_in_place(a).is_some() {
                        t.celled[l as usize] = true;
                    }
                }
            }
            Tag::Closure => {
                let rec = b.record(w);
                let (start, len) = (rec[0] as usize, rec[1] as usize);
                for c in start..start + len {
                    let l = b.cap_local[c].raw();
                    t.acc.push((topo.sub[i], l, u32_of(i)));
                    if b.cap_mode.get(c) == Some(&CaptureMode::Shared) {
                        t.celled[l as usize] = true;
                    }
                    if !b.is_instantiation(i) {
                        t.entries.push((a, l));
                    }
                }
            }
            _ => {}
        }
    }
    t.acc.sort_unstable();
    t.entries.sort_unstable();
    t.entries.dedup();
    let owned = |b: &Body, t: &Tables, l: u32| {
        t.celled[l as usize] || cx.counted(b.local_ty[l as usize]) != Tri::No
    };
    let (mut k, mut e) = (0, 0);
    for s in 0..u32_of(b.sub_root.len()) {
        let e0 = e;
        while e < t.entries.len() && t.entries[e].0 == s {
            t.entry[t.entries[e].1 as usize] = s;
            e += 1;
        }
        while k < t.acc.len() && t.acc[k].0 == s {
            let l = t.acc[k].1;
            let mut end = k;
            while end < t.acc.len() && t.acc[end].0 == s && t.acc[end].1 == l {
                end += 1;
            }
            t.placed[l as usize] = s;
            if owned(b, t, l) {
                let entry = t.entry[l as usize] == s;
                place_local(b, topo, t, s, l, k..end, entry);
            }
            k = end;
        }
        // An entry local with no access is dropped at its body's start.
        for x in e0..e {
            let l = t.entries[x].1;
            t.entry[l as usize] = NONE;
            if t.placed[l as usize] == s || !owned(b, t, l) {
                continue;
            }
            let root = b.sub_root[s as usize];
            edits.insert(
                root,
                0,
                Tag::Drop,
                [l | place::BARE_LOCAL, NONE],
                Ty::VOID,
                b.syn[root as usize],
            );
            t.exits.push((SubId::from_raw(s), LocalId::from_raw(l)));
        }
    }
}

/// The `Drop` of local `l` of sub-body `s` after its last access, its
/// accesses being `t.acc[range]`; an entry local's block is its body's
/// root.
fn place_local(
    b: &Body,
    topo: &Topology,
    t: &mut Tables,
    s: u32,
    l: u32,
    range: std::ops::Range<usize>,
    entry: bool,
) {
    let first = t.acc[range.start].2 as usize;
    if topo.blk[first] == NONE {
        return;
    }
    t.mark_chain(topo, topo.blk[first], topo.pos[first]);
    t.levels.clear();
    let mut common = 0;
    for k in range {
        let x = t.acc[k].2 as usize;
        if topo.blk[x] == NONE {
            continue;
        }
        if let Some(met) = t.meet(topo, topo.blk[x], topo.pos[x]) {
            common = common.max(met.0);
            t.levels.push(met);
        }
    }
    if entry {
        common = u32::try_from(t.chain.len() - 1).expect("depth");
    }
    let (d, at) = t.chain[common as usize];
    let last = t
        .levels
        .iter()
        .filter(|(level, _)| *level == common)
        .map(|(_, p)| *p)
        .fold(at, u32::max);
    t.unmark_chain();
    t.after.push(After {
        blk: d,
        pos: (last + 1).min(block_len(b, d)),
        age: if entry { 0 } else { u32_of(first) },
        word: l | place::BARE_LOCAL,
        syn: statement_syn(b, d, last),
    });
    t.exits.push((SubId::from_raw(s), LocalId::from_raw(l)));
    if !entry {
        t.items.push(Item {
            blk: d,
            what: l | LOCAL_BIT,
            first: 0,
            last,
        });
    }
}

/// Rule 4: the drops each jump needs before it.
fn jumps(b: &mut Body, topo: &Topology, t: &mut Tables, edits: &mut splice::Tables) {
    let n = b.len();
    for it in &t.items {
        t.item_start[it.blk as usize + 1] += 1;
    }
    for k in 0..n {
        t.item_start[k + 1] += t.item_start[k];
    }
    t.fill.copy_from_slice(&t.item_start);
    let blank = Item {
        blk: 0,
        what: 0,
        first: 0,
        last: 0,
    };
    t.sorted.resize(t.items.len(), blank);
    for k in 0..t.items.len() {
        let it = t.items[k];
        t.sorted[t.fill[it.blk as usize] as usize] = it;
        t.fill[it.blk as usize] += 1;
    }
    for j in 0..n {
        let tag = b.tags[j];
        if !matches!(tag, Tag::Break | Tag::Continue | Tag::Return | Tag::ToArm)
            || topo.blk[j] == NONE
        {
            continue;
        }
        let target = match tag {
            Tag::Break | Tag::Continue => b
                .label_inst
                .get(b.data[j][0] as usize)
                .copied()
                .unwrap_or(NONE),
            _ => NONE,
        };
        let stamp = u32_of(j);
        let (jb, jp) = (topo.blk[j], topo.pos[j]);
        let mut crossed = false;
        let (mut blk, mut ps) = (jb, jp);
        loop {
            for k in t.item_start[blk as usize]..t.item_start[blk as usize + 1] {
                let it = t.sorted[k as usize];
                if it.what & LOCAL_BIT != 0 {
                    let l = it.what & !LOCAL_BIT;
                    if tag == Tag::Return
                        || ps > it.last
                        || (crossed && t.local_stamp[l as usize] == stamp)
                    {
                        continue;
                    }
                    let word = l | place::BARE_LOCAL;
                    edits.insert(jb, jp, Tag::Drop, [word, NONE], Ty::VOID, b.syn[j]);
                } else {
                    let v = it.what;
                    if v as usize >= j
                        || ps < it.first
                        || ps > it.last
                        || (crossed && t.value_stamp[v as usize] == stamp)
                    {
                        continue;
                    }
                    let word = value_place(b, v);
                    edits.insert(jb, jp, Tag::Drop, [word, NONE], Ty::VOID, b.syn[j]);
                }
            }
            let owner = topo.owner[blk as usize];
            let done = match tag {
                Tag::Return => false,
                Tag::Break | Tag::Continue => blk == target || owner == target,
                _ => owner != NONE && b.tags[owner as usize] == Tag::Match,
            };
            if done {
                break;
            }
            // Leaving a cleanup scope's body: its suites run after the
            // jump, so what they read stays for them.
            if owner != NONE
                && b.tags[owner as usize] == Tag::Scope
                && b.data[owner as usize][0] == blk
                && b.data[owner as usize][1] != NONE
                && !b.record(b.data[owner as usize][1]).is_empty()
            {
                crossed = true;
                mark_suites(b, t, owner, stamp);
            }
            match topo.up(blk) {
                Some(next) => (blk, ps) = next,
                None => break,
            }
        }
    }
}

/// Stamps every local and value the suites of cleanup scope `scope` read
/// ([`suite_touches`]).
fn mark_suites(b: &Body, t: &mut Tables, scope: u32, stamp: u32) {
    let (ls, vs) = (&mut t.local_stamp, &mut t.value_stamp);
    let (local, value) = (
        |l: u32| ls[l as usize] = stamp,
        |v: u32| vs[v as usize] = stamp,
    );
    let roots = b.record(b.data[scope as usize][1]);
    suite_touches(b, roots, &mut t.stack, &mut t.ops, false, local, value);
}

/// Rules 5 and 6: an `IsUnique` before each in-place change of a list or
/// map place, and a `Pin` after it before each element reference; and the
/// same for the lists on the path of each read the change goes through,
/// before that read.
fn uniqueness(b: &mut Body, topo: &Topology, t: &mut Tables, edits: &mut splice::Tables) {
    for i in 0..b.len() {
        if topo.blk[i] == NONE {
            continue;
        }
        let a = b.data[i][0];
        let (blk, pos, syn) = (topo.blk[i], topo.pos[i], b.syn[i]);
        match b.tags[i] {
            Tag::Store | Tag::Ref => {
                let pin = b.tags[i] == Tag::Ref;
                let deref = path_lists(b, t, a);
                // A place rooted at a read of a local's or a binding's
                // place, which folding kept (a reference of an open type):
                // its checks name that place, and it reads the place again
                // after them, as a change does.
                let root = b.view(a).value();
                let base = root.and_then(|v| read_place(b, topo, v));
                for k in 0..t.elems.len() {
                    let word = match base {
                        Some(w) => graft(b, w, a, t.elems[k]),
                        None => prefix(b, a, t.elems[k]),
                    };
                    edits.insert(blk, pos, Tag::IsUnique, [word, 0], Ty::VOID, syn);
                    if pin {
                        edits.insert(blk, pos, Tag::Pin, [word, NONE], Ty::VOID, syn);
                    }
                }
                if let (Some(w), Some(v), false) = (base, root, t.elems.is_empty()) {
                    let w = match w & place::BARE_LOCAL {
                        0 => prefix(b, w, w + 1 + b.extra[w as usize]),
                        _ => w,
                    };
                    let mode = ReadMode::Borrow as u32;
                    let e = edits.insert(blk, pos, Tag::Read, [w, mode], b.ty[v as usize], syn);
                    edits.rewrite(u32_of(i), super::Loc::Extra(a + 2), e);
                    if lone_root_read(b, topo, t, i).is_some() {
                        edits.remove(v);
                    }
                }
                // A change through a dereference reaches the referent, not
                // the reads the reference came from.
                if !deref && let Some(root) = root {
                    through(b, topo, t, edits, root, pin);
                }
            }
            _ => {}
        }
    }
    for k in 0..t.changes.len() {
        own_place(b, topo, t, edits, t.changes[k]);
    }
}

type Change = (u32, u32, u32, Option<bool>);

/// The collection changes of instruction `i` into `t.changes`, as
/// (change, `extra` word, operand, quiet): a push's list, a map store's
/// or removal's map. Quiet is `None` for an operand that reads no place
/// rooted at a local or a top-level binding, directly or through the
/// reads of values, and else whether it reads it directly, right before
/// the change: then the read is the change's `borrow` operand, and else
/// the change reads the place again. A change whose operand
/// reads a place is an access of the local the place is rooted at, if
/// any, and an operand read again at the change owns nothing; its map
/// value steps look their entries up again there, with their keys
/// ([`Tables::relook`]).
fn changes(b: &Body, topo: &Topology, t: &mut Tables, i: usize) {
    if topo.blk[i] == NONE {
        return;
    }
    // The read a store's or a reference's place is rooted at, which
    // [`uniqueness`] replaces with a new `borrow` read, no longer needs a
    // count at the change: one only the place uses holds none, and goes;
    // any other is dropped after its last other use.
    if let Some(v) = lone_root_read(b, topo, t, i) {
        t.tracked[v as usize] = false;
    } else if let Some(v) = rerooted_read(b, topo, i)
        && t.reread[v as usize] == NONE
    {
        t.reread[v as usize] = u32_of(i);
    }
    // A store or a reference through a read of a local's place names
    // that place in its checks ([`uniqueness`]): an access of the local.
    if matches!(b.tags[i], Tag::Store | Tag::Ref)
        && let Some(l) = b
            .view(b.data[i][0])
            .value()
            .and_then(|v| read_root(b, topo, v))
            .filter(|&l| l != NONE)
    {
        t.acc.push((topo.sub[i], l, u32_of(i)));
    }
    for (at, v) in super::changed(b, i) {
        let Some(r) = Ref(v).as_inst().map(|r| r.0 as usize) else {
            continue;
        };
        // `NONE` for a place rooted at a top-level binding, which has no
        // local access to record.
        let root = (b.tags[r] == Tag::Read && topo.blk[r] != NONE)
            .then(|| named_root(b, b.data[r][0]))
            .flatten();
        // An operand through reads of a local's place, which folding kept,
        // names that place and reads it again, as one rooted there does.
        let chained = root.is_none().then(|| read_root(b, topo, v)).flatten();
        let once = t.start[r + 1] - t.start[r] == 1;
        // A read right before its change is the change's operand; one with
        // an argument between is read again, after the drops the argument
        // ends, so that no write stands between the operand's borrow and
        // the change.
        let still = once && topo.blk[r] == topo.blk[i] && topo.pos[i] == topo.pos[r] + 1;
        let kind = root
            .or(chained)
            .is_some()
            .then_some(still && chained.is_none());
        // A read used again keeps its count; the change's use moves on,
        // and a read only the change uses is a borrow: the change replaces
        // the place's buffer, which no other count may hold.
        t.tracked[r] &= kind.is_none() || !once;
        t.changes.push((u32_of(i), at, v, kind));
        if let Some(l) = root.or(chained).filter(|&l| l != NONE) {
            t.acc.push((topo.sub[i], l, u32_of(i)));
        }
        if kind == Some(false) {
            t.reread[r] = u32_of(i);
            // The keys of the map value steps of every read on the way, and
            // the value a chain from another value starts at (a reference
            // a call returned), live to the change, which reads again.
            let mut x = v;
            while b.tags[x as usize] == Tag::Read {
                path_lists(b, t, b.data[x as usize][0]);
                for k in 0..t.elems.len() {
                    if let Some(e) = entry(b, b.extra[t.elems[k] as usize + 1]) {
                        t.relook.push((b.record(b.data[e][1])[1], u32_of(i)));
                    }
                }
                let Some(u) = b.view(b.data[x as usize][0]).value() else {
                    break;
                };
                x = u;
            }
            if b.tags[x as usize] != Tag::Read {
                t.relook.push((x, u32_of(i)));
            }
        }
    }
}

/// The read that the place of `Store` or `Ref` `i` is rooted at, when
/// [`uniqueness`] gives that place a new `borrow` read and nothing else
/// needs the old one: the place has an element step, the read names a
/// local's or a top-level binding's place with no element step (whose
/// index check must stay), and the place is the read's one use. Kept, the
/// read would hold a count of the list across the `IsUnique`, which then
/// always copies the buffer: an open element type keeps such reads
/// unfolded (`crate::ir::root`, rule 3).
fn lone_root_read(b: &Body, topo: &Topology, t: &Tables, i: usize) -> Option<u32> {
    let v = rerooted_read(b, topo, i)?;
    let r = v as usize;
    (named_root(b, b.data[r][0]).is_some()
        && !has_elem(b, b.data[r][0])
        && t.start[r + 1] - t.start[r] == 1)
        .then_some(v)
}

/// The read that the place of `Store` or `Ref` `i` is rooted at, when
/// [`uniqueness`] roots the place at a new `borrow` read instead: the
/// read's chain starts at a place ([`read_place`]), and the place has an
/// element step, which gets an `IsUnique`.
fn rerooted_read(b: &Body, topo: &Topology, i: usize) -> Option<u32> {
    if !matches!(b.tags[i], Tag::Store | Tag::Ref) {
        return None;
    }
    let a = b.data[i][0];
    let v = b.view(a).value()?;
    (b.tags[v as usize] == Tag::Read && read_root(b, topo, v).is_some() && has_elem(b, a))
        .then_some(v)
}

/// Whether the place at `word` has an element projection.
fn has_elem(b: &Body, word: u32) -> bool {
    let PlaceView {
        from: mut x, to, ..
    } = b.view(word);
    while x < to {
        match b.extra[x as usize] & place::PROJ_KIND {
            place::PROJ_ELEM => return true,
            _ => x += 1,
        }
    }
    false
}

/// Rule 5 for a collection change ([`changes`]). An operand read from a
/// place is not made unique itself: the `IsUnique` names the place, and
/// the change reads the place after it, so a buffer the check or the
/// change replaces can be stored back there (`docs/compiler/tir-v2.md`,
/// "Settling the biggest risk"). A quiet operand is that read, after the
/// `IsUnique`s; any other only evaluates the receiver, first, and the
/// change takes a new `borrow` read, after `IsUnique`s that stand right
/// before it. Since a write between may grow a map on the path, and
/// growth renumbers entries, that read's map value steps take new
/// `MapEntry`s, each after its map's `IsUnique`.
fn own_place(b: &mut Body, topo: &Topology, t: &mut Tables, edits: &mut splice::Tables, c: Change) {
    let (i, r, quiet) = (c.0 as usize, c.2 as usize, c.3);
    let s = if quiet == Some(true) { r } else { i };
    let (blk, pos, syn, word) = (topo.blk[s], topo.pos[s], b.syn[s], b.data[r][0]);
    if quiet.is_none() {
        through(b, topo, t, edits, c.2, false);
        let word = read_place(b, topo, c.2).unwrap_or_else(|| value_place(b, c.2));
        edits.insert(blk, pos, Tag::IsUnique, [word, 0], Ty::VOID, syn);
        return;
    }
    let whole = |b: &mut Body, word: u32| match word & place::BARE_LOCAL {
        0 => prefix(b, word, word + 1 + b.extra[word as usize]),
        _ => word,
    };
    let (n, mode) = (u32_of(b.len()), ReadMode::Borrow as u32);
    // The re-read's own place, whose entries the new lookups replace,
    // rooted where its reads start.
    let word = match (quiet, read_place(b, topo, c.2)) {
        (Some(false), Some(w)) if b.view(word).value().is_some() => w,
        (Some(false), _) => whole(b, word),
        _ => word,
    };
    path_lists(b, t, word);
    for k in 0..t.elems.len() {
        let x = t.elems[k] as usize + 1;
        let p = prefix(b, word, t.elems[k]);
        edits.insert(blk, pos, Tag::IsUnique, [p, 0], Ty::VOID, syn);
        if let Some(e) = entry(b, b.extra[x]).filter(|_| quiet == Some(false)) {
            let [map, key] = [b.record(b.data[e][1])[0], b.record(b.data[e][1])[1]];
            let m = edits.insert(blk, pos, Tag::Read, [p, mode], b.ty[map as usize], syn);
            let rec = u32_of(b.extra.len());
            b.extra.extend([2, n + m, key]);
            let data = [IntrinsicOp::MapEntry as u32, rec];
            b.extra[x] = n + edits.insert(blk, pos, Tag::Intrinsic, data, b.ty[e], syn);
        }
    }
    let p = whole(b, word);
    edits.insert(blk, pos, Tag::IsUnique, [p, 0], Ty::VOID, syn);
    if quiet.is_some() {
        b.data[r][1] = mode;
    }
    if quiet == Some(false) {
        let e = edits.insert(blk, pos, Tag::Read, [word, mode], b.ty[r], syn);
        edits.rewrite(c.0, super::Loc::Extra(c.1), e);
        // The receiver's read, which only the change used and which checks
        // no index, has nothing left to do.
        let elem =
            |p: crate::ir::Place| p.proj.iter().any(|s| matches!(s, crate::ir::Proj::Elem(_)));
        if t.start[r + 1] - t.start[r] == 1
            && b.decode_place(b.data[r][0]).is_some_and(|p| !elem(p))
        {
            edits.remove(c.2);
        }
    }
}

/// Whether the place at `word` goes into memory: through an element or
/// a dereference.
fn in_memory(b: &Body, word: u32) -> bool {
    b.view(word).kinds(b) != (0, false)
}

/// The local the chain of reads ending at value `v` starts at, through
/// the reads of values; `Some(NONE)` for a top-level binding, or for a
/// chain from another value that goes into memory on the way (a list
/// behind a reference a call returned). `None` for a value's own parts.
fn read_root(b: &Body, topo: &Topology, mut v: u32) -> Option<u32> {
    let mut memory = false;
    for _ in 0..64 {
        let i = v as usize;
        if b.tags[i] != Tag::Read || topo.blk[i] == NONE {
            return memory.then_some(NONE);
        }
        let w = b.data[i][0];
        if let Some(l) = named_root(b, w) {
            return Some(l);
        }
        memory |= in_memory(b, w);
        v = b.view(w).value()?;
    }
    None
}

/// The place read `v` reads, rooted where its chain of reads starts
/// ([`read_root`]): `[*%0]` after `%0 = Read [$0]` is `[$0 *]`.
fn read_place(b: &mut Body, topo: &Topology, v: u32) -> Option<u32> {
    let i = v as usize;
    if b.tags[i] != Tag::Read || topo.blk[i] == NONE {
        return None;
    }
    let w = b.data[i][0];
    if named_root(b, w).is_some() {
        return Some(w);
    }
    let u = b.view(w).value()?;
    match read_place(b, topo, u) {
        Some(base) => Some(graft(b, base, w, w + 1 + b.extra[w as usize])),
        // A chain from another value names a place once it goes into
        // memory.
        None => read_root(b, topo, v).map(|_| w),
    }
}

/// A new place record: the place at `base` followed by the steps of the
/// place at `word`, which is rooted at a value, up to `extra` position
/// `end`.
fn graft(b: &mut Body, base: u32, word: u32, end: u32) -> u32 {
    let at = u32_of(b.extra.len());
    let from = word + 3;
    if base & place::BARE_LOCAL != 0 {
        b.extra
            .extend([2 + end - from, place::ROOT_LOCAL, base & !place::BARE_LOCAL]);
    } else {
        let n = b.extra[base as usize];
        b.extra.push(n + end - from);
        b.extra
            .extend_from_within(base as usize + 1..(base + 1 + n) as usize);
    }
    b.extra.extend_from_within(from as usize..end as usize);
    at
}

/// The `MapEntry` instruction an element step's index is, if any: the
/// step is a map value's.
fn entry(b: &Body, ix: u32) -> Option<usize> {
    let e = Ref(ix).as_inst()?.0 as usize;
    (e < b.len()
        && b.tags[e] == Tag::Intrinsic
        && IntrinsicOp::from_u32(b.data[e][0]) == Some(IntrinsicOp::MapEntry))
    .then_some(e)
}

/// The reads a change through value `v` goes through, from `v` back to
/// a read of a local, a global, a dereference or no read at all: for each,
/// an `IsUnique` (and a `Pin`, for a reference) of every list on its path,
/// outermost first, before the read. A coercion passes its value on, and
/// a read of a field of a value goes on to that value.
fn through(
    b: &mut Body,
    topo: &Topology,
    t: &mut Tables,
    edits: &mut splice::Tables,
    v: u32,
    pin: bool,
) {
    let mut cur = v;
    for _ in 0..64 {
        let Some(at) = Ref(cur).as_inst() else {
            return;
        };
        let i = at.0 as usize;
        match b.tags[i] {
            Tag::Coerce => cur = b.data[i][0],
            Tag::Read if topo.blk[i] != NONE => {
                let word = b.data[i][0];
                let deref = path_lists(b, t, word);
                let need = if pin { 3 } else { 1 };
                let done = t.walked[i];
                if done & need != need {
                    let (blk, pos, syn) = (topo.blk[i], topo.pos[i], b.syn[i]);
                    let base = b.view(word).value().and_then(|u| read_place(b, topo, u));
                    for k in 0..t.elems.len() {
                        let place = match base {
                            Some(w) => graft(b, w, word, t.elems[k]),
                            None => prefix(b, word, t.elems[k]),
                        };
                        if done & 1 == 0 {
                            edits.insert(blk, pos, Tag::IsUnique, [place, 0], Ty::VOID, syn);
                        }
                        if pin && done & 2 == 0 {
                            edits.insert(blk, pos, Tag::Pin, [place, NONE], Ty::VOID, syn);
                        }
                    }
                    t.walked[i] = done | need;
                }
                match b.view(word).value() {
                    Some(root) if !deref => cur = root,
                    _ => return,
                }
            }
            _ => return,
        }
    }
}

/// The `extra` positions of the element projections of the place at
/// `word`, outermost first, into `t.elems`. Whether the place has a
/// dereference.
fn path_lists(b: &Body, t: &mut Tables, word: u32) -> bool {
    t.elems.clear();
    let PlaceView {
        from: mut x, to, ..
    } = b.view(word);
    let mut deref = false;
    while x < to {
        match b.extra[x as usize] & place::PROJ_KIND {
            place::PROJ_ELEM => {
                t.elems.push(x);
                x += 2;
            }
            place::PROJ_DEREF => {
                deref = true;
                x += 1;
            }
            _ => x += 1,
        }
    }
    deref
}

/// A place of the root and the projections of the place at `word` before
/// the projection word at `end`: the list an element projection indexes.
fn prefix(b: &mut Body, word: u32, end: u32) -> u32 {
    let rec = word + 1;
    let p = b.view(word);
    if let Root::Local(l) = p.root
        && end == p.from
    {
        return l | place::BARE_LOCAL;
    }
    let at = u32_of(b.extra.len());
    b.extra.push(end - rec);
    b.extra.extend_from_within(rec as usize..end as usize);
    at
}
