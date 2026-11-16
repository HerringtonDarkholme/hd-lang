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
use hd_base::SliceSort as _;
use hd_types::Ty;

use super::Loc;
use crate::ir::{Body, NONE, Ref, Tag, Topology, slots, u32_of};

/// A checked-body insertion before member `pos` of `block` (the member
/// count means its end). Operands use the old body's numbering; insertion
/// `k` may be referenced as `old_len + k` by later insertions or records.
#[derive(Clone, Copy, Debug)]
pub struct BatchInsert {
    pub block: u32,
    pub pos: u32,
    pub tag: Tag,
    pub data: [u32; 2],
    pub ty: Ty,
    pub syn: NodeIdx,
}

/// Splices a batch once, returning old-to-new instruction numbers followed
/// by inserted-to-new numbers, in batch order. `topo` describes the original
/// instruction structure. No ownership or provider construction is added.
///
/// # Errors
/// An invalid insertion position or a malformed operand during renumbering.
pub fn insert_batch(
    b: &mut Body,
    topo: &Topology,
    inserts: &[BatchInsert],
) -> Result<Vec<u32>, String> {
    if inserts.is_empty() {
        return Ok((0..u32_of(b.len())).collect());
    }
    let mut tables = Tables::default();
    for edit in inserts {
        let block = edit.block as usize;
        if b.tags.get(block) != Some(&Tag::Block)
            || edit.pos as usize > b.record(b.data[block][0]).len()
        {
            return Err(format!(
                "invalid insertion position {} in block %{}",
                edit.pos, edit.block
            ));
        }
        tables.insert(edit.block, edit.pos, edit.tag, edit.data, edit.ty, edit.syn);
    }
    apply(b, topo, &mut tables)?;
    Ok(tables.new_of)
}

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
}

/// The edits of one body, and the scratch of their splicing, reused.
#[derive(Default)]
pub(crate) struct Tables {
    edits: Vec<Edit>,
    /// (instruction, operand position, edit): the operand names the edit.
    rewrites: Vec<(u32, Loc, u32)>,
    /// Statements to delete, which nothing uses (a `Drop` the move pass
    /// makes unneeded).
    removed: Vec<u32>,
    dead: Vec<bool>,
    blocks: Vec<u32>,
    anchor: Vec<(u32, u32)>,
    order: Vec<u32>,
    new_of: Vec<u32>,
    done: Vec<bool>,
    per_block: Vec<(u32, u32, u32)>,
    /// The first anchor of each source position in each inlined frame
    /// ([`frame`]), for the new instructions' anchors.
    by_syn: hd_base::HashMap<(NodeIdx, Option<u32>), u32>,
    words: Vec<u32>,
    locs: Vec<Loc>,
    /// Added to every constant word: 0 for a pass, the caller's constant
    /// count for the inliner's copy of a callee.
    konst: u32,
}

impl Tables {
    pub(crate) fn clear(&mut self) {
        self.edits.clear();
        self.rewrites.clear();
        self.removed.clear();
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
        });
        u32::try_from(self.edits.len() - 1).expect("edits")
    }

    /// Points operand `at` of instruction `inst` at edit `edit`'s value.
    pub(crate) fn rewrite(&mut self, inst: u32, at: Loc, edit: u32) {
        self.rewrites.push((inst, at, edit));
    }

    /// Deletes statement `inst`, which no instruction uses.
    pub(crate) fn remove(&mut self, inst: u32) {
        self.removed.push(inst);
    }
}

/// The body as the pending edits leave it, for a pass that walks it
/// before the splice: each deleted statement marked. The passes that read
/// it run before any pass inserts a statement.
#[derive(Default)]
pub(crate) struct Pending {
    /// Per instruction: a statement an earlier pass deleted.
    pub(crate) dead: Vec<bool>,
}

impl Pending {
    /// Reads `edits` over a body of `n` instructions.
    pub(crate) fn build(&mut self, n: usize, edits: &Tables) {
        self.dead.clear();
        self.dead.resize(n, false);
        for &x in &edits.removed {
            self.dead[x as usize] = true;
        }
    }
}

/// The inlined copy anchor `a` names, by the anchor of the call it
/// copied ([`Body::inlined_frame`]); `None` for the body's own
/// instructions, and for no anchor.
fn frame(b: &Body, a: u32) -> Option<u32> {
    b.inlined_frame(a).map(|(_, _, call)| call)
}

/// Splices the edits into `b`, whose block structure is `topo`, and
/// renumbers it.
pub(crate) fn apply(b: &mut Body, topo: &Topology, t: &mut Tables) -> Result<(), String> {
    if t.edits.is_empty() && t.removed.is_empty() {
        return Ok(());
    }
    let n = b.len();
    let k = t.edits.len();
    t.dead.clear();
    t.dead.resize(n, false);
    for &x in &t.removed {
        t.dead[x as usize] = true;
    }
    // Each edit's column position: before the first instruction of the
    // statement it precedes, or after a block's last statement.
    t.anchor.clear();
    for (e, ed) in t.edits.iter().enumerate() {
        let members = b.record(b.data[ed.blk as usize][0]);
        let at = match members.get(ed.pos as usize) {
            Some(&m) => topo.first[m as usize],
            None => match members.last() {
                Some(&m) => m + 1,
                None => topo.first[ed.blk as usize],
            },
        };
        t.anchor.push((at, u32_of(e)));
    }
    t.anchor.sort_stable();
    // Each new instruction with a source position that stands before a
    // statement takes the anchor of an instruction at its position, in
    // that statement's inlined frame, else the statement's anchor: the
    // code it emits runs for that position, so a panic it raises names its
    // line and frame (`crates/hd_wasm/DESIGN.md` §15.9). With none, its
    // calls took the anchor of the last call written before them, an
    // earlier statement's or an enclosing loop header's (#1998). One at a
    // block's end keeps none: it runs as the block exits, and its code
    // takes the scope's origin (§23.33). One pass over the body indexes
    // the anchors by position and frame, so each edit is one probe.
    let anchored = !b.anchors.is_empty() && t.edits.iter().any(|ed| ed.syn != NodeIdx::NONE);
    t.by_syn.clear();
    if anchored {
        for i in 0..n {
            let a = b.anchors[i];
            if a != NONE && b.syn[i] != NodeIdx::NONE {
                t.by_syn.entry((b.syn[i], frame(b, a))).or_insert(a);
            }
        }
    }
    // Append the new instructions, numbered n.. in the old numbering.
    for ed in &t.edits {
        let members = b.record(b.data[ed.blk as usize][0]);
        let anchor = match members.get(ed.pos as usize) {
            Some(&m) if anchored && ed.syn != NodeIdx::NONE => {
                let next = b.anchors[m as usize];
                (t.by_syn.get(&(ed.syn, frame(b, next))).copied()).unwrap_or(next)
            }
            _ => NONE,
        };
        b.tags.push(ed.tag);
        b.data.push(ed.data);
        b.ty.push(ed.ty);
        b.syn.push(ed.syn);
        if !b.anchors.is_empty() {
            b.anchors.push(anchor);
        }
    }
    // The blocks that gain or lose members get new records.
    t.per_block.clear();
    for (e, ed) in t.edits.iter().enumerate() {
        t.per_block.push((ed.blk, ed.pos, u32_of(e)));
    }
    t.per_block.sort_stable();
    t.blocks.clear();
    t.blocks.extend(t.edits.iter().map(|ed| ed.blk));
    t.blocks
        .extend(t.removed.iter().map(|&x| topo.blk[x as usize]));
    t.blocks.sort_stable();
    t.blocks.dedup();
    // Every edit's block is in `blocks`, so `x` is always at the first
    // edit of `blk` or past its last.
    let mut x = 0;
    for z in 0..t.blocks.len() {
        let blk = t.blocks[z];
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
                t.words.push(u32_of(n) + t.per_block[y].2);
                y += 1;
            }
            if !t.dead.get(m as usize).copied().unwrap_or(false) {
                t.words.push(m);
            }
        }
        while y < end {
            t.words.push(u32_of(n) + t.per_block[y].2);
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
            t.order.push(u32_of(n) + t.anchor[a].1);
            a += 1;
        }
        if !t.dead[i] {
            t.order.push(u32_of(i));
        }
    }
    while a < t.anchor.len() {
        t.order.push(u32_of(n) + t.anchor[a].1);
        a += 1;
    }
    t.new_of.clear();
    t.new_of.resize(n + k, NONE);
    for (new, &old) in t.order.iter().enumerate() {
        t.new_of[old as usize] = u32_of(new);
    }
    permute_columns(b, &t.order);
    renumber(b, t)
}

/// Puts the instruction columns in `order`: row `k` becomes old row
/// `order[k]`.
pub(crate) fn permute_columns(b: &mut Body, order: &[u32]) {
    permute(&mut b.tags, order);
    permute(&mut b.data, order);
    permute(&mut b.ty, order);
    permute(&mut b.syn, order);
    if !b.anchors.is_empty() {
        permute(&mut b.anchors, order);
    }
}

fn permute<T: Copy>(v: &mut Vec<T>, order: &[u32]) {
    let old = std::mem::take(v);
    v.extend(order.iter().map(|&o| old[o as usize]));
}

/// Renumbers every instruction word of `b` through `new_of`, and moves
/// every constant word `konst` rows on: the inliner's caller and copy.
pub(crate) fn renumber_through(b: &mut Body, new_of: Vec<u32>, konst: u32) -> Result<(), String> {
    let mut t = Tables {
        new_of,
        konst,
        ..Tables::default()
    };
    renumber(b, &mut t)
}

/// Renumbers every word of `b` that names an instruction, by the operand
/// schema ([`slots`]); a record several instructions share is renumbered
/// once.
fn renumber(b: &mut Body, t: &mut Tables) -> Result<(), String> {
    let map = |t: &Tables, w: u32| -> u32 {
        if w == NONE {
            return w;
        }
        if w & Ref::CONST_BIT != 0 {
            return w + t.konst;
        }
        t.new_of.get(w as usize).copied().unwrap_or(w)
    };
    t.done.clear();
    t.done.resize(b.extra.len(), false);
    for i in 0..b.len() {
        t.locs.clear();
        slots(b, i, |at, _, _| t.locs.push(at));
        for x in 0..t.locs.len() {
            let word = match t.locs[x] {
                Loc::Data(k) => b.data[i][usize::from(k)],
                Loc::Extra(p) => *b
                    .extra
                    .get(p as usize)
                    .ok_or_else(|| format!("operand record {p} out of range"))?,
            };
            if word != NONE && word & Ref::CONST_BIT == 0 && word as usize >= t.new_of.len() {
                return Err(format!(
                    "instruction operand %{word} out of range while splicing"
                ));
            }
            match t.locs[x] {
                Loc::Data(k) => {
                    let k = usize::from(k);
                    b.data[i][k] = map(t, b.data[i][k]);
                }
                Loc::Extra(p) => remap_extra(b, t, p, &map),
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
    // A row whose call or argument an edit deletes goes with it.
    b.held_args = (b.held_args.iter())
        .map(|&(c, a)| (map(t, c), map(t, a)))
        .filter(|&(c, a)| c != NONE && a != NONE)
        .collect();
    // Both words name instructions no edit deletes (a suspension point and
    // a value), and the splice keeps their order.
    for row in &mut b.unwind_drops {
        *row = (map(t, row.0), map(t, row.1));
    }
    // A read the edits delete takes its row with it; the splice keeps
    // the order of the rest.
    for row in &mut b.full_cells {
        row.0 = map(t, row.0);
    }
    b.full_cells.retain(|r| r.0 != NONE);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, INLINED, Providers, TirBuilder, TirSink};
    use hd_base::DefId;
    use hd_types::TyList;

    /// One block of calls at source positions 10, 20 and 10, the third in
    /// an inlined copy (whose syntax node numbers are its callee's), each
    /// anchored at itself.
    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let callee = Callee::Item {
            def: DefId::from_raw(7),
            targs: TyList::EMPTY,
        };
        for syn in [10, 20, 10] {
            b.call(
                &callee,
                &[],
                Providers::None,
                Ty::VOID,
                NodeIdx::from_raw(syn),
            );
        }
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::from_raw(30));
        let mut b = b.finish(root, &[]).unwrap();
        b.anchors = (0..u32_of(b.len())).collect();
        // The third call is instruction 0 of a copy made at anchor 1.
        let third = b.tags.iter().rposition(|&t| t == Tag::Call).unwrap();
        b.anchors[third] = INLINED;
        b.inlined = vec![[0, 1, 7, 1]];
        b
    }

    /// The anchor of the instruction each edit `(position, syntax node)`
    /// added, in edit order.
    fn added(edits: &[(u32, u32)]) -> Vec<u32> {
        let mut b = body();
        let topo = Topology::new(&b);
        let root = b.sub_root[0];
        let mut t = Tables::default();
        for &(pos, syn) in edits {
            let syn = if syn == 0 {
                NodeIdx::NONE
            } else {
                NodeIdx::from_raw(syn)
            };
            t.insert(root, pos, Tag::IsUnique, [0, 0], Ty::VOID, syn);
        }
        let n = b.len();
        let old = b.anchors.clone();
        apply(&mut b, &topo, &mut t).unwrap();
        // The kept instructions keep their anchors; the rest are new.
        let mut kept = old.clone();
        let mut out = Vec::new();
        for &a in &b.anchors {
            if let Some(k) = kept.iter().position(|&x| x == a) {
                kept.remove(k);
            } else {
                out.push(a);
            }
        }
        assert_eq!(b.len(), n + edits.len());
        out
    }

    #[test]
    fn a_new_instruction_takes_the_anchor_of_its_source_position_in_its_frame() {
        let body = body();
        let calls: Vec<u32> = (0..body.len())
            .filter(|&i| body.tags[i] == Tag::Call)
            .map(|i| body.anchors[i])
            .collect();
        // Before the second call, at position 10: the first call's anchor,
        // not the second's (#1998).
        assert_eq!(added(&[(1, 10)]), [calls[0]]);
        // Before the third call, in the inlined copy, at position 10: the
        // copy's own instruction, not the caller's call at that number.
        assert_eq!(added(&[(2, 10)]), [calls[2]]);
        // A position no instruction of that frame has: the statement it
        // stands before.
        assert_eq!(added(&[(1, 99)]), [calls[1]]);
        // At the block's end: none, as the block's exit takes the scope's
        // origin.
        assert_eq!(added(&[(3, 20)]), [NONE]);
        // No source position: no anchor, as the emitter writes no row for
        // it.
        assert_eq!(added(&[(1, 0)]), [NONE]);
    }
}
