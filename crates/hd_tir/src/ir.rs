//! The full TIR (`crates/hd_tir/DESIGN.md` §4.13.11; `docs/compiler/data-structures.md` §3.18):
//! the instruction catalog with its operand schema, the body columns, the
//! `TirSink` builder API, a text printer, a text parser that round-trips,
//! and the verifier.
//!
//! Constants: `Ref::konst(i)` names row `i` of the body's own constant
//! column (`consts`), so a body and its TIR hash need no run-wide table.

use std::fmt::Write as _;

use hd_base::{
    CaptureId, DefId, LabelId, LocalId, NodeIdx, NotImplemented, Range32, Stage, SubId, Symbol,
};
use hd_types::{Ty, TyList};

pub const NONE: u32 = u32::MAX;

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
/// record in `extra`: a length word, then that many words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    None,
    Value,
    Block,
    Label,
    Local,
    Meta,
    Values,
    Blocks,
    Record,
}

tags! {
    // values, locals and globals
    LocalGet: Local, None;
    LocalSet: Local, Value;
    GlobalGet: Meta, None;
    GlobalSet: Meta, Value;
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
    Interp: None, Record;
    Coerce: Value, Record;
    // suspension and hooks
    Await: Record, Values;
    AwaitValue: Value, None;
    AwaitAll: None, Values;
    AwaitRace: Value, None;
    Hook: Meta, Meta;
    // data and closures
    NewData: None, Values;
    CopyData: Value, Record;
    NewVariant: Meta, Values;
    NewTuple: Value, Values;
    NewList: Record, Record;
    NewMap: None, Values;
    Field: Value, Meta;
    FieldSet: Value, Record;
    TupleGet: Value, Meta;
    Closure: Meta, Record;
    // references (`crates/hd_tir/DESIGN.md` §4.13.11, "References"): each
    // owns, as its last operand, the block that builds or uses the
    // `std.core.Ref` closure form, which only a backend without interior
    // addresses lowers
    RefPlace: Record, Block;
    RefRead: Value, Block;
    RefWrite: Values, Block;
    RefDyn: Block, Block;
    RefIdentity: Value, Block;
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
    ForRange: Record, Blocks;
    ForList: Record, Blocks;
    ForMap: Record, Blocks;
    Break: Label, Value;
    Continue: Label, None;
    Return: Value, None;
    Unreachable: None, None;
    // matching
    Match: Value, Blocks;
    SwitchTag: Value, Record;
    SwitchInt: Value, Record;
    SwitchChar: Value, Record;
    SwitchStr: Value, Record;
    Payload: Value, Record;
    Unwrap: Value, None;
    Guard: Block, Blocks;
    ToArm: Meta, None;
}

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
/// are written in terms of themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum IntrinsicOp {
    ListLen,
    ListPush,
    ListIter,
    ListIndex,
    ListSet,
    MapLen,
    MapGet,
    MapRemove,
    MapIndex,
    MapSet,
    MapIter,
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
    /// `(map, slot) -> (K, V)`: the entry in a live slot.
    MapSlotEntry,
    /// `(value) -> value`: a copy of a value that holds a `List` or a
    /// `Map` (spec/lang/04-type-system.md, "Collection Values"). Where
    /// the type holds none, the copy is the value itself. On the Wasm GC
    /// backend a collection's copy shares its buffer, and both marked as
    /// shared copy it before their next change (`crates/hd_wasm/DESIGN.md`
    /// §15.2, "Collection values").
    ValueCopy,
    /// `(list, index) -> element`: `ListIndex` for a write through the
    /// element (`xs[i].push(v)`, `xs[i].x = v`): the list's buffer is made
    /// unique first, so the write reaches only this list.
    ListIndexMut,
    /// `(map, key) -> value`: `MapIndex` for a write through the value
    /// (`m[k].push(v)`), with the map's buffer made unique first.
    MapIndexMut,
    /// `(collection) -> collection`: the snapshot a counted `for` over a
    /// list or a map iterates (`flow.for.snapshot`): it shares the
    /// collection's buffer, which a change during the loop copies first.
    SnapBegin,
    /// `(collection, snapshot)`: the loop over `snapshot` has ended; a
    /// collection the loop did not change stops copying for it.
    SnapEnd,
    /// `(list) -> list` or `(list, index) -> list`: the list an element
    /// reference points into (`ref.elem.pin`), its buffer made unique and
    /// pinned; an index is bounds-checked first. A pinned list panics
    /// `collection-pinned` on a length change (`crates/hd_wasm/DESIGN.md`
    /// §15.2, "Element references").
    ListPin,
    /// `(list)`: releases one pin of `ListPin` after the reference's last
    /// use (`ref.elem.release`).
    ListUnpin,
}

impl IntrinsicOp {
    pub const ALL: [IntrinsicOp; 25] = [
        IntrinsicOp::ListLen,
        IntrinsicOp::ListPush,
        IntrinsicOp::ListIter,
        IntrinsicOp::ListIndex,
        IntrinsicOp::ListSet,
        IntrinsicOp::MapLen,
        IntrinsicOp::MapGet,
        IntrinsicOp::MapRemove,
        IntrinsicOp::MapIndex,
        IntrinsicOp::MapSet,
        IntrinsicOp::MapIter,
        IntrinsicOp::StrIndex,
        IntrinsicOp::StrConcat,
        IntrinsicOp::StrEq,
        IntrinsicOp::Item,
        IntrinsicOp::MapSlots,
        IntrinsicOp::MapSlotNext,
        IntrinsicOp::MapSlotEntry,
        IntrinsicOp::ValueCopy,
        IntrinsicOp::ListIndexMut,
        IntrinsicOp::MapIndexMut,
        IntrinsicOp::SnapBegin,
        IntrinsicOp::SnapEnd,
        IntrinsicOp::ListPin,
        IntrinsicOp::ListUnpin,
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
    ToTraitValue,
    ToAny,
    Supertrait,
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
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Providers of a call: pairs, a context, or pending until M3 (3 words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Providers {
    None,
    Pairs(Range32),
    Context(Ref),
    Pending { call: u32 },
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
}

/// The root kinds of a `RefPlace` record, its first word. The record is
/// `[kind, x, y, field...]`: where the place starts, then the struct
/// field indices read from there in order. A root that is a value names
/// the local the checker holds it in, so the reference reads nothing
/// that its closure form does not.
pub mod place_root {
    /// `x` the local; `y` unused.
    pub const LOCAL: u32 = 0;
    /// `x` a top-level binding's item, `y` its type.
    pub const GLOBAL: u32 = 1;
    /// `x` a local holding a reference: the place it refers to.
    pub const DEREF: u32 = 2;
    /// `x` a local holding a data value, `y` the field's index.
    pub const DATA_FIELD: u32 = 3;
    /// `x` a local holding the list `ListPin` pinned, `y` a local holding
    /// the element's index, which `ListPin` checked.
    pub const ELEM: u32 = 4;
}

/// A `RefPlace` record decoded (`place_root`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceRec<'a> {
    pub kind: u32,
    pub x: u32,
    pub y: u32,
    pub path: &'a [u32],
}

/// Local flags (u8): READ | ASSIGNED | MUTATED | CAPTURED | `CAPTURED_ASSIGNED` | PARAM.
pub mod local_flags {
    pub const READ: u8 = 1;
    pub const ASSIGNED: u8 = 2;
    pub const MUTATED: u8 = 4;
    pub const CAPTURED: u8 = 8;
    pub const CAPTURED_ASSIGNED: u8 = 16;
    pub const PARAM: u8 = 32;
    /// A `:=` binding, which `=` cannot reassign.
    pub const SHORT: u8 = 64;
    /// A parameter of the function whose default is being checked, at or
    /// after the default's own: named, but not yet visible
    /// (`fn.default.later-parameter`). A read is an error.
    pub const NOT_YET: u8 = 128;
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

/// One body: the columns of §4.13.11 "Encoding".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub item: DefId,
    pub kind: BodyKind,
    pub tags: Vec<Tag>,
    pub data: Vec<[u32; 2]>,
    pub ty: Vec<Ty>,
    pub syn: Vec<NodeIdx>,
    pub extra: Vec<u32>,
    pub local_ty: Vec<Ty>,
    pub local_name: Vec<Symbol>,
    pub local_syn: Vec<NodeIdx>,
    pub local_flags: Vec<u8>,
    pub sub_root: Vec<u32>,
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
}

impl Body {
    #[must_use]
    pub fn new(item: DefId, kind: BodyKind) -> Self {
        Self {
            item,
            kind,
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
        }
    }

    /// A record in `extra`: its words after the length word.
    #[must_use]
    pub fn record(&self, at: u32) -> &[u32] {
        let n = self.extra[at as usize] as usize;
        &self.extra[at as usize + 1..at as usize + 1 + n]
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

    /// Each instruction's sub-body: the one whose root block holds it,
    /// directly or through nested blocks. A block's members precede it,
    /// so one backward pass settles every instruction.
    #[must_use]
    pub fn inst_subs(&self) -> Vec<u32> {
        let n = self.len();
        let mut owner = vec![NONE; n];
        for (i, tag) in self.tags.iter().enumerate() {
            if *tag == Tag::Block {
                for &c in self.record(self.data[i][0]) {
                    if let Some(o) = owner.get_mut(c as usize) {
                        *o = u32::try_from(i).expect("insts");
                    }
                }
            }
        }
        let mut root_of = vec![NONE; n];
        for (k, &r) in self.sub_root.iter().enumerate() {
            if let Some(x) = root_of.get_mut(r as usize) {
                *x = u32::try_from(k).expect("subs");
            }
        }
        let mut out = vec![0; n];
        for i in (0..n).rev() {
            let o = owner[i] as usize;
            out[i] = if root_of[i] != NONE {
                root_of[i]
            } else if o < n {
                out[o]
            } else {
                0
            };
        }
        out
    }

    /// The value operands of instruction `i` that compute one result from
    /// values and own no block, label or local (an erased body outlines
    /// such an instruction, `crates/hd_mono/DESIGN.md` §13.5.1), in the order emission
    /// reads them; constants included. `None` for any other instruction.
    #[must_use]
    pub fn value_operands(&self, i: usize) -> Option<Vec<u32>> {
        let [a, w] = self.data[i];
        let one = |v: u32| if v == NONE { vec![] } else { vec![v] };
        Some(match self.tags[i] {
            Tag::Prim | Tag::NewData | Tag::NewVariant | Tag::NewMap | Tag::DefaultCall => {
                self.record(w).to_vec()
            }
            // The last three words are the call's providers.
            Tag::Call => {
                let r = self.record(w);
                r[..r.len().saturating_sub(3)].to_vec()
            }
            Tag::CallValue => [one(a), self.record(w).to_vec()].concat(),
            Tag::Intrinsic | Tag::Interp => self.record(w).to_vec(),
            Tag::Is => [one(a), one(w)].concat(),
            Tag::Coerce | Tag::Field | Tag::TupleGet | Tag::Unwrap | Tag::Payload => one(a),
            Tag::GlobalSet => one(w),
            Tag::FieldSet => [one(a), self.record(w).get(1..2).unwrap_or(&[]).to_vec()].concat(),
            Tag::CopyData => {
                let pairs = self.record(w).iter().skip(1).step_by(2).copied();
                one(a).into_iter().chain(pairs).collect()
            }
            Tag::NewTuple => [self.record(w).to_vec(), one(a)].concat(),
            Tag::NewList if a == NONE => self.record(w).to_vec(),
            Tag::GlobalGet | Tag::ProviderGet | Tag::ItemRef => vec![],
            _ => return None,
        })
    }

    /// The place of `RefPlace` instruction `i`.
    #[must_use]
    pub fn place(&self, i: usize) -> Option<PlaceRec<'_>> {
        if self.tags[i] != Tag::RefPlace {
            return None;
        }
        match self.record(self.data[i][0]) {
            [kind, x, y, path @ ..] => Some(PlaceRec {
                kind: *kind,
                x: *x,
                y: *y,
                path,
            }),
            _ => None,
        }
    }

    /// The locals the place of `RefPlace` instruction `i` reads.
    #[must_use]
    pub fn place_locals(&self, i: usize) -> Vec<u32> {
        match self.place(i) {
            Some(PlaceRec {
                kind: place_root::LOCAL | place_root::DEREF | place_root::DATA_FIELD,
                x,
                ..
            }) => vec![x],
            Some(PlaceRec {
                kind: place_root::ELEM,
                x,
                y,
                ..
            }) => vec![x, y],
            _ => vec![],
        }
    }

    /// The value operands of reference instruction `i` outside its
    /// closure-form block, which a backend that lowers the reference
    /// itself reads: none for a `RefPlace` (its locals are
    /// [`Self::place_locals`]) or a `RefDyn` (its block). `None` for any
    /// other instruction.
    #[must_use]
    pub fn ref_operands(&self, i: usize) -> Option<Vec<u32>> {
        let a = self.data[i][0];
        Some(match self.tags[i] {
            Tag::RefPlace | Tag::RefDyn => vec![],
            Tag::RefRead | Tag::RefIdentity if a != NONE => vec![a],
            Tag::RefRead | Tag::RefIdentity => vec![],
            Tag::RefWrite => self.record(a).to_vec(),
            _ => return None,
        })
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
    fn local(&mut self, ty: Ty, name: Symbol, flags: u8, syn: NodeIdx) -> LocalId;
    fn get(&mut self, l: LocalId, ty: Ty, syn: NodeIdx) -> Ref;
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx);
    fn prim(&mut self, op: u32, args: &[Ref], ty: Ty, syn: NodeIdx) -> Ref;
    fn call(&mut self, callee: &Callee, args: &[Ref], prov: Providers, ty: Ty, syn: NodeIdx)
    -> Ref;
    fn coerce(&mut self, kind: Coercion, evidence: u32, v: Ref, to: Ty, syn: NodeIdx) -> Ref;
    fn emit(&mut self, tag: Tag, a: u32, b: u32, ty: Ty, syn: NodeIdx) -> Ref;
    fn open_block(&mut self) -> BlockMark;
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
            scratch: vec![],
            open: vec![],
            open_caps: vec![],
        }
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

    /// A record of values or blocks in `extra` (the `Values`/`Blocks`
    /// operand of `If`, `CallHost` and others).
    pub fn refs_record(&mut self, refs: &[Ref]) -> u32 {
        let words: Vec<u32> = refs.iter().map(|r| r.0).collect();
        self.record(&words)
    }

    /// The body so far (for the checker's final type pass).
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
        self,
        root: Ref,
        capture_modes: &[CaptureMode],
        tolerated: &[u8],
    ) -> Result<Body, Vec<VerifyError>> {
        self.finish_checked(root, capture_modes, tolerated, None)
    }

    /// [`Self::finish_tolerating`] with the checker's facts, so the
    /// verifier also checks the invariants that need them ([`verify_in`]).
    pub fn finish_in(
        self,
        root: Ref,
        capture_modes: &[CaptureMode],
        tolerated: &[u8],
        cx: &dyn VerifyCx,
    ) -> Result<Body, Vec<VerifyError>> {
        self.finish_checked(root, capture_modes, tolerated, Some(cx))
    }

    fn finish_checked(
        mut self,
        root: Ref,
        capture_modes: &[CaptureMode],
        tolerated: &[u8],
        cx: Option<&dyn VerifyCx>,
    ) -> Result<Body, Vec<VerifyError>> {
        self.body.sub_root[0] = root.0;
        self.body.cap_mode = capture_modes.to_vec();
        self.body
            .cap_mode
            .resize(self.body.cap_local.len(), CaptureMode::Shared);
        let mut errs = match cx {
            Some(cx) => verify_in(&self.body, cx),
            None => verify(&self.body),
        };
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
    fn local(&mut self, ty: Ty, name: Symbol, flags: u8, syn: NodeIdx) -> LocalId {
        let l = LocalId::from_raw(u32::try_from(self.body.local_ty.len()).expect("locals"));
        self.body.local_ty.push(ty);
        self.body.local_name.push(name);
        self.body.local_syn.push(syn);
        self.body.local_flags.push(flags);
        l
    }
    fn get(&mut self, l: LocalId, ty: Ty, syn: NodeIdx) -> Ref {
        self.push(Tag::LocalGet, l.raw(), NONE, ty, syn)
    }
    fn set(&mut self, l: LocalId, v: Ref, syn: NodeIdx) {
        self.push(Tag::LocalSet, l.raw(), v.0, Ty::VOID, syn);
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
        let mut w = Self::refs(args);
        w.extend(match prov {
            Providers::None => [0, 0, 0],
            Providers::Pairs(r) => [1, r.start, r.len],
            Providers::Context(c) => [2, c.0, 0],
            Providers::Pending { call } => [3, call, 0],
        });
        let a = self.record(&w);
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
/// §4.13.11, "Implementation differs"). Of them, 8, 11, 13 and 17 need
/// the checker's facts ([`VerifyCx`], [`verify_in`]), and 3's `Hole` and
/// `Poison` half the module's errors ([`poison_sites`]).
pub const CHECKED_INVARIANTS: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17];

/// What the verifier reads from the checker for the invariants a body
/// alone cannot show (`crates/hd_tir/DESIGN.md` §4.13.11): read-only facts
/// about the module, its items and the types the body names.
pub trait VerifyCx {
    /// How many variants `ty` has, an enum or an optional; `None` for any
    /// other type (invariant 11).
    fn variant_count(&self, ty: Ty) -> Option<u32>;
    /// The sealed trait whose rows the compiler-supplied implementation
    /// `index` (a `BuiltinImpl` as `u32`) answers (invariant 13).
    fn builtin_trait(&self, index: u32) -> Option<DefId>;
    /// How many method-level bounds trait method `method` has
    /// (invariant 17).
    fn method_bounds(&self, method: DefId) -> Option<u32>;
    /// How many row keys a call of `callee` supplies providers for, when
    /// the callee's row is closed (invariant 8).
    fn row_keys(&self, callee: &Callee) -> Option<u32>;
}

/// [`verify`], then the invariants that need the checker's facts `cx`:
/// - 8: a call's provider pairs, one per key of the callee's closed row,
///   and no `Pending` providers after `finish`;
/// - 9: no `CallHost` (std's provider bodies are generated in Wasm, not
///   TIR) and only known intrinsics;
/// - 11: a `SwitchTag` with no default has a case for every variant, and
///   `SwitchInt`, `SwitchChar` and `SwitchStr` have a default;
/// - 13: a `Builtin` choice that names a `BuiltinImpl` (its value is the
///   impl plus one) names one of the callee's trait;
/// - 17: a `CallDyn` has one evidence operand per method-level bound.
pub fn verify_in(b: &Body, cx: &dyn VerifyCx) -> Vec<VerifyError> {
    let mut errs = verify(b);
    let ty_of = |v: u32| {
        if v & Ref::CONST_BIT != 0 {
            b.consts.get((v & !Ref::CONST_BIT) as usize).map(|c| c.0)
        } else {
            b.ty.get(v as usize).copied()
        }
    };
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
            Tag::SwitchTag => {
                let r = b.record(w);
                let (cases, default) = r.split_at(r.len().saturating_sub(1));
                if default.first().is_some_and(|d| *d == NONE) {
                    let mut seen: Vec<u32> = cases.iter().step_by(2).copied().collect();
                    seen.sort_unstable();
                    seen.dedup();
                    let have = u32::try_from(seen.len()).expect("cases");
                    if let Some(n) = ty_of(a).and_then(|t| cx.variant_count(t))
                        && have < n
                    {
                        err(11, format!("{have} of {n} variants and no default"));
                    }
                }
            }
            Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
                if b.record(w).last().is_none_or(|d| *d == NONE) {
                    err(11, format!("{} without a default", tag.name()));
                }
            }
            Tag::Call | Tag::Await => {
                let callee = Callee::from_words(b.record(a));
                if let Some(Callee::TraitMethod {
                    trait_,
                    choice: (ChoiceKind::Builtin, v),
                    ..
                }) = callee
                    && (1..=u32::from(u8::MAX)).contains(&v)
                    && cx.builtin_trait(v - 1) != Some(trait_)
                {
                    err(
                        13,
                        format!("a builtin choice {} not of the callee's trait", v - 1),
                    );
                }
                let r = b.record(w);
                match r[r.len().saturating_sub(3)..] {
                    [3, ..] => err(8, "providers still pending".into()),
                    [1, _, len] => {
                        if let Some(n) = callee.as_ref().and_then(|c| cx.row_keys(c))
                            && n != len
                        {
                            err(8, format!("{len} provider pairs for {n} row keys"));
                        }
                    }
                    _ => {}
                }
            }
            Tag::CallDyn => {
                // `[method, type arguments, evidence count, evidence...,
                // arguments, providers]`.
                if let [method, _, count, ..] = *b.record(w)
                    && let Some(n) = cx.method_bounds(DefId::from_raw(method))
                    && n != count
                {
                    err(17, format!("{count} evidence operands for {n} bounds"));
                }
            }
            _ => {}
        }
    }
    errs
}

/// Maps every type a body records through `ty`, and every type list
/// through `list`: the instruction and local columns, the constants, and
/// the types and type lists kept in records (callees, `ItemRef`,
/// `DefaultCall`, instantiated closures, `ProviderGet`, `With` and
/// `ContextNew`). The checker lowers reference types this way
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
    for i in 0..b.tags.len() {
        let [a, w] = b.data[i];
        match b.tags[i] {
            Tag::Call | Tag::Await => {
                // `[0, def, targs]` or `[1, trait, method, self_ty, targs,
                // kind, value]` (`Callee::words`).
                let at = a as usize + 1;
                match b.extra.get(at).copied() {
                    Some(0) => {
                        b.extra[at + 2] = list(TyList(b.extra[at + 2])).0;
                    }
                    Some(1) => {
                        b.extra[at + 3] = ty(Ty(b.extra[at + 3])).0;
                        b.extra[at + 4] = list(TyList(b.extra[at + 4])).0;
                    }
                    _ => {}
                }
            }
            Tag::ProviderGet => {
                let at = a as usize + 1;
                b.extra[at] = ty(Ty(b.extra[at])).0;
            }
            Tag::With | Tag::ContextNew => {
                let len = b.extra[a as usize] as usize;
                for k in 0..len / 2 {
                    let at = a as usize + 1 + 2 * k;
                    b.extra[at] = ty(Ty(b.extra[at])).0;
                }
            }
            Tag::ItemRef => {
                let at = w as usize + 1;
                b.extra[at] = list(TyList(b.extra[at])).0;
            }
            Tag::DefaultCall => {
                let at = a as usize + 2;
                b.extra[at] = list(TyList(b.extra[at])).0;
            }
            Tag::Closure if b.is_instantiation(i) => {
                let at = w as usize + 4;
                b.extra[at] = list(TyList(b.extra[at])).0;
            }
            Tag::RefPlace if b.extra.get(a as usize + 1) == Some(&place_root::GLOBAL) => {
                let at = a as usize + 3;
                b.extra[at] = ty(Ty(b.extra[at])).0;
            }
            _ => {}
        }
    }
}

/// Removes the closure form of every reference instruction (`RefPlace`,
/// `RefRead`, `RefWrite`, `RefDyn`, `RefIdentity`) for a backend that
/// lowers references as addresses: each instruction of the form's block,
/// of the blocks they own and of the closures they make becomes an
/// `Unreachable` of type `never` that nothing reads, each such block a
/// block of no statements, and the reference instruction owns no block
/// any more. Whether anything was removed.
pub fn strip_closure_forms(b: &mut Body) -> bool {
    let is_ref = |t: Tag| {
        matches!(
            t,
            Tag::RefPlace | Tag::RefRead | Tag::RefWrite | Tag::RefDyn | Tag::RefIdentity
        )
    };
    let mut dead = vec![false; b.len()];
    let mut seen = vec![false; b.len()];
    let mut work: Vec<u32> = Vec::new();
    for i in 0..b.len() {
        if is_ref(b.tags[i]) && b.data[i][1] != NONE {
            work.push(b.data[i][1]);
        }
    }
    if work.is_empty() {
        return false;
    }
    while let Some(blk) = work.pop() {
        let Some(s) = seen.get_mut(blk as usize) else {
            continue;
        };
        if *s || b.tags[blk as usize] != Tag::Block {
            continue;
        }
        *s = true;
        dead[blk as usize] = true;
        for &c in b.record(b.data[blk as usize][0]) {
            let c = c as usize;
            if c >= b.len() {
                continue;
            }
            dead[c] = true;
            work.extend(owned_blocks(b, c));
            if b.tags[c] == Tag::Closure
                && !b.is_instantiation(c)
                && let Some(&root) = b.sub_root.get(b.data[c][0] as usize)
            {
                work.push(root);
            }
        }
    }
    // A dead block stays a block, of no statements, so the live block
    // that lists it still skips it as a block it does not run.
    let empty = u32::try_from(b.extra.len()).expect("extra");
    b.extra.push(0);
    for (i, &d) in dead.iter().enumerate() {
        if d {
            b.data[i] = if b.tags[i] == Tag::Block {
                [empty, NONE]
            } else {
                b.tags[i] = Tag::Unreachable;
                [NONE, NONE]
            };
            b.ty[i] = Ty::NEVER;
        } else if is_ref(b.tags[i]) {
            b.data[i][1] = NONE;
        }
    }
    true
}

/// The blocks instruction `i` owns through its operands: a block
/// operand, a block list, a switch's two arms, or a `Defer`'s suite.
fn owned_blocks(b: &Body, i: usize) -> Vec<u32> {
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    let mut out = Vec::new();
    for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
        match op {
            Op::Block => out.push(word),
            Op::Blocks => out.extend_from_slice(b.record(word)),
            _ => {}
        }
    }
    match tag {
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
            out.extend(b.record(w).iter().skip(1).take(2));
        }
        Tag::Defer => out.push(a),
        _ => {}
    }
    out.retain(|&x| b.tags.get(x as usize) == Some(&Tag::Block));
    out
}

/// The instructions of a `defer` suite that invariant 6 forbids: a
/// suspension point, a `Return`, or a `Break` or `Continue` whose loop
/// is outside the suite. A closure inside the suite is its own body and
/// is not searched: its root block, though listed in the suite, is a
/// sub-body root.
#[must_use]
pub fn defer_suite_violations(b: &Body, suite: u32) -> Vec<u32> {
    let mut seen = vec![false; b.len()];
    for &r in &b.sub_root {
        if let Some(s) = seen.get_mut(r as usize) {
            *s = true;
        }
    }
    let mut inside = Vec::new();
    let mut stack = vec![suite];
    while let Some(i) = stack.pop() {
        let Some(s) = seen.get_mut(i as usize) else {
            continue;
        };
        if std::mem::replace(s, true) {
            continue;
        }
        inside.push(i);
        if b.tags[i as usize] == Tag::Block {
            stack.extend(b.record(b.data[i as usize][0]));
        }
        stack.extend(owned_blocks(b, i as usize));
    }
    let mut in_suite = vec![false; b.len()];
    for &i in &inside {
        in_suite[i as usize] = true;
    }
    let within = |l: u32| {
        b.label_inst
            .get(l as usize)
            .is_some_and(|&t| in_suite.get(t as usize) == Some(&true))
    };
    let mut out: Vec<u32> = inside
        .iter()
        .copied()
        .filter(|&i| match b.tags[i as usize] {
            Tag::Return => true,
            Tag::Break | Tag::Continue => !within(b.data[i as usize][0]),
            tag => is_suspension(tag),
        })
        .collect();
    out.sort_unstable();
    out
}

/// Verifies a body (§4.13.11 "Invariants").
#[must_use]
pub fn verify(b: &Body) -> Vec<VerifyError> {
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
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value if word != NONE && word & Ref::CONST_BIT == 0 && word >= i => {
                    errs.push(err(1, format!("value %{word} used before it is defined")));
                }
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
                Op::Local if (word as usize) >= b.local_ty.len() => {
                    errs.push(err(1, "unknown local".into()));
                }
                _ => {}
            }
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
            Tag::LocalSet => b.local_ty.get(a as usize).copied(),
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
        if matches!(tag, Tag::Call | Tag::Await) {
            let args = call_args(b, i);
            for (k, &v) in args.iter().enumerate() {
                let d = v as usize;
                if v & Ref::CONST_BIT != 0 || b.tags.get(d) != Some(&Tag::DefaultCall) {
                    continue;
                }
                if b.record(b.data[d][1]) != &args[..k] {
                    errs.push(VerifyError {
                        invariant: 15,
                        inst: v,
                        what: format!("argument {k} of call %{i} is a default over other values"),
                    });
                }
            }
        }
        if tag == Tag::Defer {
            // A suspension point may stand in a suite: a `using` binding's
            // `dispose!` (flow.using.suite-suspends); the checker rejects one that
            // source writes in a `defer` suite (flow.defer.suspend).
            let suites = defer_suite_violations(b, b.data[i][0]);
            for v in suites
                .into_iter()
                .filter(|v| !is_suspension(b.tags[*v as usize]))
            {
                errs.push(VerifyError {
                    invariant: 6,
                    inst: v,
                    what: format!("{} inside a defer suite", b.tags[v as usize].name()),
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
    let tree = Tree::new(b, owner);
    types_are_global(b, &mut errs);
    suspension_scopes(b, &tree, &mut errs);
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

/// Block ownership from the block lists, and the sub-body roots.
struct Tree {
    owner: Vec<u32>,
    root: Vec<bool>,
}

impl Tree {
    fn new(b: &Body, owner: Vec<u32>) -> Self {
        let mut root = vec![false; b.len()];
        for &r in &b.sub_root {
            if let Some(x) = root.get_mut(r as usize) {
                *x = true;
            }
        }
        Self { owner, root }
    }

    /// The blocks that enclose instruction `i` in its own sub-body,
    /// innermost first. A sub-body's root block ends the walk, so a
    /// closure's instructions are never inside the blocks around it.
    fn enclosing(&self, i: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = i as usize;
        while cur < self.root.len() && !self.root[cur] && out.len() < self.owner.len() {
            let o = self.owner[cur];
            if o == NONE {
                break;
            }
            out.push(o);
            cur = o as usize;
        }
        out
    }
}

/// A call's arguments: its value record without the three provider words.
fn call_args(b: &Body, i: usize) -> &[u32] {
    let r = b.record(b.data[i][1]);
    &r[..r.len().saturating_sub(3)]
}

fn is_suspension(tag: Tag) -> bool {
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
fn suspension_scopes(b: &Body, tree: &Tree, errs: &mut Vec<VerifyError>) {
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
        let want: Vec<u32> = tree
            .enclosing(row.inst)
            .into_iter()
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
    for (i, (&tag, &[a, w])) in b.tags.iter().zip(&b.data).enumerate() {
        let mut vals: Vec<u32> = Vec::new();
        for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
            match op {
                Op::Value => vals.push(word),
                Op::Values if matches!(tag, Tag::Call | Tag::Await) => {
                    vals.extend_from_slice(call_args(b, i));
                }
                Op::Values if (word as usize) < b.extra.len() => {
                    vals.extend_from_slice(b.record(word));
                }
                _ => {}
            }
        }
        vals.extend(b.value_operands(i).unwrap_or_default());
        // A record can be both a `Values` operand and a value operand.
        vals.sort_unstable();
        vals.dedup();
        for v in vals {
            if v != NONE
                && v & Ref::CONST_BIT != 0
                && (v & !Ref::CONST_BIT) as usize >= b.consts.len()
            {
                errs.push(VerifyError {
                    invariant: 12,
                    inst: u32::try_from(i).expect("i"),
                    what: format!("constant {} has no row", v & !Ref::CONST_BIT),
                });
            }
        }
    }
}

/// Prints a body as text. Every column is printed, so `parse` rebuilds it.
#[must_use]
pub fn print(b: &Body) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "body {} {:?}", b.item.raw(), b.kind);
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
            body = Some(Body::new(
                item,
                kind_of(f.get(2).copied().unwrap_or("")).map_err(e)?,
            ));
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
                    .push(u8::try_from(v[3]).map_err(|_| e("flags".into()))?);
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
    use hd_base::{DefId, NodeIdx, Range32, Symbol};
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
        let b = sample();
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
        b.data[0][1] = u32::try_from(ret).expect("i");
        b.tags[0] = Tag::LocalSet;
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
    /// bounds, and item 70's row has one key.
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
            matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(70)).then_some(1)
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
        // 8: a call of item 70 with two provider pairs for its one key,
        // and one still pending.
        let call = |prov: [u32; 3]| {
            let c = Callee::Item {
                def: DefId::from_raw(70),
                targs: TyList::EMPTY,
            };
            let mut b = sample();
            let words = c.words();
            let ra = u32::try_from(b.extra.len()).expect("extra");
            b.extra.push(u32::try_from(words.len()).expect("len"));
            b.extra.extend_from_slice(&words);
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
        assert_eq!(call([1, 0, 2]), 1);
        assert_eq!(call([1, 0, 1]), 0);
        assert_eq!(call([3, 0, 0]), 1);
        assert_eq!(call([0, 0, 0]), 0);
    }

    #[test]
    fn verifier_rejects_a_constant_without_a_row() {
        let mut b = sample();
        b.consts.clear();
        assert_eq!(invariants(&b), [12]);
    }

    #[test]
    fn verifier_rejects_leaving_a_defer_suite() {
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
        assert_eq!(Tag::ALL.len(), 64);
        assert!(
            Tag::ALL
                .iter()
                .all(|t| Tag::from_name(t.name()) == Some(*t))
        );
    }
}
