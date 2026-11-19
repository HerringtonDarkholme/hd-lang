//! Shapes of a body that emission reads: its block tree (`Em::topo`), and
//! what a store, a read or a reference names (a local, a list, a
//! projection, a read through a reference). Nothing here decides a count:
//! the ownership passes did (`docs/compiler/tir-v2.md`, "Stage 2").

use hd_base::SliceSort as _;
use hd_tir::ir::{Body, NONE, PlaceRoot, Proj, Ref, Tag};

use super::Em;

impl Em<'_, '_, '_> {
    /// The blocks around instruction `x`, innermost first, each with the
    /// position of the statement in it that holds `x`; `None` when the
    /// chain does not reach a sub-body's root (a closure's own body).
    pub(super) fn block_path(&self, mut x: u32) -> Option<Vec<(u32, usize)>> {
        let t = &self.topo;
        let mut out = Vec::new();
        loop {
            if self.b.tags[x as usize] == Tag::Block {
                if t.is_root[x as usize] {
                    return Some(out);
                }
                x = t.owner[x as usize];
                if x == NONE {
                    return None;
                }
                continue;
            }
            let blk = t.blk[x as usize];
            if blk == NONE {
                return None;
            }
            out.push((blk, t.pos[x as usize] as usize));
            x = blk;
        }
    }

    /// Whether instruction `d` runs before instruction `j` on every path
    /// of their sub-body that reaches `j`: `d` is a statement of a block
    /// around `j`, before the statement that holds `j`. Blocks are entered
    /// only at their start (a resumed suspending body is the exception,
    /// `susp.rs`), so reaching that statement ran `d`.
    /// An interval test on the body's [`Tree`] and a binary search of
    /// `d`'s block's children, which are in position order (#1982).
    pub(super) fn dominates(&self, d: u32, j: u32) -> bool {
        let blk = self.topo.blk[d as usize];
        blk != NONE
            && self
                .position_in(blk, j)
                .is_some_and(|q| q > self.topo.pos[d as usize] as usize)
    }

    /// The position of the statement of block `blk` that holds
    /// instruction `j`, when `blk` is around `j`: the entry for `blk` in
    /// `j`'s [`Em::block_path`], found in O(log) on the [`Tree`].
    pub(super) fn position_in(&self, blk: u32, j: u32) -> Option<usize> {
        let k = self.tree().child_above(blk, j)?;
        Some(self.topo.pos[k as usize] as usize)
    }

    /// The body's [`Tree`], built on first use.
    pub(super) fn tree(&self) -> &Tree {
        self.tree.get_or_init(|| Tree::new(self.b, &self.topo))
    }

    /// Whether `i` belongs to this loop, including its nested control
    /// blocks but excluding separately invoked closure bodies: a strict
    /// descendant of a block the loop runs. O(the blocks it runs).
    pub(super) fn in_loop(&self, i: u32, loop_inst: u32) -> bool {
        // An instruction or block under no root is `NONE`, past every
        // `last`, and so in no loop.
        let t = self.tree();
        let p = t.pre[i as usize];
        t.children(loop_inst)
            .iter()
            .any(|&blk| t.pre[blk as usize] < p && p <= t.last[blk as usize])
    }

    /// The instructions of loop `loop_inst` ([`Em::in_loop`]) in index
    /// order: the subtrees of the blocks it runs, O(their size, log).
    pub(super) fn loop_body(&self, loop_inst: u32) -> Vec<u32> {
        let t = self.tree();
        let mut out = Vec::new();
        for &blk in t.children(loop_inst) {
            out.extend_from_slice(t.below(blk));
        }
        crate::work::loop_scans(out.len());
        out.sort_stable();
        out
    }

    /// Every instruction inside the statements of block `blk` at
    /// positions `from..=to`, each statement included, in index order:
    /// the instructions whose [`Em::block_path`] passes through `blk` at
    /// such a position. O(their number, log).
    pub(super) fn statements(&self, blk: u32, from: usize, to: usize) -> Vec<u32> {
        let t = self.tree();
        let mut out = Vec::new();
        // A child block listed here sits under the statement that runs it.
        let record = self.b.record(self.b.data[blk as usize][0]);
        for &m in record.iter().take(to.saturating_add(1)).skip(from) {
            if self.b.tags[m as usize] != Tag::Block {
                out.extend_from_slice(t.subtree(m));
            }
        }
        crate::work::loop_scans(out.len());
        out.sort_stable();
        out
    }
}

/// A body's block tree in preorder (#1916), one pass to build, so that
/// whether an instruction is in a loop or a statement is an interval
/// test, and a loop's instructions are a slice of the order rather than
/// a scan of the body with a [`Em::block_path`] per instruction.
///
/// The tree is the one [`Em::block_path`] walks up: a non-block
/// instruction's parent is the block that lists it (`Topology::blk`), a
/// block's is the instruction that runs it (`Topology::owner`), and a
/// sub-body's root block has none. Only instructions under a root are
/// numbered: the others have no path. Cost: O(instructions) time and
/// words, built once per body on the first question.
pub(super) struct Tree {
    /// Each instruction's preorder number, or `NONE` when no root is
    /// above it.
    pre: Vec<u32>,
    /// The last preorder number in each instruction's subtree.
    last: Vec<u32>,
    /// The instructions in preorder.
    order: Vec<u32>,
    /// Each instruction's children, `kids[at[i]..at[i + 1]]`.
    at: Vec<u32>,
    kids: Vec<u32>,
    /// Each loop's own instructions, those whose innermost loop it is
    /// (`Topology::loop_of`), in index order: `own[own_at[l]..own_at[l + 1]]`.
    own_at: Vec<u32>,
    own: Vec<u32>,
}

impl Tree {
    pub(super) fn new(b: &Body, topo: &hd_tir::ir::Topology) -> Self {
        let n = b.len();
        let parent = |i: usize| -> u32 {
            if b.tags[i] == Tag::Block {
                if topo.is_root[i] { NONE } else { topo.owner[i] }
            } else {
                topo.blk[i]
            }
        };
        let (at, mut kids) = buckets(n, parent);
        // A block's children in the order of the statements that hold
        // them, so a block's statements are runs of the preorder in
        // position order ([`Tree::child_above`]).
        for x in 0..n {
            if b.tags[x] == Tag::Block {
                kids[at[x] as usize..at[x + 1] as usize]
                    .sort_stable_by_key(|&k| topo.pos[k as usize]);
            }
        }
        // Preorder from each root, by an explicit stack: an input-deep
        // tree never recurses natively. A subtree is a run of the order.
        let mut pre = vec![NONE; n];
        let mut order = Vec::with_capacity(n);
        let mut stack = Vec::new();
        for (i, &root) in topo.is_root.iter().enumerate() {
            if !root {
                continue;
            }
            stack.push(u32_of(i));
            // Each instruction has one parent, so none is reached twice.
            while let Some(x) = stack.pop() {
                let x = x as usize;
                pre[x] = u32_of(order.len());
                order.push(u32_of(x));
                stack.extend(kids[at[x] as usize..at[x + 1] as usize].iter().rev());
            }
        }
        // A subtree's size, children before parents in reverse preorder.
        let mut size = vec![1u32; n];
        for &x in order.iter().rev() {
            let p = parent(x as usize) as usize;
            if p < n {
                size[p] += size[x as usize];
            }
        }
        // `NONE` stays `NONE` for an instruction under no root.
        let last = (0..n).map(|i| pre[i].saturating_add(size[i] - 1)).collect();
        let (own_at, own) = buckets(n, |i| topo.loop_of[i]);
        crate::work::loop_scans(n);
        Self {
            pre,
            last,
            order,
            at,
            kids,
            own_at,
            own,
        }
    }

    /// The instructions whose innermost loop is `l`, in index order.
    pub(super) fn own(&self, l: u32) -> &[u32] {
        let l = l as usize;
        &self.own[self.own_at[l] as usize..self.own_at[l + 1] as usize]
    }

    /// The child of block `blk` whose subtree holds `j`, when `j` is
    /// strictly below `blk`: an interval test, then a binary search of
    /// the children, which are in preorder. O(log children).
    pub(super) fn child_above(&self, blk: u32, j: u32) -> Option<u32> {
        let (b, p) = (self.pre[blk as usize], self.pre[j as usize]);
        if b == NONE || p == NONE || p <= b || p > self.last[blk as usize] {
            return None;
        }
        let kids = self.children(blk);
        // The first child is numbered `b + 1`, at most `p`, so the
        // partition point is past it.
        let k = kids.partition_point(|&k| self.pre[k as usize] <= p);
        Some(kids[k - 1])
    }

    /// The children of instruction `i`: the blocks a non-block runs, the
    /// non-block instructions a block lists, in the order of their
    /// statements.
    fn children(&self, i: u32) -> &[u32] {
        let i = i as usize;
        &self.kids[self.at[i] as usize..self.at[i + 1] as usize]
    }

    /// Instruction `i` and every instruction below it, in preorder.
    fn subtree(&self, i: u32) -> &[u32] {
        let i = i as usize;
        &self.order[self.pre[i] as usize..=self.last[i] as usize]
    }

    /// Every instruction below `i`, in preorder.
    fn below(&self, i: u32) -> &[u32] {
        let i = i as usize;
        &self.order[self.pre[i] as usize + 1..=self.last[i] as usize]
    }
}

/// Instructions `0..n` grouped by `key` (an instruction, or `NONE` for
/// none) by counting sort: group `k` is `items[at[k]..at[k + 1]]`, in
/// index order. O(n).
fn buckets(n: usize, key: impl Fn(usize) -> u32) -> (Vec<u32>, Vec<u32>) {
    let mut at = vec![0u32; n + 1];
    for i in 0..n {
        let k = key(i) as usize;
        if k < n {
            at[k + 1] += 1;
        }
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

pub(super) fn u32_of(i: usize) -> u32 {
    // ruled out: TIR names instructions by `u32` words, so a body's
    // instruction count fits.
    u32::try_from(i).expect("instructions fit u32")
}

/// The local that value `r` reads when it is a plain local read,
/// `Read [$l]`; `None` for a constant or any other instruction.
pub(super) fn read_local(b: &Body, r: u32) -> Option<usize> {
    let i = Ref(r).as_inst()?.0 as usize;
    (i < b.len())
        .then(|| b.local_read(i))
        .flatten()
        .map(hd_base::LocalId::idx)
}

/// A `Store` `i` into a whole local: the local and the value it takes.
pub(super) fn local_store(b: &Body, i: usize) -> Option<(usize, u32)> {
    match b.stored_local(i)? {
        (l, true) => Some((l.idx(), b.data[i][1])),
        (_, false) => None,
    }
}

/// The list value whose element `Store` `i` writes (`[%xs [%i] ..]`), as
/// an element store intrinsic names its list.
pub(super) fn elem_store_list(b: &Body, i: usize) -> Option<u32> {
    if b.tags[i] != Tag::Store {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match (p.root, p.proj.first()) {
        (PlaceRoot::Value(r), Some(Proj::Elem(_))) => Some(r.0),
        _ => None,
    }
}

/// The local whose own place reference `i` names, `&x` or `&x.f`: a
/// `Ref` rooted at a local with only field projections.
pub(super) fn ref_local(b: &Body, i: usize) -> Option<usize> {
    if b.tags[i] != Tag::Ref {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match p.root {
        PlaceRoot::Local(l) if p.proj.iter().all(|s| matches!(s, Proj::Field(_))) => Some(l.idx()),
        _ => None,
    }
}

/// What a projection read of one step from a value reads, by the type
/// of that value (`Em::projected`), queried from its instantiated type.
/// A read through a reference is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Projected {
    /// No projection read.
    #[default]
    None,
    /// A field of a struct or newtype value (`[%v .k]`).
    Field,
    /// An element of a tuple (`[%v .k]`).
    Element,
    /// A field of an enum value's payload (`[%v as .V .k]`).
    Payload,
}

/// The value projection read `i` reads from, and what it reads
/// ([`Projected`]); `None` for any other instruction.
pub(super) fn projection(b: &Body, i: usize, projected: Projected) -> Option<(u32, Projected)> {
    match projected {
        Projected::None => None,
        p => b.read_step(i).map(|(v, _)| (v, p)),
    }
}

/// What a read through a reference reads through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Through {
    /// The reference a local holds.
    Local(usize),
    /// A reference value.
    Value(u32),
}

/// What a `Read` `i` whose place starts with a dereference reads
/// through, and whether it reads the whole referenced value. A data
/// object's field ([`Projected`]) is no read through a reference.
pub(super) fn read_through(b: &Body, i: usize, projected: Projected) -> Option<(Through, bool)> {
    match b.tags[i] {
        Tag::Read if projection(b, i, projected).is_none() => {
            let p = b.decode_place(b.data[i][0])?;
            if p.proj.first() != Some(&Proj::Deref) {
                return None;
            }
            let through = match p.root {
                PlaceRoot::Local(l) => Through::Local(l.idx()),
                PlaceRoot::Value(r) => Through::Value(r.0),
                _ => return None,
            };
            Some((through, p.proj.len() == 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body of one local: a store into it, a reference to it, a read
    /// and a write through that reference, then a read of the local.
    fn reference_body() -> Body {
        use hd_tir::ir::Place;
        fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
            let i = u32::try_from(b.tags.len()).unwrap();
            b.tags.push(tag);
            b.data.push([a, w]);
            b.ty.push(hd_types::Ty::VOID);
            b.syn.push(hd_base::NodeIdx::NONE);
            i
        }
        fn place(b: &mut Body, p: &Place) -> u32 {
            match (p.words(), p.root) {
                (Some(words), _) => b.push_record(&words),
                (None, PlaceRoot::Local(l)) => l.raw() | hd_tir::ir::place::BARE_LOCAL,
                (None, _) => unreachable!("only a bare local has no record"),
            }
        }
        let mut b = Body::new(hd_base::DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        b.local_ty.push(hd_types::Ty::VOID);
        b.local_name.push(hd_base::Symbol::from_raw(0));
        b.local_syn.push(hd_base::NodeIdx::NONE);
        b.local_flags.push(hd_tir::ir::local_flags::ASSIGNED);
        let local = Place::local(hd_base::LocalId::from_raw(0));
        let none = b.push_record(&[]);
        let value = inst(&mut b, Tag::NewTuple, NONE, none);
        let mut list = vec![value];
        let at = place(&mut b, &local);
        list.push(inst(&mut b, Tag::Store, at, value));
        let r = inst(&mut b, Tag::Ref, at, 1);
        let through = Place {
            root: PlaceRoot::Value(Ref(r)),
            proj: vec![Proj::Deref],
        };
        let at = place(&mut b, &through);
        let read = inst(&mut b, Tag::Read, at, 0);
        list.extend([r, read, inst(&mut b, Tag::Store, at, value)]);
        list.push(inst(&mut b, Tag::Read, hd_tir::ir::place::BARE_LOCAL, 0));
        let list = b.push_record(&list);
        let root = inst(&mut b, Tag::Block, list, NONE);
        b.sub_root.push(root);
        b.sub_parent.push(hd_base::SubId::NONE);
        b.sub_flags.push(0);
        b.sub_params.push(hd_base::Range32::default());
        b
    }

    /// The place helpers name the locals of `Read`, `Store` and `Ref`: a
    /// store into the whole local, the reference's root, and a read and
    /// a write through the reference, which reach no local.
    #[test]
    fn places_name_their_locals() {
        let new = reference_body();
        assert_eq!(local_store(&new, 1), Some((0, 0)));
        assert_eq!(ref_local(&new, 2), Some(0));
        assert_eq!(
            read_through(&new, 3, Projected::None),
            Some((Through::Value(2), true))
        );
        assert_eq!(local_store(&new, 4), None);
    }
}
