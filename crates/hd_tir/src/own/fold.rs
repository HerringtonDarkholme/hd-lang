//! Place folding, the pipeline's first step: a field read of a whole
//! local's read becomes one read of the field's place, `Read [$l .k]`
//! (`docs/compiler/tir-v2.md`, "Field reads": `p.x` is `Read [$0 .x]`).
//!
//! The checker still writes `%v = Read [$l]` then `Read [%v .k]`, because
//! the backend's own count decisions key on the plain local read
//! (#728's leftovers: places are rooted at locals by commit 12, when the
//! checker stops inserting copies). Insertion would copy all of `$l` for
//! the one field, and drop the copy after: on a struct of a thousand
//! values that is a thousand loads, a count of each leaf and a release of
//! each, per field read. So the passes root such a read at the local
//! first. A read folds when it is the very next statement after the local
//! read it projects from, and is that read's one use: nothing runs
//! between them, so the local holds the same value at both. Only struct,
//! tuple and newtype field steps fold, of a local whose type names no type
//! parameter (an erased body outlines an open read by its value). The
//! local read it replaces becomes an empty block with no source, which
//! nothing runs.

use super::{OwnCx, Role, Tri, operands};
use crate::ir::{Body, NONE, ReadMode, Tag, place};

/// The folding step's tables, reused across bodies.
#[derive(Default)]
pub(crate) struct Tables {
    uses: Vec<u32>,
    blk: Vec<u32>,
    pos: Vec<u32>,
    ops: Vec<super::Operand>,
    words: Vec<u32>,
}

/// The projection words of the place record at `word` after a root of
/// `kind`, when every one is a field step.
fn field_steps(b: &Body, word: u32, kind: u32) -> Option<(u32, &[u32])> {
    let n = *b.extra.get(word as usize)? as usize;
    let rec = b.extra.get(word as usize + 1..word as usize + 1 + n)?;
    let [k, root, steps @ ..] = rec else {
        return None;
    };
    (*k == kind
        && steps
            .iter()
            .all(|w| w & place::PROJ_KIND == place::PROJ_FIELD))
    .then_some((*root, steps))
}

/// The local and field path a read reads, when its place is a local with
/// only field steps (`[$l]`, `[$l .a .b]`); appends the steps to `out`.
fn local_fields(b: &Body, word: u32, out: &mut Vec<u32>) -> Option<u32> {
    if word & place::BARE_LOCAL != 0 {
        return Some(word & !place::BARE_LOCAL);
    }
    let (l, steps) = field_steps(b, word, place::ROOT_LOCAL)?;
    out.extend(steps.iter().map(|w| w & place::PROJ_INDEX));
    Some(l)
}

/// Folds every field read of a whole local's read that directly follows
/// it into one read of the local's field place.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, t: &mut Tables) {
    let n = b.len();
    t.uses.clear();
    t.uses.resize(n, 0);
    t.blk.clear();
    t.blk.resize(n, NONE);
    t.pos.clear();
    t.pos.resize(n, 0);
    let mut ops = std::mem::take(&mut t.ops);
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped {
                t.uses[op.value as usize] += 1;
            }
        }
        if b.tags[i] == Tag::Block {
            for (p, &m) in b.record(b.data[i][0]).iter().enumerate() {
                if (m as usize) < n {
                    t.blk[m as usize] = u32::try_from(i).expect("insts");
                    t.pos[m as usize] = u32::try_from(p).expect("positions");
                }
            }
        }
    }
    t.ops = ops;
    let mut empty = NONE;
    for i in 0..n {
        if b.tags[i] != Tag::Read || b.data[i][1] != ReadMode::Use as u32 {
            continue;
        }
        let word = b.data[i][0];
        if word & place::BARE_LOCAL != 0 {
            continue;
        }
        let Some((r, steps)) = field_steps(b, word, place::ROOT_VALUE) else {
            continue;
        };
        let v = r as usize;
        if steps.is_empty()
            || r & crate::ir::Ref::CONST_BIT != 0
            || v >= n
            || b.tags[v] != Tag::Read
            || b.data[v][1] != ReadMode::Use as u32
            || t.uses[v] != 1
            || t.blk[v] == NONE
            || t.blk[i] != t.blk[v]
            || t.pos[i] != t.pos[v] + 1
        {
            continue;
        }
        t.words.clear();
        let Some(l) = local_fields(b, b.data[v][0], &mut t.words) else {
            continue;
        };
        if cx.counted(b.local_ty[l as usize]) == Tri::Maybe {
            continue;
        }
        let (_, steps) = field_steps(b, word, place::ROOT_VALUE).expect("checked above");
        t.words.extend(steps.iter().map(|w| w & place::PROJ_INDEX));
        // The folded place: `[$l .path]`.
        let at = u32::try_from(b.extra.len()).expect("extra");
        b.extra
            .push(u32::try_from(2 + t.words.len()).expect("place"));
        b.extra.extend([place::ROOT_LOCAL, l]);
        b.extra.extend(
            t.words
                .iter()
                .map(|k| place::PROJ_FIELD | (k & place::PROJ_INDEX)),
        );
        b.data[i][0] = at;
        // The whole local's read is no longer read: an empty block.
        if empty == NONE {
            empty = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(0);
        }
        b.tags[v] = Tag::Block;
        b.data[v] = [empty, NONE];
        b.ty[v] = hd_types::Ty::VOID;
        b.syn[v] = hd_base::NodeIdx::NONE;
        t.uses[v] = 0;
    }
}
