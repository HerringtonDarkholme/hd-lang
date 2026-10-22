//! Insertion (`docs/compiler/tir-v2.md`, "Insertion"): the naive, correct
//! ownership of `crates/hd_wasm_linear/DESIGN.md` §23.16 with releases at
//! last use (§23.20), made explicit.
//!
//! 1. **Modes.** Every `use` read becomes `copy`.
//! 2. **Temporaries.** An owned value whose one use consumes it, as a
//!    statement of the value's own block (or as that block's tail, which
//!    the block's owner takes), is consumed there and gets no `Drop`. Any
//!    other consumer gets a `Read copy` of the value just before it, which
//!    it consumes instead, so a consumed value always has exactly one use;
//!    the value then gets a `Drop` after the statement of its **home**
//!    block that holds its last use. The home is the innermost block
//!    around the value that holds every use: its own block, or for a value
//!    a match decision makes and an arm reads, the block of the `Match`.
//! 3. **Locals.** An owned local gets a `Drop` after the statement of the
//!    innermost block holding all its accesses that holds the last one
//!    (`live::plan`'s rule). Parameters and captures live in their
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
//!    projection (each list on its path, outermost first), `ListPush`,
//!    `MapSet`, `MapRemove`, `MapIndexMut` and `ListPin`, of their
//!    collection operand. A read of an element that a change then goes
//!    through, which the checker marks `ListIndexMut` (`xs[i].push(v)`),
//!    becomes a `Read` of the element place, with an `IsUnique` of the list
//!    before it. The backend's intrinsics then check nothing themselves.
//! 6. **Pins.** A `Ref` of an element place gets an `IsUnique` and a `Pin`
//!    of each list on its path, outermost first. The reference holds a
//!    count of the buffer, so the pin ends with the reference's drop (rule
//!    2 or 3); nothing else releases it.
//!
//! Why it is sound: on every path an owned value is consumed by its one
//! use or dropped once, after its last use or before a jump that leaves
//! its home; a local is dropped after its last access, before a jump that
//! leaves its block, or at the exit, where a place already dropped holds
//! zeros and drops as nothing.

use hd_base::{LocalId, NodeIdx, SubId};
use hd_types::Ty;

use super::splice::for_blocks;
use super::{Loc, Operand, OwnCx, OwnScratch, Role, Tri, consumes, operands, splice, tracked};
use crate::ir::{Body, CaptureMode, IntrinsicOp, NONE, ReadMode, Ref, Tag, local_flags, place};

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
    /// Each instruction's block and position in it, or `NONE`.
    blk: Vec<u32>,
    pos: Vec<u32>,
    /// Each block's owning instruction, or `NONE`.
    owner: Vec<u32>,
    /// Each instruction's sub-body, and whether it is a sub-body's root.
    sub: Vec<u32>,
    is_root: Vec<bool>,
    tracked: Vec<bool>,
    /// The uses of each tracked value, in compressed rows by value.
    start: Vec<u32>,
    fill: Vec<u32>,
    uses: Vec<(u32, Operand)>,
    ops: Vec<Operand>,
    /// The chain of blocks around a place, innermost first, with the
    /// statement position in each; `mark` is a chain block's level plus 1.
    mark: Vec<u32>,
    chain: Vec<(u32, u32)>,
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
    /// Per instruction: an element read that a change goes through (rule
    /// 5), the checker's `ListIndexMut`.
    changed: Vec<bool>,
    /// The element projections of one place: the `extra` position of each
    /// element word.
    elems: Vec<u32>,
}

fn fill<T: Clone>(v: &mut Vec<T>, n: usize, x: T) {
    v.clear();
    v.resize(n, x);
}

impl Tables {
    fn reset(&mut self, n: usize, locals: usize) {
        fill(&mut self.blk, n, NONE);
        fill(&mut self.pos, n, 0);
        fill(&mut self.owner, n, NONE);
        fill(&mut self.sub, n, 0);
        fill(&mut self.is_root, n, false);
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
        fill(&mut self.changed, n, false);
        self.elems.clear();
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

    /// The statement that holds block `blk`: its owner's block and
    /// position, or `None` at a sub-body's root.
    fn up(&self, blk: u32) -> Option<(u32, u32)> {
        if self.is_root[blk as usize] {
            return None;
        }
        let o = self.owner[blk as usize];
        if o == NONE || self.blk[o as usize] == NONE {
            return None;
        }
        Some((self.blk[o as usize], self.pos[o as usize]))
    }

    /// Walks up from `(blk, pos)` to the first block of the marked chain:
    /// its level and the statement position there.
    fn meet(&self, mut blk: u32, mut pos: u32) -> Option<(u32, u32)> {
        loop {
            let m = self.mark[blk as usize];
            if m != 0 {
                return Some((m - 1, pos));
            }
            (blk, pos) = self.up(blk)?;
        }
    }

    /// Marks the chain of blocks from `(blk, pos)` up.
    fn mark_chain(&mut self, mut blk: u32, mut pos: u32) {
        self.chain.clear();
        loop {
            self.chain.push((blk, pos));
            self.mark[blk as usize] = u32::try_from(self.chain.len()).expect("depth");
            match self.up(blk) {
                Some(next) => (blk, pos) = next,
                None => break,
            }
        }
    }

    fn unmark_chain(&mut self) {
        for &(blk, _) in &self.chain {
            self.mark[blk as usize] = 0;
        }
    }

    /// Where use `(u, op)` stands: `u`'s block and position, or for a
    /// block's tail, the end of that block.
    fn at(&self, b: &Body, u: u32, op: &Operand) -> Option<(u32, u32)> {
        if op.role == Role::Tail {
            return Some((u, block_len(b, u)));
        }
        let blk = self.blk[u as usize];
        (blk != NONE).then(|| (blk, self.pos[u as usize]))
    }

    /// The owner a use's block tail goes to, for [`consumes`].
    fn tail_owner(&self, u: u32, op: &Operand) -> u32 {
        if op.role == Role::Tail && !self.is_root[u as usize] {
            self.owner[u as usize]
        } else {
            NONE
        }
    }
}

fn block_len(b: &Body, blk: u32) -> u32 {
    b.extra
        .get(b.data[blk as usize][0] as usize)
        .copied()
        .unwrap_or(0)
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// The root local of a place word, if it has one.
fn root_local(b: &Body, word: u32) -> Option<u32> {
    if word & place::BARE_LOCAL != 0 {
        return Some(word & !place::BARE_LOCAL);
    }
    match b.extra.get(word as usize + 1..word as usize + 3)? {
        [place::ROOT_LOCAL, l] => Some(*l),
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
    let t = &mut scratch.insert;
    let edits = &mut scratch.splice;
    edits.clear();
    t.reset(n, b.local_ty.len());
    changed_elements(b, t);
    structure(b, t);
    for i in 0..n {
        t.tracked[i] = tracked(b, cx, i);
    }
    collect_uses(b, t);
    values(b, cx, t, edits)?;
    locals(b, cx, t, edits);
    jumps(b, t, edits);
    uniqueness(b, t, edits);
    for i in 0..n {
        if b.tags[i] == Tag::Read && b.data[i][1] == ReadMode::Use as u32 {
            b.data[i][1] = ReadMode::Copy as u32;
        }
    }
    t.exits.sort_unstable_by_key(|(s, l)| (s.raw(), l.raw()));
    t.exits.dedup();
    b.exit_drops.clone_from(&t.exits);
    splice::apply(b, edits)
}

/// The block structure: each instruction's block and position, each
/// block's owner (a cleanup scope owns its suites, since it runs them, not
/// the `Defer` that registers one), and each instruction's sub-body.
fn structure(b: &Body, t: &mut Tables) {
    let n = b.len();
    for i in 0..n {
        if b.tags[i] == Tag::Block {
            for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
                if (m as usize) < n {
                    t.blk[m as usize] = u32_of(i);
                    t.pos[m as usize] = u32_of(p);
                }
            }
        }
    }
    for i in 0..n {
        let defer = b.tags[i] == Tag::Defer;
        let owner = &mut t.owner;
        for_blocks(b, i, |blk| {
            if !(defer && owner[blk as usize] != NONE) {
                owner[blk as usize] = u32_of(i);
            }
        });
    }
    for (s, &r) in b.sub_root.iter().enumerate() {
        if (r as usize) < n {
            t.is_root[r as usize] = true;
            t.sub[r as usize] = u32_of(s);
        }
    }
    // A member or an owned block comes before its block or owner.
    for i in (0..n).rev() {
        if t.is_root[i] {
            continue;
        }
        let parent = if t.blk[i] == NONE {
            t.owner[i]
        } else {
            t.blk[i]
        };
        if parent != NONE {
            t.sub[i] = t.sub[parent as usize];
        }
    }
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
    cx: &dyn OwnCx,
    t: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    for v in 0..b.len() {
        if !t.tracked[v] || t.blk[v] == NONE {
            continue;
        }
        let (lo, hi) = (t.start[v] as usize, t.start[v + 1] as usize);
        t.mark_chain(t.blk[v], t.pos[v]);
        t.levels.clear();
        let mut home = 0;
        let mut stray = None;
        for k in lo..hi {
            let (u, op) = t.uses[k];
            match t.at(b, u, &op).and_then(|(blk, pos)| t.meet(blk, pos)) {
                Some(met) => {
                    home = home.max(met.0);
                    t.levels.push(met);
                }
                None => stray = Some(u),
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
                t.blk[u as usize] == own
            };
            if direct && consumes(b, cx, u as usize, &op, t.tail_owner(u, &op)) {
                let last = match op.role {
                    Role::Tail => block_len(b, own).saturating_sub(1),
                    _ => t.pos[u as usize].saturating_sub(1),
                };
                t.items.push(Item {
                    blk: own,
                    what: u32_of(v),
                    first: t.pos[v],
                    last,
                });
                continue;
            }
        }
        for k in lo..hi {
            let (u, op) = t.uses[k];
            if !consumes(b, cx, u as usize, &op, t.tail_owner(u, &op)) {
                continue;
            }
            let (blk, pos) = match op.role {
                Role::Tail => (u, block_len(b, u)),
                _ => (t.blk[u as usize], t.pos[u as usize]),
            };
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
        edits.insert(h, at, Tag::Drop, [word, NONE], Ty::VOID, syn);
        t.items.push(Item {
            blk: h,
            what: u32_of(v),
            first,
            last,
        });
    }
    Ok(())
}

/// Rule 3: each owned local's `Drop` after its last access, and its exit
/// row.
fn locals(b: &Body, cx: &dyn OwnCx, t: &mut Tables, edits: &mut splice::Tables) {
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
                if let Some(l) = root_local(b, a) {
                    t.acc.push((t.sub[i], l, u32_of(i)));
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
                    t.acc.push((t.sub[i], l, u32_of(i)));
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
                place_local(b, t, edits, s, l, k..end, entry);
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
    t: &mut Tables,
    edits: &mut splice::Tables,
    s: u32,
    l: u32,
    range: std::ops::Range<usize>,
    entry: bool,
) {
    let first = t.acc[range.start].2 as usize;
    if t.blk[first] == NONE {
        return;
    }
    t.mark_chain(t.blk[first], t.pos[first]);
    t.levels.clear();
    let mut common = 0;
    for k in range {
        let x = t.acc[k].2 as usize;
        if t.blk[x] == NONE {
            continue;
        }
        if let Some(met) = t.meet(t.blk[x], t.pos[x]) {
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
    edits.insert(
        d,
        (last + 1).min(block_len(b, d)),
        Tag::Drop,
        [l | place::BARE_LOCAL, NONE],
        Ty::VOID,
        statement_syn(b, d, last),
    );
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
fn jumps(b: &mut Body, t: &mut Tables, edits: &mut splice::Tables) {
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
        if !matches!(tag, Tag::Break | Tag::Continue | Tag::Return | Tag::ToArm) || t.blk[j] == NONE
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
        let (jb, jp) = (t.blk[j], t.pos[j]);
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
            let owner = t.owner[blk as usize];
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
            match t.up(blk) {
                Some(next) => (blk, ps) = next,
                None => break,
            }
        }
    }
}

/// Stamps every local and value the suites of cleanup scope `scope` read.
fn mark_suites(b: &Body, t: &mut Tables, scope: u32, stamp: u32) {
    t.stack.clear();
    t.stack
        .extend_from_slice(b.record(b.data[scope as usize][1]));
    let mut ops = std::mem::take(&mut t.ops);
    while let Some(x) = t.stack.pop() {
        let i = x as usize;
        if i >= b.len() {
            continue;
        }
        if b.tags[i] == Tag::Block {
            t.stack.extend_from_slice(b.record(b.data[i][0]));
        }
        let stack = &mut t.stack;
        for_blocks(b, i, |blk| stack.push(blk));
        match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref => {
                if let Some(l) = root_local(b, b.data[i][0]) {
                    t.local_stamp[l as usize] = stamp;
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0] as usize..(rec[0] + rec[1]) as usize {
                    t.local_stamp[b.cap_local[c].idx()] = stamp;
                }
            }
            _ => {}
        }
        operands(b, i, &mut ops);
        for op in &ops {
            if Ref(op.value).as_inst().is_some() {
                t.value_stamp[op.value as usize] = stamp;
            }
        }
    }
    t.ops = ops;
}

/// Rule 5's marked reads: each `ListIndexMut [%xs %i]` becomes the element
/// read it stands for, `Read [%xs [%i]]`, marked as one a change goes
/// through. The checker writes the intrinsic because only it sees every
/// change a read leads to (a `mut self` receiver is a plain argument).
fn changed_elements(b: &mut Body, t: &mut Tables) {
    for i in 0..b.len() {
        let [op, rec] = b.data[i];
        if b.tags[i] != Tag::Intrinsic
            || IntrinsicOp::from_u32(op) != Some(IntrinsicOp::ListIndexMut)
        {
            continue;
        }
        let &[list, index] = b.record(rec) else {
            continue;
        };
        let at = u32_of(b.extra.len());
        b.extra
            .extend([4, place::ROOT_VALUE, list, place::PROJ_ELEM, index]);
        b.tags[i] = Tag::Read;
        b.data[i] = [at, ReadMode::Use as u32];
        t.changed[i] = true;
    }
}

/// Rules 5 and 6: an `IsUnique` before each in-place change of a list or
/// map place, and a `Pin` after it before each element reference.
fn uniqueness(b: &mut Body, t: &mut Tables, edits: &mut splice::Tables) {
    for i in 0..b.len() {
        if t.blk[i] == NONE {
            continue;
        }
        let [a, w] = b.data[i];
        let (blk, pos, syn) = (t.blk[i], t.pos[i], b.syn[i]);
        match b.tags[i] {
            Tag::Store | Tag::Ref => {
                let pin = b.tags[i] == Tag::Ref;
                path_lists(b, t, a);
                for k in 0..t.elems.len() {
                    let word = prefix(b, a, t.elems[k]);
                    edits.insert(blk, pos, Tag::IsUnique, [word, 0], Ty::VOID, syn);
                    if pin {
                        edits.insert(blk, pos, Tag::Pin, [word, NONE], Ty::VOID, syn);
                    }
                }
            }
            Tag::Read if t.changed[i] => {
                path_lists(b, t, a);
                if let Some(&last) = t.elems.last() {
                    let word = prefix(b, a, last);
                    edits.insert(blk, pos, Tag::IsUnique, [word, 0], Ty::VOID, syn);
                }
            }
            Tag::Intrinsic
                if matches!(
                    IntrinsicOp::from_u32(a),
                    Some(
                        IntrinsicOp::ListPush
                            | IntrinsicOp::MapSet
                            | IntrinsicOp::MapRemove
                            | IntrinsicOp::MapIndexMut
                            | IntrinsicOp::ListPin
                    )
                ) =>
            {
                let Some(&coll) = b.record(w).first() else {
                    continue;
                };
                if Ref(coll).as_inst().is_none() {
                    continue;
                }
                let word = value_place(b, coll);
                edits.insert(blk, pos, Tag::IsUnique, [word, 0], Ty::VOID, syn);
            }
            _ => {}
        }
    }
}

/// The `extra` positions of the element projections of the place at
/// `word`, outermost first, into `t.elems`.
fn path_lists(b: &Body, t: &mut Tables, word: u32) {
    t.elems.clear();
    if word & place::BARE_LOCAL != 0 {
        return;
    }
    let elems = &mut t.elems;
    super::place_words(b, word, |_, at| {
        if let Loc::Extra(x) = at
            && b.extra[x as usize - 1] == place::PROJ_ELEM
        {
            elems.push(x - 1);
        }
    });
}

/// A place of the root and the projections of the place at `word` before
/// the projection word at `end`: the list an element projection indexes.
fn prefix(b: &mut Body, word: u32, end: u32) -> u32 {
    let rec = word + 1;
    let roots = match b.extra[rec as usize] {
        place::ROOT_GLOBAL => 3,
        _ => 2,
    };
    if b.extra[rec as usize] == place::ROOT_LOCAL && end == rec + roots {
        return b.extra[rec as usize + 1] | place::BARE_LOCAL;
    }
    let at = u32_of(b.extra.len());
    b.extra.push(end - rec);
    b.extra.extend_from_within(rec as usize..end as usize);
    at
}
