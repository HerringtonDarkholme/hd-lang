//! The stored form of an emitted unit (#2423): what one instance's
//! emission made ([`super::Emitted`]), as bytes a later link merges
//! instead of emitting the instance again, as wasm-ld merges an object
//! file. A unit names only its own tables ([`Symbols::unit`]), so the
//! bytes hold no id of a module or a run. Types go by index into a list
//! the caller encodes with the program's type tables
//! (`hd_types::wire`), as a TIR body's do.
//!
//! Each type's codec is a listing of its variants and fields
//! ([`wire_enum!`], [`wire_struct!`]): the encoder's `match` and the
//! decoder's constructors are exhaustive, so a new variant or field
//! that the listing lacks is a compile error. A variant's tag is its
//! number in the listing; the entry's version names the listing.
//! Cost: linear in the unit's code and tables, one byte per tag.

use std::collections::{BTreeMap, BTreeSet};

use hd_base::Hash128;
use hd_base::wire::{Reader, Writer};
use hd_types::Ty;
use wasm_encoder::{BlockType, Ieee32, Ieee64, MemArg, ValType};

use super::{
    Edit, Emitted, Func, Global, GlobalId, Import, Ins, Part, SigId, Static, Stored, Sym, SymId,
    Symbols, Unit,
};
use crate::op::{LabelListId, Op};
use crate::rt::Helper;

/// The writer of a unit: its bytes, and the types they name by index.
#[derive(Default)]
pub(crate) struct Out {
    w: Writer,
    tys: Vec<Ty>,
    ty_at: hd_base::HashMap<Ty, u32>,
    /// The unit names something the module numbers: it is not stored.
    unstorable: bool,
}

/// The reader of a unit: its bytes and its types, decoded by the caller.
pub(crate) struct In<'a> {
    r: Reader<'a>,
    tys: &'a [Ty],
}

/// A value's stored form.
pub(crate) trait Wire: Sized {
    fn put(&self, o: &mut Out);
    /// `None` when the bytes do not hold one: the unit is a miss.
    fn get(i: &mut In<'_>) -> Option<Self>;
}

/// The codec of a struct with named fields.
macro_rules! wire_struct {
    ($t:ty { $($f:ident),* $(,)? }) => {
        impl Wire for $t {
            fn put(&self, o: &mut Out) {
                let Self { $($f),* } = self;
                $( $f.put(o); )*
            }
            fn get(i: &mut In<'_>) -> Option<Self> {
                Some(Self { $( $f: Wire::get(i)? ),* })
            }
        }
    };
}

/// The codec of an enum: each variant by its tag, then its fields.
macro_rules! wire_enum {
    ($t:ty { $( $tag:literal => $v:ident $( ( $($a:ident),+ ) )? $( { $($f:ident),+ } )? ),+ $(,)? }) => {
        impl Wire for $t {
            fn put(&self, o: &mut Out) {
                match self {
                    $( Self::$v $( ( $($a),+ ) )? $( { $($f),+ } )? => {
                        o.w.u8($tag);
                        $( $( $a.put(o); )+ )?
                        $( $( $f.put(o); )+ )?
                    } )+
                }
            }
            fn get(i: &mut In<'_>) -> Option<Self> {
                Some(match i.r.u8() {
                    $( $tag => Self::$v $( ( $( wire_field!(i, $a) ),+ ) )? $( { $( $f: Wire::get(i)? ),+ } )?, )+
                    _ => return None,
                })
            }
        }
    };
}

/// One tuple field's decoding, named by the listing for the encoder.
macro_rules! wire_field {
    ($i:ident, $a:ident) => {
        Wire::get($i)?
    };
}

impl Wire for u8 {
    fn put(&self, o: &mut Out) {
        o.w.u8(*self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.u8())
    }
}
impl Wire for bool {
    fn put(&self, o: &mut Out) {
        o.w.u8(u8::from(*self));
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        match i.r.u8() {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}
impl Wire for u16 {
    fn put(&self, o: &mut Out) {
        o.w.u32(u32::from(*self));
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        u16::try_from(i.r.u32()).ok()
    }
}
impl Wire for u32 {
    fn put(&self, o: &mut Out) {
        o.w.u32(*self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.u32())
    }
}
impl Wire for i32 {
    fn put(&self, o: &mut Out) {
        o.w.u32(self.cast_unsigned());
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.u32().cast_signed())
    }
}
impl Wire for u64 {
    fn put(&self, o: &mut Out) {
        o.w.u64(*self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.u64())
    }
}
impl Wire for i64 {
    fn put(&self, o: &mut Out) {
        o.w.u64(self.cast_unsigned());
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.u64().cast_signed())
    }
}
impl Wire for usize {
    fn put(&self, o: &mut Out) {
        o.w.u64(*self as u64);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        usize::try_from(i.r.u64()).ok()
    }
}
impl Wire for Hash128 {
    fn put(&self, o: &mut Out) {
        o.w.hash(*self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.hash())
    }
}
impl Wire for String {
    fn put(&self, o: &mut Out) {
        o.w.str(self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.str().to_owned())
    }
}
/// A compiler-written reason (`Reason::Conservative`): each distinct one
/// is kept once for the process, so their number is that of the literals
/// in the compiler, however many units are read.
impl Wire for &'static str {
    fn put(&self, o: &mut Out) {
        o.w.str(self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        static KEPT: std::sync::Mutex<BTreeSet<&'static str>> =
            std::sync::Mutex::new(BTreeSet::new());
        let s = i.r.str();
        let mut kept = KEPT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(k) = kept.get(s) {
            return Some(k);
        }
        let k: &'static str = Box::leak(s.to_owned().into_boxed_str());
        kept.insert(k);
        Some(k)
    }
}
impl Wire for Ty {
    fn put(&self, o: &mut Out) {
        let next = u32::try_from(o.tys.len()).unwrap_or(u32::MAX);
        let at = *o.ty_at.entry(*self).or_insert(next);
        if at == next {
            o.tys.push(*self);
        }
        o.w.u32(at);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        i.tys.get(i.r.u32() as usize).copied()
    }
}
impl<T: Wire> Wire for Box<T> {
    fn put(&self, o: &mut Out) {
        (**self).put(o);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        T::get(i).map(Box::new)
    }
}
impl<T: Wire> Wire for Box<[T]> {
    fn put(&self, o: &mut Out) {
        o.w.len_of(self);
        for x in self {
            x.put(o);
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Vec::<T>::get(i).map(Vec::into_boxed_slice)
    }
}
impl<T: Wire> Wire for Option<T> {
    fn put(&self, o: &mut Out) {
        match self {
            None => o.w.u8(0),
            Some(x) => {
                o.w.u8(1);
                x.put(o);
            }
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        match i.r.u8() {
            0 => Some(None),
            1 => T::get(i).map(Some),
            _ => None,
        }
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn put(&self, o: &mut Out) {
        o.w.len_of(self);
        for x in self {
            x.put(o);
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        let n = i.r.count();
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(T::get(i)?);
        }
        i.r.ok().then_some(v)
    }
}
impl<T: Wire + Ord> Wire for BTreeSet<T> {
    fn put(&self, o: &mut Out) {
        o.w.u32(u32::try_from(self.len()).unwrap_or(u32::MAX));
        for x in self {
            x.put(o);
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Vec::<T>::get(i).map(|v| v.into_iter().collect())
    }
}
impl<K: Wire + Ord, V: Wire> Wire for BTreeMap<K, V> {
    fn put(&self, o: &mut Out) {
        o.w.u32(u32::try_from(self.len()).unwrap_or(u32::MAX));
        for (k, v) in self {
            k.put(o);
            v.put(o);
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Vec::<(K, V)>::get(i).map(|v| v.into_iter().collect())
    }
}
impl<T: Wire, const N: usize> Wire for [T; N] {
    fn put(&self, o: &mut Out) {
        for x in self {
            x.put(o);
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        let mut v = Vec::with_capacity(N);
        for _ in 0..N {
            v.push(T::get(i)?);
        }
        v.try_into().ok()
    }
}
macro_rules! wire_tuple {
    ($($t:ident $x:ident),+) => {
        impl<$($t: Wire),+> Wire for ($($t,)+) {
            fn put(&self, o: &mut Out) {
                let ($($x,)+) = self;
                $( $x.put(o); )+
            }
            fn get(i: &mut In<'_>) -> Option<Self> {
                Some(($( wire_field!(i, $x), )+))
            }
        }
    };
}
wire_tuple!(A a, B b);
wire_tuple!(A a, B b, C c);
wire_tuple!(A a, B b, C c, D d);
wire_tuple!(A a, B b, C c, D d, E e);

// wasm-encoder's immediates that emission writes.
impl Wire for ValType {
    fn put(&self, o: &mut Out) {
        o.w.u8(match self {
            ValType::I32 => 0,
            ValType::I64 => 1,
            ValType::F32 => 2,
            ValType::F64 => 3,
            // The listing has the four scalars `hd_layout::Val` has: a
            // unit that names another type is emitted again each link.
            ValType::V128 | ValType::Ref(_) => {
                o.unstorable = true;
                4
            }
        });
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(match i.r.u8() {
            0 => ValType::I32,
            1 => ValType::I64,
            2 => ValType::F32,
            3 => ValType::F64,
            _ => return None,
        })
    }
}
wire_enum!(BlockType {
    0 => Empty,
    1 => Result(a),
    2 => FunctionType(a),
});
wire_struct!(MemArg {
    offset,
    align,
    memory_index
});
impl Wire for Ieee32 {
    fn put(&self, o: &mut Out) {
        o.w.u32(self.bits());
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(Ieee32::new(i.r.u32()))
    }
}
impl Wire for Ieee64 {
    fn put(&self, o: &mut Out) {
        o.w.u64(self.bits());
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(Ieee64::new(i.r.u64()))
    }
}
wire_enum!(hd_layout::Val {
    0 => I32,
    1 => I64,
    2 => F32,
    3 => F64,
});

/// The codec of a one-field tuple struct.
macro_rules! wire_newtype {
    ($t:ty) => {
        impl Wire for $t {
            fn put(&self, o: &mut Out) {
                self.0.put(o);
            }
            fn get(i: &mut In<'_>) -> Option<Self> {
                Some(Self(Wire::get(i)?))
            }
        }
    };
}
wire_newtype!(LabelListId);
wire_newtype!(SymId);
wire_newtype!(GlobalId);
wire_newtype!(SigId);

impl Wire for Box<str> {
    fn put(&self, o: &mut Out) {
        o.w.str(self);
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(i.r.str().into())
    }
}

wire_enum!(Op {
        0 => AtomicFence,
        1 => Block(a),
        2 => Br(a),
        3 => BrIf(a),
        4 => BrTable(a, b),
        5 => Drop,
        6 => Else,
        7 => End,
        8 => F32Abs,
        9 => F32Add,
        10 => F32Ceil,
        11 => F32Copysign,
        12 => F32Const(a),
        13 => F32ConvertI32S,
        14 => F32ConvertI32U,
        15 => F32ConvertI64S,
        16 => F32ConvertI64U,
        17 => F32DemoteF64,
        18 => F32Div,
        19 => F32Eq,
        20 => F32Floor,
        21 => F32Ge,
        22 => F32Gt,
        23 => F32Le,
        24 => F32Load(a),
        25 => F32Lt,
        26 => F32Mul,
        27 => F32Ne,
        28 => F32Nearest,
        29 => F32Neg,
        30 => F32ReinterpretI32,
        31 => F32Sqrt,
        32 => F32Store(a),
        33 => F32Sub,
        34 => F32Trunc,
        35 => F64Abs,
        36 => F64Add,
        37 => F64Ceil,
        38 => F64Copysign,
        39 => F64Const(a),
        40 => F64ConvertI32S,
        41 => F64ConvertI32U,
        42 => F64ConvertI64S,
        43 => F64ConvertI64U,
        44 => F64Div,
        45 => F64Eq,
        46 => F64Floor,
        47 => F64Ge,
        48 => F64Gt,
        49 => F64Le,
        50 => F64Load(a),
        51 => F64Lt,
        52 => F64Mul,
        53 => F64Ne,
        54 => F64Nearest,
        55 => F64Neg,
        56 => F64PromoteF32,
        57 => F64ReinterpretI64,
        58 => F64Sqrt,
        59 => F64Store(a),
        60 => F64Sub,
        61 => F64Trunc,
        62 => I32Add,
        63 => I32And,
        64 => I32Clz,
        65 => I32Const(a),
        66 => I32Ctz,
        67 => I32DivS,
        68 => I32DivU,
        69 => I32Eq,
        70 => I32Eqz,
        71 => I32Extend16S,
        72 => I32Extend8S,
        73 => I32GeS,
        74 => I32GeU,
        75 => I32GtS,
        76 => I32GtU,
        77 => I32LeS,
        78 => I32LeU,
        79 => I32AtomicLoad(a),
        80 => I32AtomicRmwAdd(a),
        81 => I32AtomicRmwCmpxchg(a),
        82 => I32AtomicRmwSub(a),
        83 => I32AtomicStore(a),
        84 => I32Load(a),
        85 => I32Load16S(a),
        86 => I32Load16U(a),
        87 => I32Load8S(a),
        88 => I32Load8U(a),
        89 => I32LtS,
        90 => I32LtU,
        91 => I32Mul,
        92 => I32Ne,
        93 => I32Or,
        94 => I32Popcnt,
        95 => I32ReinterpretF32,
        96 => I32RemS,
        97 => I32RemU,
        98 => I32Shl,
        99 => I32ShrS,
        100 => I32ShrU,
        101 => I32Store(a),
        102 => I32Store16(a),
        103 => I32Store8(a),
        104 => I32Sub,
        105 => I32TruncSatF32S,
        106 => I32TruncSatF32U,
        107 => I32TruncSatF64S,
        108 => I32TruncSatF64U,
        109 => I32WrapI64,
        110 => I32Xor,
        111 => I64Add,
        112 => I64And,
        113 => I64Clz,
        114 => I64Const(a),
        115 => I64Ctz,
        116 => I64DivS,
        117 => I64DivU,
        118 => I64Eq,
        119 => I64Eqz,
        120 => I64ExtendI32S,
        121 => I64ExtendI32U,
        122 => I64GeS,
        123 => I64GeU,
        124 => I64GtS,
        125 => I64GtU,
        126 => I64LeS,
        127 => I64LeU,
        128 => I64Load(a),
        129 => I64LtS,
        130 => I64LtU,
        131 => I64Mul,
        132 => I64Ne,
        133 => I64Or,
        134 => I64Popcnt,
        135 => I64ReinterpretF64,
        136 => I64RemS,
        137 => I64RemU,
        138 => I64Shl,
        139 => I64ShrS,
        140 => I64ShrU,
        141 => I64Store(a),
        142 => I64Sub,
        143 => I64TruncSatF32S,
        144 => I64TruncSatF32U,
        145 => I64TruncSatF64S,
        146 => I64TruncSatF64U,
        147 => I64Xor,
        148 => If(a),
        149 => LocalGet(a),
        150 => LocalSet(a),
        151 => LocalTee(a),
        152 => Loop(a),
        153 => MemoryCopy { src_mem, dst_mem },
        154 => MemoryFill(a),
        155 => MemoryGrow(a),
        156 => MemoryAtomicNotify(a),
        157 => MemoryAtomicWait32(a),
        158 => MemorySize(a),
        159 => Return,
        160 => Select,
        161 => Unreachable,
});

wire_enum!(Helper {
        0 => MemoryCopy { source, dest },
        1 => MemoryFill(a),
        2 => CanCancel,
        3 => CancellationRoot,
        4 => EntryInit { inits },
        5 => EntryPoll { main, providers, results, report, then, bang },
        6 => CheckedPoll { poll },
        7 => CollectPoll { poll },
        8 => TestPoll { polls, reinit },
        9 => TestInit { inits },
        10 => TestPollPart { polls },
        11 => TestInitWalk { inits, graph },
        12 => TestInitPart { inits },
        13 => EntryReset,
        14 => Teardown,
        15 => Collect,
        16 => CycleTrace,
        17 => CycleRelease,
        18 => CycleFree,
        19 => Opaque,
        20 => Panic(a),
        21 => PanicStr,
        22 => LoanShare,
        23 => LoanUnshare,
        24 => PanicWith,
        25 => ToBuf,
        26 => Alloc,
        27 => New,
        28 => NewFixed { size },
        29 => AllocClass { class, bytes },
        30 => FreeClass,
        31 => FreeClassLoaded,
        32 => FreeFrame,
        33 => Str,
        34 => ReleaseRef,
        35 => ReleaseDrain,
        36 => RetainKnown,
        37 => ReleaseKnown,
        38 => ReleaseFresh,
        39 => RetainColl,
        40 => RetainFields { leaves, parts },
        41 => ReleaseSpilled { size, leaves, shim },
        42 => DropParts { leaves, parts },
        43 => ListBuf { elem },
        44 => DropElems { elem },
        45 => CtxFind,
        46 => CtxNew,
        47 => RowAdapter(a),
        48 => CopyElems { elem },
        49 => ListUnique { elem },
        50 => ListCopy { elem },
        51 => ListGrow { elem },
        52 => MapSeed,
        53 => MapTable { entry },
        54 => DropEntries { entry },
        55 => MapHash { key, ops },
        56 => MapFind { entry, ops },
        57 => MapLookup { entry, ops, probe },
        58 => MapPut { entry, ops },
        59 => MapGrow { entry },
        60 => MapUnique { entry },
        61 => MapSlotNext,
        62 => DynSlot { target, retained_result, leaves, params, results, own, given, ctx },
        63 => DynRead { target, retained_result, read, params, results, own, given, ctx },
        64 => MapStep { entry, item },
        65 => ListTruncate { elem },
        66 => ListReserve { elem },
        67 => ListExtend { elem },
        68 => TypeIdOf(a),
        69 => TypeInfo(a),
        70 => RuntimeType(a),
        71 => DynTrap { params, results },
        72 => StrFromBytes,
        73 => StrToBytes { elem },
        74 => StrAppend,
        75 => FmtInt { signed },
        76 => StrEq,
        77 => BytesIndexSet(a),
        78 => BytesIndex,
        79 => BytesIndexPair,
        80 => BytesMismatch,
        81 => Utf8ValidLen,
        82 => HostPrim { name, args, result },
        83 => ForbidCheck,
        84 => HostCold { module, method, args, area, counts, res },
        85 => HostPoll { blocking, module, method, args, area, res },
        86 => HostCancel,
        87 => HostDrive { blocking, module, method, args, area, counts, res },
        88 => Drive { blocking, cold, params, results },
        89 => HostCall { module, method, args, result, owned },
        90 => HandleSlot { inner, params, ctx, results },
        91 => PackageHandleRelease { module },
        92 => Fit,
        93 => LebPut,
        94 => LebGet,
        95 => Enc(a),
        96 => Dec(a),
        97 => Frame(a),
        98 => FrameEnsure(a),
        99 => PollCheck,
        100 => CancelCheck,
        101 => CancelEnter,
        102 => CancelLeave,
        103 => CancelChild,
        104 => WakeMark,
        105 => WakeTake,
        106 => WakeSlot,
        107 => WakeRegister,
        108 => WakeRemove,
        109 => WakeSignal,
        110 => WakerNew,
        111 => WakerParent,
        112 => WakerSlot,
        113 => WakerLive,
        114 => WakerDrop,
        115 => WakeLink,
        116 => WakeNotify,
        117 => EntryCancel { abandon },
        118 => EntryCall { func },
        119 => RetainAny,
        120 => ChildDrop,
        121 => CleanupCold,
        122 => CleanupPoll,
        123 => CleanupCancel,
        124 => PendingCold { results },
        125 => PendingPoll { results },
        126 => PendingCancel,
        127 => WaitCell,
        128 => WaitCold,
        129 => WaitPoll,
        130 => WaitSignal,
        131 => IdleHook,
        132 => IdleWake,
        133 => ThreadStart,
        134 => IdleBlock,
        135 => Race { part, results, counted },
        136 => RaceDrop { results, counted },
        137 => All { part, elem },
});

wire_enum!(Sym {
    0 => Inst(a),
    1 => Shim(a, b, c, d),
    2 => CellDrop(a),
    3 => Part(a, b),
    4 => Helper(a),
    5 => Import(a),
});
wire_enum!(Part {
    0 => Body,
    1 => Poll,
    2 => Cancel,
    3 => Drop,
    4 => Lent,
    5 => Indirect,
});
wire_struct!(Import {
    module,
    name,
    params,
    results
});
wire_struct!(super::DescriptorKey { kind });
wire_enum!(Global {
    0 => Forbid,
    1 => HeapTop,
    2 => HeapEnd,
    3 => HashSeed,
    4 => HashSeedReady,
    5 => MapSeedCounter,
    6 => Aband,
    7 => AbSeen,
    8 => Rewake,
    9 => IdleHook,
    10 => IdleEnv,
    11 => Root,
    12 => Owner,
    13 => WakeTab,
    14 => WakeCap,
    15 => BlockingWakes,
    16 => NodeTab,
    17 => NodeCap,
    18 => NodeLen,
    19 => NodeFree,
    20 => SpillTab,
    21 => SpillCap,
    22 => SpillLen,
    23 => Waker,
    24 => WakeBit,
    25 => AllPolls,
    26 => ReleaseHead,
    27 => Releasing,
    28 => Tracing,
    29 => Stack,
    30 => StackLimit,
    31 => SessionData,
    32 => Binding(a),
    33 => Storage(a, b, c),
});
wire_enum!(super::Address {
    0 => Static(a),
    1 => Binding(a),
});
wire_enum!(Ins {
    0 => Op(a),
    1 => Static(a),
    2 => Call(a, b),
    3 => GlobalGet(a),
    4 => GlobalSet(a),
    5 => TableSlot(a),
    6 => Glue(a),
    7 => DrainGlue(a),
    8 => Descriptor(a, b),
    9 => CallIndirect(a, b),
    10 => VTable(a),
    11 => LoadAll,
    12 => SpillBase,
});

use crate::boundary::{CallRes, LCodec, LEnum, LHandle, LList, LMap, LNamed, LObject, LTy};
use crate::rt::{
    Counted, Elem, HostCounts, PrimArg, RefLayout, RefRead, ShimSyms, SlotCtx, TestGraph,
    TypeIdParts,
};
use crate::task::{FrameSpill, HostArg, HostRes};
use crate::views::{CountAction, CountOrigin};
use hd_layout::shims::{Portion, Shim};
use hd_layout::{Disc, Discr, Enc, Flat, Leaf, Mem, Niche};
use hd_tir::own::Reason;

wire_struct!(TestGraph {
    at,
    unit_offsets,
    units,
    scratch
});
wire_struct!(TypeIdParts { name, shown });
wire_struct!(RefRead { value });
wire_struct!(SlotCtx { ids, forward });
wire_struct!(RefLayout {
    size,
    leaves,
    counted
});
wire_struct!(Elem {
    size,
    leaves,
    counted,
    parts,
    cyclic
});
wire_struct!(HostCounts { borrowed, owned });
wire_struct!(ShimSyms {
    retain,
    release,
    retain_value,
    release_value,
    width,
    hooked,
    release_at,
    retain_at,
    in_place,
    small
});
wire_struct!(crate::map::Entry {
    size,
    leaves,
    keys,
    key_counted,
    val_counted,
    cyclic
});
wire_struct!(crate::rt::row::RowAdapter {
    params,
    results,
    entries,
    open,
    code
});
wire_struct!(crate::rt::row::RowEntry {
    required,
    supplier,
    projection
});
wire_struct!(FrameSpill {
    start,
    size,
    row,
    reason,
    origin,
    names_counts
});
wire_struct!(Leaf {
    offset,
    val,
    bytes,
    signed
});
wire_struct!(CountOrigin {
    origin,
    site,
    action,
    subject,
    reason
});
wire_struct!(Stored {
    vals,
    counted,
    slot
});
wire_struct!(Niche {
    offset,
    bytes,
    size,
    start,
    end
});
wire_struct!(Mem {
    size,
    align,
    leaves,
    niche,
    uninhabited
});
wire_struct!(Discr { leaf, value, enc });
wire_struct!(Flat {
    mem,
    disc,
    positions,
    offsets,
    leaves
});
wire_struct!(LList { elem, ty });
wire_struct!(LEnum { layout, variants });
wire_struct!(LCodec {
    vals,
    size,
    leaves,
    codec
});
wire_struct!(LMap {
    entry,
    ops,
    key,
    val
});
wire_struct!(LObject {
    elem,
    recursive,
    tracked,
    ty
});
/// A handle's vtable and a recursive type's codec are numbered by the
/// module ([`super::Module::in_order`]): a unit that names one is emitted
/// again each link, never stored.
impl Wire for LHandle {
    fn put(&self, o: &mut Out) {
        o.unstorable = true;
    }
    fn get(_: &mut In<'_>) -> Option<Self> {
        None
    }
}
impl Wire for LNamed {
    fn put(&self, o: &mut Out) {
        o.unstorable = true;
    }
    fn get(_: &mut In<'_>) -> Option<Self> {
        None
    }
}

wire_enum!(Counted {
    0 => None,
    1 => Address { index, known },
    2 => Coll { index },
    3 => Optional { index, known },
    4 => Shims(a),
});
wire_enum!(PrimArg {
    0 => Str,
    1 => Scalar(a),
});
wire_enum!(crate::map::KeyOps {
    0 => I32(a),
    1 => I64(a),
    2 => Str(a, b),
    3 => Call { hash, eq, own },
});
wire_enum!(HostArg {
    0 => Str,
    1 => Scalar(a),
    2 => Buf(a),
    3 => List(a),
});
wire_enum!(HostRes {
    0 => Void,
    1 => Tags,
    2 => Buf(a),
});
wire_enum!(CallRes {
    0 => Void,
    1 => Scalar(a),
    2 => Buf(a),
});
wire_enum!(LTy {
    0 => Void,
    1 => Int { bits, signed },
    2 => Char,
    3 => F32,
    4 => F64,
    5 => Str,
    6 => DiscardStr,
    7 => Bytes,
    8 => List(a),
    9 => Record(a),
    10 => Enum(a),
    11 => Handle(a),
    12 => Codec(a),
    13 => Named(a),
    14 => Map(a),
    15 => Never(a),
    16 => Object(a),
});
wire_enum!(CountAction {
    0 => Retain,
    1 => Release,
    2 => Free,
    3 => Unique,
});
wire_enum!(Reason {
    0 => Stored,
    1 => SecondOwner,
    2 => LastUse,
    3 => Overwritten,
    4 => Cancelled,
    5 => MayBeShared,
    6 => PointerLoan,
    7 => ByDesign,
    8 => Conservative(a),
});
wire_enum!(Shim {
    0 => Retain,
    1 => Release,
    2 => CellDrop,
});
wire_enum!(Portion {
    0 => All,
    1 => Thin,
    2 => Rest,
});
wire_enum!(Enc {
    0 => Direct,
    1 => Niche { untagged, first, last, start, bytes },
});
wire_enum!(Disc {
    0 => One(a),
    1 => Cell(a),
});
wire_enum!(Static {
    0 => Bytes(a, b),
    1 => Record(a, b, c),
    2 => Slot(a, b),
    3 => Label(a),
});
/// A package host interface goes in the module's contract once, by
/// content: a unit that records one is emitted again each link.
impl Wire for Edit {
    fn put(&self, o: &mut Out) {
        match self {
            Edit::Stored(a, b) => {
                o.w.u8(0);
                a.put(o);
                b.put(o);
            }
            Edit::StorageName(a, b, c) => {
                o.w.u8(1);
                a.put(o);
                b.put(o);
                c.put(o);
            }
            Edit::Frame(a, b) => {
                o.w.u8(2);
                a.put(o);
                b.put(o);
            }
            Edit::FrameSpill(a, b) => {
                o.w.u8(3);
                a.put(o);
                b.put(o);
            }
            Edit::PackageInterface(_) | Edit::CTable(_) => o.unstorable = true,
        }
    }
    fn get(i: &mut In<'_>) -> Option<Self> {
        Some(match i.r.u8() {
            0 => Edit::Stored(Wire::get(i)?, Wire::get(i)?),
            1 => Edit::StorageName(Wire::get(i)?, Wire::get(i)?, Wire::get(i)?),
            2 => Edit::Frame(Wire::get(i)?, Wire::get(i)?),
            3 => Edit::FrameSpill(Wire::get(i)?, Wire::get(i)?),
            _ => return None,
        })
    }
}

/// A function's stored fields: all but its symbol tables, which are the
/// unit's, and its callees, which its body gives again.
fn put_func(f: &Func, o: &mut Out) {
    let Func {
        symbols: _,
        labels,
        callees: _,
        name,
        params,
        results,
        locals,
        stack_values,
        body,
        origins,
        origin_at,
        origin_item,
        origin_root,
        origins_before_opt,
        local_names,
        origin_args,
        statics,
        data_refs,
        count_origin_base,
        count_origins,
        count_sites,
        reason,
        names_counts,
        raw,
        at,
        inlined,
    } = f;
    if body.iter().any(|ins| matches!(ins, Ins::VTable(_))) {
        // A vtable's number is the module's ([`super::Module::in_order`]).
        o.unstorable = true;
    }
    labels.put(o);
    name.put(o);
    params.put(o);
    results.put(o);
    locals.put(o);
    stack_values.put(o);
    body.put(o);
    origins.put(o);
    origin_at.put(o);
    origin_item.put(o);
    origin_root.put(o);
    origins_before_opt.put(o);
    local_names.put(o);
    origin_args.put(o);
    statics.put(o);
    data_refs.put(o);
    count_origin_base.put(o);
    count_origins.put(o);
    count_sites.put(o);
    reason.put(o);
    names_counts.put(o);
    raw.put(o);
    at.put(o);
    inlined.put(o);
}

fn get_func(i: &mut In<'_>, symbols: &Symbols) -> Option<Func> {
    let mut f = Func {
        symbols: symbols.clone(),
        labels: Wire::get(i)?,
        callees: Vec::new(),
        name: Wire::get(i)?,
        params: Wire::get(i)?,
        results: Wire::get(i)?,
        locals: Wire::get(i)?,
        stack_values: Wire::get(i)?,
        body: Wire::get(i)?,
        origins: Wire::get(i)?,
        origin_at: Wire::get(i)?,
        origin_item: Wire::get(i)?,
        origin_root: Wire::get(i)?,
        origins_before_opt: Wire::get(i)?,
        local_names: Wire::get(i)?,
        origin_args: Wire::get(i)?,
        statics: Wire::get(i)?,
        data_refs: Wire::get(i)?,
        count_origin_base: Wire::get(i)?,
        count_origins: Wire::get(i)?,
        count_sites: Wire::get(i)?,
        reason: Wire::get(i)?,
        names_counts: Wire::get(i)?,
        raw: Wire::get(i)?,
        at: Wire::get(i)?,
        inlined: Wire::get(i)?,
    };
    f.rebuild_callees();
    Some(f)
}

/// The version of the listings above: a stored unit of another version
/// is a miss. Bump it with any change to a listing or to what a unit
/// holds.
const VERSION: u32 = 1;

/// Unit `e`'s stored form and the types it names, or `None` for a unit
/// that is not stored: one the merge emits again in order, one whose
/// emission failed, or one that names something the module numbers (a
/// vtable, a recursive boundary codec, a package host interface).
pub(crate) fn encode<E>(e: &Emitted<E>) -> Option<(Vec<u8>, Vec<Ty>)> {
    let Ok(f) = &e.f else { return None };
    if e.unit.serial {
        return None;
    }
    let mut o = Out::default();
    o.w.u32(VERSION);
    {
        let data = e.symbols.live();
        data.names.put(&mut o);
        data.globals.put(&mut o);
        data.signatures.put(&mut o);
    }
    let Unit {
        defines,
        shims: _,
        shim_queue,
        edits,
        statics,
        static_len,
        serial: _,
        reads: _,
    } = &e.unit;
    o.w.len_of(defines);
    for (id, g) in defines {
        id.put(&mut o);
        put_func(g, &mut o);
    }
    put_func(f, &mut o);
    shim_queue.put(&mut o);
    edits.put(&mut o);
    statics.put(&mut o);
    static_len.put(&mut o);
    (!o.unstorable).then_some((o.w.bytes, o.tys))
}

/// The unit stored as `bytes`, whose types are `tys`, ready to merge;
/// `None` when the bytes are not a unit of this version.
pub(crate) fn decode<E>(bytes: &[u8], tys: &[Ty]) -> Option<Emitted<E>> {
    let i = &mut In {
        r: Reader::new(bytes),
        tys,
    };
    if i.r.u32() != VERSION {
        return None;
    }
    let symbols = Symbols::unit();
    {
        let names: Vec<Sym> = Wire::get(i)?;
        let globals: Vec<Global> = Wire::get(i)?;
        let signatures: Vec<(Vec<hd_layout::Val>, Vec<hd_layout::Val>)> = Wire::get(i)?;
        let mut data = symbols.live_mut();
        for s in names {
            let id = SymId(super::next_index(data.names.len(), super::LOCAL));
            data.ids.insert(s.clone(), id);
            data.names.push(s);
        }
        for g in globals {
            let id = GlobalId(super::next_index(data.globals.len(), super::LOCAL));
            data.global_ids.insert(g, id);
            data.globals.push(g);
        }
        for s in signatures {
            let id = SigId(super::next_index(data.signatures.len(), super::LOCAL));
            data.signature_ids.insert(s.clone(), id);
            data.signatures.push(s);
        }
    }
    let n = i.r.count();
    let mut defines = Vec::with_capacity(n);
    for _ in 0..n {
        let id = SymId::get(i)?;
        defines.push((id, get_func(i, &symbols)?));
    }
    let f = get_func(i, &symbols)?;
    let shim_queue: Vec<(Hash128, super::ShimRequest)> = Wire::get(i)?;
    let shims = shim_queue
        .iter()
        .map(|(shape, (_, kind, _, by_value, key))| ((*shape, *kind, *by_value), *key))
        .collect();
    let unit = Unit {
        defines,
        shims,
        shim_queue,
        edits: Wire::get(i)?,
        statics: Wire::get(i)?,
        static_len: Wire::get(i)?,
        serial: false,
        reads: std::cell::RefCell::default(),
    };
    (i.r.ok() && i.r.at_end()).then_some(Emitted {
        symbols,
        unit,
        f: Ok(f),
    })
}

#[cfg(feature = "testing")]
#[path = "../../tests/support/unit_wire.rs"]
pub(crate) mod tests;
