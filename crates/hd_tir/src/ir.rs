//! The full TIR (`crates/hd_tir/DESIGN.md` §4.13.11; `docs/compiler/data-structures.md` §3.18):
//! the instruction catalog with its operand schema, the body columns, the
//! `TirSink` builder API, a text printer, a text parser that round-trips,
//! and the verifier.
//!
//! Constants: `Ref::konst(i)` names row `i` of the body's own constant
//! column (`consts`), so a body and its TIR hash need no run-wide table.

use hd_base::SliceSort as _;
use std::fmt::Write as _;

use hd_base::{
    CaptureId, DefId, LabelId, LocalId, NodeIdx, NotImplemented, Range32, Stage, SubId, Symbol,
};
use hd_types::{Ty, TyList};

mod copy;
mod lift;
mod nest;
mod owned;
mod root;
mod slots;
mod topo;
mod view;
pub(crate) use copy::{
    Remap, SideRows, Through, copy_rows, grow, local_words, push_frame, renumber,
};
pub use lift::{Lifting, function_template, item_template, scopes as local_function_scopes};
pub use nest::Nest;
pub use owned::verify_owned;
pub use root::{RootTables, root_places};
pub use slots::{Loc, Role, Word, slot_words, slots};
pub use topo::Topology;
pub(crate) use topo::{for_blocks, inside};
pub use view::{DEREF, ELEM, PlaceView, Root, Steps, VARIANT};

pub const NONE: u32 = u32::MAX;
/// The bit of an anchor that names an inlined copy's instruction
/// ([`Body::inlined`]), in the TIR and in a function's `hd.lines` rows.
pub const INLINED: u32 = 1 << 31;

/// An instruction index in its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inst(pub u32);

/// A value: bit 31 clear, an instruction's value; set, a row of the
/// body's `consts` (bits 0..30).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ref(pub u32);

impl Ref {
    pub const CONST_BIT: u32 = 1 << 31;
    #[must_use]
    pub const fn inst(i: Inst) -> Ref {
        Ref(i.0)
    }
    #[must_use]
    pub const fn konst(index: u32) -> Ref {
        Ref(index | Self::CONST_BIT)
    }
    #[must_use]
    pub const fn as_inst(self) -> Option<Inst> {
        if self.0 & Self::CONST_BIT == 0 {
            Some(Inst(self.0))
        } else {
            None
        }
    }
}

macro_rules! tags {
    ($( $tag:ident : $a:ident, $b:ident ;)*) => {
        /// The instruction catalog (§4.13.11, "Instruction Catalog").
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Tag { $($tag,)* }

        impl Tag {
            pub const ALL: &'static [Tag] = &[$(Tag::$tag,)*];
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $(Tag::$tag => stringify!($tag),)* }
            }
            /// The operand schema of the two data words (§4.13.11, "Four kinds of reference").
            #[must_use]
            pub const fn operands(self) -> (Op, Op) {
                match self { $(Tag::$tag => (Op::$a, Op::$b),)* }
            }
            #[must_use]
            pub fn from_name(s: &str) -> Option<Tag> {
                Tag::ALL.iter().copied().find(|t| t.name() == s)
            }
        }
    };
}

/// What a data word means. `Values`, `Blocks` and `Record` point at a
/// record in `extra`: a length word, then that many words. `Place` is a
/// place word ([`place`]): a bare local, or a place record in `extra`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    None,
    Value,
    /// An instruction identity, not an evaluated value or a block to run.
    Inst,
    Block,
    Label,
    Meta,
    Values,
    Blocks,
    Record,
    Place,
}

tags! {
    // values: a local or a top-level binding is read with `Read`
    ItemRef: Meta, Record;
    ProviderGet: Meta, None;
    Hole: Meta, None;
    Poison: None, None;
    // operators and calls
    Prim: Meta, Values;
    And: Value, Block;
    Or: Value, Block;
    Call: Record, Values;
    CallValue: Value, Values;
    CallDyn: Value, Record;
    Is: Value, Value;
    CallHost: Meta, Values;
    Intrinsic: Meta, Record;
    DefaultCall: Record, Values;
    Coerce: Value, Record;
    // suspension and hooks
    Await: Record, Values;
    AwaitValue: Value, Values;
    AwaitAll: None, Values;
    AwaitRace: Value, None;
    Hook: Meta, Meta;
    // data and closures
    NewStruct: None, Values;
    CopyStruct: Value, Record;
    NewVariant: Meta, Values;
    NewTuple: Value, Values;
    NewList: Record, Record;
    NewMap: None, Values;
    Closure: Meta, Record;
    // references (`crates/hd_tir/DESIGN.md` §4.13.11, "References"): a
    // reference converted to a trait value reference, which its block
    // builds, and a reference's identity
    RefDyn: Block, None;
    RefIdentity: Value, None;
    // places (`crates/hd_tir/DESIGN.md` §4.13.11, "Places"): a is a place
    // word
    Read: Place, Meta;
    Store: Place, Value;
    Ref: Place, Meta;
    // ownership (`docs/compiler/tir-v2.md`, "Ownership statements"): the
    // ownership passes write these after decoding, never the checker; a
    // is a place word. `FlagSet` and `FlagClear` set and clear the drop
    // flag local in b of the move path in a (`own::drop_flags`). `Box`
    // makes local a's cell where its scope starts, b a [`box_word`].
    Drop: Place, None;
    IsUnique: Place, Meta;
    FlagSet: Place, Meta;
    FlagClear: Place, Meta;
    Box: Meta, Meta;
    // providers
    With: Record, Block;
    ContextNew: Record, None;
    ContextFor: Meta, None;
    // control flow and cleanup
    Block: Record, Value;
    Scope: Block, Blocks;
    Defer: Meta, None;
    If: Value, Blocks;
    Loop: Block, Block;
    Break: Label, Value;
    Continue: Label, None;
    Return: Value, Meta;
    Unreachable: None, None;
    // matching
    Match: Value, Blocks;
    SwitchTag: Value, Record;
    SwitchInt: Value, Record;
    SwitchChar: Value, Record;
    SwitchStr: Value, Record;
    Guard: Block, Blocks;
    ToArm: Meta, None;
    // ownership: a pointer loan's end, at the last use of a reference that
    // counts nothing; a is the holder's place word. Append ownership tags
    // so that existing tags keep their numeric encodings.
    LoanEnd: Place, None;
    // Counts: target instruction identity plus one shared descriptor.
    Retain: Inst, Record;
    Release: Inst, Record;
    // A deferred operation's uniqueness action, with the same descriptor.
    MakeUnique: Inst, Record;
    // Run-time pointer loans (`docs/compiler/ownership.md`, "Pointer
    // Loans"), which the checker places from its lifetime regions:
    // `LoanBegin` takes the loans of site a (a `Ref`, a lent argument, or
    // a call whose result points into an object), named as an instruction,
    // not used as a value, so no pass counts it as a use; b a
    // [`loan_word`];
    // `LoanClose` ends the loans of the `LoanBegin` a names, after the
    // last use of every value and local whose region holds them.
    LoanBegin: Inst, Meta;
    LoanClose: Inst, None;
    // `OwnLoans` uses the own-loan counter local its place a names
    // ([`loan_word::COUNTER_SHIFT`]) where a loan through its pointer
    // begins or closes or an access through it is checked, so the passes
    // keep the counter, and a suspending body's frame, until then.
    OwnLoans: Place, None;
}

/// The kind word of a `LoanBegin`.
pub mod loan_word {
    /// An exclusive loan on the last object crossed, for a `&mut`.
    pub const EXCL: u32 = 1;
    /// A two-phase `&mut self` receiver's activation, right before its
    /// call: the shared loan the site's own `LoanBegin` reserved becomes
    /// exclusive. Its `a` is that `LoanBegin`'s site.
    pub const ACTIVATE: u32 = 2;
    /// The object a call's returned reference points into, which the
    /// callee reached through a pointer: its site is the call.
    pub const INNER: u32 = 4;
    /// Above the flags: the local plus one that counts the function's own
    /// loans through the site's pointer, where a closure shares that
    /// pointer (`own::loans::share_counters`); 0 for none.
    pub const COUNTER_SHIFT: u32 = 8;

    /// The own-loan counter local `word` names.
    #[must_use]
    pub fn counter(word: u32) -> Option<u32> {
        (word >> COUNTER_SHIFT).checked_sub(1)
    }
}

pub mod count;

/// `Prim` operators (the `Meta` word of a `Prim` instruction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PrimOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Neg,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// Logical or bitwise not: `!b` on `bool`, `~n` on an integer.
    Not,
    /// A numeric conversion to the instruction's type (`i64(x)`).
    Conv,
    /// `a ** b`: checked integer power with an unsigned exponent, or the
    /// IEEE 754 `pow` of two floats (`expr.power.*`).
    Pow,
}

impl PrimOp {
    pub const ALL: [PrimOp; 22] = [
        PrimOp::Add,
        PrimOp::Sub,
        PrimOp::Mul,
        PrimOp::Div,
        PrimOp::Rem,
        PrimOp::Eq,
        PrimOp::Ne,
        PrimOp::Lt,
        PrimOp::Le,
        PrimOp::Gt,
        PrimOp::Ge,
        PrimOp::Neg,
        PrimOp::And,
        PrimOp::Or,
        PrimOp::BitAnd,
        PrimOp::BitOr,
        PrimOp::BitXor,
        PrimOp::Shl,
        PrimOp::Shr,
        PrimOp::Not,
        PrimOp::Conv,
        PrimOp::Pow,
    ];
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// Language-tier built-in operations (the `Meta` word of an `Intrinsic`
/// instruction): the built-in methods and indexing of `List`, `Map` and
/// `string` (spec/lang/10-modules.md#built-in-methods), whose std bodies
/// are written in terms of themselves. A list element is no intrinsic:
/// it is read and written as a place, `[%xs [%i]]`
/// ([`Body::element_read`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum IntrinsicOp {
    ListLen,
    ListPush,
    MapLen,
    MapGet,
    MapRemove,
    MapSet,
    StrIndex,
    /// `a + b` on two strings.
    StrConcat,
    /// `a == b` on two strings.
    StrEq,
    /// A call of an `@intrinsic` std function whose body is the compiler's.
    Item,
    /// `(map) -> usize`: how many entry slots the map has written, removed
    /// ones included.
    MapSlots,
    /// `(map, pos, end) -> usize`: the first live entry slot in
    /// `pos..end`, or `end`.
    MapSlotNext,
    /// `(map, slot) -> (&K, &V)`: the live entry's slots, lent for one
    /// iteration by the loop's table snapshot.
    MapSlotEntry,
    /// `(map, key) -> usize`: the number of the key's entry, a panic
    /// `index-out-of-bounds` when the map holds no equal key. A map value
    /// is a place, `[%m [%e]]` at that number (`ref.elem.map`,
    /// `types.coll.map-value.place`): the checker looks the entry up just
    /// before each read, store or reference of the place, as a list's
    /// place checks its index there. Entry numbers survive the copy that
    /// makes a map's table unique; only growth renumbers them.
    MapEntry,
    /// `(list, count)`: room for `count` more elements, so that many
    /// pushes grow no buffer (std's `list_reserve` on a value). A
    /// comprehension over a source of known length reserves its result
    /// once before its loop. It changes the list as a push does.
    ListReserve,
    /// `(map, slot) -> (&K, &mut V)`: lend a live entry of the original
    /// map place, made unique before the operation as for a map store.
    MapSlotEntryMut,
    /// `(l) -> bool`, `l` a constant: whether this instance owns its
    /// parameter local `l`, a link-time fact (the whole-program lends);
    /// a lent parameter's drop flag starts false (`own::drop_flags`).
    ParamOwned,
    /// `(place) -> bool`, the place word a constant: whether the drop
    /// slot of the place's node holds, the node and all under it whole,
    /// in the cell its root local lives in or the memory a `Deref` at its
    /// start reaches (the backend's slot layout, `own::elaborate`).
    CellFlag,
    /// `()`: the place the `Read move` right before it (past its counts)
    /// took, left empty: zeros where its values were, its enums still
    /// reading their variants (`own::elaborate`).
    Empty,
}

impl IntrinsicOp {
    pub const ALL: [IntrinsicOp; 19] = [
        IntrinsicOp::ListLen,
        IntrinsicOp::ListPush,
        IntrinsicOp::MapLen,
        IntrinsicOp::MapGet,
        IntrinsicOp::MapRemove,
        IntrinsicOp::MapSet,
        IntrinsicOp::StrIndex,
        IntrinsicOp::StrConcat,
        IntrinsicOp::StrEq,
        IntrinsicOp::Item,
        IntrinsicOp::MapSlots,
        IntrinsicOp::MapSlotNext,
        IntrinsicOp::MapSlotEntry,
        IntrinsicOp::MapEntry,
        IntrinsicOp::ListReserve,
        IntrinsicOp::MapSlotEntryMut,
        IntrinsicOp::ParamOwned,
        IntrinsicOp::CellFlag,
        IntrinsicOp::Empty,
    ];
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// Coercion kinds (§4.13.11 "Coercion kinds"; `crates/hd_check/DESIGN.md` (type checking) §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Coercion {
    Never,
    Weaken,
    Variance,
    WrapSome,
    RowSubsume,
    // Wire tags 5 and 6 are retired; preserve the surviving encodings.
    Supertrait = 7,
    SuspendFnToCtor,
    /// A least-common-type site's reserved slot whose value already has
    /// the site's type (`crates/hd_check/DESIGN.md` §4.3 step 6).
    Identity,
    /// `&S` or `&mut S` to a trait value `Tr` where `S: Tr`: the payload is
    /// the reference itself, and the dispatch table reads or passes it per
    /// method (`crates/hd_wasm/DESIGN.md` §15.2, "Trait value references").
    RefToTraitValue,
    /// A trait value reference `&dyn K` or `&mut dyn K` installed as the
    /// provider of a key `K` whose trait has a `&self` or `&mut self`
    /// method: the provider is the trait value `K` whose payload is the
    /// reference itself (`crates/hd_wasm/DESIGN.md` §15.2, "Providers of
    /// reference traits").
    RefAsProvider,
    /// The inverse of `RefAsProvider` at `$.use(K)`: the provider's payload
    /// read back as the trait value reference.
    ProviderAsRef,
    /// A checked function-row wrapper. Evidence is an interned type list
    /// of flat `(required key K, whole supplier H)` pairs, not SSA values.
    /// Unlike `RowSubsume`, this allocates a new callable environment.
    RowAdapt = 13,
}

/// A callee's choice: two words, a kind and a full 32-bit value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ChoiceKind {
    Impl,
    Bound,
    TraitValue,
    Builtin,
}

impl ChoiceKind {
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        [
            ChoiceKind::Impl,
            ChoiceKind::Bound,
            ChoiceKind::TraitValue,
            ChoiceKind::Builtin,
        ]
        .get(v as usize)
        .copied()
    }
}

/// A callee record in `extra`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Callee {
    Item {
        def: DefId,
        targs: TyList,
    },
    TraitMethod {
        trait_: DefId,
        method: DefId,
        self_ty: Ty,
        targs: TyList,
        choice: (ChoiceKind, u32),
    },
}

impl Callee {
    /// Decodes a callee record's words.
    #[must_use]
    pub fn from_words(w: &[u32]) -> Option<Callee> {
        match w {
            [0, def, targs] => Some(Callee::Item {
                def: DefId::from_raw(*def),
                targs: TyList(*targs),
            }),
            [1, trait_, method, self_ty, targs, kind, value] => Some(Callee::TraitMethod {
                trait_: DefId::from_raw(*trait_),
                method: DefId::from_raw(*method),
                self_ty: Ty(*self_ty),
                targs: TyList(*targs),
                choice: (ChoiceKind::from_u32(*kind)?, *value),
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn words(&self) -> Vec<u32> {
        match self {
            Callee::Item { def, targs } => vec![0, def.raw(), targs.0],
            Callee::TraitMethod {
                trait_,
                method,
                self_ty,
                targs,
                choice,
            } => {
                vec![
                    1,
                    trait_.raw(),
                    method.raw(),
                    self_ty.0,
                    targs.0,
                    choice.0 as u32,
                    choice.1,
                ]
            }
        }
    }
}

/// Providers of a call: pairs or an explicit context (3 words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Providers {
    None,
    Pairs(Range32),
    Context(Ref),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BodyKind {
    Fn,
    Init,
    TestCase,
    Fact,
    Default,
    DeriveInstance,
    /// A structure handle's body (`annot.handle.constants`): no
    /// parameter, and its value is built once per instance, on the first
    /// call, then returned again by every later call.
    Handle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CaptureMode {
    Copy,
    Move,
    Shared,
    /// A `Shared` capture whose cell may live in a slot of the maker's
    /// frame (`own::frame::plan_cells`): the closure never outlives the frame,
    /// and the ownership passes keep the local alive over every use of
    /// the closure.
    Frame,
}

impl CaptureMode {
    /// Whether the closure shares the local through a cell: `Shared` or
    /// `Frame`.
    #[must_use]
    pub fn shares(self) -> bool {
        matches!(self, Self::Shared | Self::Frame)
    }
}

/// Where a local lives when not in Wasm locals (`Body::storage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Storage {
    /// A slot of the function's frame on the local stack (§23.38).
    Slot,
    /// A counted cell object.
    Cell,
    /// Static data with a zero count word: a cell of a body that runs once
    /// per instance and whose value lives as long as the program.
    Static,
}

impl Storage {
    /// The kind a [`box_word`] names, `None` for no cell.
    #[must_use]
    pub fn from_word(w: u32) -> Option<Self> {
        [Self::Slot, Self::Cell, Self::Static]
            .get((w & box_word::KIND) as usize)
            .copied()
    }

    /// Its name in the text form.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Cell => "cell",
            Self::Static => "static",
        }
    }

    /// The storage named `s`.
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "slot" => Some(Self::Slot),
            "cell" => Some(Self::Cell),
            "static" => Some(Self::Static),
            _ => None,
        }
    }
}

/// The place encoding (`docs/compiler/tir-v2.md`, "Encoding, wire format
/// and caching"). A place operand is one data word ([`Op::Place`]): with
/// [`BARE_LOCAL`](place::BARE_LOCAL) set, a bare local `$n` in bits 0 to
/// 30, with no record; clear, the offset of a place record in `extra`:
/// `[len, root kind, root word, (root word 2), projection word ...]`.
/// A record is immutable, so instructions may share one.
pub mod place {
    /// The place word is a bare local, not a record offset.
    pub const BARE_LOCAL: u32 = 1 << 31;
    /// Root word: a `LocalId`.
    pub const ROOT_LOCAL: u32 = 0;
    /// Root words: a top-level binding's `DefId`, then its `Ty` (ID
    /// words, which the wire form remaps).
    pub const ROOT_GLOBAL: u32 = 1;
    /// Root word: a `Ref`, an instruction's value or a constant.
    pub const ROOT_VALUE: u32 = 2;
    /// Root word: a frame slot (tier 2, reserved; the verifier rejects it).
    pub const ROOT_FRAME: u32 = 3;
    /// Root word: a `Ref` to a `std.rt.memory.Ptr[T]` value; the place is
    /// the value of `T` at its address.
    pub const ROOT_PTR: u32 = 4;
    /// A projection word's kind, its top two bits.
    pub const PROJ_KIND: u32 = 3 << 30;
    /// A projection word's payload, bits 0 to 29.
    pub const PROJ_INDEX: u32 = (1 << 30) - 1;
    /// A struct field or tuple element, by declaration index.
    pub const PROJ_FIELD: u32 = 0;
    /// A dereference; no payload.
    pub const PROJ_DEREF: u32 = 1 << 30;
    /// A list element, and the next word is the index `Ref`. Its payload
    /// is 0 (checked against the buffer's length), [`ELEM_UNCHECKED`], or
    /// a length local's ([`ELEM_LEN_BASE`]).
    pub const PROJ_ELEM: u32 = 2 << 30;
    /// An element's payload: a proof removed its bounds check
    /// (`hd_opt::prove`), so the backend writes none. Places compare
    /// equal without any payload ([`super::Body::decode_place`] drops
    /// it), as Go's `IsInBounds` is a value apart from the address it
    /// guards.
    pub const ELEM_UNCHECKED: u32 = 1;
    /// An element's payload `k` at or above this: its bounds check reads
    /// the list's length from local `k - ELEM_LEN_BASE`, which
    /// `hd_opt::prove` writes before the loop the list is stable across.
    pub const ELEM_LEN_BASE: u32 = 2;
    /// An enum variant's payload, by variant declaration index (stage 2).
    pub const PROJ_VARIANT: u32 = 3 << 30;
}

/// Where a place starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceRoot {
    /// A parameter, binding or hidden local of the body: `$n`.
    Local(LocalId),
    /// A top-level binding and its type: `@def:ty`.
    Global(DefId, Ty),
    /// An instruction's value or a constant: `%n`. Read-only unless a
    /// dereference follows.
    Value(Ref),
    /// A slot of the function's frame (tier 2, reserved): `^n`.
    Frame(u32),
    /// The value a pointer value addresses (`std.rt.memory.Ptr[T]`):
    /// `ptr %n`. A `Read` of it moves the value out, and a `Store` into it
    /// drops nothing (`module.rt.ptr.read`, `module.rt.ptr.write.over`).
    Ptr(Ref),
}

/// One step of a place, read left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proj {
    /// A struct field or tuple element, by declaration index: `.k`.
    Field(u32),
    /// The place a reference value refers to: `*`.
    Deref,
    /// A slot of a list's buffer at an index value, bounds-checked unless
    /// a proof set its record word's [`place::ELEM_UNCHECKED`]: `[%n]`.
    /// The projected value is the list header.
    Elem(Ref),
    /// The payload of an enum value known to be that variant, by
    /// declaration index: `as .k`.
    Variant(u32),
}

/// A place: a root and its projections (`docs/compiler/tir-v2.md`,
/// "Places"). [`TirBuilder::place`] encodes one and
/// [`Body::decode_place`] decodes one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub root: PlaceRoot,
    pub proj: Vec<Proj>,
}

impl Place {
    /// A bare local, `[$n]`.
    #[must_use]
    pub fn local(l: LocalId) -> Place {
        Place {
            root: PlaceRoot::Local(l),
            proj: vec![],
        }
    }

    /// The place record's words after its length word; `None` for a bare
    /// local, which has no record.
    #[must_use]
    pub fn words(&self) -> Option<Vec<u32>> {
        use place::{
            PROJ_DEREF, PROJ_ELEM, PROJ_FIELD, PROJ_INDEX, PROJ_VARIANT, ROOT_FRAME, ROOT_GLOBAL,
            ROOT_LOCAL, ROOT_PTR, ROOT_VALUE,
        };
        let mut w = match self.root {
            PlaceRoot::Local(_) if self.proj.is_empty() => return None,
            PlaceRoot::Local(l) => vec![ROOT_LOCAL, l.raw()],
            PlaceRoot::Global(d, t) => vec![ROOT_GLOBAL, d.raw(), t.0],
            PlaceRoot::Value(r) => vec![ROOT_VALUE, r.0],
            PlaceRoot::Frame(s) => vec![ROOT_FRAME, s],
            PlaceRoot::Ptr(r) => vec![ROOT_PTR, r.0],
        };
        for p in &self.proj {
            match *p {
                Proj::Field(k) => w.push(PROJ_FIELD | (k & PROJ_INDEX)),
                Proj::Deref => w.push(PROJ_DEREF),
                Proj::Elem(r) => w.extend([PROJ_ELEM, r.0]),
                Proj::Variant(k) => w.push(PROJ_VARIANT | (k & PROJ_INDEX)),
            }
        }
        Some(w)
    }

    /// The values the place reads, in evaluation order: a value root, then
    /// each element index. Constants included.
    #[must_use]
    pub fn values(&self) -> Vec<u32> {
        let root = match self.root {
            PlaceRoot::Value(r) | PlaceRoot::Ptr(r) => Some(r.0),
            _ => None,
        };
        root.into_iter()
            .chain(self.proj.iter().filter_map(|p| match p {
                Proj::Elem(r) => Some(r.0),
                _ => None,
            }))
            .collect()
    }

    /// Whether a `Store` or a mutable `Ref` may reach the place: a local
    /// or global root, or a dereference or an element among the
    /// projections. An element is a slot of the buffer a list header
    /// names, as a dereference is what a reference names, and a pointer's
    /// target is in the heap. A value root is never written in place.
    #[must_use]
    pub fn writable(&self) -> bool {
        matches!(
            self.root,
            PlaceRoot::Local(_) | PlaceRoot::Global(..) | PlaceRoot::Ptr(_)
        ) || self
            .proj
            .iter()
            .any(|p| matches!(p, Proj::Deref | Proj::Elem(_)))
    }
}

/// The text form: `[*$0 .1 [%5] as .2]`. Dereferences right after the root
/// are written before it, later ones as `*` after the projection they
/// follow; fields and variants by index; a constant value as `%cK` (row
/// `K` of `consts`); a global root as `@def:ty`.
impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = |r: Ref| match r.as_inst() {
            Some(i) => format!("%{}", i.0),
            None => format!("%c{}", r.0 & !Ref::CONST_BIT),
        };
        let lead = self.proj.iter().take_while(|p| **p == Proj::Deref).count();
        f.write_str("[")?;
        f.write_str(&"*".repeat(lead))?;
        match self.root {
            PlaceRoot::Local(l) => write!(f, "${}", l.raw())?,
            PlaceRoot::Global(d, t) => write!(f, "@{}:{}", d.raw(), t.0)?,
            PlaceRoot::Value(r) => f.write_str(&value(r))?,
            PlaceRoot::Frame(s) => write!(f, "^{s}")?,
            PlaceRoot::Ptr(r) => write!(f, "ptr {}", value(r))?,
        }
        for p in &self.proj[lead..] {
            match *p {
                Proj::Field(k) => write!(f, " .{k}")?,
                Proj::Deref => f.write_str(" *")?,
                Proj::Elem(r) => write!(f, " [{}]", value(r))?,
                Proj::Variant(k) => write!(f, " as .{k}")?,
            }
        }
        f.write_str("]")
    }
}

impl std::str::FromStr for Place {
    type Err = String;

    /// Parses the text form [`Place`]'s `Display` writes.
    fn from_str(s: &str) -> Result<Place, String> {
        let bad = || format!("bad place `{s}`");
        let inner = s
            .strip_prefix('[')
            .and_then(|x| x.strip_suffix(']'))
            .ok_or_else(bad)?;
        let num = |t: &str| t.parse::<u32>().map_err(|_| bad());
        let value = |t: &str| match t.strip_prefix("%c") {
            Some(k) => num(k).map(Ref::konst),
            None => t.strip_prefix('%').ok_or_else(bad).and_then(num).map(Ref),
        };
        let mut toks = inner.split_whitespace();
        let mut first = toks.next().ok_or_else(bad)?;
        let ptr = first == "ptr";
        if ptr {
            first = toks.next().ok_or_else(bad)?;
        }
        let root_text = first.trim_start_matches('*');
        let lead = first.len() - root_text.len();
        let root = match root_text.as_bytes().first() {
            Some(b'$') => PlaceRoot::Local(LocalId::from_raw(num(&root_text[1..])?)),
            Some(b'@') => {
                let (d, t) = root_text[1..].split_once(':').ok_or_else(bad)?;
                PlaceRoot::Global(DefId::from_raw(num(d)?), Ty(num(t)?))
            }
            Some(b'%') if ptr => PlaceRoot::Ptr(value(root_text)?),
            Some(b'%') => PlaceRoot::Value(value(root_text)?),
            Some(b'^') => PlaceRoot::Frame(num(&root_text[1..])?),
            _ => return Err(bad()),
        };
        if ptr && !matches!(root, PlaceRoot::Ptr(_)) {
            return Err(bad());
        }
        let mut proj = vec![Proj::Deref; lead];
        while let Some(t) = toks.next() {
            proj.push(if t == "*" {
                Proj::Deref
            } else if t == "as" {
                let k = toks.next().and_then(|k| k.strip_prefix('.'));
                Proj::Variant(num(k.ok_or_else(bad)?)?)
            } else if let Some(k) = t.strip_prefix('.') {
                Proj::Field(num(k)?)
            } else if let Some(v) = t.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                Proj::Elem(value(v)?)
            } else {
                return Err(bad());
            });
        }
        Ok(Place { root, proj })
    }
}

/// The one step of a value projection read ([`Body::read_step`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// `[%v .k]`: field or element `k` of the value.
    Field(u32),
    /// `[*%v .k]`: field `k` of a reference's referent.
    DerefField(u32),
    /// `[%v as .V .k]`: field `k` of variant `V`'s payload.
    Variant(u32, u32),
}

/// The data word b of a `Box` (`own::storage::boxes`): the cell's kind,
/// a [`Storage`] as `u32` or `NONE` for a local the emitter keeps in Wasm
/// locals after all ([`crate::own::settle`]), and [`box_word::ZERO`].
pub mod box_word {
    /// The kind's bits.
    pub const KIND: u32 = 0xff;
    /// A path may leave the box's block before the declaration fills the
    /// cell, so its payload starts zero, which every release skips.
    pub const ZERO: u32 = 0x100;
}

/// The data word b of a `Ref` (`docs/compiler/tir-v2.md`, "Reads, stores
/// and references").
pub mod ref_word {
    /// `&mut`; clear for `&`.
    pub const MUT: u32 = 1;
    /// Set by the move pass ("Move" rule 4): a `Ref` of a whole local at
    /// its last access takes the local's count of its cell, and leaves the
    /// local empty. The checker never sets it (invariant 22).
    pub const TAKES: u32 = 2;
    /// Set by the checker: the `Ref` makes a local reference, `&T` or
    /// `&mut T`, or the address a `&self` or `&mut self` receiver takes,
    /// which never outlives its use
    /// (spec/lang/13-values-and-references.md#local-references).
    pub const LOCAL: u32 = 8;
    /// Set by insertion: a `Ref` of the storage of a frame local, one that
    /// the passes keep alive over every use of a value that may point into
    /// it, so the backend may keep it in a slot of the function's frame
    /// (`crates/hd_wasm/DESIGN.md` §23.38). Its count is never
    /// taken (`TAKES`). The checker never sets it (invariant 22).
    pub const FRAME: u32 = 16;
    /// Set by insertion: a `for x in &xs` loop's element reference into
    /// the list a read made before the loop (`own::insert::pinned_elements`,
    /// #1399 9k). The read pins the buffer over the loop, by its count or,
    /// where it borrows, by its span, so the reference addresses no storage
    /// of the read's own and the loop runs no `IsUnique`. The checker
    /// never sets it (invariant 22).
    pub const PINS: u32 = 32;
    /// Set by insertion: a `LOCAL` argument of a suspending call made cold
    /// whose frame only awaits in the caller's root
    /// (`own::lent`, #1355). The passes keep its local alive over those
    /// awaits, so the backend builds that frame with no count of the
    /// local's object (`Part::Lent`). The checker never sets it
    /// (invariant 22).
    pub const LENT: u32 = 64;
    /// Set by `own::storage::settle`: an argument collection passes by
    /// value (§23.40), a read of its place.
    pub const PROMOTED: u32 = 128;
}

/// An `IsUnique`'s data word b (`docs/compiler/tir-v2.md`, "Hoisting"): 0
/// for one that stands at its change, as insertion writes it.
pub mod unique_word {
    /// Hoisted out of the loop that follows it in its block: it runs only
    /// when that loop's entry test lets the loop in, once before it.
    pub const HOISTED: u32 = 1;
    /// Initialize a loop decision, without making its buffer unique yet.
    pub const DEFERRED_INIT: u32 = 5;
    /// Initialize a deferred decision whose successful buffer stays stable.
    pub const DEFERRED_CACHE: u32 = 7;
    /// Make unique on the first reached write, then reuse that decision.
    pub const DEFERRED: u32 = 6;
    /// Of all of a fresh-buffer place (`own::hoist::fresh_buffers`), whose
    /// buffer nothing else can hold: always unique, so no check runs.
    pub const FRESH: u32 = 3;
    /// Of all of a local an earlier `IsUnique` of its block already made
    /// unique, with nothing between that can share the buffer
    /// (`own::hoist::repeats`): no check runs. This decision precedes
    /// hoisting, which may later move its earlier check out of the loop.
    pub const REPEAT: u32 = 4;
}

/// A `Drop`'s data word b: its reason (`own::Reason::from_word`) and flags.
pub mod drop_word {
    /// Of all of a buffer this body allocated that nothing else holds
    /// (`own::hoist::fresh_buffers`, #2201): it frees it with no test, as
    /// Swift's SIL ends an unshared `alloc_ref` in `dealloc_ref`.
    pub const FRESH: u32 = 1 << 4;
    /// Whether word `w`, which may be `NONE`, has [`FRESH`].
    #[must_use]
    pub fn fresh(w: u32) -> bool {
        w != super::NONE && w & FRESH != 0
    }
}

/// How a `Read` reads its place (its `Meta` word b;
/// `docs/compiler/tir-v2.md`, "Read modes"). The checker writes `Use`;
/// the ownership passes (stage 2) replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ReadMode {
    /// Undecided: the checker's.
    Use,
    /// One more count of each counted leaf; the place is unchanged.
    Copy,
    /// The place's own count; the place is empty afterwards.
    Move,
    /// No count; the place must not change while the value lives.
    Borrow,
}

impl ReadMode {
    pub const ALL: [ReadMode; 4] = [
        ReadMode::Use,
        ReadMode::Copy,
        ReadMode::Move,
        ReadMode::Borrow,
    ];
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// Local flags (u16): READ | ASSIGNED | MUTATED | CAPTURED | `CAPTURED_ASSIGNED` | PARAM.
pub mod local_flags {
    pub const READ: u16 = 1;
    pub const ASSIGNED: u16 = 2;
    pub const MUTATED: u16 = 4;
    pub const CAPTURED: u16 = 8;
    pub const CAPTURED_ASSIGNED: u16 = 16;
    pub const PARAM: u16 = 32;
    /// A `:=` binding, which `=` cannot reassign.
    pub const SHORT: u16 = 64;
    /// A parameter of the function whose default is being checked, at or
    /// after the default's own: named, but not yet visible
    /// (`fn.default.later-parameter`). A read is an error.
    pub const NOT_YET: u16 = 128;
    /// A view: a binding that holds a part of a value in place, with no
    /// count of its own, and that nothing takes out of. A match guard's
    /// twin of its arm's binding is one (`flow.match.guard.borrow`).
    pub const VIEW: u16 = 256;
}

/// Sub-body flags (u8).
pub mod sub_flags {
    /// A generic local function's body (`fn.local.declare` with its own
    /// type parameters), checked once over them. Its defining `Closure`
    /// (record: capture start, capture count) makes no value. Each use is
    /// a `Closure` of the same sub whose record adds the index of the
    /// function's first own type parameter and its type arguments; that
    /// value's code is the body's instance at those arguments.
    pub const GENERIC: u8 = 1;
    /// On the root sub-body: the ownership passes run in exclusive mode
    /// (stage 8, #1118). A read through a `&T` or `&mut T` parameter may
    /// borrow across a call that may write, since the callers of such a
    /// body pass only places that stage 7 lets nothing else reach for the
    /// call. The checker never sets it.
    pub const EXCLUSIVE: u8 = 2;
    /// The root result forwards an input loan. Callers that keep its bits
    /// state their own retain; this body returns no owned count.
    pub const BORROWED_RESULT: u8 = 4;
}

/// Ownership mode in a `Return`'s second word. `NONE` keeps the ordinary
/// owned result convention; only ownership planning writes this mode.
pub mod return_word {
    pub const BORROWED: u32 = 1;
    /// Out of a suspending body by its cancellation: no value (a point's
    /// cancel block ends with it, `own::scopes`).
    pub const CANCELLED: u32 = 2;
}

/// A suspension point's side record (16 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SuspRow {
    pub inst: u32,
    pub scopes: Range32,
    pub hook_site: u32,
}
const _: () = assert!(core::mem::size_of::<SuspRow>() == 16);

/// Which side of collection a body is on (`docs/future-work/INSTANCE_TIR.md` at 421bcec83,
/// "Phase mark"): the checker's generic form, or one instance's form, whose
/// types are concrete and whose trait calls are chosen (invariant 24,
/// [`verify_owned`]). A non-generic item's checked body becomes its one
/// instance's by setting the mark, with no copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Phase {
    /// The checker's output: may name its item's type parameters.
    #[default]
    Checked,
    /// One instance's body, built by the driver at its type arguments.
    Instance,
    /// A body built lowered, its counts already stated: a count shim
    /// (`hd_layout::shims`). No ownership pass runs on it, so invariant 22
    /// does not apply, and a backend lowers each count statement as
    /// written, on a borrowed value too.
    Lowered,
    /// An instance body after its last lowering (`own::elaborate`): what a
    /// backend emits, checked by the lowered-TIR contract
    /// (`verify_owned`, [`owned::lowered`]).
    Elaborated,
}

impl Phase {
    /// The phase of wire byte `v`.
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        [
            Phase::Checked,
            Phase::Instance,
            Phase::Lowered,
            Phase::Elaborated,
        ]
        .get(usize::from(v))
        .copied()
    }
}

/// One reference-bearing fragment of a call's argument or result. Paths
/// stay relative to a value, so instance layout supplies physical slots.
/// Equal origins name possible loan sources within this call. An open
/// type parameter expands its fragment to the instance's reference slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoanSlot {
    pub call: u32,
    pub value: u32,
    pub path: Vec<LoanField>,
    pub origin: u32,
    pub expand: bool,
}

impl LoanSlot {
    /// Both instruction ids follow the existing body's copy map.
    pub(crate) fn remap_values(&mut self, mut map: impl FnMut(u32) -> Option<u32>) -> Option<()> {
        self.call = map(self.call)?;
        self.value = map(self.value)?;
        Some(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoanField {
    Field(u32),
    Variant(u32),
}

/// One body: the columns of §4.13.11 "Encoding".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub item: DefId,
    pub kind: BodyKind,
    /// Generic or instance ([`Phase`]): hashed, so an instance body's TIR
    /// hash differs from its checked body's.
    pub phase: Phase,
    pub tags: Vec<Tag>,
    pub data: Vec<[u32; 2]>,
    pub ty: Vec<Ty>,
    pub syn: Vec<NodeIdx>,
    pub extra: Vec<u32>,
    pub local_ty: Vec<Ty>,
    pub local_name: Vec<Symbol>,
    pub local_syn: Vec<NodeIdx>,
    pub local_flags: Vec<u16>,
    pub sub_root: Vec<u32>,
    /// Parameter locals per sub-body: `start` points at a length prefix in
    /// `extra`, and `len` is the number of following local IDs. An empty
    /// range reaches no words, regardless of `start`. Read through
    /// [`Body::sub_param_locals`]; the root's ordinary parameters instead
    /// come from local flags, excluding the nested sub-bodies' parameters.
    pub sub_params: Vec<Range32>,
    pub sub_parent: Vec<SubId>,
    pub sub_flags: Vec<u8>,
    pub label_inst: Vec<u32>,
    pub cap_local: Vec<LocalId>,
    pub cap_mode: Vec<CaptureMode>,
    pub susp: Vec<SuspRow>,
    /// The body's constants: type and bits (`Ref::konst(i)` is row i).
    /// A `string` constant's bits index `strings`.
    pub consts: Vec<(Ty, u64)>,
    /// The body's string literals, by `string` constant.
    pub strings: Vec<Box<str>>,
    /// What a `Fact` body's facts are attached to (`Point`, `Point.x`, a
    /// function's name), which a panic while it runs names
    /// (`annot.fact.eval.panic-runtime`); empty for any other kind.
    pub label: Box<str>,
    /// The overflow-checked operations whose integer type is a literal's
    /// `usize` default (`types.literal.local.default`): per row, the
    /// instruction and the local it reads plus one, or 0 when it reads
    /// none (`flow.panic.report.fallback`). Sorted by instruction. A
    /// location-like column, outside the hash: it changes no code.
    pub fallback: Vec<[u32; 2]>,
    /// The calls whose result a bare `&T` or `&mut T` binding stores: per
    /// row, the call and the argument whose reference the result holds
    /// (`ref.excl.call`), which the backend may register as a run-time
    /// loan (`ref.excl.alias.loan`). The checker's output, hashed.
    pub held_args: Vec<(u32, u32)>,
    /// Per-call origin groups and value-relative reference fragments,
    /// exported after generic argument/result dependencies are lined up.
    pub loan_slots: Vec<LoanSlot>,
    /// The ownership passes' exit set (`docs/compiler/tir-v2.md`,
    /// "Ownership statements"): per row, a sub-body and an owned local of
    /// it that may still hold a value at the sub-body's one exit, which
    /// drops it. Sorted, each row once. A pass output: empty in the
    /// checker's TIR (invariant 22); the wire's owned section.
    pub exit_drops: Vec<(SubId, LocalId)>,
    /// Where each local lives (`own/storage.rs`): per row, a sub-body, a
    /// local and its storage there. A local with no row lives in Wasm
    /// locals. Sorted, each pair once. A pass output: empty in the
    /// checker's TIR (invariant 22); the wire's owned section.
    pub storage: Vec<(SubId, LocalId, Storage)>,
    /// Each suspension point's cancel block, as rustc's `Yield` names its
    /// `drop` target: per row, a point and a `Block` no list holds, which
    /// the point runs when cancelled there; known-null fills it. Sorted,
    /// one row per point. A pass output: empty in the checker's TIR
    /// (invariant 22); the wire's owned section.
    pub cancel: Vec<(u32, u32)>,
    /// The cleanup suites' copies (`own::scopes`): each a wrapper `Scope`
    /// that runs one suite on one way out of its cleanup scope. A pass
    /// output, like [`Body::cancel`].
    pub suites: Vec<u32>,
    /// The hoisting pass's stable list places (`docs/compiler/tir-v2.md`,
    /// "Hoisting"): per row, a `Loop` and a place word of a list whose
    /// header and buffer nothing in the loop replaces, so `hd_opt::prove`
    /// may root the loop's accesses at its header and keep its length in
    /// a local. The
    /// place is rooted at a local, with fields and dereferences only.
    /// Then the list's region around the loop: the last instruction
    /// before the loop and the first after it that may change the list
    /// (`NONE`: none), so it keeps its length from any instruction past the
    /// first through any statement holding the loop that ends before the
    /// second. Last, 1 when the loop keeps the buffer too, 0 when it keeps
    /// only the length (an `IsUnique` in it may copy the buffer). Sorted,
    /// each row once. A pass output: empty in the checker's TIR
    /// (invariant 22); the wire's owned section.
    pub stable_lists: Vec<[u32; 5]>,
    /// Unary RC forwarding identities: (wrapper/construction, source Read).
    /// Sorted ownership-pass output. Single-hop scopes/coercions need no row;
    /// read modes remain authoritative for whether the identity is borrowed.
    pub forwarded_reads: Vec<(u32, u32)>,
    /// Per instruction of a body the ownership passes made, the checker's
    /// instruction it came from, which a call it writes names as its
    /// anchor in the line table (`crates/hd_wasm/DESIGN.md` §15.9),
    /// since the anchor is resolved against the `check` entry's body; `NONE`
    /// for an instruction the passes added. Empty in the checker's TIR,
    /// where each instruction is its own ([`Body::anchor`]). The driver
    /// fills it before the passes and every rebuild carries it; the
    /// wire's owned section.
    pub anchors: Vec<u32>,
    /// The inliner's copies, each `[first, len, callee, call]`, sorted:
    /// an anchor `INLINED | a` with `first <= a < first + len` names
    /// instruction `a - first` of the callee's checker body, which the
    /// copy's call at anchor `call` ran ([`Body::inlined_frame`]); `call`
    /// is itself `INLINED | a` of an earlier row for a copy made inside a
    /// copy (release, bottom-up). The wire's owned section.
    pub inlined: Vec<[u32; 4]>,
    /// Sparse source-instruction overrides for dense inlined anchors: the
    /// virtual anchor and its instruction in the frame's checked body.
    /// Sorted, pass output only; the wire's owned section.
    pub inlined_origins: Vec<[u32; 2]>,
    /// Logical shared captures whose function values were eliminated by
    /// inlining: (creation sub-body, row in `cap_local`/`cap_mode`). Their cell
    /// alias/loan contract survives the environment. Pass output; the wire's
    /// owned section.
    pub inlined_shared: Vec<(SubId, u32)>,
    /// Copy-capture provenance after literal elimination: (creation sub-body,
    /// snapshot local, creator local). Only captures share creator own-loan
    /// context; ordinary reference copies remain distinct aliases. Snapshots
    /// are appended after their creators. Pass output; the wire's owned
    /// section.
    pub inlined_capture_origins: Vec<(SubId, LocalId, LocalId)>,
}

impl Body {
    #[must_use]
    pub fn new(item: DefId, kind: BodyKind) -> Self {
        Self {
            item,
            kind,
            phase: Phase::Checked,
            tags: vec![],
            data: vec![],
            ty: vec![],
            syn: vec![],
            extra: vec![],
            local_ty: vec![],
            local_name: vec![],
            local_syn: vec![],
            local_flags: vec![],
            sub_root: vec![],
            sub_params: vec![],
            sub_parent: vec![],
            sub_flags: vec![],
            label_inst: vec![],
            cap_local: vec![],
            cap_mode: vec![],
            susp: vec![],
            consts: vec![],
            strings: vec![],
            label: "".into(),
            fallback: vec![],
            held_args: vec![],
            loan_slots: vec![],
            exit_drops: vec![],
            storage: vec![],
            cancel: vec![],
            suites: vec![],
            stable_lists: vec![],
            forwarded_reads: vec![],
            anchors: vec![],
            inlined: vec![],
            inlined_origins: vec![],
            inlined_shared: vec![],
            inlined_capture_origins: vec![],
        }
    }

    /// The instruction of the checker's body that instruction `i` came
    /// from, which the line table names ([`Body::anchors`]), or `NONE`.
    #[must_use]
    pub fn anchor(&self, i: u32) -> u32 {
        if self.anchors.is_empty() {
            i
        } else {
            self.anchors.get(i as usize).copied().unwrap_or(NONE)
        }
    }

    /// A number past every instruction of the checker's body that an
    /// instruction of this one comes from ([`Body::anchor`]): at least the
    /// body's length, and more when a rebuild before the passes deleted
    /// some (`root_places`).
    #[must_use]
    pub fn anchor_len(&self) -> usize {
        let named = self.anchors.iter().filter(|&&a| a != NONE).max();
        named.map_or(0, |&a| a as usize + 1).max(self.len())
    }

    /// Whether instruction `i` is a `Coerce` that keeps its operand's
    /// values as they are (`Weaken`, `Variance`, `RowSubsume`,
    /// `SuspendFnToCtor`, `Identity`, `ProviderAsRef`): its value is its
    /// operand's, so it takes the operand's count, or none of a borrow.
    #[must_use]
    pub fn same_value_coercion(&self, i: usize) -> bool {
        self.tags.get(i) == Some(&Tag::Coerce)
            && self
                .extra
                .get(self.data[i][1] as usize + 1)
                .is_some_and(|&k| {
                    [
                        Coercion::Weaken,
                        Coercion::Variance,
                        Coercion::RowSubsume,
                        Coercion::SuspendFnToCtor,
                        Coercion::Identity,
                        Coercion::ProviderAsRef,
                    ]
                    .iter()
                    .any(|c| *c as u32 == k)
                })
    }

    /// The explicit arguments of any call form. Dynamic calls retain their
    /// method/type/evidence header; host calls have no provider footer.
    /// Noncalls and empty argument records have no arguments.
    #[must_use]
    pub fn call_args(&self, i: usize) -> &[u32] {
        let Some(&tag) = self.tags.get(i) else {
            return &[];
        };
        if tag == Tag::CallHost {
            return self.record(self.data[i][1]);
        }
        let Some(footer) = self.call_footer(i) else {
            return &[];
        };
        let start = self.data[i][1] as usize + 1;
        // call_footer validated the complete record and its three-word
        // footer, so this prefix is in bounds even for malformed headers.
        let record = &self.extra[start..footer];
        if tag == Tag::CallDyn {
            // Split before indexing evidence: no header-length addition can
            // overflow on malformed records, including on a 32-bit host.
            let Some((&evidence, tail)) = record.get(2..).and_then(|r| r.split_first()) else {
                return &[];
            };
            tail.get(evidence as usize..).unwrap_or(&[])
        } else {
            record
        }
    }

    /// The provider footer of any call form; noncalls and empty records
    /// have none. Footer kinds and range words are metadata, not SSA values.
    #[must_use]
    pub fn call_providers(&self, i: usize) -> Providers {
        let Some(at) = self.call_footer(i) else {
            return Providers::None;
        };
        match self.extra.get(at..at + 3) {
            Some(&[1, start, len]) => Providers::Pairs(Range32 { start, len }),
            Some(&[2, value, _]) => Providers::Context(Ref(value)),
            _ => Providers::None,
        }
    }

    /// Replaces an existing call footer without changing argument records.
    /// Returns false for a noncall or malformed call record.
    pub fn set_call_providers(&mut self, i: usize, providers: Providers) -> bool {
        let Some(at) = self.call_footer(i) else {
            return false;
        };
        let words = match providers {
            Providers::None => [0, 0, 0],
            Providers::Pairs(range) => [1, range.start, range.len],
            Providers::Context(value) => [2, value.0, 0],
        };
        self.extra[at..at + 3].copy_from_slice(&words);
        true
    }

    /// The absolute `extra` position of the provider kind word.
    pub(crate) fn call_footer(&self, i: usize) -> Option<usize> {
        if !matches!(
            self.tags.get(i)?,
            Tag::Call
                | Tag::Await
                | Tag::CallValue
                | Tag::AwaitValue
                | Tag::DefaultCall
                | Tag::CallDyn
        ) {
            return None;
        }
        let at = self.data.get(i)?[1] as usize;
        let len = *self.extra.get(at)? as usize;
        if len < 3 {
            return None;
        }
        let end = at.checked_add(1)?.checked_add(len)?;
        self.extra.get(at..end)?;
        Some(end - 3)
    }

    /// The checked `(required key K, whole supplier H)` recipe of a row
    /// adapter. The list's type words are metadata, never value operands.
    #[must_use]
    pub fn row_adaptation(&self, i: usize) -> Option<TyList> {
        if self.tags.get(i) != Some(&Tag::Coerce) {
            return None;
        }
        let at = self.data.get(i)?[1] as usize;
        let record = self.extra.get(at..at.checked_add(3)?)?;
        (record[0] == 2 && record[1] == Coercion::RowAdapt as u32 && record[2] != NONE)
            .then_some(TyList(record[2]))
    }

    /// Whether instruction `v` is a `ProviderAsRef` coercion of a
    /// `ProviderGet` (`$.use(K)`): a reference to a provider the body
    /// received, which its caller's scope holds for the whole body. It
    /// counts nothing, so its `Drop` ends no borrow through it.
    #[must_use]
    pub fn provider_ref(&self, v: usize) -> bool {
        self.tags.get(v) == Some(&Tag::Coerce)
            && self.extra.get(self.data[v][1] as usize + 1)
                == Some(&(Coercion::ProviderAsRef as u32))
            && Ref(self.data[v][0])
                .as_inst()
                .is_some_and(|g| self.tags.get(g.0 as usize) == Some(&Tag::ProviderGet))
    }

    /// Whether instruction `v` is a top-level binding's initializer: the
    /// module's init body stores its value, through identity coercions,
    /// whole into a global. Only such a list literal of constants is
    /// static data (`TABLE := [1, 2, 3]`); a literal anywhere else builds
    /// a fresh buffer each time it runs.
    #[must_use]
    pub fn top_level_initializer(&self, v: usize) -> bool {
        if self.kind != BodyKind::Init {
            return false;
        }
        let stored = |s: usize| {
            let mut r = self.data[s][1];
            for _ in 0..32 {
                let Some(i) = Ref(r).as_inst().map(|i| i.0 as usize) else {
                    return false;
                };
                if i == v {
                    return true;
                }
                if self.tags[i] != Tag::Coerce
                    || self.record(self.data[i][1]).first() != Some(&(Coercion::Identity as u32))
                {
                    return false;
                }
                r = self.data[i][0];
            }
            false
        };
        (0..self.len()).any(|s| {
            self.tags[s] == Tag::Store && {
                let p = self.view(self.data[s][0]);
                p.whole() && matches!(p.root, Root::Global(..)) && stored(s)
            }
        })
    }

    /// Whether value `v` is a `for x in &xs` loop's element reference
    /// into the list a read pins over the loop (`ref_word::PINS`,
    /// `Ref [%h [%i]]`).
    #[must_use]
    pub fn pinned_element_ref(&self, v: usize) -> bool {
        self.tags.get(v) == Some(&Tag::Ref)
            && self.data[v][1] & ref_word::PINS != 0
            && self
                .view(self.data[v][0])
                .value()
                .is_some_and(|h| self.tags[h as usize] == Tag::Read)
    }

    /// Whether `v` forwards borrowed bits through a same-value coercion,
    /// a wrapper, or a selection of whole parameter loans. It owns no count.
    #[must_use]
    pub fn is_borrowed_coercion(&self, v: u32) -> bool {
        self.borrowed_alias(v).is_some()
            || self
                .parameter_join_reads(v)
                .is_some_and(|reads| reads.into_iter().all(|r| self.is_borrow(r)))
    }

    /// The one `borrow` read whose RC identity this forwarding value keeps.
    #[must_use]
    pub fn borrowed_alias(&self, v: u32) -> Option<u32> {
        if Ref(v).as_inst().is_none() || v as usize >= self.len() {
            return None;
        }
        let x = if self.same_value_coercion(v as usize) {
            Some(self.data[v as usize][0])
        } else {
            self.wrapper_value(v as usize)
        };
        if let Some(x) = x {
            if self.is_borrow(x) {
                return Some(x);
            }
        } else if !matches!(
            self.tags[v as usize],
            Tag::NewStruct | Tag::NewTuple | Tag::NewVariant | Tag::Coerce | Tag::Scope
        ) {
            return None;
        }
        let root = self.forwarded_read(v)?;
        self.is_borrow(root).then_some(root)
    }

    /// The canonical RC source, before ownership selects its read mode.
    pub(crate) fn forwarded_read(&self, v: u32) -> Option<u32> {
        self.forwarded_reads
            .binary_search_by_key(&v, |&(value, _)| value)
            .ok()
            .map(|at| self.forwarded_reads[at].1)
    }

    /// Canonical Read sources of unary forwarding instructions. SSA operands
    /// precede their users, so each edge is visited once, without recursion.
    pub(crate) fn forwarding_reads(
        &self,
        constructors: &[u32],
        mut visit: impl FnMut(),
    ) -> Vec<u32> {
        let mut roots = vec![NONE; self.len()];
        for i in 0..self.len() {
            visit();
            if self.tags[i] == Tag::Read {
                roots[i] = u32_of(i);
                continue;
            }
            let value = constructors
                .get(i)
                .copied()
                .filter(|&v| v != NONE)
                .or_else(|| {
                    if self.same_value_coercion(i) {
                        Some(self.data[i][0])
                    } else {
                        self.wrapper_value(i)
                    }
                });
            if let Some(source) = value
                .and_then(|v| Ref(v).as_inst())
                .map(|v| v.0 as usize)
                .filter(|&source| source < i)
            {
                roots[i] = roots[source];
            }
        }
        roots
    }

    pub(crate) fn index_forwarded_reads(&mut self, constructors: &[u32]) {
        let roots = self.forwarding_reads(constructors, || {});
        self.forwarded_reads.clear();
        for (i, &root) in roots.iter().enumerate() {
            if root == NONE || root as usize == i {
                continue;
            }
            let direct = if self.same_value_coercion(i) {
                Some(self.data[i][0])
            } else {
                self.wrapper_value(i)
            };
            if constructors.get(i).is_some_and(|&v| v != NONE) || direct != Some(root) {
                self.forwarded_reads.push((u32_of(i), root));
            }
        }
    }

    /// The value a wrapper `Scope` `s` takes from its block's last
    /// member, a `Break` to the block, when the block has no tail and the
    /// `Scope` no cleanup: an inlined call's copy (`crate::inline`). Only
    /// a block with that one `Break` is a wrapper, which the ownership
    /// passes check where they rely on it (`own::derive`) and invariant
    /// 23 checks for each one a borrow goes through.
    #[must_use]
    pub fn wrapper_value(&self, s: usize) -> Option<u32> {
        if self.tags.get(s) != Some(&Tag::Scope) || !self.record(self.data[s][1]).is_empty() {
            return None;
        }
        let blk = self.data[s][0] as usize;
        if self.tags.get(blk) != Some(&Tag::Block) || self.data[blk][1] != NONE {
            return None;
        }
        let x = *self.record(self.data[blk][0]).last()? as usize;
        (self.tags.get(x) == Some(&Tag::Break)
            && self.label_inst.get(self.data[x][0] as usize) == Some(&u32_of(blk))
            && self.data[x][1] != NONE)
            .then_some(self.data[x][1])
    }

    /// The value of each scope with exactly one producing exit. Instructions
    /// after that exit need not define its result; cleanup remains intact.
    /// Two instruction censuses visit each exit and scope once.
    pub(crate) fn scope_forwarders(&self, mut visit: impl FnMut()) -> Vec<u32> {
        let mut counts = vec![0u32; self.len()];
        let mut values = vec![NONE; self.len()];
        for (i, &tag) in self.tags.iter().enumerate() {
            visit();
            if tag == Tag::Break
                && let Some(&block) = self.label_inst.get(self.data[i][0] as usize)
                && let Some(count) = counts.get_mut(block as usize)
            {
                *count += 1;
                values[block as usize] = self.data[i][1];
            }
        }
        let mut sources = vec![NONE; self.len()];
        for (i, &tag) in self.tags.iter().enumerate() {
            visit();
            if tag == Tag::Scope
                && self.record(self.data[i][1]).is_empty()
                && let Some(&count) = counts.get(self.data[i][0] as usize)
                && count == 1
                && self.tags.get(self.data[i][0] as usize) == Some(&Tag::Block)
                && self.data[self.data[i][0] as usize][1] == NONE
            {
                sources[i] = values[self.data[i][0] as usize];
            }
        }
        sources
    }

    /// Whole parameter reads forwarded by a conditional value. The fixed
    /// proof budget bounds both fan-out and depth; exceeding it leaves the
    /// value owned. No source projection, frame address or native recursion.
    #[must_use]
    pub fn parameter_join_reads(&self, value: u32) -> Option<Vec<u32>> {
        self.parameter_join_with(value, |_| true)
    }

    pub(crate) fn parameter_join_with(
        &self,
        value: u32,
        mut permits: impl FnMut(u32) -> bool,
    ) -> Option<Vec<u32>> {
        const BUDGET: usize = 16;
        // Ordinary values and scalar coercions take the old cheap path.
        // Only a coercion chain that reaches a join allocates a worklist.
        let mut head = value;
        let mut found = false;
        for _ in 0..BUDGET {
            let i = Ref(head).as_inst()?.0 as usize;
            match *self.tags.get(i)? {
                Tag::If
                | Tag::Match
                | Tag::SwitchTag
                | Tag::SwitchInt
                | Tag::SwitchChar
                | Tag::SwitchStr => {
                    found = true;
                    break;
                }
                Tag::Coerce if self.same_value_coercion(i) && permits(head) => {
                    head = self.data[i][0];
                }
                _ => return None,
            }
        }
        if !found {
            return None;
        }
        let mut todo = vec![value];
        let mut reads = Vec::new();
        let mut joined = false;
        let mut work = 0;
        while let Some(v) = todo.pop() {
            work += 1;
            if work + todo.len() > BUDGET || Ref(v).as_inst().is_none() || !permits(v) {
                return None;
            }
            let i = v as usize;
            match *self.tags.get(i)? {
                Tag::Read => {
                    let p = self.view(self.data[i][0]);
                    let l = p.local().filter(|_| p.whole())?;
                    if self.local_flags[l as usize] & local_flags::PARAM == 0 {
                        return None;
                    }
                    reads.push(v);
                }
                Tag::Coerce if self.same_value_coercion(i) => todo.push(self.data[i][0]),
                Tag::Block => {
                    let tail = self.data[i][1];
                    if tail != NONE {
                        todo.push(tail);
                    } else if self.ty[i] != Ty::NEVER {
                        return None;
                    }
                }
                Tag::If
                | Tag::Match
                | Tag::SwitchTag
                | Tag::SwitchInt
                | Tag::SwitchChar
                | Tag::SwitchStr => {
                    // The schema visits every arm. Bound its record before
                    // visiting it, so a wide switch cannot multiply scans.
                    if self.record(self.data[i][1]).len() > BUDGET {
                        return None;
                    }
                    joined = true;
                    topo::for_blocks(self, i, |blk| {
                        if crate::own::owner_takes_tail(self, v, blk) {
                            todo.push(blk);
                        }
                    });
                }
                _ => return None,
            }
        }
        (joined && !reads.is_empty()).then_some(reads)
    }

    /// A value that carries only loans, including a conditional selection
    /// of whole parameter loans. Unlike `loan_read`, this need not have one
    /// source read.
    #[must_use]
    pub fn loan_value(&self, v: u32) -> Option<u32> {
        (self.is_borrow(v) || self.is_borrowed_coercion(v)).then_some(v)
    }

    /// The `borrow` read whose value `v` is, itself or through a
    /// [`Body::is_borrowed_coercion`]: what a binding local that a `Store`
    /// of `v` fills holds (`crate::own::borrow`); `None` for any other
    /// value, including a selection with several source reads.
    #[must_use]
    pub fn loan_read(&self, v: u32) -> Option<u32> {
        if self.is_borrow(v) {
            Some(v)
        } else {
            self.borrowed_alias(v)
        }
    }

    /// Whether value `v` is a `borrow` read's value.
    #[must_use]
    pub fn is_borrow(&self, v: u32) -> bool {
        Ref(v).as_inst().is_some()
            && (v as usize) < self.len()
            && self.tags[v as usize] == Tag::Read
            && self.data[v as usize][1] == ReadMode::Borrow as u32
    }

    /// The callee, its instruction and the call's anchor that an inlined
    /// copy's anchor `a` names ([`Body::inlined`]); `None` for any other.
    #[must_use]
    pub fn inlined_frame(&self, a: u32) -> Option<(DefId, u32, u32)> {
        if a == NONE || a & INLINED == 0 {
            return None;
        }
        let a = a & !INLINED;
        let k = self.inlined.partition_point(|e| e[0] <= a).checked_sub(1)?;
        let [first, _, callee, call] = self.inlined[k];
        let source = self
            .inlined_origins
            .binary_search_by_key(&a, |row| row[0])
            .map_or(a - first, |i| self.inlined_origins[i][1]);
        Some((DefId::from_raw(callee), source, call))
    }

    /// A record in `extra`: its words after the length word.
    #[must_use]
    pub fn record(&self, at: u32) -> &[u32] {
        let n = self.extra[at as usize] as usize;
        &self.extra[at as usize + 1..at as usize + 1 + n]
    }

    /// Whether wrapper `Scope` `i` runs a cleanup suite ([`Body::suites`]).
    #[must_use]
    pub fn is_suite(&self, i: u32) -> bool {
        self.suites.binary_search(&i).is_ok()
    }

    /// Suspension point `at`'s cancel block ([`Body::cancel`]): one probe.
    #[must_use]
    pub fn cancel_block(&self, at: u32) -> Option<u32> {
        let k = self.cancel.binary_search_by_key(&at, |r| r.0).ok()?;
        Some(self.cancel[k].1)
    }

    /// What cancelling at each point releases, as (point, value): the
    /// `Release` statements of each cancel block, in row and list order,
    /// and the value `Drop`s its elaborated guards hold, once per part
    /// (`own::elaborate`).
    pub fn cancel_releases(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.cancel.iter().flat_map(move |&(at, blk)| {
            let mut out = Vec::new();
            let mut stack = vec![(blk, true)];
            while let Some((blk, top)) = stack.pop() {
                for &s in self.record(self.data[blk as usize][0]) {
                    let ([a, w], tag) = (self.data[s as usize], self.tags[s as usize]);
                    if tag == Tag::Release && top {
                        out.push((at, a));
                    } else if tag == Tag::Drop && !top && w == crate::own::CANCELLED {
                        out.extend(Some(self.view(a).root).and_then(|r| match r {
                            Root::Value(v) => Some((at, v)),
                            _ => None,
                        }));
                    } else if tag == Tag::If {
                        stack.extend(
                            self.record(w)
                                .iter()
                                .filter(|&&x| x != NONE)
                                .map(|&x| (x, false)),
                        );
                    }
                }
            }
            out
        })
    }

    /// The parameter local whose ownership value `v` is (an
    /// `IntrinsicOp::ParamOwned`), if it is one.
    #[must_use]
    pub fn param_owned(&self, v: u32) -> Option<u32> {
        let i = Ref(v).as_inst()?.0 as usize;
        if self.tags.get(i) != Some(&Tag::Intrinsic)
            || IntrinsicOp::from_u32(self.data[i][0]) != Some(IntrinsicOp::ParamOwned)
        {
            return None;
        }
        let k = *self.record(self.data[i][1]).first()? & !Ref::CONST_BIT;
        u32::try_from(self.consts.get(k as usize)?.1).ok()
    }

    /// The parameter locals explicitly recorded for sub-body `sub`.
    /// Empty ranges do not dereference their offset, including a copied
    /// body's default root range. The range length is authoritative.
    #[must_use]
    pub fn sub_param_locals(&self, sub: usize) -> &[u32] {
        let r = self.sub_params[sub];
        if r.len == 0 {
            return &[];
        }
        let start = r.start as usize + 1;
        &self.extra[start..start + r.len as usize]
    }

    /// Whether sub-body `sub` is a generic local function's
    /// ([`sub_flags::GENERIC`]).
    #[must_use]
    pub fn sub_is_generic(&self, sub: usize) -> bool {
        self.sub_flags
            .get(sub)
            .is_some_and(|f| f & sub_flags::GENERIC != 0)
    }

    /// Whether `Closure` instruction `i` instantiates a generic local
    /// function: its record holds the instantiation
    /// ([`sub_flags::GENERIC`]).
    #[must_use]
    pub fn is_instantiation(&self, i: usize) -> bool {
        self.tags[i] == Tag::Closure && self.record(self.data[i][1]).len() == 4
    }

    /// Each instruction's sub-body ([`Topology::sub`]).
    #[must_use]
    pub fn inst_subs(&self) -> Vec<u32> {
        Topology::new(self).sub
    }

    /// The value operands of instruction `i` that compute one result from
    /// values and own no block, label or local, in the order emission
    /// reads them ([`slots`]); constants included. `None` for any other
    /// instruction. A call's providers are not listed, nor a place's local
    /// or global root.
    #[must_use]
    pub fn value_operands(&self, i: usize) -> Option<Vec<u32>> {
        let outlined = match self.tags[i] {
            Tag::Prim | Tag::NewStruct | Tag::NewVariant | Tag::NewMap | Tag::DefaultCall => true,
            Tag::Call | Tag::CallValue | Tag::Intrinsic | Tag::Is => true,
            Tag::Coerce | Tag::CopyStruct | Tag::NewTuple | Tag::ProviderGet | Tag::ItemRef => true,
            Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Store => true,
            Tag::NewList => self.data[i][0] == NONE,
            _ => false,
        };
        outlined.then(|| slot_words(self, i, |s| matches!(s, Word::Value(_))))
    }

    /// Decodes a place word ([`place`]); `None` when it is malformed: a
    /// record out of range, an unknown root kind, a projection with stray
    /// payload bits, or an element with no index word.
    #[must_use]
    pub fn decode_place(&self, word: u32) -> Option<Place> {
        use place::{
            BARE_LOCAL, PROJ_DEREF, PROJ_ELEM, PROJ_FIELD, PROJ_INDEX, PROJ_KIND, PROJ_VARIANT,
            ROOT_FRAME, ROOT_GLOBAL, ROOT_LOCAL, ROOT_PTR, ROOT_VALUE,
        };
        if word & BARE_LOCAL != 0 {
            return Some(Place::local(LocalId::from_raw(word & !BARE_LOCAL)));
        }
        let at = word as usize;
        let n = *self.extra.get(at)? as usize;
        let rec = self.extra.get(at + 1..at + 1 + n)?;
        let (root, mut rest) = match rec {
            [ROOT_LOCAL, l, rest @ ..] => (PlaceRoot::Local(LocalId::from_raw(*l)), rest),
            [ROOT_GLOBAL, d, t, rest @ ..] => {
                (PlaceRoot::Global(DefId::from_raw(*d), Ty(*t)), rest)
            }
            [ROOT_VALUE, v, rest @ ..] => (PlaceRoot::Value(Ref(*v)), rest),
            [ROOT_FRAME, s, rest @ ..] => (PlaceRoot::Frame(*s), rest),
            [ROOT_PTR, v, rest @ ..] => (PlaceRoot::Ptr(Ref(*v)), rest),
            _ => return None,
        };
        let mut proj = Vec::with_capacity(rest.len());
        while let [w, more @ ..] = rest {
            rest = more;
            proj.push(match (w & PROJ_KIND, w & PROJ_INDEX) {
                (PROJ_FIELD, k) => Proj::Field(k),
                (PROJ_DEREF, 0) => Proj::Deref,
                (PROJ_ELEM, _) => {
                    let [index, more @ ..] = rest else {
                        return None;
                    };
                    rest = more;
                    Proj::Elem(Ref(*index))
                }
                (PROJ_VARIANT, k) => Proj::Variant(k),
                _ => return None,
            });
        }
        Some(Place { root, proj })
    }

    /// The local `Store` `i` writes in place, and whether it replaces all
    /// of it: a place rooted at a local whose projections are all struct
    /// fields (`[$l]`, `[$l .x .y]`). `None` for any other instruction or
    /// place.
    #[must_use]
    pub fn stored_local(&self, i: usize) -> Option<(LocalId, bool)> {
        let v = (self.tags[i] == Tag::Store).then(|| self.view(self.data[i][0]))?;
        let l = v
            .local()
            .filter(|_| v.steps(self).all(|p| matches!(p, Proj::Field(_))))?;
        Some((LocalId::from_raw(l), v.whole()))
    }

    /// The local whose own storage the place at `word` lies in: a place
    /// rooted at a local whose steps are all fields and variants (`[$l]`,
    /// `[$l .x as .1 .0]`). A place through a dereference or an element
    /// lies in a referent or a list's buffer, which the local only names
    /// (`[*$l .x]`, `[$l [%i]]`): a `Store` there does not change the
    /// local, and a `Ref` there does not address it. `None` for any other
    /// place.
    #[must_use]
    pub fn local_in_place(&self, word: u32) -> Option<LocalId> {
        let v = self.view(word);
        v.local()
            .filter(|_| v.in_place(self))
            .map(LocalId::from_raw)
    }

    /// The local a plain local read `i` reads: a `Read` of a bare local,
    /// `[$n]`, in any mode. `None` for any other instruction.
    #[must_use]
    pub fn local_read(&self, i: usize) -> Option<LocalId> {
        let word = self.data[i][0];
        (self.tags[i] == Tag::Read && word & place::BARE_LOCAL != 0)
            .then(|| LocalId::from_raw(word & !place::BARE_LOCAL))
    }

    /// The unaddressed immutable binding of recursive closure `ci`.
    /// Its value is reconstructed from the code and current environment;
    /// the checker's shared initialization capture needs no mutable cell.
    #[must_use]
    pub fn closure_self(&self, ci: usize) -> Option<LocalId> {
        crate::own::RecursiveBindings::new(self).self_local(ci)
    }

    /// The value projection read `i` projects from and its one step: a
    /// `Read` of a value root followed by one field, a dereference and one
    /// field, or a variant and one of its fields. These are the reads of a
    /// struct field or tuple element (`[%v .k]`), of a referent's field
    /// (`[*%v .k]`), and of a payload (`[%v as .V .k]`).
    /// It decodes the record in place. `None` for any other instruction or
    /// place.
    #[must_use]
    pub fn read_step(&self, i: usize) -> Option<(u32, Step)> {
        let (v, mut s) = self.value_steps(i, Tag::Read)?;
        let step = match (s.next()?, s.next(), s.next()) {
            (Proj::Field(k), None, _) => Step::Field(k),
            (Proj::Deref, Some(Proj::Field(k)), None) => Step::DerefField(k),
            (Proj::Variant(x), Some(Proj::Field(k)), None) => Step::Variant(x, k),
            _ => return None,
        };
        Some((v, step))
    }

    /// The root value of instruction `i`'s place (a constant included)
    /// and its steps, when `i` is a `tag` with a place rooted at a value.
    fn value_steps(&self, i: usize, tag: Tag) -> Option<(u32, Steps<'_>)> {
        let p = (self.tags[i] == tag).then(|| self.view(self.data[i][0]))?;
        match p.root {
            Root::Value(v) | Root::Const(v) => Some((v, p.steps(self))),
            _ => None,
        }
    }

    /// The list value and the index value element read `i` reads: a
    /// `Read` of a value root with one element projection, `[%l [%i]]`,
    /// a slot of the buffer of the list header `%l`. It decodes the record
    /// in place. `None` for any other instruction or place.
    #[must_use]
    pub fn element_read(&self, i: usize) -> Option<(u32, u32)> {
        let (list, mut s) = self.value_steps(i, Tag::Read)?;
        match (s.next()?, s.next()) {
            (Proj::Elem(index), None) => Some((list, index.0)),
            _ => None,
        }
    }

    /// The value a copy of a whole value reads: a `Read` of a value root
    /// with no projection, `[%v]`, which the ownership passes write for a
    /// consumer of a value used more than once. `None` for any other
    /// instruction or place.
    #[must_use]
    pub fn value_read(&self, i: usize) -> Option<u32> {
        let (v, mut s) = self.value_steps(i, Tag::Read)?;
        s.next().is_none().then_some(v)
    }

    /// The pointer value whose target `Read` or `Store` `i` reads or
    /// writes, `[ptr %p]`. `None` for any other instruction or place.
    #[must_use]
    pub fn ptr_target(&self, i: usize) -> Option<u32> {
        let p =
            matches!(self.tags[i], Tag::Read | Tag::Store).then(|| self.view(self.data[i][0]))?;
        match p.root {
            Root::Ptr(x) if p.whole() => Some(x),
            _ => None,
        }
    }

    /// The top-level binding a plain global read `i` reads: a `Read` of
    /// a global root with no projection, `[@g]`. `None` for any other
    /// instruction.
    #[must_use]
    pub fn global_read(&self, i: usize) -> Option<DefId> {
        let p = (self.tags[i] == Tag::Read).then(|| self.view(self.data[i][0]))?;
        match p.root {
            Root::Global(d, _) if p.whole() => Some(d),
            _ => None,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tags.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

/// A builder checkpoint (§3.9.5), for speculation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TirCheckpoint {
    pub insts: u32,
    pub extra: u32,
    pub scratch: u32,
    pub locals: u32,
    pub captures: u32,
    pub subs: u32,
    pub labels: u32,
    pub susp: u32,
    /// The constant, string and closed-capture columns: a trial's
    /// literals and closures leave no row behind.
    pub consts: u32,
    pub strings: u32,
    pub cap_local: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BlockMark(u32);
#[derive(Clone, Copy, Debug)]
pub struct ScopeMark(u32);
#[derive(Clone, Copy, Debug)]
pub struct LoopMark(pub LabelId);
#[derive(Clone, Copy, Debug)]
pub struct Slot(u32);

impl Slot {
    /// The reserved instruction, which the value it is filled with takes.
    #[must_use]
    pub fn as_ref(self) -> Ref {
        Ref(self.0)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct SubMark(SubId, u32);

impl SubMark {
    /// The sub-body this mark opened.
    #[must_use]
    pub fn sub(self) -> SubId {
        self.0
    }
}

/// What the checker calls (§4.13.11 "The Builder API"). `TirBuilder`
/// implements it; a discarding sink implements it for no-emit mode.
pub trait TirSink {
    fn konst(&mut self, index: u32) -> Ref;
    fn local(&mut self, ty: Ty, name: Symbol, flags: u16, syn: NodeIdx) -> LocalId;
    fn get(&mut self, l: LocalId, ty: Ty, syn: NodeIdx) -> Ref;
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx);
    fn prim(&mut self, op: u32, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref;
    fn call(&mut self, callee: &Callee, args: &[Ref], prov: Providers, ty: Ty, syn: NodeIdx)
    -> Ref;
    fn coerce(&mut self, kind: Coercion, evidence: u32, v: Ref, to: Ty, syn: NodeIdx) -> Ref;
    fn emit(&mut self, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_block(&mut self) -> BlockMark;
    /// Removes an open block mark without grouping the instructions emitted
    /// since it. Used when a checker discovers that speculative grouping was
    /// unnecessary.
    fn cancel_block(&mut self, m: BlockMark);
    fn close_block(&mut self, m: BlockMark, tail: Option<Ref>, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_scope(&mut self) -> ScopeMark;
    fn defer(&mut self, suite: Ref, syn: NodeIdx);
    fn close_scope(&mut self, m: ScopeMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_loop(&mut self) -> LoopMark;
    fn close_loop(&mut self, m: LoopMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref;
    fn reserve(&mut self) -> Slot;
    fn fill(&mut self, slot: Slot, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_sub(&mut self, params: &[LocalId]) -> SubMark;
    fn capture(&mut self, sub: SubMark, outer: LocalId) -> CaptureId;
    fn close_sub(&mut self, m: SubMark, root: Ref, fn_ty: Ty, syn: NodeIdx) -> Ref;
    fn checkpoint(&self) -> TirCheckpoint;
    fn rollback(&mut self, c: TirCheckpoint);
}

/// The builder: the only writer of TIR. Instructions are appended to the
/// columns; the open block's list lives on a scratch stack until it closes.
pub struct TirBuilder {
    body: Body,
    verify: bool,
    scratch: Vec<u32>,
    open: Vec<u32>,
    open_caps: Vec<(SubId, LocalId)>,
}

impl TirBuilder {
    #[must_use]
    pub fn new(item: DefId, kind: BodyKind) -> Self {
        let mut body = Body::new(item, kind);
        body.sub_root.push(NONE);
        body.sub_params.push(Range32::default());
        body.sub_parent.push(SubId::NONE);
        body.sub_flags.push(0);
        Self {
            body,
            verify: true,
            scratch: vec![],
            open: vec![],
            open_caps: vec![],
        }
    }

    /// Select checked-body verification for this compiler run. Direct builder
    /// clients verify by default; debug compilers always verify, even when
    /// their caller disables the optional release-compiler check.
    #[must_use]
    pub fn with_verification(mut self, enabled: bool) -> Self {
        self.verify = cfg!(debug_assertions) || enabled;
        self
    }

    fn record(&mut self, words: &[u32]) -> u32 {
        let at = u32::try_from(self.body.extra.len()).expect("extra");
        self.body
            .extra
            .push(u32::try_from(words.len()).expect("record"));
        self.body.extra.extend_from_slice(words);
        at
    }

    fn push(&mut self, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let i = u32::try_from(self.body.tags.len()).expect("insts");
        self.body.tags.push(tag);
        self.body.data.push([a, b]);
        self.body.ty.push(ty);
        self.body.syn.push(syn);
        if !self.open.is_empty() {
            self.scratch.push(i);
        }
        Ref(i)
    }

    /// A record of plain numbers in `extra`, such as the spread positions
    /// of a `NewList`.
    pub fn words_record(&mut self, words: &[u32]) -> u32 {
        self.record(words)
    }

    /// The common argument/footer record of a call. A dynamic call embeds
    /// its contents after its method, type arguments and evidence header.
    pub fn call_args_record(&mut self, args: &[Ref], prov: Providers) -> u32 {
        let mut words = Self::refs(args);
        words.extend(match prov {
            Providers::None => [0, 0, 0],
            Providers::Pairs(r) => [1, r.start, r.len],
            Providers::Context(c) => [2, c.0, 0],
        });
        self.record(&words)
    }

    /// A record of values or blocks in `extra` (the `Values`/`Blocks`
    /// operand of `If`, `CallHost` and others).
    pub fn refs_record(&mut self, refs: &[Ref]) -> u32 {
        let words: Vec<u32> = refs.iter().map(|r| r.0).collect();
        self.record(&words)
    }

    /// The place word of `p` ([`place`]): a bare local as itself, any
    /// other place as a new record. Instructions over one place may share
    /// the word.
    pub fn place(&mut self, p: &Place) -> u32 {
        if let (PlaceRoot::Local(l), []) = (p.root, p.proj.as_slice()) {
            assert!(l.raw() & place::BARE_LOCAL == 0, "local out of range");
            return l.raw() | place::BARE_LOCAL;
        }
        let words = p.words().unwrap_or_default();
        self.record(&words)
    }

    /// `Read [@def]`: top-level binding `def`, of type `ty`, read whole
    /// with mode `use`.
    pub fn read_global(&mut self, def: DefId, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.place(&Place {
            root: PlaceRoot::Global(def, ty),
            proj: vec![],
        });
        self.push(Tag::Read, word, ReadMode::Use as u32, ty, syn)
    }

    /// `Read [%base .k]`: field `k` of a struct, newtype or tuple value,
    /// mode `use`.
    pub fn field(&mut self, base: Ref, k: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.record(&[place::ROOT_VALUE, base.0, place::PROJ_FIELD | k]);
        self.push(Tag::Read, word, ReadMode::Use as u32, ty, syn)
    }

    /// `Read [%base as .variant .k]`: field `k` of the payload of enum
    /// value `base`, known to hold `variant`, mode `use`.
    pub fn payload(&mut self, base: Ref, variant: u32, k: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.record(&[
            place::ROOT_VALUE,
            base.0,
            place::PROJ_VARIANT | variant,
            place::PROJ_FIELD | k,
        ]);
        self.push(Tag::Read, word, ReadMode::Use as u32, ty, syn)
    }

    /// `Read [%list [%index]]`: the element at `index` of the list header
    /// `list`, bounds-checked, mode `use`.
    pub fn element(&mut self, list: Ref, index: Ref, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.record(&[place::ROOT_VALUE, list.0, place::PROJ_ELEM, index.0]);
        self.push(Tag::Read, word, ReadMode::Use as u32, ty, syn)
    }

    /// The body so far (for the checker's final type pass).
    #[must_use]
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// The body so far, to change in place.
    pub fn body_mut(&mut self) -> &mut Body {
        &mut self.body
    }

    /// The locals the open sub-body `m` captures so far.
    #[must_use]
    pub fn open_captures(&self, m: SubMark) -> Vec<LocalId> {
        self.open_caps
            .get(m.1 as usize..)
            .unwrap_or(&[])
            .iter()
            .filter(|c| c.0 == m.0)
            .map(|c| c.1)
            .collect()
    }

    /// Lifts the open sub-body `m`'s captures `caps` to the fields of one
    /// leading tuple parameter `p`, as Swift's closure lifting makes
    /// captures parameters: each place emitted since instruction `from`,
    /// all `m`'s, rooted at `caps[i]` is rooted at `p .i`, a nested closure
    /// captures `p` in their place, and `m` captures nothing. Cost: the
    /// instructions since `from`, once.
    pub fn lift_captures(&mut self, m: SubMark, from: usize, caps: &[LocalId], p: LocalId) {
        let field = |l: u32| caps.iter().position(|c| c.raw() == l).map(u32_of);
        for i in from..self.body.tags.len() {
            let (oa, ob) = self.body.tags[i].operands();
            for (k, op) in [(0, oa), (1, ob)] {
                let w = self.body.data[i][k];
                if op != Op::Place || w == NONE {
                    continue;
                }
                let (l, rest) = if w & place::BARE_LOCAL != 0 {
                    (w & !place::BARE_LOCAL, &[][..])
                } else if self.body.extra.get(w as usize + 1) == Some(&place::ROOT_LOCAL) {
                    let n = self.body.extra[w as usize] as usize;
                    (
                        self.body.extra[w as usize + 2],
                        &self.body.extra[w as usize + 3..=w as usize + n],
                    )
                } else {
                    continue;
                };
                let Some(f) = field(l) else { continue };
                let words: Vec<u32> = [place::ROOT_LOCAL, p.raw(), place::PROJ_FIELD | f]
                    .into_iter()
                    .chain(rest.iter().copied())
                    .collect();
                self.body.data[i][k] = self.record(&words);
            }
            if self.body.tags[i] == Tag::Closure {
                let r = self.body.data[i][1] as usize;
                let (start, n) = (
                    self.body.extra[r + 1] as usize,
                    self.body.extra[r + 2] as usize,
                );
                let old = &self.body.cap_local[start..start + n];
                if old.iter().any(|c| field(c.raw()).is_some()) {
                    let mut kept: Vec<LocalId> = old
                        .iter()
                        .copied()
                        .filter(|c| field(c.raw()).is_none())
                        .collect();
                    kept.push(p);
                    self.body.extra[r + 1] = u32_of(self.body.cap_local.len());
                    self.body.extra[r + 2] = u32_of(kept.len());
                    self.body.cap_local.extend(kept);
                }
            }
        }
        let words: Vec<u32> = std::iter::once(p.raw())
            .chain(self.body.sub_param_locals(m.0.idx()).iter().copied())
            .collect();
        let at = self.record(&words);
        self.body.sub_params[m.0.idx()] = Range32::new(at, u32_of(words.len()));
        let mut at = 0;
        self.open_caps.retain(|c| {
            at += 1;
            at <= m.1 as usize || c.0 != m.0
        });
    }

    /// A constant of the body's own column.
    pub fn const_value(&mut self, ty: Ty, bits: u64) -> Ref {
        let i = u32::try_from(self.body.consts.len()).expect("consts");
        self.body.consts.push((ty, bits));
        Ref::konst(i)
    }

    /// A `string` constant: its text joins the body's string table.
    pub fn const_str(&mut self, text: &str) -> Ref {
        let s = self.body.strings.len() as u64;
        self.body.strings.push(text.into());
        self.const_value(Ty::STRING, s)
    }

    /// The type of a value: an instruction's or a constant's.
    #[must_use]
    pub fn ty_of(&self, r: Ref) -> Ty {
        match r.as_inst() {
            Some(i) => self
                .body
                .ty
                .get(i.0 as usize)
                .copied()
                .unwrap_or(Ty::POISON),
            None => self
                .body
                .consts
                .get((r.0 & !Ref::CONST_BIT) as usize)
                .map_or(Ty::POISON, |c| c.0),
        }
    }

    /// The constant behind a value, if it is one.
    #[must_use]
    pub fn const_of(&self, r: Ref) -> Option<(Ty, u64)> {
        if r.as_inst().is_some() {
            return None;
        }
        self.body
            .consts
            .get((r.0 & !Ref::CONST_BIT) as usize)
            .copied()
    }

    /// The local's declared type.
    #[must_use]
    pub fn local_ty(&self, l: LocalId) -> Ty {
        self.body.local_ty[l.idx()]
    }

    fn refs(rs: &[Ref]) -> Vec<u32> {
        rs.iter().map(|r| r.0).collect()
    }

    /// End of the body: the root block's instruction, capture modes, then
    /// the verifier. Unfilled slots are rejected by the verifier.
    pub fn finish(
        self,
        root: Ref,
        capture_modes: &[CaptureMode],
    ) -> Result<Body, Vec<VerifyError>> {
        self.finish_tolerating(root, capture_modes, &[])
    }

    /// `finish`, ignoring findings of the `tolerated` invariants: a body
    /// in a module with errors may break invariant 4 after a reported
    /// mismatch, and D2 does not lower it.
    pub fn finish_tolerating(
        mut self,
        root: Ref,
        capture_modes: &[CaptureMode],
        tolerated: &[u8],
    ) -> Result<Body, Vec<VerifyError>> {
        self.body.sub_root[0] = root.0;
        self.body.cap_mode = capture_modes.to_vec();
        self.body
            .cap_mode
            .resize(self.body.cap_local.len(), CaptureMode::Shared);
        if !self.verify {
            return Ok(self.body);
        }
        let mut errs = verify_checked(&self.body);
        errs.retain(|e| !tolerated.contains(&e.invariant));
        if errs.is_empty() {
            Ok(self.body)
        } else {
            Err(errs)
        }
    }
}

impl TirSink for TirBuilder {
    fn konst(&mut self, index: u32) -> Ref {
        Ref::konst(index)
    }
    fn local(&mut self, ty: Ty, name: Symbol, flags: u16, syn: NodeIdx) -> LocalId {
        let l = LocalId::from_raw(u32::try_from(self.body.local_ty.len()).expect("locals"));
        self.body.local_ty.push(ty);
        self.body.local_name.push(name);
        self.body.local_syn.push(syn);
        self.body.local_flags.push(flags);
        l
    }
    fn get(&mut self, l: LocalId, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.place(&Place::local(l));
        self.push(Tag::Read, word, ReadMode::Use as u32, ty, syn)
    }
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx) {
        let word = self.place(&Place::local(l));
        self.push(Tag::Store, word, v.0, Ty::VOID, syn);
    }
    fn prim(&mut self, op: u32, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref {
        let r = self.record(&Self::refs(args));
        self.push(Tag::Prim, op, r, ty, syn)
    }
    fn call(
        &mut self,
        callee: &Callee,
        args: &[Ref],
        prov: Providers,
        ty: Ty,
        syn: NodeIdx,
    ) -> Ref {
        let c = self.record(&callee.words());
        let a = self.call_args_record(args, prov);
        self.push(Tag::Call, c, a, ty, syn)
    }
    fn coerce(&mut self, kind: Coercion, evidence: u32, v: Ref, to: Ty, syn: NodeIdx) -> Ref {
        let r = self.record(&[kind as u32, evidence]);
        self.push(Tag::Coerce, v.0, r, to, syn)
    }
    fn emit(&mut self, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref {
        self.push(tag, a, b, ty, syn)
    }
    fn open_block(&mut self) -> BlockMark {
        self.open
            .push(u32::try_from(self.scratch.len()).expect("scratch"));
        BlockMark(u32::try_from(self.open.len()).expect("open"))
    }
    fn cancel_block(&mut self, m: BlockMark) {
        assert_eq!(m.0 as usize, self.open.len(), "blocks cancel in order");
        self.open.pop().expect("open block");
    }
    fn close_block(&mut self, m: BlockMark, tail: Option<Ref>, ty: Ty, syn: NodeIdx) -> Ref {
        assert_eq!(m.0 as usize, self.open.len(), "blocks close in order");
        let start = self.open.pop().expect("open block") as usize;
        let list: Vec<u32> = self.scratch.drain(start..).collect();
        let r = self.record(&list);
        self.push(Tag::Block, r, tail.map_or(NONE, |t| t.0), ty, syn)
    }
    fn open_scope(&mut self) -> ScopeMark {
        ScopeMark(u32::try_from(self.body.tags.len()).expect("insts"))
    }
    fn defer(&mut self, suite: Ref, syn: NodeIdx) {
        self.push(Tag::Defer, suite.0, NONE, Ty::VOID, syn);
    }
    /// The scope's suites are the `Defer`s emitted since `open_scope`,
    /// in instruction order.
    fn close_scope(&mut self, m: ScopeMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref {
        let suites: Vec<u32> = (m.0 as usize..self.body.tags.len())
            .filter(|&i| self.body.tags[i] == Tag::Defer)
            .map(|i| self.body.data[i][0])
            .collect();
        let r = self.record(&suites);
        self.push(Tag::Scope, body.0, r, ty, syn)
    }
    fn open_loop(&mut self) -> LoopMark {
        let l = LabelId::from_raw(u32::try_from(self.body.label_inst.len()).expect("labels"));
        self.body.label_inst.push(NONE);
        LoopMark(l)
    }
    fn close_loop(&mut self, m: LoopMark, body: Ref, ty: Ty, syn: NodeIdx) -> Ref {
        let r = self.push(Tag::Loop, body.0, NONE, ty, syn);
        self.body.label_inst[m.0.idx()] = r.0;
        r
    }
    fn reserve(&mut self) -> Slot {
        let r = self.push(Tag::Unreachable, NONE, NONE, Ty::NEVER, NodeIdx::NONE);
        self.body.tags[r.0 as usize] = Tag::Hole;
        self.body.data[r.0 as usize] = [NONE, NONE];
        Slot(r.0)
    }
    fn fill(&mut self, slot: Slot, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let i = slot.0 as usize;
        assert!(
            self.body.tags[i] == Tag::Hole && self.body.data[i] == [NONE, NONE],
            "slot filled once"
        );
        self.body.tags[i] = tag;
        self.body.data[i] = [a, b];
        self.body.ty[i] = ty;
        self.body.syn[i] = syn;
        Ref(slot.0)
    }
    fn open_sub(&mut self, params: &[LocalId]) -> SubMark {
        let s = SubId::from_raw(u32::try_from(self.body.sub_root.len()).expect("subs"));
        let words: Vec<u32> = params.iter().map(|l| l.raw()).collect();
        let at = self.record(&words);
        self.body.sub_root.push(NONE);
        self.body.sub_params.push(Range32::new(
            at,
            u32::try_from(params.len()).expect("params"),
        ));
        self.body.sub_parent.push(SubId::from_raw(0));
        self.body.sub_flags.push(0);
        SubMark(s, u32::try_from(self.open_caps.len()).expect("caps"))
    }
    fn capture(&mut self, sub: SubMark, outer: LocalId) -> CaptureId {
        if let Some(i) = self.open_caps[sub.1 as usize..]
            .iter()
            .position(|c| *c == (sub.0, outer))
        {
            return CaptureId::from_raw(sub.1 + u32::try_from(i).expect("cap"));
        }
        self.open_caps.push((sub.0, outer));
        CaptureId::from_raw(u32::try_from(self.open_caps.len() - 1).expect("caps"))
    }
    fn close_sub(&mut self, m: SubMark, root: Ref, fn_ty: Ty, syn: NodeIdx) -> Ref {
        self.body.sub_root[m.0.idx()] = root.0;
        // A read inside this sub also captured the local into each
        // enclosing open sub; those captures stay open.
        let (mine, outer): (Vec<_>, Vec<_>) = self
            .open_caps
            .drain(m.1 as usize..)
            .partition(|c| c.0 == m.0);
        self.open_caps.extend(outer);
        let mine: Vec<LocalId> = mine.into_iter().map(|c| c.1).collect();
        let start = u32::try_from(self.body.cap_local.len()).expect("caps");
        self.body.cap_local.extend(&mine);
        let r = self.record(&[start, u32::try_from(mine.len()).expect("caps")]);
        self.push(Tag::Closure, m.0.raw(), r, fn_ty, syn)
    }
    fn checkpoint(&self) -> TirCheckpoint {
        let n = |v: usize| u32::try_from(v).expect("checkpoint");
        TirCheckpoint {
            insts: n(self.body.tags.len()),
            extra: n(self.body.extra.len()),
            scratch: n(self.scratch.len()),
            locals: n(self.body.local_ty.len()),
            captures: n(self.open_caps.len()),
            subs: n(self.body.sub_root.len()),
            labels: n(self.body.label_inst.len()),
            susp: n(self.body.susp.len()),
            consts: n(self.body.consts.len()),
            strings: n(self.body.strings.len()),
            cap_local: n(self.body.cap_local.len()),
        }
    }
    fn rollback(&mut self, c: TirCheckpoint) {
        let b = &mut self.body;
        b.tags.truncate(c.insts as usize);
        b.data.truncate(c.insts as usize);
        b.ty.truncate(c.insts as usize);
        b.syn.truncate(c.insts as usize);
        b.extra.truncate(c.extra as usize);
        b.local_ty.truncate(c.locals as usize);
        b.local_name.truncate(c.locals as usize);
        b.local_syn.truncate(c.locals as usize);
        b.local_flags.truncate(c.locals as usize);
        b.sub_root.truncate(c.subs as usize);
        b.sub_params.truncate(c.subs as usize);
        b.sub_parent.truncate(c.subs as usize);
        b.sub_flags.truncate(c.subs as usize);
        b.label_inst.truncate(c.labels as usize);
        b.susp.truncate(c.susp as usize);
        b.consts.truncate(c.consts as usize);
        b.strings.truncate(c.strings as usize);
        b.cap_local.truncate(c.cap_local as usize);
        self.scratch.truncate(c.scratch as usize);
        self.open_caps.truncate(c.captures as usize);
    }
}

/// A verifier finding: the invariant number of §4.13.11 and the instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyError {
    pub invariant: u8,
    pub inst: u32,
    pub what: String,
}

impl VerifyError {
    /// The stop of a body that failed the verifier: a compiler bug, since
    /// the checker builds only bodies that hold every invariant.
    /// `check` names the verifier's caller, such as `TIR verifier`.
    #[must_use]
    pub fn bug(stage: Stage, check: &str, errs: &[VerifyError]) -> NotImplemented {
        let mut what = format!("{check}:");
        for (n, e) in errs.iter().enumerate() {
            let sep = if n == 0 { " " } else { "; " };
            let _ = write!(
                what,
                "{sep}invariant {} at %{}: {}",
                e.invariant, e.inst, e.what
            );
        }
        NotImplemented::bug(stage, what)
    }
}

/// Invariants this verifier checks, some in part (`crates/hd_tir/DESIGN.md`
/// §4.13.11, "Implementation differs"). The raw corpus supplies independent
/// facts for 8, 11, 13 and 17; the checker constructs those relationships.
/// 3's `Hole` and `Poison` half needs the module's errors ([`poison_sites`]),
/// and 23 holds after ownership ([`verify_owned`]) where 22 holds before;
/// 24 holds on an instance body, which [`verify_owned`] checks too.
pub const CHECKED_INVARIANTS: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 20, 21, 22, 23, 24,
];

/// Verifies a checked body, including record rules that need no external facts.
/// The checker constructs switch defaults, provider contexts and selected trait
/// evidence; raw contextual corruption checks live with the test corpus.
#[must_use]
pub fn verify_checked(b: &Body) -> Vec<VerifyError> {
    let mut errs = verify(b);
    for (i, (&tag, &[a, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let inst = u32::try_from(i).expect("i");
        let mut err = |invariant: u8, what: String| {
            errs.push(VerifyError {
                invariant,
                inst,
                what,
            });
        };
        match tag {
            Tag::CallHost => err(9, "a host call in a TIR body".into()),
            Tag::Intrinsic if IntrinsicOp::from_u32(a).is_none() => {
                err(9, format!("unknown intrinsic {a}"));
            }
            Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
                if b.record(w).last().is_none_or(|d| *d == NONE) =>
            {
                err(11, format!("{} without a default", tag.name()));
            }
            _ => {}
        }
    }
    errs
}

#[cfg(test)]
mod context {
    use crate::ir;
    include!("../tests/support/context.rs");
}
#[cfg(test)]
pub use context::{VerifyCx, verify_in};

/// Maps every type a body records through `ty`, and every type list
/// through `list`: the instruction and local columns, the constants, and
/// the types and type lists kept in records (callees, `ItemRef`,
/// `DefaultCall`, instantiated closures, `ProviderGet`, `With` and
/// `ContextNew`, and `RowAdapt` evidence). The checker lowers reference types this way
/// (`hd_check/src/refs.rs`).
pub fn map_body_types(b: &mut Body, ty: &dyn Fn(Ty) -> Ty, list: &dyn Fn(TyList) -> TyList) {
    for t in &mut b.ty {
        *t = ty(*t);
    }
    for t in &mut b.local_ty {
        *t = ty(*t);
    }
    for c in &mut b.consts {
        c.0 = ty(c.0);
    }
    let mut words = Vec::new();
    record_type_words(b, &mut |at, word| words.push((at, word)));
    for (at, word) in words {
        b.extra[at] = match word {
            TypeWord::Ty => ty(Ty(b.extra[at])).0,
            TypeWord::List => list(TyList(b.extra[at])).0,
        };
    }
}

/// What a type word in a record holds ([`record_type_words`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeWord {
    Ty,
    List,
}

/// Calls `f` with each `extra` position that holds a type or a type list,
/// each once: the records [`map_body_types`] maps (callees, `ItemRef`,
/// `DefaultCall`, instantiated closures, `ProviderGet`, `With` and
/// `ContextNew`, `RowAdapt` evidence, provider keys and global place
/// roots). One pass over the instructions, plus a sort of the provider
/// keys and place roots that instructions may share.
pub fn record_type_words(b: &Body, f: &mut dyn FnMut(usize, TypeWord)) {
    for i in 0..b.tags.len() {
        inst_record_type_words(b, i, f);
    }
    let mut keys = Vec::new();
    for i in 0..b.len() {
        if let Providers::Pairs(r) = b.call_providers(i) {
            keys.extend((0..r.len).step_by(2).map(|k| (r.start + k) as usize));
        }
    }
    keys.sort_stable();
    keys.dedup();
    for at in keys {
        f(at, TypeWord::Ty);
    }
    // A global root's type; instructions may share a place record, which
    // is visited once.
    for at in global_place_types(b) {
        f(at, TypeWord::Ty);
    }
}

/// The type words of instruction `i`'s own records ([`record_type_words`]),
/// without its call's provider keys or a global place root's type.
pub fn inst_record_type_words(b: &Body, i: usize, f: &mut dyn FnMut(usize, TypeWord)) {
    let [a, w] = b.data[i];
    match b.tags[i] {
        Tag::Call | Tag::Await => {
            // `[0, def, targs]` or `[1, trait, method, self_ty, targs,
            // kind, value]` (`Callee::words`).
            let at = a as usize + 1;
            match b.extra.get(at).copied() {
                Some(0) => f(at + 2, TypeWord::List),
                Some(1) => {
                    f(at + 3, TypeWord::Ty);
                    f(at + 4, TypeWord::List);
                }
                _ => {}
            }
        }
        Tag::Coerce if b.row_adaptation(i).is_some() => f(w as usize + 2, TypeWord::List),
        Tag::ProviderGet => f(a as usize + 1, TypeWord::Ty),
        Tag::With | Tag::ContextNew => {
            let len = b.extra[a as usize] as usize;
            for k in 0..len / 2 {
                f(a as usize + 1 + 2 * k, TypeWord::Ty);
            }
        }
        Tag::ItemRef => f(w as usize + 1, TypeWord::List),
        Tag::CallDyn => f(w as usize + 2, TypeWord::List),
        Tag::DefaultCall => f(a as usize + 2, TypeWord::List),
        Tag::Closure if b.is_instantiation(i) => f(w as usize + 4, TypeWord::List),
        _ => {}
    }
}

/// The `extra` positions of the type words of global place roots, each
/// once, though instructions share a record ([`place`]).
#[must_use]
pub fn global_place_types(b: &Body) -> Vec<usize> {
    let mut out: Vec<usize> = (0..b.len())
        .flat_map(|i| {
            let (oa, ob) = b.tags[i].operands();
            [(b.data[i][0], oa), (b.data[i][1], ob)]
        })
        .filter(|&(word, op)| {
            op == Op::Place
                && word & place::BARE_LOCAL == 0
                && b.extra.get(word as usize + 1) == Some(&place::ROOT_GLOBAL)
        })
        .map(|(word, _)| word as usize + 3)
        .collect();
    out.sort_stable();
    out.dedup();
    out
}

/// The instructions of a cleanup suite that invariant 6 forbids: a
/// suspension point, a `Return`, or a `Break` or `Continue` whose loop
/// is outside the suite. A closure inside the suite is its own body and
/// is not searched: its root block, though listed in the suite, is a
/// sub-body root.
#[must_use]
pub fn cleanup_suite_violations(b: &Body, suite: u32) -> Vec<u32> {
    let mut seen = vec![false; b.len()];
    for &r in &b.sub_root {
        if let Some(s) = seen.get_mut(r as usize) {
            *s = true;
        }
    }
    let (mut found, mut in_suite) = (Vec::new(), vec![false; b.len()]);
    inside(b, &[suite], &mut Vec::new(), |i| {
        let fresh = !std::mem::replace(&mut seen[i], true);
        if fresh {
            found.push(u32_of(i));
            in_suite[i] = true;
        }
        fresh
    });
    let within = |l: u32| {
        b.label_inst
            .get(l as usize)
            .is_some_and(|&t| in_suite.get(t as usize) == Some(&true))
    };
    let mut out: Vec<u32> = found
        .into_iter()
        .filter(|&i| match b.tags[i as usize] {
            Tag::Return => true,
            Tag::Break | Tag::Continue => !within(b.data[i as usize][0]),
            tag => is_suspension(tag),
        })
        .collect();
    out.sort_stable();
    out
}

/// Verifies the checker's TIR of a body (§4.13.11 "Invariants"): the
/// structural invariants, and 22, that it has no ownership yet. A body
/// after the ownership passes is verified with [`verify_owned`]. A
/// [`Phase::Lowered`] body is built with its counts stated, so 22 is not
/// its invariant.
#[must_use]
pub fn verify(b: &Body) -> Vec<VerifyError> {
    let mut errs = verify_structure(b, &Topology::new(b));
    if b.phase != Phase::Lowered {
        no_ownership(b, &mut errs);
    }
    errs
}

/// Invariant 22: the checker's TIR has no ownership
/// (`docs/compiler/tir-v2.md`, "Invariants" 3): every `Read` has mode
/// `use`, no `Ref` takes a count, and there is no ownership statement,
/// exit set row or lifetime region row.
fn no_ownership(b: &Body, errs: &mut Vec<VerifyError>) {
    for (i, (&tag, &[_, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let what = match tag {
            Tag::Read if w != ReadMode::Use as u32 => match ReadMode::from_u32(w) {
                Some(m) => format!("a `Read` of mode {m:?} before the ownership passes"),
                None => format!("a `Read` of unknown mode {w}"),
            },
            Tag::Drop
            | Tag::LoanEnd
            | Tag::Retain
            | Tag::Release
            | Tag::MakeUnique
            | Tag::IsUnique
            | Tag::FlagSet
            | Tag::FlagClear
            | Tag::Box => {
                format!("an ownership statement `{}` before the passes", tag.name())
            }
            Tag::Ref if w & ref_word::TAKES != 0 => {
                "a `Ref` that takes its local's count before the passes".to_owned()
            }
            Tag::Ref if w & ref_word::FRAME != 0 => "a frame `Ref` before the passes".to_owned(),
            Tag::Ref if w & ref_word::PINS != 0 => "a pinning `Ref` before the passes".to_owned(),
            Tag::Ref if w & ref_word::LENT != 0 => "a lent `Ref` before the passes".to_owned(),
            _ => continue,
        };
        errs.push(VerifyError {
            invariant: 22,
            inst: u32::try_from(i).expect("i"),
            what,
        });
    }
    if !b.forwarded_reads.is_empty() {
        errs.push(VerifyError {
            invariant: 22,
            inst: NONE,
            what: "forwarded reads before the ownership passes".into(),
        });
    }
    if !b.exit_drops.is_empty()
        || !b.storage.is_empty()
        || !b.cancel.is_empty()
        || !b.suites.is_empty()
        || !b.stable_lists.is_empty()
        || !b.anchors.is_empty()
    {
        errs.push(VerifyError {
            invariant: 22,
            inst: NONE,
            what: format!(
                "{} exit drop, {} storage, {} cancel block, {} stable list and {} anchor rows before the ownership passes",
                b.exit_drops.len(),
                b.storage.len(),
                b.cancel.len(),
                b.stable_lists.len(),
                b.anchors.len()
            ),
        });
    }
}

/// The invariants of every body, before or after the ownership passes:
/// all of [`CHECKED_INVARIANTS`] but 22 and 23.
pub(crate) fn verify_structure(b: &Body, topo: &Topology) -> Vec<VerifyError> {
    let mut errs = Vec::new();
    let n = u32::try_from(b.len()).expect("insts");
    let mut owner = vec![NONE; b.len()];
    for (i, (&tag, &[a, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let i = u32::try_from(i).expect("i");
        let err = |inv: u8, what: String| VerifyError {
            invariant: inv,
            inst: i,
            what,
        };
        if tag == Tag::Hole && a == NONE && w == NONE {
            errs.push(err(14, "reserved slot never filled".into()));
            continue;
        }
        let call = matches!(
            tag,
            Tag::Call
                | Tag::Await
                | Tag::CallValue
                | Tag::AwaitValue
                | Tag::DefaultCall
                | Tag::CallDyn
        );
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Inst if word >= n => {
                    errs.push(err(1, "count target instruction out of range".into()));
                }
                Op::Inst if tag == Tag::LoanClose && b.tags[word as usize] != Tag::LoanBegin => {
                    errs.push(err(1, format!("`LoanClose` of %{word}, no `LoanBegin`")));
                }
                Op::Inst if tag == Tag::LoanBegin && word >= i => {
                    errs.push(err(1, format!("`LoanBegin` of %{word}, after it")));
                }
                Op::Value if word != NONE && word & Ref::CONST_BIT == 0 && word >= i => {
                    errs.push(err(1, format!("value %{word} used before it is defined")));
                }
                Op::Values if call => {} // Arguments and providers are checked via slots below.
                Op::Values => {
                    if (word as usize) >= b.extra.len() {
                        errs.push(err(1, "operand record out of range".into()));
                        continue;
                    }
                    for &v in b.record(word) {
                        if v & Ref::CONST_BIT == 0
                            && v >= i
                            && !matches!(tag, Tag::Call | Tag::Await)
                        {
                            errs.push(err(1, format!("value %{v} used before it is defined")));
                        }
                    }
                }
                Op::Label if (word as usize) >= b.label_inst.len() => {
                    errs.push(err(5, "unknown label".into()));
                }
                Op::Place => errs.extend(place_errors(b, i, word)),
                _ => {}
            }
        }
        if call {
            slots(b, i as usize, |loc, v, slot| {
                if matches!(loc, Loc::Extra(_))
                    && matches!(slot, Word::Value(_) | Word::Provider | Word::ProviderRef)
                    && v != NONE
                    && v & Ref::CONST_BIT == 0
                    && v >= i
                {
                    errs.push(err(1, format!("value %{v} used before it is defined")));
                }
            });
            // Empty synthetic records have no footer. Every nonempty call
            // record carries the same three-word provider footer.
            if w != NONE && b.extra.get(w as usize).is_some_and(|&n| n > 0) {
                match b.call_footer(i as usize) {
                    Some(at) => {
                        let (kind, payload, len) = (b.extra[at], b.extra[at + 1], b.extra[at + 2]);
                        let valid = match kind {
                            0 => payload == 0 && len == 0,
                            1 => {
                                len % 2 == 0
                                    && payload
                                        .checked_add(len)
                                        .is_some_and(|end| end as usize <= b.extra.len())
                            }
                            2 => len == 0,
                            _ => false,
                        };
                        if !valid {
                            errs.push(err(8, "malformed call provider footer".into()));
                        }
                        if tag == Tag::CallDyn {
                            let start = w as usize + 1;
                            if b.extra
                                .get(start + 2)
                                .is_none_or(|&count| start + 3 + count as usize > at)
                            {
                                errs.push(err(17, "malformed dynamic call header".into()));
                            }
                        }
                    }
                    None => errs.push(err(8, "missing call provider footer".into())),
                }
            }
        }
        if tag == Tag::Coerce
            && b.extra.get(w as usize + 1) == Some(&(Coercion::RowAdapt as u32))
            && b.row_adaptation(i as usize).is_none()
        {
            errs.push(err(
                4,
                "RowAdapt requires a two-word coercion record with type-list evidence".into(),
            ));
        }
        if tag == Tag::Block {
            for &child in b.record(a) {
                if child >= n {
                    errs.push(err(2, format!("block lists missing instruction {child}")));
                } else if owner[child as usize] != NONE {
                    errs.push(err(2, format!("instruction {child} is in two blocks")));
                } else {
                    owner[child as usize] = i;
                }
            }
        }
    }
    let ty_of = |v: u32| {
        if v & Ref::CONST_BIT != 0 {
            b.consts.get((v & !Ref::CONST_BIT) as usize).map(|c| c.0)
        } else {
            b.ty.get(v as usize).copied()
        }
    };
    for (i, &tag) in b.tags.iter().enumerate() {
        let [a, w] = b.data[i];
        // Of invariant 4, the operands a body's own columns type: a
        // stored value against its local, a `break` value against its
        // target. `never` goes anywhere, and poison follows an error.
        let want = match tag {
            Tag::Store if a & place::BARE_LOCAL != 0 => {
                b.local_ty.get((a & !place::BARE_LOCAL) as usize).copied()
            }
            Tag::Break if w != NONE => b
                .label_inst
                .get(a as usize)
                .and_then(|&t| b.ty.get(t as usize).copied()),
            _ => None,
        };
        if let Some(want) = want
            && let Some(got) = ty_of(w)
            && got != want
            && ![got, want]
                .iter()
                .any(|t| *t == Ty::NEVER || *t == Ty::POISON)
        {
            errs.push(VerifyError {
                invariant: 4,
                inst: u32::try_from(i).expect("i"),
                what: format!(
                    "{} value of type {} where {} is expected",
                    tag.name(),
                    got.0,
                    want.0
                ),
            });
        }
        // Invariant 21: a `Store` or a mutable `Ref` reaches a writable
        // place. A malformed place is invariant 20's.
        if (tag == Tag::Store || (tag == Tag::Ref && w & ref_word::MUT != 0))
            && let Some(p) = b.decode_place(a)
            && !p.writable()
        {
            errs.push(VerifyError {
                invariant: 21,
                inst: u32::try_from(i).expect("i"),
                what: format!("{} into place {p}, which is not writable", tag.name()),
            });
        }
        if matches!(tag, Tag::Call | Tag::Await) {
            let args = b.call_args(i);
            for (k, &v) in args.iter().enumerate() {
                let d = v as usize;
                if v & Ref::CONST_BIT != 0 || b.tags.get(d) != Some(&Tag::DefaultCall) {
                    continue;
                }
                if b.call_args(d) != &args[..k] {
                    errs.push(VerifyError {
                        invariant: 15,
                        inst: v,
                        what: format!("argument {k} of call %{i} is a default over other values"),
                    });
                }
            }
        }
        if tag == Tag::Defer {
            // A suspension point may stand in a suite: a `using!` binding's
            // `dispose!` (flow.using.async.suite-suspends).
            let suites = cleanup_suite_violations(b, b.data[i][0]);
            for v in suites
                .into_iter()
                .filter(|v| !is_suspension(b.tags[*v as usize]))
            {
                errs.push(VerifyError {
                    invariant: 6,
                    inst: v,
                    what: format!("{} inside a cleanup suite", b.tags[v as usize].name()),
                });
            }
        }
    }
    for (l, &at) in b.label_inst.iter().enumerate() {
        if at == NONE {
            errs.push(VerifyError {
                invariant: 5,
                inst: NONE,
                what: format!("label {l} never written"),
            });
        }
    }
    for (c, l) in b.cap_local.iter().enumerate() {
        if l.idx() >= b.local_ty.len() || c >= b.cap_mode.len() {
            errs.push(VerifyError {
                invariant: 10,
                inst: NONE,
                what: format!("capture {c} names no local or has no mode"),
            });
        }
    }
    for &(sub, capture) in &b.inlined_shared {
        if sub.idx() >= b.sub_root.len()
            || capture as usize >= b.cap_local.len()
            || b.cap_mode.get(capture as usize).is_none_or(|m| !m.shares())
        {
            errs.push(VerifyError {
                invariant: 10,
                inst: NONE,
                what: format!("invalid inlined shared capture {capture} in sub {sub:?}"),
            });
        }
    }
    let mut snapshots = hd_base::HashSet::default();
    for &(sub, snapshot, creator) in &b.inlined_capture_origins {
        if sub.idx() >= b.sub_root.len()
            || snapshot.idx() >= b.local_ty.len()
            || creator.idx() >= snapshot.idx()
            || !snapshots.insert(snapshot)
        {
            errs.push(VerifyError {
                invariant: 10,
                inst: NONE,
                what: format!(
                    "invalid inlined capture origin {snapshot:?} <- {creator:?} in {sub:?}"
                ),
            });
        }
    }
    for row in &b.loan_slots {
        let in_range = row.call < n && row.value < n;
        let mut need_field = false;
        let mut path_ok = true;
        for step in &row.path {
            match step {
                LoanField::Variant(_) => {
                    path_ok &= !need_field;
                    need_field = true;
                }
                LoanField::Field(_) => need_field = false,
            }
        }
        if !in_range || !path_ok || need_field {
            errs.push(VerifyError {
                invariant: 1,
                inst: row.call,
                what: "a call-loan fragment names an invalid value or path".into(),
            });
        }
    }
    types_are_global(b, &mut errs);
    suspension_scopes(b, topo, &mut errs);
    constant_rows(b, &mut errs);
    errs
}

/// The `Hole` and `Poison` instructions of `b`, a reserved slot never
/// filled aside (invariant 14's). Invariant 3 allows them only in a module
/// with errors, which only the driver knows in full: an import's error is
/// reported outside the body that reads its poisoned name.
#[must_use]
pub fn poison_sites(b: &Body) -> Vec<u32> {
    (0..b.len())
        .filter(|&i| match b.tags[i] {
            Tag::Hole => b.data[i] != [NONE, NONE],
            Tag::Poison => true,
            _ => false,
        })
        .map(|i| u32::try_from(i).expect("i"))
        .collect()
}

/// Invariant 20: place word `word` of instruction `user` is well formed.
/// It decodes; a local root names a local of the body; a value root and
/// each element index precede their user (of invariant 1, def-before-use
/// only); a frame root occurs only in tier 2, so never yet; a pointer
/// root is the whole place of a `Read` or a `Store`.
fn place_errors(b: &Body, user: u32, word: u32) -> Vec<VerifyError> {
    let err = |what: String| VerifyError {
        invariant: 20,
        inst: user,
        what,
    };
    let Some(p) = b.decode_place(word) else {
        return vec![err(format!("malformed place word {word}"))];
    };
    let mut out = Vec::new();
    match p.root {
        PlaceRoot::Local(l) if l.idx() >= b.local_ty.len() => {
            out.push(err(format!("place {p} names no local")));
        }
        PlaceRoot::Frame(_) => out.push(err(format!("place {p} has a frame root (tier 2)"))),
        PlaceRoot::Ptr(_)
            if !p.proj.is_empty()
                || !matches!(b.tags.get(user as usize), Some(Tag::Read | Tag::Store)) =>
        {
            out.push(err(format!("place {p} is no whole read or store")));
        }
        _ => {}
    }
    for v in p.values() {
        if v & Ref::CONST_BIT == 0 && v >= user {
            out.push(err(format!(
                "value %{v} of place {p} used before it is defined"
            )));
        }
    }
    out
}

/// An instruction number or column length as the `u32` the columns hold.
/// Converts a checked TIR column or record position to its encoded index.
#[must_use]
pub fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("insts")
}

pub(crate) fn is_suspension(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
    )
}

/// Invariant 3: after `finish` no type is in the body-local pool: no
/// instruction, local or constant type, and no type or type list in a
/// record the wire form remaps.
fn types_are_global(b: &Body, errs: &mut Vec<VerifyError>) {
    // A type list is a pool index too: bit 31 marks a local one.
    let local = |w: u32| w != NONE && Ty(w).is_local();
    let mut err = |inst: u32, what: String| {
        errs.push(VerifyError {
            invariant: 3,
            inst,
            what,
        });
    };
    for (i, t) in b.ty.iter().enumerate() {
        if local(t.0) {
            err(
                u32::try_from(i).expect("i"),
                "result type is body-local".into(),
            );
        }
    }
    for (l, t) in b.local_ty.iter().enumerate() {
        if local(t.0) {
            err(NONE, format!("local {l} has a body-local type"));
        }
    }
    for (c, (t, _)) in b.consts.iter().enumerate() {
        if local(t.0) {
            err(NONE, format!("constant {c} has a body-local type"));
        }
    }
    for i in 0..b.len() {
        // A tag the wire form does not map is refused there.
        let Ok(words) = crate::wire::id_words(b, i) else {
            continue;
        };
        for w in words {
            if let crate::wire::IdWord::Ty(at) | crate::wire::IdWord::List(at) = w
                && b.extra.get(at).is_some_and(|&x| local(x))
            {
                err(
                    u32::try_from(i).expect("i"),
                    format!("record word {at} is a body-local type"),
                );
            }
        }
    }
}

/// Invariant 7: a suspension row names a suspension point, and its scope
/// list (words of `extra`) is the `Scope`s around that point, innermost
/// first. A suspension point with no row is not checked: no body writes
/// rows yet.
fn suspension_scopes(b: &Body, topo: &Topology, errs: &mut Vec<VerifyError>) {
    let n = b.len();
    let mut scope_of = vec![NONE; n];
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag == Tag::Scope
            && let Some(x) = scope_of.get_mut(b.data[i][0] as usize)
        {
            *x = u32::try_from(i).expect("i");
        }
    }
    for row in &b.susp {
        let mut err = |what: &str| {
            errs.push(VerifyError {
                invariant: 7,
                inst: row.inst,
                what: what.into(),
            });
        };
        if (row.inst as usize) >= n || !is_suspension(b.tags[row.inst as usize]) {
            err("suspension row names no suspension point");
            continue;
        }
        let want: Vec<u32> = topo
            .enclosing(row.inst)
            .map(|blk| scope_of[blk as usize])
            .filter(|&s| s != NONE)
            .collect();
        let (start, len) = (row.scopes.start as usize, row.scopes.len as usize);
        match b.extra.get(start..start + len) {
            None => err("suspension row's scope list is out of range"),
            Some(got) if got != want.as_slice() => {
                err("suspension row does not list exactly the enclosing scopes");
            }
            Some(_) => {}
        }
    }
}

/// Invariant 12, its first half: a constant `Ref` names a row of `consts`.
/// The row's type against its position is invariant 4's.
fn constant_rows(b: &Body, errs: &mut Vec<VerifyError>) {
    for i in 0..b.len() {
        let mut rows = slot_words(b, i, |s| {
            matches!(s, Word::Value(_) | Word::Provider | Word::ProviderRef)
        });
        rows.retain(|&v| v != NONE && v & Ref::CONST_BIT != 0);
        // A constant can fill two slots.
        rows.sort_stable();
        rows.dedup();
        for k in rows.into_iter().map(|v| v & !Ref::CONST_BIT) {
            if k as usize >= b.consts.len() {
                errs.push(VerifyError {
                    invariant: 12,
                    inst: u32_of(i),
                    what: format!("constant {k} has no row"),
                });
            }
        }
    }
}

/// Prints a body as text. Every column is printed, so `parse` rebuilds it.
#[must_use]
pub fn print(b: &Body) -> String {
    let mut o = String::new();
    let phase = match b.phase {
        Phase::Checked => "",
        Phase::Instance => " Instance",
        Phase::Lowered => " Lowered",
        Phase::Elaborated => " Elaborated",
    };
    let _ = writeln!(o, "body {} {:?}{phase}", b.item.raw(), b.kind);
    let words = |v: &[u32]| {
        v.iter()
            .map(|w| {
                if *w == NONE {
                    "-".to_owned()
                } else {
                    w.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    for i in 0..b.local_ty.len() {
        let _ = writeln!(
            o,
            "local {} {} {} {}",
            b.local_ty[i].0,
            b.local_name[i].raw(),
            words(&[b.local_syn[i].raw()]),
            b.local_flags[i]
        );
    }
    for i in 0..b.sub_root.len() {
        let p = b.sub_params[i];
        let _ = writeln!(
            o,
            "sub {} {} {} {} {}",
            words(&[b.sub_root[i]]),
            p.start,
            p.len,
            words(&[b.sub_parent[i].raw()]),
            b.sub_flags[i]
        );
    }
    let _ = writeln!(o, "labels {}", words(&b.label_inst));
    let caps: Vec<u32> = b.cap_local.iter().map(|l| l.raw()).collect();
    let _ = writeln!(o, "caps {}", words(&caps));
    let modes: Vec<u32> = b.cap_mode.iter().map(|m| *m as u32).collect();
    let _ = writeln!(o, "modes {}", words(&modes));
    for s in &b.susp {
        let _ = writeln!(
            o,
            "susp {} {} {} {}",
            s.inst, s.scopes.start, s.scopes.len, s.hook_site
        );
    }
    let _ = writeln!(o, "extra {}", words(&b.extra));
    for st in &b.strings {
        let _ = write!(o, "str ");
        for x in st.bytes() {
            let _ = write!(o, "{x:02x}");
        }
        let _ = writeln!(o, ".");
    }
    if !b.label.is_empty() {
        let _ = write!(o, "label ");
        for x in b.label.bytes() {
            let _ = write!(o, "{x:02x}");
        }
        let _ = writeln!(o, ".");
    }
    for [inst, local] in &b.fallback {
        let _ = writeln!(o, "fallback {inst} {local}");
    }
    for (call, arg) in &b.held_args {
        let _ = writeln!(o, "heldarg {call} {arg}");
    }
    for slot in &b.loan_slots {
        let _ = write!(
            o,
            "loanslot {} {} {} {}",
            slot.call,
            slot.value,
            slot.origin,
            u32::from(slot.expand)
        );
        for step in &slot.path {
            match step {
                LoanField::Field(index) => {
                    let _ = write!(o, " 0 {index}");
                }
                LoanField::Variant(index) => {
                    let _ = write!(o, " 1 {index}");
                }
            }
        }
        let _ = writeln!(o);
    }
    for (sub, local) in &b.exit_drops {
        let _ = writeln!(o, "exitdrop {} {}", sub.raw(), local.raw());
    }
    for (sub, local, storage) in &b.storage {
        let _ = writeln!(
            o,
            "storage {} {} {}",
            sub.raw(),
            local.raw(),
            storage.name()
        );
    }
    for (inst, block) in &b.cancel {
        let _ = writeln!(o, "cancel {inst} {block}");
    }
    for [inst, word, before, after, buffer] in &b.stable_lists {
        let _ = writeln!(o, "stablelist {inst} {word} {before} {after} {buffer}");
    }
    for (inst, read) in &b.forwarded_reads {
        let _ = writeln!(o, "forwardread {inst} {read}");
    }
    for (t, bits) in &b.consts {
        let _ = writeln!(o, "const {} {bits}", t.0);
    }
    for i in 0..b.len() {
        let [a, w] = b.data[i];
        let _ = writeln!(
            o,
            "%{i} = {} {} : {} @{}",
            b.tags[i].name(),
            words(&[a, w]),
            b.ty[i].0,
            words(&[b.syn[i].raw()])
        );
    }
    o
}

/// A parse error in TIR text: line number and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub what: String,
}

fn word(s: &str) -> Result<u32, String> {
    if s == "-" {
        Ok(NONE)
    } else {
        s.parse().map_err(|_| format!("bad number `{s}`"))
    }
}

fn kind_of(s: &str) -> Result<BodyKind, String> {
    Ok(match s {
        "Fn" => BodyKind::Fn,
        "Init" => BodyKind::Init,
        "TestCase" => BodyKind::TestCase,
        "Fact" => BodyKind::Fact,
        "Default" => BodyKind::Default,
        "DeriveInstance" => BodyKind::DeriveInstance,
        "Handle" => BodyKind::Handle,
        _ => return Err(format!("bad body kind `{s}`")),
    })
}

/// Parses `print`'s text back into a body.
pub fn parse(text: &str) -> Result<Body, ParseError> {
    let mut body: Option<Body> = None;
    for (ln, line) in text.lines().enumerate() {
        let e = |what: String| ParseError { line: ln + 1, what };
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        if f[0] == "body" {
            let item = DefId::from_raw(
                word(f.get(1).ok_or_else(|| e("missing item".into()))?).map_err(e)?,
            );
            let mut b = Body::new(item, kind_of(f.get(2).copied().unwrap_or("")).map_err(e)?);
            b.phase = match f.get(3).copied() {
                None => Phase::Checked,
                Some("Instance") => Phase::Instance,
                Some("Lowered") => Phase::Lowered,
                Some("Elaborated") => Phase::Elaborated,
                Some(other) => return Err(e(format!("unknown phase `{other}`"))),
            };
            body = Some(b);
            continue;
        }
        let b = body
            .as_mut()
            .ok_or_else(|| e("missing `body` line".into()))?;
        let nums = |from: usize| -> Result<Vec<u32>, ParseError> {
            f[from..].iter().map(|s| word(s).map_err(e)).collect()
        };
        match f[0] {
            "local" => {
                let v = nums(1)?;
                if v.len() != 4 {
                    return Err(e("local needs 4 fields".into()));
                }
                b.local_ty.push(Ty(v[0]));
                b.local_name.push(Symbol::from_raw(v[1]));
                b.local_syn.push(NodeIdx::from_raw(v[2]));
                b.local_flags
                    .push(u16::try_from(v[3]).map_err(|_| e("flags".into()))?);
            }
            "sub" => {
                let v = nums(1)?;
                if v.len() != 5 {
                    return Err(e("sub needs 5 fields".into()));
                }
                b.sub_root.push(v[0]);
                b.sub_params.push(Range32::new(v[1], v[2]));
                b.sub_parent.push(SubId::from_raw(v[3]));
                b.sub_flags
                    .push(u8::try_from(v[4]).map_err(|_| e("flags".into()))?);
            }
            "labels" => b.label_inst = nums(1)?,
            "caps" => b.cap_local = nums(1)?.into_iter().map(LocalId::from_raw).collect(),
            "modes" => {
                b.cap_mode = nums(1)?
                    .into_iter()
                    .map(|m| match m {
                        0 => Ok(CaptureMode::Copy),
                        1 => Ok(CaptureMode::Move),
                        2 => Ok(CaptureMode::Shared),
                        3 => Ok(CaptureMode::Frame),
                        _ => Err(e("bad capture mode".into())),
                    })
                    .collect::<Result<_, _>>()?;
            }
            "susp" => {
                let v = nums(1)?;
                if v.len() != 4 {
                    return Err(e("susp needs 4 fields".into()));
                }
                b.susp.push(SuspRow {
                    inst: v[0],
                    scopes: Range32::new(v[1], v[2]),
                    hook_site: v[3],
                });
            }
            "str" | "label" => {
                let hex = f.get(1).copied().unwrap_or(".").trim_end_matches('.');
                let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
                    .collect();
                let text = bytes
                    .and_then(|v| String::from_utf8(v).ok())
                    .ok_or_else(|| e("bad string".into()))?;
                if f[0] == "label" {
                    b.label = text.into();
                } else {
                    b.strings.push(text.into());
                }
            }
            "extra" => b.extra = nums(1)?,
            "fallback" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("fallback needs 2 fields".into()));
                }
                b.fallback.push([v[0], v[1]]);
            }
            "forwardread" => match nums(1)?[..] {
                [value, read] => b.forwarded_reads.push((value, read)),
                _ => return Err(e("forwardread needs 2 fields".into())),
            },
            "heldarg" => match nums(1)?[..] {
                [c, a] => b.held_args.push((c, a)),
                _ => return Err(e("heldarg needs 2 fields".into())),
            },
            "loanslot" => {
                let v = nums(1)?;
                let [call, value, origin, expand, path @ ..] = v.as_slice() else {
                    return Err(e("loanslot needs at least 4 fields".into()));
                };
                if *expand > 1 || path.len() % 2 != 0 {
                    return Err(e("bad loanslot path or expansion".into()));
                }
                let path = path
                    .chunks_exact(2)
                    .map(|p| match p {
                        [0, index] => Ok(LoanField::Field(*index)),
                        [1, index] => Ok(LoanField::Variant(*index)),
                        _ => Err(e("bad loanslot path step".into())),
                    })
                    .collect::<Result<_, _>>()?;
                b.loan_slots.push(LoanSlot {
                    call: *call,
                    value: *value,
                    origin: *origin,
                    expand: *expand != 0,
                    path,
                });
            }
            "exitdrop" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("exitdrop needs 2 fields".into()));
                }
                b.exit_drops
                    .push((SubId::from_raw(v[0]), LocalId::from_raw(v[1])));
            }
            "storage" => {
                let [_, s, l, k] = f[..] else {
                    return Err(e("storage needs 3 fields".into()));
                };
                let (Ok(s), Ok(l)) = (s.parse(), l.parse()) else {
                    return Err(e("storage needs a sub-body and a local".into()));
                };
                let Some(k) = Storage::from_name(k) else {
                    return Err(e(format!("unknown storage {k}")));
                };
                b.storage
                    .push((SubId::from_raw(s), LocalId::from_raw(l), k));
            }
            "cancel" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("cancel needs 2 fields".into()));
                }
                b.cancel.push((v[0], v[1]));
            }
            "stablelist" => {
                let v = nums(1)?;
                let Ok(row) = v.try_into() else {
                    return Err(e("stablelist needs 5 fields".into()));
                };
                b.stable_lists.push(row);
            }
            "const" => {
                let t = word(f.get(1).copied().unwrap_or("")).map_err(e)?;
                let bits: u64 = f
                    .get(2)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| e("const bits".into()))?;
                b.consts.push((Ty(t), bits));
            }
            s if s.starts_with('%') => {
                // %i = Tag a b : ty @syn
                if f.len() != 8 || f[1] != "=" || f[5] != ":" || !f[7].starts_with('@') {
                    return Err(e("instruction line shape".into()));
                }
                let tag =
                    Tag::from_name(f[2]).ok_or_else(|| e(format!("unknown tag `{}`", f[2])))?;
                b.tags.push(tag);
                b.data
                    .push([word(f[3]).map_err(e)?, word(f[4]).map_err(e)?]);
                b.ty.push(Ty(word(f[6]).map_err(e)?));
                b.syn.push(NodeIdx::from_raw(word(&f[7][1..]).map_err(e)?));
            }
            other => return Err(e(format!("unknown line `{other}`"))),
        }
    }
    body.ok_or(ParseError {
        line: 0,
        what: "empty text".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        BodyKind, Callee, CaptureMode, ChoiceKind, Coercion, NONE, Providers, SuspRow, Tag,
        TirBuilder, TirSink, VerifyCx, parse, print, verify, verify_in,
    };
    use hd_base::{DefId, LocalId, NodeIdx, Range32, SubId, Symbol};
    use hd_types::{Ty, TyList};

    fn sample() -> super::Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let x = b.local(Ty::I32, Symbol::from_raw(0), super::local_flags::PARAM, n);
        let one = b.const_value(Ty::I32, 3);
        let g = b.get(x, Ty::I32, n);
        let sum = b.prim(0, &[g, one], Ty::I32, n);
        let callee = Callee::Item {
            def: DefId::from_raw(9),
            targs: TyList::EMPTY,
        };
        let c = b.call(&callee, &[sum], Providers::None, Ty::I32, n);
        let w = b.coerce(Coercion::WrapSome, super::NONE, c, Ty::I32, n);
        let cl = b.open_sub(&[]);
        let inner = b.open_block();
        b.capture(cl, x);
        let ig = b.get(x, Ty::I32, n);
        let ib = b.close_block(inner, Some(ig), Ty::I32, n);
        b.close_sub(cl, ib, Ty::I32, n);
        let lp = b.open_loop();
        let lb = b.open_block();
        b.emit(Tag::Break, lp.0.raw(), super::NONE, Ty::NEVER, n);
        let lbody = b.close_block(lb, None, Ty::VOID, n);
        b.close_loop(lp, lbody, Ty::VOID, n);
        b.emit(Tag::Return, w.0, super::NONE, Ty::NEVER, n);
        let root = b.close_block(blk, None, Ty::NEVER, n);
        b.finish(root, &[CaptureMode::Copy]).expect("verifies")
    }

    #[test]
    fn printer_and_parser_round_trip() {
        let mut b = sample();
        b.loan_slots.push(super::LoanSlot {
            call: 1,
            value: 0,
            path: vec![super::LoanField::Variant(1), super::LoanField::Field(2)],
            origin: 5,
            expand: true,
        });
        let text = print(&b);
        let back = parse(&text).expect("parses");
        assert_eq!(back, b);
        assert_eq!(print(&back), text);
    }

    #[test]
    fn verifier_rejects_use_before_def_and_double_ownership() {
        let mut b = sample();
        assert!(verify(&b).is_empty());
        let ret = b.tags.iter().position(|t| *t == Tag::Return).expect("ret");
        b.data[0] = [super::place::BARE_LOCAL, u32::try_from(ret).expect("i")];
        b.tags[0] = Tag::Store;
        assert!(verify(&b).iter().any(|e| e.invariant == 1));
        let mut b = sample();
        let root = b.sub_root[0] as usize;
        let rec = b.data[root][0] as usize;
        let first = b.extra[rec + 1];
        b.extra[rec + 2] = first;
        assert!(verify(&b).iter().any(|e| e.invariant == 2));
    }

    fn invariants(b: &super::Body) -> Vec<u8> {
        verify(b).iter().map(|e| e.invariant).collect()
    }

    #[test]
    fn verifier_rejects_body_local_types() {
        let local = 1 << 31;
        let mut b = sample();
        b.ty[0] = Ty(local);
        assert_eq!(invariants(&b), [3]);
        let mut b = sample();
        b.local_ty[0] = Ty(local);
        assert_eq!(invariants(&b), [3]);
        let mut b = sample();
        b.consts[0].0 = Ty(local);
        assert_eq!(invariants(&b), [3]);
        // The type arguments of a callee record.
        let mut b = sample();
        let call = b.tags.iter().position(|t| *t == Tag::Call).expect("call");
        let at = b.data[call][0] as usize;
        b.extra[at + 3] = local;
        assert_eq!(invariants(&b), [3]);
    }

    #[test]
    fn verifier_checks_suspension_scopes() {
        let mut b = TirBuilder::new(DefId::from_raw(2), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let sm = b.open_scope();
        let inner = b.open_block();
        let callee = Callee::Item {
            def: DefId::from_raw(9),
            targs: TyList::EMPTY,
        };
        let aw = b.call(&callee, &[], Providers::None, Ty::VOID, n);
        b.body_mut().tags[aw.0 as usize] = Tag::Await;
        let suite = b.open_block();
        let suite = b.close_block(suite, None, Ty::VOID, n);
        b.defer(suite, n);
        let body = b.close_block(inner, None, Ty::VOID, n);
        let scope = b.close_scope(sm, body, Ty::VOID, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        let list = b.words_record(&[scope.0]);
        b.body_mut().susp.push(SuspRow {
            inst: aw.0,
            scopes: Range32::new(list + 1, 1),
            hook_site: super::NONE,
        });
        let good = b.finish(root, &[]).expect("the row lists its scope");
        let mut bad = good.clone();
        bad.susp[0].scopes = Range32::new(0, 0);
        assert_eq!(invariants(&bad), [7]);
        let mut bad = good;
        bad.susp[0].inst = root.0;
        assert_eq!(invariants(&bad), [7]);
    }

    /// The checker's facts for the context checks: enum type 77 has three
    /// variants, builtin impl 3 answers trait 50, method 60 has two
    /// bounds, item 70's row has one key, and item 71's row has two.
    struct Facts;

    impl VerifyCx for Facts {
        fn variant_count(&self, ty: Ty) -> Option<u32> {
            (ty == Ty(77)).then_some(3)
        }
        fn builtin_trait(&self, index: u32) -> Option<DefId> {
            (index == 3).then_some(DefId::from_raw(50))
        }
        fn method_bounds(&self, method: DefId) -> Option<u32> {
            (method == DefId::from_raw(60)).then_some(2)
        }
        fn row_keys(&self, callee: &Callee) -> Option<u32> {
            match callee {
                Callee::Item { def, .. } if *def == DefId::from_raw(70) => Some(1),
                Callee::Item { def, .. } if *def == DefId::from_raw(71) => Some(2),
                _ => None,
            }
        }
    }

    /// `sample()` with one more instruction, `tag` over `a` and the record
    /// of `w`, and what [`verify_in`] finds beyond [`verify`].
    fn context_findings(tag: Tag, a: u32, w: Option<&[u32]>) -> Vec<u8> {
        let mut b = sample();
        let at = |b: &mut super::Body, words: &[u32]| {
            let r = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(words.len()).expect("len"));
            b.extra.extend_from_slice(words);
            r
        };
        let a = if tag == Tag::Call {
            at(&mut b, &[a])
        } else {
            a
        };
        let w = w.map_or(NONE, |w| at(&mut b, w));
        b.tags.push(tag);
        b.data.push([a, w]);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::from_raw(1));
        let base = invariants(&b);
        let mut out: Vec<u8> = verify_in(&b, &Facts).iter().map(|e| e.invariant).collect();
        for x in base {
            if let Some(k) = out.iter().position(|y| *y == x) {
                out.remove(k);
            }
        }
        out
    }

    #[test]
    fn verifier_checks_the_invariants_that_need_the_checker() {
        let k = |t: u32| t | super::Ref::CONST_BIT;
        // 3's other half: poison sites, for the driver to weigh.
        let mut b = sample();
        assert!(super::poison_sites(&b).is_empty());
        b.tags.push(Tag::Poison);
        b.data.push([NONE, NONE]);
        b.ty.push(Ty::POISON);
        b.syn.push(NodeIdx::from_raw(1));
        assert_eq!(
            super::poison_sites(&b),
            [u32::try_from(b.len() - 1).expect("i")]
        );
        // 9: no host call in TIR, and only known intrinsics.
        assert_eq!(context_findings(Tag::CallHost, 0, Some(&[])), [9]);
        assert_eq!(context_findings(Tag::Intrinsic, 9999, Some(&[])), [9]);
        assert!(context_findings(Tag::Intrinsic, 0, Some(&[])).is_empty());
        // 11: a tag switch without a default covers every variant; the
        // sample's constant 0 stands in for a value of enum type 77.
        let tag_switch = |cases: &[u32], default: u32| {
            let mut b = sample();
            b.consts[0].0 = Ty(77);
            let mut w: Vec<u32> = cases.iter().flat_map(|c| [*c, NONE]).collect();
            w.push(default);
            let r = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(w.len()).expect("len"));
            b.extra.extend_from_slice(&w);
            b.tags.push(Tag::SwitchTag);
            b.data.push([k(0), r]);
            b.ty.push(Ty::NEVER);
            b.syn.push(NodeIdx::from_raw(1));
            verify_in(&b, &Facts)
                .iter()
                .filter(|e| e.invariant == 11)
                .count()
        };
        assert_eq!(tag_switch(&[0, 1], NONE), 1);
        assert_eq!(tag_switch(&[0, 1, 2], NONE), 0);
        assert_eq!(tag_switch(&[0], 5), 0);
        assert_eq!(
            context_findings(Tag::SwitchInt, k(0), Some(&[k(0), 5, NONE])),
            [11]
        );
        // 13: a builtin choice names an impl of the callee's trait.
        let builtin = |trait_: u32, v: u32| {
            let c = Callee::TraitMethod {
                trait_: DefId::from_raw(trait_),
                method: DefId::from_raw(61),
                self_ty: Ty::I32,
                targs: TyList::EMPTY,
                choice: (ChoiceKind::Builtin, v),
            };
            let mut b = sample();
            let words = c.words();
            let ra = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(words.len()).expect("len"));
            b.extra.extend_from_slice(&words);
            let rw = u32::try_from(b.extra.len()).expect("extra");
            b.extra.extend_from_slice(&[3, 0, 0, 0]);
            b.tags.push(Tag::Call);
            b.data.push([ra, rw]);
            b.ty.push(Ty::VOID);
            b.syn.push(NodeIdx::from_raw(1));
            verify_in(&b, &Facts)
                .iter()
                .filter(|e| e.invariant == 13)
                .count()
        };
        assert_eq!(builtin(50, 4), 0);
        assert_eq!(builtin(51, 4), 1);
        assert_eq!(builtin(51, 0), 0, "no impl named");
        // 17: one evidence operand per method-level bound.
        assert_eq!(
            context_findings(Tag::CallDyn, k(0), Some(&[60, 0, 1])),
            [17]
        );
        assert!(context_findings(Tag::CallDyn, k(0), Some(&[60, 0, 2])).is_empty());
    }

    #[test]
    fn verifier_checks_provider_pairs_against_the_row() {
        // 8: projected pairs cover at most the row; lengths count words.
        let call = |mut prov: [u32; 3], item| {
            let c = Callee::Item {
                def: DefId::from_raw(item),
                targs: TyList::EMPTY,
            };
            let mut b = sample();
            let words = c.words();
            let ra = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(words.len()).expect("len"));
            b.extra.extend_from_slice(&words);
            if prov[0] == 1 {
                prov[1] = u32::try_from(b.extra.len()).expect("extra");
                b.extra.extend([
                    Ty::I32.0,
                    super::Ref::CONST_BIT,
                    Ty::STRING.0,
                    super::Ref::CONST_BIT,
                ]);
            }
            let rw = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(3);
            b.extra.extend_from_slice(&prov);
            b.tags.push(Tag::Call);
            b.data.push([ra, rw]);
            b.ty.push(Ty::VOID);
            b.syn.push(NodeIdx::from_raw(1));
            verify_in(&b, &Facts)
                .iter()
                .filter(|e| e.invariant == 8)
                .count()
        };
        assert_eq!(call([1, 0, 4], 70), 1);
        assert_eq!(call([1, 0, 2], 70), 0);
        assert_eq!(call([1, 0, 2], 71), 0); // The other key uses exact lookup.
        assert_eq!(call([1, 0, 0], 70), 0); // Identity-only keys need no explicit pair.
        assert!(call([1, 0, 1], 70) > 0);
        assert_eq!(call([3, 0, 0], 70), 1);
        assert_eq!(call([0, 0, 0], 70), 0);
    }

    #[test]
    fn verifier_rejects_a_constant_without_a_row() {
        let mut b = sample();
        b.consts.clear();
        assert_eq!(invariants(&b), [12]);
    }

    #[test]
    fn verifier_rejects_leaving_a_cleanup_suite() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let zero = b.const_value(Ty::I32, 0);
        let blk = b.open_block();
        let outer = b.open_loop();
        let ob = b.open_block();
        let sm = b.open_scope();
        let sb = b.open_block();
        let su = b.open_block();
        let inner = b.open_loop();
        let ib = b.open_block();
        b.emit(Tag::Break, inner.0.raw(), super::NONE, Ty::NEVER, n);
        let ibody = b.close_block(ib, None, Ty::VOID, n);
        b.close_loop(inner, ibody, Ty::VOID, n);
        let out = b.emit(Tag::Break, outer.0.raw(), super::NONE, Ty::NEVER, n);
        let cl = b.open_sub(&[]);
        let cb = b.open_block();
        b.emit(Tag::Return, zero.0, super::NONE, Ty::NEVER, n);
        let croot = b.close_block(cb, None, Ty::NEVER, n);
        b.close_sub(cl, croot, Ty::I32, n);
        let ret = b.emit(Tag::Return, zero.0, super::NONE, Ty::NEVER, n);
        let suite = b.close_block(su, None, Ty::VOID, n);
        b.defer(suite, n);
        let sbody = b.close_block(sb, None, Ty::VOID, n);
        b.close_scope(sm, sbody, Ty::VOID, n);
        let obody = b.close_block(ob, None, Ty::VOID, n);
        b.close_loop(outer, obody, Ty::VOID, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        let errs = b.finish(root, &[]).expect_err("leaves the suite");
        let six: Vec<u32> = errs
            .iter()
            .filter(|e| e.invariant == 6)
            .map(|e| e.inst)
            .collect();
        assert_eq!(six, [out.0, ret.0], "{errs:?}");
        assert_eq!(errs.len(), 2, "{errs:?}");
    }

    /// Places of every root and projection kind, from the design's
    /// examples (`docs/compiler/tir-v2.md`, "Places").
    fn places() -> Vec<super::Place> {
        [
            "[$3]",
            "[*$0 .0]",
            "[$1 .2 .0]",
            "[*$0 [%5] .3]",
            "[*%3 .0]",
            "[*$4 .1]",
            "[$0 as .1 .0]",
            "[@7:9 .1]",
            "[%c2 * .1]",
            "[**$2 .0 * [%c0]]",
            "[^2]",
        ]
        .iter()
        .map(|t| t.parse().expect(t))
        .collect()
    }

    #[test]
    fn places_round_trip_through_words_and_text() {
        use super::{Place, PlaceRoot, Proj, place};
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        for p in places() {
            let word = b.place(&p);
            assert_eq!(b.body_mut().decode_place(word).as_ref(), Some(&p), "{p}");
            assert_eq!(p.to_string().parse::<Place>().as_ref(), Ok(&p));
            let bare = matches!(p.root, PlaceRoot::Local(_)) && p.proj.is_empty();
            assert_eq!(word & place::BARE_LOCAL != 0, bare, "{p}");
        }
        // `self.x` through `self: &mut Point`: `[4, 0, 0, 1<<30, 0]`.
        let p: Place = "[*$0 .0]".parse().expect("place");
        let word = b.place(&p);
        let body = b.body_mut();
        assert_eq!(body.extra[word as usize..], [4, 0, 0, 1 << 30, 0]);
        assert_eq!(p.proj, [Proj::Deref, Proj::Field(0)]);
        // Leading dereferences print before the root, later ones after.
        let p: Place = "[**$2 .0 * [%c0]]".parse().expect("place");
        assert_eq!(p.proj[..3], [Proj::Deref, Proj::Deref, Proj::Field(0)]);
        assert_eq!(p.to_string(), "[**$2 .0 * [%c0]]");
        assert_eq!(p.values(), [super::Ref::konst(0).0]);
        // A pointer's target: `[2, 4, %3]`.
        let p: Place = "[ptr %3]".parse().expect("place");
        let word = b.place(&p);
        assert_eq!(b.body_mut().extra[word as usize..], [2, 4, 3]);
        assert_eq!(
            (p.to_string(), p.values(), p.writable()),
            ("[ptr %3]".into(), vec![3], true)
        );
        for bad in [
            "$0", "[]", "[$x]", "[$0 .]", "[$0 as]", "[$0 [5]]", "[#1]", "[ptr $0]",
        ] {
            assert!(bad.parse::<Place>().is_err(), "{bad}");
        }
    }

    #[test]
    fn an_unchecked_element_marks_one_access_and_decodes_as_its_place() {
        use super::{Place, Tag, place};
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let p: Place = "[**$2 .0 * [%c0]]".parse().expect("place");
        let word = b.place(&p);
        let body = b.body_mut();
        body.local_ty = vec![hd_types::Ty::VOID; 3];
        // A `Read` and a `Store` share the record; a `Call` names no place.
        for tag in [Tag::Read, Tag::Store, Tag::Call] {
            body.tags.push(tag);
            body.data.push([word, 0]);
        }
        let len = body.extra.len();
        body.uncheck_elem(0, 3); // a dereference, not an element
        body.uncheck_elem(2, 4); // not a place
        assert_eq!(body.extra.len(), len);
        body.uncheck_elem(0, 4);
        assert!(body.elem_unchecked(0, 4));
        assert!(!body.elem_unchecked(1, 4), "the shared record is copied");
        assert!(!body.elem_unchecked(0, 5), "past the last step");
        assert!(!body.elem_unchecked(2, 4));
        assert_eq!(body.decode_place(body.data[0][0]).as_ref(), Some(&p));
        let at = body.data[0][0] as usize;
        assert_eq!(body.extra[at + 7], place::PROJ_ELEM | place::ELEM_UNCHECKED);
    }

    #[test]
    fn a_rerooted_element_keeps_its_steps_and_marks_its_length() {
        use super::{Place, Ref, Tag, place};
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let p: Place = "[*$0 [%c0] .1]".parse().expect("place");
        let word = b.place(&p);
        let body = b.body_mut();
        body.local_ty = vec![hd_types::Ty::VOID];
        for tag in [Tag::Read, Tag::Store, Tag::Read] {
            body.tags.push(tag);
            body.data.push([word, 0]);
        }
        body.data[2][0] = place::BARE_LOCAL; // `[$0]`: no element
        body.uncheck_elem(1, 1);
        let len = LocalId::from_raw(7);
        let extra = body.extra.len();
        body.reroot_elem(2, Ref(1), len);
        assert_eq!(body.extra.len(), extra, "no element, no record");
        body.reroot_elem(0, Ref(1), len);
        body.reroot_elem(1, Ref(1), len);
        let want: Place = "[%1 [%c0] .1]".parse().expect("place");
        for i in [0, 1] {
            assert_eq!(body.decode_place(body.data[i][0]).as_ref(), Some(&want));
        }
        assert_eq!(body.elem_len(0, 0), Some(len));
        assert!(!body.elem_unchecked(0, 0));
        assert_eq!(body.elem_len(1, 0), None, "a cleared check needs no length");
        assert!(body.elem_unchecked(1, 0));
        assert_eq!(
            body.decode_place(word).as_ref(),
            Some(&p),
            "the shared record stays"
        );
    }

    #[test]
    fn malformed_place_records_do_not_decode() {
        use super::place::{PROJ_DEREF, PROJ_ELEM};
        let mut body = super::Body::new(DefId::from_raw(1), BodyKind::Fn);
        for (k, rec) in [
            vec![2, 5, 0],                 // root kind 5
            vec![3, 0, 0, PROJ_DEREF | 1], // a dereference with a payload
            vec![3, 0, 0, PROJ_ELEM],      // an element with no index
            vec![1, 0],                    // a global root missing its type
            vec![9, 0, 0],                 // past the end of `extra`
        ]
        .into_iter()
        .enumerate()
        {
            let at = u32::try_from(body.extra.len()).expect("extra");
            body.extra.extend(&rec);
            assert_eq!(body.decode_place(at), None, "record {k}");
        }
    }

    #[test]
    fn places_are_writable_through_locals_globals_and_dereferences() {
        let writable: Vec<bool> = places().iter().map(super::Place::writable).collect();
        assert_eq!(
            writable,
            [
                true, true, true, true, true, true, true, true, true, true, false
            ]
        );
        let value: super::Place = "[%3 .0]".parse().expect("place");
        assert!(!value.writable());
        let element: super::Place = "[%3 [%4] .0]".parse().expect("place");
        assert!(element.writable());
    }

    #[test]
    fn verifier_rejects_ill_formed_places() {
        let place_errors = |b: &super::Body, user: u32, text: &str| -> Vec<u8> {
            let p: super::Place = text.parse().expect("place");
            let mut b = b.clone();
            let word = match p.words() {
                None => {
                    let super::PlaceRoot::Local(l) = p.root else {
                        return vec![];
                    };
                    l.raw() | super::place::BARE_LOCAL
                }
                Some(words) => {
                    let at = u32::try_from(b.extra.len()).expect("extra");
                    b.extra.push(u32::try_from(words.len()).expect("len"));
                    b.extra.extend(words);
                    at
                }
            };
            super::place_errors(&b, user, word)
                .iter()
                .map(|e| e.invariant)
                .collect()
        };
        let b = sample();
        // The sample has one local and four instructions before its call.
        assert!(place_errors(&b, 4, "[*$0 .1 [%2]]").is_empty());
        assert!(place_errors(&b, 4, "[*%3 [%c0]]").is_empty());
        assert_eq!(place_errors(&b, 4, "[$1]"), [20], "no such local");
        assert_eq!(place_errors(&b, 4, "[*$7 .0]"), [20], "no such local");
        assert_eq!(
            place_errors(&b, 3, "[*%3 .0]"),
            [20],
            "root not yet defined"
        );
        assert_eq!(
            place_errors(&b, 3, "[*$0 [%5]]"),
            [20],
            "index not yet defined"
        );
        assert_eq!(place_errors(&b, 4, "[^0]"), [20], "frame roots are tier 2");
        assert_eq!(b.tags[0], Tag::Read);
        assert!(place_errors(&b, 0, "[ptr %c0]").is_empty());
        assert_eq!(
            place_errors(&b, 0, "[ptr %c0 .0]"),
            [20],
            "a pointer's whole target"
        );
        assert_eq!(
            place_errors(&b, 2, "[ptr %c0]"),
            [20],
            "only a read or a store"
        );
        assert_eq!(
            super::place_errors(&b, 4, 9999)
                .iter()
                .map(|e| e.invariant)
                .collect::<Vec<_>>(),
            [20],
            "malformed"
        );
    }

    /// Stage 1's forms (`docs/compiler/tir-v2.md`): `Point.shift`'s
    /// `self.x += dx`, whose read and store share one place record, then
    /// `p = 1; r := &mut p; *r = 5`. Types 40 to 42 stand in for `Point`,
    /// `&mut Point` and `&mut i32`.
    fn place_sample() -> super::Body {
        use super::{Place, PlaceRoot, PrimOp, Proj, ReadMode, local_flags::PARAM};
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let _this = b.local(Ty(41), Symbol::from_raw(0), PARAM, n);
        let dx = b.local(Ty::I32, Symbol::from_raw(1), PARAM, n);
        let p = b.local(Ty::I32, Symbol::from_raw(2), 0, n);
        let r = b.local(Ty(42), Symbol::from_raw(3), 0, n);
        let x = b.place(&"[*$0 .0]".parse().expect("place"));
        let read = b.emit(Tag::Read, x, ReadMode::Use as u32, Ty::I32, n);
        let d = b.get(dx, Ty::I32, n);
        let sum = b.prim(PrimOp::Add as u32, &[read, d], Ty::I32, n);
        b.emit(Tag::Store, x, sum.0, Ty::VOID, n);
        let one = b.const_value(Ty::I32, 1);
        let at_p = b.place(&Place::local(p));
        b.emit(Tag::Store, at_p, one.0, Ty::VOID, n);
        let rf = b.emit(Tag::Ref, at_p, 1, Ty(42), n);
        let at_r = b.place(&Place::local(r));
        b.emit(Tag::Store, at_r, rf.0, Ty::VOID, n);
        let rv = b.get(r, Ty(42), n);
        let five = b.const_value(Ty::I32, 5);
        let through = b.place(&Place {
            root: PlaceRoot::Value(rv),
            proj: vec![Proj::Deref],
        });
        b.emit(Tag::Store, through, five.0, Ty::VOID, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        b.finish(root, &[]).expect("verifies")
    }

    /// A top-level binding is read with `Read [@g]`: a place record with a
    /// global root and its type, which the type map reaches once.
    #[test]
    fn global_reads_are_reads_of_a_global_root() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let g = b.read_global(DefId::from_raw(9), Ty::I32, n);
        let root = b.close_block(blk, Some(g), Ty::I32, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        assert_eq!(body.global_read(0), Some(DefId::from_raw(9)));
        assert_eq!(body.local_read(0), None);
        assert_eq!(
            body.decode_place(body.data[0][0])
                .expect("place")
                .to_string(),
            format!("[@9:{}]", Ty::I32.0)
        );
        assert_eq!(parse(&print(&body)).expect("parses"), body);
        super::map_body_types(
            &mut body,
            &|t| if t == Ty::I32 { Ty(77) } else { t },
            &|l| l,
        );
        assert_eq!(
            body.decode_place(body.data[0][0]).expect("place").root,
            super::PlaceRoot::Global(DefId::from_raw(9), Ty(77))
        );
    }

    /// A struct field, a referent's field and a payload field are reads
    /// of one projection step from a value, which `read_step` names.
    #[test]
    fn projection_reads_name_their_step() {
        use super::Step;
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let v = b.const_value(Ty::I32, 7);
        let f = b.field(v, 2, Ty::I32, n);
        let place = b.place(&super::Place {
            root: super::PlaceRoot::Value(f),
            proj: vec![super::Proj::Deref, super::Proj::Field(1)],
        });
        let d = b.emit(Tag::Read, place, 0, Ty::I32, n);
        let p = b.payload(d, 1, 0, Ty::I32, n);
        let whole = b.place(&"[*%2]".parse().expect("place"));
        let r = b.emit(Tag::Read, whole, 0, Ty::I32, n);
        let root = b.close_block(blk, Some(r), Ty::I32, n);
        let body = b.finish(root, &[]).expect("verifies");
        assert_eq!(parse(&print(&body)).expect("parses"), body);
        let shown = |r: super::Ref| {
            let i = r.0 as usize;
            body.decode_place(body.data[i][0])
                .expect("place")
                .to_string()
        };
        assert_eq!(shown(f), "[%c0 .2]");
        assert_eq!(shown(d), "[*%0 .1]");
        assert_eq!(shown(p), "[%1 as .1 .0]");
        assert_eq!(body.read_step(0), Some((v.0, Step::Field(2))));
        assert_eq!(body.read_step(1), Some((0, Step::DerefField(1))));
        assert_eq!(body.read_step(2), Some((1, Step::Variant(1, 0))));
        assert_eq!(body.read_step(3), None);
        assert_eq!(body.value_operands(2), Some(vec![1]));
    }

    /// A list element is a read of an element place rooted at the list
    /// value, which `element_read` names; it is no projection step.
    #[test]
    fn element_reads_name_their_list_and_index() {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let xs = b.const_value(Ty::I32, 7);
        let k = b.const_value(Ty::I32, 0);
        let e = b.element(xs, k, Ty::I32, n);
        let f = b.field(e, 0, Ty::I32, n);
        let root = b.close_block(blk, Some(f), Ty::I32, n);
        let body = b.finish(root, &[]).expect("verifies");
        assert_eq!(parse(&print(&body)).expect("parses"), body);
        let place = body.decode_place(body.data[0][0]).expect("place");
        assert_eq!(place.to_string(), "[%c0 [%c1]]");
        assert_eq!(body.element_read(0), Some((xs.0, k.0)));
        assert_eq!(body.read_step(0), None);
        assert_eq!(body.element_read(1), None);
        assert_eq!(body.value_operands(0), Some(vec![xs.0, k.0]));
    }

    #[test]
    fn place_tags_verify_print_and_name_their_values() {
        let b = place_sample();
        let text = print(&b);
        assert_eq!(parse(&text).expect("parses"), b);
        let shown: Vec<String> = (0..b.len())
            .filter(|&i| matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref))
            .map(|i| {
                let [a, w] = b.data[i];
                let p = b.decode_place(a).expect("place");
                format!(
                    "{} {p} {:?}",
                    b.tags[i].name(),
                    b.value_operands(i).expect("values")
                ) + &if b.tags[i] == Tag::Store {
                    String::new()
                } else {
                    format!(" {w}")
                }
            })
            .collect();
        let c = |k: u32| super::Ref::konst(k).0;
        assert_eq!(
            shown,
            [
                "Read [*$0 .0] [] 0".to_owned(),
                "Read [$1] [] 0".to_owned(),
                "Store [*$0 .0] [2]".to_owned(),
                format!("Store [$2] [{}]", c(0)),
                "Ref [$2] [] 1".to_owned(),
                "Store [$3] [5]".to_owned(),
                "Read [$3] [] 0".to_owned(),
                format!("Store [*%7] [7, {}]", c(1)),
            ]
        );
        // A plain local read is a `Read` of the bare local, with no record.
        assert_eq!(b.local_read(1), Some(LocalId::from_raw(1)));
        assert_eq!(b.local_read(0), None);
        assert_eq!(b.global_read(1), None);
        // The read and the store of `self.x` share one record.
        let x = |t: Tag| b.tags.iter().position(|x| *x == t).map(|i| b.data[i][0]);
        assert_eq!(x(Tag::Read), x(Tag::Store));
    }

    #[test]
    fn verifier_checks_stores_and_mutable_references_reach_writable_places() {
        use super::{Place, PlaceRoot, Proj};
        // `sample()` plus one instruction over `place`.
        let with = |tag: Tag, place: &Place, w: u32| {
            let mut b = sample();
            let words = place.words().expect("a record");
            let at = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(words.len()).expect("len"));
            b.extra.extend(words);
            b.tags.push(tag);
            b.data.push([at, w]);
            b.ty.push(Ty::VOID);
            b.syn.push(NodeIdx::from_raw(1));
            invariants(&b)
        };
        let k = super::Ref::konst(0);
        let value = |proj: Vec<Proj>| Place {
            root: PlaceRoot::Value(super::Ref(3)),
            proj,
        };
        assert_eq!(with(Tag::Store, &value(vec![Proj::Field(0)]), k.0), [21]);
        assert_eq!(with(Tag::Ref, &value(vec![Proj::Field(0)]), 1), [21]);
        assert!(with(Tag::Ref, &value(vec![Proj::Field(0)]), 0).is_empty());
        assert!(with(Tag::Store, &value(vec![Proj::Deref, Proj::Field(0)]), k.0).is_empty());
        let local = "[$0 .1]".parse().expect("place");
        assert!(with(Tag::Store, &local, k.0).is_empty());
        // A stored value against a bare local's type.
        let mut b = sample();
        b.tags.push(Tag::Store);
        b.data.push([super::place::BARE_LOCAL, k.0]);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::from_raw(1));
        b.consts[0].0 = Ty::BOOL;
        assert!(invariants(&b).contains(&4));
    }

    /// A body as the ownership passes leave it, built by `f` inside one
    /// root block of a body with a parameter `$0` and a local `$1`, both
    /// `i32`; invariant 22 is tolerated, since it holds only before the
    /// passes.
    fn owned_body(f: impl FnOnce(&mut TirBuilder, NodeIdx)) -> super::Body {
        use super::local_flags::PARAM;
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let _x = b.local(Ty::I32, Symbol::from_raw(0), PARAM, n);
        let _y = b.local(Ty::I32, Symbol::from_raw(1), 0, n);
        f(&mut b, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        b.finish_tolerating(root, &[], &[22])
            .expect("structurally valid")
    }

    fn read(b: &mut TirBuilder, place: &str, mode: super::ReadMode, n: NodeIdx) -> super::Ref {
        let word = b.place(&place.parse().expect("place"));
        b.emit(Tag::Read, word, mode as u32, Ty::I32, n)
    }

    fn statement(b: &mut TirBuilder, tag: Tag, place: &str, n: NodeIdx) {
        let word = b.place(&place.parse().expect("place"));
        let w = if tag == Tag::IsUnique { 0 } else { NONE };
        b.emit(tag, word, w, Ty::VOID, n);
    }

    fn findings(errs: &[super::VerifyError]) -> Vec<(u8, u32)> {
        errs.iter().map(|e| (e.invariant, e.inst)).collect()
    }

    /// The passes' facts for the tests: `string` is counted, nothing else
    /// is, and every parameter is a value parameter.
    pub(crate) struct Counts;

    crate::own::test_cx::own_cx!(Counts, [temporary_owner_name, may_write], {
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }

        fn parts(&self, _: Ty, _: Option<u32>, out: &mut Vec<Ty>) -> bool {
            out.clear();
            false
        }
        fn received(&self, _: hd_base::DefId, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    });

    /// Invariant 22: the checker's TIR has no ownership: no decided read
    /// mode, no ownership statement, no exit set, cancel block or region row.
    #[test]
    fn checker_output_has_no_ownership() {
        use super::{ReadMode, verify};
        let body = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Copy, n);
            statement(b, Tag::Drop, "[$0]", n);
            read(b, "[$1]", ReadMode::Use, n);
        });
        assert_eq!(findings(&verify(&body)), [(22, 0), (22, 1)]);
        let mut rows = owned_body(|_, _| {});
        assert!(verify(&rows).is_empty());
        rows.exit_drops
            .push((SubId::from_raw(0), LocalId::from_raw(1)));
        assert_eq!(findings(&verify(&rows)), [(22, NONE)]);
        rows.exit_drops.clear();
        rows.cancel.push((0, 0));
        assert_eq!(findings(&verify(&rows)), [(22, NONE)]);
    }

    /// The new statements and columns print and parse back.
    #[test]
    fn ownership_statements_and_columns_round_trip() {
        use super::ReadMode;
        let mut body = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Borrow, n);
            statement(b, Tag::IsUnique, "[$1]", n);
            statement(b, Tag::Drop, "[$0]", n);
        });
        body.exit_drops
            .push((SubId::from_raw(0), LocalId::from_raw(1)));
        body.storage.push((
            SubId::from_raw(0),
            LocalId::from_raw(0),
            super::Storage::Slot,
        ));
        body.storage.push((
            SubId::from_raw(0),
            LocalId::from_raw(1),
            super::Storage::Cell,
        ));
        body.storage.push((
            SubId::from_raw(1),
            LocalId::from_raw(0),
            super::Storage::Static,
        ));
        body.cancel.push((2, 0));
        assert_eq!(parse(&print(&body)).expect("parses"), body);
    }

    /// Invariant 23: no read of a place after its move or drop on any
    /// path without a store that refills it, a loop's next round
    /// included.
    #[test]
    fn verifier_rejects_reads_after_a_move() {
        use super::{ReadMode, verify_owned};
        let moved = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Move, n);
            read(b, "[$0]", ReadMode::Copy, n);
        });
        assert_eq!(findings(&verify_owned(&moved, &Counts)), [(23, 1)]);
        let refilled = owned_body(|b, n| {
            let v = read(b, "[$0]", ReadMode::Move, n);
            let at = b.place(&"[$0]".parse().expect("place"));
            b.emit(Tag::Store, at, v.0, Ty::VOID, n);
            read(b, "[$0]", ReadMode::Copy, n);
            statement(b, Tag::Drop, "[$0]", n);
            statement(b, Tag::Drop, "[$0]", n);
        });
        assert!(verify_owned(&refilled, &Counts).is_empty());
        let in_a_loop = owned_body(|b, n| {
            let lp = b.open_loop();
            let body = b.open_block();
            read(b, "[$0]", ReadMode::Copy, n);
            read(b, "[$0]", ReadMode::Move, n);
            let yes = b.const_value(Ty::BOOL, 1);
            let tb = b.open_block();
            b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, n);
            let then = b.close_block(tb, None, Ty::VOID, n);
            let eb = b.open_block();
            let els = b.close_block(eb, None, Ty::VOID, n);
            let arms = b.refs_record(&[then, els]);
            b.emit(Tag::If, yes.0, arms, Ty::VOID, n);
            let body = b.close_block(body, None, Ty::VOID, n);
            b.close_loop(lp, body, Ty::VOID, n);
        });
        // The next round copies, then moves again, what this one moved.
        assert_eq!(
            findings(&verify_owned(&in_a_loop, &Counts)),
            [(23, 0), (23, 1)]
        );
    }

    /// Moving an owner out of a local empties the handle slot, not heap
    /// storage a raw pointer reaches. The new owner keeps that storage alive.
    #[test]
    fn verifier_allows_a_pointer_read_after_moving_an_addressed_owner() {
        use super::{ReadMode, verify_owned};
        let body = owned_body(|b, n| {
            let owner = b.place(&"[$0]".parse().expect("place"));
            b.emit(Tag::Ref, owner, 0, Ty::I32, n);
            let pointer = read(b, "[$1]", ReadMode::Copy, n);
            read(b, "[$0]", ReadMode::Move, n);
            read(b, &format!("[ptr %{}]", pointer.0), ReadMode::Copy, n);
        });
        assert!(verify_owned(&body, &Counts).is_empty());
    }

    /// A known reference into another field stays usable after a partial
    /// move. An overlapping reference, including one reassigned later,
    /// must still report the read of moved storage.
    #[test]
    fn verifier_distinguishes_known_local_reference_fields() {
        use super::{ReadMode, ref_word, verify_owned};
        for (field, reassign, allowed) in [(1, false, true), (0, false, false), (1, true, false)] {
            let body = owned_body(|b, n| {
                let at = b.place(&format!("[$0 .{field}]").parse().expect("place"));
                let reference = b.emit(Tag::Ref, at, ref_word::LOCAL, Ty::I32, n);
                let slot = b.place(&"[$1]".parse().expect("place"));
                b.emit(Tag::Store, slot, reference.0, Ty::VOID, n);
                if reassign {
                    let at = b.place(&"[$0 .0]".parse().expect("place"));
                    let reference = b.emit(Tag::Ref, at, ref_word::LOCAL, Ty::I32, n);
                    b.emit(Tag::Store, slot, reference.0, Ty::VOID, n);
                }
                read(b, "[$0 .0]", ReadMode::Move, n);
                read(b, "[*$1 .0]", ReadMode::Copy, n);
            });
            let errors = verify_owned(&body, &Counts);
            assert_eq!(errors.is_empty(), allowed, "{field} {reassign}: {errors:?}");
        }
    }

    #[test]
    fn verifier_resolves_long_local_reference_alias_chains() {
        use super::{ReadMode, ref_word, verify_owned};
        let body = owned_body(|b, n| {
            let at = b.place(&"[$0 .1]".parse().expect("place"));
            let reference = b.emit(Tag::Ref, at, ref_word::LOCAL, Ty::I32, n);
            let slot = b.place(&"[$1]".parse().expect("place"));
            b.emit(Tag::Store, slot, reference.0, Ty::VOID, n);
            let mut previous = 1;
            for k in 0..24 {
                let local = b.local(Ty::I32, Symbol::from_raw(k + 2), 0, n);
                let value = read(b, &format!("[${previous}]"), ReadMode::Copy, n);
                b.set(local, value, n);
                previous = local.raw();
            }
            read(b, "[$0 .0]", ReadMode::Move, n);
            read(b, &format!("[*${previous} .0]"), ReadMode::Copy, n);
        });
        let errors = verify_owned(&body, &Counts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    /// Invariant 23: no value a loop's body computes is still owned where
    /// the loop goes round again.
    #[test]
    fn verifier_rejects_a_value_owned_across_a_back_edge() {
        use super::local_flags::PARAM;
        use super::{ReadMode, verify_owned};
        let body = owned_body(|b, n| {
            b.local(Ty::STRING, Symbol::from_raw(2), PARAM, n);
            let lp = b.open_loop();
            let body = b.open_block();
            let word = b.place(&"[$2]".parse().expect("place"));
            b.emit(Tag::Read, word, ReadMode::Copy as u32, Ty::STRING, n);
            let body = b.close_block(body, None, Ty::VOID, n);
            b.close_loop(lp, body, Ty::VOID, n);
        });
        let errs = verify_owned(&body, &Counts);
        assert_eq!(findings(&errs), [(23, 2)]);
        assert!(
            errs[0]
                .what
                .contains("%0 is still owned where loop %2 goes round")
        );
    }

    /// Invariant 23: a borrow lives across no write to an overlapping place
    /// before its value's last use.
    #[test]
    fn verifier_rejects_a_borrow_across_a_write() {
        use super::{PrimOp, ReadMode, verify_owned};
        let across = |at: &'static str| {
            owned_body(move |b, n| {
                let v = read(b, "[$0]", ReadMode::Borrow, n);
                let one = b.const_value(Ty::I32, 1);
                let place = b.place(&at.parse().expect("place"));
                b.emit(Tag::Store, place, one.0, Ty::VOID, n);
                b.prim(PrimOp::Add as u32, &[v, one], Ty::I32, n);
            })
        };
        assert_eq!(findings(&verify_owned(&across("[$0]"), &Counts)), [(23, 0)]);
        assert!(verify_owned(&across("[$1]"), &Counts).is_empty());
        let after = owned_body(|b, n| {
            let v = read(b, "[$0]", ReadMode::Borrow, n);
            let one = b.const_value(Ty::I32, 1);
            b.prim(PrimOp::Add as u32, &[v, one], Ty::I32, n);
            statement(b, Tag::Drop, "[$0]", n);
        });
        assert!(verify_owned(&after, &Counts).is_empty());
    }

    /// Invariant 23: a decided mode for every read and an exit set in
    /// order.
    #[test]
    fn verifier_rejects_undecided_ownership() {
        use super::{ReadMode, verify_owned};
        let mut body = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Use, n);
        });
        body.exit_drops = vec![
            (SubId::from_raw(0), LocalId::from_raw(1)),
            (SubId::from_raw(0), LocalId::from_raw(0)),
        ];
        assert_eq!(
            findings(&verify_owned(&body, &Counts)),
            [(23, 0), (23, NONE)]
        );
    }

    #[test]
    fn rollback_restores_columns_and_every_tag_has_a_schema() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Init);
        let blk = b.open_block();
        let cp = b.checkpoint();
        b.emit(
            Tag::Unreachable,
            super::NONE,
            super::NONE,
            Ty::NEVER,
            NodeIdx::NONE,
        );
        let _ = b.const_str("trial");
        b.rollback(cp);
        assert_eq!(b.checkpoint(), cp, "constants and strings too");
        let one = b.const_value(Ty::I32, 1);
        let root = b.close_block(blk, Some(one), Ty::I32, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("ok");
        assert_eq!(body.len(), 1);
        assert_eq!(Tag::ALL.len(), 63);
        assert_eq!(Tag::Release as usize, 58);
        assert_eq!(Tag::MakeUnique as usize, 59);
        assert_eq!(
            Tag::MakeUnique.operands(),
            (super::Op::Inst, super::Op::Record)
        );
        assert!(
            Tag::ALL
                .iter()
                .all(|t| Tag::from_name(t.name()) == Some(*t))
        );
    }
}
