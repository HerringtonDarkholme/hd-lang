//! Frame locals (`crates/hd_wasm_linear/DESIGN.md` §23.38): locals that
//! local references point into, which the backend may keep in a slot of
//! the function's frame rather than a counted cell. A slot holds no count,
//! so every use of a value that may point into the local counts as a use
//! of it, and insertion drops it after the last one. Such uses stay in
//! the local's scope (`spec/lang/13-values-and-references.md#local-scope`).
//!
//! 1. **Candidates.** A local of the root sub-body, a parameter too but
//!    not a capture, every `Ref` of whose storage is local
//!    (`ref_word::LOCAL`) there, in a root with no suspension point; the
//!    first 64. A parameter's value moves into its slot at entry.
//! 2. **Origins.** Per value and local, the candidates it may point into,
//!    as the checker solves them (`hd_check/src/dep_results.rs`) but over
//!    this body, where the inliner binds parameters to locals: a `Ref`
//!    points where its place lies, a read of a reference holds what its
//!    place holds, a `Store` adds to what its place's locals hold (an
//!    element's is whatever holds its list, as for a `ListPush` or
//!    `MapSet`), a
//!    `Break` to its target, and any other value that may hold a reference
//!    ([`OwnCx::holds_ref`]) takes its operands' and its blocks'. A read
//!    through a reference may become a borrow, which lives on what it
//!    reads, so its uses keep those locals alive too (`dep`).
//! 3. **Escapes.** A candidate keeps its cell when a value with its origin
//!    is stored where no local of the body holds it, returned (a default
//!    argument's body returns its value), awaited,
//!    captured, read by a cleanup suite (which runs after a jump dropped
//!    what it leaves), or consumed by an instruction other than a call
//!    whose value holds no reference.
//!
//! Their `Ref`s get `ref_word::FRAME`, and move never takes their count.
//!
//! 4. **Cell locals.** In every sub-body, a local that keeps its cell (a closure captures it,
//!    or a candidate escapes) is tracked the same way, after the frame
//!    candidates: the uses of what points into it are its accesses too, so
//!    its drop follows them, as a frame local's does. A reference into
//!    what a cell holds (an element of a list in it) takes no count of the
//!    cell, so the local's own count keeps the cell alive until then.
//! 5. **Roots.** A counted local a place dereferences (`[*$p .items]`) or
//!    takes an element of (`[$xs [%i]]`), or whose whole value a coercion
//!    lends as a local reference (`ref.convert.lend`, [`OwnCx::lends`]),
//!    is tracked the same way
//!    (`ref.box.root`), in a body with no frame candidate too: what a
//!    reference or a borrowed read through it points into holds no count
//!    of its object or buffer, so the uses of what may point there are the
//!    local's uses too, and its drop or a move of it waits for them (the
//!    move pass reads them through [`roots`]). A value such a coercion
//!    lends stays undropped until the reference's last use (insertion,
//!    rule 2).
//! 6. **Frame cells** ([`frame_cells`]). A local a closure of the root
//!    shares, whose closure never leaves the frame, could live in a frame
//!    slot: the closure value points into it, and rule 3's escapes apply,
//!    with a call that may keep its argument, a compiler-supplied call,
//!    another closure's capture, a slot of a collection, and a local
//!    declared outside the loop round that declares the cell (each round
//!    reuses the slot) added. Its captures become
//!    `CaptureMode::Frame`, and every use of what may point into it is
//!    its use, so insertion drops it after the closure's last use.

use hd_base::SliceSort as _;

use super::{OwnCx, Role, fill, operands, suite_touches};
use crate::ir::{
    Body, BodyKind, Callee, IntrinsicOp, NONE, PlaceView, Proj, Ref, Root, Tag, Topology, Word,
    is_suspension, local_flags, slots, u32_of,
};

/// Frame locals' dense tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    /// Per local: its candidate bit, or 0.
    bit: Vec<u64>,
    /// Per local: whether some `Ref` of its storage rules it out.
    bad: Vec<bool>,
    /// The candidates, by bit.
    cands: Vec<u32>,
    /// Per value and per local: the candidates it may point into.
    val: Vec<u64>,
    held: Vec<u64>,
    /// Per value and per local: the candidates a value read through a
    /// reference lives on, as a borrow of it does (a superset of `val`'s
    /// and `held`'s uses: these only extend a local's life).
    dep: Vec<u64>,
    ldep: Vec<u64>,
    /// The candidates a use lets escape.
    out: u64,
    /// The candidates the last run found frame locals.
    frame: u64,
    /// Projected places for a field-liveness query; empty for frame planning.
    targets: Vec<(u32, u32)>,
    ops: Vec<super::Operand>,
    /// Roots (rule 5): a value whose references are all counted, or a
    /// call's result of a counted type that holds no reference, keeps its
    /// own count of what it points into (or the borrow pass lends it only
    /// from a place that outlives it), so it carries no origin. A scalar
    /// read through the root still does: an address may become a `Ptr`
    /// and then a reference.
    roots: bool,
    /// Frame cells ([`frame_cells`]): a closure points into the locals it
    /// shares, and a call that may keep an argument, an instruction the
    /// compiler supplies, or another closure's capture lets it escape.
    cells: bool,
    /// Frame cells only, per local: the first instruction of the innermost
    /// loop of the sub-body around its first store, and the loop's, or `NONE`.
    /// Each round reuses a slot, so a closure over it must not be stored in
    /// a local that outlives the round.
    round: Vec<(u32, u32)>,
    /// Frame cells only, per local: its first store, or `NONE` for a
    /// parameter, which holds a value from before any loop.
    decl: Vec<u32>,
}

impl Tables {
    /// Whether the last [`run`] found local `l` a frame local.
    pub(crate) fn is_frame(&self, l: usize) -> bool {
        self.bit.get(l).is_some_and(|&m| m & self.frame != 0)
    }

    /// The candidates the value word `v` may point into.
    fn of(&self, v: u32) -> u64 {
        if v == NONE || Ref(v).as_inst().is_none() {
            return 0;
        }
        self.val.get(v as usize).copied().unwrap_or(0)
    }

    /// The candidates the place at `word` lies in and those the value there
    /// may point into; `None` for a place no local of the body holds (a
    /// parameter's referent, a top-level binding).
    fn place(&self, b: &Body, word: u32) -> Option<(u64, u64)> {
        let p = b.view(word);
        let (mut lies, mut held) = match p.root {
            Root::Local(l) => (self.bit[l as usize], self.held[l as usize]),
            // A read of all of a local (`%h = Read [$xs]`) is that local's
            // storage, a borrow's, or a copy sharing its buffer.
            Root::Value(v) => match whole_local_read(b, v) {
                Some(w) => (self.place(b, w).map_or(0, |(lies, _)| lies), self.of(v)),
                None => (0, self.of(v)),
            },
            _ => return None,
        };
        if let Root::Local(l) = p.root
            && !self.targets.is_empty()
        {
            // A direct field reference lends just that field. A reference
            // derived through another reference keeps its parent's origins.
            let prefix: Vec<_> = p
                .steps(b)
                .take_while(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
                .collect();
            let mut mask = 0;
            for (k, &(local, word)) in self.targets.iter().enumerate() {
                if local == l
                    && prefix
                        .iter()
                        .copied()
                        .zip(b.view(word).steps(b))
                        .all(|(a, c)| a == c)
                {
                    mask |= 1 << k;
                }
            }
            lies &= mask;
        }
        for s in p.steps(b) {
            match s {
                // Through a reference: where it points, and what is there.
                Proj::Deref if held != 0 => (lies, held) = (held, self.holdings(held)),
                Proj::Deref | Proj::Elem(_) if !self.targets.is_empty() => {
                    return Some((lies, held));
                }
                // An element lies in the buffer, and what it holds is what
                // the list holds: its local's reference or closure.
                Proj::Deref | Proj::Elem(_) => lies = 0,
                _ => {}
            }
        }
        Some((lies, held))
    }

    /// The locals whose values hold the slot at `word`, as a store into it
    /// reaches them: the root local when no reference lies on the way, and
    /// the candidates a reference on the way points into. An element is
    /// held by whatever holds its list. A frame cell's closure in an element
    /// has no such holder: it escapes ([`Self::stored`]).
    fn slot_holders(&self, b: &Body, word: u32) -> (Option<u32>, u64) {
        let p = b.view(word);
        if self.cells && p.kinds(b).1 {
            return (None, 0);
        }
        let (mut direct, mut lies, mut held) = match p.root {
            Root::Local(l) => (Some(l), self.bit[l as usize], self.held[l as usize]),
            Root::Value(v) => (
                whole_local_read(b, v).and_then(|w| b.view(w).local()),
                whole_local_read(b, v)
                    .and_then(|w| self.place(b, w))
                    .map_or(0, |(lies, _)| lies),
                self.of(v),
            ),
            _ => return (None, 0),
        };
        for s in p.steps(b) {
            if s == Proj::Deref {
                direct = None;
                (lies, held) = (held, self.holdings(held));
            }
        }
        (direct, lies)
    }

    /// A slot of the collection value `v` (`ListPush`, `MapSet`): who holds
    /// it, as [`Self::slot_holders`].
    fn collection_holders(&self, b: &Body, v: u32) -> (Option<u32>, u64) {
        match Ref(v).as_inst() {
            Some(x) if b.tags[x.0 as usize] == Tag::Read => {
                self.slot_holders(b, b.data[x.0 as usize][0])
            }
            _ => (None, 0),
        }
    }

    /// A slot `holders` hold now holds what `m` points into: if no local
    /// of the body holds it, that escapes. Whether any set grew.
    fn stored(&mut self, (direct, holders): (Option<u32>, u64), m: u64) -> bool {
        let mut changed = false;
        if let Some(l) = direct {
            changed |= self.hold(l, m);
        }
        for k in bits(holders) {
            changed |= self.hold(self.cands[k], m);
        }
        if direct.is_none() && holders == 0 {
            changed |= grow(&mut self.out, m);
        }
        changed
    }

    /// Local `l` holds what `m` points into. What it outlives the round of
    /// escapes (a local reference cannot: `ref.dep.scope.loop`).
    fn hold(&mut self, l: u32, m: u64) -> bool {
        let mut out = 0;
        if !self.round.is_empty() {
            for k in bits(m) {
                let (first, end) = self.round[self.cands[k] as usize];
                if first != NONE && !(first..=end).contains(&self.decl[l as usize]) {
                    out |= 1 << k;
                }
            }
        }
        grow(&mut self.held[l as usize], m) | grow(&mut self.out, out)
    }

    /// What the candidates `m` hold.
    fn holdings(&self, m: u64) -> u64 {
        let mut out = 0;
        for k in bits(m) {
            out |= self.held[self.cands[k] as usize];
        }
        out
    }
}

/// The set bits of `m`.
fn bits(mut m: u64) -> impl Iterator<Item = usize> {
    std::iter::from_fn(move || {
        (m != 0).then(|| {
            let k = m.trailing_zeros() as usize;
            m &= m - 1;
            k
        })
    })
}

/// Finds the body's frame locals, sets `ref_word::FRAME` on their `Ref`s,
/// and calls `uses` with each instruction that uses a tracked local (a
/// frame local or a cell local, rule 4) through a value or a local that
/// may point into it: (instruction, local, whether it is a frame local).
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    mut uses: impl FnMut(u32, u32, bool),
) {
    let (n, locals) = (b.len(), b.local_ty.len());
    fill(&mut t.bit, locals, 0);
    fill(&mut t.bad, locals, false);
    t.cands.clear();
    t.targets.clear();
    t.out = 0;
    t.frame = 0;
    // A suspending root keeps its locals in its frame object, so none is
    // a frame slot, but a reference into one of its cells, or one its
    // synchronous closures lend, holds no count of the cell.
    if runs_framed(b, topo, cx) {
        extra_cell_uses(b, cx, t, |_| true, &mut uses);
        root_uses(b, cx, &mut uses);
        return;
    }
    cell_uses(b, topo, cx, &mut uses);
    for c in &b.cap_local {
        t.bad[c.idx()] = true;
    }
    for i in 0..n {
        if b.tags[i] != Tag::Ref {
            continue;
        }
        let Some(l) = b.local_in_place(b.data[i][0]).map(hd_base::LocalId::idx) else {
            continue;
        };
        if b.data[i][1] & crate::ir::ref_word::LOCAL == 0 || topo.sub[i] != 0 {
            t.bad[l] = true;
        }
    }
    // The frame candidates take the first bits, then cell locals from
    // every sub-body. Only the root may use physical frame slots.
    for cells in [false, true] {
        for i in 0..n {
            if b.tags[i] != Tag::Ref || (!cells && topo.sub[i] != 0) {
                continue;
            }
            let Some(l) = b.local_in_place(b.data[i][0]).map(hd_base::LocalId::idx) else {
                continue;
            };
            if t.bad[l] == cells && t.bit[l] == 0 && t.cands.len() < 64 {
                t.bit[l] = 1 << t.cands.len();
                t.cands.push(u32_of(l));
            }
        }
    }
    for k in 0..t.cands.len() {
        if t.bad[t.cands[k] as usize] {
            t.out |= 1 << k;
        }
    }
    if t.cands.is_empty() {
        root_uses(b, cx, &mut uses);
        return;
    }
    fill(&mut t.val, n, 0);
    fill(&mut t.held, locals, 0);
    fill(&mut t.dep, n, 0);
    fill(&mut t.ldep, locals, 0);
    let mut ops = std::mem::take(&mut t.ops);
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            changed |= step(b, cx, t, &mut ops, i);
        }
    }
    escape_exits(b, true, t, &mut ops);
    t.ops = ops;
    let tracked = u64::MAX >> (64 - t.cands.len());
    let frame = !t.out & tracked;
    t.frame = frame;
    for i in 0..n {
        let [a, w] = b.data[i];
        let mut m = 0;
        if matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique)
            && let Some(l) = b.view(a).local()
        {
            m |= t.held[l as usize] | t.ldep[l as usize];
        }
        operands(b, i, &mut t.ops);
        for op in &t.ops {
            m |= t.of(op.value) | t.dep.get(op.value as usize).copied().unwrap_or(0);
        }
        for k in bits(m & tracked) {
            uses(u32_of(i), t.cands[k], frame & (1 << k) != 0);
        }
        if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(a)
            && frame & t.bit[l.idx()] != 0
        {
            b.data[i][1] = w | crate::ir::ref_word::FRAME;
        }
    }
    extra_cell_uses(b, cx, t, |_| true, &mut uses);
    root_uses(b, cx, &mut uses);
}

/// Whether the root of `b` runs over a frame object: it has a suspension
/// point, or its item suspends and it isn't an entry root, which would
/// end on its first poll.
pub(crate) fn runs_framed(b: &Body, topo: &Topology, cx: &dyn OwnCx) -> bool {
    let point = (0..b.len()).any(|i| is_suspension(b.tags[i]) && topo.sub[i] == 0);
    point || b.kind != BodyKind::Default && cx.suspending(b.item) && !cx.entry_root(b.item)
}

/// Each local's first store and the loop round around it ([`Tables::round`]).
fn rounds(b: &Body, topo: &Topology, t: &mut Tables) {
    let locals = b.local_ty.len();
    fill(&mut t.round, locals, (NONE, NONE));
    fill(&mut t.decl, locals, NONE);
    for i in (0..b.len()).rev() {
        if b.tags[i] != Tag::Store {
            continue;
        }
        let Some(l) = b.view(b.data[i][0]).local().map(|l| l as usize) else {
            continue;
        };
        if b.local_flags[l] & local_flags::PARAM != 0 {
            continue;
        }
        t.decl[l] = u32_of(i);
        let lp = topo.loop_of[i];
        t.round[l] = if lp != NONE && topo.sub[lp as usize] == topo.sub[i] {
            (topo.first[lp as usize], lp)
        } else {
            (NONE, NONE)
        };
    }
}

/// Frame cells: plans each local of the root a closure shares (a
/// `Shared` capture) that may live in a frame slot into `t`, and returns
/// their bits. The closure value points into the cell, and the cell
/// escapes as a frame candidate's value does (rule 3), and also when a
/// call that may keep an argument ([`super::keeps_arg`]) is passed it, an
/// `Intrinsic` or host call reads it, or another closure captures it. A
/// local some closure copies or moves, one a closure outside the root
/// shares, or one with a `Ref` that rule 1 rules out, is none.
fn plan_cells(b: &Body, topo: &Topology, cx: &dyn OwnCx, t: &mut Tables) -> u64 {
    let (n, locals) = (b.len(), b.local_ty.len());
    t.cells = true;
    t.cands.clear();
    t.targets.clear();
    t.out = 0;
    t.frame = 0;
    fill(&mut t.bit, locals, 0);
    fill(&mut t.bad, locals, false);
    rounds(b, topo, t);
    for i in 0..n {
        match b.tags[i] {
            Tag::Ref => {
                if let Some(l) = b.local_in_place(b.data[i][0])
                    && (b.data[i][1] & crate::ir::ref_word::LOCAL == 0 || topo.sub[i] != 0)
                {
                    t.bad[l.idx()] = true;
                }
            }
            Tag::Closure => {
                for (l, m) in closure_caps(b, i) {
                    if topo.sub[i] != 0 || !m.shares() {
                        t.bad[l.idx()] = true;
                    }
                }
            }
            _ => {}
        }
    }
    for i in (0..n).filter(|&i| b.tags[i] == Tag::Closure && topo.sub[i] == 0) {
        for (l, _) in closure_caps(b, i) {
            if !t.bad[l.idx()] && t.bit[l.idx()] == 0 && t.cands.len() < 64 {
                t.bit[l.idx()] = 1 << t.cands.len();
                t.cands.push(l.raw());
            }
        }
    }
    if t.cands.is_empty() {
        return 0;
    }
    fill(&mut t.val, n, 0);
    fill(&mut t.held, locals, 0);
    fill(&mut t.dep, n, 0);
    fill(&mut t.ldep, locals, 0);
    let mut ops = std::mem::take(&mut t.ops);
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            changed |= step(b, cx, t, &mut ops, i);
        }
    }
    escape_exits(b, false, t, &mut ops);
    t.ops = ops;
    t.frame = !t.out & (u64::MAX >> (64 - t.cands.len()));
    t.frame
}

/// Rule 6 in a body with no suspension point in its root: each frame
/// cell's captures become `CaptureMode::Frame`, and `uses` gets each
/// instruction that uses a value or a local that may point into one, as
/// (instruction, local, true), so its drop follows the closure's last use.
fn cell_uses(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, uses: &mut impl FnMut(u32, u32, bool)) {
    if !b.cap_mode.iter().any(|m| m.shares()) {
        return;
    }
    let mut t = Tables::default();
    let frame = plan_cells(b, topo, cx, &mut t);
    if frame == 0 {
        return;
    }
    for i in 0..b.len() {
        let mut m = 0;
        if matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique)
            && let Some(l) = b.view(b.data[i][0]).local()
        {
            m |= t.held[l as usize] | t.ldep[l as usize];
        }
        operands(b, i, &mut t.ops);
        for op in &t.ops {
            m |= t.of(op.value) | t.dep.get(op.value as usize).copied().unwrap_or(0);
        }
        for k in bits(m & frame) {
            uses(u32_of(i), t.cands[k], true);
        }
        if b.tags[i] == Tag::Closure {
            let rec = b.record(b.data[i][1]);
            let (start, len) = (rec[0] as usize, rec[1] as usize);
            for c in start..start + len {
                if b.cap_mode[c] == crate::ir::CaptureMode::Shared
                    && t.bit[b.cap_local[c].idx()] & frame != 0
                {
                    b.cap_mode[c] = crate::ir::CaptureMode::Frame;
                }
            }
        }
    }
}

/// Closure `i`'s captures: each local and how it is captured.
fn closure_caps(
    b: &Body,
    i: usize,
) -> impl Iterator<Item = (&hd_base::LocalId, &crate::ir::CaptureMode)> {
    let rec = b.record(b.data[i][1]);
    let (start, len) = (rec[0] as usize, rec[1] as usize);
    b.cap_local[start..start + len]
        .iter()
        .zip(&b.cap_mode[start..start + len])
}

/// Whether `Store` `i`, after insertion, writes a whole local `l` that
/// holds no value on any path to it, so the store releases nothing: `i`
/// is `l`'s only whole store, and either no loop of its sub-body runs it
/// again, or every path from it back to the loop drops `l`. That holds
/// when a block around `i`, inside the loop's body, drops `l` after the
/// statement holding `i`, and each `Break` or `Continue` under that block
/// that leaves it either comes right after drops that include `l`'s or
/// is a `Break` that lands outside every loop. A `Return` ends the body,
/// whose exit set drops `l`; a `ToArm` counts as leaving.
#[must_use]
pub fn store_finds_empty(b: &Body, topo: &Topology, i: usize) -> bool {
    let whole_local = |x: usize, tag: Tag| -> Option<u32> {
        (b.tags[x] == tag)
            .then(|| b.view(b.data[x][0]))
            .filter(|p| p.whole())
            .and_then(PlaceView::local)
    };
    let Some(l) = whole_local(i, Tag::Store) else {
        return false;
    };
    if (0..b.len()).any(|x| x != i && whole_local(x, Tag::Store) == Some(l)) {
        return false;
    }
    let lp = topo.loop_of[i];
    if lp == NONE || topo.sub[lp as usize] != topo.sub[i] {
        return true;
    }
    // The block that drops `l` after the statement around `i`, inside
    // the loop's body.
    let mut chain = Vec::new();
    super::values::statement_chain(topo, u32_of(i), &mut chain);
    let dropped_in = chain
        .iter()
        .take_while(|&&(_, _, x)| x != lp)
        .find(|&&(blk, pos, _)| {
            b.record(b.data[blk as usize][0])[pos as usize + 1..]
                .iter()
                .any(|&x| whole_local(x as usize, Tag::Drop) == Some(l))
        });
    let Some(&(h, _, _)) = dropped_in else {
        return false;
    };
    let mut chain = Vec::new();
    let mut under_h = |x: u32| {
        super::values::statement_chain(topo, x, &mut chain);
        chain.iter().any(|&(blk, _, _)| blk == h)
    };
    for j in 0..b.len() {
        let target = match b.tags[j] {
            Tag::Break | Tag::Continue => b
                .label_inst
                .get(b.data[j][0] as usize)
                .copied()
                .unwrap_or(NONE),
            Tag::ToArm => NONE,
            _ => continue,
        };
        if topo.sub[j] != topo.sub[i] || !under_h(u32_of(j)) {
            continue;
        }
        if target != NONE && target != h && under_h(target) {
            continue;
        }
        // A `Break` that lands outside every loop never comes back.
        let outer = topo.loop_of.get(target as usize).copied().unwrap_or(0);
        if b.tags[j] == Tag::Break
            && target != NONE
            && (outer == NONE || topo.sub[outer as usize] != topo.sub[i])
        {
            continue;
        }
        let jl = b.record(b.data[topo.blk[j] as usize][0]);
        let mut drops = jl[..topo.pos[j] as usize]
            .iter()
            .rev()
            .take_while(|&&x| b.tags[x as usize] == Tag::Drop);
        if !drops.any(|&x| whole_local(x as usize, Tag::Drop) == Some(l)) {
            return false;
        }
    }
    true
}

/// The place word of value `v` when it is a read of all of a local
/// (`%h = Read [$xs]`), which a place rooted at `%h` goes through.
fn whole_local_read(b: &Body, v: u32) -> Option<u32> {
    let x = Ref(v).as_inst()?.0 as usize;
    let w = b.data[x][0];
    (b.tags[x] == Tag::Read && b.view(w).whole() && b.view(w).local().is_some()).then_some(w)
}

/// Rules 4 and 5 alone: `uses` gets each use of a cell local (one a local
/// `Ref` that is no frame slot's addresses) or a root local through what may
/// point into it, as (instruction, local). The move pass reads them, so
/// neither is moved, nor its cell taken, before a reference's last use.
pub(crate) fn roots(b: &Body, cx: &dyn OwnCx, mut uses: impl FnMut(u32, u32)) {
    let t = Tables::default();
    let mut each = |i, l, _| uses(i, l);
    extra_cell_uses(
        b,
        cx,
        &t,
        |i| {
            let w = b.data[i][1];
            w & crate::ir::ref_word::LOCAL != 0 && w & crate::ir::ref_word::FRAME == 0
        },
        &mut each,
    );
    root_uses(b, cx, &mut each);
}

/// Rule 5, roots (`ref.box.root`): a counted local whose value a place
/// dereferences (`[*$p .items]`, a `*T`) or takes an element of
/// (`[$xs [%i]]`, a list or a map) is the root of
/// what a reference or a borrowed read through it points into, which
/// holds no count of that object. Each use of such a value is a use of
/// the root, so its drop, or a move of it, follows the reference's last
/// use, in a suspending body's frame too.
fn root_uses(b: &Body, cx: &dyn OwnCx, uses: &mut impl FnMut(u32, u32, bool)) {
    let mut locals = Vec::new();
    let mut lent_reads: Vec<u32> = Vec::new();
    let mut lent = Vec::new();
    for c in 0..b.len() {
        super::lent_values(b, cx, c, &mut lent);
        lent_reads.extend_from_slice(&lent);
    }
    lent_reads.sort_stable();
    lent_reads.dedup();
    for i in 0..b.len() {
        if !matches!(b.tags[i], Tag::Ref | Tag::Read) {
            continue;
        }
        let p = b.view(b.data[i][0]);
        // A read of all of a counted local that a coercion lends as a
        // local reference roots the reference there too.
        let lent = b.tags[i] == Tag::Read
            && p.whole()
            && (lent_reads.binary_search(&u32_of(i)).is_ok() || super::lending_read(b, cx, i));
        let root = match p.root {
            Root::Value(v) => whole_local_read(b, v).map_or(p.root, |w| b.view(w).root),
            r => r,
        };
        if let Root::Local(l) = root
            && (lent || p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_))))
            && cx.counted(b.local_ty[l as usize])
            && !b.cap_local.iter().any(|c| c.raw() == l)
        {
            locals.push(l);
        }
    }
    locals.sort_stable();
    locals.dedup();
    let mut extra = Tables {
        roots: true,
        ..Tables::default()
    };
    for batch in locals.chunks(64) {
        let targets: Vec<_> = batch
            .iter()
            .map(|&l| (l, l | crate::ir::place::BARE_LOCAL))
            .collect();
        part_uses(b, cx, &mut extra, &targets, |i, k| uses(i, batch[k], false));
    }
}

/// What a coercion `i` that lends a counted reference as a local one
/// (`ref.convert.lend`) points into: where the counted reference it reads
/// lies, as a `Ref` through it would (`[*$p]`), so the local holding the
/// counted reference lives until the local one's last use.
fn lent_from(b: &Body, cx: &dyn OwnCx, t: &Tables, i: usize) -> u64 {
    let mut lent = Vec::new();
    super::lent_values(b, cx, i, &mut lent);
    lent.iter()
        .filter(|&&v| b.tags[v as usize] == Tag::Read)
        .fold(0, |m, &v| {
            m | t
                .place(b, b.data[v as usize][0])
                .map_or(0, |(lies, _)| lies)
        })
}

/// Frame-slot promotion is bounded, but cell liveness is required for
/// every uncounted reference. Track the remaining owners in batches.
fn extra_cell_uses(
    b: &Body,
    cx: &dyn OwnCx,
    t: &Tables,
    eligible: impl Fn(usize) -> bool,
    uses: &mut impl FnMut(u32, u32, bool),
) {
    let mut locals = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag == Tag::Ref
            && eligible(i)
            && let Some(l) = b.local_in_place(b.data[i][0])
            && t.bit.get(l.idx()).is_none_or(|&m| m == 0)
        {
            locals.push(l.raw());
        }
    }
    locals.sort_stable();
    locals.dedup();
    let mut extra = Tables::default();
    for batch in locals.chunks(64) {
        let targets: Vec<_> = batch
            .iter()
            .map(|&l| (l, l | crate::ir::place::BARE_LOCAL))
            .collect();
        part_uses(b, cx, &mut extra, &targets, |i, k| uses(i, batch[k], false));
    }
}

/// Uses through references into up to 64 projected places. Reuse the
/// frame-origin fixed point, but seed each field separately and leave
/// frame planning and reference flags alone. The returned bits escaped
/// the body's tracked locals and cannot be moved based on local liveness.
pub(crate) fn part_uses(
    b: &Body,
    cx: &dyn OwnCx,
    t: &mut Tables,
    targets: &[(u32, u32)],
    mut uses: impl FnMut(u32, usize),
) -> u64 {
    debug_assert!(targets.len() <= 64);
    let (n, locals) = (b.len(), b.local_ty.len());
    fill(&mut t.bit, locals, 0);
    t.cands.clear();
    t.targets.clear();
    t.targets.extend_from_slice(targets);
    t.out = 0;
    t.frame = 0;
    for (k, &(local, _)) in targets.iter().enumerate() {
        t.bit[local as usize] |= 1 << k;
        t.cands.push(local);
    }
    fill(&mut t.val, n, 0);
    fill(&mut t.held, locals, 0);
    fill(&mut t.dep, n, 0);
    fill(&mut t.ldep, locals, 0);
    let mut ops = std::mem::take(&mut t.ops);
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            changed |= step(b, cx, t, &mut ops, i);
        }
    }
    escape_exits(b, false, t, &mut ops);
    for i in 0..n {
        if b.tags[i] == Tag::Drop {
            continue;
        }
        let mut origins = 0;
        if matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique)
            && let Some(l) = b.view(b.data[i][0]).local()
        {
            origins |= t.held[l as usize] | t.ldep[l as usize];
        }
        operands(b, i, &mut ops);
        for op in &ops {
            origins |= t.of(op.value) | t.dep.get(op.value as usize).copied().unwrap_or(0);
        }
        for k in bits(origins) {
            uses(u32_of(i), k);
        }
    }
    t.ops = ops;
    t.out
}

/// Returned values and cleanup suites outlive the ordinary instruction
/// sequence, in both frame planning and projected-place liveness. In frame
/// planning (`planning`), a suite's own access of a candidate (a `using`
/// binding's `Ref` for its `dispose`) is no escape: insertion keeps a
/// place a suite reads alive for it, dropping it after the scope or in the
/// exit set, never at a jump that crosses the scope (its rule 4). What a
/// local the suite reads holds, and what its other values point into,
/// still escapes: a jump may drop that first.
fn escape_exits(b: &Body, planning: bool, t: &mut Tables, ops: &mut Vec<super::Operand>) {
    for &r in &b.sub_root {
        t.out |= t.val.get(r as usize).copied().unwrap_or(0);
    }
    let mut stack = Vec::new();
    for i in 0..b.len() {
        let roots = match b.tags[i] {
            Tag::Scope if b.data[i][1] != NONE => b.record(b.data[i][1]),
            Tag::Defer => std::slice::from_ref(&b.data[i][0]),
            _ => continue,
        };
        let (held_out, val_out, own) = (
            std::cell::Cell::new(0u64),
            std::cell::Cell::new(0u64),
            std::cell::Cell::new(0u64),
        );
        let (held, val, bit) = (&t.held, &t.val, &t.bit);
        suite_touches(
            b,
            roots,
            &mut stack,
            ops,
            true,
            |l| {
                held_out.set(held_out.get() | held[l as usize]);
                own.set(own.get() | bit.get(l as usize).copied().unwrap_or(0));
            },
            |v| val_out.set(val_out.get() | val.get(v as usize).copied().unwrap_or(0)),
        );
        let kept = if planning { own.get() } else { 0 };
        t.out |= held_out.get() | (val_out.get() & !kept);
    }
}

/// Adds `m` to `set`: whether it grew.
fn grow(set: &mut u64, m: u64) -> bool {
    let old = *set;
    *set |= m;
    *set != old
}

/// One instruction's constraints: whether any set grew. A read through a
/// reference may become a borrow, which lives on the place it reads, so
/// its value's uses and a local it is stored into keep the place's
/// locals alive (`dep`).
fn step(b: &Body, cx: &dyn OwnCx, t: &mut Tables, ops: &mut Vec<super::Operand>, i: usize) -> bool {
    if t.roots
        && (cx.counted_refs_only(b.ty[i])
            || matches!(b.tags[i], Tag::Call | Tag::CallValue)
                && cx.counted(b.ty[i])
                && !cx.holds_ref(b.ty[i]))
    {
        return false;
    }
    let [a, w] = b.data[i];
    let dep = |t: &Tables, v: u32| match Ref(v).as_inst() {
        Some(_) => t.dep.get(v as usize).copied().unwrap_or(0) | t.of(v),
        None => 0,
    };
    let p = b.view(a);
    let d = match b.tags[i] {
        Tag::Read | Tag::Ref => {
            let root = match p.root {
                Root::Local(l) => t.ldep[l as usize],
                Root::Value(v) => dep(t, v),
                _ => 0,
            };
            let through = p.steps(b).any(|s| s == Proj::Deref);
            root | if through {
                t.place(b, a).map_or(0, |(lies, _)| lies)
            } else {
                0
            }
        }
        Tag::Coerce => dep(t, a) | lent_from(b, cx, t, i),
        // A value that may hold a reference, a `Ptr` too, made from
        // others lives on what they live on: `Ptr.as_ref` of an address
        // read out of a borrowed buffer points into its owner.
        Tag::Store | Tag::Break => 0,
        _ if cx.holds_ref(b.ty[i]) => {
            operands(b, i, ops);
            let mut m = ops.iter().fold(0, |m, op| m | dep(t, op.value)) | lent_from(b, cx, t, i);
            slots(b, i, |_, x, word| {
                if word == Word::Block {
                    m |= dep(t, x);
                }
            });
            m
        }
        _ => 0,
    };
    let mut changed = grow(&mut t.dep[i], d);
    if b.tags[i] == Tag::Break
        && let Some(&x) = b.label_inst.get(a as usize)
        && x != NONE
        && (x as usize) < t.dep.len()
    {
        let m = dep(t, w);
        changed |= grow(&mut t.dep[x as usize], m);
    }
    if let (Tag::Store, Root::Local(l)) = (b.tags[i], p.root)
        && p.in_place(b)
    {
        let m = dep(t, w);
        changed |= grow(&mut t.ldep[l as usize], m);
    }
    changed | step_val(b, cx, t, ops, i)
}

/// [`step`]'s origins: what a value or a local may point into.
fn step_val(
    b: &Body,
    cx: &dyn OwnCx,
    t: &mut Tables,
    ops: &mut Vec<super::Operand>,
    i: usize,
) -> bool {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    match tag {
        Tag::Ref => {
            // A frame cell's reference to an element takes what is there.
            let m = t
                .place(b, a)
                .map_or(0, |(lies, held)| lies | if t.cells { held } else { 0 });
            return grow(&mut t.val[i], m);
        }
        Tag::Coerce if super::lending_coercion(b, cx, i) => {
            let m = lent_from(b, cx, t, i) | t.of(a);
            return grow(&mut t.val[i], m);
        }
        Tag::Read if cx.holds_ref(b.ty[i]) => {
            let (lies, held) = t.place(b, a).unwrap_or((0, 0));
            let m = if super::lending_read(b, cx, i) {
                lies | held
            } else {
                held
            };
            return grow(&mut t.val[i], m);
        }
        Tag::Store if t.of(w) != 0 => {
            let m = t.of(w);
            let holders = t.slot_holders(b, a);
            return t.stored(holders, m);
        }
        Tag::Store => return false,
        // A capture of a reference is a checker error; one that held it
        // would keep it past the local.
        Tag::Closure => {
            let rec = b.record(w);
            let (start, len) = (rec[0] as usize, rec[1] as usize);
            let mut m = 0;
            for c in &b.cap_local[start..start + len] {
                m |= t.held[c.idx()];
            }
            if t.cells {
                // A frame cell's closure captured by another escapes; the
                // closure points into the cells it shares.
                let mut cells = 0;
                for (l, mode) in closure_caps(b, i) {
                    if mode.shares() {
                        cells |= t.bit[l.idx()];
                    }
                }
                return grow(&mut t.out, m) | grow(&mut t.val[i], cells);
            }
            // The closure points where the references it captured do; a
            // use that lets it escape lets them escape.
            return grow(&mut t.val[i], m);
        }
        Tag::Break => {
            let m = t.of(w);
            return match b.label_inst.get(a as usize) {
                Some(&x) if x != NONE && (x as usize) < t.val.len() => {
                    grow(&mut t.val[x as usize], m)
                }
                _ => false,
            };
        }
        _ => {}
    }
    operands(b, i, ops);
    // A list push or map store is a store into a slot of its collection.
    if tag == Tag::Intrinsic
        && matches!(
            IntrinsicOp::from_u32(a),
            Some(IntrinsicOp::ListPush | IntrinsicOp::MapSet)
        )
    {
        let m = ops.iter().skip(1).fold(0, |m, op| m | t.of(op.value));
        if m == 0 {
            return false;
        }
        let holders = if t.cells {
            (None, 0)
        } else {
            t.collection_holders(b, ops[0].value)
        };
        return t.stored(holders, m);
    }
    let mut m = 0;
    let mut consumed = 0;
    for op in ops.iter() {
        let o = t.of(op.value);
        m |= o;
        if op.role == Role::Consume {
            consumed |= o;
        }
    }
    // A result that depends on a counted reference the call lends points
    // where that reference lies (`ref.dep.call`).
    if matches!(tag, Tag::Call | Tag::CallValue) {
        m |= lent_from(b, cx, t, i);
    }
    if tag == Tag::Return || is_suspension(tag) {
        return grow(&mut t.out, m);
    }
    let mut changed = false;
    if t.cells {
        // What a frame cell's closure is passed to may keep it.
        for op in ops.iter() {
            let kept = match (tag, op.role) {
                (Tag::Intrinsic | Tag::CallHost, _) => true,
                (Tag::Call | Tag::CallValue, Role::Arg(k)) => super::keeps_arg(b, cx, i, k),
                _ => false,
            };
            if kept {
                let m = t.of(op.value);
                changed |= grow(&mut t.out, m);
            }
        }
    }
    // A call may store what its other arguments point into through a
    // `&mut` of a local whose type holds a reference (`ref.generic.store`,
    // `ref.lifetime.store.call`), so that local holds them: a candidate
    // the reference points into, or the local its place lies in.
    if matches!(tag, Tag::Call | Tag::CallValue) {
        for op in ops.iter() {
            let v = op.value as usize;
            if Ref(op.value).as_inst().is_none()
                || b.tags[v] != Tag::Ref
                || b.data[v][1] & crate::ir::ref_word::MUT == 0
            {
                continue;
            }
            let others = ops
                .iter()
                .filter(|o| o.value != op.value)
                .filter(|o| match (tag, op.role, o.role) {
                    (Tag::Call, Role::Arg(into), Role::Arg(from)) => {
                        Callee::from_words(b.record(b.data[i][0]))
                            .is_none_or(|c| cx.stores_arg(&c, into as usize, from as usize))
                    }
                    _ => true,
                })
                .fold(0, |acc, o| acc | t.of(o.value));
            if others == 0 {
                continue;
            }
            for k in bits(t.of(op.value)) {
                let l = t.cands[k] as usize;
                if cx.holds_ref(b.local_ty[l]) {
                    changed |= grow(&mut t.held[l], others);
                }
            }
            if let Some(l) = b.local_in_place(b.data[v][0]).map(hd_base::LocalId::idx)
                && cx.holds_ref(b.local_ty[l])
            {
                changed |= grow(&mut t.held[l], others);
            }
        }
    }
    if !cx.holds_ref(b.ty[i]) {
        let call = matches!(tag, Tag::Call | Tag::CallValue | Tag::CallHost | Tag::Drop);
        return (!call && grow(&mut t.out, consumed)) | changed;
    }
    slots(b, i, |_, x, word| {
        if word == Word::Block {
            m |= t.of(x);
        }
    });
    grow(&mut t.val[i], m) | changed
}
