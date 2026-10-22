//! The ownership passes (`docs/compiler/tir-v2.md`, "The ownership
//! passes"): a fixed pipeline over one body that decides every count once
//! per generic body, after decoding and before any instance is emitted.
//! The checker's TIR reads every place with mode `use` and has no
//! ownership statement (invariant 22); the passes leave every read's mode
//! decided, the `Drop`s each owned value and local needs, and the exit
//! set (invariant 23, [`crate::ir::verify_owned`]).
//!
//! Built so far: **insertion** (`insert`), after the field reads of a
//! whole local's read are folded into reads of the local's field places
//! (`fold`), the naive, correct ownership
//! of `crates/hd_wasm_linear/DESIGN.md` §23.16 with releases at last use
//! (§23.20), made explicit. Borrow, move, pair cancellation, known-null
//! and hoisting are later commits of stage 2; each only weakens or deletes
//! what insertion writes, so the pipeline is correct after any prefix.
//!
//! The passes read the body and [`OwnCx`], signature and layout facts the
//! driver gives; never another body. They follow "Data layout of the
//! passes": dense tables indexed by instruction and local in one
//! [`OwnScratch`] reused across bodies, no hash maps, and every edit
//! recorded in a side table and spliced in once ([`splice`]).

use hd_types::Ty;

use crate::ir::{Body, Callee, NONE, Op, Ref, Tag, place};

mod fold;
mod insert;
mod splice;

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
    /// constructor counts what it keeps), or a parameter whose mode
    /// depends on the instance: the caller keeps its value.
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
    /// false only for the few std functions known pure. Insertion does not
    /// read it; the borrow and hoisting passes will.
    fn may_write(&self, callee: &Callee) -> bool;
}

/// How an instruction uses one of its value operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The use reads the value and keeps nothing: an operand of `Prim`, a
    /// place's root or element index, a borrowed argument.
    Lend,
    /// The use takes the value's count: a stored value, a construction
    /// part, a `Return` or `Break` value.
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
                        values(b, word, &mut |v, at| push(out, v, Role::Lend, at));
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
/// a statement has none; a switch has one outside a decision tree.
pub(crate) fn tracked(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    let t = b.ty[i];
    if t == Ty::VOID || t == Ty::NEVER || t == Ty::POISON {
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
    pub(crate) fold: fold::Tables,
    pub(crate) insert: insert::Tables,
    pub(crate) splice: splice::Tables,
}

/// Runs the ownership passes over every sub-body of `b`, in order: place
/// folding ([`fold`]), then insertion, the only pass so far. A body the
/// passes cannot handle is refused, with what stopped them.
pub fn run(b: &mut Body, cx: &dyn OwnCx, scratch: &mut OwnScratch) -> Result<(), String> {
    fold::run(b, cx, &mut scratch.fold);
    insert::run(b, cx, scratch)
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
            if matches!(tag, Tag::Read | Tag::Drop | Tag::Store | Tag::Ref)
                && let Some(p) = b.decode_place(b.data[i][0])
            {
                if tag == Tag::Read
                    && let Some(m) = ReadMode::from_u32(b.data[i][1])
                {
                    line.push_str(&format!(" {m:?}").to_lowercase());
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

    /// A local read twice: both reads copy, and the local is dropped after
    /// its last access, the read itself, and listed in the exit set; the
    /// parameter likewise. (A block lists the blocks its statements run;
    /// they are not statements.)
    #[test]
    fn locals_are_dropped_after_their_last_access_and_listed_at_the_exit() {
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
            "%0 Call\n%1 Store [$1] %0\n%2 Read copy [$1]\n%3 Call %2\n%4 Read copy [$1]\n\
             %5 Drop [$1]\n%6 Read copy [$0]\n%7 Drop [$0]\n%8 Call %4 %6\n%9 Drop [%4]\n\
             %10 Drop [%6]\n%11 Block [%0 %1 %2 %3 %4 %5 %6 %7 %8 %9 %10]\nexit 0 $0\nexit 0 $1\n"
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

    /// The result of a body moves out as its root block's tail.
    #[test]
    fn a_tail_moves_into_its_block() {
        let text = passes(|b| Some(call(b, 7, &[], Ty::STRING)), Ty::STRING);
        assert_eq!(text, "%0 Call\n%1 Block [%0] -> %0\n");
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
}
