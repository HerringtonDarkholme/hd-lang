//! The task runtime of suspending code (`crates/hd_wasm/DESIGN.md`
//! §23.21, job 13): the frame's task prefix and flags, the runtime checks
//! of a poll and a cancel, the wake table, the host's leaf frames, and the
//! entry's driver of a suspending root. These implement the task
//! protocol of §14 over linear memory.

use crate::op::Op as Instruction;
use hd_base::Hash128;
use wasm_encoder::BlockType;

use crate::boundary::LTy;
use crate::layout::{Leaf, LeafOps as _};
use crate::module::{BUF, Func, Global, HEADER, HEAP, Import, Ins, Module, Part, Sym, Val, mem};
use crate::rt::Helper;
use crate::rt::{Elem, HostCounts};

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
    /// A local reference to a list of numbers, for a C provider
    /// (`module.host.package.c-references`): its value is the list
    /// place's address, and the import takes the elements' address and
    /// the length. A `&mut` one first makes the buffer the place's own
    /// with this element's `rt:list_unique`, so C writes no other holder's
    /// elements.
    List(Option<crate::rt::Elem>),
}

impl HostArg {
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            HostArg::Str => vec![Val::I32, Val::I32],
            HostArg::Scalar(v) => vec![*v],
            HostArg::Buf(b) => b.vals(),
            HostArg::List(_) => vec![Val::I32],
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
pub const LEAF_ARGS: u32 = (F_SAVED + 4).next_multiple_of(8);
/// Reserved padding in a host leaf's prefix: a borrowed leaf's argument-drop callback.
/// Other frame kinds do not read it.
pub const BORROWED_DROP: u64 = 20;
// Host leaves do not use a resume state or an owned child. A borrowed leaf
// uses those words for its live registration, which cannot outlive the drive.
const BLOCKING_NEXT: u64 = F_STATE;
const BLOCKING_READY: u64 = F_CHILD;
const HOST_BORROWED: i32 = 256;

/// Room for a host leaf, whose record of `values` has at most eight bytes per leaf.
pub fn host_frame_capacity(values: usize) -> u32 {
    LEAF_ARGS + u32::try_from(values).expect("host arguments") * 8
}

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
pub fn flags_or(f: &mut Func, t: u32, bits: i32) {
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
pub fn new_zeroed(f: &mut Func, size: u32, drop: Option<Sym>, cyclic: bool) -> u32 {
    new_zeroed_with(f, size, drop, cyclic, Helper::New)
}

/// [`new_zeroed`] through allocation helper `new`, [`Helper::New`] or
/// [`Helper::NewFixed`] of `size`.
fn new_zeroed_with(f: &mut Func, size: u32, drop: Option<Sym>, cyclic: bool, new: Helper) -> u32 {
    let at = f.local(Val::I32);
    f.u32(size);
    let tagged = drop.is_some() && cyclic;
    crate::rt::push_desc(f, drop, cyclic);
    f.helper(new);
    f.set(at);
    if tagged {
        f.get(at);
        f.helper(Helper::Opaque);
    }
    f.get(at);
    f.i32(0);
    f.u32(size);
    f.push(crate::bulk::fill(HEAP));
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
        Helper::FrameEnsure(key) => ensure_frame(*key, m),
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
        Helper::CancelChild => cancel_child(m),
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
            counts,
            res,
        } => host_cold(
            m,
            &HostLeaf {
                module,
                method,
                args,
                area,
                counts,
                res,
            },
        ),
        Helper::HostPoll {
            blocking,
            module,
            method,
            args,
            area,
            res,
        } => host_poll(m, *blocking, module, method, args, area, res),
        Helper::HostCancel => host_cancel(m),
        Helper::HostDrive {
            blocking,
            module,
            method,
            args,
            area,
            counts,
            res,
        } => host_drive(
            m,
            *blocking,
            &HostLeaf {
                module,
                method,
                args,
                area,
                counts,
                res,
            },
        ),
        Helper::Drive {
            blocking,
            cold,
            params,
            results,
        } => drive(m, *blocking, cold, params, results),
        Helper::HostCall {
            module,
            method,
            args,
            result,
            owned,
        } => crate::boundary::host_call(m, module, method, args, result, owned.as_ref()),
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
        Helper::WaitCell => wait_cell(m),
        Helper::WaitCold => wait_cold(m),
        Helper::WaitPoll => wait_poll(m),
        Helper::WaitSignal => wait_signal(m),
        Helper::IdleHook => idle_hook(m),
        Helper::IdleWake => idle_wake(m),
        Helper::IdleBlock => idle_block(m),
        Helper::Race {
            part,
            results,
            counted,
        } => match part {
            0 => race_cold(m, results, counted),
            1 => race_poll(m, results),
            _ => race_cancel(m),
        },
        Helper::RaceDrop { results, counted } => race_drop(m, results, counted),
        Helper::All { part, elem } => match part {
            0 => all_cold(m, elem),
            1 => all_poll(m, elem),
            2 => all_cancel(m, elem),
            3 => all_drop(m, elem),
            4 => all_node_poll(m, elem),
            _ => all_init(m),
        },
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
            | Helper::WaitPoll
            | Helper::Race { part: 1 | 2, .. }
            | Helper::All { part: 1 | 2, .. }
    ) {
        guard_frame(&mut f);
    }
    #[cfg(feature = "checked-report")]
    match h {
        Helper::WakerNew | Helper::Race { part: 0, .. } => crate::rt::checked_kind(&mut f, m, 4),
        Helper::Frame(_)
        | Helper::HostCold { .. }
        | Helper::CleanupCold
        | Helper::All { part: 0, .. }
        | Helper::PendingCold { .. }
        | Helper::WaitCold => crate::rt::checked_kind(&mut f, m, 3),
        Helper::WaitCell => crate::rt::checked_kind(&mut f, m, 4),
        _ => {}
    }
    f
}

/// Defers collector tracing while local 0's frame is executing. A body
/// may materialize a frame from 0, and a nested helper may already have
/// set RUNNING: restore only the previous running bit on every exit.
/// Results already on the Wasm stack are left in place by the stores.
pub fn guard_frame(f: &mut Func) {
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    f.origin_at = f.origin_root;
    let previous = f.local(Val::I32);
    let flags = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::If(BlockType::Empty));
    // One load of the flags word: the previous bit, then the word with it.
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.op(Instruction::LocalTee(flags));
    f.i32(RUNNING);
    f.op(Instruction::I32And);
    f.set(previous);
    f.get(flags);
    f.i32(RUNNING);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
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

/// Scalar storage after the stable addressable-cell region of a task.
#[cfg_attr(feature = "testing", derive(Debug))]
pub(crate) struct FrameSpill {
    pub start: u32,
    pub size: u32,
    pub row: Vec<u32>,
    pub reason: Option<hd_tir::own::Reason>,
    pub origin: Option<(String, u32, usize)>,
    pub names_counts: bool,
}

/// One allocation/migration/provider-acquisition helper per instance.
/// Addressable cells remain at their original heap addresses.
fn ensure_frame(key: Hash128, m: &Module) -> Func {
    let mut f = m.func("rt:frame_ensure", vec![Val::I32; 4], vec![Val::I32; 2]);
    if let Some(spill) = m.frame_spills.get(&key) {
        f.names_counts = spill.names_counts;
        f.reason = spill.reason;
        if let Some((item, root, len)) = &spill.origin {
            f.origins = Some(Vec::new());
            f.set_origin_source_len(*len);
            f.origin_item = Some(item.clone());
            f.origin_root = Some(*root);
            f.origin_at = f.origin_root;
        }
        f.get(0);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Frame(key));
        f.set(0);
        flags_or(&mut f, 0, RUNNING);
        f.get(0);
        f.u32(spill.start);
        f.op(Instruction::I32Add);
        f.get(1);
        f.u32(spill.size - spill.start);
        f.push(crate::bulk::copy(HEAP, HEAP));
        f.i32(1);
        f.set(2);
        f.op(Instruction::End);
        // Cold construction already acquired these counts. A frame made
        // for addressable slots acquires them only when handed out.
        f.get(3);
        f.get(2);
        f.op(Instruction::I32And);
        f.op(Instruction::If(BlockType::Empty));
        for &at in &spill.row {
            f.get(0);
            f.op(Instruction::I32Load(mem(HEAP, u64::from(at), 2)));
            f.count_call(
                Sym::Inst(m.runtime.retain),
                crate::views::CountAction::Retain,
            );
        }
        f.i32(0);
        f.set(2);
        f.op(Instruction::End);
    }
    f.get(0);
    f.get(2);
    f
}

/// [`Helper::Frame`]: a new frame of suspending instance `key`, zeroed,
/// with its poll and cancel.
fn frame(key: Hash128, m: &Module) -> Func {
    let size = m.frames.get(&key).copied().unwrap_or(F_SAVED);
    let mut f = m.func("rt:frame", vec![], vec![Val::I32]);
    // What a suspending body's frame saves is not known here, so it takes
    // part in cycle collection (§23.28).
    // Its size is known here, so it takes its size class's allocation
    // directly, as a value of known size does: every suspending call that
    // makes a frame, and every awaiter whose slots make one (#1707), pays
    // the class's free-list pop, not the generic allocator's size search.
    let new = if m.runtime.checked.is_none() && size + HEADER <= 2048 {
        Helper::NewFixed { size }
    } else {
        Helper::New
    };
    let at = new_zeroed_with(&mut f, size, Some(Sym::Part(key, Part::Drop)), true, new);
    f.get(at);
    f.table_slot(Sym::Part(key, Part::Poll));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, at, Sym::Part(key, Part::Cancel));
    f.get(at);
    f
}

/// [`Helper::PollCheck`] (`crates/hd_wasm/DESIGN.md` §14.3, §14.6):
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

/// [`Helper::CancelCheck`] (`crates/hd_wasm/DESIGN.md` §14.6): whether
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

/// [`Helper::CancelChild`]: one copy of what each point's cancellation
/// did inline: the cancel bracket around the child's cancel.
fn cancel_child(m: &mut Module) -> Func {
    let mut f = m.func("rt:cancel_child", vec![Val::I32], vec![Val::I32]);
    let (seen, c, wait) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.helper(Helper::CancelEnter);
    f.set(seen);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.set(c);
    f.get(c);
    f.op(Instruction::If(BlockType::Empty));
    f.get(c);
    f.get(c);
    f.op(Instruction::I32Load(mem(HEAP, F_CANCEL, 2)));
    f.call_indirect(vec![Val::I32], vec![]);
    f.get(c);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(UNWINDING);
    f.op(Instruction::I32And);
    f.set(wait);
    f.op(Instruction::End);
    f.get(0);
    f.get(seen);
    f.helper(Helper::CancelLeave);
    f.get(wait);
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
pub const W_SLOT: u64 = 4;

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
    let cap = f.local(Val::I32);
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
    crate::rt::grow_table(
        m,
        &mut f,
        [Global::NodeTab, Global::NodeCap],
        cap,
        NODE_STRIDE,
    );
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

/// [`Helper::WakeSlot`]: grows the general registration table before a
/// registration. Allocating once per capacity keeps lookup O(1).
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
    f.call(Sym::Inst(m.runtime.alloc));
    f.set(fresh);
    f.get(fresh);
    f.i32(0);
    f.get(cap);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::fill(HEAP));
    f.global_get(Global::WakeTab);
    f.op(Instruction::If(BlockType::Empty));
    f.get(fresh);
    f.global_get(Global::WakeTab);
    f.global_get(Global::WakeCap);
    f.i32(WAKE_STRIDE);
    f.op(Instruction::I32Mul);
    f.push(crate::bulk::copy(HEAP, HEAP));
    {
        let r = m.runtime;
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

/// Register only storage whose synchronous Wasm invocation remains active.
fn blocking_wake_register(f: &mut Func, frame: u32) {
    f.get(frame);
    f.global_get(Global::BlockingWakes);
    f.op(Instruction::I32Store(mem(HEAP, BLOCKING_NEXT, 2)));
    f.get(frame);
    f.global_set(Global::BlockingWakes);
}

/// Remove by address, including a cancelled outer drive below a nested one.
fn blocking_wake_remove(f: &mut Func, frame: u32) {
    let (previous, at, next) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.global_get(Global::BlockingWakes);
    f.set(at);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, BLOCKING_NEXT, 2)));
    f.set(next);
    f.get(at);
    f.get(frame);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(previous);
    f.op(Instruction::If(BlockType::Empty));
    f.get(previous);
    f.get(next);
    f.op(Instruction::I32Store(mem(HEAP, BLOCKING_NEXT, 2)));
    f.op(Instruction::Else);
    f.get(next);
    f.global_set(Global::BlockingWakes);
    f.op(Instruction::End);
    for offset in [BLOCKING_NEXT, BLOCKING_READY] {
        f.get(at);
        f.i32(0);
        f.op(Instruction::I32Store(mem(HEAP, offset, 2)));
    }
    f.op(Instruction::Br(2));
    f.op(Instruction::End);
    f.get(at);
    f.set(previous);
    f.get(next);
    f.set(at);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

fn blocking_wake_mark(f: &mut Func, handle: u32) {
    let at = f.local(Val::I32);
    f.global_get(Global::BlockingWakes);
    f.set(at);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.get(handle);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, BLOCKING_READY, 2)));
    f.op(Instruction::End);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, BLOCKING_NEXT, 2)));
    f.set(at);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
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
    crate::rt::push_desc(
        &mut f,
        Some(Sym::Helper(Helper::DropParts {
            leaves: vec![Leaf {
                offset: 8,
                val: Val::I32,
                bytes: 4,
                signed: false,
            }],
            parts: vec![(0, ADDRESS)],
        })),
        false,
    );
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
    blocking_wake_mark(&mut f, h);
    // Delivery never creates a registration. A valid general registration
    // already reserved its slot before returning Pending to the host driver.
    slot(&mut f, h);
    f.global_get(Global::WakeCap);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    wake_address(&mut f, h);
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
/// `area`: one count less on each of its arguments.
#[must_use]
pub fn leaf_drop(area: &Elem) -> Option<Sym> {
    (!area.parts.is_empty()).then(|| parts_drop(area, &area.parts))
}

/// The glue that releases `parts` of a leaf frame's arguments laid out as
/// `area`: each part through its type's count, a hooked one in place.
fn parts_drop(area: &Elem, parts: &[(u32, crate::rt::Counted)]) -> Sym {
    let leaves = area
        .leaves
        .iter()
        .map(|l| Leaf {
            offset: l.offset + LEAF_ARGS,
            ..*l
        })
        .collect();
    Sym::Helper(Helper::DropParts {
        leaves,
        parts: parts.to_vec(),
    })
}

/// One address found through its block, as a counted reference is.
const ADDRESS: crate::rt::Counted = crate::rt::Counted::Address {
    index: 0,
    known: false,
};

/// [`Helper::HostCold`]: `(data, arguments) -> frame`, a host method
/// that may wait, called cold: a leaf frame holding the arguments, laid
/// out as `area`, taking transferred counts and copying borrowed
/// arguments as declared by the slot. No handle exists yet (-1).
pub fn host_cold(m: &Module, leaf: &HostLeaf<'_>) -> Func {
    let HostLeaf {
        module,
        method,
        args,
        area,
        ..
    } = *leaf;
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut f = m.func(format!("rt:{module}/{method}"), params, vec![Val::I32]);
    // A leaf frame takes part as its arguments' types say.
    let fr = new_zeroed(&mut f, LEAF_ARGS + area.size, leaf_drop(area), area.cyclic);
    init_host_leaf(&mut f, m, fr, leaf, false);
    f.get(fr);
    f
}

/// `std.thread`'s wait for its thread's end (`module.rt.thread.running`):
/// a leaf of `hd:rt` `thread_watch(address, seen)`, whose host waits off
/// this thread until the word at `address` no longer holds `seen`, and
/// whose cancel ends that wait. Cost: one frame and one host operation.
pub fn thread_watch() -> Helper {
    let word = |offset| Leaf {
        offset,
        val: Val::I32,
        bytes: 4,
        signed: false,
    };
    Helper::HostCold {
        module: "hd:rt".to_owned(),
        method: "thread_watch".to_owned(),
        args: vec![HostArg::Scalar(Val::I32); 2],
        area: Elem {
            size: 8,
            leaves: vec![word(0), word(4)],
            counted: crate::rt::Counted::None,
            parts: Vec::new(),
            cyclic: false,
        },
        counts: Box::default(),
        res: HostRes::Void,
    }
}

pub struct HostLeaf<'a> {
    pub module: &'a str,
    pub method: &'a str,
    pub args: &'a [HostArg],
    pub area: &'a Elem,
    pub counts: &'a HostCounts,
    pub res: &'a HostRes,
}

fn init_host_leaf(f: &mut Func, m: &Module, fr: u32, leaf: &HostLeaf<'_>, blocking: bool) {
    let HostLeaf {
        module,
        method,
        args,
        area,
        counts,
        res,
        ..
    } = *leaf;
    let poll = Sym::Helper(Helper::HostPoll {
        blocking,
        module: module.to_owned(),
        method: method.to_owned(),
        args: args.to_vec(),
        area: area.clone(),
        res: res.clone(),
    });
    f.get(fr);
    f.table_slot(poll);
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(f, m, fr, Sym::Helper(Helper::HostCancel));
    f.get(fr);
    f.i32(-1);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(F_SAVED), 2)));
    for (k, l) in area.leaves.iter().enumerate() {
        f.get(fr);
        f.get(u32::try_from(k + 1).expect("params"));
        f.op(l.store(LEAF_ARGS));
    }
    // An owned leaf keeps borrowed references past its caller by retaining
    // them. Collection values transfer their existing counts. A blocking
    // leaf's borrowed fields remain live in the caller, and its transferred
    // fields are released by the shadow frame's terminal callback.
    if !blocking {
        for (first, counted) in &counts.borrowed {
            let first = *first;
            counted.emit(f, m, &|f, k| f.get(first + k + 1), true);
        }
    }
}

/// [`Helper::HostPoll`]: `(frame) -> (ready, results)`. The first poll
/// calls `.start` with the arguments; a call that finished at once is
/// Ready, else its handle waits in the frame. A later poll is Pending
/// until the wake table holds the handle, then calls `.finish`.
fn host_poll(
    m: &Module,
    blocking: bool,
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
        format!(
            "rt:{module}/{method}.{}",
            if blocking { "block_poll" } else { "poll" }
        ),
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
    if blocking {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, BLOCKING_READY, 2)));
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        pending(&mut f);
        f.op(Instruction::End);
        blocking_wake_remove(&mut f, 0);
    } else {
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
    }
    f.get(h);
    f.call(Sym::Import(Import::new(
        module,
        &format!("{method}.finish"),
        &[Val::I32],
        &[Val::I32],
    )));
    f.set(n);
    if !blocking {
        f.op(Instruction::End);
    }
    flags_or(&mut f, 0, DONE);
    f.i32(1);
    host_result(&mut f, res, n);
    f
}

/// Pushes a finished host call's results from the exchange buffer, whose
/// length is local `n`.
fn host_result(f: &mut Func, res: &HostRes, n: u32) {
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
}

/// [`Helper::HostDrive`]: `(data, arguments) -> (frame, results)`, a
/// host method that may wait, awaited at once (§23.39). It calls `.start`
/// with the arguments as the leaf's first poll would. A call that
/// finished at once answers frame 0 and its results, with no frame made.
/// An async Pending call gets the owned leaf that [`Helper::HostCold`] makes.
/// A blocking slot receives caller-local storage as its final parameter;
/// Pending initializes the same prefix there, with a terminal callback
/// for transferred argument counts. Both register the handle with the
/// current waker. The blocking caller keeps borrowed arguments live
/// until its drive ends.
pub fn host_drive(m: &Module, blocking: bool, leaf: &HostLeaf<'_>) -> Func {
    let HostLeaf {
        module,
        method,
        args,
        area,
        counts,
        res,
    } = *leaf;
    let owned = (!counts.owned.is_empty()).then_some(&counts.owned);
    let rvals = res.vals();
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut results = vec![Val::I32];
    results.extend(rvals.iter().copied());
    let nparams = u32::try_from(params.len()).expect("params");
    if blocking {
        params.push(Val::I32);
    }
    let suffix = if blocking { "block_drive" } else { "drive" };
    let mut f = m.func(format!("rt:{module}/{method}.{suffix}"), params, results);
    let (st, n, fr) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let imp = crate::boundary::push_args(&mut f, args, &|f, k| f.get(k + 1));
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
    if blocking {
        // The caller's lexical shadow frame outlives the synchronous drive.
        // Its zero count header makes any ordinary reference drop a no-op.
        f.get(nparams);
        f.set(fr);
        f.get(fr);
        f.u32(crate::module::HEADER);
        f.op(Instruction::I32Sub);
        f.i32(0);
        f.u32(crate::module::HEADER + LEAF_ARGS + area.size);
        f.push(crate::bulk::fill(HEAP));
        init_host_leaf(&mut f, m, fr, leaf, true);
        flags_or(&mut f, fr, HOST_BORROWED);
        f.get(fr);
        if let Some(owned) = owned {
            f.table_slot(parts_drop(area, owned));
        } else {
            f.i32(-1);
        }
        f.op(Instruction::I32Store(mem(HEAP, BORROWED_DROP, 2)));
    } else {
        for k in 0..nparams {
            f.get(k);
        }
        f.helper(Helper::HostCold {
            module: module.to_owned(),
            method: method.to_owned(),
            args: args.to_vec(),
            area: area.clone(),
            counts: Box::new(counts.clone()),
            res: res.clone(),
        });
        f.set(fr);
    }
    f.get(fr);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, u64::from(F_SAVED), 2)));
    if blocking {
        blocking_wake_register(&mut f, fr);
    } else {
        f.get(n);
        f.helper(Helper::WakeRegister);
    }
    f.get(fr);
    for v in &rvals {
        zero(&mut f, *v);
    }
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.i32(0);
    host_result(&mut f, res, n);
    if let Some(owned) = owned {
        crate::boundary::release_host_arguments(&mut f, m, area, owned);
    }
    f
}

/// [`Helper::Drive`]: `(params) -> (frame, results)`, the cold slot
/// `cold` and its frame's first poll (§23.39), so a provider written in
/// hd behaves and allocates as a cold call polled at once does. A Ready
/// frame is released here, its results handed on; a Pending one is
/// handed on with zero results.
fn drive(m: &Module, blocking: bool, cold: &Helper, params: &[Val], results: &[Val]) -> Func {
    let mut all = vec![Val::I32];
    all.extend(results.iter().copied());
    let signature = if blocking {
        [params.to_vec(), vec![Val::I32]].concat()
    } else {
        params.to_vec()
    };
    let mut f = m.func(
        if blocking {
            "rt:block_drive"
        } else {
            "rt:drive"
        },
        signature,
        all,
    );
    let fr = f.local(Val::I32);
    let tmp: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    for k in 0..u32::try_from(params.len()).expect("params") {
        f.get(k);
    }
    f.helper(cold.clone());
    f.set(fr);
    f.get(fr);
    f.get(fr);
    f.op(Instruction::I32Load(mem(HEAP, F_POLL, 2)));
    let mut polled = vec![Val::I32];
    polled.extend(results.iter().copied());
    f.call_indirect(vec![Val::I32], polled);
    for l in tmp.iter().rev() {
        f.set(*l);
    }
    f.op(Instruction::If(BlockType::Empty));
    {
        f.get(fr);
        f.helper(Helper::ReleaseRef);
    }
    f.i32(0);
    for l in &tmp {
        f.get(*l);
    }
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(fr);
    for v in results {
        zero(&mut f, *v);
    }
    f
}

/// [`Helper::HostCancel`]: a leaf's cancel. A done or cancelled call is
/// left; any other is marked cancelled, and abandoned under `abandon()`.
/// A call waiting on the host is aborted. No completion will wake its
/// awaiter, so the entry polls its root again.
fn host_cancel(m: &Module) -> Func {
    let mut f = m.func("rt:host_cancel", vec![Val::I32], vec![]);
    flags_any(&mut f, 0, DONE | CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_FLAGS, 2)));
    f.i32(CANCELLED);
    f.op(Instruction::I32Or);
    f.global_get(Global::Aband);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32Eqz);
    f.i32(ABANDONED);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Or);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.i32(0);
    f.op(Instruction::I32GeS);
    f.op(Instruction::If(BlockType::Empty));
    flags_any(&mut f, 0, HOST_BORROWED);
    f.op(Instruction::If(BlockType::Empty));
    blocking_wake_remove(&mut f, 0);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.call(Sym::Import(Import::new("hd:rt", "abort", &[Val::I32], &[])));
    let at = f.local(Val::I32);
    flags_any(&mut f, 0, HOST_BORROWED);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    let h = f.local(Val::I32);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.set(h);
    slot(&mut f, h);
    f.global_get(Global::WakeCap);
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    wake_address(&mut f, h);
    f.set(at);
    f.get(at);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.helper(Helper::WakeNotify);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.helper(Helper::WakeRemove);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(1);
    f.global_set(Global::Rewake);
    f.op(Instruction::End);
    f
}

/// `hd.poll(ctx)` of a suspending root (`crates/hd_wasm/DESIGN.md` §14.4):
/// the first poll builds the root's frame with the host providers (no
/// data beside each vtable of host stubs) and claims it for its driver;
/// each poll then polls it, again while an abandon, an aborted host call
/// or a `block_on` that took host completions asks for it. -1 is Pending; a Ready root's result goes to
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
    if m.idle {
        // Every task waits: run the idle hook before the host waits
        // (§14.4, "Idle hook").
        f.get(ready);
        f.global_get(Global::Rewake);
        f.op(Instruction::I32Or);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::IdleWake);
        f.op(Instruction::End);
    }
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
/// frame unwinding while the task unwinds (`req.cancel.nested.using-split`).
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
pub fn pending_cold(m: &Module, results: &[Val]) -> Func {
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

/// A wait cell's bytes: the head of its waiter edges (counted, as
/// [`wake_link`] makes them), then its epoch, which each signal raises.
const CELL_EPOCH: u64 = 4;

/// [`Helper::WaitCell`]: `() -> cell`, a cell with no waiters. Its glue
/// releases the waiter edges still linked.
fn wait_cell(m: &Module) -> Func {
    let mut f = m.func("rt:wait_cell", vec![], vec![Val::I32]);
    let edges = Helper::DropParts {
        leaves: vec![Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 4,
            signed: false,
        }],
        parts: vec![(0, ADDRESS)],
    };
    let cell = new_zeroed(&mut f, 8, Some(Sym::Helper(edges)), false);
    f.get(cell);
    f
}

/// [`Helper::WaitCold`]: `(cell) -> frame`, a frame over `cell`, which
/// it holds as its child with a count of its own, and the cell's epoch
/// when it was made. No wait is lost between a check in hd code and the
/// frame's first poll: nothing runs in between
/// (`req.schedule.yield-points`), and a signal in between raises the
/// epoch the frame compares.
fn wait_cold(m: &Module) -> Func {
    let mut f = m.func("rt:wait_cold", vec![Val::I32], vec![Val::I32]);
    let fr = new_zeroed(
        &mut f,
        F_SAVED + 4,
        Some(Sym::Helper(Helper::ChildDrop)),
        false,
    );
    f.get(fr);
    f.table_slot(Sym::Helper(Helper::WaitPoll));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(&mut f, m, fr, Sym::Helper(Helper::PendingCancel));
    f.get(fr);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, F_CHILD, 2)));
    f.get(0);
    f.helper(Helper::RetainAny);
    f.get(fr);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, CELL_EPOCH, 2)));
    f.op(Instruction::I32Store(mem(HEAP, u64::from(F_SAVED), 2)));
    f.get(fr);
    f
}

/// [`Helper::WaitPoll`]: Ready once the cell's epoch moved on; else it
/// links the enclosing waker to the cell and stays Pending. A cancelled
/// frame stays Pending, as `PendingPoll` does.
fn wait_poll(m: &Module) -> Func {
    let mut f = m.func("rt:wait_poll", vec![Val::I32], vec![Val::I32]);
    let cell = f.local(Val::I32);
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_CHILD, 2)));
    f.set(cell);
    f.get(cell);
    f.op(Instruction::I32Load(mem(HEAP, CELL_EPOCH, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(F_SAVED), 2)));
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    flags_or(&mut f, 0, DONE);
    f.i32(1);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(cell);
    f.global_get(Global::Waker);
    f.global_get(Global::WakeBit);
    f.helper(Helper::WakeLink);
    f.i32(0);
    f
}

/// [`Helper::WaitSignal`]: `(cell)`, raises the epoch, wakes each waiter
/// linked to the cell and unlinks them (a waiter still pending links
/// again on its next poll), then asks the driver to poll again, as a
/// host cancel does: no host event will.
fn wait_signal(m: &Module) -> Func {
    let mut f = m.func("rt:wait_signal", vec![Val::I32], vec![]);
    let head = f.local(Val::I32);
    f.get(0);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, CELL_EPOCH, 2)));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, CELL_EPOCH, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(head);
    f.get(0);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(head);
    f.op(Instruction::If(BlockType::Empty));
    f.get(head);
    f.helper(Helper::WakeNotify);
    f.get(head);
    f.helper(Helper::ReleaseRef);
    f.op(Instruction::End);
    f.i32(1);
    f.global_set(Global::Rewake);
    f
}

/// [`Helper::IdleHook`]: `(code, env)`, installs std's idle hook, a
/// function value of type `fn() -> void`, in [`Global::IdleHook`] and
/// [`Global::IdleEnv`]. std passes a top-level function, whose
/// environment is empty, so the globals own no count. Installing it again
/// replaces it.
fn idle_hook(m: &Module) -> Func {
    let mut f = m.func("rt:idle_hook", vec![Val::I32, Val::I32], vec![]);
    f.get(0);
    f.global_set(Global::IdleHook);
    f.get(1);
    f.global_set(Global::IdleEnv);
    f
}

/// [`Helper::IdleWake`]: `()`, runs the idle hook once, if one is
/// installed: the drivers call it where every task waits and they would
/// wait for the host. The hook signals the wait cells of what it made
/// ready, which sets `Rewake`; a hook that readied nothing leaves it
/// clear, and the driver waits for the host. Cost: one global read when no
/// hook is installed.
fn idle_wake(m: &Module) -> Func {
    let mut f = m.func("rt:idle_wake", vec![], vec![]);
    f.global_get(Global::IdleHook);
    f.op(Instruction::If(BlockType::Empty));
    // A function value's call: its environment, then the context of its
    // empty row, through its code's table slot (`emit/closure.rs`).
    f.global_get(Global::IdleEnv);
    f.i32(0);
    f.global_get(Global::IdleHook);
    f.call_indirect(vec![Val::I32, Val::I32], vec![]);
    f.op(Instruction::End);
    f
}

/// [`Helper::IdleBlock`]: `() -> completions`, `block_on`'s wait. With an
/// idle hook reachable, the hook runs first; when it readied something,
/// no host completion is taken and the driver's next poll follows at
/// once, so the hook's `Rewake` is cleared. Else the host's `block`.
fn idle_block(m: &Module) -> Func {
    let mut f = m.func("rt:idle_block", vec![], vec![Val::I32]);
    if m.idle {
        f.helper(Helper::IdleWake);
        f.global_get(Global::Rewake);
        f.op(Instruction::If(BlockType::Empty));
        f.i32(0);
        f.global_set(Global::Rewake);
        f.i32(0);
        f.op(Instruction::Return);
        f.op(Instruction::End);
    }
    f.call(Sym::Import(Import::new("hd:rt", "block", &[], &[Val::I32])));
    f
}

// A homogeneous all frame owns its task list, a result buffer whose
// published length stays zero until complete, per-child initialized bits,
// and a compact 31-way waker tree. Internal nodes summarize ready branches;
// draining set bits visits no unwoken sibling. Wasm32 bounds depth to seven.
const ALL_TASKS: u64 = 24;
const ALL_RESULTS: u64 = 28;
const ALL_LEFT: u64 = 32;
const ALL_LEN: u64 = 36;
const ALL_COUNT: u64 = 40;
const ALL_NODES: u64 = 44;
const ALL_CELLS: u32 = 48;
const ALL_NODE_SIZE: u32 = 16;
const ALL_LEAF: i32 = i32::MIN;

pub fn all_field(f: &mut Func, frame: u32, offset: u64) {
    f.get(frame);
    f.op(Instruction::I32Load(mem(HEAP, offset, 2)));
}

fn all_cell(f: &mut Func, frame: u32, index: u32) {
    f.get(frame);
    f.u32(ALL_CELLS);
    f.op(Instruction::I32Add);
    f.get(index);
    f.i32(4);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
}

fn all_node(f: &mut Func, frame: u32, index: u32) {
    all_field(f, frame, ALL_NODES);
    f.get(index);
    f.u32(ALL_NODE_SIZE);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
}

fn all_child(f: &mut Func, frame: u32, index: u32) {
    all_field(f, frame, ALL_TASKS);
    f.get(index);
    f.i32(4);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load(mem(
        HEAP,
        u64::from(crate::rt::LIST_ELEMS),
        2,
    )));
}

pub fn all_for(f: &mut Func, end: u32, mut body: impl FnMut(&mut Func, u32)) {
    let index = f.local(Val::I32);
    f.i32(0);
    f.set(index);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(index);
    f.get(end);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    body(f, index);
    f.get(index);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(index);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

fn all_done(f: &mut Func, index: u32, state: i32) {
    all_cell(f, 0, index);
    f.i32(state);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(0);
    all_field(f, 0, ALL_LEFT);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Store(mem(HEAP, ALL_LEFT, 2)));
}

fn all_cold(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:all_cold", vec![Val::I32], vec![Val::I32]);
    let (n, count, width, previous, offset, row, base, children, at) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, crate::rt::LIST_LEN, 2)));
    f.set(n);
    // No overflowing n+30: quotient plus a nonzero remainder.
    f.get(n);
    f.i32(31);
    f.op(Instruction::I32DivU);
    f.get(n);
    f.i32(31);
    f.op(Instruction::I32RemU);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32Add);
    f.set(width);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(width);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(count);
    f.get(width);
    f.op(Instruction::I32Add);
    f.set(count);
    f.get(width);
    f.i32(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::BrIf(1));
    f.get(width);
    f.i32(30);
    f.op(Instruction::I32Add);
    f.i32(31);
    f.op(Instruction::I32DivU);
    f.set(width);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    let bytes = f.local(Val::I64);
    f.get(n);
    f.op(Instruction::I64ExtendI32U);
    f.op(Instruction::I64Const(4));
    f.op(Instruction::I64Mul);
    f.get(count);
    f.op(Instruction::I64ExtendI32U);
    f.op(Instruction::I64Const(i64::from(ALL_NODE_SIZE)));
    f.op(Instruction::I64Mul);
    f.op(Instruction::I64Add);
    f.op(Instruction::I64Const(i64::from(ALL_CELLS)));
    f.op(Instruction::I64Add);
    f.set(bytes);
    f.get(bytes);
    f.op(Instruction::I64Const(i64::from(
        u32::MAX - crate::module::HEADER,
    )));
    f.op(Instruction::I64GtU);
    f.op(Instruction::If(BlockType::Empty));
    panic(&mut f, "heap-exhausted: an all frame does not fit the heap");
    f.op(Instruction::End);
    f.get(bytes);
    f.op(Instruction::I32WrapI64);
    crate::rt::push_desc(
        &mut f,
        Some(Sym::Helper(Helper::All {
            part: 3,
            elem: elem.clone(),
        })),
        true,
    );
    f.helper(Helper::New);
    f.set(at);
    f.get(at);
    f.helper(Helper::Opaque);
    f.get(at);
    f.i32(0);
    f.get(bytes);
    f.op(Instruction::I32WrapI64);
    f.push(crate::bulk::fill(HEAP));
    f.get(at);
    f.table_slot(Sym::Helper(Helper::All {
        part: 1,
        elem: elem.clone(),
    }));
    f.op(Instruction::I32Store(mem(HEAP, F_POLL, 2)));
    cancel_slot(
        &mut f,
        m,
        at,
        Sym::Helper(Helper::All {
            part: 2,
            elem: elem.clone(),
        }),
    );
    for field in [ALL_LEN, ALL_LEFT] {
        f.get(at);
        f.get(n);
        f.op(Instruction::I32Store(mem(HEAP, field, 2)));
    }
    f.get(at);
    f.get(count);
    f.op(Instruction::I32Store(mem(HEAP, ALL_COUNT, 2)));
    f.get(at);
    f.get(at);
    f.get(n);
    f.i32(4);
    f.op(Instruction::I32Mul);
    f.u32(ALL_CELLS);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, ALL_NODES, 2)));
    f.get(at);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, ALL_TASKS, 2)));
    f.get(0);
    f.helper(Helper::RetainAny);
    f.get(at);
    f.get(n);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.op(Instruction::I32Store(mem(HEAP, ALL_RESULTS, 2)));
    // Build leaf metadata, then successive parent levels. Children at each
    // level are contiguous, so ctz traversal preserves input order.
    f.get(n);
    f.set(children);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(children);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(children);
    f.i32(30);
    f.op(Instruction::I32Add);
    f.i32(31);
    f.op(Instruction::I32DivU);
    f.set(width);
    all_for(&mut f, width, |f, j| {
        f.get(j);
        f.i32(31);
        f.op(Instruction::I32Mul);
        f.set(base);
        f.get(offset);
        f.get(j);
        f.op(Instruction::I32Add);
        f.set(row);
        all_node(f, at, row);
        let node = f.local(Val::I32);
        f.set(node);
        f.get(node);
        f.get(previous);
        f.get(base);
        f.op(Instruction::I32Add);
        f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
        f.get(node);
        f.i32(1);
        f.get(children);
        f.get(base);
        f.op(Instruction::I32Sub);
        f.i32(31);
        f.op(Instruction::I32LtU);
        f.op(Instruction::If(BlockType::Result(
            wasm_encoder::ValType::I32,
        )));
        f.get(children);
        f.get(base);
        f.op(Instruction::I32Sub);
        f.op(Instruction::Else);
        f.i32(31);
        f.op(Instruction::End);
        f.op(Instruction::I32Shl);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.op(Instruction::I32Store(mem(HEAP, 8, 2)));
        f.get(offset);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.get(node);
        f.i32(ALL_LEAF);
        f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
        f.op(Instruction::Else);
        let end = f.local(Val::I32);
        f.get(node);
        f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
        f.op(Instruction::I32Popcnt);
        f.set(end);
        all_for(f, end, |f, k| {
            f.get(previous);
            f.get(base);
            f.op(Instruction::I32Add);
            f.get(k);
            f.op(Instruction::I32Add);
            let child = f.local(Val::I32);
            f.set(child);
            all_node(f, at, child);
            let child_at = f.local(Val::I32);
            f.set(child_at);
            f.get(child_at);
            f.get(child_at);
            f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
            f.i32(ALL_LEAF);
            f.op(Instruction::I32And);
            f.get(row);
            f.op(Instruction::I32Or);
            f.op(Instruction::I32Store(mem(HEAP, 12, 2)));
        });
        f.op(Instruction::End);
    });
    f.get(width);
    f.i32(1);
    f.op(Instruction::I32Eq);
    f.op(Instruction::BrIf(1));
    f.get(offset);
    f.set(previous);
    f.get(offset);
    f.get(width);
    f.op(Instruction::I32Add);
    f.set(offset);
    f.get(width);
    f.set(children);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.get(at);
    f
}

fn all_init(m: &Module) -> Func {
    let mut f = m.func("rt:all_init", vec![Val::I32], vec![]);
    let (count, parent, parent_bit, node, index) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.global_get(Global::Waker);
    f.set(parent);
    f.global_get(Global::WakeBit);
    f.set(parent_bit);
    all_field(&mut f, 0, ALL_COUNT);
    f.set(count);
    all_for(&mut f, count, |f, j| {
        f.get(count);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.get(j);
        f.op(Instruction::I32Sub);
        f.set(index);
        all_node(f, 0, index);
        f.set(node);
        f.get(j);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        f.get(parent);
        f.global_set(Global::Waker);
        f.get(parent_bit);
        f.global_set(Global::WakeBit);
        f.op(Instruction::Else);
        f.get(node);
        f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
        f.i32(i32::MAX);
        f.op(Instruction::I32And);
        let parent_index = f.local(Val::I32);
        f.set(parent_index);
        all_node(f, 0, parent_index);
        let parent_at = f.local(Val::I32);
        f.set(parent_at);
        f.get(parent_at);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.global_set(Global::Waker);
        f.i32(1);
        f.get(index);
        f.get(parent_at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Sub);
        f.op(Instruction::I32Shl);
        f.global_set(Global::WakeBit);
        f.op(Instruction::End);
        f.get(node);
        f.get(node);
        f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
        f.helper(Helper::WakerNew);
        f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    });
    f.get(parent);
    f.global_set(Global::Waker);
    f.get(parent_bit);
    f.global_set(Global::WakeBit);
    f.get(0);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, F_STATE, 2)));
    f
}

fn all_cancel_child(f: &mut Func, index: u32) {
    let child = f.local(Val::I32);
    all_child(f, 0, index);
    f.set(child);
    call_cancel(f, child);
    f.global_get(Global::AbSeen);
    flags_any(f, child, ABANDONED);
    f.op(Instruction::I32Or);
    f.global_set(Global::AbSeen);
    flags_any(f, child, UNWINDING);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    all_done(f, index, 2);
    f.op(Instruction::End);
}

fn all_node_poll(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:all_node", vec![Val::I32, Val::I32], vec![]);
    let (node, waker, bits, bit, index, child, ready, result) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let tmp: Vec<_> = elem.leaves.iter().map(|l| f.local(l.val)).collect();
    all_node(&mut f, 0, 1);
    f.set(node);
    f.get(node);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(waker);
    f.get(waker);
    f.op(Instruction::I32Load(mem(HEAP, W_READY, 2)));
    f.set(bits);
    f.get(waker);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, W_READY, 2)));
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(bits);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    f.get(bits);
    f.op(Instruction::I32Ctz);
    f.set(bit);
    f.get(bits);
    f.get(bits);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32And);
    f.set(bits);
    f.get(node);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.get(bit);
    f.op(Instruction::I32Add);
    f.set(index);
    f.get(node);
    f.op(Instruction::I32Load(mem(HEAP, 12, 2)));
    f.i32(0);
    f.op(Instruction::I32LtS);
    f.op(Instruction::If(BlockType::Empty));
    all_cell(&mut f, 0, index);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.get(waker);
    f.global_set(Global::Waker);
    f.i32(1);
    f.get(bit);
    f.op(Instruction::I32Shl);
    f.global_set(Global::WakeBit);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
    f.i32(2);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    all_cancel_child(&mut f, index);
    f.op(Instruction::Else);
    all_child(&mut f, 0, index);
    f.set(child);
    if !m.release {
        f.global_get(Global::AllPolls);
        f.i32(1);
        f.op(Instruction::I32Add);
        f.global_set(Global::AllPolls);
    }
    f.get(child);
    f.get(child);
    f.op(Instruction::I32Load(mem(HEAP, F_POLL, 2)));
    f.call_indirect(
        vec![Val::I32],
        [vec![Val::I32], elem.leaves.iter().map(|l| l.val).collect()].concat(),
    );
    for local in tmp.iter().rev() {
        f.set(*local);
    }
    f.set(ready);
    f.get(ready);
    f.op(Instruction::If(BlockType::Empty));
    all_field(&mut f, 0, ALL_RESULTS);
    f.get(index);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.u32(crate::rt::LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.set(result);
    f.store_locals(result, &elem.leaves, tmp.iter().copied());
    all_done(&mut f, index, 1);
    f.op(Instruction::Else);
    flags_any(&mut f, child, CANCELLED);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.i32(3);
    f.op(Instruction::I32Store(mem(HEAP, F_STATE, 2)));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::Else);
    f.get(0);
    f.get(index);
    f.helper(Helper::All {
        part: 4,
        elem: elem.clone(),
    });
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
    f.i32(3);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f
}

fn all_root(f: &mut Func, elem: &Elem) {
    let count = f.local(Val::I32);
    all_field(f, 0, ALL_COUNT);
    f.set(count);
    f.get(count);
    f.op(Instruction::If(BlockType::Empty));
    f.get(count);
    f.i32(1);
    f.op(Instruction::I32Sub);
    let root = f.local(Val::I32);
    f.set(root);
    all_node(f, 0, root);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.helper(Helper::WakerParent);
    f.get(0);
    f.get(root);
    f.helper(Helper::All {
        part: 4,
        elem: elem.clone(),
    });
    f.op(Instruction::End);
}

fn all_poll(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:all_poll", vec![Val::I32], vec![Val::I32, Val::I32]);
    let (parent, parent_bit, buffer) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.helper(Helper::PollCheck);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.global_get(Global::Waker);
    f.set(parent);
    f.global_get(Global::WakeBit);
    f.set(parent_bit);
    flags_or(&mut f, 0, ACTIVE);
    f.get(0);
    f.helper(Helper::All {
        part: 5,
        elem: elem.clone(),
    });
    all_root(&mut f, elem);
    f.get(parent);
    f.global_set(Global::Waker);
    f.get(parent_bit);
    f.global_set(Global::WakeBit);
    f.get(0);
    all_field(&mut f, 0, F_FLAGS);
    f.i32(!ACTIVE);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
    f.i32(3);
    f.op(Instruction::I32Eq);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.helper(Helper::All {
        part: 2,
        elem: elem.clone(),
    });
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    all_field(&mut f, 0, ALL_LEFT);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    flags_or(&mut f, 0, DONE);
    all_field(&mut f, 0, ALL_RESULTS);
    f.set(buffer);
    f.get(buffer);
    all_field(&mut f, 0, ALL_LEN);
    f.op(Instruction::I32Store(mem(HEAP, crate::rt::LIST_LEN, 2)));
    f.get(0);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, ALL_RESULTS, 2)));
    f.i32(1);
    f.get(buffer);
    f
}

fn all_cancel(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:all_cancel", vec![Val::I32], vec![]);
    let (parent, parent_bit, n, initial, wait, seen) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(0);
    f.helper(Helper::CancelCheck);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.helper(Helper::CancelEnter);
    f.set(seen);
    f.global_get(Global::Waker);
    f.set(parent);
    f.global_get(Global::WakeBit);
    f.set(parent_bit);
    flags_or(&mut f, 0, ACTIVE);
    f.get(0);
    f.helper(Helper::All {
        part: 5,
        elem: elem.clone(),
    });
    all_field(&mut f, 0, ALL_COUNT);
    let count = f.local(Val::I32);
    f.set(count);
    f.get(count);
    f.op(Instruction::If(BlockType::Empty));
    f.get(count);
    f.i32(1);
    f.op(Instruction::I32Sub);
    let root = f.local(Val::I32);
    f.set(root);
    all_node(&mut f, 0, root);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.helper(Helper::WakerParent);
    f.op(Instruction::End);
    all_field(&mut f, 0, F_STATE);
    f.i32(2);
    f.op(Instruction::I32Ne);
    f.global_get(Global::Aband);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32Or);
    f.set(initial);
    f.get(0);
    f.i32(2);
    f.op(Instruction::I32Store(mem(HEAP, F_STATE, 2)));
    f.get(initial);
    f.op(Instruction::If(BlockType::Empty));
    all_field(&mut f, 0, ALL_LEN);
    f.set(n);
    all_for(&mut f, n, |f, index| {
        all_cell(f, 0, index);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.op(Instruction::I32Eqz);
        f.op(Instruction::If(BlockType::Empty));
        // Leaf groups are the first ceil(n/31) metadata records.
        f.get(index);
        f.i32(31);
        f.op(Instruction::I32DivU);
        let leaf = f.local(Val::I32);
        f.set(leaf);
        all_node(f, 0, leaf);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.global_set(Global::Waker);
        f.i32(1);
        f.get(index);
        f.i32(31);
        f.op(Instruction::I32RemU);
        f.op(Instruction::I32Shl);
        f.global_set(Global::WakeBit);
        all_cancel_child(f, index);
        f.op(Instruction::End);
    });
    f.op(Instruction::Else);
    all_root(&mut f, elem);
    f.op(Instruction::End);
    f.get(parent);
    f.global_set(Global::Waker);
    f.get(parent_bit);
    f.global_set(Global::WakeBit);
    f.get(0);
    all_field(&mut f, 0, F_FLAGS);
    f.i32(!ACTIVE);
    f.op(Instruction::I32And);
    f.op(Instruction::I32Store(mem(HEAP, F_FLAGS, 2)));
    all_field(&mut f, 0, ALL_LEFT);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.set(wait);
    mark_cancelled(&mut f, wait);
    f.get(0);
    f.get(seen);
    f.helper(Helper::CancelLeave);
    f
}

fn all_drop(m: &Module, elem: &Elem) -> Func {
    let mut f = m.func("rt:all_drop", vec![Val::I32], vec![]);
    let (n, count, buffer, address) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    all_field(&mut f, 0, ALL_LEN);
    f.set(n);
    all_field(&mut f, 0, ALL_RESULTS);
    f.set(buffer);
    if elem.counted.counts() {
        f.get(buffer);
        f.op(Instruction::If(BlockType::Empty));
        all_for(&mut f, n, |f, index| {
            all_cell(f, 0, index);
            f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
            f.i32(1);
            f.op(Instruction::I32Eq);
            f.op(Instruction::If(BlockType::Empty));
            f.get(buffer);
            f.get(index);
            f.u32(elem.size);
            f.op(Instruction::I32Mul);
            f.op(Instruction::I32Add);
            f.u32(crate::rt::LIST_ELEMS);
            f.op(Instruction::I32Add);
            f.set(address);
            let values = f.load_locals(address, &elem.leaves);
            let off = elem.leaves.first().map_or(0, |l| l.offset) - elem.counted.first_offset();
            elem.counted.emit_at(
                f,
                m,
                &|f| {
                    f.get(address);
                    if off != 0 {
                        f.u32(off);
                        f.op(Instruction::I32Add);
                    }
                },
                &|f, k| f.get(values[k as usize]),
                false,
            );
            f.op(Instruction::End);
        });
        f.op(Instruction::End);
    }
    f.get(buffer);
    f.helper(Helper::ReleaseRef);
    all_field(&mut f, 0, ALL_TASKS);
    f.helper(Helper::ReleaseRef);
    all_field(&mut f, 0, ALL_COUNT);
    f.set(count);
    all_for(&mut f, count, |f, index| {
        all_node(f, 0, index);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.helper(Helper::ReleaseRef);
    });
    f
}

/// The list a race frame holds: its buffer's address at its first field.
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

/// [`Helper::Race`] part 0: `(list) -> frame`, `race!`'s frame over its
/// list of tasks, which it holds with a count of its own.
fn race_cold(m: &Module, results: &[Val], counted: &crate::rt::Counted) -> Func {
    let (_, size) = race_area(results);
    let mut f = m.func("rt:race_cold", vec![Val::I32], vec![Val::I32]);
    let drop = Sym::Helper(Helper::RaceDrop {
        results: results.to_vec(),
        counted: counted.clone(),
    });
    let fr = new_zeroed(&mut f, size, Some(drop), true);
    let h = |k| Helper::Race {
        part: k,
        results: results.to_vec(),
        counted: counted.clone(),
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

/// [`Helper::Race`] part 1, the poll (`crates/hd_wasm/DESIGN.md` §14.5):
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
fn race_drop(m: &Module, results: &[Val], counted: &crate::rt::Counted) -> Func {
    let (area, _) = race_area(results);
    let mut f = m.func("rt:race_drop", vec![Val::I32], vec![]);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, u64::from(RACE_LIST), 2)));
    f.helper(Helper::ReleaseRef);
    if counted.counts() {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, F_STATE, 2)));
        flags_any(&mut f, 0, DONE);
        f.op(Instruction::I32Eqz);
        f.op(Instruction::I32And);
        f.op(Instruction::If(BlockType::Empty));
        let ls: Vec<u32> = area.iter().map(|(_, v)| f.local(*v)).collect();
        for ((at, v), l) in area.iter().zip(&ls) {
            f.get(0);
            f.op(load(*v, *at));
            f.set(*l);
        }
        counted.emit(&mut f, m, &|f, k| f.get(ls[k as usize]), false);
        f.op(Instruction::End);
    }
    f
}
