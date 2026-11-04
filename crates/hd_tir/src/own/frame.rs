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
//!    place holds, a `Store` adds to what its place's locals hold, a
//!    `Break` to its target, and any other value that may hold a reference
//!    ([`OwnCx::holds_ref`]) takes its operands' and its blocks'. A read
//!    through a reference may become a borrow, which lives on what it
//!    reads, so its uses keep those locals alive too (`dep`).
//! 3. **Escapes.** A candidate keeps its cell when a value with its origin
//!    is stored where no local of the body holds it, returned (a default
//!    argument's body returns its value), awaited,
//!    captured, read by a cleanup suite (which runs after a jump dropped
//!    what it leaves), or consumed by an instruction other than a call
//!    whose value holds no reference (a list push).
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
//!    takes an element of (`[$xs [%i]]`) is tracked the same way
//!    (`ref.box.root`), in a body with no frame candidate too: what a
//!    reference or a borrowed read through it points into holds no count
//!    of its object or buffer, so the uses of what may point there are the
//!    local's uses too, and its drop or a move of it waits for them.

use hd_base::SliceSort as _;

use super::{OwnCx, Role, fill, operands, suite_touches};
use crate::ir::{Body, NONE, Proj, Ref, Root, Tag, Topology, Word, is_suspension, slots, u32_of};

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
    /// parameter's referent, a top-level binding, a buffer's element).
    fn place(&self, b: &Body, word: u32) -> Option<(u64, u64)> {
        let p = b.view(word);
        let (mut lies, mut held) = match p.root {
            Root::Local(l) => (self.bit[l as usize], self.held[l as usize]),
            Root::Value(v) => (0, self.of(v)),
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
                Proj::Deref | Proj::Elem(_) => return None,
                _ => {}
            }
        }
        Some((lies, held))
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
    // A suspending root keeps its locals in its frame object, but its
    // synchronous closures may still lend uncounted cell references.
    if (0..n).any(|i| is_suspension(b.tags[i]) && topo.sub[i] == 0) {
        extra_cell_uses(b, cx, t, |i| topo.sub[i] != 0, &mut uses);
        root_uses(b, cx, t, &mut uses);
        return;
    }
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
        root_uses(b, cx, t, &mut uses);
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
    escape_exits(b, t, &mut ops);
    t.ops = ops;
    let tracked = u64::MAX >> (64 - t.cands.len());
    let frame = !t.out & tracked;
    t.frame = frame;
    for i in 0..n {
        let [a, w] = b.data[i];
        let mut m = 0;
        if matches!(
            b.tags[i],
            Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Pin
        ) && let Some(l) = b.view(a).local()
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
    root_uses(b, cx, t, &mut uses);
}

/// Rule 5, roots (`ref.box.root`): a counted local whose value a place
/// dereferences (`[*$p .items]`, a `*T`) or takes an element of
/// (`[$xs [%i]]`, a list or a map) is the root of
/// what a reference or a borrowed read through it points into, which
/// holds no count of that object. Each use of such a value is a use of
/// the root, so its drop, or a move of it, follows the reference's last
/// use, in a suspending body's frame too.
fn root_uses(b: &Body, cx: &dyn OwnCx, t: &Tables, uses: &mut impl FnMut(u32, u32, bool)) {
    let mut locals = Vec::new();
    for i in 0..b.len() {
        if !matches!(b.tags[i], Tag::Ref | Tag::Read) {
            continue;
        }
        let p = b.view(b.data[i][0]);
        if let Root::Local(l) = p.root
            && p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
            && cx.counted(b.local_ty[l as usize]) == super::Tri::Yes
            && t.bit.get(l as usize).is_none_or(|&m| m == 0)
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
            && t.bit[l.idx()] == 0
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
    escape_exits(b, t, &mut ops);
    for i in 0..n {
        if b.tags[i] == Tag::Drop {
            continue;
        }
        let mut origins = 0;
        if matches!(
            b.tags[i],
            Tag::Read | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Pin
        ) && let Some(l) = b.view(b.data[i][0]).local()
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
/// sequence, in both frame planning and projected-place liveness.
fn escape_exits(b: &Body, t: &mut Tables, ops: &mut Vec<super::Operand>) {
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
        let (out, held, val) = (std::cell::Cell::new(0u64), &t.held, &t.val);
        suite_touches(
            b,
            roots,
            &mut stack,
            ops,
            true,
            |l| out.set(out.get() | held[l as usize]),
            |v| out.set(out.get() | val.get(v as usize).copied().unwrap_or(0)),
        );
        t.out |= out.get();
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
            || matches!(b.tags[i], Tag::Call | Tag::CallValue | Tag::CallDyn)
                && cx.counted(b.ty[i]) == super::Tri::Yes
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
        Tag::Coerce => dep(t, a),
        // A value that may hold a reference, a `Ptr` too, made from
        // others lives on what they live on: `Ptr.as_ref` of an address
        // read out of a borrowed buffer points into its owner.
        Tag::Store | Tag::Break => 0,
        _ if cx.holds_ref(b.ty[i]) => {
            operands(b, i, ops);
            let mut m = ops.iter().fold(0, |m, op| m | dep(t, op.value));
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
            let m = t.place(b, a).map_or(0, |(lies, _)| lies);
            return grow(&mut t.val[i], m);
        }
        Tag::Read if cx.holds_ref(b.ty[i]) => {
            let m = t.place(b, a).map_or(0, |(_, held)| held);
            return grow(&mut t.val[i], m);
        }
        Tag::Store if t.of(w) != 0 => {
            let m = t.of(w);
            let p = b.view(a);
            if let Root::Local(l) = p.root
                && p.in_place(b)
            {
                return grow(&mut t.held[l as usize], m);
            }
            let mut changed = false;
            match t.place(b, a) {
                Some((lies, _)) if lies != 0 => {
                    for k in bits(lies) {
                        changed |= grow(&mut t.held[t.cands[k] as usize], m);
                    }
                }
                _ => changed = grow(&mut t.out, m),
            }
            return changed;
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
            return grow(&mut t.out, m);
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
    let mut m = 0;
    let mut consumed = 0;
    for op in ops.iter() {
        let o = t.of(op.value);
        m |= o;
        if op.role == Role::Consume {
            consumed |= o;
        }
    }
    if tag == Tag::Return || is_suspension(tag) {
        return grow(&mut t.out, m);
    }
    // A generic call may store what its other arguments point into
    // through a `&mut` of a local whose type holds a reference
    // (`ref.generic.store`), so that local holds them.
    let mut changed = false;
    if matches!(tag, Tag::Call | Tag::CallValue | Tag::CallDyn) {
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
        }
    }
    if !cx.holds_ref(b.ty[i]) {
        let call = matches!(
            tag,
            Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::Drop
        );
        return (!call && grow(&mut t.out, consumed)) | changed;
    }
    slots(b, i, |_, x, word| {
        if word == Word::Block {
            m |= t.of(x);
        }
    });
    grow(&mut t.val[i], m) | changed
}
