//! Runtime helpers the compiler builds (runtime-and-host.md §16.1,
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
    ACTIVE, CANCELLED, DONE, F_CANCEL, F_FLAGS, F_POLL, F_SAVED, F_STATE, cancel_fn, storage,
};
use crate::{Code, GSym, Sym, VT, WTy, decode_vts, encode_vts, unsupported};

/// How a host method's argument crosses (runtime-and-host.md §17.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArgCodec {
    /// A string: its viewed bytes into the exchange buffer, then the length.
    Str,
    /// A data value of one `i64` field (`Duration`): the field, as a scalar.
    DataI64(WTy),
    /// A scalar, as itself.
    Scalar(VT),
    /// A structured value, encoded into the exchange buffer after the
    /// arguments before it (runtime-and-host.md §17.4).
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
            2 => ArgCodec::Scalar(decode_vts(r, 0)?.pop()?),
            3 => ArgCodec::Buf(BTy::decode(r)?),
            _ => return None,
        })
    }
}

/// How a host method's result crosses (runtime-and-host.md §17.2).
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
            1 => ResCodec::Scalar(decode_vts(r, 0)?.pop()?),
            2 => ResCodec::DataI64(dec_wty(r)?),
            3 => ResCodec::Tags,
            4 => ResCodec::Buf(BTy::decode(r)?),
            _ => return None,
        })
    }
}

/// The wake table (suspension.md §14.4): completed host handles by slot.
#[must_use]
pub fn wake_table() -> GSym {
    GSym::Rt("wake".into(), VT::rn(WTy::Array(VT::I32)))
}

/// The forbidden-context counter (suspension.md §14.9): how many forbidden
/// contexts are running; `block_on` panics while it is not zero.
#[must_use]
pub fn forbid_global() -> GSym {
    GSym::Rt("forbid".into(), VT::I32)
}

/// The entry's root suspension, for `main!` (suspension.md §14.4).
#[must_use]
pub fn root_global() -> GSym {
    GSym::Rt("root".into(), VT::rn(crate::layout::task_base()))
}

/// The host runtime's `abort` import (runtime-and-host.md §17.2).
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

/// How a map hashes and compares its key (codegen.md §13.12).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyOps {
    /// An integer, `bool` or `char` in one `i32`.
    I32,
    /// A 64-bit integer.
    I64,
    /// A string: its viewed bytes.
    Str,
    /// Calls of the key type's `hash_of` and `Eq.eq` instances, which take
    /// the key's values as `params`.
    Call {
        hash: Box<Sym>,
        eq: Box<Sym>,
        params: Vec<VT>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// A pooled string literal's getter (wasm-layout.md §15.4).
    Lit(Vec<u8>),
    /// `Display.to_string` of an integer: a view over a fresh buffer.
    IntToStr { wide: bool, signed: bool },
    /// String `==` over the viewed bytes.
    StrEq,
    /// Copies a string's viewed bytes to the exchange buffer; its length.
    StrToBuf,
    /// A panic stub with a fixed message (wasm-layout.md §15.5).
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
        /// The environment `{code, map, pos, end, len}`, which names the
        /// code and map types.
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
    /// (runtime-and-host.md §17.2).
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
    /// `name`, the erased value's recorded type.
    RuntimeType { sig: WTy, ty: WTy, name: String },
    /// `(n)`: marks the `n` handles in the exchange buffer as completed.
    WakeMark,
    /// `(h) -> i32`: whether handle `h` completed; clears its slot.
    WakeTake,
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
    /// A vtable slot over a concrete impl method: unerases the receiver.
    Adapter {
        sig: WTy,
        self_vts: Vec<VT>,
        target: Box<Sym>,
        params: Vec<VT>,
        results: Vec<VT>,
    },
    /// `hd.init`: every reachable group's init, in order.
    EntryInit { inits: Vec<Hash128> },
    /// `hd.poll`: builds the default profile's providers and runs `main`,
    /// or polls `main!`'s root (`bang` is its `$Suspend_L`); `-1` while
    /// pending, else the exit status.
    EntryPoll {
        main: Hash128,
        providers: Vec<(WTy, Vec<Helper>)>,
        results: u32,
        bang: Option<(WTy, WTy)>,
        /// The `std.rt` result function's instance, which turns the
        /// root's result into the exit status; `None` for `void`.
        report: Option<(Hash128, Vec<VT>)>,
    },
    /// `hd.wake(n)`: marks the completed handles.
    EntryWake,
}

fn enc_wty(t: &WTy, w: &mut Writer) {
    encode_vts(&[VT::r(t.clone())], w);
}

fn dec_wty(r: &mut Reader<'_>) -> Option<WTy> {
    match decode_vts(r, 0)?.pop()? {
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
            KeyOps::I32 => w.u8(0),
            KeyOps::I64 => w.u8(1),
            KeyOps::Str => w.u8(2),
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
            0 => KeyOps::I32,
            1 => KeyOps::I64,
            2 => KeyOps::Str,
            _ => KeyOps::Call {
                hash: Box::new(Sym::decode(r)?),
                eq: Box::new(Sym::decode(r)?),
                params: decode_vts(r, 0)?,
            },
        })
    }
}

impl Helper {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Helper::Lit(b) => {
                w.u8(0);
                w.blob(b);
            }
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
                env,
                comps,
                tuple,
                opt,
            } => {
                w.u8(24);
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
            Helper::RuntimeType { sig, ty, name } => {
                w.u8(25);
                enc_wty(sig, w);
                enc_wty(ty, w);
                w.str(name);
            }
            Helper::WakeMark => w.u8(13),
            Helper::WakeTake => w.u8(14),
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
            Helper::Adapter {
                sig,
                self_vts,
                target,
                params,
                results,
            } => {
                w.u8(9);
                enc_wty(sig, w);
                encode_vts(self_vts, w);
                target.encode(w);
                encode_vts(params, w);
                encode_vts(results, w);
            }
            Helper::EntryInit { inits } => {
                w.u8(18);
                w.len_of(inits);
                for k in inits {
                    w.hash(*k);
                }
            }
            Helper::EntryPoll {
                main,
                providers,
                results,
                bang,
                report,
            } => {
                w.u8(10);
                w.hash(*main);
                w.len_of(providers);
                for (vt, slots) in providers {
                    enc_wty(vt, w);
                    w.len_of(slots);
                    for s in slots {
                        s.encode(w);
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
            }
            Helper::EntryWake => w.u8(19),
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Helper> {
        Some(match r.u8() {
            0 => Helper::Lit(r.blob().to_vec()),
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
                elem: decode_vts(r, 0)?,
                opt: dec_opt(r)?,
            },
            20 => Helper::MapHash {
                key: decode_vts(r, 0)?,
                ops: KeyOps::decode(r)?,
            },
            21 => Helper::MapFind {
                map: dec_wty(r)?,
                key: decode_vts(r, 0)?,
                ops: KeyOps::decode(r)?,
            },
            22 => Helper::MapPut {
                map: dec_wty(r)?,
                key: decode_vts(r, 0)?,
                val: decode_vts(r, 0)?,
                ops: KeyOps::decode(r)?,
            },
            23 => Helper::MapGrow {
                map: dec_wty(r)?,
                comps: decode_vts(r, 0)?,
            },
            24 => Helper::MapStep {
                env: dec_wty(r)?,
                comps: decode_vts(r, 0)?,
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
            25 => Helper::RuntimeType {
                sig: dec_wty(r)?,
                ty: dec_wty(r)?,
                name: r.str().to_owned(),
            },
            13 => Helper::WakeMark,
            14 => Helper::WakeTake,
            t @ 15..=17 => {
                let (list, base, poll, result) =
                    (dec_wty(r)?, dec_wty(r)?, dec_wty(r)?, decode_vts(r, 0)?);
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
            18 => Helper::EntryInit {
                inits: (0..r.count()).map(|_| r.hash()).collect(),
            },
            19 => Helper::EntryWake,
            9 => Helper::Adapter {
                sig: dec_wty(r)?,
                self_vts: decode_vts(r, 0)?,
                target: Box::new(Sym::decode(r)?),
                params: decode_vts(r, 0)?,
                results: decode_vts(r, 0)?,
            },
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
                    providers.push((vt, slots));
                }
                let results = r.u32();
                let bang = if r.u8() == 1 {
                    Some((dec_wty(r)?, dec_wty(r)?))
                } else {
                    None
                };
                let report = if r.u8() == 1 {
                    Some((r.hash(), decode_vts(r, 0)?))
                } else {
                    None
                };
                Helper::EntryPoll {
                    main,
                    providers,
                    results,
                    bang,
                    report,
                }
            }
            _ => return None,
        })
    }
}

/// The host runtime's stderr import (runtime-and-host.md §17.2).
#[must_use]
pub fn stderr_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "stderr".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// The host runtime's `block` import (suspension.md §14.9).
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

/// A literal getter, once link knows its segment offset and cell global.
#[must_use]
pub fn lit_code(off: u32, len: u32, global: u32) -> Code {
    let mut a = Asm::new(vec![]);
    a.s().global_get(global).ref_is_null();
    a.if_();
    a.i32(off.cast_signed());
    a.i32(len.cast_signed());
    a.array_new_data(&WTy::Bytes, 0);
    a.s().global_set(global);
    a.end();
    a.s().global_get(global).ref_as_non_null();
    a.finish(vec![VT::r(WTy::Bytes)])
}

/// The code of a helper.
pub fn helper_code(h: &Helper) -> StageResult<Code> {
    Ok(match h {
        Helper::Lit(_) => lit_code(0, 0, 0),
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
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(0);
            a.get(1);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.get(2);
            a.get(3);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
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
            // Grow the exchange buffer to fit (runtime-and-host.md §17.3).
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
            write_lit(&mut a, &format!("panic: {msg}\n"));
            a.s().unreachable();
            a.finish(vec![])
        }
        Helper::PanicStr => {
            let mut a = Asm::new(str_vts());
            write_lit(&mut a, "panic: explicit-panic: ");
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
            env,
            comps,
            tuple,
            opt,
        } => crate::map::step_code(env, comps, tuple.as_ref(), opt)?,
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
        Helper::RuntimeType { sig, ty, name } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("a `runtime_type` slot without a signature");
            };
            let mut a = Asm::new(params.clone());
            push_type_id(&mut a, ty, name);
            a.finish(results.clone())
        }
        Helper::WakeMark => wake_mark(),
        Helper::WakeTake => wake_take(),
        Helper::RaceCold {
            list,
            base,
            poll,
            result,
        } => {
            let fr = race_frame(list, base);
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
        Helper::RaceCancel { list, base, .. } => {
            let fr = race_frame(list, base);
            let mut a = Asm::new(vec![VT::Eq]);
            let f = a.local(VT::r(fr.clone()));
            a.get(0);
            a.ref_cast(&fr, false);
            a.set(f);
            a.get(f);
            a.struct_get(&fr, F_FLAGS);
            a.i32(DONE | CANCELLED);
            a.s().i32_and();
            a.if_();
            a.s().return_();
            a.end();
            a.get(f);
            a.i32(CANCELLED);
            a.struct_set(&fr, F_FLAGS);
            cancel_list(&mut a, f, &fr, list, None);
            a.finish(vec![])
        }
        Helper::Adapter {
            sig,
            self_vts,
            target,
            params,
            results,
        } => {
            let WTy::Func(sp, sr) = sig else {
                return unsupported("an adapter without a signature");
            };
            let mut a = Asm::new(sp.clone());
            unerase(&mut a, 0, self_vts);
            for (k, (j, p)) in (self_vts.len()..).zip(sp.iter().enumerate().skip(1)) {
                a.get(u32::try_from(j).expect("j"));
                if let Some(want) = params.get(k) {
                    a.conv(p, want);
                }
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
            for k in inits {
                a.call(Sym::Inst(*k));
            }
            a.finish(vec![])
        }
        Helper::EntryPoll {
            main,
            providers,
            results,
            bang,
            report,
        } => {
            let mut a = Asm::new(vec![]);
            let build = |a: &mut Asm| {
                for (vt, slots) in providers {
                    a.ref_null_eq();
                    for s in slots {
                        a.ref_func(Sym::Helper(s.clone()));
                    }
                    a.struct_new(vt);
                }
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
                    a.end();
                    a.global_get(root_global());
                    a.ref_cast(base, false);
                    a.set(root);
                    a.get(root);
                    a.get(root);
                    a.struct_get(base, F_POLL);
                    a.call_ref(poll);
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
    })
}

/// Writes a fixed text to standard error through the runtime import.
pub(crate) fn write_lit(a: &mut Asm, text: &str) {
    a.call(Sym::Helper(Helper::Lit(text.as_bytes().to_vec())));
    a.i64(i64::try_from(text.len()).unwrap_or(0) << 32);
    a.call(Sym::Helper(Helper::StrToBuf));
    a.call(stderr_import());
}

/// Pushes a `TypeId`, the std data type `st` over its key string, whose
/// key is `name` (trait.typeid.name).
pub fn push_type_id(a: &mut Asm, st: &WTy, name: &str) {
    a.call(Sym::Helper(Helper::Lit(name.as_bytes().to_vec())));
    a.i64(i64::try_from(name.len()).unwrap_or(0) << 32);
    a.struct_new(st);
}

/// Unerases parameter `p` (an `eqref`) to `vts`: one reference is cast,
/// anything else is read out of its box.
pub fn unerase(a: &mut Asm, p: u32, vts: &[VT]) {
    match vts {
        [VT::Eq] => a.get(p),
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

/// `race!`'s frame: a suspension holding its list of children.
fn race_frame(list: &WTy, base: &WTy) -> WTy {
    crate::layout::frame_of(base, &[VT::rn(list.clone())])
}

/// Cancels every child of a race frame in list order, but `skip`.
fn cancel_list(a: &mut Asm, f: u32, fr: &WTy, list: &WTy, skip: Option<u32>) {
    let task = crate::layout::task_base();
    let (l, i, c) = (
        a.local(VT::r(list.clone())),
        a.local(VT::I32),
        a.local(VT::r(task.clone())),
    );
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
    a.get(c);
    a.get(c);
    a.struct_get(&task, F_CANCEL);
    a.call_ref(&cancel_fn());
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

/// `race!`'s poll (suspension.md §14.5): polls the children in list order;
/// the first Ready wins, and every other child is cancelled in list order
/// before it returns. An empty list panics.
fn race_poll(list: &WTy, base: &WTy, poll: &WTy, result: &[VT]) -> Code {
    let fr = race_frame(list, base);
    let mut res = vec![VT::I32];
    res.extend(result.iter().map(VT::dflt));
    let mut a = Asm::new(vec![VT::Eq]);
    let (f, l, i, c, ready) = (
        a.local(VT::r(fr.clone())),
        a.local(VT::r(list.clone())),
        a.local(VT::I32),
        a.local(VT::r(base.clone())),
        a.local(VT::I32),
    );
    let tmp: Vec<u32> = result.iter().map(|v| a.local(v.dflt())).collect();
    a.get(0);
    a.ref_cast(&fr, false);
    a.set(f);
    a.get(f);
    a.struct_get(&fr, F_FLAGS);
    a.i32(DONE | CANCELLED | ACTIVE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-invalid-state: a completed or cancelled `race!` was polled".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(f);
    a.struct_get(&fr, F_SAVED - 1);
    a.s().ref_as_non_null();
    a.set(l);
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
    cancel_list(&mut a, f, &fr, list, Some(i));
    a.get(f);
    a.i32(DONE);
    a.struct_set(&fr, F_FLAGS);
    a.i32(1);
    for t in &tmp {
        a.raw_get(*t);
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
    let _ = F_STATE;
    for v in &res {
        a.zero(v);
    }
    a.finish(res)
}

/// Pushes a host import's arguments (runtime-and-host.md §17.2): the
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
            // An enum of tags (runtime-and-host.md §17.4): the variant
            // index, then a payload variant index when present.
            a.i32(0);
            a.mem8_load(0);
            a.get(n);
            a.i32(1);
            a.s().i32_gt_s();
            a.s().if_(BlockType::Result(wasm_encoder::ValType::I32));
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

/// A host method that never waits (runtime-and-host.md §17.2): one import
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

/// The leaf frame's poll (runtime-and-host.md §17.2).
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
    a.i32(DONE | CANCELLED);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-invalid-state: a completed or cancelled host call was polled".into(),
    )));
    a.s().unreachable();
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
    pending(&mut a);
    a.end();
    a.else_();
    a.get(h);
    a.call(Sym::Helper(Helper::WakeTake));
    a.s().i32_eqz();
    a.if_();
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
/// buffer and records each in the wake table under its slot (suspension.md
/// §14.4). The table grows to fit.
fn wake_mark() -> Code {
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
    a.get(i);
    a.i32(1);
    a.s().i32_add();
    a.set(i);
    a.br(0);
    a.end();
    a.end();
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
