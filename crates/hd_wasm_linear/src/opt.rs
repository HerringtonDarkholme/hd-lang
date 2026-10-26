//! A function's local cleanup before encoding (`crates/hd_wasm_linear/DESIGN.md`
//! §23.22): emission gives every TIR value a fresh local, so most values
//! pass through a `local.set` and a `local.get` that follow each other.
//! Each rule below keeps what every path through the function computes:
//!
//! 1. `local.set t; local.get t`, where that set is `t`'s only write and
//!    that get its only read, is the value left on the stack;
//! 2. another `local.set t; local.get t` is `local.tee t`;
//! 3. a `local.set t` of a local no instruction reads is a `drop`, and a
//!    constant or `local.get` it drops goes too;
//! 4. a copy `P; local.set t`, where `P` pushes a local, a global or a
//!    constant and that set is `t`'s only write, moves to `t`'s only read
//!    when the read follows in the same straight-line run of
//!    instructions and nothing between writes what `P` reads;
//! 5. `if A else br N end`, with `N` past the `if` and no branch in `A`
//!    to the `if` itself, is `i32.eqz; br_if N-1; A`, and `if br N else A
//!    end` is `br_if N-1; A`: the shape of a `while` loop's test;
//! 6. `if A; local.set x else B; local.set x end; local.get x`, where
//!    those are `x`'s only writes and that get its only read and neither
//!    arm branches to the `if` itself, is `if (result T) A else B end`;
//! 7. constants fold: a nonzero constant's zero check goes, two constants
//!    joined by `+`, `-` or `*` are one constant, adding 0 on either side
//!    goes, `+ a ± b` is one addition, `x == 0` is `i32.eqz`, a test of
//!    `x != 0` is a test of `x`, a multiplication, quotient or remainder by
//!    a power of two is a shift or a mask, and a constant added to a load's
//!    or store's address is its offset;
//! 8. `local.set a; R; local.set b; local.get a; local.get b`, where
//!    those are `a`'s and `b`'s only writes and reads and `R` is a
//!    straight-line run that pushes one value over what it found, is
//!    `R` over `a`'s value: two arguments evaluated in order;
//! 9. `i32.const c; i32.add; R; store`, with `R` such a run, is `R` and
//!    the store at `c` more offset;
//! 10. `X; local.set a; P; local.get a`, where those are `a`'s only write
//!     and read, `X` is a straight-line run that pushes one value and `P`
//!     pushes a local, a global or a constant that `X` cannot change, is
//!     `P; X`;
//! 11. a local no instruction names is not declared, and the others are
//!     numbered again in order;
//! 12. a release of zero and a redundant store of zero go when forward
//!     local facts prove them zero on every path, including loop backedges;
//! 13. a `local.set` or `local.tee` of a local that no path reads before
//!     its next write goes, a set's value dropped: the zero a move leaves
//!     in a local stays only where some release still reads it;
//! 14. in `local.set t; R; local.get t`, with that get the first access
//!     of `t` after the set and `R` a straight-line run (panic guards
//!     included), a pure push before the set moves to the get when `t`
//!     is dead after it; else, when `R` leaves the stack as it found it,
//!     the get goes, the value staying under `R`, and the set goes too,
//!     or becomes a tee when `t` is read again;
//! 15. once the rules are done ([`coalesce`]), locals of one type whose
//!     values are never live at once share one local, and a copy between
//!     two locals that share one goes.
//!
//! Across functions, [`constants`] reads a top-level binding that only
//! its init writes, with a constant, as that constant.

use std::collections::BTreeMap;

use crate::op::{LabelListId, Op as I};
use hd_base::Hash128;

use crate::module::{Func, Global, Ins, Sym, SymId, Symbols};
use crate::origins::Remap;
use crate::rt::Helper;

/// Reads each top-level binding that one instruction writes, a constant
/// stored by an init, as that constant, and drops the store. An init runs
/// before any code that reads its bindings (`module.init.order`), so every
/// read sees the constant.
pub fn constants(funcs: &mut [Option<Func>], symbols: &Symbols) {
    let mut inits: Vec<Hash128> = Vec::new();
    for (id, _) in funcs.iter().enumerate().filter(|(_, f)| f.is_some()) {
        match &*symbols.get(SymId(u32::try_from(id).expect("symbols"))) {
            Sym::Helper(Helper::EntryInit { inits: ks }) => inits.extend(ks.iter().map(|k| k.0)),
            Sym::Helper(Helper::EntryPoll { then, .. }) => inits.extend(then.iter().map(|k| k.0)),
            _ => {}
        }
    }
    let mut sets: BTreeMap<Global, (u32, Option<Write>)> = BTreeMap::new();
    for (id, f) in funcs
        .iter()
        .enumerate()
        .filter_map(|(i, f)| Some((i, f.as_ref()?)))
    {
        let id = SymId(u32::try_from(id).expect("symbols"));
        let init = matches!(&*symbols.get(id), Sym::Inst(key) if inits.contains(key));
        for (k, i) in f.body.iter().enumerate() {
            if let Ins::GlobalSet(g) = i {
                let global = symbols.global(*g);
                if !matches!(global, Global::Storage(..)) {
                    continue;
                }
                let e = sets.entry(global).or_insert((0, None));
                e.0 += 1;
                e.1 = match (init, k.checked_sub(1).map(|p| &f.body[p])) {
                    (true, Some(c @ Ins::Op(I::I32Const(_) | I::I64Const(_)))) => Some((id, k, *c)),
                    _ => None,
                };
            }
        }
    }
    let known: BTreeMap<Global, Write> = sets
        .into_iter()
        .filter_map(|(g, (n, at))| (n == 1).then_some(at).flatten().map(|at| (g, at)))
        .collect();
    if known.is_empty() {
        return;
    }
    for f in funcs.iter_mut().flatten() {
        for i in &mut f.body {
            if let Ins::GlobalGet(g) = i
                && let Some((_, _, c)) = known.get(&symbols.global(*g))
            {
                *i = *c;
            }
        }
    }
    let mut drops = vec![Vec::new(); funcs.len()];
    for (id, k, _) in known.into_values() {
        drops[id.idx()].push(k);
    }
    for (id, ks) in drops.iter_mut().enumerate() {
        ks.sort_unstable();
        if let Some(f) = &mut funcs[id] {
            for &k in ks.iter().rev() {
                f.drain(k - 1..k + 1);
            }
        }
    }
}

/// A binding's one write when an init stores a constant: the function,
/// the store's index and the constant.
type Write = (SymId, usize, Ins);

/// The instructions a rewrite removes, and an instruction it changes.
type Edit = (Vec<usize>, Option<(usize, Ins)>);

/// The parameter and result counts of each function a call can name.
pub type Arity = Vec<Option<(usize, usize)>>;

/// Cleans up `f`'s locals by the rules of the module documentation.
pub fn locals(f: &mut Func, arity: &Arity) {
    let np = u32::try_from(f.params.len()).expect("params");
    loop {
        let (reads, writes) = counts(f);
        let mut out: Vec<Ins> = Vec::with_capacity(f.body.len());
        let mut origins = Remap::new(f.origins.as_deref());
        let mut changed = false;
        let mut k = 0;
        while k < f.body.len() {
            let next = f.body.get(k + 1);
            match (&f.body[k], next) {
                (Ins::Op(I::LocalSet(t)), Some(Ins::Op(I::LocalGet(u)))) if t == u => {
                    let t = *t as usize;
                    if *u >= np && reads[t] == 1 && writes[t] == 1 {
                        changed = true;
                    } else {
                        out.push(Ins::Op(I::LocalTee(*u)));
                        origins.merged(k..k + 2, 1);
                        changed = true;
                    }
                    k += 2;
                }
                (Ins::Op(I::LocalSet(t)), _) if reads[*t as usize] == 0 => {
                    changed = true;
                    if let Some(Ins::Op(
                        I::LocalGet(_)
                        | I::I32Const(_)
                        | I::I64Const(_)
                        | I::F32Const(_)
                        | I::F64Const(_),
                    )) = out.last()
                    {
                        out.pop();
                        origins.pop();
                    } else {
                        out.push(Ins::Op(I::Drop));
                        origins.copy(k..k + 1);
                    }
                    k += 1;
                }
                (Ins::Op(I::LocalTee(t)), _) if reads[*t as usize] == 0 => {
                    changed = true;
                    k += 1;
                }
                (other, _) => {
                    out.push(*other);
                    origins.copy(k..k + 1);
                    k += 1;
                }
            }
        }
        f.body = out;
        origins.finish(f);
        changed |= forward(f, np);
        changed |= shapes(f, np);
        changed |= fold(f);
        changed |= schedule(f, np, arity);
        // The liveness rules run whenever the others settle.
        if !changed && !zero_releases(f) && !live_rules(f, arity) {
            break;
        }
    }
    renumber(f);
}

/// Local zero facts at structured control-flow edges. Branches meet by
/// intersection; loop backedges participate in the same fixed point.
enum ZeroEdges {
    Next,
    Exit,
    Jump(usize),
    Either(usize, usize),
    Table(Vec<usize>),
}

impl ZeroEdges {
    fn each(&self, next: usize, mut visit: impl FnMut(usize)) {
        match self {
            Self::Next => visit(next),
            Self::Exit => {}
            Self::Jump(to) => visit(*to),
            Self::Either(a, b) => {
                visit(*a);
                visit(*b);
            }
            Self::Table(targets) => targets.iter().copied().for_each(visit),
        }
    }
}

/// The CFG of the closed set of control instructions the backend emits.
/// Its compact IR has no exception, reference-branch or tail-call opcodes.
fn zero_edges(body: &[Ins], label_lists: &[Vec<u32>]) -> Option<Vec<ZeroEdges>> {
    let mut ends = vec![body.len(); body.len()];
    let mut other = vec![None; body.len()];
    let mut labels = Vec::new();
    for (at, ins) in body.iter().enumerate() {
        match ins {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => labels.push(at),
            Ins::Op(I::Else) => other[*labels.last()?] = Some(at),
            Ins::Op(I::End) => {
                if let Some(open) = labels.pop() {
                    ends[open] = at;
                }
            }
            _ => {}
        }
    }
    if !labels.is_empty() {
        return None;
    }
    let mut edges = Vec::with_capacity(body.len());
    for (at, ins) in body.iter().enumerate() {
        let target = |depth: u32, labels: &[usize]| {
            let Some(pos) = labels.len().checked_sub(depth as usize + 1) else {
                return body.len(); // The implicit function label.
            };
            let open = labels[pos];
            if matches!(body[open], Ins::Op(I::Loop(_))) {
                open + 1
            } else {
                ends[open] + 1
            }
        };
        let edge = match ins {
            Ins::Op(I::Block(_) | I::Loop(_)) => {
                labels.push(at);
                ZeroEdges::Next
            }
            Ins::Op(I::If(_)) => {
                labels.push(at);
                ZeroEdges::Either(at + 1, other[at].map_or(ends[at] + 1, |e| e + 1))
            }
            Ins::Op(I::Else) => ZeroEdges::Jump(ends[*labels.last()?] + 1),
            Ins::Op(I::End) => {
                labels.pop();
                ZeroEdges::Next
            }
            Ins::Op(I::Br(depth)) => ZeroEdges::Jump(target(*depth, &labels)),
            Ins::Op(I::BrIf(depth)) => ZeroEdges::Either(at + 1, target(*depth, &labels)),
            Ins::Op(I::BrTable(table, default)) => ZeroEdges::Table(
                label_lists[table.0 as usize]
                    .iter()
                    .chain([default])
                    .map(|d| target(*d, &labels))
                    .collect(),
            ),
            Ins::Op(I::Return | I::Unreachable) => ZeroEdges::Exit,
            _ => ZeroEdges::Next,
        };
        edges.push(edge);
    }
    Some(edges)
}

fn zero_local(known: &[u64], local: u32) -> bool {
    known[local as usize / 64] & (1 << (local % 64)) != 0
}

fn set_zero(known: &mut [u64], local: u32, zero: bool) {
    let word = &mut known[local as usize / 64];
    let bit = 1 << (local % 64);
    if zero {
        *word |= bit;
    } else {
        *word &= !bit;
    }
}

/// A pure push of zero bits; every other expression remains unknown.
fn pushes_zero(ins: &Ins, known: &[u64]) -> bool {
    match ins {
        Ins::Op(I::I32Const(0) | I::I64Const(0)) => true,
        Ins::Op(I::F32Const(v)) => v.bits() == 0,
        Ins::Op(I::F64Const(v)) => v.bits() == 0,
        Ins::Op(I::LocalGet(l)) => zero_local(known, *l),
        _ => false,
    }
}

/// Updates local facts after one original instruction. A store's source
/// is its physical predecessor, including at a control-flow target.
fn zero_after(body: &[Ins], at: usize, known: &mut [u64]) {
    match &body[at] {
        Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => {
            let zero = at.checked_sub(1).is_some_and(|p| {
                pushes_zero(&body[p], known)
                    || matches!(body[p], Ins::Op(I::LocalTee(t)) if zero_local(known, t))
            });
            set_zero(known, *l, zero);
        }
        Ins::LoadAll => known.fill(0), // A suspended frame may contain any values.
        _ => {}
    }
}

/// Deletes only release arguments/calls and stores proved redundant on
/// every incoming path. Packed facts are stored only at basic-block entries;
/// one working set carries facts through each block.
fn zero_releases(f: &mut Func) -> bool {
    let symbols = f.symbols.clone();
    let release_width = |ins: &Ins| {
        let Ins::Call(id) = ins else {
            return None;
        };
        match &*symbols.get(*id) {
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown) => Some(1),
            Sym::Helper(Helper::Release { vals, .. }) => Some(vals.len()),
            _ => None,
        }
    };
    if !f.body.iter().any(|i| release_width(i).is_some()) {
        return false;
    }
    let Some(edges) = zero_edges(&f.body, &f.labels) else {
        return false;
    };
    if f.body.is_empty() {
        return false;
    }
    let (point_at, starts) = basic_blocks(&edges, f.body.len());
    let n = f.params.len() + f.locals.len();
    let words = n.div_ceil(64);
    let mut known = vec![0u64; words];
    for l in f.params.len()..n {
        set_zero(&mut known, u32::try_from(l).expect("locals"), true);
    }
    let mut facts = vec![0u64; starts.len() * words];
    facts[..words].copy_from_slice(&known);
    // Reachability cannot be encoded by the row: a function without
    // locals has zero words but can still have removable releases.
    let mut reachable = vec![false; starts.len()];
    reachable[0] = true;
    let mut pending = std::collections::VecDeque::from([0]);
    let mut queued = vec![false; starts.len()];
    queued[0] = true;
    while let Some(point) = pending.pop_front() {
        queued[point] = false;
        known.copy_from_slice(&facts[point * words..(point + 1) * words]);
        let end = starts.get(point + 1).copied().unwrap_or(f.body.len());
        for at in starts[point]..end {
            zero_after(&f.body, at, &mut known);
        }
        edges[end - 1].each(end, |to| {
            let Some(&next) = point_at.get(to) else {
                return;
            };
            debug_assert_ne!(next, usize::MAX, "a block successor begins a block");
            let before = &mut facts[next * words..(next + 1) * words];
            let changed = if reachable[next] {
                let mut changed = false;
                for (a, b) in before.iter_mut().zip(&known) {
                    let meet = *a & *b;
                    changed |= meet != *a;
                    *a = meet;
                }
                changed
            } else {
                reachable[next] = true;
                before.copy_from_slice(&known);
                true
            };
            if changed && !queued[next] {
                pending.push_back(next);
                queued[next] = true;
            }
        });
    }
    let mut gone = vec![false; f.body.len()];
    for (point, &start) in starts.iter().enumerate() {
        if !reachable[point] {
            continue;
        }
        known.copy_from_slice(&facts[point * words..(point + 1) * words]);
        let end = starts.get(point + 1).copied().unwrap_or(f.body.len());
        for at in start..end {
            let ins = &f.body[at];
            if let Some(n) = release_width(ins) {
                let mut args = Vec::new();
                for p in (0..at).rev() {
                    if matches!(f.body[p], Ins::Anchor(_)) {
                        continue;
                    }
                    if args.len() == n || !pushes_zero(&f.body[p], &known) {
                        break;
                    }
                    args.push(p);
                }
                if args.len() == n {
                    gone[at] = true;
                    for p in args {
                        gone[p] = true;
                    }
                }
            } else if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
                && zero_local(&known, *l)
                && at
                    .checked_sub(1)
                    .is_some_and(|p| pushes_zero(&f.body[p], &known))
            {
                gone[at] = true;
                if matches!(ins, Ins::Op(I::LocalSet(_))) {
                    gone[at - 1] = true;
                }
            }
            // Replay original transfers after deciding from the incoming
            // facts; pending removals must not affect later decisions.
            zero_after(&f.body, at, &mut known);
        }
    }
    if !gone.iter().any(|g| *g) {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    f.body = std::mem::take(&mut f.body)
        .into_iter()
        .enumerate()
        .filter_map(|(at, ins)| {
            if gone[at] {
                None
            } else {
                origins.copy(at..at + 1);
                Some(ins)
            }
        })
        .collect();
    origins.finish(f);
    true
}

/// The basic blocks of a body whose control edges are `edges`: each
/// instruction's block, and each block's first instruction. A
/// terminator ends its block and every branch target starts one.
fn basic_blocks(edges: &[ZeroEdges], len: usize) -> (Vec<usize>, Vec<usize>) {
    let mut point_at = vec![usize::MAX; len];
    if let Some(first) = point_at.first_mut() {
        *first = 0;
    }
    for (at, edge) in edges.iter().enumerate() {
        if !matches!(edge, ZeroEdges::Next) {
            if let Some(next) = point_at.get_mut(at + 1) {
                *next = 0;
            }
            edge.each(at + 1, |to| {
                if let Some(target) = point_at.get_mut(to) {
                    *target = 0;
                }
            });
        }
    }
    let mut starts = Vec::new();
    for (at, point) in point_at.iter_mut().enumerate() {
        if *point == 0 {
            *point = starts.len();
            starts.push(at);
        } else {
            *point = starts.len() - 1;
        }
    }
    (point_at, starts)
}

/// The locals live into each basic block of a body, by a backward pass
/// to a fixed point (loop backedges included). `SaveAll` reads every
/// local, and `LoadAll` writes none for this purpose.
struct Live {
    edges: Vec<ZeroEdges>,
    point_at: Vec<usize>,
    starts: Vec<usize>,
    words: usize,
    into: Vec<u64>,
}

impl Live {
    fn new(f: &Func) -> Option<Self> {
        let n = f.params.len() + f.locals.len();
        if n == 0 || f.body.is_empty() {
            return None;
        }
        let edges = zero_edges(&f.body, &f.labels)?;
        let (point_at, starts) = basic_blocks(&edges, f.body.len());
        let words = n.div_ceil(64);
        let mut me = Self {
            into: vec![0u64; starts.len() * words],
            edges,
            point_at,
            starts,
            words,
        };
        let mut live = vec![0u64; words];
        loop {
            let mut changed = false;
            for p in (0..me.starts.len()).rev() {
                me.out(p, f.body.len(), &mut live);
                for ins in f.body[me.starts[p]..me.end(p, f.body.len())].iter().rev() {
                    Self::step(ins, &mut live);
                }
                let row = &mut me.into[p * words..(p + 1) * words];
                if row != live.as_slice() {
                    row.copy_from_slice(&live);
                    changed = true;
                }
            }
            if !changed {
                return Some(me);
            }
        }
    }

    fn end(&self, p: usize, len: usize) -> usize {
        self.starts.get(p + 1).copied().unwrap_or(len)
    }

    /// The locals live after block `p`: those live into its successors.
    fn out(&self, p: usize, len: usize, live: &mut [u64]) {
        live.fill(0);
        let end = self.end(p, len);
        let w = self.words;
        self.edges[end - 1].each(end, |to| {
            if let Some(&q) = self.point_at.get(to) {
                for (a, b) in live.iter_mut().zip(&self.into[q * w..(q + 1) * w]) {
                    *a |= *b;
                }
            }
        });
    }

    fn step(ins: &Ins, live: &mut [u64]) {
        match ins {
            Ins::Op(I::LocalGet(l)) => set_zero(live, *l, true),
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => set_zero(live, *l, false),
            Ins::SaveAll => live.fill(u64::MAX),
            _ => {}
        }
    }

    /// Visits each instruction of `body`, last to first within each
    /// block, with the locals live just after it.
    fn walk_back(&self, body: &[Ins], mut visit: impl FnMut(usize, &[u64])) {
        let mut live = vec![0u64; self.words];
        for p in 0..self.starts.len() {
            self.out(p, body.len(), &mut live);
            for at in (self.starts[p]..self.end(p, body.len())).rev() {
                visit(at, &live);
                Self::step(&body[at], &mut live);
            }
        }
    }

    /// The locals live at the function's entry.
    fn entry(&self) -> &[u64] {
        &self.into[..self.words]
    }
}

/// Rule 15: locals of one type whose values are never live at once share
/// one local, as a register allocator colors its interference graph. A
/// write conflicts with each local live just after it, except with the
/// source of a copy, which holds the same value; parameters are written
/// at entry, where every local live there holds its parameter or its
/// initial zero. Each local in turn joins the first class of its type
/// (parameters first) it conflicts with no member of; a copy between two
/// locals of one class then goes.
pub fn coalesce(f: &mut Func) {
    if f.raw.is_some()
        || f.body
            .iter()
            .any(|i| matches!(i, Ins::SaveAll | Ins::LoadAll))
    {
        return;
    }
    let Some(live) = Live::new(f) else {
        return;
    };
    let np = f.params.len();
    let n = np + f.locals.len();
    let w = live.words;
    let body = &f.body;
    // `rows[x]`: the locals live where `x` is written.
    let mut rows = vec![0u64; n * w];
    let mut used = vec![false; n];
    let mut write = vec![0u64; w];
    live.walk_back(body, |at, after| {
        let Ins::Op(I::LocalSet(x) | I::LocalTee(x) | I::LocalGet(x)) = &body[at] else {
            return;
        };
        let x = *x as usize;
        used[x] = true;
        if matches!(body[at], Ins::Op(I::LocalGet(_))) {
            return;
        }
        write.copy_from_slice(after);
        if let Some(Ins::Op(I::LocalGet(y))) = at.checked_sub(1).map(|p| &body[p]) {
            set_zero(&mut write, *y, false);
        }
        for (r, a) in rows[x * w..(x + 1) * w].iter_mut().zip(&write) {
            *r |= *a;
        }
    });
    for p in 0..np {
        for (r, a) in rows[p * w..(p + 1) * w].iter_mut().zip(live.entry()) {
            *r |= *a;
        }
    }
    let ty = |l: usize| {
        if l < np {
            f.params[l]
        } else {
            f.locals[l - np]
        }
    };
    // Each class: its local, its type, its members and the union of
    // their rows.
    let mut classes: Vec<(u32, crate::module::Val, Vec<u64>, Vec<u64>)> = Vec::new();
    let mut to: Vec<u32> = (0..u32::try_from(n).expect("locals")).collect();
    for x in 0..n {
        if x >= np && !used[x] {
            continue;
        }
        let xl = u32::try_from(x).expect("locals");
        let row = &rows[x * w..(x + 1) * w];
        let fits = |(_, t, members, union): &&mut (u32, crate::module::Val, Vec<u64>, Vec<u64>)| {
            *t == ty(x)
                && !zero_local(union, xl)
                && row.iter().zip(members.iter()).all(|(a, b)| a & b == 0)
        };
        if let Some((rep, _, members, union)) = classes.iter_mut().find(|c| x >= np && fits(c)) {
            to[x] = *rep;
            set_zero(members, xl, true);
            for (u, r) in union.iter_mut().zip(row) {
                *u |= *r;
            }
        } else {
            let mut members = vec![0u64; w];
            set_zero(&mut members, xl, true);
            classes.push((xl, ty(x), members, row.to_vec()));
        }
    }
    if to.iter().enumerate().all(|(x, t)| *t as usize == x) {
        return;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out: Vec<Ins> = Vec::with_capacity(body.len());
    for (at, ins) in body.iter().enumerate() {
        let ins = match *ins {
            Ins::Op(I::LocalGet(l)) => Ins::Op(I::LocalGet(to[l as usize])),
            Ins::Op(I::LocalSet(l)) => Ins::Op(I::LocalSet(to[l as usize])),
            Ins::Op(I::LocalTee(l)) => Ins::Op(I::LocalTee(to[l as usize])),
            other => other,
        };
        match (out.last(), ins) {
            (Some(Ins::Op(I::LocalGet(a))), Ins::Op(I::LocalSet(b))) if *a == b => {
                out.pop();
                origins.pop();
            }
            (Some(Ins::Op(I::LocalGet(a))), Ins::Op(I::LocalTee(b))) if *a == b => {}
            _ => {
                out.push(ins);
                origins.copy(at..at + 1);
            }
        }
    }
    f.body = out;
    origins.finish(f);
    if let Some(names) = &mut f.local_names {
        let mut merged = BTreeMap::new();
        for (l, name) in std::mem::take(names) {
            merged.entry(to[l as usize]).or_insert(name);
        }
        *names = merged;
    }
    renumber(f);
}

/// Rules 13 and 14, from one backward liveness pass over the basic blocks
/// (loop backedges to a fixed point; `SaveAll` reads every local, and
/// `LoadAll` writes none for this purpose).
///
/// Rule 13 deletes a `local.set` or `local.tee` of a local that no path
/// reads before its next write, dropping a set's value, or the pure push
/// that made it. A zero left in a moved-from local stays exactly when
/// some path still releases it (the one exit, a loop's next drop, a
/// suspension's saved frame).
///
/// Rule 14 keeps a value on the stack. In `local.set t; R; local.get t`,
/// where that get is the first access of `t` after the set and `R` is a
/// straight-line run (panic guards `if ... unreachable end` included):
///
/// - when `t` is dead after the get, the pure push the set takes (the
///   one just before it, or its partner in a multi-value copy) moves to
///   the get, if `R` writes nothing it reads: rule 4 by liveness rather
///   than by counts; a `local.tee t` after a pure push goes the same
///   way, the get pushing that push again;
/// - otherwise, when `R` leaves the stack as it found it without
///   reaching below, the get goes and the value stays under `R`; the
///   set goes too when `t` is dead after the get, and becomes a tee
///   when it is not.
///
/// A rewrite's span is entered only at its write, its get is the write's
/// only reader, and nothing in it writes a moved push's local, so no
/// rewrite changes a liveness fact another one reads. A rewrite that
/// leaves its value under its run nests in or follows another such
/// rewrite, and holds both or neither end of a moved multi-value copy,
/// so each run keeps the stack level its rewrite needs.
fn live_rules(f: &mut Func, arity: &Arity) -> bool {
    let mut any = false;
    while live_once(f, arity) {
        any = true;
    }
    any
}

fn live_once(f: &mut Func, arity: &Arity) -> bool {
    let Some(live) = Live::new(f) else {
        return false;
    };
    let body = &f.body;
    // `dead[at]`: a write that no read follows, or a read after which
    // its local is dead.
    let mut dead = vec![false; body.len()];
    live.walk_back(body, |at, after| {
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l) | I::LocalGet(l)) = &body[at] {
            dead[at] = !zero_local(after, *l);
        }
    });
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(body.len());
    let dead_write =
        |at: usize| matches!(body[at], Ins::Op(I::LocalSet(_) | I::LocalTee(_))) && dead[at];
    let mut gone = vec![false; body.len()];
    let mut moved = vec![None; body.len()];
    // The spans of the pairs that leave their value under their run, and
    // the pushes and sets between a moved push and its set in a
    // multi-value copy: a pair that keeps the stack level must not hold
    // only one end of such a copy.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut in_copy = vec![false; body.len()];
    let mut teed = vec![false; body.len()];
    // `next[at]`: the next access of the local that `at` writes.
    let mut next = vec![usize::MAX; body.len()];
    let mut seen = vec![usize::MAX; f.params.len() + f.locals.len()];
    for (at, ins) in body.iter().enumerate().rev() {
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l) | I::LocalGet(l)) = ins {
            next[at] = seen[*l as usize];
            seen[*l as usize] = at;
        }
    }
    for k in 0..body.len() {
        let tee = match body[k] {
            Ins::Op(I::LocalSet(_)) => false,
            Ins::Op(I::LocalTee(_)) => true,
            _ => continue,
        };
        if dead[k] {
            continue;
        }
        while open.last().is_some_and(|&(_, e)| e < k) {
            open.pop();
        }
        let j = next[k];
        if j == usize::MAX || !matches!(body[j], Ins::Op(I::LocalGet(_))) {
            continue;
        }
        let last = dead[j];
        let Some(level) = single_use(body, k, j, arity, &f.symbols) else {
            continue;
        };
        if let Some(p) = if tee {
            k.checked_sub(1)
        } else {
            matching_push(body, k)
        } && last
            && pure_push(&body[p])
            && !gone[p]
            && moved[p].is_none()
            && open.last().is_none_or(|&(s, _)| s <= p)
            && !body[p + 1..j].iter().any(|i| clobbers(&body[p], i))
        {
            // A tee's push stays where it is, and the get reads it again.
            gone[p] = !tee;
            moved[j] = Some(p);
            in_copy[p + 1..k].fill(true);
        } else if level && !tee && !in_copy[k] && open.last().is_none_or(|&(_, e)| j <= e) {
            // The value stays under the run; a set read again later
            // becomes a tee, unless the get feeds a copy, which rule 14
            // moves instead.
            if !last && matches!(body.get(j + 1), Some(Ins::Op(I::LocalSet(_)))) {
                continue;
            }
            gone[j] = true;
            open.push((k, j));
            if !last {
                teed[k] = true;
                continue;
            }
        } else {
            continue;
        }
        gone[k] = true;
    }
    if !gone.iter().any(|g| *g) && !(0..body.len()).any(dead_write) {
        return false;
    }
    for (at, ins) in body.iter().enumerate() {
        if gone[at] {
            continue;
        }
        if let (true, Ins::Op(I::LocalSet(t))) = (teed[at], ins) {
            out.push(Ins::Op(I::LocalTee(*t)));
            origins.copy(at..at + 1);
        } else if let Some(p) = moved[at] {
            out.push(body[p]);
            let origin = origins
                .first(p..p + 1)
                .or_else(|| origins.first(at..at + 1));
            origins.repeat(origin, 1);
        } else if dead_write(at) {
            // A dead set drops its value, or takes back the pure push
            // just before it when that push is still in place.
            if matches!(ins, Ins::Op(I::LocalTee(_))) {
                continue;
            }
            if at > 0 && pure_push(&body[at - 1]) && !gone[at - 1] && moved[at - 1].is_none() {
                out.pop();
                origins.pop();
            } else {
                out.push(Ins::Op(I::Drop));
                origins.copy(at..at + 1);
            }
        } else {
            out.push(*ins);
            origins.copy(at..at + 1);
        }
    }
    f.body = out;
    origins.finish(f);
    true
}

/// The pure push that the `local.set` at `k` takes its value from, when
/// a row of pure pushes and then a row of sets ends at `k`: the sets
/// take the pushes back in reverse, as a multi-value copy does.
fn matching_push(b: &[Ins], k: usize) -> Option<usize> {
    let mut i = k;
    while i > 0 && matches!(b[i - 1], Ins::Op(I::LocalSet(_))) {
        i -= 1;
    }
    let p = i.checked_sub(1 + (k - i))?;
    b[p..i].iter().all(pure_push).then_some(p)
}

/// Whether the read at `j` is in the same straight-line run as the write
/// at `k` (a panic guard counts as straight line), and if so, whether
/// the run between leaves the stack as it found it without reaching
/// below.
fn single_use(b: &[Ins], k: usize, j: usize, arity: &Arity, symbols: &Symbols) -> Option<bool> {
    // Bounds the scan, so the rule stays linear in the body.
    const REACH: usize = 128;
    if j - k > REACH {
        return None;
    }
    let (mut depth, mut low) = (0usize, false);
    let mut at = k + 1;
    while at < j {
        let i = &b[at];
        let (pops, pushes) = match i {
            Ins::Op(I::If(wasm_encoder::BlockType::Empty)) => {
                at = guard_end(b, at)?;
                (1, 0)
            }
            _ if ends_run(i) => return None,
            _ => effect(i, arity, symbols)?,
        };
        low |= pops > depth;
        depth = depth.saturating_sub(pops) + pushes;
        at += 1;
    }
    (at == j).then_some(depth == 0 && !low)
}

/// The `end` of the `if` at `at` when its one arm calls with constant
/// arguments and ends in `unreachable`, as a bounds or overflow check's
/// panic does: no local, branch or stack value of the code around it.
fn guard_end(b: &[Ins], at: usize) -> Option<usize> {
    for (j, i) in b.iter().enumerate().skip(at + 1) {
        match i {
            Ins::Op(I::End) => return matches!(b[j - 1], Ins::Op(I::Unreachable)).then_some(j),
            Ins::Op(I::Unreachable | I::I32Const(_) | I::I64Const(_))
            | Ins::Call(_)
            | Ins::Anchor(_) => {}
            _ => return None,
        }
    }
    None
}

/// An instruction that pushes one value and has no other effect.
fn pure_push(i: &Ins) -> bool {
    matches!(
        i,
        Ins::Op(I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_))
            | Ins::GlobalGet(_)
    )
}

/// Rule 4: moves each copy into a local read once to its read.
fn forward(f: &mut Func, np: u32) -> bool {
    let (reads, writes) = counts(f);
    let mut changed = false;
    let mut k = 0;
    while k + 1 < f.body.len() {
        let Ins::Op(I::LocalSet(t)) = f.body[k + 1] else {
            k += 1;
            continue;
        };
        let p = f.body[k];
        let pure = matches!(
            p,
            Ins::Op(
                I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_)
            ) | Ins::GlobalGet(_)
                | Ins::TableSlot(_)
                | Ins::VTable(_)
        );
        #[cfg(feature = "checked-report")]
        let pure = pure || matches!(p, Ins::Descriptor(..));
        if !pure || t < np || reads[t as usize] != 1 || writes[t as usize] != 1 {
            k += 1;
            continue;
        }
        let mut found = None;
        for j in k + 2..f.body.len() {
            let i = &f.body[j];
            if matches!(i, Ins::Op(I::LocalGet(u)) if *u == t) {
                found = Some(j);
                break;
            }
            if clobbers(&p, i) || ends_run(i) {
                break;
            }
        }
        if let Some(j) = found {
            f.body[j] = p;
            if let Some(origins) = &mut f.origins {
                origins[j] = origins[k].or(origins[k + 1]).or(origins[j]);
            }
            f.drain(k..k + 2);
            changed = true;
        } else {
            k += 1;
        }
    }
    changed
}

/// Rule 7: folds the constants of each window of instructions.
fn fold(f: &mut Func) -> bool {
    let mut origins = Remap::new(f.origins.as_deref());
    let b = std::mem::take(&mut f.body);
    let mut out: Vec<Ins> = Vec::with_capacity(b.len());
    let mut changed = false;
    let mut k = 0;
    let c32 = |i: Option<&Ins>| match i {
        Some(Ins::Op(I::I32Const(c))) => Some(*c),
        _ => None,
    };
    let push = |i: Option<&Ins>| {
        matches!(
            i,
            Some(
                Ins::Op(
                    I::LocalGet(_)
                        | I::I32Const(_)
                        | I::I64Const(_)
                        | I::F32Const(_)
                        | I::F64Const(_)
                ) | Ins::GlobalGet(_)
            )
        )
    };
    while k < b.len() {
        let from = k;
        let output_start = out.len();
        let at = |j: usize| b.get(k + j);
        // `c; local.tee t; i32.eqz; if ... end` of a nonzero `c`, with no
        // `else`: the test never holds.
        if let (
            Some(c),
            Some(Ins::Op(I::LocalTee(t))),
            Some(Ins::Op(I::I32Eqz)),
            Some(Ins::Op(I::If(_))),
        ) = (c32(at(0)), at(1), at(2), at(3))
            && c != 0
            && let Some(end) = block_end(&b, k + 3)
        {
            out.push(Ins::Op(I::I32Const(c)));
            out.push(Ins::Op(I::LocalSet(*t)));
            k = end + 1;
            changed = true;
            origins.merged(from..k, out.len() - output_start);
            continue;
        }
        match (at(0), at(1), at(2), at(3)) {
            // Two constants joined by `+`, `-` or `*` are one constant
            // (`i32` wraps either way).
            (
                Some(Ins::Op(I::I32Const(a))),
                Some(Ins::Op(I::I32Const(b))),
                Some(Ins::Op(op @ (I::I32Add | I::I32Sub | I::I32Mul))),
                _,
            ) => {
                let c = match op {
                    I::I32Add => a.wrapping_add(*b),
                    I::I32Sub => a.wrapping_sub(*b),
                    _ => a.wrapping_mul(*b),
                };
                out.push(Ins::Op(I::I32Const(c)));
                k += 3;
                changed = true;
            }
            // Adding 0 goes, on either side.
            (Some(Ins::Op(I::I32Const(0))), Some(Ins::Op(I::I32Add)), ..) => {
                k += 2;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(0))), v, Some(Ins::Op(I::I32Add)), _) if push(v) => {
                out.push(*v.expect("a push"));
                k += 3;
                changed = true;
            }
            // `+ a ± b` is one addition, of `a ± b` (`i32` wraps either way).
            (
                Some(Ins::Op(I::I32Const(a))),
                Some(Ins::Op(I::I32Add)),
                Some(Ins::Op(I::I32Const(b))),
                Some(Ins::Op(op @ (I::I32Add | I::I32Sub))),
            ) => {
                let c = if matches!(op, I::I32Add) {
                    a.wrapping_add(*b)
                } else {
                    a.wrapping_sub(*b)
                };
                out.extend([Ins::Op(I::I32Const(c)), Ins::Op(I::I32Add)]);
                k += 4;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(0))), Some(Ins::Op(I::I32Eq)), ..) => {
                out.push(Ins::Op(I::I32Eqz));
                k += 2;
                changed = true;
            }
            (
                Some(Ins::Op(I::I32Const(0))),
                Some(Ins::Op(I::I32Ne)),
                Some(Ins::Op(I::If(_) | I::BrIf(_))),
                _,
            ) => {
                k += 2;
                changed = true;
            }
            (
                Some(Ins::Op(I::I32Const(c))),
                Some(Ins::Op(op @ (I::I32DivU | I::I32RemU | I::I32Mul))),
                ..,
            ) if *c > 0
                && c.cast_unsigned().is_power_of_two()
                && (*c >= 64 || !matches!(op, I::I32Mul)) =>
            {
                let n = c.cast_unsigned().trailing_zeros().cast_signed();
                match op {
                    I::I32DivU => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32ShrU)]),
                    I::I32RemU => out.extend([Ins::Op(I::I32Const(c - 1)), Ins::Op(I::I32And)]),
                    _ => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32Shl)]),
                }
                k += 2;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add)), Some(Ins::Op(load)), _)
                if *c >= 0 && with_offset(load, 0).is_some() && is_load(load) =>
            {
                out.push(Ins::Op(
                    with_offset(load, c.cast_unsigned()).expect("a load"),
                ));
                k += 3;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add)), v, Some(Ins::Op(store)))
                if *c >= 0 && push(v) && with_offset(store, 0).is_some() && !is_load(store) =>
            {
                out.push(*v.expect("a push"));
                out.push(Ins::Op(
                    with_offset(store, c.cast_unsigned()).expect("a store"),
                ));
                k += 4;
                changed = true;
            }
            _ => {
                out.push(b[k]);
                k += 1;
            }
        }
        origins.merged(from..k, out.len() - output_start);
    }
    f.body = out;
    origins.finish(f);
    changed
}

/// Rules 8 and 9: lets values stay on the stack across a straight-line
/// run that pushes one more.
fn schedule(f: &mut Func, np: u32, arity: &Arity) -> bool {
    let mut changed = false;
    'outer: loop {
        let (reads, writes) = counts(f);
        let once = |l: u32| l >= np && reads[l as usize] == 1 && writes[l as usize] == 1;
        let b = &f.body;
        let mut edit: Option<Edit> = None;
        for q in 0..b.len() {
            // Rule 9.
            if let (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add))) =
                (b.get(q), b.get(q + 1))
                && *c >= 0
                && let Some(t) = store_after(b, q + 2, arity, &f.symbols)
                && let Ins::Op(st) = &b[t]
                && !is_load(st)
                && let Some(moved) = with_offset(st, c.cast_unsigned())
            {
                edit = Some((vec![q, q + 1], Some((t, Ins::Op(moved)))));
                break;
            }
            // Rule 10.
            if let (Some(Ins::Op(I::LocalSet(a))), Some(p), Some(Ins::Op(I::LocalGet(g)))) =
                (b.get(q), b.get(q + 1), b.get(q + 2))
                && a == g
                && once(*a)
                && matches!(
                    p,
                    Ins::Op(I::LocalGet(_) | I::I32Const(_) | I::I64Const(_)) | Ins::GlobalGet(_)
                )
                && let Some(s) = run_start(b, q, arity, &f.symbols)
                && !b[s..q].iter().any(|x| clobbers(p, x))
            {
                let mut body = b[..s].to_vec();
                body.push(*p);
                body.extend_from_slice(&b[s..q]);
                body.extend_from_slice(&b[q + 3..]);
                let mut origins = Remap::new(f.origins.as_deref());
                origins.copy(0..s);
                origins.copy(q + 1..q + 2);
                origins.copy(s..q);
                origins.copy(q + 3..b.len());
                f.body = body;
                origins.finish(f);
                changed = true;
                continue 'outer;
            }
            // Rule 8.
            let (
                Some(Ins::Op(I::LocalSet(bl))),
                Some(Ins::Op(I::LocalGet(al))),
                Some(Ins::Op(I::LocalGet(bg))),
            ) = (b.get(q), b.get(q + 1), b.get(q + 2))
            else {
                continue;
            };
            if bl != bg || al == bl || !once(*al) || !once(*bl) {
                continue;
            }
            let Some(p) = (0..q)
                .rev()
                .find(|&p| matches!(b[p], Ins::Op(I::LocalSet(x)) if x == *al))
            else {
                continue;
            };
            if net(b, p + 1, q, arity, &f.symbols) == Some(1) {
                edit = Some((vec![p, q, q + 1, q + 2], None));
                break;
            }
        }
        let Some((gone, replace)) = edit else {
            return changed;
        };
        if let Some((t, i)) = replace {
            f.body[t] = i;
            if let Some(origins) = &mut f.origins {
                origins[t] = gone
                    .iter()
                    .chain(std::iter::once(&t))
                    .find_map(|&i| origins[i]);
            }
        }
        for k in gone.into_iter().rev() {
            f.remove(k);
        }
        changed = true;
    }
}

/// The depth over its start that straight-line run `b[from..to]` ends at,
/// when no instruction of it reaches below that start.
fn net(b: &[Ins], from: usize, to: usize, arity: &Arity, symbols: &Symbols) -> Option<usize> {
    let mut depth = 0usize;
    for i in &b[from..to] {
        let (pops, pushes) = effect(i, arity, symbols)?;
        depth = depth.checked_sub(pops)? + pushes;
    }
    Some(depth)
}

/// The latest `s` before `q` whose run `b[s..q]` ends one deeper than it
/// starts without reaching below its start: what `net(b, s, q) == Some(1)`
/// finds, in one backward walk. Each step prepends an instruction to the
/// run, keeping the depth the run needs at its start and its net change.
fn run_start(b: &[Ins], q: usize, arity: &Arity, symbols: &Symbols) -> Option<usize> {
    let (mut need, mut net) = (0usize, 0isize);
    for s in (0..q).rev() {
        let (pops, pushes) = effect(&b[s], arity, symbols)?;
        need = pops + need.saturating_sub(pushes);
        net += pushes.cast_signed() - pops.cast_signed();
        if need == 0 && net == 1 {
            return Some(s);
        }
    }
    None
}

/// The store that takes the address below the straight-line run from
/// `at` and the one value the run pushes over it: the first instruction
/// of the run that reaches below its start, when that is a store at depth
/// one.
fn store_after(b: &[Ins], at: usize, arity: &Arity, symbols: &Symbols) -> Option<usize> {
    let mut depth = 0usize;
    for (k, i) in b.iter().enumerate().skip(at) {
        let (pops, pushes) = effect(i, arity, symbols)?;
        if pops > depth {
            let store = matches!(i, Ins::Op(op) if !is_load(op) && with_offset(op, 0).is_some());
            return (store && depth == 1).then_some(k);
        }
        depth = depth - pops + pushes;
    }
    None
}

/// What straight-line instruction `i` pops and pushes; `None` for control
/// and for anything this table does not know.
fn effect(i: &Ins, arity: &Arity, symbols: &Symbols) -> Option<(usize, usize)> {
    Some(match i {
        Ins::GlobalGet(_) | Ins::TableSlot(_) | Ins::VTable(_) => (0, 1),
        #[cfg(feature = "checked-report")]
        Ins::Descriptor(..) => (0, 1),
        Ins::Anchor(_) => (0, 0),
        Ins::GlobalSet(_) => (1, 0),
        Ins::Call(id) => match &*symbols.get(*id) {
            Sym::Import(im) => (im.params.len(), im.results.len()),
            _ => (*arity.get(id.idx())?)?,
        },
        Ins::CallIndirect(id) => {
            let signature = symbols.signature(*id);
            (signature.0.len() + 1, signature.1.len())
        }
        Ins::Op(op) => match op {
            I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_) => {
                (0, 1)
            }
            I::LocalTee(_) => (1, 1),
            I::LocalSet(_) | I::Drop => (1, 0),
            I::Select => (3, 1),
            I::I32Eqz
            | I::I64Eqz
            | I::I32WrapI64
            | I::I64ExtendI32S
            | I::I64ExtendI32U
            | I::I32Clz
            | I::I64Clz
            | I::I32Ctz
            | I::I64Ctz
            | I::I32Popcnt
            | I::I64Popcnt
            | I::I32Extend8S
            | I::I32Extend16S
            | I::I32ReinterpretF32
            | I::I64ReinterpretF64
            | I::F32ReinterpretI32
            | I::F64ReinterpretI64
            | I::I32TruncSatF32S
            | I::I32TruncSatF32U
            | I::I32TruncSatF64S
            | I::I32TruncSatF64U
            | I::I64TruncSatF32S
            | I::I64TruncSatF32U
            | I::I64TruncSatF64S
            | I::I64TruncSatF64U
            | I::F32ConvertI32S
            | I::F32ConvertI32U
            | I::F32ConvertI64S
            | I::F32ConvertI64U
            | I::F64ConvertI32S
            | I::F64ConvertI32U
            | I::F64ConvertI64S
            | I::F64ConvertI64U
            | I::F32DemoteF64
            | I::F64PromoteF32
            | I::F32Neg
            | I::F64Neg
            | I::F32Sqrt
            | I::F64Sqrt
            | I::MemoryGrow(_) => (1, 1),
            I::MemorySize(_) => (0, 1),
            I::MemoryCopy { .. } | I::MemoryFill(_) => (3, 0),
            I::I32Add
            | I::I32Sub
            | I::I32Mul
            | I::I32And
            | I::I32Or
            | I::I32Xor
            | I::I32Shl
            | I::I32ShrU
            | I::I32ShrS
            | I::I32Eq
            | I::I32Ne
            | I::I32LtU
            | I::I32LtS
            | I::I32GtU
            | I::I32GtS
            | I::I32LeU
            | I::I32LeS
            | I::I32GeU
            | I::I32GeS
            | I::I64Add
            | I::I64Sub
            | I::I64Mul
            | I::I64And
            | I::I64Or
            | I::I64Xor
            | I::I64Shl
            | I::I64ShrU
            | I::I64ShrS
            | I::I64Eq
            | I::I64Ne
            | I::I64LtU
            | I::I64LtS
            | I::I64GtU
            | I::I64GtS
            | I::I64LeU
            | I::I64LeS
            | I::I64GeU
            | I::I64GeS
            | I::I32DivS
            | I::I32DivU
            | I::I32RemS
            | I::I32RemU
            | I::I64DivS
            | I::I64DivU
            | I::I64RemS
            | I::I64RemU
            | I::F32Add
            | I::F32Sub
            | I::F32Mul
            | I::F32Div
            | I::F32Eq
            | I::F32Ne
            | I::F32Lt
            | I::F32Gt
            | I::F32Le
            | I::F32Ge
            | I::F64Add
            | I::F64Sub
            | I::F64Mul
            | I::F64Div
            | I::F64Eq
            | I::F64Ne
            | I::F64Lt
            | I::F64Gt
            | I::F64Le
            | I::F64Ge => (2, 1),
            other if is_load(other) => (1, 1),
            other if with_offset(other, 0).is_some() => (2, 0),
            _ => return None,
        },
        _ => return None,
    })
}

/// The `end` of the block that instruction `at` of `b` opens, when it has
/// no `else`.
fn block_end(b: &[Ins], at: usize) -> Option<usize> {
    let mut d = 0u32;
    for (k, i) in b.iter().enumerate().skip(at) {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::Else) if d == 1 => return None,
            Ins::Op(I::End) => {
                d -= 1;
                if d == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether memory instruction `i` loads.
fn is_load(i: &I) -> bool {
    matches!(
        i,
        I::I32Load(_)
            | I::I64Load(_)
            | I::F32Load(_)
            | I::F64Load(_)
            | I::I32Load8S(_)
            | I::I32Load8U(_)
            | I::I32Load16S(_)
            | I::I32Load16U(_)
            | I::I64Load8U(_)
            | I::I64Load16U(_)
            | I::I64Load32U(_)
    )
}

/// Load or store `i` with `by` more on its offset; `None` for any other
/// instruction, or an offset past 32 bits.
fn with_offset(i: &I, by: u32) -> Option<I> {
    let bump = |m: &wasm_encoder::MemArg| {
        let offset = m
            .offset
            .checked_add(u64::from(by))
            .filter(|o| u32::try_from(*o).is_ok())?;
        Some(wasm_encoder::MemArg { offset, ..*m })
    };
    Some(match i {
        I::I32Load(m) => I::I32Load(bump(m)?),
        I::I64Load(m) => I::I64Load(bump(m)?),
        I::F32Load(m) => I::F32Load(bump(m)?),
        I::F64Load(m) => I::F64Load(bump(m)?),
        I::I32Load8S(m) => I::I32Load8S(bump(m)?),
        I::I32Load8U(m) => I::I32Load8U(bump(m)?),
        I::I32Load16S(m) => I::I32Load16S(bump(m)?),
        I::I32Load16U(m) => I::I32Load16U(bump(m)?),
        I::I64Load8U(m) => I::I64Load8U(bump(m)?),
        I::I64Load16U(m) => I::I64Load16U(bump(m)?),
        I::I64Load32U(m) => I::I64Load32U(bump(m)?),
        I::I32Store(m) => I::I32Store(bump(m)?),
        I::I64Store(m) => I::I64Store(bump(m)?),
        I::F32Store(m) => I::F32Store(bump(m)?),
        I::F64Store(m) => I::F64Store(bump(m)?),
        I::I32Store8(m) => I::I32Store8(bump(m)?),
        I::I32Store16(m) => I::I32Store16(bump(m)?),
        I::I64Store8(m) => I::I64Store8(bump(m)?),
        I::I64Store16(m) => I::I64Store16(bump(m)?),
        I::I64Store32(m) => I::I64Store32(bump(m)?),
        _ => return None,
    })
}

/// Rules 5 and 6: rewrites each `if` of those shapes.
fn shapes(f: &mut Func, np: u32) -> bool {
    let mut changed = false;
    while let Some((next, origins)) = shape_once(f, np) {
        f.body = next;
        origins.finish(f);
        changed = true;
    }
    changed
}

/// The body after the first rewrite by rule 5 or 6, if there is one.
fn shape_once(f: &mut Func, np: u32) -> Option<(Vec<Ins>, Remap)> {
    let (reads, writes) = counts(f);
    let b = &f.body;
    for (s, e, n) in ifs(b) {
        let Ins::Op(I::If(wasm_encoder::BlockType::Empty)) = b[s] else {
            continue;
        };
        let Some(e) = e else { continue };
        let then = s + 1..e;
        let other = e + 1..n;
        // Rule 5.
        let lone_br = |r: std::ops::Range<usize>| match &b[r] {
            [Ins::Op(I::Br(d))] if *d >= 1 => Some(*d),
            _ => None,
        };
        for (exit, keep, negate) in [
            (other.clone(), then.clone(), true),
            (then.clone(), other.clone(), false),
        ] {
            if let Some(d) = lone_br(exit.clone())
                && !targets_self(&b[keep.clone()], &f.labels)
            {
                let mut out = b[..s].to_vec();
                if negate {
                    out.push(Ins::Op(I::I32Eqz));
                }
                out.push(Ins::Op(I::BrIf(d - 1)));
                let mut origins = Remap::new(f.origins.as_deref());
                origins.copy(0..s);
                let origin = origins.first(s..s + 1).or_else(|| origins.first(exit));
                origins.repeat(origin, usize::from(negate) + 1);
                origins.copy(keep.clone());
                origins.copy(n + 1..b.len());
                out.extend(lowered(&b[keep], &mut f.labels));
                out.extend_from_slice(&b[n + 1..]);
                return Some((out, origins));
            }
        }
        // Rule 6.
        if let (
            Some(Ins::Op(I::LocalSet(x))),
            Some(Ins::Op(I::LocalSet(y))),
            Some(Ins::Op(I::LocalGet(z))),
        ) = (b.get(e - 1), b.get(n - 1), b.get(n + 1))
            && x == y
            && y == z
            && *x >= np
            && reads[*x as usize] == 1
            && writes[*x as usize] == 2
            && !targets_self(&b[then.clone()], &f.labels)
            && !targets_self(&b[other.clone()], &f.labels)
        {
            let t = f.locals[(*x - np) as usize].wasm();
            let mut out = b[..s].to_vec();
            out.push(Ins::Op(I::If(wasm_encoder::BlockType::Result(t))));
            out.extend_from_slice(&b[s + 1..e - 1]);
            out.push(Ins::Op(I::Else));
            out.extend_from_slice(&b[e + 1..n - 1]);
            out.push(Ins::Op(I::End));
            out.extend_from_slice(&b[n + 2..]);
            let mut origins = Remap::new(f.origins.as_deref());
            origins.copy(0..s);
            let origin = origins
                .first(s..s + 1)
                .or_else(|| origins.first(e - 1..e))
                .or_else(|| origins.first(n - 1..n))
                .or_else(|| origins.first(n + 1..n + 2));
            origins.repeat(origin, 1);
            origins.copy(s + 1..e - 1);
            origins.copy(e..n - 1);
            origins.copy(n..n + 1);
            origins.copy(n + 2..b.len());
            return Some((out, origins));
        }
    }
    None
}

/// Each `if` of `b`: its own index, its `else`'s and its `end`'s.
fn ifs(b: &[Ins]) -> Vec<(usize, Option<usize>, usize)> {
    let mut open: Vec<(usize, bool, Option<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (k, i) in b.iter().enumerate() {
        match i {
            Ins::Op(I::If(_)) => open.push((k, true, None)),
            Ins::Op(I::Block(_) | I::Loop(_)) => open.push((k, false, None)),
            Ins::Op(I::Else) => {
                if let Some(top) = open.last_mut() {
                    top.2 = Some(k);
                }
            }
            Ins::Op(I::End) => {
                if let Some((at, is_if, els)) = open.pop()
                    && is_if
                {
                    out.push((at, els, k));
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether instructions `r`, an arm of an `if`, branch to that `if`.
fn targets_self(r: &[Ins], labels: &[Vec<u32>]) -> bool {
    let mut d = 0u32;
    for i in r {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::End) => d = d.saturating_sub(1),
            Ins::Op(I::Br(l) | I::BrIf(l)) if *l == d => return true,
            Ins::Op(I::BrTable(ls, def)) if *def == d || labels[ls.0 as usize].contains(&d) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// An arm of an `if` moved out of it: each branch past the `if` names one
/// label fewer.
fn lowered(r: &[Ins], labels: &mut Vec<Vec<u32>>) -> Vec<Ins> {
    let mut d = 0u32;
    let mut out = Vec::with_capacity(r.len());
    for i in r {
        let down = |l: u32| if l > d { l - 1 } else { l };
        out.push(match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
                d += 1;
                *i
            }
            Ins::Op(I::End) => {
                d = d.saturating_sub(1);
                *i
            }
            Ins::Op(I::Br(l)) => Ins::Op(I::Br(down(*l))),
            Ins::Op(I::BrIf(l)) => Ins::Op(I::BrIf(down(*l))),
            Ins::Op(I::BrTable(ls, def)) => {
                let lowered = labels[ls.0 as usize].iter().map(|l| down(*l)).collect();
                let id = LabelListId(u32::try_from(labels.len()).expect("branch tables"));
                labels.push(lowered);
                Ins::Op(I::BrTable(id, down(*def)))
            }
            other => *other,
        });
    }
    out
}

/// Whether `i` may change what pure push `p` pushes.
fn clobbers(p: &Ins, i: &Ins) -> bool {
    match (p, i) {
        (Ins::Op(I::LocalGet(a)), Ins::Op(I::LocalSet(b) | I::LocalTee(b))) => a == b,
        (Ins::GlobalGet(g), Ins::GlobalSet(h)) => g == h,
        (Ins::GlobalGet(_), Ins::Call(_) | Ins::CallIndirect(..)) => true,
        _ => false,
    }
}

/// Whether `i` ends a straight-line run: a branch, or a block's edge.
fn ends_run(i: &Ins) -> bool {
    matches!(
        i,
        Ins::Op(
            I::Block(_)
                | I::Loop(_)
                | I::If(_)
                | I::Else
                | I::End
                | I::Br(_)
                | I::BrIf(_)
                | I::BrTable(..)
                | I::Return
                | I::Unreachable
        ) | Ins::SaveAll
            | Ins::LoadAll
    )
}

/// Each local's reads and writes; a `local.tee` is a write.
fn counts(f: &Func) -> (Vec<u32>, Vec<u32>) {
    let n = f.params.len() + f.locals.len();
    let (mut reads, mut writes) = (vec![0u32; n], vec![0u32; n]);
    for i in &f.body {
        match i {
            Ins::Op(I::LocalGet(l)) => reads[*l as usize] += 1,
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => writes[*l as usize] += 1,
            _ => {}
        }
    }
    (reads, writes)
}

/// Drops the locals past the parameters that no instruction names, and
/// numbers the others again in order.
pub(crate) fn renumber(f: &mut Func) {
    let np = f.params.len();
    let mut used = vec![false; f.locals.len()];
    for i in &f.body {
        if let Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) = i
            && let Some(at) = (*l as usize).checked_sub(np)
        {
            used[at] = true;
        }
    }
    let mut map = Vec::with_capacity(f.locals.len());
    let mut kept = Vec::new();
    for (j, v) in f.locals.iter().enumerate() {
        map.push(u32::try_from(np + kept.len()).expect("locals"));
        if used[j] {
            kept.push(*v);
        }
    }
    let to = |l: u32| -> u32 {
        match (l as usize).checked_sub(np) {
            Some(at) => map[at],
            None => l,
        }
    };
    for i in &mut f.body {
        match i {
            Ins::Op(I::LocalGet(l)) => *l = to(*l),
            Ins::Op(I::LocalSet(l)) => *l = to(*l),
            Ins::Op(I::LocalTee(l)) => *l = to(*l),
            _ => {}
        }
    }
    if let Some(names) = &mut f.local_names {
        *names = std::mem::take(names)
            .into_iter()
            .filter(|(l, _)| (*l as usize).checked_sub(np).is_none_or(|at| used[at]))
            .map(|(l, n)| (to(l), n))
            .collect();
    }
    f.locals = kept;
}

/// Cancels balanced count changes on unchanged locals in a short straight
/// line. Calls other than retains, stores to an operand local, and control
/// flow end the proof: any of them may invalidate the original owner.
pub fn count_pairs(f: &mut Func, retain: Hash128) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Count {
        Ref,
        KnownRef,
        Value(SymId),
    }
    fn count(ins: &Ins, retain: Hash128, symbols: &Symbols) -> Option<(bool, Count, usize)> {
        let Ins::Call(id) = ins else {
            return None;
        };
        match &*symbols.get(*id) {
            Sym::Inst(k) if *k == retain => Some((true, Count::Ref, 1)),
            Sym::Helper(Helper::ReleaseRef) => Some((false, Count::Ref, 1)),
            Sym::Helper(Helper::RetainKnown) => Some((true, Count::KnownRef, 1)),
            Sym::Helper(Helper::ReleaseKnown) => Some((false, Count::KnownRef, 1)),
            Sym::Helper(Helper::Retain { vals, .. }) => Some((true, Count::Value(*id), vals.len())),
            Sym::Helper(Helper::Release { vals, .. }) => {
                Some((false, Count::Value(*id), vals.len()))
            }
            _ => None,
        }
    }
    fn same_count(a: Count, b: Count, symbols: &Symbols) -> bool {
        match (a, b) {
            (Count::Value(a), Count::Value(b)) => {
                let a = symbols.get(a);
                let b = symbols.get(b);
                match (&*a, &*b) {
                    (
                        Sym::Helper(
                            Helper::Retain { vals: av, walk: aw }
                            | Helper::Release { vals: av, walk: aw },
                        ),
                        Sym::Helper(
                            Helper::Retain { vals: bv, walk: bw }
                            | Helper::Release { vals: bv, walk: bw },
                        ),
                    ) => av == bv && aw == bw,
                    _ => false,
                }
            }
            _ => a == b,
        }
    }
    struct Arguments {
        // A tee chain leaves the same value in every named local. Keep
        // those assignments: removing the consuming count call changes
        // only its final tee into a set, consuming the producer's value.
        locals: Vec<Vec<u32>>,
        gets: Vec<usize>,
        tee: Option<usize>,
    }
    fn arguments(body: &[Ins], at: usize, n: usize) -> Option<Arguments> {
        let mut locals = Vec::new();
        let mut gets = Vec::new();
        for (i, ins) in body[..at].iter().enumerate().rev() {
            match ins {
                Ins::Anchor(_) => continue,
                Ins::Op(I::LocalGet(l)) => {
                    locals.push(vec![*l]);
                    gets.push(i);
                }
                // The single argument's producer can be arbitrary; the
                // tees themselves prove its aliases without looking
                // through the producer or any other call.
                Ins::Op(I::LocalTee(_)) if n == 1 => {
                    let mut aliases = Vec::new();
                    for ins in body[..=i].iter().rev() {
                        match ins {
                            Ins::Anchor(_) => {}
                            Ins::Op(I::LocalTee(l)) => aliases.push(*l),
                            Ins::Op(I::LocalGet(l)) => {
                                aliases.push(*l);
                                break;
                            }
                            _ => break,
                        }
                    }
                    return Some(Arguments {
                        locals: vec![aliases],
                        gets: Vec::new(),
                        tee: Some(i),
                    });
                }
                _ => return None,
            }
            if locals.len() == n {
                locals.reverse();
                return Some(Arguments {
                    locals,
                    gets,
                    tee: None,
                });
            }
        }
        None
    }
    let mut gone = std::collections::BTreeSet::new();
    let mut sets = std::collections::BTreeSet::new();
    for (i, ins) in f.body.iter().enumerate() {
        if gone.contains(&i) {
            continue;
        }
        let Some((true, key, n)) = count(ins, retain, &f.symbols) else {
            continue;
        };
        let Some(args) = arguments(&f.body, i, n) else {
            continue;
        };
        for (j, next) in f.body.iter().enumerate().skip(i + 1).take(128) {
            if gone.contains(&j) {
                continue;
            }
            if let Some((adding, other, width)) = count(next, retain, &f.symbols) {
                if adding {
                    continue;
                }
                if same_count(other, key, &f.symbols)
                    && width == n
                    && let Some(end) = arguments(&f.body, j, n)
                    && args
                        .locals
                        .iter()
                        .zip(&end.locals)
                        .all(|(a, b)| a.iter().any(|l| b.contains(l)))
                {
                    gone.extend([i, j]);
                    gone.extend(args.gets.iter().chain(&end.gets).copied());
                    sets.extend(args.tee.into_iter().chain(end.tee));
                }
                break;
            }
            match next {
                Ins::Anchor(_)
                | Ins::Op(
                    I::LocalGet(_)
                    | I::I32Const(_)
                    | I::I64Const(_)
                    | I::F32Const(_)
                    | I::F64Const(_)
                    | I::Drop
                    | I::I32Add
                    | I::I32Sub
                    | I::I32Mul
                    | I::I32Eq
                    | I::I32Ne
                    | I::I32Eqz
                    | I::I32And
                    | I::I32Or
                    | I::I32Xor,
                ) => {}
                Ins::Op(I::LocalSet(l) | I::LocalTee(l))
                    if !args.locals.iter().any(|a| a.contains(l)) => {}
                _ => break,
            }
        }
    }
    if gone.is_empty() {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    f.body = std::mem::take(&mut f.body)
        .into_iter()
        .enumerate()
        .filter_map(|(i, ins)| {
            if gone.contains(&i) {
                None
            } else {
                origins.copy(i..i + 1);
                Some(match ins {
                    Ins::Op(I::LocalTee(l)) if sets.contains(&i) => Ins::Op(I::LocalSet(l)),
                    _ => ins,
                })
            }
        })
        .collect();
    origins.finish(f);
    true
}

/// Folds code that a constant decides (#655): an `if` whose condition is
/// an `i32` constant becomes the arm that runs (or goes, with no `else`
/// and no result), and an `i32` local whose every write stores 0 is read
/// as 0, so the flag of a branch folded away goes with it. An arm that
/// branches to the `if` itself stays in a `block`, which keeps the
/// label depth of the `if`; any other arm takes the `if`'s place, each
/// branch past it naming one label fewer, so the straight-line runs the
/// local rules work on continue through it. Gives whether anything
/// changed.
pub fn fold_constant_branches(f: &mut Func) -> bool {
    use crate::module::Val;
    use wasm_encoder::BlockType;
    let mut changed = false;
    loop {
        let mut again = false;
        // Constant conditions.
        let mut i = 0;
        while i + 1 < f.body.len() {
            let (Ins::Op(I::I32Const(c)), Ins::Op(I::If(bt))) = (&f.body[i], &f.body[i + 1]) else {
                i += 1;
                continue;
            };
            let (c, bt) = (*c, *bt);
            // The `if`'s `else` (if any) and `end`.
            let (mut depth, mut els, mut end) = (0usize, None, None);
            for (k, ins) in f.body.iter().enumerate().skip(i + 2) {
                match ins {
                    Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => depth += 1,
                    Ins::Op(I::Else) if depth == 0 => els = Some(k),
                    Ins::Op(I::End) if depth == 0 => {
                        end = Some(k);
                        break;
                    }
                    Ins::Op(I::End) => depth -= 1,
                    _ => {}
                }
            }
            let Some(end) = end else {
                i += 1;
                continue;
            };
            let then = i + 2..els.unwrap_or(end);
            let other = els.map_or(end..end, |e| e + 1..end);
            let selected = if c != 0 { then } else { other };
            let arm: Vec<Ins> = f.body[selected.clone()].to_vec();
            let mut origins = Remap::new(f.origins.as_deref());
            origins.copy(0..i);
            let mut out: Vec<Ins> = Vec::new();
            if !targets_self(&arm, &f.labels) {
                out = lowered(&arm, &mut f.labels);
                origins.copy(selected);
            } else if !(arm.is_empty() && matches!(bt, BlockType::Empty)) {
                out.push(Ins::Op(I::Block(bt)));
                out.extend(arm);
                out.push(Ins::Op(I::End));
                origins.merged(i..i + 2, 1);
                origins.copy(selected);
                origins.copy(end..end + 1);
            }
            origins.copy(end + 1..f.body.len());
            f.body.splice(i..=end, out);
            origins.finish(f);
            again = true;
        }
        // Locals that only ever hold 0.
        let np = f.params.len();
        let mut zero: Vec<bool> = f.locals.iter().map(|v| *v == Val::I32).collect();
        for (k, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
                && let Some(at) = (*l as usize).checked_sub(np)
                && !(k > 0 && matches!(f.body[k - 1], Ins::Op(I::I32Const(0))))
            {
                zero[at] = false;
            }
        }
        let np32 = u32::try_from(np).expect("params");
        if zero.iter().any(|z| *z)
            && f.body.iter().any(|ins| {
                matches!(ins, Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l))
                    if *l >= np32 && zero[(*l - np32) as usize])
            })
        {
            let mut origins = Remap::new(f.origins.as_deref());
            let body = std::mem::take(&mut f.body);
            f.origins.as_mut().map(Vec::clear);
            for (position, ins) in body.into_iter().enumerate() {
                match ins {
                    Ins::Op(I::LocalGet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.op(I::I32Const(0));
                        origins.copy(position..position + 1);
                    }
                    // Every write stores a constant 0 just pushed: a set
                    // takes it back, and a tee leaves it.
                    Ins::Op(I::LocalSet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.body.pop();
                        if let Some(spans) = &mut f.origins {
                            spans.pop();
                        }
                        origins.pop();
                    }
                    Ins::Op(I::LocalTee(l)) if l >= np32 && zero[(l - np32) as usize] => {}
                    other => {
                        f.push(other);
                        origins.copy(position..position + 1);
                    }
                }
            }
            origins.finish(f);
            again = true;
        }
        if !again {
            return changed;
        }
        changed = true;
    }
}

/// Inlines the fast paths of the counts on known block starts
/// ([`Helper::RetainKnown`], [`Helper::ReleaseKnown`]; §23.30): a retain
/// adds one to the header's count unless the address is null or a static
/// object of count 0, and a release that leaves a count above zero takes
/// one off. A last count, an object that takes part in cycle collection
/// (so its decrement reaches `suspect`), or a release while the collector
/// traces (`Tracing`) still calls the helper, which does all the rest.
/// `acyclic` drops the two collector tests, as no object takes part.
/// Gives whether anything changed.
pub fn inline_counts(f: &mut Func, acyclic: bool) -> bool {
    use crate::module::{HEAP, Val, mem};
    use wasm_encoder::BlockType;
    if !f.body.iter().any(|i| {
        matches!(i, Ins::Call(id) if matches!(&*f.symbol(*id), Sym::Helper(Helper::RetainKnown | Helper::ReleaseKnown)))
    }) {
        return false;
    }
    let (t, b, c) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    // Leaves the header's address in `b` and its count in `c`, branching
    // out of the enclosing block for null.
    let header = |f: &mut Func| {
        f.set(t);
        f.op(I::Block(BlockType::Empty));
        f.get(t);
        f.op(I::I32Eqz);
        f.op(I::BrIf(0));
        f.get(t);
        f.i32(8);
        f.op(I::I32Sub);
        f.op(I::LocalTee(b));
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.op(I::LocalTee(c));
    };
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        let known = match ins {
            Ins::Call(id) => match &*f.symbol(id) {
                Sym::Helper(Helper::RetainKnown) => Some(true),
                Sym::Helper(Helper::ReleaseKnown) => Some(false),
                _ => None,
            },
            _ => None,
        };
        match known {
            Some(true) => {
                header(f);
                f.op(I::I32Eqz);
                f.op(I::BrIf(0));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Add);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::End);
            }
            Some(false) => {
                header(f);
                f.i32(1);
                f.op(I::I32GtU);
                if !acyclic {
                    f.global_get(Global::Tracing);
                    f.op(I::I32Eqz);
                    f.op(I::I32And);
                    f.get(b);
                    f.op(I::I32Load(mem(HEAP, 4, 2)));
                    f.i32(0);
                    f.op(I::I32GeS);
                    f.op(I::I32And);
                }
                f.op(I::If(BlockType::Empty));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Sub);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::Else);
                // A count of 0 is a static object's, except while the
                // collector traces: a gray object's count may have reached
                // 0, and `visit` must still see the reference.
                f.get(c);
                if !acyclic {
                    f.global_get(Global::Tracing);
                    f.op(I::I32Or);
                }
                f.op(I::If(BlockType::Empty));
                f.get(t);
                f.call(Sym::Helper(Helper::ReleaseKnown));
                f.op(I::End);
                f.op(I::End);
                f.op(I::End);
            }
            None => f.push(ins),
        }
    }
    f.origin_at = outer;
    true
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    use crate::module::{HEAP, Val, mem};
    use wasm_encoder::BlockType;

    fn call(symbols: &Symbols, symbol: Sym) -> Ins {
        Ins::Call(symbols.intern(symbol))
    }

    fn tracked(symbols: &Symbols, params: usize, locals: usize, body: Vec<Ins>) -> Func {
        let mut f =
            crate::module::Module::default().func("origins", vec![Val::I32; params], vec![]);
        f.symbols = symbols.clone();
        f.locals = vec![Val::I32; locals];
        f.origins = Some(
            (0..body.len())
                .map(|i| Some(10 + u32::try_from(i).unwrap()))
                .collect(),
        );
        f.body = body;
        f
    }

    fn run(mut f: Func, pass: impl Fn(&mut Func)) -> Func {
        let mut plain = f.clone();
        plain.origins = None;
        pass(&mut plain);
        pass(&mut f);
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        assert_eq!(f.origins.as_ref().unwrap().len(), f.body.len());
        f
    }

    fn spans(f: &Func) -> Vec<u32> {
        f.origins
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.expect("retained source"))
            .collect()
    }

    #[test]
    fn origins_follow_local_cleanup_forwarding_and_scheduling() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![op(I::LocalGet(0)), op(I::LocalSet(1)), op(I::LocalGet(1))],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalSet(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert!(spans(&f).is_empty());
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalTee(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert!(
            f.body
                .iter()
                .zip(f.origins.as_ref().unwrap())
                .any(|(i, s)| matches!(i, Ins::Op(I::LocalTee(_))) && *s == Some(14))
        );
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::I32Const(2)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(forward(f, 1));
            },
        );
        assert_eq!(spans(&f), [12, 13, 10]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                ],
            ),
            |f| {
                assert!(schedule(f, 1, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [10, 13, 11]);
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(2)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        // Rule 10 takes precedence here, moving the second value before its producer.
        assert_eq!(spans(&f), [10, 11, 14, 12, 16]);
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![
                    op(I::I32Const(3)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(5)),
                    op(I::LocalGet(0)),
                    op(I::I32Sub),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [12, 10, 14]);
    }

    #[test]
    fn origins_follow_folds_and_both_control_shapes() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![op(I::LocalGet(0)), op(I::I32Const(0)), op(I::I32Eq)],
        );
        f.origins.as_mut().unwrap()[1] = None;
        let f = run(f, |f| {
            assert!(fold(f));
        });
        assert_eq!(spans(&f), [10, 12]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::Br(1)),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(shapes(f, 1));
            },
        );
        assert_eq!(spans(&f), [10, 11, 11, 12, 13]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::LocalSet(1)),
                    op(I::Else),
                    op(I::I32Const(8)),
                    op(I::LocalSet(1)),
                    op(I::End),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(shapes(f, 1));
            },
        );
        assert_eq!(spans(&f), [10, 11, 12, 14, 15, 17]);
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::I32Const(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(10)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::I32Const(20)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [15, 16]);
        // An arm takes its `if`'s place, one label fewer past it, unless
        // it branches to the `if` itself.
        for (target, kept) in [(1, false), (0, true)] {
            let mut f = tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::I32Const(1)),
                    op(I::If(BlockType::Empty)),
                    op(I::Br(target)),
                    op(I::End),
                    op(I::End),
                ],
            );
            assert!(fold_constant_branches(&mut f));
            let body: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
            let want = if kept {
                ["Block", "Block", "Br(0)", "End", "End"].as_slice()
            } else {
                ["Block", "Br(0)", "End"].as_slice()
            };
            assert_eq!(body.len(), want.len(), "{body:?}");
            assert!(
                body.iter().zip(want).all(|(b, w)| b.contains(w)),
                "{body:?}"
            );
        }
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![op(I::I32Const(0)), op(I::LocalSet(0)), op(I::LocalGet(0))],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [12]);
    }

    #[test]
    fn origins_follow_count_deletion_and_fast_path_expansion() {
        let symbols = Symbols::default();
        let retain = Hash128(7);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    call(&symbols, Sym::Inst(retain)),
                    Ins::Op(I::LocalGet(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(count_pairs(f, retain));
            },
        );
        assert!(spans(&f).is_empty());
        for acyclic in [false, true] {
            let f = run(
                tracked(
                    &symbols,
                    1,
                    0,
                    vec![
                        Ins::Op(I::LocalGet(0)),
                        call(&symbols, Sym::Helper(Helper::RetainKnown)),
                        Ins::Op(I::LocalGet(0)),
                        call(&symbols, Sym::Helper(Helper::ReleaseKnown)),
                    ],
                ),
                |f| {
                    assert!(inline_counts(f, acyclic));
                },
            );
            let s = spans(&f);
            assert!(s.contains(&11) && s.contains(&13));
            assert!(s.iter().all(|i| (10..=13).contains(i)));
        }
    }
    #[test]
    fn count_pairs_keep_tee_assignments_and_their_origins() {
        let symbols = Symbols::default();
        let retain = Hash128(7);
        // The producer's call and both alias definitions must survive.
        // Cancellation may use either alias, or the source of a tee.
        for source in [
            call(&symbols, Sym::Inst(Hash128(8))),
            Ins::Op(I::LocalGet(0)),
        ] {
            for released in 1..=2 {
                let f = run(
                    tracked(
                        &symbols,
                        1,
                        2,
                        vec![
                            source,
                            Ins::Op(I::LocalTee(1)),
                            Ins::Anchor(0),
                            Ins::Op(I::LocalTee(2)),
                            call(&symbols, Sym::Inst(retain)),
                            Ins::Op(I::LocalGet(released)),
                            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                            Ins::Op(I::LocalGet(1)),
                            Ins::Op(I::LocalGet(2)),
                        ],
                    ),
                    |f| {
                        assert!(count_pairs(f, retain));
                    },
                );
                assert!(matches!(f.body[0], Ins::Call(_) | Ins::Op(I::LocalGet(0))));
                assert!(matches!(f.body[1], Ins::Op(I::LocalTee(1))));
                assert!(matches!(f.body[3], Ins::Op(I::LocalSet(2))));
                assert_eq!(spans(&f), [10, 11, 12, 13, 17, 18]);
            }
        }
        // A release may also define an alias; keep that assignment.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    call(&symbols, Sym::Helper(Helper::RetainKnown)),
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::LocalTee(1)),
                    call(&symbols, Sym::Helper(Helper::ReleaseKnown)),
                    Ins::Op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(count_pairs(f, retain));
            },
        );
        assert!(matches!(f.body[1], Ins::Op(I::LocalSet(1))));
        assert_eq!(spans(&f), [12, 13, 15]);
    }

    #[test]
    fn tee_count_pairs_stop_at_overwrites_calls_and_control_flow() {
        let symbols = Symbols::default();
        let retain = Hash128(7);
        for barrier in [
            vec![Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))],
            vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalTee(2)),
                Ins::Op(I::Drop),
            ],
            vec![call(&symbols, Sym::Inst(Hash128(8)))],
            vec![Ins::Op(I::Block(BlockType::Empty)), Ins::Op(I::End)],
        ] {
            let mut body = vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalTee(1)),
                Ins::Op(I::LocalTee(2)),
                call(&symbols, Sym::Inst(retain)),
            ];
            body.extend(barrier);
            body.extend([
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ]);
            run(tracked(&symbols, 1, 2, body), |f| {
                assert!(!count_pairs(f, retain));
            });
        }
    }

    fn releases(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| {
                matches!(i, Ins::Call(id) if matches!(&*f.symbol(*id),
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown | Helper::Release { .. })))
            })
            .count()
    }

    #[test]
    fn zero_facts_remove_initial_and_moved_slots_but_keep_live_values() {
        let symbols = Symbols::default();
        let f = tracked(
            &symbols,
            1,
            2,
            vec![
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseKnown)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f);
        });
        assert_eq!(releases(&f), 2);
        assert_eq!(
            f.body
                .iter()
                .filter(|i| matches!(i, Ins::Op(I::LocalSet(2))))
                .count(),
            2
        );
    }

    #[test]
    fn zero_facts_meet_both_if_arms_and_exclude_returned_paths() {
        let symbols = Symbols::default();
        let branch = |live: bool| {
            tracked(
                &symbols,
                2,
                1,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(if live { I::LocalGet(1) } else { I::I32Const(0) }),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(2)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            )
        };
        assert_eq!(
            releases(&run(branch(false), |f| {
                zero_releases(f);
            })),
            0
        );
        assert_eq!(
            releases(&run(branch(true), |f| {
                zero_releases(f);
            })),
            1
        );
        let mut exits = branch(true);
        exits.body.insert(4, Ins::Op(I::Return));
        exits.origins = Some(vec![Some(17); exits.body.len()]);
        assert_eq!(
            releases(&run(exits, |f| {
                zero_releases(f);
            })),
            0
        );
    }

    #[test]
    fn zero_facts_include_loop_backedges_and_branch_table_exits() {
        let symbols = Symbols::default();
        let loop_body = |reset: bool| {
            let mut body = vec![
                Ins::Op(I::Loop(BlockType::Empty)),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalSet(1)),
            ];
            if reset {
                body.extend([Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]);
            }
            body.extend([
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrIf(0)),
                Ins::Op(I::End),
            ]);
            tracked(&symbols, 1, 1, body)
        };
        assert_eq!(
            releases(&run(loop_body(false), |f| {
                zero_releases(f);
            })),
            1
        );
        assert_eq!(
            releases(&run(loop_body(true), |f| {
                zero_releases(f);
            })),
            0
        );
        let mut table = tracked(
            &symbols,
            1,
            1,
            vec![
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrTable(LabelListId(0), 1)),
                Ins::Op(I::End),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(1)),
                Ins::Op(I::End),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        table.labels.push(vec![0]);
        assert_eq!(
            releases(&run(table, |f| {
                zero_releases(f);
            })),
            1
        );
    }

    #[test]
    fn dead_stores_go_unless_a_release_reads_them_on_some_path() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        let zeros = |f: &Func| {
            f.body
                .windows(2)
                .filter(|w| matches!(w, [Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]))
                .count()
        };
        // A loop that moves local 1 out and zeroes it: the zero stays when
        // the release after the loop, or the release at the loop head on
        // the backedge, reads it, and goes when the next iteration
        // overwrites it first.
        let body = |head_release: bool, exit_release: bool| {
            let mut b = vec![op(I::Loop(BlockType::Empty))];
            if head_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            b.extend([
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(1)),
                op(I::Drop),
                op(I::I32Const(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ]);
            if exit_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            tracked(&symbols, 1, 1, b)
        };
        for (head, exit, kept) in [(false, false, 0), (true, false, 1), (false, true, 1)] {
            let f = run(body(head, exit), |f| {
                live_rules(f, &Arity::new());
            });
            assert_eq!(zeros(&f), kept, "{:?}", f.body);
        }
        // A suspension saves every local, so a store before it stays; a
        // dead set of a call's result keeps the call and drops its value.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![op(I::I32Const(0)), op(I::LocalSet(1)), Ins::SaveAll],
            ),
            |f| {
                assert!(!live_rules(f, &Arity::new()));
            },
        );
        assert_eq!(zeros(&f), 1);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    release,
                    op(I::I32Const(5)),
                    op(I::LocalSet(1)),
                ],
            ),
            |f| {
                assert!(live_rules(f, &Arity::new()));
            },
        );
        assert_eq!(f.body.len(), 2);
        assert_eq!(spans(&f), [10, 11]);
    }

    #[test]
    fn single_uses_stay_on_the_stack_by_liveness() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let guard = [
            op(I::If(BlockType::Empty)),
            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            op(I::Unreachable),
            op(I::End),
        ];
        // Local 2 is written twice, so the count rules leave it; each
        // write has one reader, and the loop head writes it again.
        let mut body = vec![op(I::Loop(BlockType::Empty))];
        body.extend([op(I::LocalGet(0)), op(I::LocalSet(2))]);
        body.extend([op(I::LocalGet(1)), op(I::I32Eqz)]);
        body.extend(guard);
        body.extend([op(I::LocalGet(2)), op(I::Drop)]);
        body.extend([op(I::LocalGet(1)), op(I::I32Const(1)), op(I::I32Add)]);
        body.extend([op(I::LocalSet(2)), op(I::LocalGet(1)), op(I::LocalSet(3))]);
        body.extend([
            op(I::LocalGet(2)),
            op(I::LocalGet(3)),
            op(I::BrIf(0)),
            op(I::Drop),
        ]);
        body.push(op(I::End));
        let f = run(tracked(&symbols, 2, 2, body), |f| {
            while live_rules(f, &Arity::new()) {}
        });
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        // The copy of 0 moves past the guard to its read, the copy of 1
        // moves to its read, and the sum stays on the stack under it.
        assert!(
            !shown
                .iter()
                .any(|i| i.contains("Local") && i.contains("(2)")),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("(3)")), "{shown:?}");
        // A loop test evaluated before the loop and again at its head:
        // each value stays on the stack, written by a tee for the head.
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(0)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::Drop),
                    op(I::Loop(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::BrIf(0)),
                    op(I::End),
                ],
            ),
            |f| {
                while live_rules(f, &Arity::new()) {}
            },
        );
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        assert_eq!(
            shown.iter().filter(|i| i.contains("LocalTee")).count(),
            2,
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("LocalSet")), "{shown:?}");
        // A run that consumes the value keeps its local.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(!live_rules(f, &Arity::new()));
            },
        );
        assert_eq!(f.body.len(), 5);
    }

    #[test]
    fn coalesced_locals_never_hold_two_live_values() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        // Locals 1 and 2 live one after the other: one local. Local 3
        // overlaps local 4, and local 5 is read before any write, so it
        // keeps its own zero while 4 is live.
        let body = vec![
            op(I::LocalGet(0)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::I32Eqz),
            op(I::LocalSet(2)),
            op(I::LocalGet(2)),
            release,
            op(I::I32Const(7)),
            op(I::LocalSet(3)),
            op(I::I32Const(8)),
            op(I::LocalSet(4)),
            op(I::LocalGet(3)),
            op(I::LocalGet(4)),
            op(I::I32Add),
            release,
            op(I::LocalGet(5)),
            release,
        ];
        let mut f = run(tracked(&symbols, 1, 5, body), coalesce);
        let named: Vec<u32> = f
            .body
            .iter()
            .filter_map(|i| match i {
                Ins::Op(I::LocalGet(l) | I::LocalSet(l)) => Some(*l),
                _ => None,
            })
            .collect();
        // 1 and 2 take the parameter's local, whose value is dead after
        // its last read; 3 joins them too. 4 and 5 stay apart.
        assert_eq!(named, [0, 0, 0, 0, 0, 1, 0, 1, 2], "{:?}", f.body);
        assert_eq!(f.locals.len(), 2);
        // A copy whose source keeps its value shares the source's local
        // and goes.
        f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    release,
                    op(I::LocalGet(1)),
                    release,
                ],
            ),
            coalesce,
        );
        assert_eq!(f.body.len(), 4);
        assert!(f.locals.is_empty());
    }

    #[test]
    fn zero_facts_without_locals_distinguish_reachable_and_unreachable_releases() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::Return),
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f));
            },
        );
        assert!(matches!(
            f.body.as_slice(),
            [Ins::Op(I::Return), Ins::Op(I::I32Const(0)), Ins::Call(_)]
        ));
        assert_eq!(releases(&f), 1);
    }

    #[test]
    fn zero_facts_meet_multiple_words_without_losing_other_high_locals() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                1,
                129,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(65)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::LocalGet(129)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f));
            },
        );
        assert_eq!(releases(&f), 1);
        assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::LocalGet(65)))));
        assert!(
            !f.body
                .iter()
                .any(|i| matches!(i, Ins::Op(I::LocalGet(129))))
        );
    }

    #[test]
    fn zero_facts_respect_frame_loads_and_preserve_source_origins() {
        let symbols = Symbols::default();
        let loaded = tracked(
            &symbols,
            0,
            1,
            vec![
                Ins::LoadAll,
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        assert_eq!(
            releases(&run(loaded, |f| {
                zero_releases(f);
            })),
            1
        );
        let f = tracked(
            &symbols,
            1,
            0,
            vec![
                Ins::Op(I::I32Const(0)),
                Ins::Anchor(99),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f);
        });
        assert_eq!(spans(&f), [11, 13, 14]);
        let value = tracked(
            &symbols,
            0,
            2,
            vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalGet(1)),
                call(
                    &symbols,
                    Sym::Helper(Helper::Release {
                        vals: vec![Val::I32, Val::I32],
                        walk: crate::layout::Walk::Switch(
                            0,
                            vec![None, Some(crate::layout::Walk::Ref(1))],
                        ),
                    }),
                ),
            ],
        );
        assert_eq!(
            releases(&run(value, |f| {
                zero_releases(f);
            })),
            0
        );
    }
}
