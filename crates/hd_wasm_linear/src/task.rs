//! The task runtime of suspending code (`crates/hd_wasm_linear/DESIGN.md`
//! §23.21, job 13): the frame's task prefix and flags, the runtime checks
//! of a poll and a cancel, the wake table, the host's leaf frames, and the
//! entry's driver of a suspending root. These implement the task
//! protocol of §14 over linear memory.

use crate::op::Op as Instruction;
use hd_base::Hash128;
use wasm_encoder::BlockType;

use crate::boundary::LTy;
use crate::layout::{Leaf, Walk};
use crate::module::{BUF, Func, Global, HEAP, Import, Ins, Module, Part, Sym, Val, mem};
use crate::rt::Elem;
use crate::rt::Helper;

/// A frame's task prefix: its poll function's table slot.
pub const F_POLL: u64 = 0;
/// Its cancel function's table slot.
pub const F_CANCEL: u64 = 4;
/// Its resume point: 0 before it started.
pub const F_STATE: u64 = 8;
/// Its flags: [`ACTIVE`] and the others.
pub const F_FLAGS: u64 = 12;
/// The child it waits on, with a count of its own, or 0.
pub const F_CHILD: u64 = 16;
/// The start of a frame's own fields, 8-aligned: a body's save area, a
/// host leaf's handle and arguments.
pub const F_SAVED: u32 = 24;

/// Being polled or cancelled.
pub const ACTIVE: i32 = 1;
/// Completed: its poll answered Ready.
pub const DONE: i32 = 4;
/// Cancelled.
pub const CANCELLED: i32 = 8;
/// Cancelled, with its cleanup not ended: a later cancel continues it.
pub const UNWINDING: i32 = 16;
/// Its cleanup was cut short by `abandon()`.
pub const ABANDONED: i32 = 32;
/// Its body left, by completing or by its cancellation: the save area
/// holds nothing of its own.
pub const EXITED: i32 = 64;
/// Frame slots may differ from the running body's locals. Kept separate
/// from ACTIVE so direct body calls and helper polls do not change the
/// suspension protocol's reentrancy checks. std.rt.cycle reads this bit.
pub const RUNNING: i32 = 128;

/// What `hd.poll` returns for a root that ended cancelled once its
/// cleanup ran to its end; one less when a part of it was cut short.
pub const CANCELLED_END: i32 = -2;

/// A host method's argument (`crates/hd_run_wasmtime/DESIGN.md` §17.4,
/// "Arguments").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HostArg {
    /// A string, the method's only argument: its bytes cross in the
    /// exchange buffer and its length is the parameter.
    Str,
    /// A scalar, a Wasm parameter of the import.
    Scalar(Val),
    /// A structured value, encoded in the exchange buffer after the ones
    /// before it (`boundary.rs`).
    Buf(LTy),
}

impl HostArg {
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            HostArg::Str => vec![Val::I32, Val::I32],
            HostArg::Scalar(v) => vec![*v],
            HostArg::Buf(b) => b.vals(),
        }
    }
}

/// A host leaf's result as its import answers it, always in the buffer
/// (§17.4): nothing; an enum of tags, read as its variant index and its
/// payload's; or any other value, through its decoder.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HostRes {
    Void,
    Tags,
    Buf(LTy),
}

impl HostRes {
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            HostRes::Void => vec![],
            HostRes::Tags => vec![Val::I32, Val::I32],
            HostRes::Buf(b) => b.vals(),
        }
    }
}

/// Where a leaf frame keeps its arguments: after the handle at
/// [`F_SAVED`], 8-aligned, laid out as one record (`Helper::HostCold`'s
/// `area`).
const LEAF_ARGS: u32 = (F_SAVED + 4).next_multiple_of(8);

/// The size of a `v` in memory.
fn size_of(v: Val) -> u32 {
    match v {
        Val::I32 | Val::F32 => 4,
        Val::I64 | Val::F64 => 8,
    }
}

fn store(v: Val, at: u32) -> Instruction {
    let at = u64::from(at);
    match v {
        Val::I32 => Instruction::I32Store(mem(HEAP, at, 2)),
        Val::I64 => Instruction::I64Store(mem(HEAP, at, 3)),
        Val::F32 => Instruction::F32Store(mem(HEAP, at, 2)),
        Val::F64 => Instruction::F64Store(mem(HEAP, at, 3)),
    }
}

fn load(v: Val, at: u32) -> Instruction {
    let at = u64::from(at);
    match v {
        Val::I32 => Instruction::I32Load(mem(HEAP, at, 2)),
        Val::I64 => Instruction::I64Load(mem(HEAP, at, 3)),
        Val::F32 => Instruction::F32Load(mem(HEAP, at, 2)),
        Val::F64 => Instruction::F64Load(mem(HEAP, at, 3)),
    }
}

/// Pushes the zero of `v`.
pub fn zero(f: &mut Func, v: Val) {
    f.op(match v {
        Val::I32 => Instruction::I32Const(0),
        Val::I64 => Instruction::I64Const(0),
        Val::F32 => Instruction::F32Const(0.0.into()),
        Val::F64 => Instruction::F64Const(0.0.into()),
    });
}

/// Sets bits `bits` in the flags of the task in local `t`.
fn flags_or(f: &mut Func, t: u32, bits: i32) {
    f.get(t);
    f.get(t);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(bits);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
}

/// Pushes whether the task in local `t` has any of `bits`.
fn flags_any(f: &mut Func, t: u32, bits: i32) {
    f.get(t);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(bits);
    f.op(Instruction::I32And);
}

/// A new counted object of `size` payload bytes, zeroed, with release
/// glue `drop` (none for `None`), in a fresh local of `f`. A frame that
/// takes part in cycle collection (`cyclic`, and glue) is tagged for the
/// collector ([`Helper::Opaque`]), which enters it only while its saved
/// slots are stable.
fn new_zeroed(f: &mut Func, size: u32, drop: Option<Sym>, cyclic: bool) -> u32 {
    let at = f.local(Val::I32);
    f.u32(size);
    let tagged = drop.is_some() && cyclic;
    crate::rt::push_desc(f, drop, cyclic);
    f.helper(Helper::New);
    f.set(at);
    if tagged {
        f.get(at);
        f.helper(Helper::Opaque);
    }
    f.get(at);
    f.i32(0);
    f.u32(size);
    f.op(Instruction::MemoryFill(HEAP));
    at
}

/// Cancel entry points are not dependencies until a reachable operation or
/// exported entry can use them. The zeroed frame needs no slot otherwise.
fn cancel_slot(f: &mut Func, m: &Module, frame: u32, cancel: Sym) {
    if m.cancellable {
        f.get(frame);
        f.table_slot(cancel);
        f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
    }
}

fn panic(f: &mut Func, msg: &str) {
    f.helper(Helper::Panic(msg.into()));
}

const REENTRANT: &str =
    "suspension-reentrant-poll: a suspension was polled or cancelled while active";
const INVALID: &str = "suspension-invalid-state: a completed suspension was polled";

/// The helpers of this module, which [`crate::rt::helper`] sends here.
pub fn helper(h: &Helper, m: &mut Module) -> Func {
    let mut f = match h {
        Helper::Frame(key) => frame(*key, m),
        Helper::PollCheck => poll_check(m),
        Helper::CancelCheck => cancel_check(m),
        Helper::CancelEnter => {
            let mut f = m.func("rt:cancel_enter", vec![], vec![Val::I32]);
            f.global_get(Global::AbSeen);
            f.i32(0);
            f.global_set(Global::AbSeen);
            f
        }
        Helper::CancelLeave => cancel_leave(m),
        Helper::WakeMark => wake_mark(m),
        Helper::WakeTake => wake_take(m),
        Helper::WakeSlot => wake_slot(m),
        Helper::WakeRegister => wake_register(m),
        Helper::WakeRemove => wake_remove(m),
        Helper::WakeSignal => wake_signal(m),
        Helper::WakerNew => waker_new(m),
        Helper::WakerParent => waker_parent(m),
        Helper::WakerSlot => waker_slot(m),
        Helper::WakerLive => waker_live(m),
        Helper::WakerDrop => waker_drop(m),
        // No node can be registered without a reachable WakerNew. Keep
        // host-only suspensions from linking the weak registry machinery.
        Helper::WakeLink if !m.wakers => m.func("rt:wake_link", vec![Val::I32; 3], vec![]),
        Helper::WakeNotify if !m.wakers => m.func("rt:wake_notify", vec![Val::I32], vec![]),
        Helper::WakeLink => wake_link(m),
        Helper::WakeNotify => wake_notify(m),
        Helper::HostCold {
            module,
            method,
            args,
            area,
            res,
        } => host_cold(m, module, method, args, area, res),
        Helper::HostPoll {
            module,
            method,
            args,
            area,
            res,
        } => host_poll(m, module, method, args, area, res),
        Helper::HostCancel => host_cancel(m),
        Helper::HostCall {
            module,
            method,
            args,
            result,
        } => crate::boundary::host_call(m, module, method, args, result),
        Helper::EntryCancel { abandon } => {
            let mut f = m.func("rt:entry_cancel", vec![], vec![]);
            let root = f.local(Val::I32);
            f.global_get(Global::Root);
            f.set(root);
            f.get(root);
            f.op(Instruction::If(BlockType::Empty));
            if *abandon {
                abandon_by(&mut f, 1);
            }
            f.get(root);
            f.get(root);
            f.op(Instruction::I32Load(mem(HEAP, F_CANCEL, 2)));
            f.call_indirect(vec![Val::I32], vec![]);
            if *abandon {
                abandon_by(&mut f, -1);
            }
            f.op(Instruction::End);
            f
        }
        Helper::ChildDrop => child_drop(m),
        Helper::CleanupCold => cleanup_cold(m),
        Helper::CleanupPoll => cleanup_poll(m),
        Helper::CleanupCancel => cleanup_cancel(m),
        Helper::PendingCold { results } => pending_cold(m, results),
        Helper::PendingPoll { results } => pending_poll(m, results),
        Helper::PendingCancel => {
            let mut f = m.func("rt:pending_cancel", vec![Val::I32], vec![]);
            f.get(0);
            f.helper(Helper::CancelCheck);
            f.op(Instruction::Drop);
            f
        }
        Helper::Race {
            part,
            results,
            walk,
        } => match part {
            0 => race_cold(m, results, walk.as_ref()),
            1 => race_poll(m, results),
            _ => race_cancel(m),
        },
        Helper::RaceDrop { results, walk } => race_drop(m, results, walk.as_ref()),
        Helper::OpenFrame {
            part: 0,
            reference,
            leaves,
            walk,
            counts,
        } => open_cold(m, leaves, walk.as_ref(), *counts, reference.as_ref()),
        Helper::OpenFrame {
            reference,
            leaves,
            walk,
            counts,
            ..
        } => open_poll(m, leaves, walk.as_ref(), *counts, reference.as_ref()),
        other => unreachable!("not a task helper: {other:?}"),
    };
    if matches!(
        h,
        Helper::HostPoll { .. }
            | Helper::HostCancel
            | Helper::CleanupPoll
            | Helper::CleanupCancel
            | Helper::PendingPoll { .. }
            | Helper::PendingCancel
            | Helper::Race { part: 1 | 2, .. }
            | Helper::OpenFrame { part: 1, .. }
    ) {
        guard_frame(&mut f);
    }
    #[cfg(feature = "checked-report")]
    match h {
        Helper::WakerNew | Helper::Race { part: 0, .. } => crate::rt::checked_kind(&mut f, m, 4),
        Helper::Frame(_)
        | Helper::HostCold { .. }
        | Helper::CleanupCold
        | Helper::PendingCold { .. }
        | Helper::OpenFrame { part: 0, .. } => crate::rt::checked_kind(&mut f, m, 3),
        _ => {}
    }
    f
}

/// Defers collector tracing while local 0's frame is executing. A body
/// may materialize a frame from 0, and a nested helper may already have
/// set RUNNING: restore only the previous running bit on every exit.
/// Results already on the Wasm stack are left in place by the stores.
pub(crate) fn guard_frame(f: &mut Func) {
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    f.origin_at = f.origin_root;
    let previous = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(RUNNING);
    f.op(Instruction::I32And);
    f.set(previous);
    flags_or(f, 0, RUNNING);
    f.op(Instruction::End);
    let restore = |f: &mut Func| {
        f.get(0);
        f.op(Instruction::If(BlockType::Empty));
        f.get(0);
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
        f.i32(!RUNNING);
        f.op(Instruction::I32And);
        f.get(previous);
        f.op(Instruction::I32Or);
        f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
        f.op(Instruction::End);
    };
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        if matches!(ins, Ins::Op(Instruction::Return)) {
            restore(f);
        }
        f.push(ins);
    }
    f.origin_at = f.origin_root;
    restore(f);
    f.origin_at = outer;
}

/// Adds `by` to the `abandon()` depth.
pub fn abandon_by(f: &mut Func, by: i32) {
    f.global_get(Global::Aband);
    f.i32(by);
    f.op(Instruction::I32Add);
    f.global_set(Global::Aband);
}

/// [`Helper::Frame`]: a new frame of suspending instance `key`, zeroed,
/// with its poll and cancel.
fn frame(key: Hash128, m: &Module) -> Func {
    let size = m.frames.get(&key).copied().unwrap_or(F_SAVED);
    let mut f = m.func("rt:frame", vec![], vec![Val::I32]);
    // What a suspending body's frame saves is not known here, so it takes
    // part in cycle collection (§23.28).
    let at = new_zeroed(&mut f, size, Some(Sym::Part(key, Part::Drop)), true);
    f.get(at);
    f.table_slot(Sym::Part(key, Part::Poll));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, at, Sym::Part(key, Part::Cancel));
    f.get(at);
    f
}

/// [`Helper::PollCheck`] (`crates/hd_wasm_linear/DESIGN.md` §14.3, §14.6):
/// whether the poll answers Pending at once. An active frame is a
/// re-entrant poll and a completed one an invalid state, both panics; a
/// cancelled one answers Pending, and a poll of one that still unwinds
/// continues its cleanup.
fn poll_check(m: &Module) -> Func {
    let mut f = m.func("rt:poll_check", vec![Val::I32], vec![Val::I32]);
    flags_any(&mut f, 0, ACTIVE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, REENTRANT);
    f.op(Instruction::End);
    flags_any(&mut f, 0, DONE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, INVALID);
    f.op(Instruction::End);
    if !m.cancellable {
        f.i32(0);
        return f;
    }
    flags_any(&mut f, 0, CANCELLED);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    flags_any(&mut f, 0, UNWINDING);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CANCEL, 2)));
    f.call_indirect(vec![Val::I32], vec![]);
    f.op(Instruction::End);
    f.i32(1);
    f
}

/// [`Helper::CancelCheck`] (`crates/hd_wasm_linear/DESIGN.md` §14.6): whether
/// the cancel goes on. An active frame panics; a done one, or a cancelled
/// one that no longer unwinds, is left as it is; any other is marked
/// cancelled, and abandoned under `abandon()`.
fn cancel_check(m: &Module) -> Func {
    let mut f = m.func("rt:cancel_check", vec![Val::I32], vec![Val::I32]);
    let flags = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.set(flags);
    f.get(flags);
    f.i32(ACTIVE);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, REENTRANT);
    f.op(Instruction::End);
    f.get(flags);
    f.i32(UNWINDING);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Eqz);
    f.get(flags);
    f.i32(DONE | CANCELLED);
    f.op(Instruction::I32And);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.get(flags);
    f.i32(CANCELLED);
    f.op(Instruction::I32Or);
    f.global_get(Global::Aband);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32Eqz);
    f.i32(ABANDONED);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
    f.i32(1);
    f
}

/// [`Helper::CancelLeave`]: the `ABANDONED` bits that the frame's cancels
/// saw mark it (`req.abandon.carry`); then `abseen` holds the bits seen
/// before with the frame's own.
fn cancel_leave(m: &Module) -> Func {
    let mut f = m.func("rt:cancel_leave", vec![Val::I32, Val::I32], vec![]);
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.global_get(Global::AbSeen);
    f.i32(ABANDONED);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
    f.get(1);
    flags_any(&mut f, 0, ABANDONED);
    f.op(Instruction::I32Or);
    f.global_set(Global::AbSeen);
    f
}

/// The wake table's slot of handle `h` (a local): its low 20 bits.
fn slot(f: &mut Func, h: u32) {
    f.get(h);
    f.i32(0xF_FFFF);
    f.op(Instruction::I32And);
}

/// A wake registration holds the completed handle, registered handle,
/// counted waiter list and one reserved word. Generations are compared in full.
const WAKE_STRIDE: i32 = 16;
/// A waker node owns its waiter-edge storage, with weak parent targets.
pub const W_READY: u64 = 8;

/// Registry rows are weak: address, generation, next free slot. Slot zero
/// is null; a node's spare word records its one-based registry slot.
const NODE_STRIDE: i32 = 12;
const W_SLOT: u64 = 4;

fn node_address(f: &mut Func, id: u32) {
    f.global_get(Global::NodeTab);
    f.get(id);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.i32(NODE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
}

/// Grow only for a fresh slot; existing weak handles use `waker_live` and
/// never allocate or read freed node memory.
fn waker_slot(m: &Module) -> Func {
    let mut f = m.func("rt:waker_slot", vec![Val::I32], vec![Val::I32]);
    let (cap, fresh) = (f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.global_get(Global::NodeCap);
    f.op(Instruction::I32GtU);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.i32(2);
    f.op(Instruction::I32Mul);
    f.i32(16);
    f.op(Instruction::I32Add);
    f.set(cap);
    f.get(cap);
    f.i32(NODE_STRIDE);
    f.op(Instruction::I32Mul);
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.alloc)),
        None => f.helper(Helper::Alloc),
    }
    f.set(fresh);
    f.get(fresh);
    f.i32(0);
    f.get(cap);
    f.i32(NODE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryFill(HEAP));
    f.global_get(Global::NodeTab);
    f.op(Instruction::If(BlockType::Empty));
    f.get(fresh);
    f.global_get(Global::NodeTab);
    f.global_get(Global::NodeCap);
    f.i32(NODE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    if let Some(r) = m.runtime {
        f.global_get(Global::NodeTab);
        f.call(Sym::Inst(r.acyclic.free));
    }
    f.op(Instruction::End);
    f.get(fresh);
    f.global_set(Global::NodeTab);
    f.get(cap);
    f.global_set(Global::NodeCap);
    f.op(Instruction::End);
    node_address(&mut f, 0);
    f
}

/// Resolve a weak handle before touching the target, including in checked
/// quarantine. The registry itself outlives all its registered objects.
fn waker_live(m: &Module) -> Func {
    let mut f = m.func("rt:waker_live", vec![Val::I32; 2], vec![Val::I32]);
    let at = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Eqz);
    f.get(0);
    f.global_get(Global::NodeLen);
    f.op(Instruction::I32GtU);
    f.op(Instruction::I32Or);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    node_address(&mut f, 0);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::Else);
    f.i32(0);
    f.op(Instruction::End);
    f
}

/// Invalidate before dropping edge storage. A generation never wraps:
/// the exhausted slot is retired, so no stale handle can become live.
fn waker_drop(m: &Module) -> Func {
    let mut f = m.func("rt:waker_drop", vec![Val::I32], vec![]);
    let (id, at, generation) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, W_SLOT, 2)));
    f.set(id);
    node_address(&mut f, id);
    f.set(at);
    f.get(at);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(generation);
    f.get(generation);
    f.i32(-1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.get(at);
    f.get(generation);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.get(at);
    f.global_get(Global::NodeFree);
    f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
    f.get(id);
    f.global_set(Global::NodeFree);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.helper(Helper::ReleaseRef);
    f
}

/// [`Helper::WakeSlot`]: grows the registration table before either a
/// registration or a wake. Allocating once per capacity keeps lookup O(1).
fn wake_slot(m: &Module) -> Func {
    let mut f = m.func("rt:wake_slot", vec![Val::I32], vec![Val::I32]);
    let (cap, fresh) = (f.local(Val::I32), f.local(Val::I32));
    slot(&mut f, 0);
    f.global_get(Global::WakeCap);
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Empty));
    slot(&mut f, 0);
    f.i32(2);
    f.op(Instruction::I32Mul);
    f.i32(16);
    f.op(Instruction::I32Add);
    f.set(cap);
    f.get(cap);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.alloc)),
        None => f.helper(Helper::Alloc),
    }
    f.set(fresh);
    f.get(fresh);
    f.i32(0);
    f.get(cap);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryFill(HEAP));
    f.global_get(Global::WakeTab);
    f.op(Instruction::If(BlockType::Empty));
    f.get(fresh);
    f.global_get(Global::WakeTab);
    f.global_get(Global::WakeCap);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    if let Some(r) = m.runtime {
        // A raw block: the free that skips the collector's candidate test.
        f.global_get(Global::WakeTab);
        f.call(Sym::Inst(r.acyclic.free));
    }
    f.op(Instruction::End);
    f.get(fresh);
    f.global_set(Global::WakeTab);
    f.get(cap);
    f.global_set(Global::WakeCap);
    f.op(Instruction::End);
    wake_address(&mut f, 0);
    f
}

fn wake_address(f: &mut Func, h: u32) {
    f.global_get(Global::WakeTab);
    slot(f, h);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
}

/// [`Helper::WakeSignal`]: a wake follows only its enclosing waiters,
/// marking their child bits, never polling a sibling.
fn wake_signal(m: &Module) -> Func {
    let mut f = m.func("rt:wake_signal", vec![Val::I32, Val::I32], vec![]);
    f.get(0);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, W_READY, 2)));
    f.get(1);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, W_READY, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.helper(Helper::WakeNotify);
    f
}

/// Shared cancellation cleanups can have several awaiters. Each edge
/// stores a weak (slot, generation) target, next owned edge, and bits.
fn wake_link(m: &Module) -> Func {
    #[cfg(not(feature = "checked-report"))]
    let _ = m;
    let mut f = m.func("rt:wake_link", vec![Val::I32; 3], vec![]);
    let (edge, id, generation, at, stale) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(1);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(1);
    f.op(Instruction::I32Load(mem(HEAP, W_SLOT, 2)));
    f.set(id);
    node_address(&mut f, id);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(generation);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(edge);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(edge);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(id);
    f.op(Instruction::I32Eq);
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(generation);
    f.op(Instruction::I32Eq);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.get(edge);
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
    f.get(2);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    // Remember stale storage, but find an existing live target first.
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.helper(Helper::WakerLive);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.get(stale);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.get(edge);
    f.set(stale);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.set(edge);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(stale);
    f.op(Instruction::If(BlockType::Empty));
    store_waiter(&mut f, stale, id, generation);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.i32(16);
    f.table_slot(Sym::Helper(Helper::DropFields {
        leaves: vec![Leaf {
            offset: 8,
            val: Val::I32,
            bytes: 4,
            signed: false,
        }],
        walk: Walk::Ref(0),
    }));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.helper(Helper::New);
    f.set(edge);
    #[cfg(feature = "checked-report")]
    crate::rt::checked_kind(&mut f, m, 4);
    store_waiter(&mut f, edge, id, generation);
    // The previous head's count moves into this edge.
    f.get(edge);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
    f.get(0);
    f.get(edge);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f
}

fn store_waiter(f: &mut Func, edge: u32, id: u32, generation: u32) {
    for (off, value) in [(0, id), (4, generation), (12, 2)] {
        f.get(edge);
        f.get(value);
        f.op(Instruction::I32Store(mem(HEAP, off, 2)));
    }
}

fn wake_notify(m: &Module) -> Func {
    let mut f = m.func("rt:wake_notify", vec![Val::I32], vec![]);
    let node = f.local(Val::I32);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.helper(Helper::WakerLive);
    f.set(node);
    f.get(node);
    f.op(Instruction::If(BlockType::Empty));
    f.get(node);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
    f.helper(Helper::WakeSignal);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.set(0);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

fn waker_parent(m: &Module) -> Func {
    let mut f = m.func("rt:waker_parent", vec![Val::I32], vec![]);
    f.get(0);
    f.global_get(Global::Waker);
    f.global_get(Global::WakeBit);
    f.helper(Helper::WakeLink);
    f
}

fn waker_new(m: &Module) -> Func {
    let mut f = m.func("rt:waker_new", vec![Val::I32], vec![Val::I32]);
    let node = new_zeroed(&mut f, 12, Some(Sym::Helper(Helper::WakerDrop)), false);
    let (id, at) = (f.local(Val::I32), f.local(Val::I32));
    f.global_get(Global::NodeFree);
    f.set(id);
    f.get(id);
    f.op(Instruction::If(BlockType::Empty));
    f.get(id);
    f.helper(Helper::WakerSlot);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.global_set(Global::NodeFree);
    f.op(Instruction::Else);
    f.global_get(Global::NodeLen);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(id);
    f.get(id);
    f.global_set(Global::NodeLen);
    f.get(id);
    f.helper(Helper::WakerSlot);
    f.set(at);
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.op(Instruction::End);
    f.get(at);
    f.get(node);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(node);
    f.get(id);
    f.op(Instruction::I32Store(mem(HEAP, W_SLOT, 2)));
    f.get(node);
    f.helper(Helper::WakerParent);
    f.get(node);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, W_READY, 2)));
    f.get(node);
    f
}

fn wake_register(m: &Module) -> Func {
    let mut f = m.func("rt:wake_register", vec![Val::I32], vec![]);
    let at = f.local(Val::I32);
    f.get(0);
    f.helper(Helper::WakeSlot);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::ReleaseRef);
    f.get(at);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
    f.get(at);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.op(Instruction::End);
    f.get(at);
    f.i32(8);
    f.op(Instruction::I32Add);
    f.global_get(Global::Waker);
    f.global_get(Global::WakeBit);
    f.helper(Helper::WakeLink);
    f
}

/// Remove only the registered generation. A late wake cannot touch a
/// released node or an unrelated new occupant of the same host slot.
fn wake_remove(m: &Module) -> Func {
    let mut f = m.func("rt:wake_remove", vec![Val::I32], vec![]);
    let at = f.local(Val::I32);
    slot(&mut f, 0);
    f.global_get(Global::WakeCap);
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    wake_address(&mut f, 0);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(0);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::ReleaseRef);
    for off in [4, 8, 12] {
        f.get(at);
        f.i32(0);
        f.op(Instruction::I32Store(mem(HEAP, off, 2)));
    }
    f.op(Instruction::End);
    f
}

/// [`Helper::WakeMark`] (`hd.wake(n)`): record completions, and mark the
/// child named by each live registration plus its enclosing parents.
fn wake_mark(m: &Module) -> Func {
    let mut f = m.func("rt:wake_mark", vec![Val::I32], vec![]);
    let (i, h, at) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    f.get(0);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(i);
    f.i32(4);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Load(mem(BUF, 0, 2)));
    f.set(h);
    f.get(h);
    f.helper(Helper::WakeSlot);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(h);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    // An old generation must not overwrite a newer completion's mark.
    f.get(at);
    f.get(h);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::WakeNotify);
    f.op(Instruction::End);
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(i);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

/// [`Helper::WakeTake`]: consume the matching completion and release its
/// registration's ownership. Stale generations leave the current one alone.
fn wake_take(m: &Module) -> Func {
    let mut f = m.func("rt:wake_take", vec![Val::I32], vec![Val::I32]);
    let at = f.local(Val::I32);
    slot(&mut f, 0);
    f.global_get(Global::WakeCap);
    f.op(Instruction::I32GeU);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    wake_address(&mut f, 0);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(at);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(0);
    f.helper(Helper::WakeRemove);
    f.i32(1);
    f
}

/// The release glue of a leaf frame whose arguments are laid out as
/// `area`: one count less on each counted reference among them.
fn leaf_drop(area: &Elem) -> Option<Sym> {
    let walk = area.walk.as_ref()?;
    // Only the leaves the walk reads, so the glue loads nothing else.
    let mut used = Vec::new();
    walk_indices(walk, &mut used);
    used.sort_unstable();
    used.dedup();
    let leaves = used
        .iter()
        .map(|k| {
            let l = area.leaves[*k as usize];
            Leaf {
                offset: l.offset + LEAF_ARGS,
                ..l
            }
        })
        .collect();
    let walk = walk.remap(&|k| {
        u32::try_from(used.iter().position(|u| *u == k).expect("used")).expect("leaves")
    });
    Some(Sym::Helper(Helper::DropFields { leaves, walk }))
}

/// Every value index a walk reads: its references and its tags.
fn walk_indices(w: &Walk, out: &mut Vec<u32>) {
    match w {
        Walk::Ref(k) => out.push(*k),
        Walk::Seq(ws) => ws.iter().for_each(|w| walk_indices(w, out)),
        Walk::Switch(k, arms) => {
            out.push(*k);
            arms.iter().flatten().for_each(|w| walk_indices(w, out));
        }
    }
}

/// [`Helper::HostCold`]: `(data, arguments) -> frame`, a host method
/// that may wait, called cold: a leaf frame holding the arguments, laid
/// out as `area`, each with a count of its own (a collection as its
/// copy), and no handle yet (-1).
fn host_cold(
    m: &Module,
    module: &str,
    method: &str,
    args: &[HostArg],
    area: &Elem,
    res: &HostRes,
) -> Func {
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut f = m.func(format!("rt:{module}/{method}"), params, vec![Val::I32]);
    // A leaf frame takes part as its arguments' types say.
    let fr = new_zeroed(&mut f, LEAF_ARGS + area.size, leaf_drop(area), area.cyclic);
    let poll = Sym::Helper(Helper::HostPoll {
        module: module.to_owned(),
        method: method.to_owned(),
        args: args.to_vec(),
        area: area.clone(),
        res: res.clone(),
    });
    f.get(fr);
    f.table_slot(poll);
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(Helper::HostCancel));
    f.get(fr);
    f.i32(-1);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(F_SAVED), 2)));
    for (k, l) in area.leaves.iter().enumerate() {
        f.get(fr);
        f.get(u32::try_from(k + 1).expect("params"));
        f.op(l.store(LEAF_ARGS));
    }
    // The frame's count of each counted argument; a collection becomes
    // its copy.
    if !area.colls.is_empty() {
        f.get(fr);
        f.u32(LEAF_ARGS);
        f.op(Instruction::I32Add);
        f.i32(1);
        f.helper(Helper::CopyElems { elem: area.clone() });
    } else if let Some(w) = &area.walk {
        crate::rt::walk_code(&mut f, w, &|f, k| f.get(k + 1), &|f| {
            f.call(Sym::Helper(Helper::RetainAny));
        });
    }
    f.get(fr);
    f
}

/// [`Helper::HostPoll`]: `(frame) -> (ready, results)`. The first poll
/// calls `.start` with the arguments; a call that finished at once is
/// Ready, else its handle waits in the frame. A later poll is Pending
/// until the wake table holds the handle, then calls `.finish`.
fn host_poll(
    m: &Module,
    module: &str,
    method: &str,
    args: &[HostArg],
    area: &Elem,
    res: &HostRes,
) -> Func {
    let rvals = res.vals();
    let mut results = vec![Val::I32];
    results.extend(rvals.iter().copied());
    let mut f = m.func(
        format!("rt:{module}/{method}.poll"),
        vec![Val::I32],
        results,
    );
    let (st, n, h) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let pending = |f: &mut Func| {
        f.i32(0);
        for v in &rvals {
            zero(f, *v);
        }
        f.op(Instruction::Return);
    };
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    pending(&mut f);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.set(h);
    f.get(h);
    f.i32(0);
    f.op(Instruction::I32LtS);
    f.op(Instruction::If(BlockType::Empty));
    let leaves = area.leaves.clone();
    let imp = crate::boundary::push_args(&mut f, args, &|f, k| {
        f.get(0);
        f.op(leaves[k as usize].load(LEAF_ARGS));
    });
    f.call(Sym::Import(Import::new(
        module,
        &format!("{method}.start"),
        &imp,
        &[Val::I32, Val::I32],
    )));
    f.set(n);
    f.set(st);
    f.get(st);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(F_SAVED), 2)));
    f.get(n);
    f.helper(Helper::WakeRegister);
    pending(&mut f);
    f.op(Instruction::End);
    f.op(Instruction::Else);
    f.get(h);
    f.helper(Helper::WakeRegister);
    f.get(h);
    f.helper(Helper::WakeTake);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    pending(&mut f);
    f.op(Instruction::End);
    f.get(h);
    f.call(Sym::Import(Import::new(
        module,
        &format!("{method}.finish"),
        &[Val::I32],
        &[Val::I32],
    )));
    f.set(n);
    f.op(Instruction::End);
    flags_or(&mut f, 0, DONE);
    f.i32(1);
    match res {
        HostRes::Void => {}
        HostRes::Tags => {
            // The variant index, then the payload's variant index when
            // the result has one.
            f.i32(0);
            f.op(Instruction::I32Load8U(mem(BUF, 0, 0)));
            f.get(n);
            f.i32(1);
            f.op(Instruction::I32GtS);
            f.op(Instruction::If(BlockType::Result(
                wasm_encoder::ValType::I32,
            )));
            f.i32(0);
            f.op(Instruction::I32Load8U(mem(BUF, 1, 0)));
            f.op(Instruction::Else);
            f.i32(0);
            f.op(Instruction::End);
        }
        HostRes::Buf(b) => {
            f.i32(0);
            f.helper(Helper::Dec(b.clone()));
            f.op(Instruction::Drop);
        }
    }
    f
}

/// [`Helper::HostCancel`]: a leaf's cancel. A done or cancelled call is
/// left; any other is marked cancelled, and a call waiting on the host is
/// aborted. No completion will wake its awaiter, so the entry polls its
/// root again.
fn host_cancel(m: &Module) -> Func {
    let mut f = m.func("rt:host_cancel", vec![Val::I32], vec![]);
    flags_any(&mut f, 0, DONE | CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    flags_or(&mut f, 0, CANCELLED);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.i32(0);
    f.op(Instruction::I32GeS);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.call(Sym::Import(Import::new("hd:rt", "abort", &[Val::I32], &[])));
    let at = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.helper(Helper::WakeSlot);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::WakeNotify);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.helper(Helper::WakeRemove);
    f.i32(1);
    f.global_set(Global::Rewake);
    f.op(Instruction::End);
    f
}

/// `hd.poll(ctx)` of a suspending root (`crates/hd_wasm_linear/DESIGN.md` §14.4):
/// the first poll builds the root's frame with the host providers (no
/// data beside each vtable of host stubs) and claims it for its driver;
/// each poll then polls it, again while an abandon or an aborted host
/// call asks for it. -1 is Pending; a Ready root's result goes to
/// `report`, whose status is the answer; a root that ended cancelled
/// answers -2, or -3 when its cleanup was cut short.
#[must_use]
pub fn root_poll(
    m: &Module,
    main: Hash128,
    providers: &[(usize, Option<Hash128>)],
    rvals: &[Val],
    report: Option<Hash128>,
) -> Func {
    let mut f = m.func("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
    let (root, ready) = (f.local(Val::I32), f.local(Val::I32));
    let got: Vec<u32> = rvals.iter().map(|v| f.local(*v)).collect();
    let release = |f: &mut Func| {
        f.global_get(Global::Root);
        f.helper(Helper::ReleaseRef);
        f.i32(0);
        f.global_set(Global::Root);
        f.i32(0);
        f.global_set(Global::Owner);
    };
    f.global_get(Global::Root);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    crate::rt::host_providers(&mut f, providers);
    f.call(Sym::Inst(main));
    f.global_set(Global::Root);
    f.get(0);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.global_set(Global::Owner);
    f.op(Instruction::End);
    f.global_get(Global::Root);
    f.set(root);
    flags_any(&mut f, root, ACTIVE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, REENTRANT);
    f.op(Instruction::End);
    flags_any(&mut f, root, DONE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, INVALID);
    f.op(Instruction::End);
    f.global_get(Global::Owner);
    f.get(0);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    panic(
        &mut f,
        "suspension-competing-driver: a suspension was polled by a driver that does not own it",
    );
    f.op(Instruction::End);
    // A cancelled root is no longer polled: its cancel continues its
    // cleanup.
    f.op(Instruction::Loop(BlockType::Empty));
    f.i32(0);
    f.global_set(Global::Rewake);
    flags_any(&mut f, root, CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    f.get(root);
    f.get(root);
    f.op(Instruction::I32Load(mem(HEAP, F_CANCEL, 2)));
    f.call_indirect(vec![Val::I32], vec![]);
    f.i32(0);
    f.set(ready);
    f.op(Instruction::Else);
    f.get(root);
    f.get(root);
    f.op(Instruction::I32Load(mem(HEAP, F_POLL, 2)));
    let mut pres = vec![Val::I32];
    pres.extend(rvals.iter().copied());
    f.call_indirect(vec![Val::I32], pres);
    for l in got.iter().rev() {
        f.set(*l);
    }
    f.set(ready);
    f.op(Instruction::End);
    f.get(ready);
    f.op(Instruction::I32Eqz);
    f.global_get(Global::Rewake);
    f.op(Instruction::I32And);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    flags_any(&mut f, root, CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    flags_any(&mut f, root, UNWINDING);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(CANCELLED_END);
    flags_any(&mut f, root, ABANDONED);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32Sub);
    release(&mut f);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(ready);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(-1);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    release(&mut f);
    match report {
        Some(k) => {
            for l in &got {
                f.get(*l);
            }
            f.call(Sym::Inst(k));
        }
        None => f.i32(0),
    }
    f
}

/// The glue of a frame whose child is its only counted field: one count
/// less on the child.
fn child_drop(m: &Module) -> Func {
    let mut f = m.func("rt:drop_child", vec![Val::I32], vec![]);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.helper(Helper::ReleaseRef);
    f.op(Instruction::End);
    f
}

/// Calls the cancel of the task in local `c`.
fn call_cancel(f: &mut Func, c: u32) {
    f.get(c);
    f.get(c);
    f.op(Instruction::I32Load(mem(HEAP, F_CANCEL, 2)));
    f.call_indirect(vec![Val::I32], vec![]);
}

/// [`Helper::CleanupCold`]: `(task) -> frame`, `cancel!`'s cleanup frame
/// over `task`, which it holds as its child with a count of its own.
fn cleanup_cold(m: &Module) -> Func {
    let mut f = m.func("rt:cleanup_cold", vec![Val::I32], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, Some(Sym::Helper(Helper::ChildDrop)), true);
    f.get(fr);
    f.table_slot(Sym::Helper(Helper::CleanupPoll));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(Helper::CleanupCancel));
    f.get(fr);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, F_CHILD, 2)));
    f.get(0);
    f.helper(Helper::RetainAny);
    f.get(fr);
    f
}

/// Starts or continues the cleanup of the task a cleanup frame (local 0)
/// holds, and sets local `wait` while the task still unwinds.
fn continue_cleanup(f: &mut Func, wait: u32) {
    let c = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.set(c);
    call_cancel(f, c);
    flags_any(f, c, UNWINDING);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.set(wait);
}

/// [`Helper::CleanupPoll`]: `(frame) -> (ready, abandoned)`. The task's
/// own cancel starts its cleanup, or continues it while it unwinds, and
/// does nothing once it ended: Pending while the task unwinds, then Ready
/// with whether the cleanup was cut short.
fn cleanup_poll(m: &Module) -> Func {
    let mut f = m.func("rt:cleanup_poll", vec![Val::I32], vec![Val::I32, Val::I32]);
    let wait = f.local(Val::I32);
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    continue_cleanup(&mut f, wait);
    f.get(wait);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    flags_or(&mut f, 0, DONE);
    f.i32(1);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(ABANDONED);
    f.op(Instruction::I32And);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f
}

/// Leaves a runtime frame's cancel unless it unwinds: a done or cancelled
/// frame that no longer unwinds is left as it is.
fn cancel_guard(f: &mut Func) {
    flags_any(f, 0, UNWINDING);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    flags_any(f, 0, DONE | CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// Marks the frame in local 0 cancelled, and unwinding while local `wait`
/// is set.
fn mark_cancelled(f: &mut Func, wait: u32) {
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(!UNWINDING);
    f.op(Instruction::I32And);
    f.i32(CANCELLED);
    f.get(wait);
    f.i32(UNWINDING);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
}

/// [`Helper::CleanupCancel`]: continues the cleanup too, and leaves the
/// frame unwinding while the task unwinds (`req.cancel.nested`).
fn cleanup_cancel(m: &Module) -> Func {
    let mut f = m.func("rt:cleanup_cancel", vec![Val::I32], vec![]);
    let wait = f.local(Val::I32);
    cancel_guard(&mut f);
    continue_cleanup(&mut f, wait);
    mark_cancelled(&mut f, wait);
    f
}

/// [`Helper::PendingCold`]: `() -> frame`, a frame that never completes,
/// for std's `timeout!`.
fn pending_cold(m: &Module, results: &[Val]) -> Func {
    let mut f = m.func("rt:pending_cold", vec![], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, None, false);
    f.get(fr);
    f.table_slot(Sym::Helper(Helper::PendingPoll {
        results: results.to_vec(),
    }));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(Helper::PendingCancel));
    f.get(fr);
    f
}

/// [`Helper::PendingPoll`]: Pending whether or not it was cancelled; it
/// registers no wake, since nothing will wake it.
fn pending_poll(m: &Module, results: &[Val]) -> Func {
    let mut res = vec![Val::I32];
    res.extend(results.iter().copied());
    let mut f = m.func("rt:pending_poll", vec![Val::I32], res.clone());
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::Drop);
    for v in &res {
        zero(&mut f, *v);
    }
    f
}

/// The list a race frame holds: its header's address at its first field.
const RACE_LIST: u32 = F_SAVED;
/// Where a race frame keeps its winner's value while losers settle.
const RACE_WIN: u32 = F_SAVED + 8;

/// Each value of a race's result with its offset in the frame, and the
/// frame's payload size.
fn race_area(results: &[Val]) -> (Vec<(u32, Val)>, u32) {
    let mut at = RACE_WIN;
    let mut out = Vec::new();
    for v in results {
        let size = size_of(*v);
        at = at.div_ceil(size) * size;
        out.push((at, *v));
        at += size;
    }
    (out, at.div_ceil(8) * 8)
}

/// [`Helper::RaceCold`]: `(list) -> frame`, `race!`'s frame over its
/// list of tasks, which it holds with a count of its own.
fn race_cold(m: &Module, results: &[Val], walk: Option<&Walk>) -> Func {
    let (_, size) = race_area(results);
    let mut f = m.func("rt:race_cold", vec![Val::I32], vec![Val::I32]);
    let drop = Sym::Helper(Helper::RaceDrop {
        results: results.to_vec(),
        walk: walk.cloned(),
    });
    let fr = new_zeroed(&mut f, size, Some(drop), true);
    let h = |k| Helper::Race {
        part: k,
        results: results.to_vec(),
        walk: walk.cloned(),
    };
    f.get(fr);
    f.table_slot(Sym::Helper(h(1)));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(h(2)));
    f.get(fr);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(RACE_LIST), 2)));
    f.get(0);
    f.helper(Helper::RetainAny);
    f.get(fr);
    f
}

/// Pushes the address of task `i` (a local) of the race frame in local 0,
/// and leaves the list's length in local `n`.
fn race_task(f: &mut Func, i: u32) {
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(RACE_LIST), 2)));
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(i);
    f.i32(4);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load(mem(
        HEAP,
        u64::from(crate::rt::LIST_ELEMS),
        2,
    )));
}

fn race_len(f: &mut Func) {
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(RACE_LIST), 2)));
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Load(mem(HEAP, crate::rt::LIST_LEN, 2)));
}

/// Cancels every task of the race frame in local 0 in list order, but the
/// one at local `skip`, and sets local `wait` when one is left unwinding.
fn cancel_tasks(f: &mut Func, skip: Option<u32>, wait: u32) {
    let (i, c) = (f.local(Val::I32), f.local(Val::I32));
    f.i32(0);
    f.set(wait);
    f.i32(0);
    f.set(i);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    race_len(f);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    if let Some(s) = skip {
        f.get(i);
        f.get(s);
        f.op(Instruction::I32Ne);
        f.op(Instruction::If(BlockType::Empty));
    }
    race_task(f, i);
    f.set(c);
    call_cancel(f, c);
    flags_any(f, c, UNWINDING);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(1);
    f.set(wait);
    f.op(Instruction::End);
    if skip.is_some() {
        f.op(Instruction::End);
    }
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(i);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// [`Helper::Race`] part 1, the poll (`crates/hd_wasm_linear/DESIGN.md` §14.5):
/// polls the tasks in list order; the first Ready wins, and every other
/// task is cancelled in list order before it returns, the winner's value
/// waiting in the frame while a loser unwinds. A task that another task
/// cancelled cancels the race. An empty list panics.
fn race_poll(m: &Module, results: &[Val]) -> Func {
    let (area, _) = race_area(results);
    let mut res = vec![Val::I32];
    res.extend(results.iter().copied());
    let mut f = m.func("rt:race_poll", vec![Val::I32], res);
    let (i, c, ready, wait) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let tmp: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    let pending = |f: &mut Func| {
        f.i32(0);
        for v in results {
            zero(f, *v);
        }
        f.op(Instruction::Return);
    };
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    pending(&mut f);
    f.op(Instruction::End);
    let settle = |f: &mut Func| {
        f.get(wait);
        f.op(Instruction::If(BlockType::Empty));
        pending(f);
        f.op(Instruction::End);
        flags_or(f, 0, DONE);
        f.i32(1);
        for (at, v) in &area {
            f.get(0);
            f.op(load(*v, *at));
        }
        f.op(Instruction::Return);
    };
    // Settling: the winner waits while a loser unwinds.
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
    f.op(Instruction::If(BlockType::Empty));
    cancel_tasks(&mut f, None, wait);
    settle(&mut f);
    f.op(Instruction::End);
    race_len(&mut f);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, "explicit-panic: `race!` of an empty list");
    f.op(Instruction::End);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(i);
    race_len(&mut f);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    race_task(&mut f, i);
    f.set(c);
    f.get(c);
    f.get(c);
    f.op(Instruction::I32Load(mem(HEAP, F_POLL, 2)));
    let mut pres = vec![Val::I32];
    pres.extend(results.iter().copied());
    f.call_indirect(vec![Val::I32], pres);
    for t in tmp.iter().rev() {
        f.set(*t);
    }
    f.set(ready);
    f.get(ready);
    f.op(Instruction::If(BlockType::Empty));
    for ((at, v), t) in area.iter().zip(&tmp) {
        f.get(0);
        f.get(*t);
        f.op(store(*v, *at));
    }
    f.get(0);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, F_STATE, 2)));
    cancel_tasks(&mut f, Some(i), wait);
    settle(&mut f);
    f.op(Instruction::End);
    flags_any(&mut f, c, CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    cancel_tasks(&mut f, None, wait);
    mark_cancelled(&mut f, wait);
    pending(&mut f);
    f.op(Instruction::End);
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(i);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    pending(&mut f);
    f
}

/// [`Helper::Race`] part 2, the cancel: every task, in list order; a
/// loser left unwinding keeps the race unwinding.
fn race_cancel(m: &Module) -> Func {
    let mut f = m.func("rt:race_cancel", vec![Val::I32], vec![]);
    let wait = f.local(Val::I32);
    cancel_guard(&mut f);
    cancel_tasks(&mut f, None, wait);
    mark_cancelled(&mut f, wait);
    f
}

/// [`Helper::RaceDrop`]: one count less on the list, and on a winner's
/// value that no poll handed out.
fn race_drop(m: &Module, results: &[Val], walk: Option<&Walk>) -> Func {
    let (area, _) = race_area(results);
    let mut f = m.func("rt:race_drop", vec![Val::I32], vec![]);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(RACE_LIST), 2)));
    f.helper(Helper::ReleaseRef);
    if let Some(w) = walk {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
        flags_any(&mut f, 0, DONE);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::I32And);
        f.op(Instruction::If(BlockType::Empty));
        for (at, v) in &area {
            f.get(0);
            f.op(load(*v, *at));
        }
        f.helper(Helper::Release {
            vals: results.to_vec(),
            walk: w.clone(),
        });
        f.op(Instruction::End);
    }
    f
}

/// [`Helper::OpenFrame`] part 0: `(child) -> frame`, the frame of a kept
/// suspension of a generic method called through a trait value over the
/// erased frame `child`, whose count it takes. It is `cancel!`'s cleanup
/// frame but for its poll: its cancel cancels the child and unwinds while
/// the child does, and its glue releases the child.
fn open_cold(
    m: &Module,
    leaves: &[Leaf],
    walk: Option<&Walk>,
    counts: bool,
    reference: Option<&crate::rt::RefLayout>,
) -> Func {
    let mut f = m.func("rt:open_cold", vec![Val::I32], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, Some(Sym::Helper(Helper::ChildDrop)), true);
    f.get(fr);
    f.table_slot(Sym::Helper(Helper::OpenFrame {
        part: 1,
        reference: reference.cloned(),
        leaves: leaves.to_vec(),
        walk: walk.cloned(),
        counts,
    }));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(Helper::CleanupCancel));
    f.get(fr);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, F_CHILD, 2)));
    f.get(fr);
    f
}

/// [`Helper::OpenFrame`] part 1, the poll: polls the child, and once it
/// is Ready answers the value its open result holds, with counts of its
/// own, and releases the open result.
fn open_poll(
    m: &Module,
    leaves: &[Leaf],
    walk: Option<&Walk>,
    counts: bool,
    reference: Option<&crate::rt::RefLayout>,
) -> Func {
    let vals: Vec<Val> = leaves.iter().map(|l| l.val).collect();
    let mut res = vec![Val::I32];
    res.extend(vals.iter().copied());
    let mut f = m.func("rt:open_poll", vec![Val::I32], res);
    let (c, ready, o) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let xs: Vec<u32> = vals.iter().map(|v| f.local(*v)).collect();
    let pending = |f: &mut Func| {
        f.i32(0);
        for v in &vals {
            zero(f, *v);
        }
        f.op(Instruction::Return);
    };
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    pending(&mut f);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.set(c);
    f.get(c);
    f.get(c);
    f.op(Instruction::I32Load(mem(HEAP, F_POLL, 2)));
    f.call_indirect(vec![Val::I32], vec![Val::I32, Val::I32]);
    f.set(o);
    f.set(ready);
    f.get(ready);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    pending(&mut f);
    f.op(Instruction::End);
    flags_or(&mut f, 0, DONE);
    if let Some(source) = reference {
        f.get(o);
        f.helper(Helper::RefView {
            source: source.clone(),
        });
        f.set(xs[0]);
    } else {
        for (leaf, x) in leaves.iter().zip(&xs) {
            f.get(o);
            f.op(leaf.load(0));
            f.set(*x);
        }
        if let Some(w) = walk {
            for x in &xs {
                f.get(*x);
            }
            f.helper(Helper::Retain {
                vals: vals.clone(),
                walk: w.clone(),
            });
        }
    }
    if counts && !leaves.is_empty() {
        f.get(o);
        f.helper(Helper::ReleaseRef);
    }
    f.i32(1);
    for x in &xs {
        f.get(*x);
    }
    f
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn frame_guard_uses_scope_for_setup_and_return_for_early_exit() {
        let m = Module::default();
        let mut f = m.func("guarded", vec![Val::I32], vec![]);
        f.body = vec![Ins::Op(Instruction::Return)];
        f.origins = Some(vec![Some(10)]);
        f.origin_root = Some(99);
        let mut plain = f.clone();
        plain.origins = None;
        guard_frame(&mut plain);
        guard_frame(&mut f);
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        let spans = f.origins.as_ref().unwrap();
        assert_eq!(spans.len(), f.body.len());
        assert_eq!(spans.first(), Some(&Some(99)));
        assert_eq!(spans.last(), Some(&Some(99)));
        assert!(
            f.body
                .iter()
                .zip(spans)
                .any(|(i, s)| matches!(i, Ins::Op(Instruction::Return)) && *s == Some(10))
        );
        assert!(spans.iter().all(|s| matches!(s, Some(10 | 99))));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn generic_and_pending_frames_link_cancel_slots_only_when_needed() {
        let mut m = crate::module::Module::default();
        for enabled in [false, true] {
            m.cancellable = enabled;
            for f in [
                super::open_cold(&m, &[], None, false, None),
                super::pending_cold(&m, &[]),
            ] {
                let linked = f.body.iter().any(|ins| {
                    matches!(ins, crate::module::Ins::TableSlot(id)
                        if matches!(&*f.symbols.get(*id), crate::module::Sym::Helper(
                            crate::rt::Helper::CleanupCancel | crate::rt::Helper::PendingCancel)))
                });
                assert_eq!(linked, enabled, "{}", f.name);
            }
        }
    }

    use std::io::Write as _;
    use std::process::{Command, Stdio};

    use super::{Helper, Instruction as I, W_READY, W_SLOT};
    use crate::module::{HEAP, Module, Sym, Val, mem};

    #[test]
    fn weak_waiters_ignore_reused_and_exhausted_generations() {
        let mut m = Module::default();
        for (name, helper) in [
            ("newNode", Helper::WakerNew),
            ("dropNode", Helper::WakerDrop),
            ("live", Helper::WakerLive),
            ("link", Helper::WakeLink),
        ] {
            m.export(name, Sym::Helper(helper));
        }
        for (index, (name, offset)) in [("ready", W_READY), ("slot", W_SLOT)]
            .into_iter()
            .enumerate()
        {
            let mut f = m.func(name, vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.op(I::I32Load(mem(HEAP, offset, 2)));
            let key = Sym::Inst(hd_base::Hash128(index as u128 + 1));
            m.define(key.clone(), f);
            m.export(name, key);
        }
        let mut f = m.func("notify", vec![Val::I32], vec![]);
        f.get(0);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.helper(Helper::WakeNotify);
        let key = Sym::Inst(hd_base::Hash128(3));
        m.define(key.clone(), f);
        m.export("notify", key);
        let mut f = m.func("generation", vec![Val::I32; 2], vec![]);
        f.get(0);
        f.helper(Helper::WakerSlot);
        f.get(1);
        f.op(I::I32Store(mem(HEAP, 4, 2)));
        let key = Sym::Inst(hd_base::Hash128(4));
        m.define(key.clone(), f);
        m.export("generation", key);
        let mut f = m.func("peek", vec![Val::I32; 2], vec![Val::I32]);
        f.get(0);
        f.get(1);
        f.op(I::I32Add);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        let key = Sym::Inst(hd_base::Hash128(5));
        m.define(key.clone(), f);
        m.export("peek", key);
        let wasm = m.finish().expect("weak-waker module");
        let script = r"
const fs = require('node:fs');
const assert = require('node:assert/strict');
const module_ = new WebAssembly.Module(fs.readFileSync(0));
const imports = {};
for (const i of WebAssembly.Module.imports(module_)) {
  (imports[i.module] ??= {})[i.name] = () => { throw Error(i.module + ':' + i.name); };
}
const w = new WebAssembly.Instance(module_, imports).exports;
const parent = w.newNode(0), child = w.newNode(0);
const old = w.slot(parent);
w.link(child, parent, 1);
w.dropNode(parent);
const replacement = w.newNode(0);
assert.equal(w.slot(replacement), old, 'reuse the weak slot');
assert.equal(w.live(old, 1), 0, 'old generation is dead');
assert.equal(w.live(old, 2), replacement, 'new generation resolves');
w.notify(child);
assert.equal(w.ready(replacement), 0, 'a stale edge cannot wake the new occupant');
w.link(child, replacement, 2);
const other = w.newNode(0);
w.link(child, other, 4);
w.link(child, replacement, 8);
w.notify(child);
assert.equal(w.ready(replacement), 10, 'merge only one target generation');
assert.equal(w.ready(other), 4, 'keep distinct live awaiters');
w.dropNode(other);
w.link(child, replacement, 32);
let copies = 0;
for (let edge = w.peek(child, 0); edge; edge = w.peek(edge, 8)) {
  if (w.peek(edge, 0) === old && w.peek(edge, 4) === 2) copies++;
}
assert.equal(copies, 1, 'a stale head cannot hide an existing live target');
const otherAgain = w.newNode(0);
w.link(child, otherAgain, 64);
w.notify(child);
assert.equal(w.ready(otherAgain), 64, 'reuse stale storage after searching all live targets');
w.generation(old, -1);
w.link(child, replacement, 16);
assert.equal(w.live(old, -1), replacement);
w.dropNode(replacement);
const fresh = w.newNode(0);
assert.notEqual(w.slot(fresh), old, 'exhausted generation retires the slot');
assert.equal(w.live(old, -1), 0);
w.notify(child);
assert.equal(w.ready(fresh), 0, 'retired-generation edges remain dead');
let last;
for (let i = 0; i < 96; i++) last = w.newNode(0);
assert.equal(w.live(w.slot(child), 1), child, 'growth preserves old live rows');
w.link(child, last, 32);
w.notify(child);
assert.equal(w.ready(last), 32, 'weak lookup survives registry growth');
";
        let mut child = Command::new("node")
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&wasm)
            .expect("wasm input");
        let out = child.wait_with_output().expect("weak-waker test");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
