//! The ownership passes (`docs/compiler/tir-v2.md`, "The ownership
//! passes"): a fixed pipeline over one body that decides every count once
//! per generic body, after decoding and before any instance is emitted.
//! The checker's TIR reads every place with mode `use` and has no
//! ownership statement (invariant 22); the passes leave every read's mode
//! decided, the `Drop`s each owned value and local needs, and the exit
//! set (invariant 23, [`crate::ir::verify_owned`]).
//!
//! Built so far: **insertion** (`insert`), over places the checker roots
//! at their local on this path (`crate::ir::root_places`), the naive,
//! correct ownership of `crates/hd_wasm_linear/DESIGN.md` §23.16 with
//! releases at last use (§23.20), made explicit, with an `IsUnique` before each change of a
//! collection place and the pins of element references; then **borrow**
//! (`borrow`), which turns a `copy` whose value can live on its place's
//! count into a `borrow` and deletes its `Drop`s; then **move** (`moves`),
//! which turns a `copy` that no path reads after into a `move` and deletes
//! the `Drop` it leaves with nothing to release; then **pair
//! cancellation** (`pairs`), which turns a `copy` of a part whose base's
//! `Drop` follows with nothing in between that touches the part into a
//! `move`, and deletes or splits that drop; then **known-null** (`known`),
//! which deletes the `Drop`s no path reaches with a count, drops the exit
//! rows of locals empty at every way out, and writes each suspension
//! point's unwind set; then **hoisting** (`hoist`), which moves an
//! `IsUnique` out of a loop that cannot make a second owner of its place,
//! to run once before it under the loop's entry test, and lists the
//! loops' stable list places. Each pass only weakens, deletes or moves
//! what insertion writes, so the pipeline is correct after any prefix.
//! Insertion rebuilds the body once ([`splice`]); borrow, move, pair
//! cancellation, known-null and hoisting record their edits for one more
//! rebuild at the end.
//!
//! The passes read the body and [`OwnCx`], signature and layout facts the
//! driver gives; never another body. They follow "Data layout of the
//! passes": dense tables indexed by instruction and local in one
//! [`OwnScratch`] reused across bodies, no hash maps, and every edit
//! recorded in a side table and spliced in once ([`splice`]).

use hd_types::Ty;

use crate::ir::{Body, Callee, IntrinsicOp, NONE, Op, Ref, Tag, place};

pub use hoist::{EntryTest, entry_test};
pub(crate) use splice::for_blocks as splice_for_blocks;

mod borrow;
pub(crate) mod hoist;
mod insert;
mod known;
mod moves;
mod pairs;
pub(crate) mod splice;

/// Whether a type holds a counted leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    No,
    Yes,
    /// It mentions a type parameter or an associated type, so it depends
    /// on the instance: the emitter drops the glue for a scalar one.
    Maybe,
}

/// How a call passes one argument, by the callee's signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// A value parameter: the callee owns the argument's count, so the
    /// call consumes it.
    Owned,
    /// A reference parameter, a suspending callee's argument (its cold
    /// constructor counts what it keeps), an `@intrinsic` callee's
    /// argument (the backend lowers the call in place and counts what it
    /// keeps), or a parameter whose mode depends on the instance: the
    /// caller keeps its value.
    Lent,
}

/// The facts the passes read besides the body (`docs/compiler/tir-v2.md`,
/// "What the passes read"): layout and signature facts, never another
/// body's.
pub trait OwnCx {
    /// Whether a value of `ty` holds a counted leaf.
    fn counted(&self, ty: Ty) -> Tri;
    /// How argument `k` of `n` of a `Call` or `Await` of `callee` is
    /// passed. `Lent` whenever it is not certainly `Owned`: a lent
    /// argument the callee's instance owns takes a count at the call
    /// (`layout::Pass` in the backend).
    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass;
    /// [`OwnCx::pass`] for a `CallValue` of a value of function type
    /// `fn_ty`.
    fn pass_value(&self, fn_ty: Ty, k: usize, n: usize) -> Pass;
    /// Whether a call of `callee` may reach memory outside its arguments:
    /// false only for the few std functions known pure. The borrow pass
    /// and hoisting read it.
    fn may_write(&self, callee: &Callee) -> bool;
    /// The parts of a value of `ty` that is laid out in place in every
    /// instance, into `out`: the field types of a struct, a tuple or a
    /// newtype (`variant` `None`), or of variant `variant` of an enum that
    /// does not contain itself. False for any other type: a scalar, a
    /// string, a collection, a reference, a data type, a recursive enum,
    /// and a type that names a type parameter or an associated type, whose
    /// value is one counted view in an erased instance. The move pass
    /// splits a base's `Drop` into its other parts' only when this holds.
    fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool;
    /// How the body of `item` receives its own parameter `k` of `n`:
    /// `Lent` when its caller keeps the argument alive for the call, so the
    /// body need not own it ([`OwnCx::pass`]'s `Lent`, for a function that
    /// does not suspend); `Owned` for a value parameter, for every
    /// parameter of a suspending function, whose frame holds a count of
    /// each, and for a body that is no function's. The borrow pass leaves
    /// a lent parameter the body never stores unowned.
    fn received(&self, item: hd_base::DefId, k: usize, n: usize) -> Pass;
}

/// How an instruction uses one of its value operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The use reads the value and keeps nothing: an operand of `Prim`, a
    /// place's root or element index, a borrowed argument.
    Lend,
    /// The use takes the value's count: a stored value (a list push's and
    /// a map store's too), a construction part, a `Return` or `Break`
    /// value.
    Consume,
    /// Argument `k` of a call, which consumes it when its parameter is a
    /// value parameter ([`OwnCx::pass`]).
    Arg(u32),
    /// A block's tail, which its block's value takes when the block's
    /// owner uses that value.
    Tail,
    /// The place of a `Drop`: no use, the value's end.
    Dropped,
}

/// Where an operand word sits, so a pass can rewrite it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    /// Data word `k` of the instruction.
    Data(u8),
    /// A word of `extra`.
    Extra(u32),
}

/// One value operand of an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operand {
    pub value: u32,
    pub role: Role,
    pub at: Loc,
}

/// The value operands instruction `i` reads, in emission order, each with
/// its role and position: the operands of `Body::value_operands`, a call's
/// providers, `With`'s and `ContextNew`'s provider values, a block's tail,
/// and the value operands of control flow (a condition, a scrutinee, a
/// `Return` or `Break` value). Constants and `NONE` are left out. One
/// definition serves the passes, their verifier and the emitter, so all
/// three agree on what a use is.
pub fn operands(b: &Body, i: usize, out: &mut Vec<Operand>) {
    out.clear();
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    let push = |out: &mut Vec<Operand>, value: u32, role: Role, at: Loc| {
        if value != NONE && Ref(value).as_inst().is_some() && (value as usize) < b.len() {
            out.push(Operand { value, role, at });
        }
    };
    let record = |at: u32| -> (u32, u32) {
        let n = b.extra.get(at as usize).copied().unwrap_or(0);
        (at + 1, n)
    };
    match tag {
        Tag::Block => push(out, w, Role::Tail, Loc::Data(1)),
        Tag::Read | Tag::Ref | Tag::Store | Tag::Drop | Tag::IsUnique | Tag::Pin => {
            let role = if tag == Tag::Drop {
                Role::Dropped
            } else {
                Role::Lend
            };
            if a & place::BARE_LOCAL == 0 {
                place_words(b, a, |value, at| push(out, value, role, at));
            }
            if tag == Tag::Store {
                push(out, w, Role::Consume, Loc::Data(1));
            }
        }
        Tag::Call | Tag::Await => {
            let (start, n) = record(w);
            let args = n.saturating_sub(3);
            for k in 0..args {
                let at = start + k;
                push(out, b.extra[at as usize], Role::Arg(k), Loc::Extra(at));
            }
            // The providers: (key, value) pairs at `x` (1), or a context
            // value (2).
            if n >= 3 {
                let kind = b.extra[(start + n - 3) as usize];
                let x = b.extra[(start + n - 2) as usize];
                let len = b.extra[(start + n - 1) as usize];
                match kind {
                    1 => {
                        for k in (1..len).step_by(2) {
                            let at = x + k;
                            if let Some(&v) = b.extra.get(at as usize) {
                                push(out, v, Role::Lend, Loc::Extra(at));
                            }
                        }
                    }
                    2 => push(out, x, Role::Lend, Loc::Extra(start + n - 2)),
                    _ => {}
                }
            }
        }
        Tag::CallValue => {
            push(out, a, Role::Lend, Loc::Data(0));
            let (start, n) = record(w);
            for k in 0..n {
                let at = start + k;
                push(out, b.extra[at as usize], Role::Arg(k), Loc::Extra(at));
            }
        }
        Tag::NewStruct | Tag::NewVariant | Tag::NewMap => {
            values(b, w, &mut |v, at| push(out, v, Role::Consume, at));
        }
        Tag::NewTuple => {
            values(b, w, &mut |v, at| push(out, v, Role::Consume, at));
            push(out, a, Role::Consume, Loc::Data(0));
        }
        Tag::NewList => {
            values(b, w, &mut |v, at| push(out, v, Role::Consume, at));
        }
        Tag::CopyStruct => {
            push(out, a, Role::Lend, Loc::Data(0));
            let (start, n) = record(w);
            for k in (1..n).step_by(2) {
                let at = start + k;
                push(out, b.extra[at as usize], Role::Consume, Loc::Extra(at));
            }
        }
        Tag::With | Tag::ContextNew => {
            let (start, n) = record(a);
            for k in (1..n).step_by(2) {
                let at = start + k;
                push(out, b.extra[at as usize], Role::Lend, Loc::Extra(at));
            }
        }
        Tag::Return => push(out, a, Role::Consume, Loc::Data(0)),
        Tag::Break => push(out, w, Role::Consume, Loc::Data(1)),
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
            push(out, a, Role::Lend, Loc::Data(0));
        }
        _ => {
            for (k, (word, op)) in [(a, tag.operands().0), (w, tag.operands().1)]
                .into_iter()
                .enumerate()
            {
                match op {
                    Op::Value => push(out, word, Role::Lend, Loc::Data(u8::from(k == 1))),
                    Op::Values if word != NONE => {
                        values(b, word, &mut |v, at| push(out, v, Role::Lend, at));
                    }
                    Op::Record if word != NONE && matches!(tag, Tag::Intrinsic | Tag::Interp) => {
                        // The value a list push or a map store puts in
                        // the collection, which keeps it: a store.
                        let stored = match IntrinsicOp::from_u32(a) {
                            _ if tag != Tag::Intrinsic => NONE,
                            Some(IntrinsicOp::ListPush) => 1,
                            Some(IntrinsicOp::MapSet) => 2,
                            _ => NONE,
                        };
                        let mut k = 0;
                        values(b, word, &mut |v, at| {
                            let role = if k == stored {
                                Role::Consume
                            } else {
                                Role::Lend
                            };
                            k += 1;
                            push(out, v, role, at);
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Each word of the record at `at`, as a value operand.
fn values(b: &Body, at: u32, f: &mut dyn FnMut(u32, Loc)) {
    if at == NONE {
        return;
    }
    let n = b.extra.get(at as usize).copied().unwrap_or(0);
    for k in 0..n {
        let x = at + 1 + k;
        if let Some(&v) = b.extra.get(x as usize) {
            f(v, Loc::Extra(x));
        }
    }
}

/// The value words of the place record at `at`: a value root, then each
/// element index, with their positions.
pub(crate) fn place_words(b: &Body, at: u32, mut f: impl FnMut(u32, Loc)) {
    let Some(&n) = b.extra.get(at as usize) else {
        return;
    };
    let rec = at + 1;
    let end = rec + n;
    let word = |x: u32| b.extra.get(x as usize).copied().unwrap_or(0);
    let mut x = match word(rec) {
        place::ROOT_VALUE => {
            f(word(rec + 1), Loc::Extra(rec + 1));
            rec + 2
        }
        place::ROOT_GLOBAL => rec + 3,
        _ => rec + 2,
    };
    while x < end {
        if word(x) & place::PROJ_KIND == place::PROJ_ELEM {
            f(word(x + 1), Loc::Extra(x + 1));
            x += 2;
        } else {
            x += 1;
        }
    }
}

/// The block owners whose value takes their block's tail: the arms of an
/// `If`, a `Match` and a switch (a switch outside a decision tree has a
/// value, as a generated body's do), a cleanup scope's and a `With`'s
/// body, a `RefDyn`'s address form, the right block of `And` and `Or`,
/// and a `Guard`'s condition. A loop's body, a match's decision block, a
/// `Guard`'s arms and a cleanup scope's suites end in no value anyone
/// reads.
pub(crate) fn owner_takes_tail(b: &Body, owner: u32, blk: u32) -> bool {
    let Some(&tag) = b.tags.get(owner as usize) else {
        return false;
    };
    let [a, w] = b.data[owner as usize];
    match tag {
        Tag::If
        | Tag::With
        | Tag::RefDyn
        | Tag::And
        | Tag::Or
        | Tag::SwitchTag
        | Tag::SwitchInt
        | Tag::SwitchChar
        | Tag::SwitchStr => true,
        Tag::Match => b.record(w).first() != Some(&blk),
        Tag::Scope => a == blk,
        Tag::Guard => a == blk,
        _ => false,
    }
}

/// Whether instruction `i`'s value is one the passes track: an owned
/// value that may hold a counted leaf. A block's value is its owner's, and
/// a statement has none; a switch has one outside a decision tree. A
/// `borrow` read owns no count.
pub(crate) fn tracked(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    let t = b.ty[i];
    if t == Ty::VOID || t == Ty::NEVER || t == Ty::POISON {
        return false;
    }
    if b.tags[i] == Tag::Read && b.data[i][1] == crate::ir::ReadMode::Borrow as u32 {
        return false;
    }
    if matches!(
        b.tags[i],
        Tag::Block
            | Tag::Store
            | Tag::Drop
            | Tag::IsUnique
            | Tag::Pin
            | Tag::FlagSet
            | Tag::FlagClear
            | Tag::Defer
            | Tag::Hook
            | Tag::Break
            | Tag::Continue
            | Tag::Return
            | Tag::Unreachable
            | Tag::ToArm
            | Tag::Guard
            | Tag::Hole
            | Tag::Poison
    ) {
        return false;
    }
    cx.counted(t) != Tri::No
}

/// Whether operand `op` of instruction `i` consumes its value
/// (`docs/compiler/tir-v2.md`, "Where values are owned"). For a block's
/// tail, `owner` is the block's owner, `NONE` for a sub-body's root.
pub(crate) fn consumes(b: &Body, cx: &dyn OwnCx, i: usize, op: &Operand, owner: u32) -> bool {
    match op.role {
        Role::Consume => true,
        Role::Lend | Role::Dropped => false,
        // `NONE`: a sub-body's root, whose tail is the body's result.
        Role::Tail => owner == NONE || owner_takes_tail(b, owner, u32::try_from(i).expect("insts")),
        Role::Arg(k) => {
            let [a, w] = b.data[i];
            let n = b.extra.get(w as usize).copied().unwrap_or(0) as usize;
            match b.tags[i] {
                Tag::Call | Tag::Await => {
                    let Some(callee) = Callee::from_words(b.record(a)) else {
                        return false;
                    };
                    // A default argument's body reads the earlier arguments
                    // themselves (invariant 15), so the call lends those.
                    let args = &b.record(w)[..n.saturating_sub(3)];
                    let read_by_default = args.iter().any(|&d| {
                        Ref(d).as_inst().is_some()
                            && b.tags.get(d as usize) == Some(&Tag::DefaultCall)
                            && b.record(b.data[d as usize][1]).contains(&op.value)
                    });
                    !read_by_default
                        && cx.pass(&callee, k as usize, n.saturating_sub(3)) == Pass::Owned
                }
                Tag::CallValue => {
                    let fn_ty = match Ref(a).as_inst() {
                        Some(x) => b.ty[x.0 as usize],
                        None => return false,
                    };
                    cx.pass_value(fn_ty, k as usize, n) == Pass::Owned
                }
                _ => false,
            }
        }
    }
}

/// Each value's consumer in a body after the passes, or `NONE`: the one
/// use of a tracked value that consumes it as a statement of the value's
/// own block, or as that block's tail which the block's owner takes. This
/// is insertion's rule, which gives every other owned value a `Drop`
/// instead; a backend that emits the body reads it from here, so the two
/// always agree on which use takes a count.
pub fn consumers(b: &Body, cx: &dyn OwnCx, out: &mut Vec<u32>) {
    let n = b.len();
    let mut blk = vec![NONE; n];
    let mut owner = vec![NONE; n];
    let mut is_root = vec![false; n];
    for i in 0..n {
        if b.tags[i] == Tag::Block {
            for &m in b.record(b.data[i][0]) {
                if let Some(x) = blk.get_mut(m as usize) {
                    *x = u32::try_from(i).expect("insts");
                }
            }
        }
        let defer = b.tags[i] == Tag::Defer;
        splice::for_blocks(b, i, |x| {
            if !(defer && owner[x as usize] != NONE) {
                owner[x as usize] = u32::try_from(i).expect("insts");
            }
        });
    }
    for &r in &b.sub_root {
        if let Some(x) = is_root.get_mut(r as usize) {
            *x = true;
        }
    }
    // Per value: how many uses, and the last one with its operand.
    let mut count = vec![0u32; n];
    let mut last = vec![(NONE, None::<Operand>); n];
    let mut ops = Vec::new();
    for i in 0..n {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role == Role::Dropped {
                continue;
            }
            let v = op.value as usize;
            count[v] += 1;
            last[v] = (u32::try_from(i).expect("insts"), Some(*op));
        }
    }
    out.clear();
    out.resize(n, NONE);
    for v in 0..n {
        let (u, Some(op)) = last[v] else {
            continue;
        };
        // A borrow owns no count to give.
        let borrow = b.tags[v] == Tag::Read && b.data[v][1] == crate::ir::ReadMode::Borrow as u32;
        if count[v] != 1 || blk[v] == NONE || borrow || !tracked(b, cx, v) {
            continue;
        }
        let (direct, tail_owner) = if op.role == Role::Tail {
            (
                u == blk[v],
                if is_root[u as usize] {
                    NONE
                } else {
                    owner[u as usize]
                },
            )
        } else {
            (blk[u as usize] == blk[v], NONE)
        };
        if direct && consumes(b, cx, u as usize, &op, tail_owner) {
            out[v] = u;
        }
    }
}

/// The tables the passes reuse across bodies: the driver keeps one for
/// the whole run (`docs/compiler/tir-v2.md`, "Data layout of the passes").
#[derive(Default)]
pub struct OwnScratch {
    pub(crate) borrow: borrow::Tables,
    pub(crate) hoist: hoist::Tables,
    pub(crate) insert: insert::Tables,
    pub(crate) known: known::Tables,
    pub(crate) moves: moves::Tables,
    pub(crate) pairs: pairs::Tables,
    pub(crate) splice: splice::Tables,
}

/// Runs the ownership passes over every sub-body of `b`, in order:
/// insertion, borrow, move, pair cancellation, known-null, then hoisting.
/// The last five record their deletions and insertions in one set of
/// edits, applied once at the end. A body the passes cannot handle is
/// refused, with what stopped them.
pub fn run(b: &mut Body, cx: &dyn OwnCx, scratch: &mut OwnScratch) -> Result<(), String> {
    insert::run(b, cx, scratch)?;
    borrow::run(b, cx, &mut scratch.borrow, &mut scratch.splice);
    moves::run(b, cx, &mut scratch.moves, &mut scratch.splice);
    pairs::run(
        b,
        cx,
        &mut scratch.moves,
        &mut scratch.pairs,
        &mut scratch.splice,
    );
    known::run(b, cx, &mut scratch.known, &mut scratch.splice);
    hoist::run(b, cx, &mut scratch.hoist, &mut scratch.splice);
    splice::apply(b, &mut scratch.splice)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{OwnScratch, Pass, Tri, run};
    use crate::ir::{
        Body, BodyKind, Callee, NONE, Providers, ReadMode, Tag, TirBuilder, TirSink, local_flags,
        verify_owned,
    };
    use hd_base::{DefId, LocalId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    /// `string` is counted; callee 9 takes its argument as a value
    /// parameter, callee 8 by reference.
    struct Cx;

    impl super::OwnCx for Cx {
        fn counted(&self, ty: Ty) -> Tri {
            if ty == Ty::STRING { Tri::Yes } else { Tri::No }
        }
        fn pass(&self, callee: &Callee, _: usize, _: usize) -> Pass {
            match callee {
                Callee::Item { def, .. } if *def == DefId::from_raw(9) => Pass::Owned,
                _ => Pass::Lent,
            }
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
        fn may_write(&self, _: &Callee) -> bool {
            true
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Owned
        }
    }

    fn call(b: &mut TirBuilder, def: u32, args: &[crate::ir::Ref], ty: Ty) -> crate::ir::Ref {
        let callee = Callee::Item {
            def: DefId::from_raw(def),
            targs: TyList::EMPTY,
        };
        b.call(&callee, args, Providers::None, ty, NodeIdx::from_raw(1))
    }

    /// A body of one root block built by `f`, run through the passes and
    /// verified, as one line per instruction in block order.
    fn passes(f: impl FnOnce(&mut TirBuilder) -> Option<crate::ir::Ref>, ty: Ty) -> String {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let tail = f(&mut b);
        let root = b.close_block(blk, tail, ty, NodeIdx::from_raw(1));
        let mut body = b.finish(root, &[]).expect("verifies");
        run(&mut body, &Cx, &mut OwnScratch::default()).expect("passes");
        let errs = verify_owned(&body, &Cx);
        assert!(errs.is_empty(), "{errs:?}\n{}", show(&body));
        show(&body)
    }

    /// Each instruction's tag and its place or value operands, in index
    /// order, then the exit set.
    fn show(b: &Body) -> String {
        let mut out = String::new();
        let mut ops = Vec::new();
        for i in 0..b.len() {
            let tag = b.tags[i];
            let mut line = format!("%{i} {}", tag.name());
            if matches!(
                tag,
                Tag::Read | Tag::Drop | Tag::Store | Tag::Ref | Tag::IsUnique | Tag::Pin
            ) && let Some(p) = b.decode_place(b.data[i][0])
            {
                if tag == Tag::Read
                    && let Some(m) = ReadMode::from_u32(b.data[i][1])
                {
                    line.push_str(&format!(" {m:?}").to_lowercase());
                }
                if tag == Tag::Ref && b.data[i][1] & crate::ir::ref_word::TAKES != 0 {
                    line.push_str(" take");
                }
                if tag == Tag::IsUnique && b.data[i][1] == crate::ir::unique_word::HOISTED {
                    line.push_str(" hoisted");
                }
                if tag == Tag::IsUnique && b.data[i][1] == crate::ir::unique_word::COVERED {
                    line.push_str(" covered");
                }
                let _ = write!(line, " {p}");
                if tag == Tag::Store {
                    let _ = write!(line, " %{}", b.data[i][1]);
                }
            } else if tag == Tag::Block {
                let ms: Vec<String> = b
                    .record(b.data[i][0])
                    .iter()
                    .map(|m| format!("%{m}"))
                    .collect();
                let _ = write!(line, " [{}]", ms.join(" "));
                if b.data[i][1] != NONE {
                    let _ = write!(line, " -> %{}", b.data[i][1]);
                }
            } else {
                super::operands(b, i, &mut ops);
                for op in &ops {
                    let _ = write!(line, " %{}", op.value);
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
        for (s, l) in &b.exit_drops {
            let _ = writeln!(out, "exit {} ${}", s.raw(), l.raw());
        }
        for (at, v) in &b.unwind_drops {
            let _ = writeln!(out, "unwind %{at} %{v}");
        }
        for (at, word) in &b.stable_lists {
            if let Some(p) = b.decode_place(*word) {
                let _ = writeln!(out, "stable %{at} {p}");
            }
        }
        out
    }

    fn local(b: &mut TirBuilder, flags: u8) -> LocalId {
        b.local(Ty::STRING, Symbol::from_raw(0), flags, NodeIdx::from_raw(1))
    }

    /// A call's result that an owned argument takes moves into it; a
    /// result that only a borrowed argument reads is dropped after it.
    #[test]
    fn a_value_is_consumed_by_its_one_owned_use_or_dropped_after_its_last() {
        let text = passes(
            |b| {
                let s = call(b, 7, &[], Ty::STRING);
                call(b, 9, &[s], Ty::VOID);
                let t = call(b, 7, &[], Ty::STRING);
                call(b, 8, &[t], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Call %0\n%2 Call\n%3 Call %2\n%4 Drop [%2]\n%5 Block [%0 %1 %2 %3 %4]\n"
        );
    }

    /// A local read twice: insertion copies both reads and drops the local
    /// after its last access, the second read; move turns that read into a
    /// move and deletes the drop. The parameter likewise. The local, empty
    /// at the exit, leaves the exit set (known-null); the parameter stays,
    /// since the backend reads a parameter outside it as unowned. (A block
    /// lists the blocks its statements run; they are not statements.)
    #[test]
    fn a_local_read_at_its_last_access_moves_and_its_drop_goes() {
        let text = passes(
            |b| {
                let p = local(b, local_flags::PARAM);
                let x = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, NodeIdx::from_raw(1));
                let r1 = b.get(x, Ty::STRING, NodeIdx::from_raw(1));
                call(b, 9, &[r1], Ty::VOID);
                let r2 = b.get(x, Ty::STRING, NodeIdx::from_raw(1));
                let r3 = b.get(p, Ty::STRING, NodeIdx::from_raw(1));
                call(b, 8, &[r2, r3], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Store [$1] %0\n%2 Read copy [$1]\n%3 Call %2\n%4 Read move [$1]\n\
             %5 Read move [$0]\n%6 Call %4 %5\n%7 Drop [%4]\n%8 Drop [%5]\n\
             %9 Block [%0 %1 %2 %3 %4 %5 %6 %7 %8]\nexit 0 $0\n"
        );
    }

    /// A value made before a loop and consumed inside it: the consumer
    /// takes a copy made in the loop, and the value is dropped after the
    /// loop. A value made in the loop and still owned at a `break` is
    /// dropped before it.
    #[test]
    fn a_consumer_in_a_loop_takes_a_copy_and_a_break_drops_what_it_leaves() {
        let text = passes(
            |b| {
                let s = call(b, 7, &[], Ty::STRING);
                let lp = b.open_loop();
                let body = b.open_block();
                call(b, 9, &[s], Ty::VOID);
                let t = call(b, 7, &[], Ty::STRING);
                let n = NodeIdx::from_raw(1);
                b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, n);
                call(b, 8, &[t], Ty::VOID);
                let body = b.close_block(body, None, Ty::VOID, n);
                b.close_loop(lp, body, Ty::VOID, n);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Read copy [%0]\n%2 Call %1\n%3 Call\n%4 Drop [%3]\n%5 Break\n\
             %6 Call %3\n%7 Drop [%3]\n%8 Block [%1 %2 %3 %4 %5 %6 %7]\n%9 Loop\n%10 Drop [%0]\n\
             %11 Block [%0 %8 %9 %10]\n"
        );
    }

    /// Rule 2: a value lent once and then consumed gets a copy for its
    /// consumer and a drop after it; the copy is the value's last use and
    /// its drop follows, so the copy moves and the drop goes.
    #[test]
    fn a_copy_of_a_value_at_its_last_use_moves() {
        let text = passes(
            |b| {
                let s = call(b, 7, &[], Ty::STRING);
                call(b, 8, &[s], Ty::VOID);
                call(b, 9, &[s], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Call %0\n%2 Read move [%0]\n%3 Call %2\n%4 Block [%0 %1 %2 %3]\n"
        );
    }

    /// Liveness around a loop: a local stored before the loop and read in
    /// it is read again on the next round, so its read does not move, and
    /// since it is only lent and nothing in the loop writes the local, it
    /// borrows; a local stored again in every round before its read moves
    /// there.
    #[test]
    fn a_read_in_a_loop_moves_only_when_each_round_stores_first() {
        let text = passes(
            |b| {
                let n = NodeIdx::from_raw(1);
                let kept = local(b, 0);
                let fresh = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(kept, s, n);
                let lp = b.open_loop();
                let body = b.open_block();
                let r = b.get(kept, Ty::STRING, n);
                call(b, 8, &[r], Ty::VOID);
                let t = call(b, 7, &[], Ty::STRING);
                b.set(fresh, t, n);
                let r = b.get(fresh, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let body = b.close_block(body, None, Ty::VOID, n);
                b.close_loop(lp, body, Ty::VOID, n);
                None
            },
            Ty::VOID,
        );
        assert!(text.contains("Read borrow [$0]"), "{text}");
        assert!(text.contains("Read move [$1]"), "{text}");
        assert!(!text.contains("Drop [$1]\n%"), "{text}");
    }

    /// The result of a body moves out as its root block's tail.
    #[test]
    fn a_tail_moves_into_its_block() {
        let text = passes(|b| Some(call(b, 7, &[], Ty::STRING)), Ty::STRING);
        assert_eq!(text, "%0 Call\n%1 Block [%0] -> %0\n");
    }

    /// Rules 5 and 6: an `IsUnique` of the collection before each change
    /// of it (a push, an element store, each list on the path outermost
    /// first), the checker's `ListIndexMut` read as its element place after
    /// an `IsUnique` of the list, and an `IsUnique` and a `Pin` of the list
    /// before an element reference. The element reference addresses the
    /// buffer, not the local, so the local (of a type with no counted
    /// leaf here) takes no cell and no drop.
    #[test]
    fn changes_make_their_collection_unique_and_element_references_pin_it() {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let text = passes(
            |b| {
                let xs = b.local(us, Symbol::from_raw(0), local_flags::PARAM, n);
                let list = call(b, 6, &[], us);
                let i = call(b, 6, &[], us);
                let rec = b.refs_record(&[list, i]);
                let e = b.emit(Tag::Intrinsic, IntrinsicOp::ListIndexMut as u32, rec, us, n);
                let rec = b.refs_record(&[e, i]);
                b.emit(
                    Tag::Intrinsic,
                    IntrinsicOp::ListPush as u32,
                    rec,
                    Ty::VOID,
                    n,
                );
                let word = b.place(
                    &format!("[%{} [%{}] .0 [%{}]]", list.0, i.0, i.0)
                        .parse()
                        .expect("place"),
                );
                b.emit(Tag::Store, word, i.0, Ty::VOID, n);
                let word = b.place(
                    &format!("[${} [%{}]]", xs.raw(), i.0)
                        .parse()
                        .expect("place"),
                );
                b.emit(Tag::Ref, word, 1, us, n);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Call\n%2 IsUnique [%0]\n%3 Read copy [%0 [%1]]\n%4 IsUnique [%3]\n\
             %5 Intrinsic %3 %1\n%6 IsUnique [%0]\n%7 IsUnique [%0 [%1] .0]\n\
             %8 Store [%0 [%1] .0 [%1]] %1\n%9 IsUnique [$0]\n%10 Pin [$0]\n%11 Ref [$0 [%1]]\n\
             %12 Block [%0 %1 %2 %3 %4 %5 %6 %7 %8 %9 %10 %11]\n"
        );
    }

    /// Borrow: a local read that only lends, with no write of the local
    /// before its value's last use, borrows, and its drop goes. A read whose
    /// local a store replaces before its value's last use cannot borrow; as
    /// nothing reads the old value after it, it moves instead. Empty at the
    /// exit after its last read moves, the local leaves the exit set.
    #[test]
    fn a_lent_read_borrows_unless_its_place_is_written_in_its_span() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let r1 = b.get(x, Ty::STRING, n);
                call(b, 8, &[r1], Ty::VOID);
                let r2 = b.get(x, Ty::STRING, n);
                let t = call(b, 7, &[], Ty::STRING);
                b.set(x, t, n);
                call(b, 8, &[r2], Ty::VOID);
                let r3 = b.get(x, Ty::STRING, n);
                call(b, 9, &[r3], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Store [$0] %0\n%2 Read borrow [$0]\n%3 Call %2\n%4 Read move [$0]\n\
             %5 Call\n%6 Store [$0] %5\n%7 Call %4\n%8 Drop [%4]\n%9 Read move [$0]\n%10 Call %9\n\
             %11 Block [%0 %1 %2 %3 %4 %5 %6 %7 %8 %9 %10]\n"
        );
    }

    /// Borrow's binding local: an element read stored into a local that is
    /// stored once and only lent borrows, so the local holds a borrowed
    /// value: its drop goes and it leaves the exit set. A read behind a
    /// reference that a call may write keeps its copy.
    #[test]
    fn a_binding_local_holds_a_borrow_and_a_call_ends_a_heap_borrow() {
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let text = passes(
            |b| {
                let list = b.local(us, Symbol::from_raw(0), 0, n);
                let item = local(b, 0);
                let l = call(b, 6, &[], us);
                b.set(list, l, n);
                let h = b.get(list, us, n);
                let i = call(b, 6, &[], us);
                let word = b.place(&format!("[%{} [%{}]]", h.0, i.0).parse().expect("place"));
                let e = b.emit(Tag::Read, word, ReadMode::Use as u32, Ty::STRING, n);
                b.set(item, e, n);
                let r = b.get(item, Ty::STRING, n);
                call(b, 8, &[r], Ty::VOID);
                let o = call(b, 6, &[], us);
                let word = b.place(&format!("[*%{} .0]", o.0).parse().expect("place"));
                let f = b.emit(Tag::Read, word, ReadMode::Use as u32, Ty::STRING, n);
                call(b, 8, &[f], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert!(text.contains("Read borrow [%2 [%3]]"), "{text}");
        assert!(text.contains("Read borrow [$1]"), "{text}");
        assert!(text.contains("Read copy [*%"), "{text}");
        assert!(!text.contains("Drop [$1]"), "{text}");
        assert!(!text.contains("exit 0 $1"), "{text}");
    }

    /// Pair cancellation: a part read of a temporary that is read again
    /// later copies under move's per-base liveness; with nothing touching
    /// that part before the base's drop, the copy moves. The test facts
    /// lay no type out in place, so the drop stays whole and releases what
    /// the moves left.
    #[test]
    fn a_part_copy_before_the_drop_of_its_base_moves() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let s = call(b, 7, &[], Ty::STRING);
                for k in 0..2 {
                    let word = b.place(&format!("[%{} .{k}]", s.0).parse().expect("place"));
                    let part = b.emit(Tag::Read, word, ReadMode::Use as u32, Ty::STRING, n);
                    call(b, 9, &[part], Ty::VOID);
                }
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Read move [%0 .0]\n%2 Call %1\n%3 Read move [%0 .1]\n%4 Drop [%0]\n\
             %5 Call %3\n%6 Block [%0 %1 %2 %3 %4 %5]\n"
        );
    }

    /// The verifier rejects a borrow whose place a store replaces while the
    /// borrowed value lives.
    #[test]
    fn the_verifier_rejects_a_borrow_across_a_store_of_its_place() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let n = NodeIdx::from_raw(1);
        let x = local(&mut b, 0);
        let s = call(&mut b, 7, &[], Ty::STRING);
        b.set(x, s, n);
        let r = b.emit(
            Tag::Read,
            x.raw() | crate::ir::place::BARE_LOCAL,
            ReadMode::Borrow as u32,
            Ty::STRING,
            n,
        );
        let t = call(&mut b, 7, &[], Ty::STRING);
        b.emit(
            Tag::Store,
            x.raw() | crate::ir::place::BARE_LOCAL,
            t.0,
            Ty::VOID,
            n,
        );
        call(&mut b, 8, &[r], Ty::VOID);
        b.emit(
            Tag::Drop,
            x.raw() | crate::ir::place::BARE_LOCAL,
            NONE,
            Ty::VOID,
            n,
        );
        let root = b.close_block(blk, None, Ty::VOID, n);
        let mut body = b.finish_tolerating(root, &[], &[22]).expect("valid");
        body.exit_drops.push((hd_base::SubId::from_raw(0), x));
        assert!(
            verify_owned(&body, &Cx)
                .iter()
                .any(|e| e.what.contains("lives across `Store`")),
            "{:?}",
            verify_owned(&body, &Cx)
        );
    }

    /// A part taken by a move read lets more parts or a drop of the rest
    /// follow, and nothing else: the verifier accepts a drop of the whole
    /// after it, and rejects a use.
    #[test]
    fn the_verifier_lets_only_the_rest_follow_a_part_move() {
        let build = |use_after: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let s = call(&mut b, 7, &[], Ty::STRING);
            let n = NodeIdx::from_raw(1);
            let word = b.place(&format!("[%{} .0]", s.0).parse().expect("place"));
            let part = b.emit(Tag::Read, word, ReadMode::Move as u32, Ty::STRING, n);
            call(&mut b, 9, &[part], Ty::VOID);
            if use_after {
                call(&mut b, 8, &[s], Ty::VOID);
            }
            let word = b.place(&format!("[%{}]", s.0).parse().expect("place"));
            b.emit(Tag::Drop, word, NONE, Ty::VOID, n);
            let root = b.close_block(blk, None, Ty::VOID, n);
            b.finish_tolerating(root, &[], &[22]).expect("valid")
        };
        assert!(verify_owned(&build(false), &Cx).is_empty());
        assert!(
            verify_owned(&build(true), &Cx)
                .iter()
                .any(|e| e.what.contains("after its consumer or its drop"))
        );
    }

    /// The verifier catches a value never dropped, one dropped twice and
    /// one read after its drop.
    #[test]
    fn the_verifier_rejects_leaks_double_drops_and_uses_after_a_drop() {
        let build = |f: &dyn Fn(&mut TirBuilder, crate::ir::Ref)| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let s = call(&mut b, 7, &[], Ty::STRING);
            f(&mut b, s);
            let root = b.close_block(blk, None, Ty::VOID, NodeIdx::from_raw(1));
            b.finish_tolerating(root, &[], &[22]).expect("valid")
        };
        let drop = |b: &mut TirBuilder, v: crate::ir::Ref| {
            let word = b.place(&format!("[%{}]", v.0).parse().expect("place"));
            b.emit(Tag::Drop, word, NONE, Ty::VOID, NodeIdx::from_raw(1));
        };
        let leak = build(&|_, _| {});
        assert!(
            verify_owned(&leak, &Cx)
                .iter()
                .any(|e| e.what.contains("still owned"))
        );
        let twice = build(&|b, s| {
            drop(b, s);
            drop(b, s);
        });
        assert!(
            verify_owned(&twice, &Cx)
                .iter()
                .any(|e| e.what.contains("twice"))
        );
        let after = build(&|b, s| {
            drop(b, s);
            call(b, 8, &[s], Ty::VOID);
        });
        assert!(
            verify_owned(&after, &Cx)
                .iter()
                .any(|e| e.what.contains("after its consumer or its drop"))
        );
    }

    /// Known-null: a local moved on every branch of an `If` is empty after
    /// it, so the drop move left there goes; a local moved on one branch
    /// keeps it. Both are empty at the exit, so neither has an exit row.
    #[test]
    fn a_drop_after_branches_that_all_move_goes() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let y = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let t = call(b, 7, &[], Ty::STRING);
                b.set(y, t, n);
                let c = call(b, 6, &[], Ty::BOOL);
                let then = b.open_block();
                let r = b.get(x, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let r = b.get(y, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let then = b.close_block(then, None, Ty::VOID, n);
                let els = b.open_block();
                let r = b.get(x, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let els = b.close_block(els, None, Ty::VOID, n);
                let arms = b.refs_record(&[then, els]);
                b.emit(Tag::If, c.0, arms, Ty::VOID, n);
                None
            },
            Ty::VOID,
        );
        assert_eq!(
            text,
            "%0 Call\n%1 Store [$0] %0\n%2 Call\n%3 Store [$1] %2\n%4 Call\n%5 Read move [$0]\n\
             %6 Call %5\n%7 Read move [$1]\n%8 Call %7\n%9 Block [%5 %6 %7 %8]\n\
             %10 Read move [$0]\n%11 Call %10\n%12 Block [%10 %11]\n%13 If %4\n%14 Drop [$1]\n\
             %15 Block [%0 %1 %2 %3 %4 %9 %12 %13 %14]\n"
        );
    }

    /// Known-null in a loop: a local stored and moved on each round is
    /// empty at the loop's head and after it, so the drop after the loop
    /// and its exit row go.
    #[test]
    fn a_drop_after_a_loop_that_moves_each_round_goes() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let lp = b.open_loop();
                let body = b.open_block();
                let r = b.get(x, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let c = call(b, 6, &[], Ty::BOOL);
                let then = b.open_block();
                b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, n);
                let then = b.close_block(then, None, Ty::NEVER, n);
                let arms = b.refs_record(&[then]);
                b.emit(Tag::If, c.0, arms, Ty::VOID, n);
                let t = call(b, 7, &[], Ty::STRING);
                b.set(x, t, n);
                let body = b.close_block(body, None, Ty::VOID, n);
                b.close_loop(lp, body, Ty::VOID, n);
                None
            },
            Ty::VOID,
        );
        assert!(!text.contains("Drop [$0]"), "{text}");
        assert!(!text.contains("exit 0 $0"), "{text}");
    }

    /// Known-null at a suspension point: its unwind set lists the values
    /// that may hold a count there, a lent argument and a value consumed
    /// after it, and a local that may hold a value there keeps its exit
    /// row although it is empty at the exit itself; a local moved before
    /// it leaves the exit set.
    #[test]
    fn a_suspension_point_unwinds_what_it_holds() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let y = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let r = b.get(x, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(y, s, n);
                let a = call(b, 7, &[], Ty::STRING);
                let t = call(b, 7, &[], Ty::STRING);
                let aw = call(b, 8, &[a], Ty::VOID);
                b.body_mut().tags[aw.0 as usize] = Tag::Await;
                call(b, 9, &[t], Ty::VOID);
                let r = b.get(y, Ty::STRING, n);
                call(b, 9, &[r], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert!(text.contains("exit 0 $1\n"), "{text}");
        assert!(!text.contains("exit 0 $0"), "{text}");
        assert!(text.contains("unwind %8 %6\nunwind %8 %7\n"), "{text}");
    }

    /// Move's rule 4: a `Ref` of a celled local at its last access takes
    /// the local's count, but not in a sub-body that holds a suspension
    /// point, whose frame saves the cell's local at each point: there the
    /// reference counts and the local keeps its drop.
    #[test]
    fn a_reference_takes_its_cell_only_outside_a_suspending_body() {
        let n = NodeIdx::from_raw(1);
        let body = |suspend: bool| {
            passes(
                move |b| {
                    let x = local(b, 0);
                    let s = call(b, 7, &[], Ty::STRING);
                    b.set(x, s, n);
                    let word = b.place(&format!("[${}]", x.raw()).parse().expect("place"));
                    let r = b.emit(Tag::Ref, word, 0, Ty::STRING, n);
                    let c = call(b, 8, &[r], Ty::VOID);
                    if suspend {
                        b.body_mut().tags[c.0 as usize] = Tag::Await;
                    }
                    None
                },
                Ty::VOID,
            )
        };
        let plain = body(false);
        let suspending = body(true);
        assert!(plain.contains("Ref take [$0]"), "{plain}");
        assert!(!plain.contains("Drop [$0]"), "{plain}");
        assert!(!suspending.contains("take"), "{suspending}");
        assert!(suspending.contains("Drop [$0]"), "{suspending}");
        // The verifier refuses a take that a suspension point follows.
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let x = local(&mut b, 0);
        let s = call(&mut b, 7, &[], Ty::STRING);
        b.set(x, s, n);
        let word = b.place(&format!("[${}]", x.raw()).parse().expect("place"));
        let r = b.emit(Tag::Ref, word, 0, Ty::STRING, n);
        call(&mut b, 8, &[r], Ty::VOID);
        let root = b.close_block(blk, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        run(&mut body, &Cx, &mut OwnScratch::default()).expect("passes");
        assert!(verify_owned(&body, &Cx).is_empty());
        let at = (0..body.len())
            .rev()
            .find(|&i| body.tags[i] == Tag::Call)
            .expect("call");
        body.tags[at] = Tag::Await;
        let errs = verify_owned(&body, &Cx);
        assert!(
            errs.iter().any(|e| e.what.contains("takes a cell's count")),
            "{errs:?}"
        );
    }

    /// A loop whose iteration block changes a list (`ListPush` over a
    /// string-typed stand-in), behind an entry test of a constant:
    /// `header` builds the header value the change reads, `extra` adds
    /// statements after it, `pure_prefix` says whether the entry test's
    /// prefix is pure.
    fn loop_body(
        b: &mut TirBuilder,
        header: &dyn Fn(&mut TirBuilder) -> crate::ir::Ref,
        extra: &dyn Fn(&mut TirBuilder),
        pure_prefix: bool,
    ) {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let lp = b.open_loop();
        let body = b.open_block();
        if !pure_prefix {
            call(b, 6, &[], us);
        }
        let yes = b.const_value(Ty::BOOL, 1);
        let eb = b.open_block();
        let h = header(b);
        let k = b.const_value(us, 1);
        let rec = b.refs_record(&[h, k]);
        b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            rec,
            Ty::VOID,
            n,
        );
        extra(b);
        let els = b.close_block(eb, None, Ty::VOID, n);
        let tb = b.open_block();
        b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, n);
        let then = b.close_block(tb, None, Ty::VOID, n);
        let arms = b.refs_record(&[then, els]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, n);
        let body = b.close_block(body, None, Ty::VOID, n);
        b.close_loop(lp, body, Ty::VOID, n);
    }

    /// Hoisting: the `IsUnique` a push in the iteration block needs moves
    /// before the loop, marked hoisted, for a fresh local list, even with a
    /// call in the loop (rule 3's private local). Not when the entry test's
    /// prefix calls something, and not for a parameter's list with a call
    /// in the loop.
    #[test]
    fn a_change_in_a_loop_makes_its_list_unique_before_the_loop() {
        let n = NodeIdx::from_raw(1);
        let run_with = |param: bool, pure_prefix: bool, calls: bool| {
            passes(
                move |b| {
                    let xs = local(b, if param { local_flags::PARAM } else { 0 });
                    if !param {
                        let s = call(b, 7, &[], Ty::STRING);
                        b.set(xs, s, n);
                    }
                    loop_body(
                        b,
                        &|b| b.get(xs, Ty::STRING, n),
                        &|b| {
                            if calls {
                                call(b, 6, &[], Ty::prim(hd_types::Prim::Usize));
                            }
                        },
                        pure_prefix,
                    );
                    None
                },
                Ty::VOID,
            )
        };
        let local_list = run_with(false, true, true);
        assert!(local_list.contains("IsUnique hoisted [$0]"), "{local_list}");
        assert_eq!(local_list.matches("IsUnique").count(), 1, "{local_list}");
        let hoisted = local_list
            .lines()
            .position(|l| l.contains("IsUnique hoisted"))
            .expect("hoisted");
        let lp = local_list
            .lines()
            .position(|l| l.contains(" Loop"))
            .expect("loop");
        assert!(hoisted < lp, "{local_list}");
        let impure = run_with(false, false, false);
        assert!(!impure.contains("hoisted"), "{impure}");
        let param_call = run_with(true, true, true);
        assert!(!param_call.contains("hoisted"), "{param_call}");
        let param = run_with(true, true, false);
        assert!(param.contains("IsUnique hoisted [$0]"), "{param}");
    }

    /// A change through a reference parameter's list hoists as the
    /// referent's place, and the `IsUnique` in the loop stays, covered, for
    /// the instances where a view's getter makes the header fresh. A loop
    /// that only reads elements keeps its list stable.
    #[test]
    fn a_list_behind_a_reference_hoists_covered_and_a_read_loop_keeps_its_list() {
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let text = passes(
            |b| {
                let r = b.local(us, Symbol::from_raw(0), local_flags::PARAM, n);
                loop_body(
                    b,
                    &|b| {
                        let rv = b.get(r, us, n);
                        let word = b.place(&format!("[*%{}]", rv.0).parse().expect("place"));
                        b.emit(Tag::Read, word, ReadMode::Use as u32, Ty::STRING, n)
                    },
                    &|_| {},
                    true,
                );
                None
            },
            Ty::VOID,
        );
        assert!(text.contains("IsUnique hoisted [*$0]"), "{text}");
        assert!(text.contains("IsUnique covered"), "{text}");
        let text = passes(
            |b| {
                let xs = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(xs, s, n);
                let lp = b.open_loop();
                let body = b.open_block();
                let h = b.get(xs, Ty::STRING, n);
                let i = call(b, 6, &[], us);
                let word = b.place(&format!("[%{} [%{}]]", h.0, i.0).parse().expect("place"));
                b.emit(Tag::Read, word, ReadMode::Use as u32, us, n);
                let body = b.close_block(body, None, Ty::VOID, n);
                b.close_loop(lp, body, Ty::VOID, n);
                None
            },
            Ty::VOID,
        );
        assert!(
            text.contains("stable %") && text.contains(" [$0]"),
            "{text}"
        );
    }
}
