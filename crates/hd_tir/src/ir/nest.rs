//! A body's dominance and loop nesting from its structured blocks (#2175).
//! A block is entered only at its start, so a statement dominates its
//! block's later statements, and the dominator tree is the block tree in
//! statement order: one preorder numbering, each subtree an interval, as
//! Go's `SparseTree` entry and exit numbers (`ssa/sparsetree.go`). The
//! loop forest (Go's `loopnest`) is `Topology::loop_of`, bucketed per loop.
//! Cost: O(instructions) once per body, by an explicit stack.

use super::{Body, NONE, Tag, Topology, u32_of};
use hd_base::SliceSort as _;

/// The block tree in preorder, with each loop's own instructions: a
/// non-block's parent is `Topology::blk`, a block's `Topology::owner`.
pub struct Nest {
    /// Each instruction's preorder number, or `NONE` under no root.
    entry: Vec<u32>,
    /// The last preorder number in each instruction's subtree.
    exit: Vec<u32>,
    /// The instructions in preorder.
    order: Vec<u32>,
    /// Each instruction's children, `kids[at[i]..at[i + 1]]`, in the
    /// order of the statements that hold them.
    at: Vec<u32>,
    kids: Vec<u32>,
    /// Each loop's own instructions, those whose innermost loop it is,
    /// in index order: `own[own_at[l]..own_at[l + 1]]`.
    own_at: Vec<u32>,
    own: Vec<u32>,
}

impl Nest {
    #[must_use]
    pub fn new(b: &Body, topo: &Topology) -> Self {
        let n = b.len();
        let parent = |i: usize| -> u32 {
            match (b.tags[i], topo.is_root[i]) {
                (Tag::Block, true) => NONE,
                (Tag::Block, false) => topo.owner[i],
                _ => topo.blk[i],
            }
        };
        let (at, mut kids) = buckets(n, parent);
        for x in (0..n).filter(|&x| b.tags[x] == Tag::Block) {
            kids[at[x] as usize..at[x + 1] as usize].sort_stable_by_key(|&k| topo.pos[k as usize]);
        }
        let mut entry = vec![NONE; n];
        let mut order = Vec::with_capacity(n);
        let mut stack = Vec::new();
        for root in (0..n).filter(|&i| topo.is_root[i]) {
            stack.push(u32_of(root));
            // Each instruction has one parent, so none is reached twice.
            while let Some(x) = stack.pop() {
                let x = x as usize;
                entry[x] = u32_of(order.len());
                order.push(u32_of(x));
                stack.extend(kids[at[x] as usize..at[x + 1] as usize].iter().rev());
            }
        }
        // Subtree sizes, children before parents in reverse preorder.
        let mut size = vec![1u32; n];
        for &x in order.iter().rev() {
            let p = parent(x as usize) as usize;
            if p < n {
                size[p] += size[x as usize];
            }
        }
        // `NONE` saturates, so an unnumbered instruction stays `NONE`.
        let exit = (0..n)
            .map(|i| entry[i].saturating_add(size[i] - 1))
            .collect();
        let (own_at, own) = buckets(n, |i| topo.loop_of[i]);
        Self {
            entry,
            exit,
            order,
            at,
            kids,
            own_at,
            own,
        }
    }

    /// The children of instruction `i`: the blocks a non-block runs, the
    /// instructions a block lists, in statement order.
    fn children(&self, i: u32) -> &[u32] {
        &self.kids[self.at[i as usize] as usize..self.at[i as usize + 1] as usize]
    }

    /// Instruction `i` and everything below it, in preorder.
    fn subtree(&self, i: u32) -> &[u32] {
        &self.order[self.entry[i as usize] as usize..=self.exit[i as usize] as usize]
    }

    /// Instruction `i`'s preorder number, `NONE` under no root.
    #[must_use]
    pub fn entry(&self, i: u32) -> u32 {
        self.entry[i as usize]
    }

    /// The instructions in preorder.
    #[must_use]
    pub fn preorder(&self) -> &[u32] {
        &self.order
    }

    /// The child of block `blk` whose subtree holds `j`, when `j` is
    /// strictly below `blk`: an interval test, then a binary search of
    /// the children, which are in preorder. O(log children).
    #[must_use]
    pub fn child_above(&self, blk: u32, j: u32) -> Option<u32> {
        let (e, p) = (self.entry[blk as usize], self.entry[j as usize]);
        if e == NONE || p == NONE || p <= e || p > self.exit[blk as usize] {
            return None;
        }
        let kids = self.children(blk);
        // The first child is numbered `e + 1`, at most `p`.
        Some(kids[kids.partition_point(|&k| self.entry[k as usize] <= p) - 1])
    }

    /// Whether `d` runs before `j` on every path of their sub-body that
    /// reaches `j`: `d` is a statement of a block around `j`, before the
    /// statement that holds `j`. O(log children).
    #[must_use]
    pub fn dominates(&self, topo: &Topology, d: u32, j: u32) -> bool {
        let blk = topo.blk[d as usize];
        blk != NONE
            && self
                .child_above(blk, j)
                .is_some_and(|k| topo.pos[k as usize] > topo.pos[d as usize])
    }

    /// Whether `i` is in loop `l`'s body, nested blocks included, closure
    /// bodies not: strictly below a block the loop runs. O(its blocks).
    #[must_use]
    pub fn in_loop(&self, i: u32, l: u32) -> bool {
        let p = self.entry[i as usize];
        self.children(l)
            .iter()
            .any(|&blk| self.entry[blk as usize] < p && p <= self.exit[blk as usize])
    }

    /// The instructions whose innermost loop is `l`, in index order.
    #[must_use]
    pub fn own(&self, l: u32) -> &[u32] {
        &self.own[self.own_at[l as usize] as usize..self.own_at[l as usize + 1] as usize]
    }

    /// Loop `l`'s instructions ([`Nest::in_loop`]) in index order.
    /// O(their number, log).
    #[must_use]
    pub fn loop_body(&self, l: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for &blk in self.children(l) {
            out.extend_from_slice(&self.subtree(blk)[1..]);
        }
        out.sort_stable();
        out
    }

    /// Every instruction of the statements of block `blk` at positions
    /// `from..=to`, each statement included, in index order. O(their
    /// number, log).
    #[must_use]
    pub fn statements(&self, b: &Body, blk: u32, from: usize, to: usize) -> Vec<u32> {
        let mut out = Vec::new();
        // A child block listed here sits under the statement that runs it.
        let record = b.record(b.data[blk as usize][0]);
        for &m in record.iter().take(to.saturating_add(1)).skip(from) {
            if b.tags[m as usize] != Tag::Block {
                out.extend_from_slice(self.subtree(m));
            }
        }
        out.sort_stable();
        out
    }

    /// The blocks around `x`, innermost first, each with the position of
    /// the statement in it that holds `x`; `None` when the chain does not
    /// reach a sub-body's root (a closure's own body). O(depth).
    #[must_use]
    pub fn block_path(topo: &Topology, b: &Body, mut x: u32) -> Option<Vec<(u32, usize)>> {
        let mut out = Vec::new();
        loop {
            if b.tags[x as usize] == Tag::Block {
                if topo.is_root[x as usize] {
                    return Some(out);
                }
                x = topo.owner[x as usize];
                if x == NONE {
                    return None;
                }
                continue;
            }
            let blk = topo.blk[x as usize];
            if blk == NONE {
                return None;
            }
            out.push((blk, topo.pos[x as usize] as usize));
            x = blk;
        }
    }
}

/// Instructions `0..n` grouped by `key` (an instruction, or `NONE` for
/// none) by counting sort: group `k` is `items[at[k]..at[k + 1]]`, in
/// index order. O(n).
fn buckets(n: usize, key: impl Fn(usize) -> u32) -> (Vec<u32>, Vec<u32>) {
    let mut at = vec![0u32; n + 1];
    for k in (0..n).map(&key).filter(|&k| (k as usize) < n) {
        at[k as usize + 1] += 1;
    }
    for i in 0..n {
        at[i + 1] += at[i];
    }
    let mut fill = at.clone();
    let mut items = vec![0u32; at[n] as usize];
    for i in 0..n {
        let k = key(i) as usize;
        if k < n {
            items[fill[k] as usize] = u32_of(i);
            fill[k] += 1;
        }
    }
    (at, items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, PrimOp, TirBuilder, TirSink as _};
    use hd_base::{DefId, NodeIdx};
    use hd_types::Ty;

    /// `s0; loop { s1; if s1 { break }; s2 }; s3`: dominance runs down a
    /// block's later statements and into the blocks they run, never up
    /// or across; the loop holds its body's instructions, not those
    /// around it.
    #[test]
    fn dominance_and_loops_follow_the_block_tree() {
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let root = b.open_block();
        let one = b.const_value(us, 1);
        let s0 = b.prim(PrimOp::Add as u32, &[one, one], us, n);
        let lp = b.open_loop();
        let body = b.open_block();
        let s1 = b.prim(PrimOp::Lt as u32, &[s0, one], Ty::BOOL, n);
        let tb = b.open_block();
        b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, n);
        let then = b.close_block(tb, None, Ty::VOID, n);
        let eb = b.open_block();
        let els = b.close_block(eb, None, Ty::VOID, n);
        let arms = b.refs_record(&[then, els]);
        let test = b.emit(Tag::If, s1.0, arms, Ty::VOID, n);
        let s2 = b.prim(PrimOp::Add as u32, &[s0, one], us, n);
        let body = b.close_block(body, None, Ty::VOID, n);
        let l = b.close_loop(lp, body, Ty::VOID, n);
        let s3 = b.prim(PrimOp::Add as u32, &[s0, one], us, n);
        let root = b.close_block(root, Some(s3), us, n);
        let b = b.finish(root, &[]).expect("verifies");
        let topo = Topology::new(&b);
        let nest = Nest::new(&b, &topo);
        let [s0, s1, s2, s3, l, test, body, root, then] =
            [s0, s1, s2, s3, l, test, body, root, then].map(|r| r.0);
        assert!(nest.dominates(&topo, s0, s1) && nest.dominates(&topo, s0, s3));
        assert!(nest.dominates(&topo, s1, s2) && nest.dominates(&topo, test, s2));
        assert!(!nest.dominates(&topo, s1, s0) && !nest.dominates(&topo, s2, s3));
        assert!(!nest.dominates(&topo, s1, s1) && !nest.dominates(&topo, root, s0));
        assert!(nest.in_loop(s1, l) && nest.in_loop(s2, l) && !nest.in_loop(s3, l));
        assert!(!nest.in_loop(s0, l) && !nest.in_loop(l, l));
        assert_eq!(nest.child_above(root, s1), Some(l));
        assert_eq!(nest.child_above(body, s0), None);
        assert_eq!(nest.child_above(body, body), None);
        let brk = nest.loop_body(l);
        assert!(brk.windows(2).all(|w| w[0] < w[1]) && brk.contains(&then));
        assert!(brk.contains(&s1) && brk.contains(&s2) && !brk.contains(&s3));
        assert!(nest.own(l).contains(&s1) && !nest.own(l).contains(&s0));
        assert_eq!(nest.statements(&b, root, 0, 0), [s0]);
        let at = |i: u32| topo.pos[i as usize] as usize;
        assert_eq!(nest.statements(&b, body, at(s2), usize::MAX), [s2]);
        assert!(
            nest.statements(&b, body, at(test), at(test))
                .contains(&then)
        );
        assert_eq!(
            Nest::block_path(&topo, &b, s1),
            Some(vec![(body, at(s1)), (root, at(l))])
        );
        assert_eq!(Nest::block_path(&topo, &b, root), Some(Vec::new()));
    }
}
