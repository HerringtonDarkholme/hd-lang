//! Releases at last use (`crates/hd_wasm/DESIGN.md` §23.20, job 12). A
//! reference is released after its last use, not at the end of its block
//! (`OBJECT_MODEL_TIER1.md` §6.3 rule 5), so a counted value whose last use
//! has passed must give up its count there, or an element reference would
//! pin its list until the function returns. Within one body, at statement
//! granularity: an instruction's value is released after the statement of
//! its own block that holds its last use, and a TIR local after the
//! statement of the innermost block holding all its uses that holds the
//! last. Each place is zeroed once released, so the function's exit, which
//! still releases every place, releases it as nothing. A value used where
//! no statement of its block holds the use (a block's result, a match
//! decision's value read in an arm) keeps its count to the exit.

use hd_base::HashMap;
use hd_tir::ir::{Body, NONE, Op, Ref, Tag};

/// What to release after a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Drop {
    /// An instruction's value.
    Val(u32),
    /// A TIR local.
    Local(usize),
}

/// The releases to make after each statement, keyed by the statement's
/// block and its position in that block.
pub(super) type Drops = HashMap<(u32, usize), Vec<Drop>>;

/// The block structure of a body: each instruction's block and position,
/// and each block's owning instruction.
struct Tree {
    /// The block whose list holds each instruction, and its position.
    parent: Vec<(u32, usize)>,
    /// The instruction that runs each block (an `If`, a `Loop`, ...).
    owner: Vec<u32>,
}

impl Tree {
    fn new(b: &Body) -> Tree {
        let n = b.len();
        let mut parent = vec![(NONE, 0); n];
        let mut owner = vec![NONE; n];
        for i in 0..n {
            let [a, w] = b.data[i];
            let tag = b.tags[i];
            if tag == Tag::Block {
                for (p, &c) in b.record(a).iter().enumerate() {
                    if let Some(x) = parent.get_mut(c as usize) {
                        *x = (u32::try_from(i).expect("insts"), p);
                    }
                }
                continue;
            }
            let mut blocks = Vec::new();
            for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
                match op {
                    Op::Block => blocks.push(word),
                    Op::Blocks if word != NONE => blocks.extend_from_slice(b.record(word)),
                    _ => {}
                }
            }
            if matches!(
                tag,
                Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
            ) {
                blocks.extend(b.record(w).iter().skip(1).take(2));
            }
            for blk in blocks {
                if b.tags.get(blk as usize) == Some(&Tag::Block) {
                    owner[blk as usize] = u32::try_from(i).expect("insts");
                }
            }
        }
        Tree { parent, owner }
    }

    /// The blocks around instruction `x`, innermost first, each with the
    /// position of the statement in it that holds `x`; `None` when the
    /// chain does not reach a sub-body's root (a closure's own body).
    fn path(&self, b: &Body, mut x: u32) -> Option<Vec<(u32, usize)>> {
        let mut out = Vec::new();
        loop {
            if b.tags[x as usize] == Tag::Block {
                if b.sub_root.contains(&x) {
                    return Some(out);
                }
                x = self.owner[x as usize];
                if x == NONE {
                    return None;
                }
                continue;
            }
            let (blk, p) = self.parent[x as usize];
            if blk == NONE {
                return None;
            }
            out.push((blk, p));
            x = blk;
        }
    }
}

/// The instruction values instruction `i` reads: TIR's own list where it
/// has one, else every operand word that names an instruction, an
/// over-approximation that only ever delays a release.
fn uses(b: &Body, i: usize) -> Vec<u32> {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    // A block's record is its statements; only its tail is a use.
    if tag == Tag::Block {
        return if w != NONE && Ref(w).as_inst().is_some() {
            vec![w]
        } else {
            vec![]
        };
    }
    let mut out = if let Some(mut v) = b.value_operands(i) {
        // A call's last three words are its providers: a range of key and
        // value pairs (1), or a context value (2).
        if tag == Tag::Call {
            let r = b.record(w);
            if let [.., kind, x, n] = *r {
                match kind {
                    1 => v.extend(b.extra.iter().skip(x as usize).take(n as usize)),
                    2 => v.push(x),
                    _ => {}
                }
            }
        }
        v
    } else if matches!(tag, Tag::With | Tag::ContextNew) {
        // Keys and values in pairs.
        b.record(a).iter().skip(1).step_by(2).copied().collect()
    } else {
        let mut out = Vec::new();
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value => out.push(word),
                Op::Values if word != NONE => out.extend_from_slice(b.record(word)),
                // Only these records hold values; a closure's holds its
                // captures' range, a switch's its case and arms.
                Op::Record
                    if word != NONE
                        && matches!(
                            tag,
                            Tag::CallDyn
                                | Tag::Await
                                | Tag::NewList
                                | Tag::ForRange
                                | Tag::ForList
                                | Tag::ForMap
                        ) =>
                {
                    out.extend_from_slice(b.record(word));
                }
                _ => {}
            }
        }
        out
    };
    out.retain(|&r| r != NONE && Ref(r).as_inst().is_some() && (r as usize) < b.len());
    out
}

/// The releases of sub-body `sub`'s values and locals at their last uses.
/// `entry` are the locals that hold a value from the prologue: the
/// parameters, and a closure's captures.
pub(super) fn plan(b: &Body, sub: u32, entry: &[usize]) -> Drops {
    let tree = Tree::new(b);
    let subs = b.inst_subs();
    let mut drops = Drops::default();
    let mut users: HashMap<u32, Vec<u32>> = HashMap::default();
    let mut access: HashMap<usize, Vec<u32>> = HashMap::default();
    for (i, &s) in subs.iter().enumerate() {
        if s != sub {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        for r in uses(b, i) {
            users.entry(r).or_default().push(x);
        }
        match b.tags[i] {
            Tag::LocalGet | Tag::LocalSet => {
                access.entry(b.data[i][0] as usize).or_default().push(x);
            }
            // A reference reads the locals its place starts from.
            Tag::RefPlace => {
                for l in b.place_locals(i) {
                    access.entry(l as usize).or_default().push(x);
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                let (start, n) = (rec[0] as usize, rec[1] as usize);
                for l in &b.cap_local[start..start + n] {
                    access.entry(l.raw() as usize).or_default().push(x);
                }
            }
            _ => {}
        }
    }
    // Values: the last statement of the defining block that holds a use.
    for (i, &s) in subs.iter().enumerate() {
        if s != sub || b.tags[i] == Tag::Block {
            continue;
        }
        let x = u32::try_from(i).expect("insts");
        let (blk, def) = tree.parent[i];
        if blk == NONE {
            continue;
        }
        let mut last = def;
        let mut ok = true;
        for &u in users.get(&x).map_or(&[][..], Vec::as_slice) {
            match tree
                .path(b, u)
                .and_then(|p| p.into_iter().find(|(k, _)| *k == blk))
            {
                Some((_, p)) => last = last.max(p),
                None => ok = false,
            }
        }
        if ok {
            drops.entry((blk, last)).or_default().push(Drop::Val(x));
        }
    }
    // Locals: the innermost block that holds every access, after the last
    // statement there that holds one.
    for (l, xs) in access {
        let paths: Option<Vec<Vec<(u32, usize)>>> = xs.iter().map(|&x| tree.path(b, x)).collect();
        let Some(paths) = paths else {
            continue;
        };
        let Some(first) = paths.first() else {
            continue;
        };
        // The blocks of the first path, outermost first, that every path
        // passes through; the last of them is the innermost common one.
        let common = first
            .iter()
            .rev()
            .take_while(|(k, _)| paths.iter().all(|p| p.iter().any(|(q, _)| q == k)));
        let Some(&(mut blk, _)) = common.last() else {
            continue;
        };
        // A parameter or a capture holds its value from the prologue, so
        // it lives in the root block, never only in a loop's body.
        if entry.contains(&l) {
            blk = b.sub_root[sub as usize];
        }
        let last = paths
            .iter()
            .filter_map(|p| p.iter().find(|(k, _)| *k == blk).map(|(_, p)| *p))
            .max()
            .unwrap_or(0);
        drops.entry((blk, last)).or_default().push(Drop::Local(l));
    }
    drops
}
