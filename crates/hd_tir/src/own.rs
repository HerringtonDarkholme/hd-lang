//! The ownership passes (`docs/compiler/tir-v2.md`, "The ownership
//! passes"): a fixed pipeline over one body that decides every count once
//! per generic body, after decoding and before any instance is emitted.
//! The checker's TIR reads every place with mode `use` and has no
//! ownership statement (invariant 22); the passes leave every read's mode
//! decided, the `Drop`s each owned value and local needs, and the exit
//! set (invariant 23, [`crate::ir::verify_owned`]).
//!
//! Built so far: **late operand reads** (`late`), which moves an operand
//! read of a local's place to just before its consumer when a later
//! operand reads the local, so the read is its last access; then
//! **insertion** (`insert`), over places the checker roots at their local
//! on this path (`crate::ir::root_places`), the naive,
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

use crate::ir::{
    Body, Callee, CaptureMode, IntrinsicOp, NONE, ReadMode, Ref, Root, Tag, Topology, Word, inside,
    slots, u32_of,
};

pub use hoist::{EntryTest, entry_test, loop_statements, loop_test};

pub(crate) mod borrow;
pub(crate) use borrow::keeps_drops;
pub mod flow;
mod frame;
pub(crate) mod hoist;
mod insert;
mod known;
mod late;
mod moves;
mod pairs;
pub(crate) mod parts;
pub mod promote;
mod reason;
pub(crate) mod splice;

pub use reason::{Reason, reason};

/// Whether a value of a type may be copied ([`OwnCx::copies`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    No,
    Yes,
    /// It holds a type parameter declared `?Copy`, which an instance
    /// decides: only a generic item's own body holds one.
    Maybe,
}

/// How a call passes one argument, by the callee's signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// A value parameter: the callee owns the argument's count, so the
    /// call consumes it. A type parameter's (or an associated type's)
    /// value parameter is one too: an instance that makes it a value owns
    /// the argument, and one that makes it a reference borrows it, after
    /// which the backend releases what the call consumed.
    Owned,
    /// A reference parameter, a suspending callee's argument (its cold
    /// constructor counts what it keeps), or an `@intrinsic` callee's
    /// argument (the backend lowers the call in place and counts what it
    /// keeps), unless the intrinsic takes over its value argument, as
    /// `string_from_bytes` does: the caller keeps its value.
    Lent,
}

/// How a compiler-supplied call relates to raw memory. The release write
/// summary uses this to distinguish a write through an address the body made
/// from one through an address its caller may reach. Ordinary hd functions
/// have no such effect: their walked bodies supply their summaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallMemory {
    /// No raw-memory fact beyond [`OwnCx::may_write`].
    None,
    /// The call returns a fresh allocation the caller could not reach before
    /// this body made it.
    Allocate,
    /// The call's result derives its address from argument `k`.
    Derive(u8),
    /// The call writes through argument `k`.
    Write(u8),
}

/// The facts the passes read besides the body (`docs/compiler/tir-v2.md`,
/// "What the passes read"): layout and signature facts, never another
/// body's.
pub trait OwnCx {
    /// Whether a value of `ty` holds a counted leaf. The passes run over
    /// instances, whose types are concrete, and over a generic item's own
    /// body only where an erased body and its thunks still run it: there a
    /// type parameter's value is one counted address
    /// (`crates/hd_wasm_linear/src/emit/erased.rs`), so it counts.
    fn counted(&self, ty: Ty) -> bool;
    /// Whether `ty` names a type parameter an erased body leaves open,
    /// whose values its thunks make fresh: a read of one keeps its `Drop`s
    /// where it borrows, and only a type that does not is counted in every
    /// instance. The default answers no.
    fn thunk_made(&self, ty: Ty) -> bool {
        let _ = ty;
        false
    }
    /// How argument `k` of `n` of a `Call` or `Await` of `callee` is
    /// passed. `Lent` whenever it is not `Owned`: a lent argument the
    /// callee's instance owns takes a count at the call, and an owned one
    /// an instance borrows is released after it (`layout::Pass` in the
    /// backend).
    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass;
    /// [`OwnCx::pass`] for a `CallValue` of a value of function type
    /// `fn_ty`.
    fn pass_value(&self, fn_ty: Ty, k: usize, n: usize) -> Pass;
    /// Whether a call of `callee` may reach memory outside its arguments:
    /// false only for the few std functions known pure. The borrow pass
    /// and hoisting read it.
    fn may_write(&self, callee: &Callee) -> bool;
    /// The raw-memory relation of a compiler-supplied call. The default is
    /// none so a target that supplies no such calls stays conservative.
    fn call_memory(&self, callee: &Callee) -> CallMemory {
        let _ = callee;
        CallMemory::None
    }
    /// Whether parameter `k` of `n` of `item` is declared `&T` or `&mut T`,
    /// no counted reference: the reads behind it that exclusive mode may
    /// borrow across a call ([`crate::ir::sub_flags::EXCLUSIVE`]).
    fn local_ref_param(&self, item: hd_base::DefId, k: usize, n: usize) -> bool {
        let _ = (item, k, n);
        false
    }
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
    /// does not suspend); `Owned` for a value parameter (a type
    /// parameter's too), for every parameter of a suspending function,
    /// whose frame holds a count of each, and for a body that is no
    /// function's. The borrow pass leaves a lent parameter the body never
    /// stores unowned, and an owned one too when no read of it keeps its
    /// value and the body does not suspend ([`OwnCx::suspending`]).
    fn received(&self, item: hd_base::DefId, k: usize, n: usize) -> Pass;
    /// Whether the body of `item` suspends, so that its frame holds a
    /// count of every parameter. The default answers yes, which leaves
    /// every owned parameter owned.
    fn suspending(&self, item: hd_base::DefId) -> bool {
        let _ = item;
        true
    }
    /// Whether a value of `ty` may hold a list or a map in place, which a
    /// count of it copies: a list, a map, a struct holding one in a field
    /// at any depth, or a type the instance decides. Hoisting reads it.
    fn holds_coll(&self, ty: Ty) -> bool;
    /// Whether a value of `ty` may hold a local reference: a reference of
    /// either kind (a `&self` receiver's address has the counted
    /// carrier), a trait value, or a type built of one. A type parameter
    /// never points into a local of its body (`ref.generic.body`). Frame locals' origins follow such
    /// values (`frame`). The default answers yes.
    fn holds_ref(&self, ty: Ty) -> bool {
        let _ = ty;
        true
    }
    /// Whether a same-value coercion from `from` to `to` lends a counted
    /// reference as a local one (`ref.convert.lend`): the result points
    /// where the counted one does and holds none of its count, so the
    /// coercion consumes nothing, and what it lends from lives until the
    /// result's last use (`frame`, rule 5). The default answers no.
    fn lends(&self, from: Ty, to: Ty) -> bool {
        let _ = (from, to);
        false
    }
    /// Whether every reference a value of `ty` may hold is counted (a
    /// `*T`), and it holds one: a copy of it keeps its own count of what
    /// it points into, and the borrow pass lends one only from a place
    /// that outlives the loan. Roots (`frame`, rule 5) follow no such
    /// value. The default answers no.
    fn counted_refs_only(&self, ty: Ty) -> bool {
        let _ = ty;
        false
    }
    /// Whether a value of `ty` may be copied (`ref.copy.structure`): `No`
    /// when it holds a type declared `!Copy` by value, `Maybe` when it
    /// holds a type parameter declared `?Copy`, which an instance decides.
    /// A `&mut` counts as copied here: a copy of one is a reborrow, which
    /// takes no count. The copy verifier ([`uncopied`]) reads it. The
    /// default answers yes.
    fn copies(&self, ty: Ty) -> Tri {
        let _ = ty;
        Tri::Yes
    }
    /// Whether the optimized pipeline's release summaries read the body's
    /// result (`docs/future-work/RELEASE_INTERPROC.md`): an owned parameter
    /// whose only kept reads go to binding locals is then left unowned too,
    /// so that callers may lend it.
    fn summarizes(&self) -> bool {
        false
    }
}

pub use crate::ir::{Loc, Role};

/// One value operand of an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operand {
    pub value: u32,
    pub role: Role,
    pub at: Loc,
}

/// The value operands instruction `i` reads, in emission order, each with
/// its role and position: its value and provider [`slots`], which are the
/// operands of `Body::value_operands`, a call's providers, `With`'s and
/// `ContextNew`'s provider values, a block's tail, and the value operands
/// of control flow (a condition, a scrutinee, a `Return` or `Break`
/// value). Constants and `NONE` are left out. One definition serves the
/// passes, their verifier and the emitter, so all three agree on what a
/// use is.
pub fn operands(b: &Body, i: usize, out: &mut Vec<Operand>) {
    out.clear();
    slots(b, i, |at, value, slot| {
        let role = match slot {
            Word::Value(role) => role,
            Word::Provider => Role::Lend,
            Word::Block | Word::Inst => return,
        };
        if Ref(value).as_inst().is_some() && (value as usize) < b.len() {
            out.push(Operand { value, role, at });
        }
    });
}

/// Marks what the cleanup suites `roots` touch, as [`inside`] walks
/// them: each place's root local and each captured local with `local`,
/// each operand with `value`, and with `own` each instruction's own value
/// too. A suite runs at its scope's exits, which the passes' walks do not
/// follow.
pub(crate) fn suite_touches(
    b: &Body,
    roots: &[u32],
    stack: &mut Vec<u32>,
    ops: &mut Vec<Operand>,
    own: bool,
    mut local: impl FnMut(u32),
    mut value: impl FnMut(u32),
) {
    inside(b, roots, stack, |i| {
        match b.tags[i] {
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique => {
                if let Some(l) = b.view(b.data[i][0]).local() {
                    local(l);
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                (rec[0]..rec[0] + rec[1]).for_each(|c| local(b.cap_local[c as usize].raw()));
            }
            _ => {}
        }
        operands(b, i, ops);
        for op in ops.iter() {
            value(op.value);
        }
        if own {
            value(u32_of(i));
        }
        true
    });
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

/// The collection operands instruction `i` changes in place, as (`extra`
/// position, value): a push's or reserve's list, a map store's or
/// removal's map.
pub(crate) fn changed(b: &Body, i: usize) -> impl Iterator<Item = (u32, u32)> + '_ {
    use IntrinsicOp::{ListPush, ListReserve, MapRemove, MapSet, MapSlotEntryMut};
    let [a, w] = b.data[i];
    let direct = b.tags[i] == Tag::Intrinsic
        && matches!(
            IntrinsicOp::from_u32(a),
            Some(ListPush | ListReserve | MapSet | MapRemove | MapSlotEntryMut)
        );
    direct.then(|| (w + 1, b.record(w)[0])).into_iter()
}

/// Clears `v` and refills it with `n` copies of `x`, as a pass resets one
/// of its tables.
pub(crate) fn fill<T: Clone>(v: &mut Vec<T>, n: usize, x: T) {
    v.clear();
    v.resize(n, x);
}

/// Whether instruction `i`'s value is one the passes track: an owned
/// value that may hold a counted leaf. A block's value is its owner's, and
/// a statement has none; a switch has one outside a decision tree. A
/// `borrow` read owns no count.
pub(crate) fn tracked(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    owned_value(b, i) && cx.counted(b.ty[i])
}

/// An owned value, including one with no runtime count or drop.
pub(crate) fn owned_value(b: &Body, i: usize) -> bool {
    let t = b.ty[i];
    if t == Ty::VOID || t == Ty::NEVER || t == Ty::POISON {
        return false;
    }
    if b.is_borrow(u32_of(i)) || b.is_borrowed_coercion(u32_of(i)) {
        return false;
    }
    if matches!(
        b.tags[i],
        Tag::Block
            | Tag::Store
            | Tag::Drop
            | Tag::IsUnique
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
    true
}

/// Whether operand `op` of instruction `i` consumes its value
/// (`docs/compiler/tir-v2.md`, "Where values are owned"). For a block's
/// tail, `owner` is the block's owner, `NONE` for a sub-body's root.
/// Whether instruction `i` is a coercion that lends a counted reference
/// as a local one ([`OwnCx::lends`]).
pub(crate) fn lending_coercion(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    b.same_value_coercion(i)
        && Ref(b.data[i][0]).as_inst().is_some()
        && cx.lends(b.ty[b.data[i][0] as usize], b.ty[i])
}

/// Whether `Read` `i` reads all of a local holding a counted reference
/// as a local one (`ref.convert.lend`, a read the checker typed with the
/// local reference's carrier): it lends the local, which never moves
/// there and lives until the read value's last use (`frame`, rule 5).
pub(crate) fn lending_read(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    if b.tags[i] != Tag::Read {
        return false;
    }
    let p = b.view(b.data[i][0]);
    p.whole()
        && matches!(p.root, crate::ir::Root::Local(l)
            if cx.lends(b.local_ty[l as usize], b.ty[i]))
}

/// The values instruction `i` lends as local references that its own
/// value may point through, into `out`: a lending coercion's operand
/// ([`lending_coercion`]), and each counted reference a call lends whose
/// result holds a local reference (`ref.dep.call`): the result may point
/// into what that reference refers to. What each one refers to must live
/// until the last use of `i`'s value.
pub(crate) fn lent_values(b: &Body, cx: &dyn OwnCx, i: usize, out: &mut Vec<u32>) {
    out.clear();
    if lending_coercion(b, cx, i) {
        out.push(b.data[i][0]);
        return;
    }
    if !matches!(b.tags[i], Tag::Call | Tag::CallValue | Tag::CallDyn)
        || !cx.holds_ref(b.ty[i])
        || cx.counted_refs_only(b.ty[i])
    {
        return;
    }
    let mut ops = Vec::new();
    operands(b, i, &mut ops);
    for op in &ops {
        let v = op.value;
        if Ref(v).as_inst().is_some()
            && matches!(op.role, Role::Arg(_))
            && cx.counted_refs_only(b.ty[v as usize])
            && !consumes(b, cx, i, op, NONE)
        {
            out.push(v);
        }
    }
}

pub(crate) fn consumes(b: &Body, cx: &dyn OwnCx, i: usize, op: &Operand, owner: u32) -> bool {
    match op.role {
        Role::Consume => !lending_coercion(b, cx, i),
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
pub fn consumers(b: &Body, topo: &Topology, cx: &dyn OwnCx, out: &mut Vec<u32>) {
    let n = b.len();
    let Topology {
        blk,
        owner,
        is_root,
        ..
    } = topo;
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
        if count[v] != 1 || blk[v] == NONE || b.is_borrow(u32_of(v)) || !tracked(b, cx, v) {
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
    /// The block structure of the body the passes run over: before
    /// insertion's edits, then after them for the passes that follow.
    pub(crate) topo: Topology,
    pub(crate) borrow: borrow::Tables,
    pub(crate) frame: frame::Tables,
    pub(crate) hoist: hoist::Tables,
    pub(crate) insert: insert::Tables,
    pub(crate) known: known::Tables,
    pub(crate) late: late::Tables,
    pub(crate) moves: moves::Tables,
    pub(crate) pairs: pairs::Tables,
    pub(crate) splice: splice::Tables,
    /// The copy verifier's findings in the body the passes ran over last.
    pub(crate) uncopied: Vec<Uncopied>,
}

impl OwnScratch {
    /// Bit k: exclusive mode borrowed a read through reference parameter k
    /// across a call that may write, in the body the passes ran over last;
    /// in release, for a plain body, that it would have (a probe that
    /// changes no decision).
    #[must_use]
    pub fn relies(&self) -> u64 {
        self.borrow.relies
    }

    /// The copies of a non-copyable value the copy verifier found in the
    /// body the passes ran over last ([`uncopied`]).
    #[must_use]
    pub fn uncopied(&self) -> &[Uncopied] {
        &self.uncopied
    }
}

/// Whether a copy of a non-copyable value fails the build (#1295,
/// `docs/future-work/RAW_BUFFER.md`): an internal error in every build.
/// False only to survey the passes, when the driver collects each finding
/// without failing.
pub const UNCOPIED_FAILS: bool = true;

/// A copy of a value whose type is not `Copy`, after the passes: a `Read`
/// with mode `copy`, or a capture with mode `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uncopied {
    /// The `Read`, or for a capture the `Closure` that makes it.
    pub inst: u32,
    /// For a capture, its row in `Body::cap_local`.
    pub capture: Option<u32>,
    /// The copied value's type.
    pub ty: Ty,
    /// `Maybe`: the type holds a `?Copy` type parameter, so only some
    /// instances copy a non-copyable value; `No`: every instance does.
    pub copies: Tri,
}

/// The copy verifier (`docs/future-work/RAW_BUFFER.md`, "The verifier"):
/// after the passes, no `Read` of a non-copyable type has mode `copy`
/// and no closure captures one by copy, since a copy is a retain of what
/// the value owns. Every finding goes to `out`, in instruction order. One
/// pass over the body's instructions and captures.
pub fn uncopied(b: &Body, cx: &dyn OwnCx, out: &mut Vec<Uncopied>) {
    out.clear();
    let mut last = (Ty::VOID, Tri::Yes);
    let mut copies = |t: Ty| {
        if last.0 != t {
            last = (t, cx.copies(t));
        }
        last.1
    };
    for i in 0..b.len() {
        match b.tags[i] {
            Tag::Read if b.data[i][1] == ReadMode::Copy as u32 => {
                let c = copies(b.ty[i]);
                if c != Tri::Yes {
                    out.push(Uncopied {
                        inst: u32_of(i),
                        capture: None,
                        ty: b.ty[i],
                        copies: c,
                    });
                }
            }
            Tag::Closure => {
                let rec = b.record(b.data[i][1]);
                let (start, len) = (rec[0], rec[1]);
                for k in start..start + len {
                    let mode = b.cap_mode.get(k as usize);
                    let local = b.cap_local.get(k as usize);
                    let (Some(CaptureMode::Copy), Some(l)) = (mode, local) else {
                        continue;
                    };
                    let t = b.local_ty[l.idx()];
                    let c = copies(t);
                    if c != Tri::Yes {
                        out.push(Uncopied {
                            inst: u32_of(i),
                            capture: Some(k),
                            ty: t,
                            copies: c,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Runs the ownership passes over every sub-body of `b`, in order: late
/// operand reads, insertion, borrow, move, pair cancellation, known-null,
/// then hoisting. The last five record their deletions and insertions in
/// one set of edits, applied once at the end. Then the copy verifier
/// ([`uncopied`]) leaves its findings in `scratch`, for the driver to
/// report or refuse ([`UNCOPIED_FAILS`]). A body the passes cannot
/// handle is refused, with what stopped them.
pub fn run(b: &mut Body, cx: &dyn OwnCx, scratch: &mut OwnScratch) -> Result<(), String> {
    late::run(
        b,
        &mut scratch.topo,
        cx,
        &mut scratch.late,
        &mut scratch.splice,
    )?;
    insert::run(b, cx, scratch)?;
    scratch.topo.build(b);
    let (topo, edits) = (&scratch.topo, &mut scratch.splice);
    borrow::run(b, topo, cx, &mut scratch.borrow, edits);
    moves::run(b, topo, cx, &mut scratch.moves, edits);
    pairs::write_backs(b, topo, cx, &scratch.moves, &mut scratch.pairs, edits);
    pairs::run(b, topo, cx, &mut scratch.moves, &mut scratch.pairs, edits);
    known::run(b, topo, cx, &mut scratch.known, edits);
    hoist::run(b, topo, cx, &mut scratch.hoist, edits);
    splice::apply(b, topo, edits)?;
    uncopied(b, cx, &mut scratch.uncopied);
    Ok(())
}

/// What the optimized pipeline reads of one body
/// (`docs/future-work/RELEASE_INTERPROC.md`, "Data structures"): facts of
/// its code, never of its signature, which type checking never reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Summary {
    /// Bit k: the body may keep parameter k (receiver first) past the
    /// call. Parameters past 63 are always kept; a suspending body has no
    /// summary.
    pub kept: u64,
    /// Bit k: value parameter k may hold a count and the body keeps none of it
    /// (`kept` clear), so a caller may pass it borrowed, with no count on
    /// either side (#1289). Parameters past 63 never borrow.
    pub borrows: u64,
    /// The parameter every result of the body is, unchanged: a whole read
    /// of a parameter the body never stores.
    pub returns: Option<u8>,
    /// Whether the body may write memory its caller can reach
    /// ([`writes`]).
    pub writes: bool,
}

impl Summary {
    /// The facts that hold of both `self` and `other`.
    #[must_use]
    pub fn meet(self, other: Self) -> Self {
        Self {
            kept: self.kept | other.kept,
            borrows: self.borrows & other.borrows,
            returns: self.returns.filter(|&k| other.returns == Some(k)),
            writes: self.writes || other.writes,
        }
    }
}

/// The locals of `b`'s own parameters, the receiver first; a closure's are
/// its sub-body's.
#[must_use]
pub fn params(b: &Body) -> Vec<u32> {
    let mut inner = vec![false; b.local_ty.len()];
    for r in b.sub_params.iter().skip(1) {
        for k in 0..r.len {
            inner[b.extra[(r.start + 1 + k) as usize] as usize] = true;
        }
    }
    (0..u32_of(b.local_ty.len()))
        .filter(|&l| {
            b.local_flags[l as usize] & crate::ir::local_flags::PARAM != 0 && !inner[l as usize]
        })
        .collect()
}

/// Stage 8 (#1118): whether `Call` `call` of `b` may run its callee's
/// exclusive copy, which borrows through its parameters in `relies`
/// across calls that may write. Each such argument must be one that stage
/// 7 keeps exclusive for the call: `&` or `&mut` of a part of a local no
/// closure captures, or of the referent of a parameter of `b` in `own`
/// (those its own exclusive copy relies on; none in a plain body), or that
/// parameter's reference itself. Only fields and variant payloads may
/// follow: no other dereference, no element, no top-level binding. A body
/// with a suspension point keeps the plain callee.
#[must_use]
pub fn exclusive_call(b: &Body, call: usize, relies: u64, own: u64) -> bool {
    if b.tags[call] != Tag::Call || b.tags.iter().any(|&t| crate::ir::is_suspension(t)) {
        return false;
    }
    let params = params(b);
    let exclusive_param = |l: u32| {
        (params.iter().position(|&p| p == l)).is_some_and(|k| k < 64 && own & (1 << k) != 0)
    };
    // The local a value reads whole.
    let whole = |i: usize| {
        let p = (b.tags[i] == Tag::Read).then(|| b.view(b.data[i][0]))?;
        if p.whole() { p.local() } else { None }
    };
    let words = b.record(b.data[call][1]);
    let args = &words[..words.len().saturating_sub(3)];
    (0..64).filter(|k| relies & (1 << k) != 0).all(|k| {
        let Some(i) = args
            .get(k)
            .and_then(|&v| Ref(v).as_inst())
            .map(|r| r.0 as usize)
        else {
            return false;
        };
        if b.tags[i] == Tag::Read {
            return whole(i).is_some_and(exclusive_param);
        }
        if b.tags[i] != Tag::Ref {
            return false;
        }
        let p = b.view(b.data[i][0]);
        let mut steps = p.steps(b).peekable();
        let deref = steps.next_if_eq(&crate::ir::Proj::Deref).is_some();
        let root = match p.root {
            Root::Local(l) if deref => exclusive_param(l),
            // A closure's capture is the only way to it besides the call.
            Root::Local(l) => !b.cap_local.iter().any(|c| c.raw() == l),
            Root::Value(u) => deref && whole(u as usize).is_some_and(exclusive_param),
            _ => false,
        };
        root && steps.all(|s| matches!(s, crate::ir::Proj::Field(_) | crate::ir::Proj::Variant(_)))
    })
}

/// The parameter every result of `b` is ([`Summary::returns`]), read
/// before the passes: each `Return` of its own sub-body and its root's
/// tail are a whole read of that parameter, which no `Store`, `Ref`,
/// change in place or shared capture touches. A root with no tail must
/// never complete.
#[must_use]
pub fn returned_param(b: &Body) -> Option<u8> {
    let whole = |v: u32| -> Option<u32> {
        let i = Ref(v).as_inst()?.0 as usize;
        let p = b.view(b.data[i][0]);
        (b.tags[i] == Tag::Read && p.whole()).then_some(p.local()?)
    };
    let root = *b.sub_root.first()? as usize;
    let mut at = match b.data[root][1] {
        NONE if b.ty[root] == Ty::NEVER => NONE,
        NONE => return None,
        tail => whole(tail)?,
    };
    let topo = Topology::new(b);
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Return && topo.sub[i] == 0) {
        let l = whole(b.data[i][0])?;
        if at != NONE && at != l {
            return None;
        }
        at = l;
    }
    let shared = |c: usize| b.cap_local[c].raw() == at && b.cap_mode[c] == CaptureMode::Shared;
    let touched = |i: usize| {
        matches!(b.tags[i], Tag::Store | Tag::Ref | Tag::IsUnique)
            && b.local_in_place(b.data[i][0]).map(hd_base::LocalId::raw) == Some(at)
    };
    if (0..b.cap_local.len()).any(shared) || (0..b.len()).any(touched) {
        return None;
    }
    let k = params(b).iter().position(|&l| l == at)?;
    u8::try_from(k).ok()
}

/// Which same-body allocation an address-like value comes from. `Many` keeps
/// the analysis conservative for a value built from more than one allocation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AllocationSeed {
    #[default]
    None,
    One(u32),
    Many,
}

impl AllocationSeed {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, seed) | (seed, Self::None) => seed,
            (Self::One(a), Self::One(b)) if a == b => Self::One(a),
            _ => Self::Many,
        }
    }

    fn overlaps(self, other: Self) -> bool {
        match (self, other) {
            (Self::None, _) | (_, Self::None) => false,
            (Self::One(a), Self::One(b)) => a == b,
            _ => true,
        }
    }
}

/// Whether an address-like value is derived only from an allocation this body
/// made, whether that derivation has reached an allocation, and which one.
/// `safe` without `seeded` is the optimistic edge of a loop-carried local; a
/// local is fresh only when another one of its stores supplies the seed.
#[derive(Clone, Copy, Debug, Default)]
struct AllocationOrigin {
    safe: bool,
    seeded: bool,
    seed: AllocationSeed,
}

impl AllocationOrigin {
    const NONE: Self = Self {
        safe: false,
        seeded: false,
        seed: AllocationSeed::None,
    };
    const LOOP: Self = Self {
        safe: true,
        seeded: false,
        seed: AllocationSeed::None,
    };

    fn from_seed(seed: u32) -> Self {
        Self {
            safe: true,
            seeded: true,
            seed: AllocationSeed::One(seed),
        }
    }

    fn fresh(self) -> bool {
        self.safe && self.seeded
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum OriginState {
    #[default]
    New,
    Visiting,
    Done(AllocationOrigin),
}

/// The same-body allocation provenance of values and locals. It is deliberately
/// narrow: fields preserve their base, `+` and `-` preserve an address operand,
/// and [`CallMemory::Derive`] names compiler-supplied address projections.
struct AllocationOrigins<'a> {
    body: &'a Body,
    cx: &'a dyn OwnCx,
    values: Vec<OriginState>,
    locals: Vec<OriginState>,
}

impl<'a> AllocationOrigins<'a> {
    fn new(body: &'a Body, cx: &'a dyn OwnCx) -> Self {
        Self {
            body,
            cx,
            values: vec![OriginState::New; body.len()],
            locals: vec![OriginState::New; body.local_ty.len()],
        }
    }

    fn call_args(&self, i: usize) -> &[u32] {
        let words = self.body.record(self.body.data[i][1]);
        &words[..words.len().saturating_sub(3)]
    }

    fn owns_pointer_place(&mut self, word: u32, before: usize) -> bool {
        let place = self.body.view(word);
        place.in_place(self.body)
            && matches!(place.root, Root::Ptr(value) if self.fresh_before(value, before))
    }

    fn derived(&mut self, values: impl IntoIterator<Item = u32>) -> AllocationOrigin {
        let mut out = AllocationOrigin::NONE;
        for value in values {
            let origin = self.value(value);
            if origin.safe {
                out.safe = true;
                out.seeded |= origin.seeded;
                out.seed = out.seed.merge(origin.seed);
            }
        }
        out
    }

    fn local(&mut self, local: u32) -> AllocationOrigin {
        let k = local as usize;
        let Some(state) = self.locals.get(k).copied() else {
            return AllocationOrigin::NONE;
        };
        match state {
            OriginState::Visiting => return AllocationOrigin::LOOP,
            OriginState::Done(origin) => return origin,
            OriginState::New => {}
        }
        if self.body.local_flags[k] & crate::ir::local_flags::PARAM != 0 {
            self.locals[k] = OriginState::Done(AllocationOrigin::NONE);
            return AllocationOrigin::NONE;
        }
        self.locals[k] = OriginState::Visiting;
        let mut out = AllocationOrigin::LOOP;
        let mut found = false;
        for i in 0..self.body.len() {
            if self.body.tags[i] != Tag::Store {
                continue;
            }
            let place = self.body.view(self.body.data[i][0]);
            if place.local() != Some(local) {
                continue;
            }
            found = true;
            let origin = if place.whole() {
                self.value(self.body.data[i][1])
            } else {
                AllocationOrigin::NONE
            };
            out.safe &= origin.safe;
            out.seeded |= origin.seeded;
            out.seed = out.seed.merge(origin.seed);
        }
        if !found {
            out = AllocationOrigin::NONE;
        }
        // A safe, unseeded answer may be the open edge of a mutual local
        // cycle. Recompute it after the outer local has found its seed.
        self.locals[k] = if out.safe && !out.seeded {
            OriginState::New
        } else {
            OriginState::Done(out)
        };
        out
    }

    fn value(&mut self, value: u32) -> AllocationOrigin {
        let Some(inst) = Ref(value).as_inst() else {
            return AllocationOrigin::NONE;
        };
        let i = inst.0 as usize;
        let Some(state) = self.values.get(i).copied() else {
            return AllocationOrigin::NONE;
        };
        match state {
            OriginState::Visiting => return AllocationOrigin::LOOP,
            OriginState::Done(origin) => return origin,
            OriginState::New => {}
        }
        self.values[i] = OriginState::Visiting;
        let [a, w] = self.body.data[i];
        let origin = match self.body.tags[i] {
            Tag::Call => {
                Callee::from_words(self.body.record(a)).map_or(AllocationOrigin::NONE, |callee| {
                    match self.cx.call_memory(&callee) {
                        CallMemory::Allocate => AllocationOrigin::from_seed(u32_of(i)),
                        CallMemory::Derive(k) => self
                            .call_args(i)
                            .get(k as usize)
                            .copied()
                            .map_or(AllocationOrigin::NONE, |v| self.value(v)),
                        CallMemory::None | CallMemory::Write(_) => AllocationOrigin::NONE,
                    }
                })
            }
            Tag::Read => {
                let place = self.body.view(a);
                if place.in_place(self.body) {
                    match place.root {
                        Root::Local(local) => self.local(local),
                        Root::Value(value) => self.value(value),
                        _ => AllocationOrigin::NONE,
                    }
                } else {
                    AllocationOrigin::NONE
                }
            }
            Tag::Prim
                if matches!(
                    crate::ir::PrimOp::from_u32(a),
                    Some(crate::ir::PrimOp::Add | crate::ir::PrimOp::Sub)
                ) =>
            {
                self.derived(self.body.record(w).to_vec())
            }
            Tag::Coerce => self.value(a),
            Tag::NewStruct | Tag::NewTuple | Tag::CopyStruct => self
                .body
                .value_operands(i)
                .map_or(AllocationOrigin::NONE, |values| self.derived(values)),
            Tag::Block if w != NONE => self.value(w),
            _ => AllocationOrigin::NONE,
        };
        self.values[i] = if origin.safe && !origin.seeded {
            OriginState::New
        } else {
            OriginState::Done(origin)
        };
        origin
    }

    /// Whether `value` still names only same-body storage just before
    /// instruction `before`. A whole local binding merely carries the value;
    /// any earlier consuming store, construction or call can make the
    /// allocation reachable from another value and ends its freshness.
    fn fresh_before(&mut self, value: u32, before: usize) -> bool {
        let origin = self.value(value);
        if !origin.fresh() || origin.seed == AllocationSeed::None {
            return false;
        }
        let mut ops = Vec::new();
        for i in 0..before {
            if self.body.tags[i] == Tag::Store {
                let place = self.body.view(self.body.data[i][0]);
                if place.whole() && matches!(place.root, Root::Local(_)) {
                    continue;
                }
            }
            operands(self.body, i, &mut ops);
            for op in &ops {
                if op.role == Role::Tail || !consumes(self.body, self.cx, i, op, NONE) {
                    continue;
                }
                if self.value(op.value).seed.overlaps(origin.seed) {
                    return false;
                }
            }
        }
        true
    }
}

/// Whether `b` may write memory its caller can reach ([`Summary::writes`]):
/// a store, uniqueness check, pin or move of a place that is not its own,
/// a `&mut` or element reference of one (the passes pin its lists), an
/// in-place change of a collection read from one, a call `cx` says may
/// write, or a call it cannot name (a function value, a trait value, the
/// host, a provider, a default, a suspension). A place is its own when it
/// is rooted at a local or a fresh value (a copy included), through the
/// reads that name their place, with no dereference: its caller reaches
/// neither these nor a buffer they make unique first.
#[must_use]
pub fn writes(b: &Body, cx: &dyn OwnCx) -> bool {
    writes_with(b, 0..b.len(), &|c| cx.may_write(c), Some(cx))
}

/// Whether an instruction of `b` in `range` may write memory its caller
/// can reach, as [`writes`] reads them, with `may_write` for a call.
#[must_use]
pub fn writes_in(
    b: &Body,
    range: std::ops::Range<usize>,
    may_write: &dyn Fn(&Callee) -> bool,
) -> bool {
    writes_with(b, range, may_write, None)
}

fn writes_with(
    b: &Body,
    mut range: std::ops::Range<usize>,
    may_write: &dyn Fn(&Callee) -> bool,
    cx: Option<&dyn OwnCx>,
) -> bool {
    // Whether a place is the body's own, and whether its path holds an
    // element.
    let place = |mut word: u32| {
        let (mut own, mut elem) = (true, false);
        for _ in 0..b.len() {
            let p = b.view(word);
            let (derefs, e) = p.kinds(b);
            (own, elem) = (own && derefs == 0, elem || e);
            match p.root {
                // A read that names its place, not a copy or move of it.
                Root::Value(v)
                    if b.tags[v as usize] == Tag::Read
                        && matches!(
                            ReadMode::from_u32(b.data[v as usize][1]),
                            Some(ReadMode::Use | ReadMode::Borrow)
                        ) =>
                {
                    word = b.data[v as usize][0];
                }
                Root::Local(_) | Root::Value(_) => return (own, elem),
                _ => break,
            }
        }
        (false, true)
    };
    let read = |v: u32| {
        Ref(v)
            .as_inst()
            .filter(|i| b.tags[i.0 as usize] == Tag::Read)
    };
    let mut origins = cx.map(|cx| AllocationOrigins::new(b, cx));
    range.any(|i| {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Store | Tag::IsUnique | Tag::FlagSet | Tag::FlagClear => {
                !place(a).0
                    && !origins
                        .as_mut()
                        .is_some_and(|origins| origins.owns_pointer_place(a, i))
            }
            // A move out of a place the body does not own; before the
            // passes, a read of a pointer's target is one too, since
            // `Ptr.read` moves its value out (`module.rt.ptr.read`).
            Tag::Read => {
                (w == ReadMode::Move as u32
                    || (w == ReadMode::Use as u32 && matches!(b.view(a).root, Root::Ptr(_))))
                    && !place(a).0
                    && !origins
                        .as_mut()
                        .is_some_and(|origins| origins.owns_pointer_place(a, i))
            }
            Tag::Ref => {
                matches!(place(a), (false, elem) if elem || w & crate::ir::ref_word::MUT != 0)
                    && !origins
                        .as_mut()
                        .is_some_and(|origins| origins.owns_pointer_place(a, i))
            }
            Tag::Intrinsic => {
                matches!(IntrinsicOp::from_u32(a), None | Some(IntrinsicOp::Item))
                    || changed(b, i)
                        .any(|(_, v)| read(v).is_none_or(|r| !place(b.data[r.0 as usize][0]).0))
            }
            Tag::Call => Callee::from_words(b.record(a)).is_none_or(|callee| {
                let Some(origins) = origins.as_mut() else {
                    return may_write(&callee);
                };
                match origins.cx.call_memory(&callee) {
                    CallMemory::Allocate | CallMemory::Derive(_) => false,
                    CallMemory::Write(k) => !origins
                        .call_args(i)
                        .get(k as usize)
                        .copied()
                        .is_some_and(|v| origins.fresh_before(v, i)),
                    CallMemory::None => may_write(&callee),
                }
            }),
            Tag::CallValue
            | Tag::CallDyn
            | Tag::CallHost
            | Tag::DefaultCall
            | Tag::Hook
            | Tag::ProviderGet
            | Tag::With
            | Tag::ContextNew
            | Tag::ContextFor => true,
            t => crate::ir::is_suspension(t),
        }
    })
}

/// The summary of `b` after [`run`] over it, from the borrow pass's tables
/// in `scratch`, with `returns` from [`returned_param`] of the body before
/// the passes. `None` for a body that is no function's, or that suspends.
#[must_use]
pub fn summary(
    b: &Body,
    cx: &dyn OwnCx,
    scratch: &OwnScratch,
    returns: Option<u8>,
) -> Option<Summary> {
    let (kept, borrows) = borrow::kept(b, cx, &scratch.borrow)?;
    Some(Summary {
        kept,
        borrows,
        returns,
        writes: true,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt::Write as _;

    use super::{OwnScratch, Pass, Tri, run};
    use crate::ir::{
        Body, BodyKind, Callee, NONE, Providers, ReadMode, Tag, TirBuilder, TirSink, local_flags,
        verify_owned,
    };
    use hd_base::SliceSort as _;
    use hd_base::{DefId, LocalId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    /// A list type: counted, and changed in place by a push.
    const LIST: Ty = Ty(1_000_000);
    /// A counted type declared `!Copy`.
    const UNIQ: Ty = Ty(1_000_001);
    /// A counted type that holds a `?Copy` type parameter.
    const OPEN: Ty = Ty(1_000_002);
    /// A non-copyable value with no counted fields or drop hook.
    const UNCOUNTED: Ty = Ty(1_000_003);

    /// `string` and `LIST` are counted; callee 9 takes its argument as a value
    /// parameter, callee 8 by reference.
    pub(crate) struct Cx;

    impl super::OwnCx for Cx {
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == LIST || ty == UNIQ || ty == OPEN
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
        fn holds_coll(&self, ty: Ty) -> bool {
            ty == LIST
        }
        fn local_ref_param(&self, _: DefId, _: usize, _: usize) -> bool {
            true
        }
        fn copies(&self, ty: Ty) -> Tri {
            match ty {
                UNIQ | UNCOUNTED => Tri::No,
                OPEN => Tri::Maybe,
                _ => Tri::Yes,
            }
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
    pub(crate) fn show(b: &Body) -> String {
        let mut out = String::new();
        let mut ops = Vec::new();
        for i in 0..b.len() {
            let tag = b.tags[i];
            let mut line = format!("%{i} {}", tag.name());
            if matches!(
                tag,
                Tag::Read | Tag::Drop | Tag::Store | Tag::Ref | Tag::IsUnique
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
                if tag == Tag::Ref && b.data[i][1] & crate::ir::ref_word::FRAME != 0 {
                    line.push_str(" frame");
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
        for (at, l) in &b.full_cells {
            let _ = writeln!(out, "full %{at} ${l}");
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
    /// since the backend reads a parameter outside it as unowned. The two
    /// values the call lends are dropped after it, the younger first. (A
    /// block lists the blocks its statements run; they are not statements.)
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
             %5 Read move [$0]\n%6 Call %4 %5\n%7 Drop [%5]\n%8 Drop [%4]\n\
             %9 Block [%0 %1 %2 %3 %4 %5 %6 %7 %8]\nexit 0 $0\n"
        );
    }

    /// The copy verifier finds each `copy` read of a type that is not
    /// `Copy`, whether every instance or only some copy one; a `move` and
    /// a copyable type's copy are fine.
    #[test]
    fn the_copy_verifier_finds_each_copy_of_a_non_copyable_value() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let n = NodeIdx::from_raw(1);
        for ty in [UNIQ, OPEN, Ty::STRING] {
            let x = b.local(ty, Symbol::from_raw(0), 0, n);
            let made = call(&mut b, 7, &[], ty);
            b.set(x, made, n);
            let first = b.get(x, ty, n);
            call(&mut b, 9, &[first], Ty::VOID);
            let second = b.get(x, ty, n);
            call(&mut b, 9, &[second], Ty::VOID);
        }
        let root = b.close_block(blk, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        let mut scratch = OwnScratch::default();
        run(&mut body, &Cx, &mut scratch).expect("passes");
        let text = show(&body);
        let found: Vec<(String, Tri)> = scratch
            .uncopied()
            .iter()
            .map(|u| {
                (
                    text.lines().nth(u.inst as usize).unwrap_or("").to_owned(),
                    u.copies,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("%2 Read copy [$0]".to_owned(), Tri::No),
                ("%8 Read copy [$1]".to_owned(), Tri::Maybe)
            ],
            "{text}"
        );
        assert!(text.contains("%4 Read move [$0]"), "{text}");
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
    /// first); a push through an element names the element's place, which
    /// it reads again after the checks; and an `IsUnique` of
    /// the list before an element reference. The element reference addresses the buffer, not the
    /// local, so the local (of a type with no counted leaf here) takes no
    /// cell and no drop.
    #[test]
    fn changes_and_element_references_make_their_collection_unique() {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let us = Ty::prim(hd_types::Prim::Usize);
        let text = passes(
            |b| {
                let xs = b.local(us, Symbol::from_raw(0), local_flags::PARAM, n);
                let list = call(b, 6, &[], us);
                let i = call(b, 6, &[], us);
                let e = b.element(list, i, us, n);
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
            "%0 Call\n%1 Call\n%2 Read borrow [%0 [%1]]\n%3 IsUnique [%0]\n\
             %4 IsUnique [%0 [%1]]\n%5 Read borrow [%0 [%1]]\n%6 Intrinsic %5 %1\n\
             %7 IsUnique [%0]\n%8 IsUnique [%0 [%1] .0]\n%9 Store [%0 [%1] .0 [%1]] %1\n\
             %10 IsUnique [$0]\n%11 Ref [$0 [%1]]\n\
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

    /// A non-copyable temporary needs moves even without a runtime drop:
    /// whole reads and payload projections must not become copies.
    #[test]
    fn uncounted_noncopyable_temporaries_move_whole_and_projected() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let source = call(b, 7, &[], UNCOUNTED);
                let word = b.place(&format!("[%{} as .1 .0]", source.0).parse().expect("place"));
                let part = b.emit(Tag::Read, word, ReadMode::Use as u32, UNCOUNTED, n);
                call(b, 9, &[part], Ty::VOID);
                let source = call(b, 7, &[], UNCOUNTED);
                let word = b.place(&format!("[%{}]", source.0).parse().expect("place"));
                let whole = b.emit(Tag::Read, word, ReadMode::Use as u32, UNCOUNTED, n);
                call(b, 9, &[whole], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert!(text.contains("Read move [%0 as .1 .0]"), "{text}");
        assert!(text.contains("Read move [%3]"), "{text}");
        assert!(!text.contains("Read copy"), "{text}");
        assert!(!text.contains("Drop"), "{text}");
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

    /// References block a partial move only while their uses still reach
    /// the field. A deliberately live overlap must remain a copy.
    #[test]
    fn field_moves_wait_for_overlapping_reference_uses() {
        for live in [false, true] {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let owner = b.local(OPEN, Symbol::from_raw(0), 0, n);
            let value = call(&mut b, 7, &[], OPEN);
            b.set(owner, value, n);
            let whole = b.place(&"[$0]".parse().expect("place"));
            let reference = b.emit(Tag::Ref, whole, crate::ir::ref_word::LOCAL, Ty::I32, n);
            if !live {
                call(&mut b, 8, &[reference], Ty::VOID);
            }
            let at = b.place(&"[$0 .0]".parse().expect("place"));
            let field = b.emit(Tag::Read, at, ReadMode::Use as u32, UNIQ, n);
            call(&mut b, 9, &[field], Ty::VOID);
            if live {
                call(&mut b, 8, &[reference], Ty::VOID);
            }
            let root = b.close_block(block, None, Ty::VOID, n);
            let mut body = b.finish(root, &[]).expect("verifies");
            run(&mut body, &Cx, &mut OwnScratch::default()).expect("passes");
            let i = (0..body.len())
                .find(|&i| body.tags[i] == Tag::Read && body.ty[i] == UNIQ)
                .expect("field read");
            assert_eq!(
                body.data[i][1],
                if live { ReadMode::Copy } else { ReadMode::Move } as u32,
                "{}",
                show(&body)
            );
        }
    }

    #[test]
    fn field_reference_liveness_has_no_machine_word_field_limit() {
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        let owner = b.local(OPEN, Symbol::from_raw(0), 0, n);
        let value = call(&mut b, 7, &[], OPEN);
        b.set(owner, value, n);
        let whole = b.place(&"[$0]".parse().expect("place"));
        let reference = b.emit(Tag::Ref, whole, crate::ir::ref_word::LOCAL, Ty::I32, n);
        call(&mut b, 8, &[reference], Ty::VOID);
        for field in 0..66 {
            let at = b.place(&format!("[$0 .{field}]").parse().expect("place"));
            let value = b.emit(Tag::Read, at, ReadMode::Use as u32, UNIQ, n);
            call(&mut b, 9, &[value], Ty::VOID);
        }
        let root = b.close_block(block, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        let mut scratch = OwnScratch::default();
        run(&mut body, &Cx, &mut scratch).expect("passes");
        assert!(scratch.uncopied().is_empty(), "{}", show(&body));
        assert_eq!(
            (0..body.len())
                .filter(|&i| body.tags[i] == Tag::Read
                    && body.ty[i] == UNIQ
                    && body.data[i][1] == ReadMode::Move as u32)
                .count(),
            66
        );
    }

    /// Exclusive mode (#1118): a read through a `&T` parameter borrows across
    /// a call that may write, unless the call receives that reference.
    #[test]
    fn exclusive_mode_borrows_through_a_reference_parameter_across_a_call() {
        use crate::ir::sub_flags::EXCLUSIVE;
        let n = NodeIdx::from_raw(1);
        let build = |receive: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let p = local(&mut b, local_flags::PARAM);
            let word = b.place(&"[*$0 .0]".parse().expect("place"));
            let r = b.emit(Tag::Read, word, ReadMode::Use as u32, Ty::STRING, n);
            let args = if receive {
                vec![b.get(p, Ty::STRING, n)]
            } else {
                vec![]
            };
            call(&mut b, 8, &args, Ty::VOID);
            call(&mut b, 8, &[r], Ty::VOID);
            // Keeps the owned parameter, and so its referent, past the span.
            let whole = b.get(p, Ty::STRING, n);
            call(&mut b, 8, &[whole], Ty::VOID);
            let root = b.close_block(blk, None, Ty::VOID, n);
            b.finish(root, &[]).expect("verifies")
        };
        let passes = |mut body: Body, excl: bool| {
            if excl {
                body.sub_flags[0] |= EXCLUSIVE;
            }
            let mut scratch = OwnScratch::default();
            run(&mut body, &Cx, &mut scratch).expect("passes");
            let errs = verify_owned(&body, &Cx);
            assert!(errs.is_empty(), "{errs:?}\n{}", show(&body));
            (show(&body), scratch.relies(), body)
        };
        let (text, relies, _) = passes(build(false), false);
        assert!(text.contains("Read copy [*$0 .0]") && relies == 0, "{text}");
        let (text, relies, _) = passes(build(true), true);
        assert!(text.contains("Read copy [*$0 .0]") && relies == 0, "{text}");
        let (text, relies, mut body) = passes(build(false), true);
        assert!(
            text.contains("Read borrow [*$0 .0]") && relies == 1,
            "{text}"
        );
        body.sub_flags[0] &= !EXCLUSIVE;
        assert!(!verify_owned(&body, &Cx).is_empty(), "{text}");
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

    /// The verifier wants a change's operand to read its place right after
    /// the place's `IsUnique`, with nothing that may write between that
    /// read and the change.
    #[test]
    fn the_verifier_wants_a_change_to_read_its_place_after_its_check() {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let build = |unique: bool, call_between: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let a = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, n);
            let word = a.raw() | crate::ir::place::BARE_LOCAL;
            let s = call(&mut b, 7, &[], Ty::STRING);
            if unique {
                b.emit(Tag::IsUnique, word, 0, Ty::VOID, n);
            }
            let h = b.emit(Tag::Read, word, ReadMode::Borrow as u32, LIST, n);
            if call_between {
                call(&mut b, 8, &[], Ty::VOID);
            }
            let rec = b.refs_record(&[h, s]);
            b.emit(
                Tag::Intrinsic,
                IntrinsicOp::ListPush as u32,
                rec,
                Ty::VOID,
                n,
            );
            let root = b.close_block(blk, None, Ty::VOID, n);
            b.finish_tolerating(root, &[], &[22]).expect("valid")
        };
        let found = |body: &Body| {
            verify_owned(body, &Cx)
                .iter()
                .any(|e| e.what.contains("no read of its place"))
        };
        assert!(!found(&build(true, false)));
        assert!(found(&build(false, false)));
        assert!(found(&build(true, true)));
    }

    /// An `IsUnique` rooted at a value stores the buffer it leaves in that
    /// value alone, so the verifier rejects one whose value reads a local's
    /// place, which would keep the old buffer, and accepts a call's result.
    #[test]
    fn the_verifier_wants_a_check_rooted_at_a_value_to_read_no_place() {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let build = |from_local: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let a = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, n);
            let s = call(&mut b, 7, &[], Ty::STRING);
            let h = if from_local {
                let word = a.raw() | crate::ir::place::BARE_LOCAL;
                b.emit(Tag::Read, word, ReadMode::Borrow as u32, LIST, n)
            } else {
                call(&mut b, 8, &[], LIST)
            };
            let word = b.place(&format!("[%{}]", h.0).parse().expect("place"));
            b.emit(Tag::IsUnique, word, 0, Ty::VOID, n);
            let rec = b.refs_record(&[h, s]);
            let push = IntrinsicOp::ListPush as u32;
            b.emit(Tag::Intrinsic, push, rec, Ty::VOID, n);
            let root = b.close_block(blk, None, Ty::VOID, n);
            b.finish_tolerating(root, &[], &[22]).expect("valid")
        };
        let found = |body: &Body| {
            verify_owned(body, &Cx)
                .iter()
                .any(|e| e.what.contains("leaves its buffer in no place"))
        };
        assert!(found(&build(true)));
        assert!(!found(&build(false)));
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

    /// A whole-value read after an earlier reference moves the payload out
    /// of the cell at its last access. The backend clears and releases the
    /// cell, so an uncounted owned value is transferred rather than copied
    /// and then dropped.
    #[test]
    fn a_last_read_moves_out_of_a_cell() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let word = b.place(&format!("[${}]", x.raw()).parse().expect("place"));
                let r = b.emit(Tag::Ref, word, 0, Ty::STRING, n);
                call(b, 8, &[r], Ty::VOID);
                let moved = b.get(x, Ty::STRING, n);
                call(b, 9, &[moved], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert!(!text.contains("Ref take [$0]"), "{text}");
        assert!(text.contains("Read move [$0]"), "{text}");
        assert!(!text.contains("Drop [$0]"), "{text}");
    }

    /// A frame local (`frame`): a local reference to `$0` bound to `$1`
    /// keeps `$0` alive until the last read through `$1`, past the
    /// reference's own statement, and never takes its count. A reference
    /// that is not local (no `ref_word::LOCAL`) leaves `$0` in its cell,
    /// taken at its last access.
    #[test]
    fn a_frame_local_lives_until_the_last_read_through_its_reference() {
        let n = NodeIdx::from_raw(1);
        let body = |bits: u32| {
            passes(
                move |b| {
                    let x = local(b, 0);
                    let r = local(b, 0);
                    let s = call(b, 7, &[], Ty::STRING);
                    b.set(x, s, n);
                    let word = b.place(&format!("[${}]", x.raw()).parse().expect("place"));
                    let at = b.emit(Tag::Ref, word, bits, Ty::STRING, n);
                    b.set(r, at, n);
                    let through = b.place(&format!("[*${}]", r.raw()).parse().expect("place"));
                    let v = b.emit(Tag::Read, through, ReadMode::Use as u32, Ty::STRING, n);
                    call(b, 8, &[v], Ty::VOID);
                    None
                },
                Ty::VOID,
            )
        };
        let frame = body(crate::ir::ref_word::LOCAL);
        let x_drop = frame.find("Drop [$0]").expect("a drop of $0");
        assert!(frame.contains("Ref frame [$0]"), "{frame}");
        assert!(
            frame.find("Read copy [*$1]").expect("the read") < x_drop,
            "{frame}"
        );
        let counted = body(0);
        assert!(counted.contains("Ref take [$0]"), "{counted}");
    }

    /// Known-null's rule 5: a celled local is full once a store ran on
    /// every path. A store on one branch leaves it unknown, so neither
    /// that store nor the next one gets a row; the reference after both
    /// does.
    #[test]
    fn a_cell_is_full_after_a_store_on_every_path() {
        let n = NodeIdx::from_raw(1);
        let text = passes(
            |b| {
                let x = local(b, 0);
                let c = call(b, 6, &[], Ty::BOOL);
                let then = b.open_block();
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let then = b.close_block(then, None, Ty::VOID, n);
                let arms = b.refs_record(&[then]);
                b.emit(Tag::If, c.0, arms, Ty::VOID, n);
                let s = call(b, 7, &[], Ty::STRING);
                b.set(x, s, n);
                let word = b.place(&format!("[${}]", x.raw()).parse().expect("place"));
                let r = b.emit(Tag::Ref, word, 0, Ty::STRING, n);
                call(b, 8, &[r], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("full ")).collect();
        let at = text
            .lines()
            .find(|l| l.contains(" Ref "))
            .and_then(|l| l.split(' ').next())
            .expect("the reference");
        assert_eq!(rows, [format!("full {at} $0")], "{text}");
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

    /// [`Cx`] for a body that does not suspend, which has a summary.
    struct Flat;

    impl super::OwnCx for Flat {
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
        }
        fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
            Cx.pass(callee, k, n)
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, callee: &Callee) -> bool {
            Cx.may_write(callee)
        }
        fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, v, out)
        }
        fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
            Cx.received(item, k, n)
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    /// A flat context whose four small definition IDs model the raw-memory
    /// relations used by the write-summary tests below.
    struct Memory;

    impl super::OwnCx for Memory {
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
        }
        fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
            match callee {
                Callee::Item { def, .. } if def.raw() == 13 => Pass::Owned,
                _ => Cx.pass(callee, k, n),
            }
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn call_memory(&self, callee: &Callee) -> super::CallMemory {
            match callee {
                Callee::Item { def, .. } => match def.raw() {
                    10 => super::CallMemory::Allocate,
                    11 => super::CallMemory::Derive(0),
                    12 => super::CallMemory::Write(0),
                    _ => super::CallMemory::None,
                },
                Callee::TraitMethod { .. } => super::CallMemory::None,
            }
        }
        fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, v, out)
        }
        fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
            Cx.received(item, k, n)
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    /// A body of two string parameters built by `f`, its returned
    /// parameter, and its summary after the passes.
    fn summarized(
        f: impl FnOnce(&mut TirBuilder, LocalId, LocalId) -> Option<crate::ir::Ref>,
        ty: Ty,
    ) -> (Option<u8>, Option<super::Summary>) {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let p0 = local(&mut b, local_flags::PARAM);
        let p1 = local(&mut b, local_flags::PARAM);
        let tail = f(&mut b, p0, p1);
        let root = b.close_block(blk, tail, ty, NodeIdx::from_raw(1));
        let mut body = b.finish(root, &[]).expect("verifies");
        let returns = super::returned_param(&body);
        let mut scratch = OwnScratch::default();
        run(&mut body, &Flat, &mut scratch).expect("passes");
        (returns, super::summary(&body, &Flat, &scratch, returns))
    }

    /// A parameter that only lends is not kept, one an owned argument
    /// takes is; a body whose result is a parameter's whole read returns
    /// it, unless it stores that parameter. Two summaries meet in what
    /// both allow.
    #[test]
    fn a_summary_keeps_what_the_body_holds_and_names_the_returned_parameter() {
        let n = NodeIdx::from_raw(1);
        let (returns, s) = summarized(
            |b, p0, p1| {
                let a = b.get(p0, Ty::STRING, n);
                call(b, 8, &[a], Ty::VOID);
                let c = b.get(p1, Ty::STRING, n);
                call(b, 9, &[c], Ty::VOID);
                None
            },
            Ty::VOID,
        );
        assert_eq!(returns, None);
        assert_eq!(s.map(|s| (s.kept, s.borrows)), Some((0b10, 0b01)));
        let (returns, s) = summarized(|b, _, p1| Some(b.get(p1, Ty::STRING, n)), Ty::STRING);
        assert_eq!((returns, s.and_then(|s| s.returns)), (Some(1), Some(1)));
        let (returns, _) = summarized(
            |b, p0, _| {
                let s = call(b, 7, &[], Ty::STRING);
                b.set(p0, s, n);
                Some(b.get(p0, Ty::STRING, n))
            },
            Ty::STRING,
        );
        assert_eq!(returns, None);
        let a = super::Summary {
            kept: 0b01,
            borrows: 0b10,
            returns: Some(0),
            writes: false,
        };
        let b = super::Summary {
            kept: 0b10,
            borrows: 0b01,
            returns: Some(1),
            writes: true,
        };
        assert_eq!(
            a.meet(b),
            super::Summary {
                kept: 0b11,
                borrows: 0,
                returns: None,
                writes: true
            }
        );
        assert_eq!(a.meet(a), a);
    }

    /// A summary may tentatively lend a parameter through a binding, but a
    /// binding passed on as an owned value cannot borrow. The parameter
    /// must keep its ownership until move transfers it into the binding.
    #[test]
    fn a_rejected_binding_borrow_restores_its_parameters_ownership() {
        struct Summarizing;
        impl super::OwnCx for Summarizing {
            fn counted(&self, ty: Ty) -> bool {
                Cx.counted(ty)
            }
            fn pass(&self, c: &Callee, k: usize, n: usize) -> Pass {
                Cx.pass(c, k, n)
            }
            fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
                Cx.pass_value(ty, k, n)
            }
            fn may_write(&self, c: &Callee) -> bool {
                Cx.may_write(c)
            }
            fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
                Cx.parts(ty, v, out)
            }
            fn copies(&self, ty: Ty) -> Tri {
                Cx.copies(ty)
            }
            fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
                Cx.received(item, k, n)
            }
            fn holds_coll(&self, ty: Ty) -> bool {
                Cx.holds_coll(ty)
            }
            fn suspending(&self, _: DefId) -> bool {
                false
            }
            fn summarizes(&self) -> bool {
                true
            }
        }
        let n = NodeIdx::from_raw(1);
        for (ty, callee, mode) in [Ty::STRING, UNIQ, OPEN]
            .into_iter()
            .flat_map(|ty| [(ty, 8, ReadMode::Borrow), (ty, 9, ReadMode::Move)])
        {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let parameter = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, n);
            let binding = b.local(ty, Symbol::from_raw(1), 0, n);
            let read = b.get(parameter, ty, n);
            b.set(binding, read, n);
            let value = b.get(binding, ty, n);
            call(&mut b, callee, &[value], Ty::VOID);
            let root = b.close_block(blk, None, Ty::VOID, n);
            let mut body = b.finish(root, &[]).expect("verifies");
            let mut scratch = OwnScratch::default();
            run(&mut body, &Summarizing, &mut scratch).expect("passes");
            assert!(scratch.uncopied().is_empty(), "{}", show(&body));
            assert_eq!(body.data[0][1], mode as u32, "{}", show(&body));
            let errors = verify_owned(&body, &Summarizing);
            assert!(errors.is_empty(), "{errors:?}\n{}", show(&body));
        }
    }

    /// The writes walk: a store to a local writes nothing the caller
    /// reaches; one through a parameter's reference or to a top-level
    /// binding does, and so does a call that may write.
    #[test]
    fn writes_are_stores_the_caller_reaches_and_calls_that_may_write() {
        use crate::ir::{Place, PlaceRoot, Proj};
        let n = NodeIdx::from_raw(1);
        let writes = |root: Option<PlaceRoot>, proj: Vec<Proj>, calls: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let p = local(&mut b, local_flags::PARAM);
            let v = b.get(p, Ty::STRING, n);
            let root = root.unwrap_or(PlaceRoot::Local(p));
            let word = b.place(&Place { root, proj });
            b.emit(Tag::Store, word, v.0, Ty::VOID, n);
            if calls {
                call(&mut b, 7, &[], Ty::VOID);
            }
            let root = b.close_block(blk, None, Ty::VOID, n);
            super::writes(&b.finish(root, &[]).expect("verifies"), &Flat)
        };
        let global = PlaceRoot::Global(DefId::from_raw(5), Ty::STRING);
        assert!(!writes(None, vec![], false));
        assert!(writes(None, vec![Proj::Deref, Proj::Field(0)], false));
        assert!(writes(Some(global), vec![], false));
        assert!(writes(None, vec![], true));
    }

    /// A raw write stays local only until a consuming operation can carry its
    /// allocation elsewhere. Taking a different fresh allocation does not
    /// taint the one the write reaches.
    #[test]
    fn a_taken_allocation_is_no_longer_fresh_at_a_later_raw_write() {
        let n = NodeIdx::from_raw(1);
        let writes = |take_same: bool, take_other: bool| {
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let blk = b.open_block();
            let value = local(&mut b, 0);
            let allocation = call(&mut b, 10, &[], Ty::STRING);
            b.set(value, allocation, n);
            let held = b.get(value, Ty::STRING, n);
            let address = call(&mut b, 11, &[held], Ty::prim(hd_types::Prim::Usize));
            if take_other {
                let other = call(&mut b, 10, &[], Ty::STRING);
                call(&mut b, 13, &[other], Ty::VOID);
            }
            if take_same {
                let taken = b.get(value, Ty::STRING, n);
                call(&mut b, 13, &[taken], Ty::VOID);
            }
            call(&mut b, 12, &[address], Ty::VOID);
            let root = b.close_block(blk, None, Ty::VOID, n);
            super::writes(&b.finish(root, &[]).expect("verifies"), &Memory)
        };
        assert!(!writes(false, false));
        assert!(writes(true, false));
        assert!(!writes(false, true));
    }

    /// A parameter a push changes stays owned, though no read keeps it:
    /// the change gives up a count of the buffer it replaces. The verifier
    /// rejects such a change of a parameter the body does not own.
    #[test]
    fn a_parameter_changed_in_place_stays_owned() {
        use crate::ir::IntrinsicOp;
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let blk = b.open_block();
        let p = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, n);
        let h = b.get(p, LIST, n);
        let s = call(&mut b, 7, &[], Ty::STRING);
        let rec = b.refs_record(&[h, s]);
        b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            rec,
            Ty::VOID,
            n,
        );
        let root = b.close_block(blk, None, Ty::VOID, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        run(&mut body, &Cx, &mut OwnScratch::default()).expect("passes");
        assert_eq!(verify_owned(&body, &Cx), []);
        assert_eq!(body.exit_drops.len(), 1, "{:?}", body.exit_drops);
        body.exit_drops.clear();
        let errs = verify_owned(&body, &Cx);
        assert!(
            errs.iter()
                .any(|e| e.what.contains("a change in place of $0")),
            "{errs:?}"
        );
    }
    #[test]
    fn every_cell_owner_lives_through_its_references_last_call() {
        let n = NodeIdx::from_raw(1);
        for count in [1, 66] {
            for suspends in [false, true] {
                let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
                let block = b.open_block();
                if suspends {
                    let point = call(&mut b, 8, &[], Ty::VOID);
                    b.body_mut().tags[point.0 as usize] = Tag::Await;
                }
                let mut owners = Vec::new();
                let mut refs = Vec::new();
                for _ in 0..count {
                    let owner = local(&mut b, 0);
                    let value = call(&mut b, 7, &[], Ty::STRING);
                    b.set(owner, value, n);
                    let place = b.place(&format!("[${}]", owner.raw()).parse().expect("place"));
                    refs.push(b.emit(Tag::Ref, place, 0, Ty::STRING, n));
                    owners.push(owner.raw());
                }
                let use_at = call(&mut b, 8, &refs, Ty::VOID).0;
                let root = b.close_block(block, None, Ty::VOID, n);
                let mut body = b.finish(root, &[]).expect("verifies");
                let mut topo = crate::ir::Topology::default();
                topo.build(&body);
                let mut used = Vec::new();
                super::frame::run(
                    &mut body,
                    &topo,
                    &Cx,
                    &mut super::frame::Tables::default(),
                    |i, l, frame| {
                        if i == use_at {
                            assert!(!frame);
                            used.push(l);
                        }
                    },
                );
                used.sort_stable();
                used.dedup();
                // A suspending root's cells hold no frame slot, but a
                // reference into one holds no count of it either.
                assert_eq!(used, owners, "count={count} suspends={suspends}");
            }
        }
    }
}
