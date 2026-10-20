//! The task runtime of suspending code (`crates/hd_wasm_linear/DESIGN.md`
//! §23.21, job 13): the frame's task prefix and flags, the runtime checks
//! of a poll and a cancel, the wake table, the host's leaf frames, and the
//! entry's driver of a suspending root. These implement the task
//! protocol of §14 over linear memory.

use hd_base::Hash128;
use wasm_encoder::{BlockType, Instruction};

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

fn store(v: Val, at: u32) -> Instruction<'static> {
    let at = u64::from(at);
    match v {
        Val::I32 => Instruction::I32Store(mem(HEAP, at, 2)),
        Val::I64 => Instruction::I64Store(mem(HEAP, at, 3)),
        Val::F32 => Instruction::F32Store(mem(HEAP, at, 2)),
        Val::F64 => Instruction::F64Store(mem(HEAP, at, 3)),
    }
}

fn load(v: Val, at: u32) -> Instruction<'static> {
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
/// glue `drop` (none for `None`), in a fresh local of `f`. A frame is
/// tagged for the cycle collector ([`Helper::Opaque`]), which enters it
/// only while its saved slots are stable.
fn new_zeroed(f: &mut Func, size: u32, drop: Option<Sym>) -> u32 {
    let at = f.local(Val::I32);
    f.u32(size);
    let glue = drop.is_some();
    // A frame with glue takes part in cycle collection: what its saved
    // slots hold is not known here (§23.28).
    crate::rt::push_desc(f, drop, true);
    f.helper(Helper::New);
    f.set(at);
    if glue {
        f.get(at);
        f.helper(Helper::Opaque);
    }
    f.get(at);
    f.i32(0);
    f.u32(size);
    f.op(Instruction::MemoryFill(HEAP));
    at
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
        Helper::PollCheck => poll_check(),
        Helper::CancelCheck => cancel_check(),
        Helper::CancelEnter => {
            let mut f = Func::new("rt:cancel_enter", vec![], vec![Val::I32]);
            f.global_get(Global::AbSeen);
            f.i32(0);
            f.global_set(Global::AbSeen);
            f
        }
        Helper::CancelLeave => cancel_leave(),
        Helper::WakeMark => wake_mark(),
        Helper::WakeTake => wake_take(),
        Helper::WakeSlot => wake_slot(m),
        Helper::WakeRegister => wake_register(),
        Helper::WakeRemove => wake_remove(),
        Helper::WakeSignal => wake_signal(),
        Helper::WakerNew => waker_new(),
        Helper::WakerParent => waker_parent(),
        Helper::WakeLink => wake_link(),
        Helper::WakeNotify => wake_notify(),
        Helper::HostCold {
            module,
            method,
            args,
            area,
            res,
        } => host_cold(module, method, args, area, res),
        Helper::HostPoll {
            module,
            method,
            args,
            area,
            res,
        } => host_poll(module, method, args, area, res),
        Helper::HostCancel => host_cancel(),
        Helper::HostCall {
            module,
            method,
            args,
            result,
        } => crate::boundary::host_call(module, method, args, result),
        Helper::EntryCancel { abandon } => {
            let mut f = Func::new("rt:entry_cancel", vec![], vec![]);
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
        Helper::ChildDrop => child_drop(),
        Helper::CleanupCold => cleanup_cold(),
        Helper::CleanupPoll => cleanup_poll(),
        Helper::CleanupCancel => cleanup_cancel(),
        Helper::PendingCold { results } => pending_cold(results),
        Helper::PendingPoll { results } => pending_poll(results),
        Helper::PendingCancel => {
            let mut f = Func::new("rt:pending_cancel", vec![Val::I32], vec![]);
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
            0 => race_cold(results, walk.as_ref()),
            1 => race_poll(results),
            _ => race_cancel(),
        },
        Helper::RaceDrop { results, walk } => race_drop(results, walk.as_ref()),
        Helper::OpenFrame {
            part: 0,
            reference,
            leaves,
            walk,
            counts,
        } => open_cold(leaves, walk.as_ref(), *counts, reference.as_ref()),
        Helper::OpenFrame {
            reference,
            leaves,
            walk,
            counts,
            ..
        } => open_poll(leaves, walk.as_ref(), *counts, reference.as_ref()),
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
    f
}

/// Defers collector tracing while local 0's frame is executing. A body
/// may materialize a frame from 0, and a nested helper may already have
/// set RUNNING: restore only the previous running bit on every exit.
/// Results already on the Wasm stack are left in place by the stores.
pub(crate) fn guard_frame(f: &mut Func) {
    let body = std::mem::take(&mut f.body);
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
    for ins in body {
        if matches!(ins, Ins::Op(Instruction::Return)) {
            restore(f);
        }
        f.body.push(ins);
    }
    restore(f);
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
    let mut f = Func::new("rt:frame", vec![], vec![Val::I32]);
    let at = new_zeroed(&mut f, size, Some(Sym::Part(key, Part::Drop)));
    f.get(at);
    f.body.push(Ins::TableSlot(Sym::Part(key, Part::Poll)));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(at);
    f.body.push(Ins::TableSlot(Sym::Part(key, Part::Cancel)));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
    f.get(at);
    f
}

/// [`Helper::PollCheck`] (`crates/hd_wasm_linear/DESIGN.md` §14.3, §14.6):
/// whether the poll answers Pending at once. An active frame is a
/// re-entrant poll and a completed one an invalid state, both panics; a
/// cancelled one answers Pending, and a poll of one that still unwinds
/// continues its cleanup.
fn poll_check() -> Func {
    let mut f = Func::new("rt:poll_check", vec![Val::I32], vec![Val::I32]);
    flags_any(&mut f, 0, ACTIVE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, REENTRANT);
    f.op(Instruction::End);
    flags_any(&mut f, 0, DONE);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, INVALID);
    f.op(Instruction::End);
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
fn cancel_check() -> Func {
    let mut f = Func::new("rt:cancel_check", vec![Val::I32], vec![Val::I32]);
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
fn cancel_leave() -> Func {
    let mut f = Func::new("rt:cancel_leave", vec![Val::I32, Val::I32], vec![]);
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
/// A waker node holds its counted enclosing-waiter list and ready bits.
pub const W_READY: u64 = 8;

/// [`Helper::WakeSlot`]: grows the registration table before either a
/// registration or a wake. Allocating once per capacity keeps lookup O(1).
fn wake_slot(m: &Module) -> Func {
    let mut f = Func::new("rt:wake_slot", vec![Val::I32], vec![Val::I32]);
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
        f.global_get(Global::WakeTab);
        f.call(Sym::Inst(r.free));
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
fn wake_signal() -> Func {
    let mut f = Func::new("rt:wake_signal", vec![Val::I32, Val::I32], vec![]);
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

/// A shared cancellation cleanup can have several awaiters. Edges keep
/// every distinct enclosing node, merging child bits for the same node.
/// Their layout is parent, next edge, bits; all links own counts.
fn wake_link() -> Func {
    let mut f = Func::new("rt:wake_link", vec![Val::I32; 3], vec![]);
    let edge = f.local(Val::I32);
    f.get(1);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
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
    f.get(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(edge);
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.get(2);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(edge);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(edge);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(12);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::DropFields {
        leaves: vec![
            Leaf {
                offset: 0,
                val: Val::I32,
                bytes: 4,
                signed: false,
            },
            Leaf {
                offset: 4,
                val: Val::I32,
                bytes: 4,
                signed: false,
            },
        ],
        walk: Walk::Seq(vec![Walk::Ref(0), Walk::Ref(1)]),
    })));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.helper(Helper::New);
    f.set(edge);
    f.get(1);
    f.helper(Helper::RetainAny);
    // The previous head's count moves into the new edge.
    for (off, local) in [(0, 1), (8, 2)] {
        f.get(edge);
        f.get(local);
        f.op(Instruction::I32Store(mem(HEAP, off, 2)));
    }
    f.get(edge);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.get(0);
    f.get(edge);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f
}

fn wake_notify() -> Func {
    let mut f = Func::new("rt:wake_notify", vec![Val::I32], vec![]);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(0);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::WakeSignal);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(0);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

fn waker_parent() -> Func {
    let mut f = Func::new("rt:waker_parent", vec![Val::I32], vec![]);
    f.get(0);
    f.global_get(Global::Waker);
    f.global_get(Global::WakeBit);
    f.helper(Helper::WakeLink);
    f
}

fn waker_new() -> Func {
    let mut f = Func::new("rt:waker_new", vec![Val::I32], vec![Val::I32]);
    let node = f.local(Val::I32);
    f.i32(12);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::DropFields {
        leaves: vec![Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 4,
            signed: false,
        }],
        walk: Walk::Ref(0),
    })));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.helper(Helper::New);
    f.set(node);
    f.get(node);
    f.i32(0);
    f.i32(12);
    f.op(Instruction::MemoryFill(HEAP));
    f.get(node);
    f.helper(Helper::WakerParent);
    f.get(node);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, W_READY, 2)));
    f.get(node);
    f
}

fn wake_register() -> Func {
    let mut f = Func::new("rt:wake_register", vec![Val::I32], vec![]);
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
fn wake_remove() -> Func {
    let mut f = Func::new("rt:wake_remove", vec![Val::I32], vec![]);
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
fn wake_mark() -> Func {
    let mut f = Func::new("rt:wake_mark", vec![Val::I32], vec![]);
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
fn wake_take() -> Func {
    let mut f = Func::new("rt:wake_take", vec![Val::I32], vec![Val::I32]);
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
fn host_cold(module: &str, method: &str, args: &[HostArg], area: &Elem, res: &HostRes) -> Func {
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut f = Func::new(format!("rt:{module}/{method}"), params, vec![Val::I32]);
    let fr = new_zeroed(&mut f, LEAF_ARGS + area.size, leaf_drop(area));
    let poll = Sym::Helper(Helper::HostPoll {
        module: module.to_owned(),
        method: method.to_owned(),
        args: args.to_vec(),
        area: area.clone(),
        res: res.clone(),
    });
    f.get(fr);
    f.body.push(Ins::TableSlot(poll));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(fr);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::HostCancel)));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
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
            f.body.push(Ins::Call(Sym::Helper(Helper::RetainAny)));
        });
    }
    f.get(fr);
    f
}

/// [`Helper::HostPoll`]: `(frame) -> (ready, results)`. The first poll
/// calls `.start` with the arguments; a call that finished at once is
/// Ready, else its handle waits in the frame. A later poll is Pending
/// until the wake table holds the handle, then calls `.finish`.
fn host_poll(module: &str, method: &str, args: &[HostArg], area: &Elem, res: &HostRes) -> Func {
    let rvals = res.vals();
    let mut results = vec![Val::I32];
    results.extend(rvals.iter().copied());
    let mut f = Func::new(
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
fn host_cancel() -> Func {
    let mut f = Func::new("rt:host_cancel", vec![Val::I32], vec![]);
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
    main: Hash128,
    providers: &[(usize, Option<Hash128>)],
    rvals: &[Val],
    report: Option<Hash128>,
) -> Func {
    let mut f = Func::new("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
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
fn child_drop() -> Func {
    let mut f = Func::new("rt:drop_child", vec![Val::I32], vec![]);
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
fn cleanup_cold() -> Func {
    let mut f = Func::new("rt:cleanup_cold", vec![Val::I32], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, Some(Sym::Helper(Helper::ChildDrop)));
    f.get(fr);
    f.body
        .push(Ins::TableSlot(Sym::Helper(Helper::CleanupPoll)));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(fr);
    f.body
        .push(Ins::TableSlot(Sym::Helper(Helper::CleanupCancel)));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
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
fn cleanup_poll() -> Func {
    let mut f = Func::new("rt:cleanup_poll", vec![Val::I32], vec![Val::I32, Val::I32]);
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
fn cleanup_cancel() -> Func {
    let mut f = Func::new("rt:cleanup_cancel", vec![Val::I32], vec![]);
    let wait = f.local(Val::I32);
    cancel_guard(&mut f);
    continue_cleanup(&mut f, wait);
    mark_cancelled(&mut f, wait);
    f
}

/// [`Helper::PendingCold`]: `() -> frame`, a frame that never completes,
/// for std's `timeout!`.
fn pending_cold(results: &[Val]) -> Func {
    let mut f = Func::new("rt:pending_cold", vec![], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, None);
    f.get(fr);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::PendingPoll {
        results: results.to_vec(),
    })));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(fr);
    f.body
        .push(Ins::TableSlot(Sym::Helper(Helper::PendingCancel)));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
    f.get(fr);
    f
}

/// [`Helper::PendingPoll`]: Pending whether or not it was cancelled; it
/// registers no wake, since nothing will wake it.
fn pending_poll(results: &[Val]) -> Func {
    let mut res = vec![Val::I32];
    res.extend(results.iter().copied());
    let mut f = Func::new("rt:pending_poll", vec![Val::I32], res.clone());
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
fn race_cold(results: &[Val], walk: Option<&Walk>) -> Func {
    let (_, size) = race_area(results);
    let mut f = Func::new("rt:race_cold", vec![Val::I32], vec![Val::I32]);
    let drop = Sym::Helper(Helper::RaceDrop {
        results: results.to_vec(),
        walk: walk.cloned(),
    });
    let fr = new_zeroed(&mut f, size, Some(drop));
    let h = |k| Helper::Race {
        part: k,
        results: results.to_vec(),
        walk: walk.cloned(),
    };
    f.get(fr);
    f.body.push(Ins::TableSlot(Sym::Helper(h(1))));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(fr);
    f.body.push(Ins::TableSlot(Sym::Helper(h(2))));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
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
fn race_poll(results: &[Val]) -> Func {
    let (area, _) = race_area(results);
    let mut res = vec![Val::I32];
    res.extend(results.iter().copied());
    let mut f = Func::new("rt:race_poll", vec![Val::I32], res);
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
fn race_cancel() -> Func {
    let mut f = Func::new("rt:race_cancel", vec![Val::I32], vec![]);
    let wait = f.local(Val::I32);
    cancel_guard(&mut f);
    cancel_tasks(&mut f, None, wait);
    mark_cancelled(&mut f, wait);
    f
}

/// [`Helper::RaceDrop`]: one count less on the list, and on a winner's
/// value that no poll handed out.
fn race_drop(results: &[Val], walk: Option<&Walk>) -> Func {
    let (area, _) = race_area(results);
    let mut f = Func::new("rt:race_drop", vec![Val::I32], vec![]);
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
    leaves: &[Leaf],
    walk: Option<&Walk>,
    counts: bool,
    reference: Option<&crate::rt::RefLayout>,
) -> Func {
    let mut f = Func::new("rt:open_cold", vec![Val::I32], vec![Val::I32]);
    let fr = new_zeroed(&mut f, F_SAVED, Some(Sym::Helper(Helper::ChildDrop)));
    f.get(fr);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::OpenFrame {
        part: 1,
        reference: reference.cloned(),
        leaves: leaves.to_vec(),
        walk: walk.cloned(),
        counts,
    })));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    f.get(fr);
    f.body
        .push(Ins::TableSlot(Sym::Helper(Helper::CleanupCancel)));
    f.op(Instruction::I32Store(mem(HEAP, F_CANCEL, 2)));
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
    leaves: &[Leaf],
    walk: Option<&Walk>,
    counts: bool,
    reference: Option<&crate::rt::RefLayout>,
) -> Func {
    let vals: Vec<Val> = leaves.iter().map(|l| l.val).collect();
    let mut res = vec![Val::I32];
    res.extend(vals.iter().copied());
    let mut f = Func::new("rt:open_poll", vec![Val::I32], res);
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
