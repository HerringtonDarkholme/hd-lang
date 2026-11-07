//! What a suspension frame holds at each point
//! (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`, step 26, #1393): per
//! suspension point, the locals and values that a resume there may read,
//! so the frame saves and reloads only those. Written last, over the TIR
//! the emitter lowers.
//!
//! A backward liveness walk over the structured control flow ([`backward`]):
//! an operand reads its value, a `Drop`'s place included; a place reads
//! its root local, but a store into all of it writes it; a closure reads
//! its captures; a point reads its own operands and unwind rows, and a
//! resume reads the operands of each instruction around it again. The walk
//! follows no way out it has no jump for, so what those read is live at
//! every point: the exit set and what a cleanup suite touches. A cancelled
//! cleanup scope's ladder releases the unwind rows of every point inside
//! it, so those are live at each point that shares the scope. A
//! point the walk never reaches, one in a cleanup suite, has no row, and
//! its frame saves everything.

use super::flow::{Backward, BitArena, Flow, backward};
use super::{Operand, Role, operands, suite_touches};
use crate::ir::{Body, NONE, Op, Tag, Topology, is_suspension, place, u32_of};
use hd_base::SliceSort as _;

struct Live {
    /// Bits `0..locals` are locals, then one per instruction value.
    locals: usize,
    /// Each point's row, by instruction, once the walk has passed it.
    rows: Vec<Option<Vec<u32>>>,
    ops: Vec<Operand>,
}

impl Live {
    fn value(&self, v: u32) -> u32 {
        u32_of(self.locals) + v
    }
}

impl Backward for Live {
    fn stmt(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        let i = x as usize;
        s.set(live, self.value(x), false);
        match b.tags[i] {
            Tag::Store => {
                let p = b.view(b.data[i][0]);
                if let Some(l) = p.local() {
                    s.set(live, l, !p.whole());
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                for c in rec[0]..rec[0] + rec[1] {
                    s.set(live, b.cap_local[c as usize].raw(), true);
                }
            }
            tag => {
                let (a, w) = tag.operands();
                for (k, op) in [a, w].into_iter().enumerate() {
                    if op == Op::Place
                        && let Some(l) = b.view(b.data[i][k]).local()
                    {
                        s.set(live, l, true);
                    }
                }
            }
        }
    }

    fn uses(&mut self, _: &Body, v: u32, s: &mut BitArena, live: usize) {
        s.set(live, self.value(v), true);
    }

    fn reads(&mut self, b: &Body, x: u32, s: &mut BitArena, live: usize) {
        // The walk leaves a `Drop`'s place out of its uses.
        operands(b, x as usize, &mut self.ops);
        for k in 0..self.ops.len() {
            if self.ops[k].role == Role::Dropped {
                s.set(live, self.value(self.ops[k].value), true);
            }
        }
        if !is_suspension(b.tags[x as usize]) {
            return;
        }
        let at = b.unwind_drops.partition_point(|r| r.0 < x);
        for &(_, v) in b.unwind_drops[at..].iter().take_while(|r| r.0 == x) {
            s.set(live, self.value(v), true);
        }
        let row = s.row(live);
        let words = (0..row.len() * 64)
            .filter(|&k| row[k / 64] & (1 << (k % 64)) != 0)
            .map(|k| {
                if k < self.locals {
                    place::BARE_LOCAL | u32_of(k)
                } else {
                    u32_of(k - self.locals)
                }
            })
            .collect();
        self.rows[x as usize] = Some(words);
    }
}

/// Writes `b.live`.
pub(crate) fn run(b: &mut Body, topo: &Topology, flow: &mut Flow) {
    b.live.clear();
    let n = b.len();
    if !b.tags.iter().any(|&t| is_suspension(t)) {
        return;
    }
    let locals = b.local_ty.len();
    let mut a = Live {
        locals,
        rows: vec![None; n],
        ops: Vec::new(),
    };
    flow.arena.words = (locals + n).div_ceil(64).max(1);
    backward(b, topo, flow, &mut a);
    // What the ways out the walk does not follow read.
    let mut always = vec![false; locals + n];
    // Each point's cleanup scopes.
    let scopes = |x: u32| {
        topo.enclosing(x)
            .map(|blk| topo.owner[blk as usize])
            .filter(|&o| o != NONE && b.tags[o as usize] == Tag::Scope)
    };
    let points: Vec<(u32, Vec<u32>)> = (0..n)
        .filter(|&x| is_suspension(b.tags[x]))
        .map(|x| (u32_of(x), scopes(u32_of(x)).collect()))
        .collect();
    let mut stack = Vec::new();
    let mut ops = Vec::new();
    let mut suites: Vec<u32> = Vec::new();
    for i in 0..n {
        let [p, w] = b.data[i];
        match b.tags[i] {
            Tag::Scope if w != NONE => suites.extend_from_slice(b.record(w)),
            Tag::Defer => suites.push(p),
            _ => {}
        }
    }
    let (mut touched_l, mut touched_v) = (Vec::new(), Vec::new());
    suite_touches(
        b,
        &suites,
        &mut stack,
        &mut ops,
        false,
        |l| touched_l.push(l),
        |v| touched_v.push(v),
    );
    for l in touched_l {
        always[l as usize] = true;
    }
    for v in touched_v {
        always[locals + v as usize] = true;
    }
    for x in 0..n {
        let Some(mut row) = a.rows[x].take() else {
            continue;
        };
        let sub = topo.sub[x];
        let exits = b
            .exit_drops
            .iter()
            .filter(|(s, _)| s.raw() == sub)
            .map(|(_, l)| place::BARE_LOCAL | l.raw());
        let fixed = (0..always.len())
            .filter(|&k| always[k] && (k < locals || topo.sub[k - locals] == sub))
            .map(|k| {
                if k < locals {
                    place::BARE_LOCAL | u32_of(k)
                } else {
                    u32_of(k - locals)
                }
            });
        row.extend(exits.chain(fixed));
        // A ladder of a cleanup scope around the point releases the unwind
        // rows of every point inside it.
        let around: Vec<u32> = scopes(u32_of(x)).collect();
        for (p, theirs) in &points {
            if theirs.iter().any(|o| around.contains(o)) {
                let at = b.unwind_drops.partition_point(|r| r.0 < *p);
                row.extend(
                    b.unwind_drops[at..]
                        .iter()
                        .take_while(|r| r.0 == *p)
                        .map(|r| r.1),
                );
            }
        }
        // A resume enters each instruction around the point again, which
        // reads its operands before it enters the block that holds it.
        for blk in topo.enclosing(u32_of(x)) {
            let owner = topo.owner[blk as usize];
            if owner != NONE {
                operands(b, owner as usize, &mut ops);
                row.extend(ops.iter().map(|o| o.value));
            }
        }
        row.sort_stable();
        row.dedup();
        b.live.push((u32_of(x), row));
    }
}
