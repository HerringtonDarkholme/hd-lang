//! The rebuild of a body after a pass (`docs/compiler/tir-v2.md`, "Data
//! layout of the passes"): a pass records new instructions as edits, each
//! to stand in a block before a position, operand rewrites that point a
//! use at one of them, and the statements it deletes; [`apply`] splices
//! every edit in and drops every deleted statement at once.
//!
//! A body keeps its instructions in evaluation order, with a block's
//! members and an instruction's blocks before the block or instruction
//! (the verifier's def-before-use and the passes' span reasoning read that
//! order). So an edit takes the column position just before the first
//! instruction of the statement it precedes, and the whole body is
//! renumbered once: every word that names an instruction, by the operand
//! schema, and the columns that name instructions (labels, sub-body roots,
//! suspension rows, the fallback rows).

use hd_base::NodeIdx;
use hd_types::Ty;

use super::Loc;
use crate::ir::{Body, NONE, Op, Ref, Tag, place};

/// One new instruction: it stands in block `blk` before position `pos`
/// (`pos` = the block's length: at its end).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edit {
    blk: u32,
    pos: u32,
    tag: Tag,
    data: [u32; 2],
    ty: Ty,
    syn: NodeIdx,
    /// Stands after the other edits before the same position, right
    /// before the statement (a header copy before its use).
    late: bool,
}

/// An edit's order among the edits before one position: its number, or
/// after every other for a late one.
const LATE: u32 = 1 << 31;

/// The edits of one body, and the scratch of their splicing, reused.
#[derive(Default)]
pub(crate) struct Tables {
    edits: Vec<Edit>,
    /// (instruction, operand position, edit): the operand names the edit.
    rewrites: Vec<(u32, Loc, u32)>,
    /// (block, statement): statements to delete, which nothing uses (a
    /// `Drop` the move pass makes unneeded).
    removed: Vec<(u32, u32)>,
    /// Per edit: withdrawn by a later pass before the splice (a `Drop`
    /// that pair cancellation deletes), so it is never spliced in.
    cancelled: Vec<bool>,
    dead: Vec<bool>,
    blocks: Vec<u32>,
    start: Vec<u32>,
    anchor: Vec<(u32, u32)>,
    order: Vec<u32>,
    new_of: Vec<u32>,
    done: Vec<bool>,
    per_block: Vec<(u32, u32, u32)>,
    words: Vec<u32>,
    places: Vec<u32>,
}

impl Tables {
    pub(crate) fn clear(&mut self) {
        self.edits.clear();
        self.rewrites.clear();
        self.removed.clear();
        self.cancelled.clear();
    }

    /// Records a new instruction before position `pos` of block `blk`, and
    /// returns its edit number.
    pub(crate) fn insert(
        &mut self,
        blk: u32,
        pos: u32,
        tag: Tag,
        data: [u32; 2],
        ty: Ty,
        syn: NodeIdx,
    ) -> u32 {
        self.edits.push(Edit {
            blk,
            pos,
            tag,
            data,
            ty,
            syn,
            late: false,
        });
        self.cancelled.push(false);
        u32::try_from(self.edits.len() - 1).expect("edits")
    }

    /// Records a new instruction that stands right before statement `pos`
    /// of block `blk`, after every other edit there.
    pub(crate) fn insert_late(
        &mut self,
        blk: u32,
        pos: u32,
        tag: Tag,
        data: [u32; 2],
        ty: Ty,
        syn: NodeIdx,
    ) -> u32 {
        let e = self.insert(blk, pos, tag, data, ty, syn);
        self.edits[e as usize].late = true;
        e
    }

    /// Edit `e`'s order among the edits before its position.
    fn rank(&self, e: usize) -> u32 {
        u32_of(e) | if self.edits[e].late { LATE } else { 0 }
    }

    /// How many instructions are recorded.
    pub(crate) fn len(&self) -> usize {
        self.edits.len()
    }

    /// Edit `e`'s block, position, tag, data words and source node.
    pub(crate) fn get(&self, e: u32) -> (u32, u32, Tag, [u32; 2], NodeIdx) {
        let ed = &self.edits[e as usize];
        (ed.blk, ed.pos, ed.tag, ed.data, ed.syn)
    }

    /// Withdraws edit `e`, which no operand names.
    pub(crate) fn cancel(&mut self, e: u32) {
        self.cancelled[e as usize] = true;
    }

    /// Whether edit `e` was withdrawn.
    pub(crate) fn is_cancelled(&self, e: u32) -> bool {
        self.cancelled[e as usize]
    }

    /// Points operand `at` of instruction `inst` at edit `edit`'s value.
    pub(crate) fn rewrite(&mut self, inst: u32, at: Loc, edit: u32) {
        self.rewrites.push((inst, at, edit));
    }

    /// Deletes statement `inst` of block `blk`, which no instruction uses.
    pub(crate) fn remove(&mut self, blk: u32, inst: u32) {
        self.removed.push((blk, inst));
    }
}

/// The blocks as the pending edits leave them, for a pass that walks the
/// body before the splice: each deleted statement marked, and each block's
/// edits that still stand, by position, in recording order.
#[derive(Default)]
pub(crate) struct Pending {
    /// Per instruction: a statement an earlier pass deleted.
    pub(crate) dead: Vec<bool>,
    /// Each block's edits (by block instruction), in `row`.
    row_start: Vec<u32>,
    row: Vec<u32>,
    fill: Vec<u32>,
}

impl Pending {
    /// Reads `edits` over a body of `n` instructions.
    pub(crate) fn build(&mut self, n: usize, edits: &Tables) {
        self.dead.clear();
        self.dead.resize(n, false);
        for &(_, x) in &edits.removed {
            self.dead[x as usize] = true;
        }
        self.row_start.clear();
        self.row_start.resize(n + 1, 0);
        let live = |e: usize| !edits.cancelled[e];
        for (e, ed) in edits.edits.iter().enumerate() {
            if live(e) {
                self.row_start[ed.blk as usize + 1] += 1;
            }
        }
        for i in 0..n {
            self.row_start[i + 1] += self.row_start[i];
        }
        self.row.clear();
        self.row.resize(self.row_start[n] as usize, 0);
        self.fill.clear();
        self.fill.extend_from_slice(&self.row_start[..n]);
        for (e, ed) in edits.edits.iter().enumerate() {
            if live(e) {
                let at = &mut self.fill[ed.blk as usize];
                self.row[*at as usize] = u32_of(e);
                *at += 1;
            }
        }
        for i in 0..n {
            let (a, z) = (self.row_start[i] as usize, self.row_start[i + 1] as usize);
            if z - a > 1 {
                self.row[a..z]
                    .sort_by_key(|&e| (edits.edits[e as usize].pos, edits.rank(e as usize)));
            }
        }
    }

    /// The standing edits of block `blk`, by position.
    pub(crate) fn of(&self, blk: u32) -> &[u32] {
        let b = blk as usize;
        &self.row[self.row_start[b] as usize..self.row_start[b + 1] as usize]
    }
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

/// Splices the edits into `b` and renumbers it.
pub(crate) fn apply(b: &mut Body, t: &mut Tables) -> Result<(), String> {
    if t.edits.is_empty() && t.removed.is_empty() {
        return Ok(());
    }
    let n = b.len();
    let k = t.edits.len();
    t.dead.clear();
    t.dead.resize(n, false);
    for &(_, x) in &t.removed {
        t.dead[x as usize] = true;
    }
    // Each instruction's first instruction: its own index, or the first
    // of the blocks it runs (which precede it).
    t.start.clear();
    t.start.extend((0..n).map(u32_of));
    for i in 0..n {
        let mut s = t.start[i];
        if b.tags[i] == Tag::Block {
            for &m in b.record(b.data[i][0]) {
                if (m as usize) < i {
                    s = s.min(t.start[m as usize]);
                }
            }
        }
        for_blocks(b, i, |blk| {
            if (blk as usize) < i {
                s = s.min(t.start[blk as usize]);
            }
        });
        t.start[i] = s;
    }
    // Each edit's column position: before the first instruction of the
    // statement it precedes, or after a block's last statement.
    t.anchor.clear();
    for (e, ed) in t.edits.iter().enumerate() {
        if t.cancelled[e] {
            continue;
        }
        let members = b.record(b.data[ed.blk as usize][0]);
        let at = match members.get(ed.pos as usize) {
            Some(&m) => t.start[m as usize],
            None => match members.last() {
                Some(&m) => m + 1,
                None => t.start[ed.blk as usize],
            },
        };
        t.anchor.push((at, t.rank(e)));
    }
    t.anchor.sort_unstable();
    // Append the new instructions, numbered n.. in the old numbering.
    for ed in &t.edits {
        b.tags.push(ed.tag);
        b.data.push(ed.data);
        b.ty.push(ed.ty);
        b.syn.push(ed.syn);
        if !b.anchors.is_empty() {
            b.anchors.push(NONE);
        }
    }
    // The blocks that gain or lose members get new records.
    t.per_block.clear();
    for (e, ed) in t.edits.iter().enumerate() {
        if !t.cancelled[e] {
            t.per_block.push((ed.blk, ed.pos, t.rank(e)));
        }
    }
    t.per_block.sort_unstable();
    t.blocks.clear();
    t.blocks.extend(t.edits.iter().map(|ed| ed.blk));
    t.blocks.extend(t.removed.iter().map(|&(blk, _)| blk));
    t.blocks.sort_unstable();
    t.blocks.dedup();
    let mut x = 0;
    for z in 0..t.blocks.len() {
        let blk = t.blocks[z];
        while x < t.per_block.len() && t.per_block[x].0 < blk {
            x += 1;
        }
        let mut end = x;
        while end < t.per_block.len() && t.per_block[end].0 == blk {
            end += 1;
        }
        t.words.clear();
        let rec = b.data[blk as usize][0] as usize;
        let len = b.extra[rec] as usize;
        let mut y = x;
        for p in 0..len {
            let m = b.extra[rec + 1 + p];
            while y < end && t.per_block[y].1 as usize == p {
                t.words.push(u32_of(n) + (t.per_block[y].2 & !LATE));
                y += 1;
            }
            if !t.dead.get(m as usize).copied().unwrap_or(false) {
                t.words.push(m);
            }
        }
        while y < end {
            t.words.push(u32_of(n) + (t.per_block[y].2 & !LATE));
            y += 1;
        }
        let at = u32_of(b.extra.len());
        b.extra.push(u32_of(t.words.len()));
        b.extra.extend_from_slice(&t.words);
        b.data[blk as usize][0] = at;
        x = end;
    }
    for &(inst, at, e) in &t.rewrites {
        let v = u32_of(n) + e;
        match at {
            Loc::Data(w) => b.data[inst as usize][w as usize] = v,
            Loc::Extra(p) => b.extra[p as usize] = v,
        }
    }
    // The new order: each edit before its anchor, in recording order.
    t.order.clear();
    let mut a = 0;
    for i in 0..n {
        while a < t.anchor.len() && t.anchor[a].0 as usize == i {
            t.order.push(u32_of(n) + (t.anchor[a].1 & !LATE));
            a += 1;
        }
        if !t.dead[i] {
            t.order.push(u32_of(i));
        }
    }
    while a < t.anchor.len() {
        t.order.push(u32_of(n) + (t.anchor[a].1 & !LATE));
        a += 1;
    }
    t.new_of.clear();
    t.new_of.resize(n + k, NONE);
    for (new, &old) in t.order.iter().enumerate() {
        t.new_of[old as usize] = u32_of(new);
    }
    permute(&mut b.tags, &t.order);
    permute(&mut b.data, &t.order);
    permute(&mut b.ty, &t.order);
    permute(&mut b.syn, &t.order);
    if !b.anchors.is_empty() {
        permute(&mut b.anchors, &t.order);
    }
    renumber(b, t)
}

fn permute<T: Copy>(v: &mut Vec<T>, order: &[u32]) {
    let old = std::mem::take(v);
    v.extend(order.iter().map(|&o| old[o as usize]));
}

/// The blocks instruction `i` runs, without allocating: a block operand,
/// a block list, a switch's two arms, a `Defer`'s suite.
pub(crate) fn for_blocks(b: &Body, i: usize, mut f: impl FnMut(u32)) {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    let mut visit = |x: u32| {
        if b.tags.get(x as usize) == Some(&Tag::Block) {
            f(x);
        }
    };
    for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
        match op {
            Op::Block => visit(word),
            Op::Blocks if word != NONE => {
                for &x in b.record(word) {
                    visit(x);
                }
            }
            _ => {}
        }
    }
    match tag {
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
            for &x in b.record(w).iter().skip(1).take(2) {
                visit(x);
            }
        }
        Tag::Defer => visit(a),
        _ => {}
    }
}

/// Renumbers every word of `b` that names an instruction, by the operand
/// schema; a record several instructions share is renumbered once.
fn renumber(b: &mut Body, t: &mut Tables) -> Result<(), String> {
    let map = |t: &Tables, w: u32| -> u32 {
        if w == NONE || w & Ref::CONST_BIT != 0 {
            return w;
        }
        t.new_of.get(w as usize).copied().unwrap_or(w)
    };
    t.done.clear();
    t.done.resize(b.extra.len(), false);
    for i in 0..b.len() {
        let [a, w] = b.data[i];
        let tag = b.tags[i];
        let (oa, ob) = tag.operands();
        for (k, (word, op)) in [(a, oa), (w, ob)].into_iter().enumerate() {
            match op {
                Op::Value | Op::Block => b.data[i][k] = map(t, word),
                Op::Meta if (tag == Tag::Defer && k == 0) || (tag == Tag::Hook && k == 1) => {
                    b.data[i][k] = map(t, word);
                }
                Op::Place if word & place::BARE_LOCAL == 0 => {
                    t.places.clear();
                    super::place_words(b, word, |_, l| {
                        if let Loc::Extra(p) = l {
                            t.places.push(p);
                        }
                    });
                    for x in 0..t.places.len() {
                        let p = t.places[x];
                        remap_extra(b, t, p, &map);
                    }
                }
                Op::Values | Op::Blocks if word != NONE => {
                    let n = b.extra[word as usize];
                    let last = match tag {
                        // A call's last three words are its providers.
                        Tag::Call | Tag::Await => n.saturating_sub(3),
                        _ => n,
                    };
                    for x in 0..last {
                        remap_extra(b, t, word + 1 + x, &map);
                    }
                    if matches!(tag, Tag::Call | Tag::Await) && n >= 3 {
                        let p = word + 1 + n - 3;
                        match b.extra[p as usize] {
                            0 => {}
                            1 => {
                                let (x, len) = (b.extra[p as usize + 1], b.extra[p as usize + 2]);
                                for y in (1..len).step_by(2) {
                                    remap_extra(b, t, x + y, &map);
                                }
                            }
                            2 => remap_extra(b, t, p + 1, &map),
                            other => return Err(format!("call providers of kind {other}")),
                        }
                    }
                }
                Op::Record if word != NONE => {
                    let n = b.extra[word as usize];
                    let words = word + 1..word + 1 + n;
                    match tag {
                        Tag::Block | Tag::Intrinsic | Tag::Interp => {
                            for p in words {
                                remap_extra(b, t, p, &map);
                            }
                        }
                        Tag::NewList if k == 1 => {
                            for p in words {
                                remap_extra(b, t, p, &map);
                            }
                        }
                        Tag::CopyStruct | Tag::With | Tag::ContextNew => {
                            for p in words.skip(1).step_by(2) {
                                remap_extra(b, t, p, &map);
                            }
                        }
                        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                            let last = word + n;
                            for p in words.skip(1).step_by(2) {
                                remap_extra(b, t, p, &map);
                            }
                            if n % 2 == 1 {
                                remap_extra(b, t, last, &map);
                            }
                        }
                        Tag::CallDyn => return Err("`CallDyn`".into()),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    for x in &mut b.label_inst {
        *x = map(t, *x);
    }
    for x in &mut b.sub_root {
        *x = map(t, *x);
    }
    let mut scopes = Vec::new();
    for row in &mut b.susp {
        row.inst = map(t, row.inst);
        scopes.push(row.scopes);
    }
    for r in scopes {
        for p in r.start..r.start + r.len {
            remap_extra(b, t, p, &map);
        }
    }
    for row in &mut b.fallback {
        row[0] = map(t, row[0]);
    }
    for row in &mut b.regions {
        row.0 = map(t, row.0);
    }
    // Both words name instructions no edit deletes (a suspension point and
    // a value), and the splice keeps their order.
    for row in &mut b.unwind_drops {
        *row = (map(t, row.0), map(t, row.1));
    }
    // A loop is never deleted; the place is rooted at a local.
    for row in &mut b.stable_lists {
        row.0 = map(t, row.0);
    }
    Ok(())
}

fn remap_extra(b: &mut Body, t: &mut Tables, p: u32, map: &dyn Fn(&Tables, u32) -> u32) {
    let p = p as usize;
    if p >= b.extra.len() || t.done[p] {
        return;
    }
    t.done[p] = true;
    b.extra[p] = map(t, b.extra[p]);
}
