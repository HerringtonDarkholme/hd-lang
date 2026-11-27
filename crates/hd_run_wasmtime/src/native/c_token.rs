//! A C provider's suspending calls on Wasmtime (#2388 step 5,
//! `docs/future-work/FFI.md`, "Completions"). A `!` method's slot takes
//! `(self, token, args…)` and returns at once; the library completes the
//! token later, from any thread, through the `hd_completions` table the
//! token points to (`cli.host.c.thread`). The call rides the native
//! provider's pending-call machinery ([`super::pending_discarding`]): its
//! `.start` registers the call with the instance's reactor, a completion
//! wakes that reactor, and its `.finish` takes the value.
//!
//! # A token's lifetime
//!
//! A token is one [`Call`], shared by two references: the library's,
//! which the token pointer is, and the host's, which the call's abort
//! hook holds. The call's slot (`super::State`) and the library's flag
//! [`Call::library_holds`] together are its state:
//!
//! | State | Slot | Library holds | Left by |
//! | --- | --- | --- | --- |
//! | issued | `Waiting` | yes | the library's completion, accepted (to completed); a cancel (to aborting) |
//! | completed | `Done` | no | the guest's `.finish` (to taken); a cancel (to aborting, `completed` true) |
//! | aborting | `Ended` | until its completion or the abort slot returns | the abort slot's return (to dead) |
//! | taken | `Ended` | no | none: the host's reference drops, and the token is freed |
//! | dead | `Ended` | no | none: both references are gone, and the token is freed |
//!
//! - A completion in issued moves it to completed, wakes the reactor and
//!   returns true; it releases the library's reference as its last step
//!   (`cli.host.c.complete`). A completion in aborting, as one racing a
//!   cancel, is refused: it returns false, and the library keeps its value
//!   (decision 18). Each call is decided once, under the slot's lock: an
//!   accepted completion the guest takes, an abort with `completed` true,
//!   or a refused completion.
//! - A cancel before the guest takes the value runs the abort slot once,
//!   with `completed` true when the slot held an accepted value
//!   (`cli.host.c.abort`). After the slot returns, the host releases the
//!   library's reference if no completion has, and the token is dead.
//!   Closing the instance cancels every call it holds; the reactor drops
//!   before the providers and handles, so an abort slot's `self` is still
//!   alive.
//! - The guest taking the value drops the hook unrun: no abort follows.
//!
//! A library completes a token at most once, and uses it only until that
//! completion returns or its abort slot returns (`cli.host.c.token`,
//! decision 19); a second completion may find the token freed.
//!
//! Tokio's task state (`tokio/src/runtime/task/state.rs`) is the model:
//! one reference per party, the last release frees, and the lifecycle
//! moves in one atomic transition, so a racing party learns the outcome
//! from it, as `transition_to_join_handle_dropped` tells the `JoinHandle`
//! from `COMPLETE` whether it must drop the output. hd differs in two
//! ways. Tokio packs the count and the flags into one `AtomicUsize` with
//! compare-and-swap; hd keeps the lifecycle in the pending call's slot,
//! under the mutex every native pending call already uses, because the
//! wake must be sent under the same lock, and keeps the two references
//! as an `Arc` plus a flag for the library's: one lock per completion
//! and per cancel where Tokio makes one compare-and-swap, not measured.
//! Tokio starts at three references (owned list, scheduler,
//! `JoinHandle`); a token has two.
//!
//! Cost per call: four fixed-size heap blocks, whatever the arguments:
//! the token ([`Call`]), the slot it shares with the pending call, the
//! abort hook and the reactor's boxed [`Suspension`], plus the reactor's
//! one map entry. A completion encodes its value once, on the library's
//! thread: a `List[u8]`'s bytes are copied there, before
//! `hd_complete_bytes` returns (`cli.host.c.bytes`), and again into the
//! exchange buffer at `.finish`. A completion takes the slot's lock once
//! and pushes one wake; nothing scans the pending calls.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hd_host_abi::c::{Num, Ret};

use super::c::CTable;
use super::c_trampoline::Trampoline;
use super::{Completion, HostError, HostResult, Suspension};
use crate::codec::Enc;

/// A completion's value: the result's encoding, or why the completion
/// broke the method's contract, which the `.finish` reports as a
/// `host-contract` failure of the call.
type Value = HostResult<Vec<u8>>;

/// What a completion must hold, from the method's C result row
/// (`module.host.package.c-suspend`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Expect {
    value: Want,
    /// Whether the result is `Result[_, i32]`, which `hd_complete_error`
    /// also completes.
    result: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    Num(Num),
    Bytes,
}

impl Expect {
    /// The completion a `!` method of result `ret` takes, or `None` for a
    /// row no suspending method has.
    #[must_use]
    pub(crate) fn of(ret: &Ret) -> Option<Self> {
        let want = |ret: &Ret| match ret {
            Ret::Num(num) => Some(Want::Num(*num)),
            Ret::Bytes => Some(Want::Bytes),
            _ => None,
        };
        match ret {
            Ret::Result(value) => Some(Self {
                value: want(value)?,
                result: true,
            }),
            other => Some(Self {
                value: want(other)?,
                result: false,
            }),
        }
    }
}

/// A completion as the library made it.
#[derive(Clone, Copy, Debug)]
enum Got<'a> {
    I32(i32),
    I64(i64),
    F64(f64),
    Bytes(&'a [u8]),
    Error(i32),
}

impl Got<'_> {
    fn name(self) -> &'static str {
        match self {
            Got::I32(_) => "hd_complete_i32",
            Got::I64(_) => "hd_complete_i64",
            Got::F64(_) => "hd_complete_f64",
            Got::Bytes(_) => "hd_complete_bytes",
            Got::Error(_) => "hd_complete_error",
        }
    }
}

/// A completion of the wrong kind is still accepted: it decides the call,
/// which fails at `.finish` with `host-contract` (`cli.host.c.complete-kind`).
///
/// The exchange-buffer encoding of `got` as the result `expect`, as the
/// guest decodes it: a `Result` is its variant, then `Ok`'s value or the
/// error. A 32-bit or narrower integer takes `hd_complete_i32`, cut to its
/// width as C converts it; a 64-bit one `hd_complete_i64`; a float
/// `hd_complete_f64`, rounded to an `f32` for one.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode(expect: Expect, got: Got<'_>) -> Value {
    let mut e = Enc::default();
    if let Got::Error(code) = got {
        if !expect.result {
            return Err(HostError(
                "the C provider completed with hd_complete_error, but the method's result is not a `Result`".into(),
            ));
        }
        e.leb(1).zz(i64::from(code));
        return Ok(e.into_bytes());
    }
    if expect.result {
        e.leb(0);
    }
    match (expect.value, got) {
        (Want::Num(Num::I8), Got::I32(v)) => e.zz(i64::from(v as i8)),
        (Want::Num(Num::I16), Got::I32(v)) => e.zz(i64::from(v as i16)),
        (Want::Num(Num::I32), Got::I32(v)) => e.zz(i64::from(v)),
        (Want::Num(Num::U8), Got::I32(v)) => e.leb(u64::from(v as u8)),
        (Want::Num(Num::U16), Got::I32(v)) => e.leb(u64::from(v as u16)),
        (Want::Num(Num::U32 | Num::Usize), Got::I32(v)) => e.leb(u64::from(v.cast_unsigned())),
        (Want::Num(Num::I64), Got::I64(v)) => e.zz(v),
        (Want::Num(Num::U64), Got::I64(v)) => e.leb(v.cast_unsigned()),
        (Want::Num(Num::F32), Got::F64(v)) => e.f32(v as f32),
        (Want::Num(Num::F64), Got::F64(v)) => e.f64(v),
        (Want::Bytes, Got::Bytes(bytes)) => e.bytes(bytes),
        (want, got) => {
            // The hd name of the number type: its variant's, lowercased.
            let want = match want {
                Want::Num(num) => format!("{num:?}").to_lowercase(),
                Want::Bytes => "List[u8]".to_owned(),
            };
            return Err(HostError(format!(
                "the C provider completed a call of `{want}` with {}",
                got.name()
            )));
        }
    };
    Ok(e.into_bytes())
}

/// The host's completion table, which every token points to first: the
/// C header's `hd_completions`, in its field order.
#[repr(C)]
#[allow(clippy::struct_field_names, reason = "the C header's field names")]
struct Completions {
    complete_i32: unsafe extern "C" fn(*const Call, i32) -> bool,
    complete_i64: unsafe extern "C" fn(*const Call, i64) -> bool,
    complete_f64: unsafe extern "C" fn(*const Call, f64) -> bool,
    complete_bytes: unsafe extern "C" fn(*const Call, *const u8, usize) -> bool,
    complete_error: unsafe extern "C" fn(*const Call, i32) -> bool,
}

static COMPLETIONS: Completions = Completions {
    complete_i32,
    complete_i64,
    complete_f64,
    complete_bytes,
    complete_error,
};

/// One suspending call's token: the C header's `struct hd_call`, whose
/// first field is the completion table, then the host's own state.
#[repr(C)]
pub(crate) struct Call {
    completions: &'static Completions,
    expect: Expect,
    completion: Completion<Value>,
    /// Whether the library's reference, the token pointer, is still held:
    /// the first completion or the abort slot's return releases it, once.
    library_holds: AtomicBool,
}

#[cfg(feature = "testing")]
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many tokens are alive, for the tests that check each is freed.
#[cfg(feature = "testing")]
#[must_use]
pub fn live() -> usize {
    LIVE.load(Ordering::SeqCst)
}

#[cfg(feature = "testing")]
impl Drop for Call {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Delivers `got` to the call of `token`, then releases the library's
/// reference, the last use of the token by this thread; whether the call
/// accepted it: false once it is aborting or completed (decision 18).
///
/// # Safety
///
/// `token` is a token this host issued, still alive: its library has
/// neither completed it before nor seen its abort slot return
/// (`cli.host.c.token`).
#[allow(unsafe_code)]
unsafe fn complete(token: *const Call, got: Got<'_>) -> bool {
    // SAFETY: the caller's contract; a token is never null.
    let call = unsafe { &*token };
    let accepted = call.completion.deliver(Ok(encode(call.expect, got)));
    // SAFETY: as above.
    unsafe { release_library(token) };
    accepted
}

/// Releases the library's reference to `token`, unless it was released.
///
/// # Safety
///
/// As [`complete`].
#[allow(unsafe_code)]
unsafe fn release_library(token: *const Call) {
    // SAFETY: the caller's contract.
    if unsafe { (*token).library_holds.swap(false, Ordering::AcqRel) } {
        // SAFETY: the token pointer is the library's `Arc` reference, made
        // by `Arc::into_raw` in `start`, and the swap releases it once.
        drop(unsafe { Arc::from_raw(token) });
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn complete_i32(token: *const Call, value: i32) -> bool {
    // SAFETY: a library calls a completion with a live token.
    unsafe { complete(token, Got::I32(value)) }
}

#[allow(unsafe_code)]
unsafe extern "C" fn complete_i64(token: *const Call, value: i64) -> bool {
    // SAFETY: as `complete_i32`.
    unsafe { complete(token, Got::I64(value)) }
}

#[allow(unsafe_code)]
unsafe extern "C" fn complete_f64(token: *const Call, value: f64) -> bool {
    // SAFETY: as `complete_i32`.
    unsafe { complete(token, Got::F64(value)) }
}

#[allow(unsafe_code)]
unsafe extern "C" fn complete_bytes(token: *const Call, bytes: *const u8, len: usize) -> bool {
    let bytes = if len == 0 {
        &[][..]
    } else {
        // SAFETY: the header's contract: `bytes` points to `len` bytes
        // for this call, which `encode` copies before it returns.
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    // SAFETY: as `complete_i32`.
    unsafe { complete(token, Got::Bytes(bytes)) }
}

#[allow(unsafe_code)]
unsafe extern "C" fn complete_error(token: *const Call, error: i32) -> bool {
    // SAFETY: as `complete_i32`.
    unsafe { complete(token, Got::Error(error)) }
}

/// A `!` method's slots on one `self`, the trampoline of its abort slot,
/// `(self, token, completed)`, and the table that keeps their library
/// loaded while a call is pending.
pub(crate) struct Slots {
    pub(crate) this: *mut c_void,
    pub(crate) call: *const c_void,
    pub(crate) abort: *const c_void,
    pub(crate) abort_trampoline: Arc<Trampoline>,
    pub(crate) keep: Option<Arc<CTable>>,
}

/// The host's reference to a pending call, held by its abort hook: run
/// at a cancel, dropped unrun when the guest takes the value.
struct Abort {
    _call: Arc<Call>,
    token: *const Call,
    this: *mut c_void,
    slot: *const c_void,
    trampoline: Arc<Trampoline>,
    _keep: Option<Arc<CTable>>,
}

impl Abort {
    /// Runs the abort slot once, then releases the library's reference
    /// if no completion did; the host's goes as `self` drops.
    #[allow(unsafe_code)]
    fn run(self, completed: bool) {
        let args = [self.this as u64, self.token as u64, u64::from(completed)];
        // SAFETY: `slot` is the method's abort slot from a table whose hash
        // matched, `void (*)(void *self, hd_token, bool)`, the signature of
        // `trampoline`, and its `self` lives: the reactor, which holds
        // every pending call, drops before the instance's providers and
        // handles.
        unsafe { self.trampoline.call(self.slot, &args) };
        // SAFETY: the host's own reference keeps the token alive here.
        unsafe { release_library(self.token) };
    }
}

/// Starts a `!` method's call: issues its token, calls the slot with
/// `(self, token, args…)`, where `args` holds `self` first and the token
/// goes second, and returns the pending call for the reactor. A cancel
/// before the guest takes the value runs the abort slot once.
///
/// # Safety
///
/// `slots.call` is a `!` method's slot of `trampoline`'s signature
/// (`self`, the token, then the method's own), `slots.abort` its abort
/// slot, and `slots.this` stays alive until the call is taken or aborted.
#[allow(unsafe_code)]
pub(crate) unsafe fn start(
    slots: Slots,
    trampoline: &Trampoline,
    mut args: Vec<u64>,
    expect: Expect,
) -> Suspension {
    // The hook is set once the token exists, which holds the completion.
    let (mut pending, completion) = super::pending_discarding::<Value>(|_| {});
    let call = Arc::new(Call {
        completions: &COMPLETIONS,
        expect,
        completion,
        library_holds: AtomicBool::new(true),
    });
    #[cfg(feature = "testing")]
    LIVE.fetch_add(1, Ordering::SeqCst);
    let token = Arc::into_raw(Arc::clone(&call));
    let hook = Abort {
        _call: call,
        token,
        this: slots.this,
        slot: slots.abort,
        trampoline: slots.abort_trampoline,
        _keep: slots.keep,
    };
    pending.abort = Some(Box::new(move |completed| hook.run(completed)));
    args.insert(1, token as u64);
    // SAFETY: the caller's contract; the slot returns at once and keeps
    // the token, which stays alive until its completion or abort.
    unsafe { trampoline.call(slots.call, &args) };
    let encode: super::Encode<Value> = |value, _| value;
    Suspension::new(pending, encode)
}
