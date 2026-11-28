//! Explicit record values for the emitted-unit codec. Each variant must
//! preserve its discriminant, every field, and the exact encoded bytes.
use super::{
    BTreeMap, BTreeSet, BlockType, CallRes, CountAction, CountOrigin, Counted, Disc, Discr, Edit,
    Elem, Emitted, Enc, Flat, FrameSpill, Global, GlobalId, Hash128, Helper, HostArg, HostCounts,
    HostRes, Ieee32, Ieee64, Import, In, Ins, LCodec, LEnum, LHandle, LList, LMap, LNamed, LObject,
    LTy, LabelListId, Leaf, Mem, MemArg, Niche, Op, Out, Part, Portion, PrimArg, Reader, Reason,
    RefLayout, RefRead, Shim, ShimSyms, SigId, SlotCtx, Static, Stored, Sym, SymId, Symbols,
    TestGraph, Ty, TypeIdParts, Unit, ValType, Wire, decode, encode,
};
use std::fmt::Debug;

trait Sample {
    fn sample(seed: u32) -> Self;
}
fn s<T: Sample>(seed: u32) -> T {
    T::sample(seed)
}
macro_rules! integer_samples { ($($t:ty),*) => {$(impl Sample for $t {
    fn sample(seed: u32) -> Self { Self::try_from(seed % 97 + 1).unwrap() }
})*}; }
integer_samples!(u8, u32, u64, usize, i32, i64);
impl Sample for bool {
    fn sample(seed: u32) -> Self {
        seed.is_multiple_of(2)
    }
}
impl Sample for String {
    fn sample(seed: u32) -> Self {
        format!("field-{seed}\0é")
    }
}
impl Sample for &'static str {
    fn sample(_: u32) -> Self {
        "wire reason"
    }
}
impl Sample for Box<str> {
    fn sample(seed: u32) -> Self {
        String::sample(seed).into()
    }
}
impl Sample for Hash128 {
    fn sample(seed: u32) -> Self {
        Self(u128::from(seed) << 96 | 0x11)
    }
}
impl Sample for Ty {
    fn sample(seed: u32) -> Self {
        Self(seed + 16)
    }
}
impl<T: Sample> Sample for Box<T> {
    fn sample(seed: u32) -> Self {
        Box::new(s(seed))
    }
}
impl<T: Sample> Sample for Vec<T> {
    fn sample(seed: u32) -> Self {
        vec![s(seed), s(seed + 1)]
    }
}
impl<T: Sample> Sample for Box<[T]> {
    fn sample(seed: u32) -> Self {
        Vec::sample(seed).into()
    }
}
impl<T: Sample> Sample for Option<T> {
    fn sample(seed: u32) -> Self {
        Some(s(seed))
    }
}
impl<T: Sample + Ord> Sample for BTreeSet<T> {
    fn sample(seed: u32) -> Self {
        [s(seed)].into()
    }
}
impl<K: Sample + Ord, V: Sample> Sample for BTreeMap<K, V> {
    fn sample(seed: u32) -> Self {
        [(s(seed), s(seed + 1))].into()
    }
}
impl<T: Sample, const N: usize> Sample for [T; N] {
    fn sample(seed: u32) -> Self {
        std::array::from_fn(|i| s(seed + u32::try_from(i).unwrap()))
    }
}
macro_rules! tuple_samples { ($($t:ident $n:literal),*) => {impl<$($t:Sample),*> Sample for ($($t,)*) {fn sample(seed:u32)->Self { ($(s::<$t>(seed+$n),)*) }} }; }
tuple_samples!(A 0,B 1);
tuple_samples!(A 0,B 1,C 2);
tuple_samples!(A 0,B 1,C 2,D 3);
tuple_samples!(A 0,B 1,C 2,D 3,E 4);
impl Sample for ValType {
    fn sample(_: u32) -> Self {
        Self::F64
    }
}
impl Sample for Ieee32 {
    fn sample(_: u32) -> Self {
        Self::new(0xffc0_1234)
    }
}
impl Sample for Ieee64 {
    fn sample(_: u32) -> Self {
        Self::new(0xfff8_1234_5678_9abc)
    }
}
impl Sample for LHandle {
    fn sample(seed: u32) -> Self {
        Self {
            vtable: s(seed),
            wrap: s(seed + 1),
            release: s(seed + 2),
        }
    }
}
impl Sample for LNamed {
    fn sample(seed: u32) -> Self {
        Self {
            id: s(seed),
            vals: s(seed + 1),
        }
    }
}

fn check<T: Wire + Debug>(value: &T) {
    let mut out = Out::default();
    value.put(&mut out);
    let mut input = In {
        r: Reader::new(&out.w.bytes),
        tys: &out.tys,
    };
    let decoded = T::get(&mut input);
    if out.unstorable {
        // Module-local handle/recursive-codec numbers deliberately force re-emission.
        assert!(decoded.is_none(), "unstorable {value:?}");
        return;
    }
    let decoded = decoded.expect("valid record");
    assert!(input.r.ok() && input.r.at_end());
    assert_eq!(format!("{value:?}"), format!("{decoded:?}"));
    let mut again = Out::default();
    decoded.put(&mut again);
    assert_eq!(out.w.bytes, again.w.bytes);
    assert_eq!(out.tys, again.tys);
    for n in 0..out.w.bytes.len() {
        let mut input = In {
            r: Reader::new(&out.w.bytes[..n]),
            tys: &out.tys,
        };
        let decoded = T::get(&mut input);
        assert!(
            decoded.is_none() || !input.r.ok(),
            "truncated {value:?} at {n}"
        );
    }
}
impl Sample for MemArg {
    fn sample(seed: u32) -> Self {
        Self {
            offset: s(seed),
            align: s(seed + 1),
            memory_index: s(seed + 2),
        }
    }
}
impl Sample for Import {
    fn sample(seed: u32) -> Self {
        Self {
            module: s(seed),
            name: s(seed + 1),
            params: s(seed + 2),
            results: s(seed + 3),
        }
    }
}
impl Sample for crate::module::DescriptorKey {
    fn sample(seed: u32) -> Self {
        Self { kind: s(seed) }
    }
}
impl Sample for TestGraph {
    fn sample(seed: u32) -> Self {
        Self {
            at: s(seed),
            unit_offsets: s(seed + 1),
            units: s(seed + 2),
            scratch: s(seed + 3),
        }
    }
}
impl Sample for TypeIdParts {
    fn sample(seed: u32) -> Self {
        Self {
            name: s(seed),
            shown: s(seed + 1),
        }
    }
}
impl Sample for RefRead {
    fn sample(seed: u32) -> Self {
        Self { value: s(seed) }
    }
}
impl Sample for SlotCtx {
    fn sample(seed: u32) -> Self {
        Self {
            ids: s(seed),
            forward: s(seed + 1),
        }
    }
}
impl Sample for RefLayout {
    fn sample(seed: u32) -> Self {
        Self {
            size: s(seed),
            leaves: s(seed + 1),
            counted: s(seed + 2),
        }
    }
}
impl Sample for Elem {
    fn sample(seed: u32) -> Self {
        Self {
            size: s(seed),
            leaves: s(seed + 1),
            counted: s(seed + 2),
            parts: s(seed + 3),
            cyclic: s(seed + 4),
        }
    }
}
impl Sample for HostCounts {
    fn sample(seed: u32) -> Self {
        Self {
            borrowed: s(seed),
            owned: s(seed + 1),
        }
    }
}
impl Sample for ShimSyms {
    fn sample(seed: u32) -> Self {
        Self {
            retain: s(seed),
            release: s(seed + 1),
            retain_value: s(seed + 2),
            release_value: s(seed + 3),
            width: s(seed + 4),
            hooked: s(seed + 5),
            release_at: s(seed + 6),
            retain_at: s(seed + 7),
            in_place: s(seed + 8),
            small: s(seed + 9),
        }
    }
}
impl Sample for crate::map::Entry {
    fn sample(seed: u32) -> Self {
        Self {
            size: s(seed),
            leaves: s(seed + 1),
            keys: s(seed + 2),
            key_counted: s(seed + 3),
            val_counted: s(seed + 4),
            cyclic: s(seed + 5),
        }
    }
}
impl Sample for crate::rt::row::RowAdapter {
    fn sample(seed: u32) -> Self {
        Self {
            params: s(seed),
            results: s(seed + 1),
            entries: s(seed + 2),
            open: s(seed + 3),
            code: s(seed + 4),
        }
    }
}
impl Sample for crate::rt::row::RowEntry {
    fn sample(seed: u32) -> Self {
        Self {
            required: s(seed),
            supplier: s(seed + 1),
            projection: s(seed + 2),
        }
    }
}
impl Sample for FrameSpill {
    fn sample(seed: u32) -> Self {
        Self {
            start: s(seed),
            size: s(seed + 1),
            row: s(seed + 2),
            reason: s(seed + 3),
            origin: s(seed + 4),
            names_counts: s(seed + 5),
        }
    }
}
impl Sample for Leaf {
    fn sample(seed: u32) -> Self {
        Self {
            offset: s(seed),
            val: s(seed + 1),
            bytes: s(seed + 2),
            signed: s(seed + 3),
        }
    }
}
impl Sample for CountOrigin {
    fn sample(seed: u32) -> Self {
        Self {
            origin: s(seed),
            site: s(seed + 1),
            action: s(seed + 2),
            subject: s(seed + 3),
            reason: s(seed + 4),
        }
    }
}
impl Sample for Stored {
    fn sample(seed: u32) -> Self {
        Self {
            vals: s(seed),
            counted: s(seed + 1),
            slot: s(seed + 2),
        }
    }
}
impl Sample for Niche {
    fn sample(seed: u32) -> Self {
        Self {
            offset: s(seed),
            bytes: s(seed + 1),
            size: s(seed + 2),
            start: s(seed + 3),
            end: s(seed + 4),
        }
    }
}
impl Sample for Mem {
    fn sample(seed: u32) -> Self {
        Self {
            size: s(seed),
            align: s(seed + 1),
            leaves: s(seed + 2),
            niche: s(seed + 3),
            uninhabited: s(seed + 4),
        }
    }
}
impl Sample for Discr {
    fn sample(seed: u32) -> Self {
        Self {
            leaf: s(seed),
            value: s(seed + 1),
            enc: s(seed + 2),
        }
    }
}
impl Sample for Flat {
    fn sample(seed: u32) -> Self {
        Self {
            mem: s(seed),
            disc: s(seed + 1),
            positions: s(seed + 2),
            offsets: s(seed + 3),
            leaves: s(seed + 4),
        }
    }
}
impl Sample for LList {
    fn sample(seed: u32) -> Self {
        Self {
            elem: s(seed),
            ty: s(seed + 1),
        }
    }
}
impl Sample for LEnum {
    fn sample(seed: u32) -> Self {
        Self {
            layout: s(seed),
            variants: s(seed + 1),
        }
    }
}
impl Sample for LCodec {
    fn sample(seed: u32) -> Self {
        Self {
            vals: s(seed),
            size: s(seed + 1),
            leaves: s(seed + 2),
            codec: s(seed + 3),
        }
    }
}
impl Sample for LMap {
    fn sample(seed: u32) -> Self {
        Self {
            entry: s(seed),
            ops: s(seed + 1),
            key: s(seed + 2),
            val: s(seed + 3),
        }
    }
}
impl Sample for LObject {
    fn sample(seed: u32) -> Self {
        Self {
            elem: s(seed),
            recursive: s(seed + 1),
            tracked: s(seed + 2),
            ty: s(seed + 3),
        }
    }
}
impl Sample for BlockType {
    fn sample(_seed: u32) -> Self {
        BlockType::Empty
    }
}
impl Sample for hd_layout::Val {
    fn sample(_seed: u32) -> Self {
        hd_layout::Val::I32
    }
}
impl Sample for Op {
    fn sample(_seed: u32) -> Self {
        Op::AtomicFence
    }
}
impl Sample for Helper {
    fn sample(_seed: u32) -> Self {
        Helper::MemoryCopy {
            source: s(11),
            dest: s(12),
        }
    }
}
impl Sample for Sym {
    fn sample(_seed: u32) -> Self {
        Sym::Inst(s(11))
    }
}
impl Sample for Part {
    fn sample(_seed: u32) -> Self {
        Part::Body
    }
}
impl Sample for Global {
    fn sample(_seed: u32) -> Self {
        Global::Forbid
    }
}
impl Sample for crate::module::Address {
    fn sample(_seed: u32) -> Self {
        crate::module::Address::Static(s(11))
    }
}
impl Sample for Ins {
    fn sample(_seed: u32) -> Self {
        Ins::Op(s(11))
    }
}
impl Sample for Counted {
    fn sample(_seed: u32) -> Self {
        Counted::None
    }
}
impl Sample for PrimArg {
    fn sample(_seed: u32) -> Self {
        PrimArg::Str
    }
}
impl Sample for crate::map::KeyOps {
    fn sample(_seed: u32) -> Self {
        crate::map::KeyOps::I32(s(11))
    }
}
impl Sample for HostArg {
    fn sample(_seed: u32) -> Self {
        HostArg::Str
    }
}
impl Sample for HostRes {
    fn sample(_seed: u32) -> Self {
        HostRes::Void
    }
}
impl Sample for CallRes {
    fn sample(_seed: u32) -> Self {
        CallRes::Void
    }
}
impl Sample for LTy {
    fn sample(_seed: u32) -> Self {
        LTy::Void
    }
}
impl Sample for CountAction {
    fn sample(_seed: u32) -> Self {
        CountAction::Retain
    }
}
impl Sample for Reason {
    fn sample(_seed: u32) -> Self {
        Reason::Stored
    }
}
impl Sample for Shim {
    fn sample(_seed: u32) -> Self {
        Shim::Retain
    }
}
impl Sample for Portion {
    fn sample(_seed: u32) -> Self {
        Portion::All
    }
}
impl Sample for Enc {
    fn sample(_seed: u32) -> Self {
        Enc::Direct
    }
}
impl Sample for Disc {
    fn sample(_seed: u32) -> Self {
        Disc::One(s(11))
    }
}
impl Sample for Static {
    fn sample(_seed: u32) -> Self {
        Static::Bytes(s(11), s(12))
    }
}
impl Sample for LabelListId {
    fn sample(seed: u32) -> Self {
        Self(s(seed))
    }
}
impl Sample for SymId {
    fn sample(seed: u32) -> Self {
        Self(s(seed))
    }
}
impl Sample for GlobalId {
    fn sample(seed: u32) -> Self {
        Self(s(seed))
    }
}
impl Sample for SigId {
    fn sample(seed: u32) -> Self {
        Self(s(seed))
    }
}
pub fn round_trip_records() {
    exact_float_bits();
    frozen_symbol_base();
    malformed_records();
    emitted_units();
    check(&false);
    check(&true);
    check(&None::<u32>);
    for val in [ValType::I32, ValType::I64, ValType::F32, ValType::F64] {
        check(&val);
    }
    check(&i32::MIN);
    check(&i64::MIN);
    check(&u64::MAX);
    check(&ValType::V128);
    check(&ValType::Ref(wasm_encoder::RefType::FUNCREF));
    check(&Edit::Stored(s(11), s(12)));
    check(&Edit::StorageName(s(11), s(12), true));
    check(&Edit::Frame(s(11), s(12)));
    check(&Edit::FrameSpill(s(11), s(12)));

    check(&BlockType::Empty);
    check(&BlockType::Result(s(11)));
    check(&BlockType::FunctionType(s(11)));
    check(&hd_layout::Val::I32);
    check(&hd_layout::Val::I64);
    check(&hd_layout::Val::F32);
    check(&hd_layout::Val::F64);
    check(&Op::AtomicFence);
    check(&Op::Block(s(11)));
    check(&Op::Br(s(11)));
    check(&Op::BrIf(s(11)));
    check(&Op::BrTable(s(11), s(12)));
    check(&Op::Drop);
    check(&Op::Else);
    check(&Op::End);
    check(&Op::F32Abs);
    check(&Op::F32Add);
    check(&Op::F32Ceil);
    check(&Op::F32Copysign);
    check(&Op::F32Const(s(11)));
    check(&Op::F32ConvertI32S);
    check(&Op::F32ConvertI32U);
    check(&Op::F32ConvertI64S);
    check(&Op::F32ConvertI64U);
    check(&Op::F32DemoteF64);
    check(&Op::F32Div);
    check(&Op::F32Eq);
    check(&Op::F32Floor);
    check(&Op::F32Ge);
    check(&Op::F32Gt);
    check(&Op::F32Le);
    check(&Op::F32Load(s(11)));
    check(&Op::F32Lt);
    check(&Op::F32Mul);
    check(&Op::F32Ne);
    check(&Op::F32Nearest);
    check(&Op::F32Neg);
    check(&Op::F32ReinterpretI32);
    check(&Op::F32Sqrt);
    check(&Op::F32Store(s(11)));
    check(&Op::F32Sub);
    check(&Op::F32Trunc);
    check(&Op::F64Abs);
    check(&Op::F64Add);
    check(&Op::F64Ceil);
    check(&Op::F64Copysign);
    check(&Op::F64Const(s(11)));
    check(&Op::F64ConvertI32S);
    check(&Op::F64ConvertI32U);
    check(&Op::F64ConvertI64S);
    check(&Op::F64ConvertI64U);
    check(&Op::F64Div);
    check(&Op::F64Eq);
    check(&Op::F64Floor);
    check(&Op::F64Ge);
    check(&Op::F64Gt);
    check(&Op::F64Le);
    check(&Op::F64Load(s(11)));
    check(&Op::F64Lt);
    check(&Op::F64Mul);
    check(&Op::F64Ne);
    check(&Op::F64Nearest);
    check(&Op::F64Neg);
    check(&Op::F64PromoteF32);
    check(&Op::F64ReinterpretI64);
    check(&Op::F64Sqrt);
    check(&Op::F64Store(s(11)));
    check(&Op::F64Sub);
    check(&Op::F64Trunc);
    check(&Op::I32Add);
    check(&Op::I32And);
    check(&Op::I32Clz);
    check(&Op::I32Const(s(11)));
    check(&Op::I32Ctz);
    check(&Op::I32DivS);
    check(&Op::I32DivU);
    check(&Op::I32Eq);
    check(&Op::I32Eqz);
    check(&Op::I32Extend16S);
    check(&Op::I32Extend8S);
    check(&Op::I32GeS);
    check(&Op::I32GeU);
    check(&Op::I32GtS);
    check(&Op::I32GtU);
    check(&Op::I32LeS);
    check(&Op::I32LeU);
    check(&Op::I32AtomicLoad(s(11)));
    check(&Op::I32AtomicRmwAdd(s(11)));
    check(&Op::I32AtomicRmwCmpxchg(s(11)));
    check(&Op::I32AtomicRmwSub(s(11)));
    check(&Op::I32AtomicStore(s(11)));
    check(&Op::I32Load(s(11)));
    check(&Op::I32Load16S(s(11)));
    check(&Op::I32Load16U(s(11)));
    check(&Op::I32Load8S(s(11)));
    check(&Op::I32Load8U(s(11)));
    check(&Op::I32LtS);
    check(&Op::I32LtU);
    check(&Op::I32Mul);
    check(&Op::I32Ne);
    check(&Op::I32Or);
    check(&Op::I32Popcnt);
    check(&Op::I32ReinterpretF32);
    check(&Op::I32RemS);
    check(&Op::I32RemU);
    check(&Op::I32Shl);
    check(&Op::I32ShrS);
    check(&Op::I32ShrU);
    check(&Op::I32Store(s(11)));
    check(&Op::I32Store16(s(11)));
    check(&Op::I32Store8(s(11)));
    check(&Op::I32Sub);
    check(&Op::I32TruncSatF32S);
    check(&Op::I32TruncSatF32U);
    check(&Op::I32TruncSatF64S);
    check(&Op::I32TruncSatF64U);
    check(&Op::I32WrapI64);
    check(&Op::I32Xor);
    check(&Op::I64Add);
    check(&Op::I64And);
    check(&Op::I64Clz);
    check(&Op::I64Const(s(11)));
    check(&Op::I64Ctz);
    check(&Op::I64DivS);
    check(&Op::I64DivU);
    check(&Op::I64Eq);
    check(&Op::I64Eqz);
    check(&Op::I64ExtendI32S);
    check(&Op::I64ExtendI32U);
    check(&Op::I64GeS);
    check(&Op::I64GeU);
    check(&Op::I64GtS);
    check(&Op::I64GtU);
    check(&Op::I64LeS);
    check(&Op::I64LeU);
    check(&Op::I64Load(s(11)));
    check(&Op::I64LtS);
    check(&Op::I64LtU);
    check(&Op::I64Mul);
    check(&Op::I64Ne);
    check(&Op::I64Or);
    check(&Op::I64Popcnt);
    check(&Op::I64ReinterpretF64);
    check(&Op::I64RemS);
    check(&Op::I64RemU);
    check(&Op::I64Shl);
    check(&Op::I64ShrS);
    check(&Op::I64ShrU);
    check(&Op::I64Store(s(11)));
    check(&Op::I64Sub);
    check(&Op::I64TruncSatF32S);
    check(&Op::I64TruncSatF32U);
    check(&Op::I64TruncSatF64S);
    check(&Op::I64TruncSatF64U);
    check(&Op::I64Xor);
    check(&Op::If(s(11)));
    check(&Op::LocalGet(s(11)));
    check(&Op::LocalSet(s(11)));
    check(&Op::LocalTee(s(11)));
    check(&Op::Loop(s(11)));
    check(&Op::MemoryCopy {
        src_mem: s(11),
        dst_mem: s(12),
    });
    check(&Op::MemoryFill(s(11)));
    check(&Op::MemoryGrow(s(11)));
    check(&Op::MemoryAtomicNotify(s(11)));
    check(&Op::MemoryAtomicWait32(s(11)));
    check(&Op::MemorySize(s(11)));
    check(&Op::Return);
    check(&Op::Select);
    check(&Op::Unreachable);
    check(&Helper::MemoryCopy {
        source: s(11),
        dest: s(12),
    });
    check(&Helper::MemoryFill(s(11)));
    check(&Helper::CanCancel);
    check(&Helper::CancellationRoot);
    check(&Helper::EntryInit { inits: s(11) });
    check(&Helper::EntryPoll {
        main: s(11),
        providers: s(12),
        results: s(13),
        report: s(14),
        then: s(15),
        bang: s(16),
    });
    check(&Helper::CheckedPoll { poll: s(11) });
    check(&Helper::CollectPoll { poll: s(11) });
    check(&Helper::TestPoll {
        polls: s(11),
        reinit: s(12),
    });
    check(&Helper::TestInit { inits: s(11) });
    check(&Helper::TestPollPart { polls: s(11) });
    check(&Helper::TestInitWalk {
        inits: s(11),
        graph: s(12),
    });
    check(&Helper::TestInitPart { inits: s(11) });
    check(&Helper::EntryReset);
    check(&Helper::Teardown);
    check(&Helper::Collect);
    check(&Helper::CycleTrace);
    check(&Helper::CycleRelease);
    check(&Helper::CycleFree);
    check(&Helper::Opaque);
    check(&Helper::Panic(s(11)));
    check(&Helper::PanicStr);
    check(&Helper::LoanShare);
    check(&Helper::LoanUnshare);
    check(&Helper::PanicWith);
    check(&Helper::ToBuf);
    check(&Helper::Alloc);
    check(&Helper::New);
    check(&Helper::NewFixed { size: s(11) });
    check(&Helper::AllocClass {
        class: s(11),
        bytes: s(12),
    });
    check(&Helper::FreeClass);
    check(&Helper::FreeClassLoaded);
    check(&Helper::FreeFrame);
    check(&Helper::Str);
    check(&Helper::ReleaseRef);
    check(&Helper::ReleaseDrain);
    check(&Helper::RetainKnown);
    check(&Helper::ReleaseKnown);
    check(&Helper::ReleaseFresh);
    check(&Helper::RetainColl);
    check(&Helper::RetainFields {
        leaves: s(11),
        parts: s(12),
    });
    check(&Helper::ReleaseSpilled {
        size: s(11),
        leaves: s(12),
        shim: s(13),
    });
    check(&Helper::DropParts {
        leaves: s(11),
        parts: s(12),
    });
    check(&Helper::ListBuf { elem: s(11) });
    check(&Helper::DropElems { elem: s(11) });
    check(&Helper::CtxFind);
    check(&Helper::CtxNew);
    check(&Helper::RowAdapter(s(11)));
    check(&Helper::CopyElems { elem: s(11) });
    check(&Helper::ListUnique { elem: s(11) });
    check(&Helper::ListCopy { elem: s(11) });
    check(&Helper::ListGrow { elem: s(11) });
    check(&Helper::MapSeed);
    check(&Helper::MapTable { entry: s(11) });
    check(&Helper::DropEntries { entry: s(11) });
    check(&Helper::MapHash {
        key: s(11),
        ops: s(12),
    });
    check(&Helper::MapFind {
        entry: s(11),
        ops: s(12),
    });
    check(&Helper::MapLookup {
        entry: s(11),
        ops: s(12),
        probe: s(13),
    });
    check(&Helper::MapPut {
        entry: s(11),
        ops: s(12),
    });
    check(&Helper::MapGrow { entry: s(11) });
    check(&Helper::MapUnique { entry: s(11) });
    check(&Helper::MapSlotNext);
    check(&Helper::DynSlot {
        target: s(11),
        body: true,
        retained_result: s(12),
        leaves: s(13),
        params: s(14),
        results: s(15),
        own: s(16),
        given: s(17),
        ctx: s(18),
    });
    check(&Helper::DynRead {
        target: s(11),
        retained_result: s(12),
        read: s(13),
        params: s(14),
        results: s(15),
        own: s(16),
        given: s(17),
        ctx: s(18),
    });
    check(&Helper::MapStep {
        entry: s(11),
        item: s(12),
    });
    check(&Helper::ListTruncate { elem: s(11) });
    check(&Helper::ListReserve { elem: s(11) });
    check(&Helper::ListExtend { elem: s(11) });
    check(&Helper::TypeIdOf(s(11)));
    check(&Helper::TypeInfo(s(11)));
    check(&Helper::RuntimeType(s(11)));
    check(&Helper::DynTrap {
        params: s(11),
        results: s(12),
    });
    check(&Helper::StrFromBytes);
    check(&Helper::StrToBytes { elem: s(11) });
    check(&Helper::StrAppend);
    check(&Helper::FmtInt { signed: s(11) });
    check(&Helper::StrEq);
    check(&Helper::BytesIndexSet(s(11)));
    check(&Helper::BytesIndex);
    check(&Helper::BytesIndexPair);
    check(&Helper::BytesMismatch);
    check(&Helper::Utf8ValidLen);
    check(&Helper::HostPrim {
        name: s(11),
        args: s(12),
        result: s(13),
    });
    check(&Helper::ForbidCheck);
    check(&Helper::HostCold {
        module: s(11),
        method: s(12),
        args: s(13),
        area: s(14),
        counts: s(15),
        res: s(16),
    });
    check(&Helper::HostPoll {
        blocking: s(11),
        module: s(12),
        method: s(13),
        args: s(14),
        area: s(15),
        res: s(16),
    });
    check(&Helper::HostCancel);
    check(&Helper::HostDrive {
        blocking: s(11),
        module: s(12),
        method: s(13),
        args: s(14),
        area: s(15),
        counts: s(16),
        res: s(17),
    });
    check(&Helper::Drive {
        blocking: s(11),
        cold: s(12),
        params: s(13),
        results: s(14),
    });
    check(&Helper::HostCall {
        module: s(11),
        method: s(12),
        args: s(13),
        result: s(14),
        owned: s(15),
    });
    check(&Helper::HandleSlot {
        inner: s(11),
        params: s(12),
        ctx: s(13),
        results: s(14),
    });
    check(&Helper::PackageHandleRelease { module: s(11) });
    check(&Helper::Fit);
    check(&Helper::LebPut);
    check(&Helper::LebGet);
    check(&Helper::Enc(s(11)));
    check(&Helper::Dec(s(11)));
    check(&Helper::Frame(s(11)));
    check(&Helper::FrameEnsure(s(11)));
    check(&Helper::PollCheck);
    check(&Helper::CancelCheck);
    check(&Helper::CancelEnter);
    check(&Helper::CancelLeave);
    check(&Helper::CancelChild);
    check(&Helper::WakeMark);
    check(&Helper::WakeTake);
    check(&Helper::WakeSlot);
    check(&Helper::WakeRegister);
    check(&Helper::WakeRemove);
    check(&Helper::WakeSignal);
    check(&Helper::WakerNew);
    check(&Helper::WakerParent);
    check(&Helper::WakerSlot);
    check(&Helper::WakerLive);
    check(&Helper::WakerDrop);
    check(&Helper::WakeLink);
    check(&Helper::WakeNotify);
    check(&Helper::EntryCancel { abandon: s(11) });
    check(&Helper::EntryCall { func: s(11) });
    check(&Helper::RetainAny);
    check(&Helper::ChildDrop);
    check(&Helper::CleanupCold);
    check(&Helper::CleanupPoll);
    check(&Helper::CleanupCancel);
    check(&Helper::PendingCold { results: s(11) });
    check(&Helper::PendingPoll { results: s(11) });
    check(&Helper::PendingCancel);
    check(&Helper::WaitCell);
    check(&Helper::WaitCold);
    check(&Helper::WaitPoll);
    check(&Helper::WaitSignal);
    check(&Helper::IdleHook);
    check(&Helper::IdleWake);
    check(&Helper::ThreadStart);
    check(&Helper::IdleBlock);
    check(&Helper::Race {
        part: s(11),
        results: s(12),
        counted: s(13),
    });
    check(&Helper::RaceDrop {
        results: s(11),
        counted: s(12),
    });
    check(&Helper::All {
        part: s(11),
        elem: s(12),
    });
    check(&Sym::Inst(s(11)));
    check(&Sym::Shim(s(11), s(12), s(13), s(14)));
    check(&Sym::CellDrop(s(11)));
    check(&Sym::Part(s(11), s(12)));
    check(&Sym::Helper(s(11)));
    check(&Sym::Import(s(11)));
    check(&Part::Body);
    check(&Part::Poll);
    check(&Part::Cancel);
    check(&Part::Drop);
    check(&Part::Lent);
    check(&Part::Indirect);
    check(&Global::Forbid);
    check(&Global::HeapTop);
    check(&Global::HeapEnd);
    check(&Global::HashSeed);
    check(&Global::HashSeedReady);
    check(&Global::MapSeedCounter);
    check(&Global::Aband);
    check(&Global::AbSeen);
    check(&Global::Rewake);
    check(&Global::IdleHook);
    check(&Global::Root);
    check(&Global::Owner);
    check(&Global::WakeTab);
    check(&Global::WakeCap);
    check(&Global::BlockingWakes);
    check(&Global::NodeTab);
    check(&Global::NodeCap);
    check(&Global::NodeLen);
    check(&Global::NodeFree);
    check(&Global::SpillTab);
    check(&Global::SpillCap);
    check(&Global::SpillLen);
    check(&Global::Waker);
    check(&Global::WakeBit);
    check(&Global::AllPolls);
    check(&Global::ReleaseHead);
    check(&Global::Releasing);
    check(&Global::Tracing);
    check(&Global::Stack);
    check(&Global::StackLimit);
    check(&Global::SessionData);
    check(&Global::Binding(s(11)));
    check(&Global::Storage(s(11), s(12), s(13)));
    check(&crate::module::Address::Static(s(11)));
    check(&crate::module::Address::Binding(s(11)));
    check(&Ins::Op(s(11)));
    check(&Ins::Static(s(11)));
    check(&Ins::Call(s(11), s(12)));
    check(&Ins::GlobalGet(s(11)));
    check(&Ins::GlobalSet(s(11)));
    check(&Ins::TableSlot(s(11)));
    check(&Ins::Glue(s(11)));
    check(&Ins::DrainGlue(s(11)));
    check(&Ins::Descriptor(s(11), s(12)));
    check(&Ins::CallIndirect(s(11), s(12)));
    check(&Ins::VTable(s(11)));
    check(&Ins::LoadAll);
    check(&Ins::SpillBase);
    check(&Counted::None);
    check(&Counted::Address {
        index: s(11),
        known: s(12),
    });
    check(&Counted::Coll { index: s(11) });
    check(&Counted::Optional {
        index: s(11),
        known: s(12),
    });
    check(&Counted::Shims(s(11)));
    check(&PrimArg::Str);
    check(&PrimArg::Scalar(s(11)));
    check(&crate::map::KeyOps::I32(s(11)));
    check(&crate::map::KeyOps::I64(s(11)));
    check(&crate::map::KeyOps::Str(s(11), s(12)));
    check(&crate::map::KeyOps::Call {
        hash: s(11),
        eq: s(12),
        own: s(13),
    });
    check(&HostArg::Str);
    check(&HostArg::Scalar(s(11)));
    check(&HostArg::Buf(s(11)));
    check(&HostArg::List(s(11)));
    check(&HostRes::Void);
    check(&HostRes::Tags);
    check(&HostRes::Buf(s(11)));
    check(&CallRes::Void);
    check(&CallRes::Scalar(s(11)));
    check(&CallRes::Buf(s(11)));
    check(&LTy::Void);
    check(&LTy::Int {
        bits: s(11),
        signed: s(12),
    });
    check(&LTy::Char);
    check(&LTy::F32);
    check(&LTy::F64);
    check(&LTy::Str);
    check(&LTy::DiscardStr);
    check(&LTy::Bytes);
    check(&LTy::List(s(11)));
    check(&LTy::Record(s(11)));
    check(&LTy::Enum(s(11)));
    check(&LTy::Handle(s(11)));
    check(&LTy::Codec(s(11)));
    check(&LTy::Named(s(11)));
    check(&LTy::Map(s(11)));
    check(&LTy::Never(s(11)));
    check(&LTy::Object(s(11)));
    check(&CountAction::Retain);
    check(&CountAction::Release);
    check(&CountAction::Free);
    check(&CountAction::Unique);
    check(&Reason::Stored);
    check(&Reason::SecondOwner);
    check(&Reason::LastUse);
    check(&Reason::Overwritten);
    check(&Reason::Cancelled);
    check(&Reason::MayBeShared);
    check(&Reason::PointerLoan);
    check(&Reason::ByDesign);
    check(&Reason::Conservative(s(11)));
    check(&Shim::Retain);
    check(&Shim::Release);
    check(&Shim::CellDrop);
    check(&Portion::All);
    check(&Portion::Thin);
    check(&Portion::Rest);
    check(&Enc::Direct);
    check(&Enc::Niche {
        untagged: s(11),
        first: s(12),
        last: s(13),
        start: s(14),
        bytes: s(15),
    });
    check(&Disc::One(s(11)));
    check(&Disc::Cell(s(11)));
    check(&Static::Bytes(s(11), s(12)));
    check(&Static::Record(s(11), s(12), s(13)));
    check(&Static::Slot(s(11), s(12)));
    check(&Static::Label(s(11)));
    check(&s::<MemArg>(19));
    check(&s::<Import>(19));
    check(&s::<crate::module::DescriptorKey>(19));
    check(&s::<TestGraph>(19));
    check(&s::<TypeIdParts>(19));
    check(&s::<RefRead>(19));
    check(&s::<SlotCtx>(19));
    check(&s::<RefLayout>(19));
    check(&s::<Elem>(19));
    check(&s::<HostCounts>(19));
    check(&s::<ShimSyms>(19));
    check(&s::<crate::map::Entry>(19));
    check(&s::<crate::rt::row::RowAdapter>(19));
    check(&s::<crate::rt::row::RowEntry>(19));
    check(&s::<FrameSpill>(19));
    check(&s::<Leaf>(19));
    check(&s::<CountOrigin>(19));
    check(&s::<Stored>(19));
    check(&s::<Niche>(19));
    check(&s::<Mem>(19));
    check(&s::<Discr>(19));
    check(&s::<Flat>(19));
    check(&s::<LList>(19));
    check(&s::<LEnum>(19));
    check(&s::<LCodec>(19));
    check(&s::<LMap>(19));
    check(&s::<LObject>(19));
}

fn malformed<T: Wire>(bytes: &[u8], tys: &[Ty]) {
    let mut input = In {
        r: Reader::new(bytes),
        tys,
    };
    assert!(T::get(&mut input).is_none() || !input.r.ok());
}
fn malformed_records() {
    malformed::<bool>(&[2], &[]);
    malformed::<Option<u32>>(&[2], &[]);
    malformed::<Ty>(&0u32.to_le_bytes(), &[]);
    malformed::<Op>(&[255], &[]);
    malformed::<Edit>(&[255], &[]);
    malformed::<ValType>(&[255], &[]);
    malformed::<Vec<u32>>(&[255, 255, 255, 255], &[]);
}
fn fresh_unit() -> Emitted<()> {
    let symbols = Symbols::unit();
    let m = crate::module::Module::default();
    let mut f = m.func(
        "stored-body",
        vec![hd_layout::Val::I32],
        vec![hd_layout::Val::I32],
    );
    f.symbols = symbols.clone();
    f.i32(37);
    Emitted {
        symbols,
        unit: Unit::default(),
        f: Ok(f),
    }
}
fn emitted_units() {
    let mut unit = fresh_unit();
    let shape = Hash128(51);
    let key = Hash128(52);
    unit.unit.shim_queue = vec![(shape, (Ty::STRING, Shim::Retain, Portion::All, true, key))];
    unit.unit.shims.insert((shape, Shim::Retain, true), key);
    unit.unit.edits.push(Edit::StorageName(s(2), s(3), true));
    let mut defined = fresh_unit().f.unwrap();
    defined.symbols = unit.symbols.clone();
    let target = Sym::Inst(Hash128(73));
    let defined_id = unit.symbols.intern(target.clone());
    unit.unit.defines.push((defined_id, defined));
    unit.f.as_mut().unwrap().call(target);
    let _global = unit.symbols.intern_global(Global::HeapTop);
    let _signature = unit
        .symbols
        .intern_signature(vec![hd_layout::Val::I32], vec![hd_layout::Val::I64]);
    let (bytes, tys) = encode(&unit).expect("storable unit");
    let decoded = decode::<()>(&bytes, &tys).expect("unit round trip");
    assert_eq!(encode(&decoded), Some((bytes.clone(), tys.clone())));
    assert_eq!(unit.unit.shims, decoded.unit.shims);
    assert_eq!(
        format!("{:?}", unit.f.as_ref().unwrap().callees),
        format!("{:?}", decoded.f.as_ref().unwrap().callees)
    );
    for n in 0..bytes.len() {
        assert!(decode::<()>(&bytes[..n], &tys).is_none());
    }
    let mut bad = bytes.clone();
    bad[0] = 255;
    assert!(decode::<()>(&bad, &tys).is_none());
    let mut bad = bytes;
    bad.push(1);
    assert!(decode::<()>(&bad, &tys).is_none());
    unit.unit.serial = true;
    assert!(encode(&unit).is_none());
    unit.unit.serial = false;
    unit.f = Err(());
    assert!(encode(&unit).is_none());
    let mut unit = fresh_unit();
    unit.f.as_mut().unwrap().body.push(Ins::VTable(17));
    assert!(encode(&unit).is_none());
    let interface = hd_host_abi::schema::Interface {
        identity: hd_host_abi::schema::Identity {
            package: "pkg".into(),
            version: "1".into(),
            trait_path: "Disk".into(),
        },
        module: "pkg:Disk".into(),
        selectors: vec!["app.Disk".into()],
        handle: false,
        methods: vec![],
    };
    for edit in [
        Edit::PackageInterface(interface),
        Edit::CTable(hd_host_abi::c::Table {
            module: "c:Disk".into(),
            trait_name: "Disk".into(),
            handle: false,
            methods: vec![],
        }),
    ] {
        let mut unit = fresh_unit();
        unit.unit.edits.push(edit);
        assert!(encode(&unit).is_none());
    }
}

fn frozen_symbol_base() {
    let base = Symbols::default();
    let sym = Sym::Inst(Hash128(73));
    let name = base.intern(sym.clone());
    let global = base.intern_global(Global::HeapTop);
    let signature = base.intern_signature(vec![hd_layout::Val::I32], vec![hd_layout::Val::F64]);
    base.freeze();
    let local = Symbols::local(&base);
    assert_eq!(local.intern(sym), name);
    assert_eq!(local.intern_global(Global::HeapTop), global);
    assert_eq!(
        local.intern_signature(vec![hd_layout::Val::I32], vec![hd_layout::Val::F64]),
        signature
    );
}

fn exact_float_bits() {
    for bits in [0u32, 0x8000_0000, 0x7fc0_1234, 0x7f80_0000] {
        let mut out = Out::default();
        Ieee32::new(bits).put(&mut out);
        assert_eq!(out.w.bytes, bits.to_le_bytes());
        let mut input = In {
            r: Reader::new(&out.w.bytes),
            tys: &[],
        };
        assert_eq!(Ieee32::get(&mut input).unwrap().bits(), bits);
    }
    for bits in [
        0u64,
        0x8000_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
        0x7ff0_0000_0000_0000,
    ] {
        let mut out = Out::default();
        Ieee64::new(bits).put(&mut out);
        assert_eq!(out.w.bytes, bits.to_le_bytes());
        let mut input = In {
            r: Reader::new(&out.w.bytes),
            tys: &[],
        };
        assert_eq!(Ieee64::get(&mut input).unwrap().bits(), bits);
    }
}
