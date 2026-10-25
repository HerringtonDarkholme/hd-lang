//! A body's block structure, built again only when its blocks change and
//! shared by reference by the ownership passes, the verifier and the
//! backend (`docs/compiler/tir-v2.md`, "Data layout of the passes"):
//! which block lists each instruction, which instruction runs each
//! block, and what follows from the two. A block's members and an
//! instruction's blocks come before it, so each column takes one pass.

use super::{Body, NONE, Tag, Word, slots, u32_of};
use crate::own::fill;

/// The block structure of a body, a column per instruction.
#[derive(Default)]
pub struct Topology {
    /// The block whose list holds each instruction, or `NONE`, and the
    /// instruction's position there. A child block is listed too, in the
    /// block around the instruction that runs it, so of the blocks only
    /// the body's root has `NONE`.
    pub blk: Vec<u32>,
    pub pos: Vec<u32>,
    /// The instruction that runs each block, or `NONE`: it names the
    /// block in an operand, while `blk` holds the block that lists it.
    /// A cleanup scope owns its suites, since it runs them, not the
    /// `Defer` that registers one.
    pub owner: Vec<u32>,
    /// The lowest instruction inside each instruction: its own index, or
    /// the first of the blocks it lists or runs.
    pub first: Vec<u32>,
    /// Each instruction's sub-body: a root's own, else its block's, else
    /// its owner's; 0 for one in no block.
    pub sub: Vec<u32>,
    /// Whether each instruction is a sub-body's root block.
    pub is_root: Vec<bool>,
    /// The label each instruction is the target of, or `NONE`.
    pub label_of: Vec<u32>,
    /// The innermost loop whose body holds each instruction, or `NONE`.
    pub loop_of: Vec<u32>,
}

impl Topology {
    #[must_use]
    pub fn new(b: &Body) -> Self {
        let mut t = Self::default();
        t.build(b);
        t
    }

    /// Rebuilds every column for `b`, reusing their storage.
    pub fn build(&mut self, b: &Body) {
        let n = b.len();
        fill(&mut self.blk, n, NONE);
        fill(&mut self.pos, n, 0);
        fill(&mut self.owner, n, NONE);
        fill(&mut self.sub, n, 0);
        fill(&mut self.is_root, n, false);
        fill(&mut self.label_of, n, NONE);
        fill(&mut self.loop_of, n, NONE);
        self.first.clear();
        for i in 0..n {
            let mut f = u32_of(i);
            if b.tags[i] == Tag::Block {
                for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
                    if (m as usize) < n {
                        self.blk[m as usize] = u32_of(i);
                        self.pos[m as usize] = u32_of(p);
                        if (m as usize) < i {
                            f = f.min(self.first[m as usize]);
                        }
                    }
                }
            }
            let defer = b.tags[i] == Tag::Defer;
            let (owner, first) = (&mut self.owner, &self.first);
            for_blocks(b, i, |x| {
                if (x as usize) < i {
                    f = f.min(first[x as usize]);
                }
                if !(defer && owner[x as usize] != NONE) {
                    owner[x as usize] = u32_of(i);
                }
            });
            self.first.push(f);
        }
        for (s, &r) in b.sub_root.iter().enumerate() {
            if (r as usize) < n {
                self.is_root[r as usize] = true;
                self.sub[r as usize] = u32_of(s);
            }
        }
        for (l, &x) in b.label_inst.iter().enumerate() {
            if let Some(slot) = self.label_of.get_mut(x as usize) {
                *slot = u32_of(l);
            }
        }
        // A child block's parent is the instruction that runs it, not the
        // block that also lists it, so a loop's body sees its `Loop`.
        for i in (0..n).rev() {
            let p = if self.owner[i] == NONE {
                self.blk[i]
            } else {
                self.owner[i]
            };
            if self.is_root[i] || p == NONE {
                continue;
            }
            let p = p as usize;
            self.sub[i] = self.sub[p];
            self.loop_of[i] =
                if b.tags[i] == Tag::Block && b.tags[p] == Tag::Loop && b.data[p][0] == u32_of(i) {
                    u32_of(p)
                } else {
                    self.loop_of[p]
                };
        }
    }

    /// The statement that holds block `blk`: its owner's block and
    /// position, or `None` at a sub-body's root.
    #[must_use]
    pub fn up(&self, blk: u32) -> Option<(u32, u32)> {
        let o = self.owner[blk as usize];
        if self.is_root[blk as usize] || o == NONE || self.blk[o as usize] == NONE {
            return None;
        }
        Some((self.blk[o as usize], self.pos[o as usize]))
    }

    /// The blocks that enclose instruction `i` in its own sub-body,
    /// innermost first. A sub-body's root block ends the walk, so a
    /// closure's instructions are never inside the blocks around it.
    pub fn enclosing(&self, i: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(Some(i), |&x| {
            let x = x as usize;
            (x < self.blk.len() && !self.is_root[x] && self.blk[x] != NONE).then(|| self.blk[x])
        })
        .skip(1)
        .take(self.blk.len())
    }
}

/// The blocks instruction `i` runs, without allocating: its
/// [`Word::Block`]s, a block operand, a block list, a switch's arms, a
/// `Defer`'s suite.
pub(crate) fn for_blocks(b: &Body, i: usize, mut f: impl FnMut(u32)) {
    slots(b, i, |_, x, s| {
        if s == Word::Block && b.tags.get(x as usize) == Some(&Tag::Block) {
            f(x);
        }
    });
}

/// Walks every instruction the blocks `roots` hold, nested blocks
/// included, depth first through `stack`: calls `f` on each, and goes into
/// its blocks when `f` returns true.
pub(crate) fn inside(
    b: &Body,
    roots: &[u32],
    stack: &mut Vec<u32>,
    mut f: impl FnMut(usize) -> bool,
) {
    stack.clear();
    stack.extend_from_slice(roots);
    while let Some(x) = stack.pop() {
        let i = x as usize;
        if i >= b.len() || !f(i) {
            continue;
        }
        if b.tags[i] == Tag::Block {
            stack.extend_from_slice(b.record(b.data[i][0]));
        }
        for_blocks(b, i, |blk| stack.push(blk));
    }
}
