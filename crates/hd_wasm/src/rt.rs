//! Runtime helpers the compiler builds (`crates/hd_run_wasmtime/DESIGN.md` §16.1,
//! "generated"): literal getters, number formatting, string equality, the
//! exchange-buffer copy, panic stubs, the default profile's host provider
//! stubs (from `hd_host_abi::TABLE`), vtable adapters, the list cursor and
//! the entry wrapper. Each is keyed by what it depends on, so link builds
//! it once per program.

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, StageResult};
use wasm_encoder::BlockType;

use crate::asm::Asm;
use crate::boundary::BTy;
use crate::layout::{
    ABANDONED, ACTIVE, CANCELLED, DONE, F_CANCEL, F_CHILD, F_FLAGS, F_POLL, F_SAVED, F_STATE,
    UNWINDING, cancel_fn, ctx_keys, ctx_provs, storage, task_base,
};
use crate::{Code, GSym, Sym, VT, WTy, decode_vts, encode_vts, unsupported};

/// How a host method's argument crosses (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArgCodec {
    /// A string: its viewed bytes into the exchange buffer, then the length.
    Str,
    /// A data value of one `i64` field (`Duration`): the field, as a scalar.
    DataI64(WTy),
    /// A scalar, as itself.
    Scalar(VT),
    /// A structured value, encoded into the exchange buffer after the
    /// arguments before it (`crates/hd_run_wasmtime/DESIGN.md` §17.4).
    Buf(BTy),
}

impl ArgCodec {
    /// The argument's values in the frame.
    #[must_use]
    pub fn vts(&self) -> Vec<VT> {
        match self {
            ArgCodec::Str => vec![VT::r(WTy::Bytes), VT::I64],
            ArgCodec::DataI64(t) => vec![VT::r(t.clone())],
            ArgCodec::Scalar(v) => vec![v.clone()],
            ArgCodec::Buf(b) => b.vts(),
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            ArgCodec::Str => w.u8(0),
            ArgCodec::DataI64(t) => {
                w.u8(1);
                enc_wty(t, w);
            }
            ArgCodec::Scalar(v) => {
                w.u8(2);
                encode_vts(std::slice::from_ref(v), w);
            }
            ArgCodec::Buf(b) => {
                w.u8(3);
                b.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<ArgCodec> {
        Some(match r.u8() {
            0 => ArgCodec::Str,
            1 => ArgCodec::DataI64(dec_wty(r)?),
            2 => ArgCodec::Scalar(decode_vts(r)?.pop()?),
            3 => ArgCodec::Buf(BTy::decode(r)?),
            _ => return None,
        })
    }
}

/// How a host method's result crosses (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResCodec {
    Void,
    /// A scalar, as the import's result.
    Scalar(VT),
    /// A data value of one `i64` field (`Timestamp`), as the import's
    /// `i64` result.
    DataI64(WTy),
    /// An enum of tags (`BTy::is_tags`), read as one or two bytes.
    Tags,
    /// A structured value in the exchange buffer.
    Buf(BTy),
}

impl ResCodec {
    /// The result's values.
    #[must_use]
    pub fn vts(&self) -> Vec<VT> {
        match self {
            ResCodec::Void => vec![],
            ResCodec::Scalar(v) => vec![v.clone()],
            ResCodec::DataI64(t) => vec![VT::r(t.clone())],
            ResCodec::Tags => vec![VT::I32, VT::I32],
            ResCodec::Buf(b) => b.vts(),
        }
    }
    /// The import's results, for a method that never waits.
    fn import(&self) -> Vec<VT> {
        match self {
            ResCodec::Void => vec![],
            ResCodec::Scalar(v) => vec![v.clone()],
            ResCodec::DataI64(_) => vec![VT::I64],
            ResCodec::Tags | ResCodec::Buf(_) => vec![VT::I32],
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            ResCodec::Void => w.u8(0),
            ResCodec::Scalar(v) => {
                w.u8(1);
                encode_vts(std::slice::from_ref(v), w);
            }
            ResCodec::DataI64(t) => {
                w.u8(2);
                enc_wty(t, w);
            }
            ResCodec::Tags => w.u8(3),
            ResCodec::Buf(b) => {
                w.u8(4);
                b.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<ResCodec> {
        Some(match r.u8() {
            0 => ResCodec::Void,
            1 => ResCodec::Scalar(decode_vts(r)?.pop()?),
            2 => ResCodec::DataI64(dec_wty(r)?),
            3 => ResCodec::Tags,
            4 => ResCodec::Buf(BTy::decode(r)?),
            _ => return None,
        })
    }
}

/// The wake table (`crates/hd_wasm/DESIGN.md` §14.4): completed host handles by slot.
#[must_use]
pub fn wake_table() -> GSym {
    GSym::Rt("wake".into(), VT::rn(WTy::Array(VT::I32)))
}

/// A waker node (`crates/hd_wasm/DESIGN.md` §14.4): an `all!`'s wake mask,
/// its parent node (`eqref`, null at the top) and its slot there. A
/// host leaf's registration has the same shape: its full handle, the node
/// it sits under, and its slot.
#[must_use]
pub fn wake_node() -> WTy {
    WTy::Struct {
        fields: vec![VT::I32, VT::Eq, VT::I32],
        sup: None,
        open: false,
    }
}

/// The node and slot that a host leaf polled now sits under: set by
/// `all!` around each child's poll, cleared by `block_on`.
#[must_use]
pub fn wake_cur_node() -> GSym {
    GSym::Rt("wnode".into(), VT::Eq)
}

/// The slot of [`wake_cur_node`].
#[must_use]
pub fn wake_cur_slot() -> GSym {
    GSym::Rt("wslot".into(), VT::I32)
}

/// Each pending host handle's registration, by slot: a [`wake_node`]
/// struct `(handle, node, slot)`.
#[must_use]
pub fn wakers_global() -> GSym {
    GSym::Rt("wakers".into(), VT::rn(WTy::Array(VT::Eq)))
}

/// How many host leaves have registered: an `all!` child whose poll
/// registers none has nothing that would wake it, so it stays woken.
#[must_use]
pub fn wake_regs() -> GSym {
    GSym::Rt("wregs".into(), VT::I32)
}

/// How many `all!` children have been polled: a test-only count, exported
/// as `hd.all_polls`.
#[must_use]
pub fn all_polls() -> GSym {
    GSym::Rt("allpolls".into(), VT::I32)
}

/// How many `abandon()` calls are running (`crates/hd_wasm/DESIGN.md`
/// §14.6, "Abandoning"): while it is not zero, every frame a cancel
/// reaches is abandoned, and a shielded `dispose!` is killed at its
/// suspension point instead of waited for.
#[must_use]
pub fn abandoning_global() -> GSym {
    GSym::Rt("aband".into(), VT::I32)
}

/// The `ABANDONED` bits that the cancels a frame's cancel made have
/// seen: each frame's cancel ORs in its own, so an abandoned cleanup
/// carries up through every cleanup that folds it (`req.abandon.carry`).
#[must_use]
pub fn abandon_seen_global() -> GSym {
    GSym::Rt("abseen".into(), VT::I32)
}

/// What `hd.poll` returns for a root that ended cancelled once its
/// cleanup ran to its end (`req.entry.cancelled`); one less, -3, when a
/// part of the cleanup was cut short. -1 is Pending, and an exit status
/// is never negative.
pub const CANCELLED_END: i32 = -2;

/// Set by an `abandon()` that ended a cleanup: pending parties that
/// waited on it registered no wake, so the entry polls its root again
/// (`crates/hd_wasm/DESIGN.md` §14.6, "Abandoning").
#[must_use]
pub fn rewake_global() -> GSym {
    GSym::Rt("rewake".into(), VT::I32)
}

/// The forbidden-context counter (`crates/hd_wasm/DESIGN.md` §14.9): how many forbidden
/// contexts are running; `block_on` panics while it is not zero.
#[must_use]
pub fn forbid_global() -> GSym {
    GSym::Rt("forbid".into(), VT::I32)
}

/// The code of [`Helper::ForbidCheck`]: with `full`, a panic while the
/// forbidden-context counter is not zero; else nothing, for a program in
/// which nothing raises the counter (`crates/hd_wasm/DESIGN.md` §14.9).
#[must_use]
pub fn forbid_check(full: bool) -> Code {
    let mut a = Asm::new(vec![]);
    if full {
        a.global_get(forbid_global());
        a.if_();
        a.call(Sym::Helper(Helper::Panic(
            "suspension-forbidden-context: block_on in a forbidden context".into(),
        )));
        a.s().unreachable();
        a.end();
    }
    a.finish(vec![])
}

/// Adds `by` to the forbidden-context counter.
pub fn forbid_by(a: &mut Asm, by: i32) {
    a.global_get(forbid_global());
    a.i32(by);
    a.s().i32_add();
    a.global_set(forbid_global());
}

/// The entry's pending root suspension, for `main!` or a test body.
/// Completion releases the root so a retained test instance can run again.
#[must_use]
pub fn root_global() -> GSym {
    GSym::Rt("root".into(), VT::rn(crate::layout::task_base()))
}

/// The driver that owns the pending root, as its `hd.poll` context plus
/// one; zero is unowned. The first poll claims the root; a live root
/// answers only its owner, so a second driver's poll reports
/// `suspension-competing-driver`.
#[must_use]
pub fn owner_global() -> GSym {
    GSym::Rt("owner".into(), VT::I32)
}

/// The run's map hash key (`map_hash_seed`): 0 until the first map key
/// is hashed, which asks the host's `hash_seed` import. It is a
/// runtime global, so the inputs of a session share one key and a map
/// built by one input is found by the next.
#[must_use]
pub fn hash_seed_global() -> GSym {
    GSym::Rt("hseed".into(), VT::I64)
}

/// The last serial number that reference identity drew (`ref_serial`,
/// `crates/hd_wasm/DESIGN.md` §15.2 "References"): 0 until the first. A
/// runtime global, so the inputs of a session share one sequence and a
/// reference one input keeps never meets an equal serial in the next.
#[must_use]
pub fn ref_serial_global() -> GSym {
    GSym::Rt("refserial".into(), VT::I64)
}

/// The host runtime's `hash_seed` import: 64 random bits, drawn once per
/// run (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[must_use]
pub fn hash_seed_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "hash_seed".into(),
        params: vec![],
        results: vec![VT::I64],
    }
}

/// Every runtime global: what a session's first module defines for the
/// modules after it (`LinkMode::Session`).
#[must_use]
pub fn runtime_globals() -> [GSym; 14] {
    [
        wake_table(),
        forbid_global(),
        abandoning_global(),
        abandon_seen_global(),
        rewake_global(),
        root_global(),
        owner_global(),
        wake_cur_node(),
        wake_cur_slot(),
        wakers_global(),
        wake_regs(),
        all_polls(),
        hash_seed_global(),
        ref_serial_global(),
    ]
}

/// The host runtime's `abort` import (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[must_use]
pub fn abort_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "abort".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// How a `T?` is built (`layout::OptShape` without its payload values).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OptForm {
    NullRef,
    NullStr,
    Tagged,
    Boxed(WTy),
}

/// How a map hashes and compares its key (`crates/hd_mono/DESIGN.md` §13.12).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyOps {
    /// An integer, `bool` or `char` in one `i32`: a call of the keyed
    /// bucket hash `std.hash.int_bucket_hash` over its bits, zero-extended
    /// to a `u64`; equality is `i32.eq`.
    I32(Box<Sym>),
    /// A 64-bit integer: a call of `std.hash.int_bucket_hash`; equality
    /// is `i64.eq`.
    I64(Box<Sym>),
    /// A string: a call of the keyed bucket hash
    /// `std.hash.string_bucket_hash` over its viewed bytes; equality is
    /// `StrEq`.
    Str(Box<Sym>),
    /// Calls of the key type's `key_bucket_hash` and `Eq.eq` instances,
    /// which take the key's values as `params`.
    Call {
        hash: Box<Sym>,
        eq: Box<Sym>,
        params: Vec<VT>,
    },
}

/// How a vtable adapter reads the value a reference payload refers to
/// (`crates/hd_wasm/DESIGN.md` §15.2, "Trait value references"): the
/// `std.core.Ref[T]` struct `data`, its `get` closure's field and value
/// type, and that closure's base and code types, which a call casts to
/// when `cast` (an abstract code reference); the call returns `results`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefRead {
    pub data: WTy,
    pub field: u32,
    pub field_vt: VT,
    pub base: WTy,
    pub code: WTy,
    pub cast: bool,
    pub results: Vec<VT>,
}

impl RefRead {
    fn encode(&self, w: &mut Writer) {
        enc_wty(&self.data, w);
        w.u32(self.field);
        encode_vts(std::slice::from_ref(&self.field_vt), w);
        enc_wty(&self.base, w);
        enc_wty(&self.code, w);
        w.u8(u8::from(self.cast));
        encode_vts(&self.results, w);
    }

    fn decode(r: &mut Reader<'_>) -> Option<Self> {
        Some(RefRead {
            data: dec_wty(r)?,
            field: r.u32(),
            field_vt: decode_vts(r)?.into_iter().next()?,
            base: dec_wty(r)?,
            code: dec_wty(r)?,
            cast: r.u8() != 0,
            results: decode_vts(r)?,
        })
    }

    /// Replaces the reference on top of the stack with the values its
    /// `get` closure returns.
    pub(crate) fn emit(&self, a: &mut Asm) {
        a.struct_get(&self.data, self.field);
        a.conv(&self.field_vt, &VT::r(self.base.clone()));
        let f = a.local(VT::r(self.base.clone()));
        a.set(f);
        a.get(f);
        a.ref_null(&ctx_keys());
        a.ref_null(&ctx_provs());
        a.get(f);
        a.struct_get(&self.base, 0);
        if self.cast {
            a.ref_cast(&self.code, false);
        }
        a.call_ref(&self.code);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// The getter of the module's pooled string literals, by number
    /// (`crates/hd_wasm/DESIGN.md` §15.4): a call site pushes the number
    /// as a [`Reloc::Lit`](crate::Reloc::Lit) immediate.
    Lit,
    /// `Display.to_string` of an integer: a view over a fresh buffer.
    IntToStr { wide: bool, signed: bool },
    /// String `==` over the viewed bytes.
    StrEq,
    /// Copies a string's viewed bytes to the exchange buffer; its length.
    StrToBuf,
    /// A panic stub with a fixed message (`crates/hd_wasm/DESIGN.md` §15.5).
    Panic(String),
    /// `panic(message)`.
    PanicStr,
    /// The cursor closure of `List.iter()`.
    ListStep {
        code: WTy,
        env: WTy,
        list: WTy,
        elem: Vec<VT>,
        opt: OptForm,
    },
    /// `(key...) -> i32`: a map key's bucket hash, 31 bits.
    MapHash { key: Vec<VT>, ops: KeyOps },
    /// `(map, hash, key...) -> i32`: the key's live entry, or -1.
    MapFind { map: WTy, key: Vec<VT>, ops: KeyOps },
    /// `(map, hash, key..., value...)`: replaces the value of the key's
    /// entry, or appends a new entry.
    MapPut {
        map: WTy,
        key: Vec<VT>,
        val: Vec<VT>,
        ops: KeyOps,
    },
    /// `(map)`: makes room for one more entry: drops removed entries,
    /// doubling the capacity when at least half are live, and rebuilds
    /// the index; entry order is kept.
    MapGrow { map: WTy, comps: Vec<VT> },
    /// The cursor closure of `Map.iter()`.
    MapStep {
        /// The code type, `(env, ctx) -> (dflt(T?)...)`.
        code: WTy,
        /// The environment `{code, map, pos, end, len}`, which names the
        /// map type.
        env: WTy,
        /// The key's values, then the value's.
        comps: Vec<VT>,
        /// The `(K, V)` box, when the tuple is boxed.
        tuple: Option<WTy>,
        opt: OptForm,
    },
    /// A host method that may wait, called cold: its leaf frame.
    HostCold {
        sig: WTy,
        frame: WTy,
        poll: Box<Helper>,
        cancel: Box<Helper>,
    },
    /// The leaf frame's poll: `.start`; after a wait, `.finish` once the
    /// wake table holds the handle.
    HostPoll {
        frame: WTy,
        module: String,
        method: String,
        args: Vec<ArgCodec>,
        /// `Void`, `Tags` or `Buf`: a waiting method's result is in the
        /// exchange buffer.
        result: ResCodec,
    },
    /// A host method that never waits: one import call
    /// (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
    HostCall {
        sig: WTy,
        module: String,
        method: String,
        args: Vec<ArgCodec>,
        result: ResCodec,
    },
    /// A host primitive of std (`hd:prim`, `format_f64` and its kin): one
    /// import call that never waits. Unlike a provider's method it has no
    /// receiver, so `sig` holds the arguments alone.
    HostPrim {
        sig: WTy,
        name: String,
        args: Vec<ArgCodec>,
        result: ResCodec,
    },
    /// The encoder of a boundary type (`boundary::enc_code`).
    Enc(BTy),
    /// The decoder of a boundary type (`boundary::dec_code`).
    Dec(BTy),
    /// `(off, v: i64) -> off'`: unsigned LEB128.
    LebPut,
    /// `(off) -> (i64, off')`: unsigned LEB128.
    LebGet,
    /// `(x: f64, y: f64) -> f64`: IEEE 754 `pow` (`expr.power.float.pow`).
    PowF64,
    /// `(x: f64, y: f64) -> f64`: floating `%`, C `fmod`
    /// (`expr.float.remainder-truncated`).
    RemF64,
    /// `(end)`: grows the exchange buffer to `end` bytes.
    Fit,
    /// The leaf frame's cancel: aborts a pending operation.
    HostCancel { frame: WTy },
    /// A slot the compiler does not lower yet: panics when called.
    Unlowered { sig: WTy, what: String },
    /// A vtable slot of a host handle (`boundary::BTy::Handle`): reads the
    /// handle number from the receiver, a `payload` struct, and calls
    /// `inner`, a host call whose first argument after the receiver is
    /// that number.
    HandleSlot {
        sig: WTy,
        payload: WTy,
        inner: Box<Helper>,
    },
    /// `Inspectable.runtime_type`'s slot in a vtable built at a concrete
    /// type (trait.inspect.dynamic): the `TypeId` (`ty`) whose key is
    /// `name` and whose printable name is `shown`, the erased value's
    /// recorded type.
    RuntimeType {
        sig: WTy,
        ty: WTy,
        name: String,
        shown: String,
    },
    /// `() -> (ref ty)`: the `TypeId` (`ty`) whose key is `name` and whose
    /// printable name is `shown`, built on the first call and read from
    /// its global ([`type_id_global`]) after, so no call but the first
    /// allocates (`crates/hd_wasm/DESIGN.md` §15.4).
    TypeIdOf {
        ty: WTy,
        name: String,
        shown: String,
    },
    /// `(i32) -> eqref`: an erased `i32` (§15.2): an `i31ref` when it fits
    /// in 31 signed bits, else its box.
    EraseI32,
    /// `(eqref) -> i32`: what [`Helper::EraseI32`] made, back.
    UneraseI32,
    /// `(n)`: marks the `n` handles in the exchange buffer as completed.
    WakeMark,
    /// `(h) -> i32`: whether handle `h` completed; clears its slot.
    WakeTake,
    /// `block_on`'s indirect ban (`req.drive.block-on.indirect`): panics
    /// while a forbidden context runs. Link makes it empty when no code
    /// of the program raises the forbidden-context counter
    /// ([`forbid_check`]).
    ForbidCheck,
    /// `() -> i32`: opens a frame's cancel (`crates/hd_wasm/DESIGN.md`
    /// §14.6): returns the `abseen` bits of the cancels made so far and
    /// clears them, so the frame sees only its own children's.
    CancelEnter,
    /// `(frame) -> i32`: the checks of a frame's cancel. An active frame
    /// panics; a completed or cancelled one gives 0, unless it unwinds;
    /// else the frame is marked cancelled, and `ABANDONED` too under
    /// `abandon()`, and it gives 1, so the cancel goes on.
    CancelCheck,
    /// `(frame, seen)`: closes a frame's cancel: an `ABANDONED` child cleanup
    /// marks the frame, and `abseen` becomes `seen` with the frame's own bit.
    CancelLeave,
    /// `(frame) -> i32`: the checks of a frame's poll. An active or a
    /// completed frame panics; a cancelled one gives 1, so the poll
    /// answers Pending and its awaiter, which sees the frame `CANCELLED`,
    /// is cancelled (`req.cancel.await`). A cancelled frame that still
    /// unwinds is continued first, as its cancel would.
    PollCheck,
    /// `(h)`: host handle `h` went Pending under the current waker node
    /// and slot (§14.4).
    WakeRegister,
    /// `(node, slot)`: wakes child `slot` of waker node `node` and its
    /// ancestors (§14.4).
    WakeUp,
    /// `(h)`: pending host handle `h` was aborted, so no completion wakes
    /// its awaiter: sets `rewake`, and wakes `h`'s registration as a
    /// completion would (`crates/hd_wasm/DESIGN.md` §14.6, "Awaiting a
    /// cancelled suspension"). Without the `all!` node bookkeeping, only
    /// `rewake`.
    WakeAborted,
    /// `race!`'s frame (std's `task_race_frame` intrinsic): `(list) -> frame`.
    RaceCold {
        list: WTy,
        base: WTy,
        poll: WTy,
        result: Vec<VT>,
    },
    RacePoll {
        list: WTy,
        base: WTy,
        poll: WTy,
        result: Vec<VT>,
    },
    RaceCancel {
        list: WTy,
        base: WTy,
        poll: WTy,
        result: Vec<VT>,
    },
    /// `cancel!`'s cleanup frame (std's `task_cleanup_frame` intrinsic):
    /// `(task) -> frame`, a `Suspend[bool]` over `base` that waits out the
    /// task's one cleanup (`crates/hd_wasm/DESIGN.md` §14.6).
    CleanupCold { base: WTy },
    /// Its poll: starts or continues the task's cleanup, and is Ready once
    /// the task no longer unwinds, with whether the cleanup was cut short.
    CleanupPoll { base: WTy },
    /// Its cancel: continues the task's cleanup, and unwinds while the
    /// task does, so a cancelled waiter keeps the cleanup going.
    CleanupCancel { base: WTy },
    /// A frame that never completes (std's `task_pending_frame`
    /// intrinsic): `() -> frame`, a suspension over `base` with the result
    /// layout `result` (`crates/hd_wasm/DESIGN.md` §14.6, "A frame that
    /// never completes").
    PendingCold { base: WTy, result: Vec<VT> },
    /// Its poll: the runtime checks, then Pending with no wake registered.
    PendingPoll { result: Vec<VT> },
    /// Its cancel: the runtime checks, which mark it cancelled.
    PendingCancel,
    /// A vtable slot over a concrete impl method: unerases the receiver.
    /// The slot's last `2 * keys` parameters are its row's providers; the
    /// target takes, per key of its own row, provider `provs[j]` of them.
    Adapter {
        sig: WTy,
        self_vts: Vec<VT>,
        target: Box<Sym>,
        params: Vec<VT>,
        results: Vec<VT>,
        keys: u32,
        provs: Vec<u32>,
        /// For a payload that is a reference whose value the target takes
        /// (`hd_mono::SlotRecv::ReadThrough`): how to read it.
        read: Option<Box<RefRead>>,
    },
    /// `hd.init`: every reachable group's init, in order. A flagged init
    /// belongs to a non-entry module and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts`).
    EntryInit { inits: Vec<(Hash128, bool)> },
    /// `hd.poll(ctx)`: builds the default profile's providers and runs
    /// `main`, or polls `main!`'s root (`bang` is its `$Suspend_L`) with
    /// the driver's `PollContext`; `-1` while pending, else the exit
    /// status. The first poll claims the root for its driver; a live root
    /// answers only its owner.
    EntryPoll {
        main: Hash128,
        /// Per row key: its vtable type, the host slots, and for a key
        /// whose provider is a trait value reference (`req.ref.host.reference-values`) the
        /// `std.rt.host_provider` instance that puts the host's trait
        /// value behind a new object.
        providers: Vec<(WTy, Vec<Helper>, Option<Hash128>)>,
        results: u32,
        bang: Option<(WTy, WTy)>,
        /// The `std.rt` result function's instance, which turns the
        /// root's result into the exit status; `None` for `void`.
        report: Option<(Hash128, Vec<VT>)>,
        /// A script's inits after its first own body, in order: a flagged
        /// one runs as a forbidden context, an unflagged one is another of
        /// the script's own bodies and runs with the same providers.
        then: Vec<(Hash128, bool)>,
    },
    /// `hd.wake(n)`: marks the completed handles.
    EntryWake,
    /// `hd.cancel`: cancels the entry's pending root suspension through
    /// its `$Task` prefix, as a cleanup frame's first poll does; a no-op when no root
    /// is pending. A program whose entry is a suspension, and a REPL
    /// input, export it (`req.entry.cancel`).
    EntryCancel,
    /// `hd.abandon`: abandons the entry's root suspension, as
    /// `Suspend.abandon` does (`req.entry.cancel.abandon`); a no-op when no
    /// root is pending. Exported with `hd.cancel` by a program whose entry
    /// is a suspension.
    EntryAbandon,
    /// `hd.scenario.cleanup`: calls the scenario's top-level cleanup
    /// function in the same instance; its `bool` comes back as an `i32`.
    /// A scenario-only export, emitted only when the entry module
    /// declares the function.
    EntryCall { func: Hash128 },
}

fn enc_wty(t: &WTy, w: &mut Writer) {
    encode_vts(&[VT::r(t.clone())], w);
}

fn dec_wty(r: &mut Reader<'_>) -> Option<WTy> {
    match decode_vts(r)?.pop()? {
        VT::Ref(t, _) => Some(std::sync::Arc::unwrap_or_clone(t)),
        _ => None,
    }
}

impl OptForm {
    pub(crate) fn encode(&self, w: &mut Writer) {
        enc_opt(self, w);
    }
    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<OptForm> {
        dec_opt(r)
    }
}

fn enc_opt(opt: &OptForm, w: &mut Writer) {
    match opt {
        OptForm::NullRef => w.u8(0),
        OptForm::NullStr => w.u8(1),
        OptForm::Tagged => w.u8(2),
        OptForm::Boxed(b) => {
            w.u8(3);
            enc_wty(b, w);
        }
    }
}

fn dec_opt(r: &mut Reader<'_>) -> Option<OptForm> {
    Some(match r.u8() {
        0 => OptForm::NullRef,
        1 => OptForm::NullStr,
        2 => OptForm::Tagged,
        _ => OptForm::Boxed(dec_wty(r)?),
    })
}

impl KeyOps {
    fn encode(&self, w: &mut Writer) {
        match self {
            KeyOps::I32(hash) => {
                w.u8(0);
                hash.encode(w);
            }
            KeyOps::I64(hash) => {
                w.u8(1);
                hash.encode(w);
            }
            KeyOps::Str(hash) => {
                w.u8(2);
                hash.encode(w);
            }
            KeyOps::Call { hash, eq, params } => {
                w.u8(3);
                hash.encode(w);
                eq.encode(w);
                encode_vts(params, w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> Option<KeyOps> {
        Some(match r.u8() {
            0 => KeyOps::I32(Box::new(Sym::decode(r)?)),
            1 => KeyOps::I64(Box::new(Sym::decode(r)?)),
            2 => KeyOps::Str(Box::new(Sym::decode(r)?)),
            _ => KeyOps::Call {
                hash: Box::new(Sym::decode(r)?),
                eq: Box::new(Sym::decode(r)?),
                params: decode_vts(r)?,
            },
        })
    }
}

impl Helper {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Helper::Lit => w.u8(0),
            Helper::IntToStr { wide, signed } => {
                w.u8(1);
                w.u8(u8::from(*wide));
                w.u8(u8::from(*signed));
            }
            Helper::StrEq => w.u8(2),
            Helper::StrToBuf => w.u8(3),
            Helper::Panic(m) => {
                w.u8(4);
                w.str(m);
            }
            Helper::PanicStr => w.u8(5),
            Helper::ListStep {
                code,
                env,
                list,
                elem,
                opt,
            } => {
                w.u8(6);
                enc_wty(code, w);
                enc_wty(env, w);
                enc_wty(list, w);
                encode_vts(elem, w);
                enc_opt(opt, w);
            }
            Helper::MapHash { key, ops } => {
                w.u8(20);
                encode_vts(key, w);
                ops.encode(w);
            }
            Helper::MapFind { map, key, ops } => {
                w.u8(21);
                enc_wty(map, w);
                encode_vts(key, w);
                ops.encode(w);
            }
            Helper::MapPut { map, key, val, ops } => {
                w.u8(22);
                enc_wty(map, w);
                encode_vts(key, w);
                encode_vts(val, w);
                ops.encode(w);
            }
            Helper::MapGrow { map, comps } => {
                w.u8(23);
                enc_wty(map, w);
                encode_vts(comps, w);
            }
            Helper::MapStep {
                code,
                env,
                comps,
                tuple,
                opt,
            } => {
                w.u8(24);
                enc_wty(code, w);
                enc_wty(env, w);
                encode_vts(comps, w);
                match tuple {
                    Some(t) => {
                        w.u8(1);
                        enc_wty(t, w);
                    }
                    None => w.u8(0),
                }
                enc_opt(opt, w);
            }
            Helper::HostCold {
                sig,
                frame,
                poll,
                cancel,
            } => {
                w.u8(7);
                enc_wty(sig, w);
                enc_wty(frame, w);
                poll.encode(w);
                cancel.encode(w);
            }
            Helper::HostPoll {
                frame,
                module,
                method,
                args,
                result,
            } => {
                w.u8(8);
                enc_wty(frame, w);
                w.str(module);
                w.str(method);
                w.len_of(args);
                for a in args {
                    a.encode(w);
                }
                result.encode(w);
            }
            Helper::HostCall {
                sig,
                module,
                method,
                args,
                result,
            } => {
                w.u8(26);
                enc_wty(sig, w);
                w.str(module);
                w.str(method);
                w.len_of(args);
                for a in args {
                    a.encode(w);
                }
                result.encode(w);
            }
            Helper::HostPrim {
                sig,
                name,
                args,
                result,
            } => {
                w.u8(34);
                enc_wty(sig, w);
                w.str(name);
                w.len_of(args);
                for a in args {
                    a.encode(w);
                }
                result.encode(w);
            }
            Helper::Enc(b) => {
                w.u8(27);
                b.encode(w);
            }
            Helper::Dec(b) => {
                w.u8(28);
                b.encode(w);
            }
            Helper::LebPut => w.u8(29),
            Helper::LebGet => w.u8(30),
            Helper::PowF64 => w.u8(32),
            Helper::RemF64 => w.u8(35),
            Helper::Fit => w.u8(31),
            Helper::HostCancel { frame } => {
                w.u8(11);
                enc_wty(frame, w);
            }
            Helper::Unlowered { sig, what } => {
                w.u8(12);
                enc_wty(sig, w);
                w.str(what);
            }
            Helper::HandleSlot {
                sig,
                payload,
                inner,
            } => {
                w.u8(33);
                enc_wty(sig, w);
                enc_wty(payload, w);
                inner.encode(w);
            }
            Helper::TypeIdOf { ty, name, shown } => {
                w.u8(42);
                enc_wty(ty, w);
                w.str(name);
                w.str(shown);
            }
            Helper::RuntimeType {
                sig,
                ty,
                name,
                shown,
            } => {
                w.u8(25);
                enc_wty(sig, w);
                enc_wty(ty, w);
                w.str(name);
                w.str(shown);
            }
            Helper::WakeMark => w.u8(13),
            Helper::WakeTake => w.u8(14),
            Helper::EraseI32 => w.u8(43),
            Helper::UneraseI32 => w.u8(44),
            Helper::ForbidCheck => w.u8(41),
            Helper::CancelEnter => w.u8(48),
            Helper::CancelCheck => w.u8(49),
            Helper::CancelLeave => w.u8(50),
            Helper::PollCheck => w.u8(51),
            Helper::WakeRegister => w.u8(39),
            Helper::WakeUp => w.u8(40),
            Helper::WakeAborted => w.u8(52),
            Helper::RaceCold {
                list,
                base,
                poll,
                result,
            }
            | Helper::RacePoll {
                list,
                base,
                poll,
                result,
            }
            | Helper::RaceCancel {
                list,
                base,
                poll,
                result,
            } => {
                w.u8(match self {
                    Helper::RaceCold { .. } => 15,
                    Helper::RacePoll { .. } => 16,
                    _ => 17,
                });
                enc_wty(list, w);
                enc_wty(base, w);
                enc_wty(poll, w);
                encode_vts(result, w);
            }
            Helper::CleanupCold { base } => {
                w.u8(45);
                enc_wty(base, w);
            }
            Helper::CleanupPoll { base } => {
                w.u8(46);
                enc_wty(base, w);
            }
            Helper::CleanupCancel { base } => {
                w.u8(47);
                enc_wty(base, w);
            }
            Helper::PendingCold { base, result } => {
                w.u8(53);
                enc_wty(base, w);
                encode_vts(result, w);
            }
            Helper::PendingPoll { result } => {
                w.u8(54);
                encode_vts(result, w);
            }
            Helper::PendingCancel => w.u8(55),
            Helper::Adapter {
                sig,
                self_vts,
                target,
                params,
                results,
                keys,
                provs,
                read,
            } => {
                w.u8(if read.is_some() { 57 } else { 9 });
                if let Some(rd) = read {
                    rd.encode(w);
                }
                enc_wty(sig, w);
                encode_vts(self_vts, w);
                target.encode(w);
                encode_vts(params, w);
                encode_vts(results, w);
                w.u32(*keys);
                w.len_of(provs);
                for p in provs {
                    w.u32(*p);
                }
            }
            Helper::EntryInit { inits } => {
                w.u8(18);
                w.len_of(inits);
                for (k, forbid) in inits {
                    w.hash(*k);
                    w.u8(u8::from(*forbid));
                }
            }
            Helper::EntryPoll {
                main,
                providers,
                results,
                bang,
                report,
                then,
            } => {
                w.u8(10);
                w.hash(*main);
                w.len_of(providers);
                for (vt, slots, by_ref) in providers {
                    enc_wty(vt, w);
                    w.len_of(slots);
                    for s in slots {
                        s.encode(w);
                    }
                    match by_ref {
                        Some(k) => {
                            w.u8(1);
                            w.hash(*k);
                        }
                        None => w.u8(0),
                    }
                }
                w.u32(*results);
                match bang {
                    Some((b, p)) => {
                        w.u8(1);
                        enc_wty(b, w);
                        enc_wty(p, w);
                    }
                    None => w.u8(0),
                }
                match report {
                    Some((k, vts)) => {
                        w.u8(1);
                        w.hash(*k);
                        encode_vts(vts, w);
                    }
                    None => w.u8(0),
                }
                w.len_of(then);
                for (k, forbid) in then {
                    w.hash(*k);
                    w.u8(u8::from(*forbid));
                }
            }
            Helper::EntryWake => w.u8(19),
            Helper::EntryCancel => w.u8(37),
            Helper::EntryAbandon => w.u8(56),
            Helper::EntryCall { func } => {
                w.u8(38);
                w.hash(*func);
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Helper> {
        let tag = r.u8();
        Some(match tag {
            0 => Helper::Lit,
            1 => Helper::IntToStr {
                wide: r.u8() == 1,
                signed: r.u8() == 1,
            },
            2 => Helper::StrEq,
            3 => Helper::StrToBuf,
            4 => Helper::Panic(r.str().to_owned()),
            5 => Helper::PanicStr,
            6 => Helper::ListStep {
                code: dec_wty(r)?,
                env: dec_wty(r)?,
                list: dec_wty(r)?,
                elem: decode_vts(r)?,
                opt: dec_opt(r)?,
            },
            20 => Helper::MapHash {
                key: decode_vts(r)?,
                ops: KeyOps::decode(r)?,
            },
            21 => Helper::MapFind {
                map: dec_wty(r)?,
                key: decode_vts(r)?,
                ops: KeyOps::decode(r)?,
            },
            22 => Helper::MapPut {
                map: dec_wty(r)?,
                key: decode_vts(r)?,
                val: decode_vts(r)?,
                ops: KeyOps::decode(r)?,
            },
            23 => Helper::MapGrow {
                map: dec_wty(r)?,
                comps: decode_vts(r)?,
            },
            24 => Helper::MapStep {
                code: dec_wty(r)?,
                env: dec_wty(r)?,
                comps: decode_vts(r)?,
                tuple: if r.u8() == 1 { Some(dec_wty(r)?) } else { None },
                opt: dec_opt(r)?,
            },
            7 => Helper::HostCold {
                sig: dec_wty(r)?,
                frame: dec_wty(r)?,
                poll: Box::new(Helper::decode(r)?),
                cancel: Box::new(Helper::decode(r)?),
            },
            8 => Helper::HostPoll {
                frame: dec_wty(r)?,
                module: r.str().to_owned(),
                method: r.str().to_owned(),
                args: (0..r.count())
                    .map(|_| ArgCodec::decode(r))
                    .collect::<Option<Vec<_>>>()?,
                result: ResCodec::decode(r)?,
            },
            26 => Helper::HostCall {
                sig: dec_wty(r)?,
                module: r.str().to_owned(),
                method: r.str().to_owned(),
                args: (0..r.count())
                    .map(|_| ArgCodec::decode(r))
                    .collect::<Option<Vec<_>>>()?,
                result: ResCodec::decode(r)?,
            },
            34 => Helper::HostPrim {
                sig: dec_wty(r)?,
                name: r.str().to_owned(),
                args: (0..r.count())
                    .map(|_| ArgCodec::decode(r))
                    .collect::<Option<Vec<_>>>()?,
                result: ResCodec::decode(r)?,
            },
            27 => Helper::Enc(BTy::decode(r)?),
            28 => Helper::Dec(BTy::decode(r)?),
            29 => Helper::LebPut,
            30 => Helper::LebGet,
            32 => Helper::PowF64,
            35 => Helper::RemF64,
            31 => Helper::Fit,
            11 => Helper::HostCancel { frame: dec_wty(r)? },
            12 => Helper::Unlowered {
                sig: dec_wty(r)?,
                what: r.str().to_owned(),
            },
            33 => Helper::HandleSlot {
                sig: dec_wty(r)?,
                payload: dec_wty(r)?,
                inner: Box::new(Helper::decode(r)?),
            },
            42 => Helper::TypeIdOf {
                ty: dec_wty(r)?,
                name: r.str().to_owned(),
                shown: r.str().to_owned(),
            },
            25 => Helper::RuntimeType {
                sig: dec_wty(r)?,
                ty: dec_wty(r)?,
                name: r.str().to_owned(),
                shown: r.str().to_owned(),
            },
            13 => Helper::WakeMark,
            14 => Helper::WakeTake,
            43 => Helper::EraseI32,
            44 => Helper::UneraseI32,
            41 => Helper::ForbidCheck,
            48 => Helper::CancelEnter,
            49 => Helper::CancelCheck,
            50 => Helper::CancelLeave,
            51 => Helper::PollCheck,
            39 => Helper::WakeRegister,
            40 => Helper::WakeUp,
            52 => Helper::WakeAborted,
            t @ 15..=17 => {
                let (list, base, poll, result) =
                    (dec_wty(r)?, dec_wty(r)?, dec_wty(r)?, decode_vts(r)?);
                match t {
                    15 => Helper::RaceCold {
                        list,
                        base,
                        poll,
                        result,
                    },
                    16 => Helper::RacePoll {
                        list,
                        base,
                        poll,
                        result,
                    },
                    _ => Helper::RaceCancel {
                        list,
                        base,
                        poll,
                        result,
                    },
                }
            }
            45 => Helper::CleanupCold { base: dec_wty(r)? },
            46 => Helper::CleanupPoll { base: dec_wty(r)? },
            47 => Helper::CleanupCancel { base: dec_wty(r)? },
            53 => Helper::PendingCold {
                base: dec_wty(r)?,
                result: decode_vts(r)?,
            },
            54 => Helper::PendingPoll {
                result: decode_vts(r)?,
            },
            55 => Helper::PendingCancel,
            18 => Helper::EntryInit {
                inits: (0..r.count()).map(|_| (r.hash(), r.u8() != 0)).collect(),
            },
            19 => Helper::EntryWake,
            37 => Helper::EntryCancel,
            56 => Helper::EntryAbandon,
            38 => Helper::EntryCall { func: r.hash() },
            9 | 57 => {
                let read = if tag == 57 {
                    Some(Box::new(RefRead::decode(r)?))
                } else {
                    None
                };
                Helper::Adapter {
                    sig: dec_wty(r)?,
                    self_vts: decode_vts(r)?,
                    target: Box::new(Sym::decode(r)?),
                    params: decode_vts(r)?,
                    results: decode_vts(r)?,
                    keys: r.u32(),
                    provs: (0..r.count()).map(|_| r.u32()).collect(),
                    read,
                }
            }
            10 => {
                let main = r.hash();
                let n = r.count();
                let mut providers = Vec::new();
                for _ in 0..n {
                    let vt = dec_wty(r)?;
                    let m = r.count();
                    let slots = (0..m)
                        .map(|_| Helper::decode(r))
                        .collect::<Option<Vec<_>>>()?;
                    let by_ref = if r.u8() == 1 { Some(r.hash()) } else { None };
                    providers.push((vt, slots, by_ref));
                }
                let results = r.u32();
                let bang = if r.u8() == 1 {
                    Some((dec_wty(r)?, dec_wty(r)?))
                } else {
                    None
                };
                let report = if r.u8() == 1 {
                    Some((r.hash(), decode_vts(r)?))
                } else {
                    None
                };
                let then = (0..r.count()).map(|_| (r.hash(), r.u8() != 0)).collect();
                Helper::EntryPoll {
                    main,
                    providers,
                    results,
                    bang,
                    report,
                    then,
                }
            }
            _ => return None,
        })
    }
}

/// The host runtime's stderr import (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[must_use]
pub fn stderr_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "stderr".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// The host runtime's `panic` import, which begins a panic report: the
/// host writes `panic: ` to standard error, and what the instance writes
/// there until it traps, `CATEGORY: MESSAGE` and a newline, is the report,
/// which test runners read the category and message of
/// (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
#[must_use]
pub fn panic_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "panic".into(),
        params: vec![],
        results: vec![],
    }
}

/// The host runtime's `fact(len)` import (`annot.fact.eval.panic-runtime`):
/// a facts body calls it as it starts, with its label's length in the
/// exchange buffer, and with `-1` as it ends. The host keeps the labels
/// of the facts bodies that run, and starts every panic report, the
/// instance's or its own after a trap, with the innermost one, so a panic
/// while a fact is evaluated is `fact-evaluation-failed`.
#[must_use]
pub fn fact_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "fact".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// The host runtime's stdout import: the REPL's value line, which needs
/// no `Console` requirement.
#[must_use]
pub fn stdout_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "stdout".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// The host runtime's `block` import (`crates/hd_wasm/DESIGN.md` §14.9).
#[must_use]
pub fn block_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "block".into(),
        params: vec![],
        results: vec![VT::I32],
    }
}

fn str_vts() -> Vec<VT> {
    vec![VT::r(WTy::Bytes), VT::I64]
}

/// `StrEq`'s fast path: two views of one array with one span (start and
/// length) are equal with no byte loop, since a string's array is never
/// written once a string views it (`docs/compiler/lowering-catalog.md`
/// "String").
fn str_eq_same_view(a: &mut Asm) {
    a.get(0);
    a.get(2);
    a.s().ref_eq();
    a.get(1);
    a.get(3);
    a.s().i64_eq().i32_and();
    a.if_();
    a.i32(1);
    a.s().return_();
    a.end();
}

/// Pushes byte `k + j` of the string in parameters `bytes` and `span`
/// (its array and its `len << 32 | start`).
fn str_byte(a: &mut Asm, bytes: u32, span: u32, k: u32, j: i32) {
    a.get(bytes);
    a.get(span);
    a.s().i32_wrap_i64();
    a.get(k);
    a.s().i32_add();
    if j != 0 {
        a.i32(j);
        a.s().i32_add();
    }
    a.array_get(&WTy::Bytes);
}

/// The literal pool: one cell per pooled literal, filled on first use.
#[must_use]
pub fn lit_pool() -> WTy {
    WTy::Array(VT::rn(WTy::Bytes))
}

/// The literal index: each pooled literal's segment offset and length.
#[must_use]
pub fn lit_index() -> WTy {
    WTy::Array(VT::I32)
}

/// The literal getter `(k) -> (ref $bytes)`, once link knows the pool and
/// index globals, and where in segment 0 the index of `count` literals
/// starts (`docs/compiler/lowering-catalog.md` "Literals"): literal `k`'s
/// array is built from the segment on its first use and kept in the pool.
#[must_use]
pub fn lit_code(pool: u32, index: u32, base: u32, count: u32) -> Code {
    let mut a = Asm::new(vec![VT::I32]);
    let cell = a.local(VT::rn(WTy::Bytes));
    a.s().global_get(pool);
    a.get(0);
    a.array_get(&lit_pool());
    a.s().local_tee(cell).ref_is_null();
    a.if_();
    // The index is read from the segment once, on the first miss.
    a.s().global_get(index).ref_is_null();
    a.if_();
    a.i32(base.cast_signed());
    a.i32(count.saturating_mul(2).cast_signed());
    a.array_new_data(&lit_index(), 0);
    a.s().global_set(index);
    a.end();
    a.s().global_get(pool);
    a.get(0);
    for field in [0, 1] {
        a.s().global_get(index).ref_as_non_null();
        a.get(0);
        a.i32(2);
        a.s().i32_mul();
        if field == 1 {
            a.i32(1);
            a.s().i32_add();
        }
        a.array_get(&lit_index());
    }
    a.array_new_data(&WTy::Bytes, 0);
    a.s().local_tee(cell);
    a.array_set(&lit_pool());
    a.end();
    a.raw_get(cell);
    a.s().ref_as_non_null();
    a.finish(vec![VT::r(WTy::Bytes)])
}

/// The code of a helper.
pub fn helper_code(h: &Helper) -> StageResult<Code> {
    Ok(match h {
        Helper::Lit => lit_code(0, 0, 0, 0),
        Helper::IntToStr { wide, signed } => {
            let mut a = Asm::new(vec![if *wide { VT::I64 } else { VT::I32 }]);
            let (x, neg, buf, i) = (
                a.local(VT::I64),
                a.local(VT::I32),
                a.local(VT::r(WTy::Bytes)),
                a.local(VT::I32),
            );
            a.get(0);
            if !*wide {
                if *signed {
                    a.s().i64_extend_i32_s();
                } else {
                    a.s().i64_extend_i32_u();
                }
            }
            a.set(x);
            if *signed {
                a.get(x);
                a.i64(0);
                a.s().i64_lt_s();
            } else {
                a.i32(0);
            }
            a.set(neg);
            a.get(neg);
            a.if_();
            a.i64(0);
            a.get(x);
            a.s().i64_sub();
            a.set(x);
            a.end();
            a.i32(21);
            a.array_new_default(&WTy::Bytes);
            a.set(buf);
            a.i32(21);
            a.set(i);
            a.loop_();
            a.get(i);
            a.i32(1);
            a.s().i32_sub();
            a.set(i);
            a.get(buf);
            a.get(i);
            a.get(x);
            a.i64(10);
            a.s().i64_rem_u().i32_wrap_i64();
            a.i32(48);
            a.s().i32_add();
            a.array_set(&WTy::Bytes);
            a.get(x);
            a.i64(10);
            a.s().i64_div_u();
            a.set(x);
            a.get(x);
            a.i64(0);
            a.s().i64_ne();
            a.br_if(0);
            a.end();
            a.get(neg);
            a.if_();
            a.get(i);
            a.i32(1);
            a.s().i32_sub();
            a.set(i);
            a.get(buf);
            a.get(i);
            a.i32(45);
            a.array_set(&WTy::Bytes);
            a.end();
            a.get(buf);
            a.i32(21);
            a.get(i);
            a.s().i32_sub().i64_extend_i32_u();
            a.i64(32);
            a.s().i64_shl();
            a.get(i);
            a.s().i64_extend_i32_u().i64_or();
            a.finish(str_vts())
        }
        Helper::StrEq => {
            let mut a = Asm::new([str_vts(), str_vts()].concat());
            let (len, k) = (a.local(VT::I32), a.local(VT::I32));
            str_eq_same_view(&mut a);
            a.get(1);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            a.get(len);
            a.get(3);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64().i32_ne();
            a.if_();
            a.i32(0);
            a.s().return_();
            a.end();
            // Eight bytes a step, as Go's `bytealg` compares words: Wasm
            // GC has no wide array read, so the step ORs the eight
            // bytes' XORs and branches once (#317).
            let (wide, acc) = (a.local(VT::I32), a.local(VT::I32));
            a.get(len);
            a.i32(-8);
            a.s().i32_and();
            a.set(wide);
            a.block();
            a.loop_();
            a.get(k);
            a.get(wide);
            a.s().i32_ge_u();
            a.br_if(1);
            a.i32(0);
            a.set(acc);
            for j in 0..8 {
                a.get(acc);
                str_byte(&mut a, 0, 1, k, j);
                str_byte(&mut a, 2, 3, k, j);
                a.s().i32_xor().i32_or();
                a.set(acc);
            }
            a.get(acc);
            a.if_();
            a.i32(0);
            a.s().return_();
            a.end();
            a.get(k);
            a.i32(8);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            // The tail, byte by byte.
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            str_byte(&mut a, 0, 1, k, 0);
            str_byte(&mut a, 2, 3, k, 0);
            a.s().i32_ne();
            a.if_();
            a.i32(0);
            a.s().return_();
            a.end();
            a.get(k);
            a.i32(1);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            a.i32(1);
            a.finish(vec![VT::I32])
        }
        Helper::StrToBuf => {
            let mut a = Asm::new(str_vts());
            let (len, k, need) = (a.local(VT::I32), a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            // Grow the exchange buffer to fit (`crates/hd_run_wasmtime/DESIGN.md` §17.3).
            a.get(len);
            a.i32(65535);
            a.s().i32_add();
            a.i32(16);
            a.s().i32_shr_u();
            a.set(need);
            a.get(need);
            a.s().memory_size(0);
            a.s().i32_gt_u();
            a.if_();
            a.get(need);
            a.s().memory_size(0).i32_sub().memory_grow(0).drop();
            a.end();
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(k);
            a.get(0);
            a.get(1);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.mem8_store(0);
            a.get(k);
            a.i32(1);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            a.get(len);
            a.finish(vec![VT::I32])
        }
        Helper::Panic(msg) => {
            let mut a = Asm::new(vec![]);
            a.call(panic_import());
            write_lit(&mut a, &format!("{msg}\n"));
            a.s().unreachable();
            a.finish(vec![])
        }
        Helper::PanicStr => {
            let mut a = Asm::new(str_vts());
            a.call(panic_import());
            write_lit(&mut a, "explicit-panic: ");
            a.get(0);
            a.get(1);
            a.call(Sym::Helper(Helper::StrToBuf));
            a.call(stderr_import());
            write_lit(&mut a, "\n");
            a.s().unreachable();
            a.finish(vec![])
        }
        Helper::ListStep {
            code,
            env,
            list,
            elem,
            opt,
        } => {
            let WTy::Func(params, results) = code else {
                return unsupported("a list cursor without a code type");
            };
            let mut a = Asm::new(params.clone());
            let e = a.local(VT::r(env.clone()));
            let l = a.local(VT::r(list.clone()));
            let i = a.local(VT::I32);
            a.get(0);
            a.ref_cast(env, false);
            a.set(e);
            a.get(e);
            a.struct_get(env, 1);
            a.set(l);
            // A length other than the captured one is the
            // `iterator-invalidated` panic, checked on every call so an
            // exhausted iterator panics too (`flow.for.invalidate.panic`).
            a.get(l);
            a.struct_get(list, 0);
            a.get(e);
            a.struct_get(env, 3);
            a.s().i32_ne();
            a.if_();
            a.call(Sym::Helper(Helper::Panic(
                "iterator-invalidated: list changed during iteration".into(),
            )));
            a.s().unreachable();
            a.end();
            a.get(e);
            a.struct_get(env, 2);
            a.set(i);
            a.get(i);
            a.get(l);
            a.struct_get(list, 0);
            a.s().i32_lt_u();
            a.if_();
            a.get(e);
            a.get(i);
            a.i32(1);
            a.s().i32_add();
            a.struct_set(env, 2);
            if *opt == OptForm::Tagged {
                a.i32(1);
            }
            for (k, v) in elem.iter().enumerate() {
                let st = crate::layout::storage(v).dflt();
                let arr = WTy::Array(st.clone());
                a.get(l);
                a.struct_get(list, 1 + u32::try_from(k).expect("k"));
                a.get(i);
                a.array_get(&arr);
                let want = if *opt == OptForm::NullRef {
                    v.dflt()
                } else {
                    v.clone()
                };
                a.conv(&st, &want);
            }
            if let OptForm::Boxed(b) = opt {
                a.struct_new(b);
            }
            a.s().return_();
            a.end();
            for v in results {
                a.zero(v);
            }
            a.finish(results.clone())
        }
        Helper::MapHash { key, ops } => crate::map::hash_code(key, ops)?,
        Helper::MapFind { map, key, ops } => crate::map::find_code(map, key, ops)?,
        Helper::MapPut { map, key, val, ops } => crate::map::put_code(map, key, val, ops),
        Helper::MapGrow { map, comps } => crate::map::grow_code(map, comps),
        Helper::MapStep {
            code,
            env,
            comps,
            tuple,
            opt,
        } => crate::map::step_code(code, env, comps, tuple.as_ref(), opt)?,
        Helper::HostCold {
            sig,
            frame,
            poll,
            cancel,
        } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("a host stub without a signature");
            };
            let mut a = Asm::new(params.clone());
            let f = a.local(VT::r(frame.clone()));
            a.struct_new_default(frame);
            a.set(f);
            a.get(f);
            a.ref_func(Sym::Helper((**cancel).clone()));
            a.struct_set(frame, F_CANCEL);
            a.get(f);
            a.ref_func(Sym::Helper((**poll).clone()));
            a.struct_set(frame, F_POLL);
            for k in 1..params.len() {
                a.get(f);
                a.get(u32::try_from(k).expect("k"));
                a.struct_set(frame, F_SAVED - 2 + u32::try_from(k).expect("k"));
            }
            let nf = u32::try_from(frame.fields().len()).expect("fields");
            a.get(f);
            a.i32(-1);
            a.struct_set(frame, nf - 1);
            a.get(f);
            a.finish(results.clone())
        }
        Helper::HostPoll {
            frame,
            module,
            method,
            args,
            result,
        } => host_poll(frame, module, method, args, result)?,
        Helper::HostCall {
            sig,
            module,
            method,
            args,
            result,
        } => host_call(sig, module, method, args, result, 1)?,
        Helper::HostPrim {
            sig,
            name,
            args,
            result,
        } => host_call(sig, "hd:prim", name, args, result, 0)?,
        Helper::Enc(b) => crate::boundary::enc_code(b)?,
        Helper::Dec(b) => crate::boundary::dec_code(b)?,
        Helper::LebPut => crate::boundary::leb_put_code(),
        Helper::LebGet => crate::boundary::leb_get_code(),
        Helper::PowF64 => crate::pow::pow_f64_code(),
        Helper::RemF64 => crate::rem::rem_f64_code(),
        Helper::Fit => crate::boundary::fit_code(),
        Helper::HostCancel { frame } => {
            let mut a = Asm::new(vec![VT::Eq]);
            let f = a.local(VT::r(frame.clone()));
            let nf = u32::try_from(frame.fields().len()).expect("fields");
            a.get(0);
            a.ref_cast(frame, false);
            a.set(f);
            a.get(f);
            a.struct_get(frame, F_FLAGS);
            a.i32(DONE | CANCELLED);
            a.s().i32_and();
            a.if_();
            a.s().return_();
            a.end();
            a.get(f);
            a.get(f);
            a.struct_get(frame, F_FLAGS);
            a.i32(CANCELLED);
            a.s().i32_or();
            a.struct_set(frame, F_FLAGS);
            a.get(f);
            a.struct_get(frame, nf - 1);
            a.i32(0);
            a.s().i32_ge_s();
            a.if_();
            a.get(f);
            a.struct_get(frame, nf - 1);
            a.call(abort_import());
            // No completion will wake the call's awaiter, which may be
            // another task than the canceller: wake it here, so it sees the
            // call cancelled (`req.cancel.await`).
            a.get(f);
            a.struct_get(frame, nf - 1);
            a.call(Sym::Helper(Helper::WakeAborted));
            a.end();
            a.finish(vec![])
        }
        Helper::Unlowered { sig, what } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("an unlowered slot without a signature");
            };
            let mut a = Asm::new(params.clone());
            a.call(Sym::Helper(Helper::Panic(format!(
                "unsupported: {what} is not lowered yet"
            ))));
            a.s().unreachable();
            a.finish(results.clone())
        }
        Helper::HandleSlot {
            sig,
            payload,
            inner,
        } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("a handle slot without a signature");
            };
            let mut a = Asm::new(params.clone());
            a.get(0);
            a.get(0);
            a.ref_cast(payload, false);
            a.struct_get(payload, 0);
            for k in 1..params.len() {
                a.get(u32::try_from(k).expect("k"));
            }
            a.call(Sym::Helper((**inner).clone()));
            a.finish(results.clone())
        }
        Helper::RuntimeType {
            sig,
            ty,
            name,
            shown,
        } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("a `runtime_type` slot without a signature");
            };
            let mut a = Asm::new(params.clone());
            a.call(Sym::Helper(Helper::TypeIdOf {
                ty: ty.clone(),
                name: name.clone(),
                shown: shown.clone(),
            }));
            a.finish(results.clone())
        }
        Helper::TypeIdOf { ty, name, shown } => {
            let g = type_id_global(ty, name);
            let mut a = Asm::new(vec![]);
            a.global_get(g.clone());
            a.s().ref_is_null();
            a.if_();
            push_type_id(&mut a, ty, name, shown);
            a.global_set(g.clone());
            a.end();
            a.global_get(g);
            a.s().ref_as_non_null();
            a.finish(vec![VT::r(ty.clone())])
        }
        // Light until link finds an `all!` (`wake_nodes`).
        Helper::WakeMark => wake_mark(false),
        Helper::WakeTake => wake_take(),
        Helper::EraseI32 => erase_i32(),
        Helper::UneraseI32 => unerase_i32(),
        // Empty until link finds a forbidden context (`forbid_check`).
        Helper::ForbidCheck => forbid_check(false),
        Helper::CancelEnter => {
            let mut a = Asm::new(vec![]);
            a.global_get(abandon_seen_global());
            a.i32(0);
            a.global_set(abandon_seen_global());
            a.finish(vec![VT::I32])
        }
        Helper::CancelCheck => cancel_check(),
        Helper::CancelLeave => cancel_leave(),
        Helper::PollCheck => poll_check(),
        Helper::WakeRegister => wake_register(false),
        Helper::WakeUp => wake_up(),
        Helper::WakeAborted => wake_aborted(false),
        Helper::RaceCold {
            list,
            base,
            poll,
            result,
        } => {
            let fr = race_frame(list, base, result);
            let mut a = Asm::new(vec![VT::r(list.clone())]);
            let f = a.local(VT::r(fr.clone()));
            a.struct_new_default(&fr);
            a.set(f);
            a.get(f);
            let h = |k: u8| Helper::from_race(k, list, base, poll, result);
            a.ref_func(Sym::Helper(h(2)));
            a.struct_set(&fr, F_CANCEL);
            a.get(f);
            a.ref_func(Sym::Helper(h(1)));
            a.struct_set(&fr, F_POLL);
            a.get(f);
            a.get(0);
            a.struct_set(&fr, F_SAVED - 1);
            a.get(f);
            a.finish(vec![VT::r(base.clone())])
        }
        Helper::RacePoll {
            list,
            base,
            poll,
            result,
        } => race_poll(list, base, poll, result),
        // A loser left unwinding (a shielded `dispose!` waits) keeps the
        // race unwinding: each later cancel continues it (flow.using.cancel).
        Helper::RaceCancel {
            list, base, result, ..
        } => {
            let fr = race_frame(list, base, result);
            let mut a = Asm::new(vec![VT::Eq]);
            let f = a.local(VT::r(fr.clone()));
            let wait = a.local(VT::I32);
            a.get(0);
            a.ref_cast(&fr, false);
            a.set(f);
            a.get(f);
            a.struct_get(&fr, F_FLAGS);
            a.i32(UNWINDING);
            a.s().i32_and();
            a.s().i32_eqz();
            a.if_();
            a.get(f);
            a.struct_get(&fr, F_FLAGS);
            a.i32(DONE | CANCELLED);
            a.s().i32_and();
            a.if_();
            a.s().return_();
            a.end();
            a.end();
            cancel_list(&mut a, f, &fr, list, None, wait);
            a.get(f);
            a.i32(CANCELLED);
            a.get(wait);
            a.i32(UNWINDING);
            a.s().i32_mul().i32_or();
            a.struct_set(&fr, F_FLAGS);
            a.finish(vec![])
        }
        Helper::CleanupCold { base } => {
            let fr = cleanup_frame(base);
            let mut a = Asm::new(vec![VT::r(task_base())]);
            let f = a.local(VT::r(fr.clone()));
            a.struct_new_default(&fr);
            a.set(f);
            a.get(f);
            a.ref_func(Sym::Helper(Helper::CleanupCancel { base: base.clone() }));
            a.struct_set(&fr, F_CANCEL);
            a.get(f);
            a.ref_func(Sym::Helper(Helper::CleanupPoll { base: base.clone() }));
            a.struct_set(&fr, F_POLL);
            a.get(f);
            a.get(0);
            a.struct_set(&fr, F_CHILD);
            a.get(f);
            a.finish(vec![VT::r(base.clone())])
        }
        Helper::CleanupPoll { base } => cleanup_poll(base),
        Helper::CleanupCancel { base } => {
            let fr = cleanup_frame(base);
            let mut a = Asm::new(vec![VT::Eq]);
            let f = a.local(VT::r(fr.clone()));
            let wait = a.local(VT::I32);
            a.get(0);
            a.ref_cast(&fr, false);
            a.set(f);
            a.get(f);
            a.struct_get(&fr, F_FLAGS);
            a.i32(UNWINDING);
            a.s().i32_and();
            a.s().i32_eqz();
            a.if_();
            a.get(f);
            a.struct_get(&fr, F_FLAGS);
            a.i32(DONE | CANCELLED);
            a.s().i32_and();
            a.if_();
            a.s().return_();
            a.end();
            a.end();
            continue_cleanup(&mut a, f, &fr, wait);
            a.get(f);
            a.i32(CANCELLED);
            a.get(wait);
            a.i32(UNWINDING);
            a.s().i32_mul().i32_or();
            a.struct_set(&fr, F_FLAGS);
            a.finish(vec![])
        }
        Helper::PendingCold { base, result } => {
            let fr = crate::layout::frame_of(base, &[]);
            let mut a = Asm::new(vec![]);
            let f = a.local(VT::r(fr.clone()));
            a.struct_new_default(&fr);
            a.set(f);
            a.get(f);
            a.ref_func(Sym::Helper(Helper::PendingCancel));
            a.struct_set(&fr, F_CANCEL);
            a.get(f);
            a.ref_func(Sym::Helper(Helper::PendingPoll {
                result: result.clone(),
            }));
            a.struct_set(&fr, F_POLL);
            a.get(f);
            a.finish(vec![VT::r(base.clone())])
        }
        Helper::PendingPoll { result } => {
            let task = task_base();
            let mut res = vec![VT::I32];
            res.extend(result.iter().map(VT::dflt));
            let mut a = Asm::new(vec![VT::Eq]);
            // Pending whether or not it was cancelled: a cancelled one
            // cancels its awaiter (`req.cancel.await`), a live one never
            // completes. It registers no wake, since nothing will wake it.
            a.get(0);
            a.ref_cast(&task, false);
            a.call(Sym::Helper(Helper::PollCheck));
            a.s().drop();
            for v in &res {
                a.zero(v);
            }
            a.finish(res)
        }
        Helper::PendingCancel => {
            let task = task_base();
            let mut a = Asm::new(vec![VT::Eq]);
            a.get(0);
            a.ref_cast(&task, false);
            a.call(Sym::Helper(Helper::CancelCheck));
            a.s().drop();
            a.finish(vec![])
        }
        Helper::Adapter {
            sig,
            self_vts,
            target,
            params,
            results,
            keys,
            provs,
            read,
        } => {
            let WTy::Func(sp, sr) = sig else {
                return unsupported("an adapter without a signature");
            };
            let at = sp.len() - 2 * *keys as usize;
            let mut a = Asm::new(sp.clone());
            unerase(&mut a, 0, self_vts);
            let mut self_n = self_vts.len();
            if let Some(rd) = read {
                rd.emit(&mut a);
                // The read values become the target's receiver.
                let tmp: Vec<u32> = rd.results.iter().map(|v| a.local(v.clone())).collect();
                for l in tmp.iter().rev() {
                    a.set(*l);
                }
                for (l, (have, want)) in tmp.iter().zip(rd.results.iter().zip(params.iter())) {
                    a.get(*l);
                    a.conv(have, want);
                }
                self_n = rd.results.len();
            }
            for (k, (j, p)) in (self_n..).zip(sp[..at].iter().enumerate().skip(1)) {
                a.get(u32::try_from(j).expect("j"));
                if let Some(want) = params.get(k) {
                    a.conv(p, want);
                }
            }
            for p in provs {
                let j = u32::try_from(at).expect("at") + 2 * p;
                a.get(j);
                a.get(j + 1);
            }
            a.call((**target).clone());
            let tmp: Vec<u32> = results.iter().map(|v| a.local(v.clone())).collect();
            for l in tmp.iter().rev() {
                a.set(*l);
            }
            for (l, (have, want)) in tmp.iter().zip(results.iter().zip(sr)) {
                a.get(*l);
                a.conv(have, want);
            }
            a.finish(sr.clone())
        }
        Helper::EntryInit { inits } => {
            let mut a = Asm::new(vec![]);
            for (k, forbid) in inits {
                if *forbid {
                    forbid_by(&mut a, 1);
                }
                a.call(Sym::Inst(*k));
                if *forbid {
                    forbid_by(&mut a, -1);
                }
            }
            a.finish(vec![])
        }
        Helper::EntryPoll {
            main,
            providers,
            results,
            bang,
            report,
            then,
        } => {
            // Parameter 0 is the driver's `PollContext`, one `hd.poll`
            // caller among drivers that share the instance. A plain entry
            // ignores it; a suspending root answers only its owner.
            let mut a = Asm::new(vec![VT::I32]);
            let ctx = 0;
            // A host provider is a trait value with no payload. One whose
            // key takes a trait value reference (`req.ref.host.reference-values`) is that
            // trait value behind a new object, the `Ref[dyn K]` that
            // `host_provider` returns, stored as the payload with the
            // vtable again beside it, as `RefAsProvider` stores one.
            let build = |a: &mut Asm| {
                for (vt, slots, by_ref) in providers {
                    let table = |a: &mut Asm| {
                        for s in slots {
                            a.ref_func(Sym::Helper(s.clone()));
                        }
                        a.struct_new(vt);
                    };
                    a.ref_null_eq();
                    table(a);
                    if let Some(k) = by_ref {
                        a.call(Sym::Inst(*k));
                        table(a);
                    }
                }
            };
            // A release of the root also releases its driver.
            let release = |a: &mut Asm, base: &WTy| {
                a.ref_null(base);
                a.global_set(root_global());
                a.i32(0);
                a.global_set(owner_global());
            };
            match bang {
                None => {
                    build(&mut a);
                    a.call(Sym::Inst(*main));
                    // The result is on the stack, as the report's parameters.
                    if let Some((k, _)) = report {
                        a.call(Sym::Inst(*k));
                    } else {
                        for _ in 0..*results {
                            a.s().drop();
                        }
                        // A script's later inits: its own bodies, which
                        // share the first's row and result, and the
                        // others, as forbidden contexts.
                        for (k, forbid) in then {
                            if *forbid {
                                forbid_by(&mut a, 1);
                                a.call(Sym::Inst(*k));
                                forbid_by(&mut a, -1);
                            } else {
                                build(&mut a);
                                a.call(Sym::Inst(*k));
                                for _ in 0..*results {
                                    a.s().drop();
                                }
                            }
                        }
                        a.i32(0);
                    }
                }
                Some((base, poll)) => {
                    let WTy::Func(_, prs) = poll else {
                        return unsupported("a root poll type");
                    };
                    let root = a.local(VT::r(base.clone()));
                    a.global_get(root_global());
                    a.s().ref_is_null();
                    a.if_();
                    build(&mut a);
                    a.call(Sym::Inst(*main));
                    a.global_set(root_global());
                    // The first poll claims the root for its driver.
                    a.get(ctx);
                    a.i32(1);
                    a.s().i32_add();
                    a.global_set(owner_global());
                    a.end();
                    a.global_get(root_global());
                    a.ref_cast(base, false);
                    a.set(root);
                    // A live root answers only its owner. An active root is
                    // a recursive poll, whatever the driver; a finished root
                    // is invalid, whatever the driver; only then does a
                    // foreign driver compete. The root's own checks hold the
                    // same codes behind its poll.
                    a.get(root);
                    a.struct_get(base, F_FLAGS);
                    a.i32(ACTIVE);
                    a.s().i32_and();
                    a.if_();
                    a.call(Sym::Helper(Helper::Panic(
                        "suspension-reentrant-poll: a suspension was polled or cancelled while active"
                            .into(),
                    )));
                    a.s().unreachable();
                    a.end();
                    a.get(root);
                    a.struct_get(base, F_FLAGS);
                    a.i32(DONE);
                    a.s().i32_and();
                    a.if_();
                    a.call(Sym::Helper(Helper::Panic(
                        "suspension-invalid-state: a completed suspension was polled".into(),
                    )));
                    a.s().unreachable();
                    a.end();
                    a.global_get(owner_global());
                    a.get(ctx);
                    a.i32(1);
                    a.s().i32_add();
                    a.s().i32_ne();
                    a.if_();
                    a.call(Sym::Helper(Helper::Panic(
                        "suspension-competing-driver: a suspension was polled by a driver that does not own it"
                            .into(),
                    )));
                    a.s().unreachable();
                    a.end();
                    // An `abandon()` during the poll may have ended a
                    // cleanup that pending parties wait on with no wake
                    // (`crates/hd_wasm/DESIGN.md` §14.6, "Abandoning"), so
                    // the root is polled again while one asks for it.
                    // A root cancelled by an await of a cancelled
                    // suspension (`req.cancel.await`) is no longer polled:
                    // its cancel continues its cleanup.
                    let got: Vec<u32> = prs.iter().map(|v| a.local(v.clone())).collect();
                    let cancelled = |a: &mut Asm| {
                        a.get(root);
                        a.struct_get(base, F_FLAGS);
                        a.i32(CANCELLED);
                        a.s().i32_and();
                    };
                    a.loop_();
                    a.i32(0);
                    a.global_set(rewake_global());
                    cancelled(&mut a);
                    a.if_();
                    a.get(root);
                    a.get(root);
                    a.struct_get(base, F_CANCEL);
                    a.call_ref(&cancel_fn());
                    a.else_();
                    a.get(root);
                    a.get(root);
                    a.struct_get(base, F_POLL);
                    a.call_ref(poll);
                    for l in got.iter().rev() {
                        a.set(*l);
                    }
                    a.end();
                    a.get(got[0]);
                    a.s().i32_eqz();
                    a.global_get(rewake_global());
                    a.s().i32_and();
                    a.br_if(0);
                    a.end();
                    // Its cleanup ended: the run ends cancelled
                    // (`req.entry.cancelled`), -2 when the cleanup ran to
                    // its end and -3 when a part was cut short. The root
                    // is released, as a completed one is.
                    cancelled(&mut a);
                    a.if_();
                    a.get(root);
                    a.struct_get(base, F_FLAGS);
                    a.i32(UNWINDING);
                    a.s().i32_and().i32_eqz();
                    a.if_();
                    a.i32(CANCELLED_END);
                    a.get(root);
                    a.struct_get(base, F_FLAGS);
                    a.i32(ABANDONED);
                    a.s().i32_and();
                    a.i32(0);
                    a.s().i32_ne().i32_sub();
                    release(&mut a, base);
                    a.s().return_();
                    a.end();
                    a.end();
                    for l in &got {
                        a.raw_get(*l);
                    }
                    // Ready: the report of the result; else -1.
                    if let Some((k, want)) = report {
                        let tmp: Vec<u32> = prs[1..].iter().map(|v| a.local(v.clone())).collect();
                        for l in tmp.iter().rev() {
                            a.set(*l);
                        }
                        let ready = a.local(VT::I32);
                        let status = a.local(VT::I32);
                        a.set(ready);
                        a.i32(-1);
                        a.set(status);
                        a.get(ready);
                        a.if_();
                        // A completed entry no longer owns this suspension.
                        // The next property body creates a fresh root while
                        // retaining the rest of its instance's state.
                        release(&mut a, base);
                        for (l, (have, want)) in tmp.iter().zip(prs[1..].iter().zip(want)) {
                            a.get(*l);
                            a.conv(have, want);
                        }
                        a.call(Sym::Inst(*k));
                        a.set(status);
                        a.end();
                        a.get(status);
                    } else {
                        for _ in 1..prs.len() {
                            a.s().drop();
                        }
                        let ready = a.local(VT::I32);
                        a.set(ready);
                        a.get(ready);
                        a.if_();
                        release(&mut a, base);
                        a.end();
                        a.get(ready);
                        a.i32(1);
                        a.s().i32_sub();
                    }
                }
            }
            a.finish(vec![VT::I32])
        }
        Helper::EntryWake => {
            let mut a = Asm::new(vec![VT::I32]);
            a.get(0);
            a.call(Sym::Helper(Helper::WakeMark));
            a.finish(vec![])
        }
        Helper::EntryCall { func } => {
            let mut a = Asm::new(vec![]);
            a.call(Sym::Inst(*func));
            a.finish(vec![VT::I32])
        }
        Helper::EntryCancel | Helper::EntryAbandon => {
            // The root global holds a `$Suspend_L` frame, whose `$Task`
            // prefix carries the cancel function first; a null root (no
            // poll yet, or an ended root) cancels nothing. An abandon runs
            // the same cancel with the `abandon()` depth raised, as
            // `task_abandon` does (`crates/hd_wasm/DESIGN.md` §14.6).
            let abandon = matches!(h, Helper::EntryAbandon);
            let mut a = Asm::new(vec![]);
            let base = task_base();
            let root = a.local(VT::rn(base.clone()));
            a.global_get(root_global());
            a.set(root);
            a.get(root);
            a.s().ref_is_null();
            a.if_();
            a.s().return_();
            a.end();
            let depth = |a: &mut Asm, by: i32| {
                a.global_get(abandoning_global());
                a.i32(by);
                a.s().i32_add();
                a.global_set(abandoning_global());
            };
            if abandon {
                depth(&mut a, 1);
            }
            a.get(root);
            a.get(root);
            a.struct_get(&base, F_CANCEL);
            a.call_ref(&cancel_fn());
            if abandon {
                depth(&mut a, -1);
            }
            a.finish(vec![])
        }
    })
}

/// Writes a fixed text to standard error through the runtime import.
pub(crate) fn write_lit(a: &mut Asm, text: &str) {
    write_lit_to(a, text, stderr_import());
}

/// Writes the literal `text` through the runtime import `write`.
pub(crate) fn write_lit_to(a: &mut Asm, text: &str, write: Sym) {
    a.lit(text.as_bytes());
    a.i64(i64::try_from(text.len()).unwrap_or(0) << 32);
    a.call(Sym::Helper(Helper::StrToBuf));
    a.call(write);
}

/// The global that holds the `TypeId` (`st`) whose key is `key` once
/// [`Helper::TypeIdOf`] built it: null until then.
#[must_use]
pub fn type_id_global(st: &WTy, key: &str) -> GSym {
    let mut h = hd_base::StableHasher::new("type-id");
    h.str(key);
    GSym::TypeId(h.finish(), VT::rn(st.clone()))
}

/// Pushes a `TypeId`, the std data type `st` over its key string `key`
/// (the identity) and its printable name `shown` (trait.typeid.name).
pub fn push_type_id(a: &mut Asm, st: &WTy, key: &str, shown: &str) {
    for text in [key, shown] {
        a.lit(text.as_bytes());
        a.i64(i64::try_from(text.len()).unwrap_or(0) << 32);
    }
    a.struct_new(st);
}

/// The code of [`Helper::EraseI32`]: `ref.i31` when shifting the value
/// out and back by one bit keeps it, else a box.
fn erase_i32() -> Code {
    let mut a = Asm::new(vec![VT::I32]);
    a.get(0);
    a.i32(1);
    a.s().i32_shl();
    a.i32(1);
    a.s().i32_shr_s();
    a.get(0);
    a.s().i32_eq();
    a.if_ty(BlockType::Result(wasm_encoder::ValType::Ref(
        wasm_encoder::RefType::EQREF,
    )));
    a.get(0);
    a.s().ref_i31();
    a.else_();
    a.get(0);
    a.struct_new(&crate::layout::box_of(&[VT::I32]));
    a.end();
    a.finish(vec![VT::Eq])
}

/// The code of [`Helper::UneraseI32`]: an `i31ref` read signed, else the
/// box's field.
fn unerase_i32() -> Code {
    let i31 = wasm_encoder::HeapType::Abstract {
        shared: false,
        ty: wasm_encoder::AbstractHeapType::I31,
    };
    let b = crate::layout::box_of(&[VT::I32]);
    let mut a = Asm::new(vec![VT::Eq]);
    a.get(0);
    a.s().ref_test_non_null(i31);
    a.if_ty(BlockType::Result(wasm_encoder::ValType::I32));
    a.get(0);
    a.s().ref_cast_non_null(i31).i31_get_s();
    a.else_();
    a.get(0);
    a.ref_cast(&b, false);
    a.struct_get(&b, 0);
    a.end();
    a.finish(vec![VT::I32])
}

/// Unerases parameter `p` (an `eqref`) to `vts`: one reference is cast,
/// an `i32` read back from its `i31ref` or box, anything else read out of
/// its box.
pub fn unerase(a: &mut Asm, p: u32, vts: &[VT]) {
    match vts {
        [VT::Eq] => a.get(p),
        [VT::I32] => {
            a.get(p);
            a.call(Sym::Helper(Helper::UneraseI32));
        }
        [v @ VT::Ref(..)] => {
            a.get(p);
            a.conv(&VT::Eq, v);
        }
        _ => {
            let b = crate::layout::box_of(vts);
            for k in 0..vts.len() {
                a.get(p);
                a.ref_cast(&b, false);
                a.struct_get(&b, u32::try_from(k).expect("k"));
            }
        }
    }
}

impl Helper {
    fn from_race(k: u8, list: &WTy, base: &WTy, poll: &WTy, result: &[VT]) -> Helper {
        let (list, base, poll, result) =
            (list.clone(), base.clone(), poll.clone(), result.to_vec());
        match k {
            0 => Helper::RaceCold {
                list,
                base,
                poll,
                result,
            },
            1 => Helper::RacePoll {
                list,
                base,
                poll,
                result,
            },
            _ => Helper::RaceCancel {
                list,
                base,
                poll,
                result,
            },
        }
    }
}

/// `race!`'s frame: a suspension holding its list of children, then the
/// winner's value while unwinding losers settle (`F_STATE` 1).
fn race_frame(list: &WTy, base: &WTy, result: &[VT]) -> WTy {
    let mut fields = vec![VT::rn(list.clone())];
    fields.extend(result.iter().map(VT::dflt));
    crate::layout::frame_of(base, &fields)
}

/// [`Helper::CancelCheck`] (`crates/hd_wasm/DESIGN.md` §14.6).
fn cancel_check() -> Code {
    let task = task_base();
    let mut a = Asm::new(vec![VT::r(task.clone())]);
    let flags = a.local(VT::I32);
    a.get(0);
    a.struct_get(&task, F_FLAGS);
    a.set(flags);
    a.get(flags);
    a.i32(ACTIVE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-reentrant-poll: a suspension was polled or cancelled while active".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(flags);
    a.i32(UNWINDING);
    a.s().i32_and().i32_eqz();
    a.if_();
    a.get(flags);
    a.i32(DONE | CANCELLED);
    a.s().i32_and();
    a.if_();
    a.i32(0);
    a.s().return_();
    a.end();
    a.end();
    a.get(0);
    a.get(flags);
    a.i32(CANCELLED);
    a.s().i32_or();
    a.global_get(abandoning_global());
    a.s().i32_eqz().i32_eqz();
    a.i32(ABANDONED);
    a.s().i32_mul().i32_or();
    a.struct_set(&task, F_FLAGS);
    a.i32(1);
    a.finish(vec![VT::I32])
}

/// [`Helper::CancelLeave`]: the `ABANDONED` bits that the frame's
/// cancels saw mark it (`req.abandon.carry`); then `abseen` holds the
/// bits seen before the frame's cancel with the frame's own.
fn cancel_leave() -> Code {
    let task = task_base();
    let mut a = Asm::new(vec![VT::r(task.clone()), VT::I32]);
    a.get(0);
    a.get(0);
    a.struct_get(&task, F_FLAGS);
    a.global_get(abandon_seen_global());
    a.i32(ABANDONED);
    a.s().i32_and().i32_or();
    a.struct_set(&task, F_FLAGS);
    a.get(1);
    a.get(0);
    a.struct_get(&task, F_FLAGS);
    a.i32(ABANDONED);
    a.s().i32_and().i32_or();
    a.global_set(abandon_seen_global());
    a.finish(vec![])
}

/// [`Helper::PollCheck`] (`crates/hd_wasm/DESIGN.md` §14.3, §14.6).
fn poll_check() -> Code {
    let task = task_base();
    let mut a = Asm::new(vec![VT::r(task.clone())]);
    let flags = a.local(VT::I32);
    a.get(0);
    a.struct_get(&task, F_FLAGS);
    a.set(flags);
    a.get(flags);
    a.i32(ACTIVE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-reentrant-poll: a suspension was polled or cancelled while active".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(flags);
    a.i32(DONE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-invalid-state: a completed suspension was polled".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(flags);
    a.i32(CANCELLED);
    a.s().i32_and().i32_eqz();
    a.if_();
    a.i32(0);
    a.s().return_();
    a.end();
    // A cancelled frame that still unwinds: every party that polls it
    // continues its one cleanup, as a cancel would, under no waker node
    // and counted as unregistered, so the party polls again.
    a.get(flags);
    a.i32(UNWINDING);
    a.s().i32_and();
    a.if_();
    let regs = a.local(VT::I32);
    a.global_get(wake_regs());
    a.set(regs);
    unwatched(&mut a, |a| {
        a.get(0);
        a.get(0);
        a.struct_get(&task, F_CANCEL);
        a.call_ref(&cancel_fn());
    });
    a.get(regs);
    a.global_set(wake_regs());
    a.end();
    a.i32(1);
    a.finish(vec![VT::I32])
}

/// `cancel!`'s cleanup frame: a suspension holding the task whose
/// cleanup it waits out.
fn cleanup_frame(base: &WTy) -> WTy {
    crate::layout::frame_of(base, &[VT::rn(task_base())])
}

/// Runs `body`, which starts or continues cleanups, under no `all!` waker
/// node (`crates/hd_wasm/DESIGN.md` §14.6, "One cleanup per job"). Several
/// parties may continue one cleanup, so its host leaves register under
/// none of them, and each party polls again on every poll.
fn unwatched(a: &mut Asm, body: impl FnOnce(&mut Asm)) {
    let (node, slot) = (a.local(VT::Eq), a.local(VT::I32));
    a.global_get(wake_cur_node());
    a.set(node);
    a.global_get(wake_cur_slot());
    a.set(slot);
    a.ref_null_eq();
    a.global_set(wake_cur_node());
    body(a);
    a.raw_get(node);
    a.global_set(wake_cur_node());
    a.get(slot);
    a.global_set(wake_cur_slot());
}

/// Starts or continues the cleanup of the task in a cleanup frame, and
/// sets local `wait` while the task still unwinds.
fn continue_cleanup(a: &mut Asm, f: u32, fr: &WTy, wait: u32) {
    let task = task_base();
    let c = a.local(VT::r(task.clone()));
    a.get(f);
    a.struct_get(fr, F_CHILD);
    a.s().ref_as_non_null();
    a.set(c);
    unwatched(a, |a| {
        a.get(c);
        a.get(c);
        a.struct_get(&task, F_CANCEL);
        a.call_ref(&cancel_fn());
    });
    a.get(c);
    a.struct_get(&task, F_FLAGS);
    a.i32(UNWINDING);
    a.s().i32_and();
    a.i32(0);
    a.s().i32_ne();
    a.set(wait);
}

/// The cleanup frame's poll (`crates/hd_wasm/DESIGN.md` §14.6): the task's
/// own cancel starts its cleanup, or continues it while it unwinds, and
/// does nothing once it ended. Pending while the task unwinds; a Pending
/// poll registers no wake, so an `all!` polls it again. Ready with whether
/// the cleanup was cut short: the task's kept `ABANDONED` bit.
fn cleanup_poll(base: &WTy) -> Code {
    let fr = cleanup_frame(base);
    let mut a = Asm::new(vec![VT::Eq]);
    let (f, wait, regs) = (
        a.local(VT::r(fr.clone())),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    a.get(0);
    a.ref_cast(&fr, false);
    a.set(f);
    // The runtime checks; a cancelled wait answers Pending
    // (`req.cancel.await`).
    a.get(f);
    a.call(Sym::Helper(Helper::PollCheck));
    a.if_();
    a.i32(0);
    a.i32(0);
    a.s().return_();
    a.end();
    a.global_get(wake_regs());
    a.set(regs);
    continue_cleanup(&mut a, f, &fr, wait);
    a.get(wait);
    a.if_();
    a.get(regs);
    a.global_set(wake_regs());
    a.i32(0);
    a.i32(0);
    a.s().return_();
    a.end();
    a.get(f);
    a.i32(DONE);
    a.struct_set(&fr, F_FLAGS);
    a.i32(1);
    a.get(f);
    a.struct_get(&fr, F_CHILD);
    a.s().ref_as_non_null();
    a.struct_get(&task_base(), F_FLAGS);
    a.i32(ABANDONED);
    a.s().i32_and().i32_eqz().i32_eqz();
    a.finish(vec![VT::I32, VT::I32])
}

/// Cancels every child of a race frame in list order, but `skip`, and
/// sets local `wait` when one is left unwinding.
fn cancel_list(a: &mut Asm, f: u32, fr: &WTy, list: &WTy, skip: Option<u32>, wait: u32) {
    let task = crate::layout::task_base();
    let (l, i, c) = (
        a.local(VT::r(list.clone())),
        a.local(VT::I32),
        a.local(VT::r(task.clone())),
    );
    a.i32(0);
    a.set(wait);
    a.get(f);
    a.struct_get(fr, F_SAVED - 1);
    a.s().ref_as_non_null();
    a.set(l);
    a.i32(0);
    a.set(i);
    a.block();
    a.loop_();
    a.get(i);
    a.get(l);
    a.struct_get(list, 0);
    a.s().i32_ge_u();
    a.br_if(1);
    let go = skip.map(|s| {
        a.get(i);
        a.get(s);
        a.s().i32_ne();
        a.if_();
    });
    a.get(l);
    a.struct_get(list, 1);
    a.get(i);
    a.array_get(&WTy::Array(storage(&VT::r(task.clone())).dflt()));
    a.ref_cast(&task, false);
    a.set(c);
    unwatched(a, |a| {
        a.get(c);
        a.get(c);
        a.struct_get(&task, F_CANCEL);
        a.call_ref(&cancel_fn());
    });
    a.get(c);
    a.struct_get(&task, F_FLAGS);
    a.i32(UNWINDING);
    a.s().i32_and();
    a.if_();
    a.i32(1);
    a.set(wait);
    a.end();
    if go.is_some() {
        a.end();
    }
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.set(i);
    a.br(0);
    a.end();
    a.end();
}

/// `race!`'s poll (`crates/hd_wasm/DESIGN.md` §14.5): polls the children in list order;
/// the first Ready wins, and every other child is cancelled in list order
/// before it returns. An empty list panics.
fn race_poll(list: &WTy, base: &WTy, poll: &WTy, result: &[VT]) -> Code {
    let fr = race_frame(list, base, result);
    let mut res = vec![VT::I32];
    res.extend(result.iter().map(VT::dflt));
    let mut a = Asm::new(vec![VT::Eq]);
    let (f, l, i, c, ready, wait) = (
        a.local(VT::r(fr.clone())),
        a.local(VT::r(list.clone())),
        a.local(VT::I32),
        a.local(VT::r(base.clone())),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    let tmp: Vec<u32> = result.iter().map(|v| a.local(v.dflt())).collect();
    let wake_count = a.local(VT::I32);
    let first = F_SAVED;
    a.get(0);
    a.ref_cast(&fr, false);
    a.set(f);
    a.global_get(wake_regs());
    a.set(wake_count);
    // The runtime checks; a cancelled race answers Pending
    // (`req.cancel.await`).
    a.get(f);
    a.call(Sym::Helper(Helper::PollCheck));
    a.if_();
    for v in &res {
        a.zero(v);
    }
    a.s().return_();
    a.end();
    a.get(f);
    a.struct_get(&fr, F_SAVED - 1);
    a.s().ref_as_non_null();
    a.set(l);
    // Settling: the winner's value waits in the frame while a loser's
    // shielded `dispose!` runs (flow.using.cancel). The losers' leaves
    // register under no node (`unwatched`), so a settling race registers
    // no wake either, and an `all!` polls it again.
    let settle = |a: &mut Asm| {
        a.get(wait);
        a.if_();
        a.get(wake_count);
        a.global_set(wake_regs());
        for v in &res {
            a.zero(v);
        }
        a.s().return_();
        a.end();
        a.get(f);
        a.i32(DONE);
        a.struct_set(&fr, F_FLAGS);
        a.i32(1);
        for (k, _) in result.iter().enumerate() {
            a.get(f);
            a.struct_get(&fr, first + u32::try_from(k).unwrap_or(0));
        }
        a.s().return_();
    };
    a.get(f);
    a.struct_get(&fr, F_STATE);
    a.if_();
    cancel_list(&mut a, f, &fr, list, None, wait);
    settle(&mut a);
    a.end();
    a.get(l);
    a.struct_get(list, 0);
    a.s().i32_eqz();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "explicit-panic: `race!` of an empty list".into(),
    )));
    a.s().unreachable();
    a.end();
    a.block();
    a.loop_();
    a.get(i);
    a.get(l);
    a.struct_get(list, 0);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(l);
    a.struct_get(list, 1);
    a.get(i);
    a.array_get(&WTy::Array(VT::Eq));
    a.ref_cast(base, false);
    a.set(c);
    a.get(c);
    a.get(c);
    a.struct_get(base, F_POLL);
    a.call_ref(poll);
    for t in tmp.iter().rev() {
        a.set(*t);
    }
    a.set(ready);
    a.get(ready);
    a.if_();
    for (k, t) in tmp.iter().enumerate() {
        a.get(f);
        a.raw_get(*t);
        a.struct_set(&fr, first + u32::try_from(k).unwrap_or(0));
    }
    a.get(f);
    a.i32(1);
    a.struct_set(&fr, F_STATE);
    cancel_list(&mut a, f, &fr, list, Some(i), wait);
    settle(&mut a);
    a.end();
    // Pending from a child that another task cancelled: its value never
    // comes, so the race is cancelled at once
    // (`req.combinator.race-child-cancelled`). Every child's cleanup goes
    // on, in list order, and the race unwinds while one waits
    // (`req.combinator.cancel-order`).
    a.get(c);
    a.struct_get(base, F_FLAGS);
    a.i32(CANCELLED);
    a.s().i32_and();
    a.if_();
    cancel_list(&mut a, f, &fr, list, None, wait);
    a.get(f);
    a.get(f);
    a.struct_get(&fr, F_FLAGS);
    a.i32(CANCELLED);
    a.s().i32_or();
    a.get(wait);
    a.i32(UNWINDING);
    a.s().i32_mul().i32_or();
    a.struct_set(&fr, F_FLAGS);
    a.get(wake_count);
    a.global_set(wake_regs());
    for v in &res {
        a.zero(v);
    }
    a.s().return_();
    a.end();
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.set(i);
    a.br(0);
    a.end();
    a.end();
    for v in &res {
        a.zero(v);
    }
    a.finish(res)
}

/// Pushes a host import's arguments (`crates/hd_run_wasmtime/DESIGN.md` §17.2): the
/// structured ones are encoded into the exchange buffer first, one after
/// another, and their total length is the last import parameter; a lone
/// string crosses as its bytes alone, its length the parameter; scalars
/// are parameters as themselves. `load(a, i, v)` pushes the `i`-th value
/// of the arguments, of type `v`. Returns the import's parameter types.
fn marshal(a: &mut Asm, args: &[ArgCodec], load: &dyn Fn(&mut Asm, u32, &VT)) -> Vec<VT> {
    let bufs = args.iter().any(|c| matches!(c, ArgCodec::Buf(_)));
    let off = bufs.then(|| a.local(VT::I32));
    let mut i = 0;
    for c in args {
        let vts = c.vts();
        if let (ArgCodec::Buf(b), Some(off)) = (c, off) {
            a.get(off);
            for (k, v) in vts.iter().enumerate() {
                load(a, i + u32::try_from(k).expect("k"), v);
            }
            a.call(Sym::Helper(Helper::Enc(b.clone())));
            a.set(off);
        }
        i += u32::try_from(vts.len()).expect("vts");
    }
    let mut imp = Vec::new();
    i = 0;
    for c in args {
        let vts = c.vts();
        match c {
            ArgCodec::Str => {
                load(a, i, &vts[0]);
                load(a, i + 1, &vts[1]);
                a.call(Sym::Helper(Helper::StrToBuf));
                imp.push(VT::I32);
            }
            ArgCodec::DataI64(t) => {
                load(a, i, &vts[0]);
                a.struct_get(t, 0);
                imp.push(VT::I64);
            }
            ArgCodec::Scalar(v) => {
                load(a, i, v);
                imp.push(v.clone());
            }
            ArgCodec::Buf(_) => {}
        }
        i += u32::try_from(vts.len()).expect("vts");
    }
    if let Some(off) = off {
        a.get(off);
        imp.push(VT::I32);
    }
    imp
}

/// Decodes a result in the exchange buffer, whose length is in local `n`.
fn unmarshal(a: &mut Asm, result: &ResCodec, n: u32) {
    match result {
        ResCodec::Tags => {
            // An enum of tags (`crates/hd_run_wasmtime/DESIGN.md` §17.4): the variant
            // index, then a payload variant index when present.
            a.i32(0);
            a.mem8_load(0);
            a.get(n);
            a.i32(1);
            a.s().i32_gt_s();
            a.if_ty(BlockType::Result(wasm_encoder::ValType::I32));
            a.i32(0);
            a.mem8_load(1);
            a.else_();
            a.i32(0);
            a.end();
        }
        ResCodec::Buf(b) => {
            a.i32(0);
            a.call(Sym::Helper(Helper::Dec(b.clone())));
            a.s().drop();
        }
        ResCodec::DataI64(t) => {
            a.get(n);
            a.struct_new(t);
        }
        ResCodec::Void | ResCodec::Scalar(_) => {}
    }
}

/// A host method that never waits (`crates/hd_run_wasmtime/DESIGN.md` §17.2): one import
/// call, its arguments and result marshalled as `marshal` and `unmarshal`
/// do. The first `skip` parameters of `sig` are not arguments (a provider
/// method's receiver).
fn host_call(
    sig: &WTy,
    module: &str,
    method: &str,
    args: &[ArgCodec],
    result: &ResCodec,
    skip: u32,
) -> StageResult<Code> {
    let WTy::Func(params, results) = sig else {
        return unsupported("a host stub without a signature");
    };
    let mut a = Asm::new(params.clone());
    let imp = marshal(&mut a, args, &|a, i, _| a.get(skip + i));
    let out = result.import();
    a.call(Sym::Import {
        module: module.to_owned(),
        name: method.to_owned(),
        params: imp,
        results: out.clone(),
    });
    if !out.is_empty() {
        let n = a.local(out[0].clone());
        a.set(n);
        if let ResCodec::Scalar(_) = result {
            a.get(n);
        }
        unmarshal(&mut a, result, n);
    }
    Ok(a.finish(results.clone()))
}

/// The leaf frame's poll (`crates/hd_run_wasmtime/DESIGN.md` §17.2).
fn host_poll(
    frame: &WTy,
    module: &str,
    method: &str,
    args: &[ArgCodec],
    result: &ResCodec,
) -> StageResult<Code> {
    if matches!(result, ResCodec::Scalar(_) | ResCodec::DataI64(_)) {
        return unsupported("a waiting host method with a scalar result");
    }
    let mut res = vec![VT::I32];
    res.extend(result.vts().iter().map(VT::dflt));
    let nf = u32::try_from(frame.fields().len()).expect("fields");
    let mut a = Asm::new(vec![VT::Eq]);
    let (f, h, n, st) = (
        a.local(VT::r(frame.clone())),
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::I32),
    );
    let pending = |a: &mut Asm| {
        for v in &res {
            a.zero(v);
        }
        a.s().return_();
    };
    a.get(0);
    a.ref_cast(frame, false);
    a.set(f);
    a.get(f);
    a.struct_get(frame, F_FLAGS);
    a.i32(DONE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-invalid-state: a completed host call was polled".into(),
    )));
    a.s().unreachable();
    a.end();
    // A cancelled call answers Pending; its awaiter sees it `CANCELLED`
    // and is cancelled (`req.cancel.await`).
    a.get(f);
    a.struct_get(frame, F_FLAGS);
    a.i32(CANCELLED);
    a.s().i32_and();
    a.if_();
    pending(&mut a);
    a.end();
    a.get(f);
    a.struct_get(frame, nf - 1);
    a.set(h);
    a.get(h);
    a.i32(0);
    a.s().i32_lt_s();
    a.if_();
    let imp = marshal(&mut a, args, &|a, i, v| {
        a.get(f);
        a.struct_get(frame, F_SAVED - 1 + i);
        a.conv(&v.dflt(), v);
    });
    a.call(Sym::Import {
        module: module.to_owned(),
        name: format!("{method}.start"),
        params: imp,
        results: vec![VT::I32, VT::I32],
    });
    a.set(n);
    a.set(st);
    a.get(st);
    a.if_();
    a.get(f);
    a.get(n);
    a.struct_set(frame, nf - 1);
    a.get(n);
    a.call(Sym::Helper(Helper::WakeRegister));
    pending(&mut a);
    a.end();
    a.else_();
    a.get(h);
    a.call(Sym::Helper(Helper::WakeTake));
    a.s().i32_eqz();
    a.if_();
    a.get(h);
    a.call(Sym::Helper(Helper::WakeRegister));
    pending(&mut a);
    a.end();
    a.get(h);
    a.call(Sym::Import {
        module: module.to_owned(),
        name: format!("{method}.finish"),
        params: vec![VT::I32],
        results: vec![VT::I32],
    });
    a.set(n);
    a.end();
    a.get(f);
    a.i32(DONE);
    a.struct_set(frame, F_FLAGS);
    a.i32(1);
    unmarshal(&mut a, result, n);
    Ok(a.finish(res))
}

/// `(n)`: reads `n` handles (`i32`, little-endian) from the exchange
/// buffer and records each in the wake table under its slot (`crates/hd_wasm/DESIGN.md`
/// §14.4). The table grows to fit.
fn wake_mark(nodes: bool) -> Code {
    let arr = WTy::Array(VT::I32);
    let mut a = Asm::new(vec![VT::I32]);
    let (i, h, t, fresh) = (
        a.local(VT::I32),
        a.local(VT::I32),
        a.local(VT::rn(arr.clone())),
        a.local(VT::r(arr.clone())),
    );
    a.block();
    a.loop_();
    a.get(i);
    a.get(0);
    a.s().i32_ge_u();
    a.br_if(1);
    a.get(i);
    a.i32(4);
    a.s().i32_mul();
    a.s().i32_load(crate::asm::mem(0));
    a.set(h);
    a.global_get(wake_table());
    a.set(t);
    // Grow when the slot is past the end.
    a.raw_get(t);
    a.s().ref_is_null();
    a.if_();
    a.i32(16);
    a.array_new_default(&arr);
    a.set(t);
    a.end();
    a.get(h);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.s().i32_ge_u();
    a.if_();
    a.get(h);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.i32(2);
    a.s().i32_mul();
    a.i32(16);
    a.s().i32_add();
    a.array_new_default(&arr);
    a.set(fresh);
    a.get(fresh);
    a.i32(0);
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.i32(0);
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.array_copy(&arr, &arr);
    a.get(fresh);
    a.set(t);
    a.end();
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.get(h);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.get(h);
    a.array_set(&arr);
    a.raw_get(t);
    a.global_set(wake_table());
    if nodes {
        wake_walk(&mut a, h);
    }
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.set(i);
    a.br(0);
    a.end();
    a.end();
    a.finish(vec![])
}

/// Sets the wake bits of completed handle `h` (a local) through its
/// registration (`crates/hd_wasm/DESIGN.md` §14.4). A stale registration,
/// of an earlier handle in the same slot, is ignored.
fn wake_walk(a: &mut Asm, h: u32) {
    let node = wake_node();
    let tab = WTy::Array(VT::Eq);
    let (t, reg) = (a.local(VT::rn(tab.clone())), a.local(VT::rn(node.clone())));
    a.global_get(wakers_global());
    a.set(t);
    a.block();
    a.raw_get(t);
    a.s().ref_is_null();
    a.br_if(0);
    a.get(h);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.s().i32_ge_u();
    a.br_if(0);
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.get(h);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.array_get(&tab);
    a.ref_cast(&node, true);
    a.set(reg);
    a.raw_get(reg);
    a.s().ref_is_null();
    a.br_if(0);
    a.raw_get(reg);
    a.s().ref_as_non_null();
    a.struct_get(&node, 0);
    a.get(h);
    a.s().i32_ne();
    a.br_if(0);
    a.raw_get(reg);
    a.s().ref_as_non_null();
    a.struct_get(&node, 1);
    a.raw_get(reg);
    a.s().ref_as_non_null();
    a.struct_get(&node, 2);
    a.call(Sym::Helper(Helper::WakeUp));
    a.end();
}

/// `(node, slot)`: wakes child `slot` of waker node `node` (null: none):
/// sets its bit, then each node's slot in its parent's mask, up to the
/// top (`crates/hd_wasm/DESIGN.md` §14.4).
fn wake_up() -> Code {
    let node = wake_node();
    let mut a = Asm::new(vec![VT::Eq, VT::I32]);
    let n = a.local(VT::rn(node.clone()));
    a.block();
    a.loop_();
    a.raw_get(0);
    a.s().ref_is_null();
    a.br_if(1);
    a.raw_get(0);
    a.ref_cast(&node, false);
    a.set(n);
    a.raw_get(n);
    a.s().ref_as_non_null();
    a.raw_get(n);
    a.s().ref_as_non_null();
    a.struct_get(&node, 0);
    a.i32(1);
    a.raw_get(1);
    a.s().i32_shl().i32_or();
    a.struct_set(&node, 0);
    a.raw_get(n);
    a.s().ref_as_non_null();
    a.struct_get(&node, 2);
    a.s().local_set(1);
    a.raw_get(n);
    a.s().ref_as_non_null();
    a.struct_get(&node, 1);
    a.s().local_set(0);
    a.br(0);
    a.end();
    a.end();
    a.finish(vec![])
}

/// The code of [`Helper::WakeMark`], [`Helper::WakeRegister`] or
/// [`Helper::WakeAborted`]: with `nodes`, the `all!` waker-node
/// bookkeeping (`crates/hd_wasm/DESIGN.md` §14.4); else without it, for a
/// program that polls no `all!`, where every host leaf sits under no node.
#[must_use]
pub fn wake_nodes(h: &Helper, nodes: bool) -> Code {
    match h {
        Helper::WakeMark => wake_mark(nodes),
        Helper::WakeAborted => wake_aborted(nodes),
        _ => wake_register(nodes),
    }
}

/// [`Helper::WakeAborted`]: the entry polls its root again, and with
/// `nodes`, the aborted handle's registration is woken, so each `all!` on
/// the way polls the child that waited on it.
fn wake_aborted(nodes: bool) -> Code {
    let mut a = Asm::new(vec![VT::I32]);
    a.i32(1);
    a.global_set(rewake_global());
    if nodes {
        wake_walk(&mut a, 0);
    }
    a.finish(vec![])
}

/// `(h)`: host handle `h` went Pending under the current waker node and
/// slot (`crates/hd_wasm/DESIGN.md` §14.4): counts the registration, and
/// records `(h, node, slot)` under `h`'s slot when there is a node. Without
/// `nodes`, nothing.
fn wake_register(nodes: bool) -> Code {
    let node = wake_node();
    let tab = WTy::Array(VT::Eq);
    let mut a = Asm::new(vec![VT::I32]);
    if !nodes {
        return a.finish(vec![]);
    }
    let (t, fresh, s, v) = (
        a.local(VT::rn(tab.clone())),
        a.local(VT::r(tab.clone())),
        a.local(VT::I32),
        a.local(VT::Eq),
    );
    a.global_get(wake_regs());
    a.i32(1);
    a.s().i32_add();
    a.global_set(wake_regs());
    a.get(0);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.set(s);
    a.global_get(wakers_global());
    a.set(t);
    a.raw_get(t);
    a.s().ref_is_null();
    a.if_();
    // No table yet and no node: nothing to record.
    a.global_get(wake_cur_node());
    a.s().ref_is_null();
    a.if_();
    a.s().return_();
    a.end();
    a.i32(16);
    a.array_new_default(&tab);
    a.set(t);
    a.end();
    // Grow when the slot is past the end.
    a.get(s);
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.s().i32_ge_u();
    a.if_();
    a.get(s);
    a.i32(2);
    a.s().i32_mul();
    a.i32(16);
    a.s().i32_add();
    a.array_new_default(&tab);
    a.set(fresh);
    a.get(fresh);
    a.i32(0);
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.i32(0);
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.array_copy(&tab, &tab);
    a.get(fresh);
    a.set(t);
    a.end();
    // A leaf under no node clears a stale registration in its slot.
    a.global_get(wake_cur_node());
    a.s().ref_is_null();
    a.i32(0);
    a.s().i32_eq();
    a.if_();
    a.get(0);
    a.global_get(wake_cur_node());
    a.global_get(wake_cur_slot());
    a.struct_new(&node);
    a.set(v);
    a.end();
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.get(s);
    a.raw_get(v);
    a.array_set(&tab);
    a.raw_get(t);
    a.global_set(wakers_global());
    a.finish(vec![])
}

/// `(h) -> i32`: whether the wake table holds handle `h` (its full,
/// generation-tagged value); a taken handle's slot is cleared.
fn wake_take() -> Code {
    let arr = WTy::Array(VT::I32);
    let mut a = Asm::new(vec![VT::I32]);
    let (t, s) = (a.local(VT::rn(arr.clone())), a.local(VT::I32));
    a.global_get(wake_table());
    a.set(t);
    a.get(0);
    a.i32(0xFFFFF);
    a.s().i32_and();
    a.set(s);
    a.raw_get(t);
    a.s().ref_is_null();
    a.if_();
    a.i32(0);
    a.s().return_();
    a.end();
    a.get(s);
    a.raw_get(t);
    a.s().ref_as_non_null().array_len();
    a.s().i32_ge_u();
    a.if_();
    a.i32(0);
    a.s().return_();
    a.end();
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.get(s);
    a.array_get(&arr);
    a.get(0);
    a.s().i32_ne();
    a.if_();
    a.i32(0);
    a.s().return_();
    a.end();
    a.raw_get(t);
    a.s().ref_as_non_null();
    a.get(s);
    a.i32(0);
    a.array_set(&arr);
    a.i32(1);
    a.finish(vec![VT::I32])
}

impl Helper {
    /// The helper's name in the `name` section: its kind in snake case and,
    /// for a host call, the method it reaches. Never a `Debug` dump; the
    /// caller adds an index when two helpers share a name.
    pub(crate) fn short_name(&self) -> String {
        let plain = |s: &str| -> String {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
                .take(24)
                .collect()
        };
        match self {
            Helper::Lit => "lit".into(),
            Helper::IntToStr { wide, signed } => format!(
                "int_to_str({}{})",
                if *signed { "i" } else { "u" },
                if *wide { 64 } else { 32 }
            ),
            Helper::StrEq => "str_eq".into(),
            Helper::StrToBuf => "str_to_buf".into(),
            Helper::Panic(_) => "panic".into(),
            Helper::PanicStr => "panic_str".into(),
            Helper::ListStep { .. } => "list_step".into(),
            Helper::MapHash { .. } => "map_hash".into(),
            Helper::MapFind { .. } => "map_find".into(),
            Helper::MapPut { .. } => "map_put".into(),
            Helper::MapGrow { .. } => "map_grow".into(),
            Helper::MapStep { .. } => "map_step".into(),
            Helper::HostCold { poll, .. } => match &**poll {
                Helper::HostPoll { module, method, .. } => {
                    format!("host_cold({}.{})", plain(module), plain(method))
                }
                _ => "host_cold".into(),
            },
            Helper::HostPoll { module, method, .. } => {
                format!("host_poll({}.{})", plain(module), plain(method))
            }
            Helper::HostCall { module, method, .. } => {
                format!("host_call({}.{})", plain(module), plain(method))
            }
            Helper::HostPrim { name, .. } => format!("host_prim({})", plain(name)),
            Helper::Enc(_) => "enc".into(),
            Helper::Dec(_) => "dec".into(),
            Helper::LebPut => "leb_put".into(),
            Helper::LebGet => "leb_get".into(),
            Helper::PowF64 => "pow_f64".into(),
            Helper::RemF64 => "rem_f64".into(),
            Helper::Fit => "fit".into(),
            Helper::HostCancel { .. } => "host_cancel".into(),
            Helper::Unlowered { .. } => "unlowered".into(),
            Helper::HandleSlot { inner, .. } => format!("handle_slot({})", inner.short_name()),
            Helper::RuntimeType { .. } => "runtime_type".into(),
            Helper::TypeIdOf { name, .. } => format!("type_id({})", plain(name)),
            Helper::WakeMark => "wake_mark".into(),
            Helper::WakeTake => "wake_take".into(),
            Helper::EraseI32 => "erase_i32".into(),
            Helper::UneraseI32 => "unerase_i32".into(),
            Helper::ForbidCheck => "forbid_check".into(),
            Helper::CancelEnter => "cancel_enter".into(),
            Helper::CancelCheck => "cancel_check".into(),
            Helper::CancelLeave => "cancel_leave".into(),
            Helper::PollCheck => "poll_check".into(),
            Helper::WakeRegister => "wake_register".into(),
            Helper::WakeUp => "wake_up".into(),
            Helper::WakeAborted => "wake_aborted".into(),
            Helper::RaceCold { .. } => "race_cold".into(),
            Helper::RacePoll { .. } => "race_poll".into(),
            Helper::RaceCancel { .. } => "race_cancel".into(),
            Helper::CleanupCold { .. } => "cleanup_cold".into(),
            Helper::CleanupPoll { .. } => "cleanup_poll".into(),
            Helper::CleanupCancel { .. } => "cleanup_cancel".into(),
            Helper::PendingCold { .. } => "pending_cold".into(),
            Helper::PendingPoll { .. } => "pending_poll".into(),
            Helper::PendingCancel => "pending_cancel".into(),
            Helper::Adapter { .. } => "adapter".into(),
            Helper::EntryInit { .. } => "entry_init".into(),
            Helper::EntryPoll { .. } => "entry_poll".into(),
            Helper::EntryWake => "entry_wake".into(),
            Helper::EntryCancel => "entry_cancel".into(),
            Helper::EntryAbandon => "entry_abandon".into(),
            Helper::EntryCall { .. } => "entry_call".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ABANDONED, CANCELLED, CANCELLED_END, F_FLAGS, Helper, UNWINDING, helper_code, owner_global,
        rewake_global, root_global, str_eq_same_view,
    };
    use crate::asm::Asm;
    use crate::{Reloc, Sym, VT};
    use hd_base::Hash128;

    /// Compare the post-poll instructions and relocations, not padded symbol
    /// slots alone: pending must keep the root, ready must release it.
    fn assert_poll_tail(report: bool) {
        let results = if report {
            vec![VT::I64, VT::Eq]
        } else {
            vec![]
        };
        let (base, poll) = crate::layout::suspend_base(&results);
        let report_key = Hash128(2);
        let code = helper_code(&Helper::EntryPoll {
            main: Hash128(1),
            providers: vec![],
            results: u32::try_from(results.len()).unwrap(),
            bang: Some((base.clone(), poll.clone())),
            report: report.then_some((report_key, results.clone())),
            then: vec![],
        })
        .unwrap();
        let poll_at = code
            .relocs
            .iter()
            .find_map(|(offset, reloc)| {
                (*reloc == Reloc::Type(poll.clone()) && code.body[*offset as usize - 1] == 0x14)
                    .then_some(*offset + 5)
            })
            .expect("root call_ref");
        let mut expected = Asm::new(vec![VT::I32]);
        let root = expected.local(VT::r(base.clone()));
        // The poll's results, kept while an `abandon()` asks for another
        // poll (`rewake`).
        let mut prs = vec![VT::I32];
        prs.extend(results.iter().cloned());
        let got: Vec<u32> = prs.iter().map(|v| expected.local(v.clone())).collect();
        for l in got.iter().rev() {
            expected.set(*l);
        }
        expected.end();
        expected.get(got[0]);
        expected.s().i32_eqz();
        expected.global_get(rewake_global());
        expected.s().i32_and();
        expected.br_if(0);
        expected.end();
        // A cancelled root whose cleanup ended is released and ends the
        // run cancelled, -2 or -3 (`req.entry.cancelled`).
        expected.get(root);
        expected.struct_get(&base, F_FLAGS);
        expected.i32(CANCELLED);
        expected.s().i32_and();
        expected.if_();
        expected.get(root);
        expected.struct_get(&base, F_FLAGS);
        expected.i32(UNWINDING);
        expected.s().i32_and().i32_eqz();
        expected.if_();
        expected.i32(CANCELLED_END);
        expected.get(root);
        expected.struct_get(&base, F_FLAGS);
        expected.i32(ABANDONED);
        expected.s().i32_and();
        expected.i32(0);
        expected.s().i32_ne().i32_sub();
        expected.ref_null(&base);
        expected.global_set(root_global());
        expected.i32(0);
        expected.global_set(owner_global());
        expected.s().return_();
        expected.end();
        expected.end();
        for l in &got {
            expected.raw_get(*l);
        }
        if report {
            let first = expected.local(VT::I64);
            let second = expected.local(VT::Eq);
            let ready = expected.local(VT::I32);
            let status = expected.local(VT::I32);
            expected.set(second);
            expected.set(first);
            expected.set(ready);
            expected.i32(-1);
            expected.set(status);
            expected.get(ready);
            expected.if_();
            expected.ref_null(&base);
            expected.global_set(root_global());
            expected.i32(0);
            expected.global_set(owner_global());
            expected.get(first);
            expected.get(second);
            expected.call(Sym::Inst(report_key));
            expected.set(status);
            expected.end();
            expected.get(status);
        } else {
            let ready = expected.local(VT::I32);
            expected.set(ready);
            expected.get(ready);
            expected.if_();
            expected.ref_null(&base);
            expected.global_set(root_global());
            expected.i32(0);
            expected.global_set(owner_global());
            expected.end();
            expected.get(ready);
            expected.i32(1);
            expected.s().i32_sub();
        }
        expected.end();
        assert_eq!(code.body[poll_at as usize..], expected.out);
        let tail_relocs: Vec<_> = code
            .relocs
            .iter()
            .filter(|(offset, _)| *offset >= poll_at)
            .map(|(offset, reloc)| (*offset - poll_at, reloc.clone()))
            .collect();
        assert_eq!(tail_relocs, expected.relocs);
        assert_eq!(code.results, vec![VT::I32]);
    }

    #[test]
    fn completed_void_entry_releases_root_but_pending_entry_retains_it() {
        assert_poll_tail(false);
    }

    /// `hd.poll` takes the driver's context, claims the root for it on
    /// the first poll, and panics `suspension-competing-driver` for a
    /// foreign driver of a live root.
    #[test]
    fn entry_poll_claims_the_root_for_its_context() {
        let (base, poll) = crate::layout::suspend_base(&[]);
        let code = helper_code(&Helper::EntryPoll {
            main: Hash128(1),
            providers: vec![],
            results: 0,
            bang: Some((base, poll)),
            report: None,
            then: vec![],
        })
        .unwrap();
        assert_eq!(code.params, vec![VT::I32]);
        assert!(
            code.relocs.iter().any(|(_, r)| matches!(
                r,
                Reloc::Func(Sym::Helper(Helper::Panic(msg)))
                    if msg.starts_with("suspension-competing-driver")
            )),
            "{code:?}"
        );
        assert!(
            code.relocs
                .iter()
                .any(|(_, r)| matches!(r, Reloc::Global(g) if *g == owner_global())),
            "{code:?}"
        );
    }

    #[test]
    fn completed_reported_entry_releases_root_before_reporting_its_results() {
        assert_poll_tail(true);
    }

    /// `hd.cancel` calls the pending root's cancel function through its
    /// `$Task` prefix, and returns at once when no root is pending.
    #[test]
    fn entry_cancel_calls_the_root_cancel_or_nothing() {
        use super::F_CANCEL;
        use crate::layout::{cancel_fn, task_base};

        let code = helper_code(&Helper::EntryCancel).unwrap();
        let base = task_base();
        let mut expected = Asm::new(vec![]);
        let root = expected.local(VT::rn(base.clone()));
        expected.global_get(root_global());
        expected.set(root);
        expected.get(root);
        expected.s().ref_is_null();
        expected.if_();
        expected.s().return_();
        expected.end();
        expected.get(root);
        expected.get(root);
        expected.struct_get(&base, F_CANCEL);
        expected.call_ref(&cancel_fn());
        let expected = expected.finish(vec![]);
        assert_eq!(code.body, expected.body);
        assert_eq!(code.relocs, expected.relocs);
        assert_eq!(code.results, expected.results);
    }

    /// `hd.abandon` calls the same cancel function with the `abandon()`
    /// depth raised around it, and returns at once when no root is pending.
    #[test]
    fn entry_abandon_raises_the_abandon_depth_around_the_cancel() {
        use super::{F_CANCEL, abandoning_global};
        use crate::layout::{cancel_fn, task_base};

        let code = helper_code(&Helper::EntryAbandon).unwrap();
        let base = task_base();
        let mut expected = Asm::new(vec![]);
        let root = expected.local(VT::rn(base.clone()));
        expected.global_get(root_global());
        expected.set(root);
        expected.get(root);
        expected.s().ref_is_null();
        expected.if_();
        expected.s().return_();
        expected.end();
        for by in [1, -1] {
            if by < 0 {
                expected.get(root);
                expected.get(root);
                expected.struct_get(&base, F_CANCEL);
                expected.call_ref(&cancel_fn());
            }
            expected.global_get(abandoning_global());
            expected.i32(by);
            expected.s().i32_add();
            expected.global_set(abandoning_global());
        }
        let expected = expected.finish(vec![]);
        assert_eq!(code.body, expected.body);
        assert_eq!(code.relocs, expected.relocs);
    }

    /// The entry's cancel and abandon helpers round-trip through the code
    /// cache.
    #[test]
    fn entry_cancel_encodes_and_decodes() {
        use super::{Reader, Writer};

        for helper in [Helper::EntryCancel, Helper::EntryAbandon] {
            let mut w = Writer::default();
            helper.encode(&mut w);
            let mut r = Reader::new(&w.bytes);
            assert_eq!(Helper::decode(&mut r), Some(helper));
        }
    }

    /// The scenario cleanup call round-trips through the code cache.
    #[test]
    fn entry_call_encodes_and_decodes() {
        use super::{Reader, Writer};

        let helper = Helper::EntryCall { func: Hash128(7) };
        let mut w = Writer::default();
        helper.encode(&mut w);
        let mut r = Reader::new(&w.bytes);
        assert_eq!(Helper::decode(&mut r), Some(helper));
    }

    /// A string compared with itself returns before the byte loop: the
    /// helper's first instructions are the `ref.eq` and span check, and
    /// its loops come after them.
    #[test]
    fn str_eq_returns_before_the_byte_loop_on_one_view() {
        let code = helper_code(&Helper::StrEq).unwrap();
        let body = wasmparser::FunctionBody::new(wasmparser::BinaryReader::new(&code.body, 0));
        let mut ops = body.get_operators_reader().unwrap();
        let start = usize::try_from(ops.original_position()).unwrap();
        let mut fast = Asm::new(vec![]);
        str_eq_same_view(&mut fast);
        assert!(fast.out.contains(&0xd3), "ref.eq");
        assert!(code.body[start..].starts_with(&fast.out));
        let mut loops = Vec::new();
        while !ops.eof() {
            let at = usize::try_from(ops.original_position()).unwrap();
            if matches!(ops.read().unwrap(), wasmparser::Operator::Loop { .. }) {
                loops.push(at);
            }
        }
        // The eight-byte loop, then the tail's.
        assert_eq!(loops.len(), 2);
        assert!(loops.iter().all(|l| *l >= start + fast.out.len()));
    }
}
