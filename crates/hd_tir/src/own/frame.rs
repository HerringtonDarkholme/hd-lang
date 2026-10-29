//! Frame locals (`crates/hd_wasm_linear/DESIGN.md` §23.38): locals that
//! local references point into, which the backend may keep in a slot of
//! the function's frame rather than a counted cell. A slot holds no count,
//! so every use of a value that may point into the local counts as a use
//! of it, and insertion drops it after the last one. Such uses stay in
//! the local's scope (`spec/lang/13-values-and-references.md#local-scope`).
//!
//! 1. **Candidates.** A local of the root sub-body, not a parameter or a
//!    capture, every `Ref` of whose storage is local (`ref_word::LOCAL`)
//!    there, in a root with no suspension point; the first 64.
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

use super::{OwnCx, Role, fill, operands, suite_touches};
use crate::ir::{
    Body, NONE, Proj, Ref, Root, Tag, Topology, Word, is_suspension, local_flags, slots, u32_of,
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
    ops: Vec<super::Operand>,
}

impl Tables {
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
        for s in p.steps(b) {
            match s {
                // Through a reference: where it points, and what is there.
                Proj::Deref if held != 0 => (lies, held) = (held, self.holdings(held)),
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
/// and calls `uses` with each instruction that uses one of them through a
/// value or a local that may point into it: (instruction, local).
pub(crate) fn run(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    t: &mut Tables,
    mut uses: impl FnMut(u32, u32),
) {
    let (n, locals) = (b.len(), b.local_ty.len());
    fill(&mut t.bit, locals, 0);
    fill(&mut t.bad, locals, false);
    t.cands.clear();
    t.out = 0;
    // A suspending root keeps its locals in its frame object.
    if (0..n).any(|i| is_suspension(b.tags[i]) && topo.sub[i] == 0) {
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
        if b.data[i][1] & crate::ir::ref_word::LOCAL == 0
            || topo.sub[i] != 0
            || b.local_flags[l] & local_flags::PARAM != 0
        {
            t.bad[l] = true;
        }
        if !t.bad[l] && t.bit[l] == 0 && t.cands.len() < 64 {
            t.bit[l] = 1 << t.cands.len();
            t.cands.push(u32_of(l));
        }
    }
    for k in 0..t.cands.len() {
        if t.bad[t.cands[k] as usize] {
            t.out |= 1 << k;
        }
    }
    if t.cands.is_empty() {
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
    // A body's value is returned, as a default argument's reference is.
    for &r in &b.sub_root {
        t.out |= t.val.get(r as usize).copied().unwrap_or(0);
    }
    // A cleanup suite runs at its scope's exits, after a jump has dropped
    // what it leaves, so what a suite reads through a reference stays out.
    let mut stack = Vec::new();
    for i in 0..n {
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
            &mut ops,
            true,
            |l| out.set(out.get() | held[l as usize]),
            |v| out.set(out.get() | val.get(v as usize).copied().unwrap_or(0)),
        );
        t.out |= out.get();
    }
    t.ops = ops;
    let frame = !t.out & (u64::MAX >> (64 - t.cands.len()));
    if frame == 0 {
        return;
    }
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
        for k in bits(m & frame) {
            uses(u32_of(i), t.cands[k]);
        }
        if b.tags[i] == Tag::Ref
            && let Some(l) = b.local_in_place(a)
            && frame & t.bit[l.idx()] != 0
        {
            b.data[i][1] = w | crate::ir::ref_word::FRAME;
        }
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
        _ => 0,
    };
    let mut changed = grow(&mut t.dep[i], d);
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
    if !cx.holds_ref(b.ty[i]) {
        let call = matches!(
            tag,
            Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::Drop
        );
        return !call && grow(&mut t.out, consumed);
    }
    slots(b, i, |_, x, word| {
        if word == Word::Block {
            m |= t.of(x);
        }
    });
    grow(&mut t.val[i], m)
}
