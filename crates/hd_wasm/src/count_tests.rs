//! The test before a count call. Most retains and releases at run time find
//! no count to change: a null, static data, a frame slot. A call that only
//! finds that out costs more than the test, so each count call first asks
//! what its call would answer:
//!
//! - a known payload start ([`Helper::RetainKnown`], [`Helper::ReleaseKnown`]):
//!   the address is not null and its header's count is not 0. A release also
//!   calls while the collector traces, where a gray object's count may have
//!   reached 0 and `visit` must still see the reference. An acyclic module
//!   has no collector;
//! - any other address: it is not null. Finding the block needs the page
//!   table, so a frame slot or static data still reaches the call.
//!
//! The runtime's own helpers keep their plain calls. A test pays only where it
//! skips, and a helper walks the fields of an object it holds, whose addresses
//! are real counts: `binary_trees` skips none of its 30M releases, and wrapping
//! the helpers made it 10% slower with nothing to skip. So the test goes where
//! a value's origin is unknown, in compiled bodies.
//!
//! A checked build runs the same test and tells its runtime when one skips a
//! call, so the skipped work stays on the count table (`hd.skipped`).
//!
//! A test changes the size of the body it stands in, and on V8 that alone can
//! move another body's speed by several percent (#1434). `progs/list-sort`
//! runs 15 count operations, and its `sort`, called once, runs its whole loop
//! in Liftoff code, since V8 has no on-stack replacement for Wasm. The test in
//! `main` makes `main`'s Liftoff code 56 bytes longer, which moves where
//! `sort`'s byte-identical code lands. The result's sign follows the layout,
//! not the test: with the test, 384 against 403 ms at the default tier, 388
//! against 355 with `--liftoff-only`, 454 against 456 with `--no-liftoff`.

use crate::module::{Func, Global, HEAP, Ins, Sym, Val, mem};
use crate::op::Op as I;
use crate::rt::Helper;
use hd_base::Hash128;
use wasm_encoder::BlockType;

/// What the link knows about a module's count calls.
#[derive(Clone, Copy)]
pub(crate) struct Counts {
    /// The runtime's `retain`, called directly.
    pub retain: Hash128,
    /// No object takes part in cycle collection (§23.28).
    pub acyclic: bool,
    /// `inline_counts` has already lowered the known forms.
    pub known_inlined: bool,
    /// A checked build's `skipped_test`.
    pub skipped: Option<Hash128>,
}

#[derive(Clone, Copy)]
enum Kind {
    KnownRetain,
    KnownRelease,
    Other,
}

fn kind(f: &Func, ins: &Ins, c: Counts) -> Option<Kind> {
    let Ins::Call(id) = ins else {
        return None;
    };
    match &*f.symbol(*id) {
        Sym::Helper(Helper::RetainKnown) if !c.known_inlined => Some(Kind::KnownRetain),
        Sym::Helper(Helper::ReleaseKnown) if !c.known_inlined => Some(Kind::KnownRelease),
        Sym::Helper(Helper::ReleaseRef) => Some(Kind::Other),
        Sym::Inst(i) if *i == c.retain => Some(Kind::Other),
        _ => None,
    }
}

/// Puts each count call of `f` behind its test. Gives whether any changed.
pub(crate) fn lower(f: &mut Func, c: Counts) -> bool {
    if !f.body.iter().any(|i| kind(f, i, c).is_some()) {
        return false;
    }
    let t = f.local(Val::I32);
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    let close = |f: &mut Func| {
        if let Some(skipped) = c.skipped {
            f.op(I::Else);
            f.call(Sym::Inst(skipped));
        }
        f.op(I::End);
    };
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        let Some(form) = kind(f, &ins, c) else {
            f.push(ins);
            continue;
        };
        f.op(I::LocalTee(t));
        f.op(I::If(BlockType::Empty));
        let known = !matches!(form, Kind::Other);
        if known {
            f.get(t);
            f.i32(8);
            f.op(I::I32Sub);
            f.op(I::I32Load(mem(HEAP, 0, 2)));
            if matches!(form, Kind::KnownRelease) && !c.acyclic {
                f.global_get(Global::Tracing);
                f.op(I::I32Or);
            }
            f.op(I::If(BlockType::Empty));
        }
        f.get(t);
        f.push(ins);
        if known {
            close(f);
        }
        close(f);
    }
    f.origin_at = outer;
    true
}
