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

mod owned;
mod root;
pub use owned::verify_owned;
pub use root::{RootTables, root_places};

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
    Interp: None, Record;
    Coerce: Value, Record;
    // suspension and hooks
    Await: Record, Values;
    AwaitValue: Value, None;
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
    // is a place word. `FlagSet` and `FlagClear` are reserved for tier 2.
    Drop: Place, None;
    IsUnique: Place, Meta;
    Pin: Place, None;
    FlagSet: Place, None;
    FlagClear: Place, None;
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
    Return: Value, None;
    Unreachable: None, None;
    // matching
    Match: Value, Blocks;
    SwitchTag: Value, Record;
    SwitchInt: Value, Record;
    SwitchChar: Value, Record;
    SwitchStr: Value, Record;
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
    MapIndex,
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
    /// `(map, slot) -> (K, V)`: the entry in a live slot.
    MapSlotEntry,
    /// `(value) -> value`: a copy of a value that holds a `List` or a
    /// `Map` (spec/lang/04-type-system.md, "Collection Values"). Where
    /// the type holds none, the copy is the value itself. A collection's
    /// copy shares its counted buffer and copies it before its next change
    /// when another owner still holds it (`crates/hd_wasm_linear/DESIGN.md`
    /// §23.20, "Collections").
    ValueCopy,
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
    /// `(map, key) -> usize`: the number of the key's entry, a panic
    /// `index-out-of-bounds` when the map holds no equal key. A map value
    /// is a place, `[%m [%e]]` at that number (`ref.elem.map`,
    /// `types.coll.map-value.place`): the checker looks the entry up just
    /// before each read, store or reference of the place, as a list's
    /// place checks its index there.
    MapEntry,
}

impl IntrinsicOp {
    pub const ALL: [IntrinsicOp; 19] = [
        IntrinsicOp::ListLen,
        IntrinsicOp::ListPush,
        IntrinsicOp::MapLen,
        IntrinsicOp::MapGet,
        IntrinsicOp::MapRemove,
        IntrinsicOp::MapIndex,
        IntrinsicOp::MapSet,
        IntrinsicOp::StrIndex,
        IntrinsicOp::StrConcat,
        IntrinsicOp::StrEq,
        IntrinsicOp::Item,
        IntrinsicOp::MapSlots,
        IntrinsicOp::MapSlotNext,
        IntrinsicOp::MapSlotEntry,
        IntrinsicOp::ValueCopy,
        IntrinsicOp::MapIndexMut,
        IntrinsicOp::SnapBegin,
        IntrinsicOp::SnapEnd,
        IntrinsicOp::MapEntry,
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
    /// method (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Trait value references").
    RefToTraitValue,
    /// A trait value reference `&dyn K` or `&mut dyn K` installed as the
    /// provider of a key `K` whose trait has a `&self` or `&mut self`
    /// method: the provider is the trait value `K` whose payload is the
    /// reference itself (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Providers of
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
    /// A projection word's kind, its top two bits.
    pub const PROJ_KIND: u32 = 3 << 30;
    /// A projection word's payload, bits 0 to 29.
    pub const PROJ_INDEX: u32 = (1 << 30) - 1;
    /// A struct field or tuple element, by declaration index.
    pub const PROJ_FIELD: u32 = 0;
    /// A dereference; no payload.
    pub const PROJ_DEREF: u32 = 1 << 30;
    /// A list element; no payload, and the next word is the index `Ref`.
    pub const PROJ_ELEM: u32 = 2 << 30;
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
}

/// One step of a place, read left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proj {
    /// A struct field or tuple element, by declaration index: `.k`.
    Field(u32),
    /// The place a reference value refers to, or the payload of a
    /// compiler-supplied data object: `*`.
    Deref,
    /// A slot of a list's buffer at an index value, bounds-checked: `[%n]`.
    /// In stages 1 and 2 the projected value is the list header.
    Elem(Ref),
    /// The payload of an enum value known to be that variant, by
    /// declaration index: `as .k` (stage 2).
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
            ROOT_LOCAL, ROOT_VALUE,
        };
        let mut w = match self.root {
            PlaceRoot::Local(_) if self.proj.is_empty() => return None,
            PlaceRoot::Local(l) => vec![ROOT_LOCAL, l.raw()],
            PlaceRoot::Global(d, t) => vec![ROOT_GLOBAL, d.raw(), t.0],
            PlaceRoot::Value(r) => vec![ROOT_VALUE, r.0],
            PlaceRoot::Frame(s) => vec![ROOT_FRAME, s],
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
            PlaceRoot::Value(r) => Some(r.0),
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
    /// names, as a dereference is what a reference names. A value root
    /// is never written in place.
    #[must_use]
    pub fn writable(&self) -> bool {
        matches!(self.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            || self
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
        let first = toks.next().ok_or_else(bad)?;
        let root_text = first.trim_start_matches('*');
        let lead = first.len() - root_text.len();
        let root = match root_text.as_bytes().first() {
            Some(b'$') => PlaceRoot::Local(LocalId::from_raw(num(&root_text[1..])?)),
            Some(b'@') => {
                let (d, t) = root_text[1..].split_once(':').ok_or_else(bad)?;
                PlaceRoot::Global(DefId::from_raw(num(d)?), Ty(num(t)?))
            }
            Some(b'%') => PlaceRoot::Value(value(root_text)?),
            Some(b'^') => PlaceRoot::Frame(num(&root_text[1..])?),
            _ => return Err(bad()),
        };
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
    /// `[*%v .k]`: field `k` of what the value names: a data object's
    /// payload, or a reference's referent.
    DerefField(u32),
    /// `[%v as .V .k]`: field `k` of variant `V`'s payload.
    Variant(u32, u32),
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
    /// Set by the borrow pass: a `Ref` of a field of what a lent parameter
    /// refers to (`[*$p .f]`, or through a borrowed read of all of `$p`),
    /// whose referent the caller keeps alive for the whole call. It takes
    /// no count of its own; its `Drop`s stay for an instance whose
    /// reference view makes the referent fresh, and release only that.
    /// The checker never sets it (invariant 22).
    pub const LENT: u32 = 4;
}

/// An `IsUnique`'s data word b (`docs/compiler/tir-v2.md`, "Hoisting"): 0
/// for one that stands at its change, as insertion writes it.
pub mod unique_word {
    /// Hoisted out of the loop that follows it in its block: it runs only
    /// when that loop's entry test lets the loop in, once before it.
    pub const HOISTED: u32 = 1;
    /// Inside a loop that a `HOISTED` `IsUnique` of the same place
    /// precedes, which makes it unneeded where no dereference on its place
    /// makes a fresh value (no reference view is reachable); kept for the
    /// instances where one may.
    pub const COVERED: u32 = 2;
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
    /// The ownership passes' exit set (`docs/compiler/tir-v2.md`,
    /// "Ownership statements"): per row, a sub-body and an owned local of
    /// it that may still hold a value at the sub-body's one exit, which
    /// drops it. Sorted, each row once. A pass output: empty in the
    /// checker's TIR (invariant 22), never written to the wire.
    pub exit_drops: Vec<(SubId, LocalId)>,
    /// The ownership passes' unwind set (`docs/compiler/tir-v2.md`,
    /// "Known-null"): per row, a suspension point and an owned value that
    /// may still hold a count there, which the point's cancellation, a way
    /// out the body has no jump for, releases. The exit releases the exit
    /// set on that way out too, so the locals need no rows. Sorted, each
    /// row once. A pass output: empty in the checker's TIR (invariant 22),
    /// never written to the wire.
    pub unwind_drops: Vec<(u32, u32)>,
    /// Known-null's rule 5 (`docs/compiler/tir-v2.md`, "Known-null"): per
    /// row, an instruction and an addressed local (one that a `Ref` of its
    /// own storage or a shared capture puts in a cell) that certainly holds
    /// its value where the instruction runs, so the backend reads its cell
    /// with no null test. The instruction is a `Read`, `Store`, `Ref`,
    /// `IsUnique` or `Pin` of a place rooted at the local, a `Closure` that
    /// captures it, or a `Loop`. Sorted, each row once. A pass output: empty
    /// in the checker's TIR (invariant 22), never written to the wire.
    pub full_cells: Vec<(u32, u32)>,
    /// The hoisting pass's stable list places (`docs/compiler/tir-v2.md`,
    /// "Hoisting"): per row, a `Loop` and a place word of a list whose
    /// header and buffer nothing in the loop replaces, so the backend may
    /// keep the buffer's address and length in locals for the loop. The
    /// place is rooted at a local, with fields and dereferences only.
    /// Sorted, each row once. A pass output: empty in the checker's TIR
    /// (invariant 22), never written to the wire.
    pub stable_lists: Vec<(u32, u32)>,
    /// Lifetime regions of borrows, per row an instruction and its region,
    /// sorted (`docs/compiler/tir-v2.md`, "Encoding, wire format and
    /// caching"): reserved for tier 2, so empty in tier 1 (invariants 22
    /// and 23).
    pub regions: Vec<(u32, u32)>,
    /// Per instruction of a body the ownership passes made, the checker's
    /// instruction it came from, which a call it writes names as its
    /// anchor in the line table (`crates/hd_wasm_linear/DESIGN.md` §15.9),
    /// since the anchor is resolved against the `check` entry's body; `NONE`
    /// for an instruction the passes added. Empty in the checker's TIR,
    /// where each instruction is its own ([`Body::anchor`]). The driver
    /// fills it before the passes and every rebuild carries it; never
    /// written to the wire.
    pub anchors: Vec<u32>,
    /// The inliner's copies, each `[first, len, callee, call]`, sorted:
    /// an anchor `INLINED | a` with `first <= a < first + len` names
    /// instruction `a - first` of the callee's checker body, which the
    /// copy's call at anchor `call` ran ([`Body::inlined_frame`]). Never
    /// written to the wire.
    pub inlined: Vec<[u32; 4]>,
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
            exit_drops: vec![],
            unwind_drops: vec![],
            full_cells: vec![],
            stable_lists: vec![],
            regions: vec![],
            anchors: vec![],
            inlined: vec![],
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
        Some((DefId::from_raw(callee), a - first, call))
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
    /// For `Read`, `Store`, `Ref`, `IsUnique` and `Pin`, the values their
    /// place reads (a
    /// value root, then each element index), then a `Store`'s value; a
    /// local or global root is no value and is not listed.
    #[must_use]
    pub fn value_operands(&self, i: usize) -> Option<Vec<u32>> {
        let [a, w] = self.data[i];
        let one = |v: u32| if v == NONE { vec![] } else { vec![v] };
        Some(match self.tags[i] {
            Tag::Prim | Tag::NewStruct | Tag::NewVariant | Tag::NewMap | Tag::DefaultCall => {
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
            Tag::Coerce => one(a),
            Tag::CopyStruct => {
                let pairs = self.record(w).iter().skip(1).step_by(2).copied();
                one(a).into_iter().chain(pairs).collect()
            }
            Tag::NewTuple => [self.record(w).to_vec(), one(a)].concat(),
            Tag::NewList if a == NONE => self.record(w).to_vec(),
            Tag::ProviderGet | Tag::ItemRef => vec![],
            Tag::Read | Tag::Ref | Tag::IsUnique | Tag::Pin => self.place_values(a),
            Tag::Store => [self.place_values(a), one(w)].concat(),
            _ => return None,
        })
    }

    /// Decodes a place word ([`place`]); `None` when it is malformed: a
    /// record out of range, an unknown root kind, a projection with stray
    /// payload bits, or an element with no index word.
    #[must_use]
    pub fn decode_place(&self, word: u32) -> Option<Place> {
        use place::{
            BARE_LOCAL, PROJ_DEREF, PROJ_ELEM, PROJ_FIELD, PROJ_INDEX, PROJ_KIND, PROJ_VARIANT,
            ROOT_FRAME, ROOT_GLOBAL, ROOT_LOCAL, ROOT_VALUE,
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
            _ => return None,
        };
        let mut proj = Vec::with_capacity(rest.len());
        while let [w, more @ ..] = rest {
            rest = more;
            proj.push(match (w & PROJ_KIND, w & PROJ_INDEX) {
                (PROJ_FIELD, k) => Proj::Field(k),
                (PROJ_DEREF, 0) => Proj::Deref,
                (PROJ_ELEM, 0) => {
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

    /// The values place word `word` reads ([`Place::values`]); none when it
    /// is malformed.
    #[must_use]
    pub fn place_values(&self, word: u32) -> Vec<u32> {
        self.decode_place(word)
            .map(|p| p.values())
            .unwrap_or_default()
    }

    /// The local `Store` `i` writes in place, and whether it replaces all
    /// of it: a place rooted at a local whose projections are all struct
    /// fields (`[$l]`, `[$l .x .y]`). `None` for any other instruction or
    /// place.
    #[must_use]
    pub fn stored_local(&self, i: usize) -> Option<(LocalId, bool)> {
        if self.tags[i] != Tag::Store {
            return None;
        }
        let p = self.decode_place(self.data[i][0])?;
        match p.root {
            PlaceRoot::Local(l) if p.proj.iter().all(|s| matches!(s, Proj::Field(_))) => {
                Some((l, p.proj.is_empty()))
            }
            _ => None,
        }
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
        if word & place::BARE_LOCAL != 0 {
            return Some(LocalId::from_raw(word & !place::BARE_LOCAL));
        }
        let at = word as usize;
        let n = *self.extra.get(at)? as usize;
        let rec = self.extra.get(at + 1..at + 1 + n)?;
        let [place::ROOT_LOCAL, l, steps @ ..] = rec else {
            return None;
        };
        steps
            .iter()
            .all(|w| {
                matches!(
                    w & place::PROJ_KIND,
                    place::PROJ_FIELD | place::PROJ_VARIANT
                )
            })
            .then(|| LocalId::from_raw(*l))
    }

    /// The local a plain local read `i` reads: a `Read` of a bare local,
    /// `[$n]`, in any mode. `None` for any other instruction.
    #[must_use]
    pub fn local_read(&self, i: usize) -> Option<LocalId> {
        let word = self.data[i][0];
        (self.tags[i] == Tag::Read && word & place::BARE_LOCAL != 0)
            .then(|| LocalId::from_raw(word & !place::BARE_LOCAL))
    }

    /// The value projection read `i` projects from and its one step: a
    /// `Read` of a value root followed by one field, a dereference and one
    /// field, or a variant and one of its fields. These are the reads of a
    /// struct field or tuple element (`[%v .k]`), of a data object's field
    /// or a referent's (`[*%v .k]`), and of a payload (`[%v as .V .k]`).
    /// It decodes the record in place. `None` for any other instruction or
    /// place.
    #[must_use]
    pub fn read_step(&self, i: usize) -> Option<(u32, Step)> {
        use place::{
            BARE_LOCAL, PROJ_DEREF, PROJ_FIELD, PROJ_INDEX, PROJ_KIND, PROJ_VARIANT, ROOT_VALUE,
        };
        let word = self.data[i][0];
        if self.tags[i] != Tag::Read || word & BARE_LOCAL != 0 {
            return None;
        }
        let at = word as usize;
        let field = |w: u32| (w & PROJ_KIND == PROJ_FIELD).then_some(w & PROJ_INDEX);
        match *self.extra.get(at..)? {
            [3, ROOT_VALUE, v, w, ..] => Some((v, Step::Field(field(w)?))),
            [4, ROOT_VALUE, v, PROJ_DEREF, w, ..] => Some((v, Step::DerefField(field(w)?))),
            [4, ROOT_VALUE, v, x, w, ..] if x & PROJ_KIND == PROJ_VARIANT => {
                Some((v, Step::Variant(x & PROJ_INDEX, field(w)?)))
            }
            _ => None,
        }
    }

    /// The list value and the index value element read `i` reads: a
    /// `Read` of a value root with one element projection, `[%l [%i]]`,
    /// a slot of the buffer of the list header `%l`. It decodes the record
    /// in place. `None` for any other instruction or place.
    #[must_use]
    pub fn element_read(&self, i: usize) -> Option<(u32, u32)> {
        let word = self.data[i][0];
        if self.tags[i] != Tag::Read || word & place::BARE_LOCAL != 0 {
            return None;
        }
        match *self.extra.get(word as usize..)? {
            [4, place::ROOT_VALUE, list, place::PROJ_ELEM, index, ..] => Some((list, index)),
            _ => None,
        }
    }

    /// The value a copy of a whole value reads: a `Read` of a value root
    /// with no projection, `[%v]`, which the ownership passes write for a
    /// consumer of a value used more than once. `None` for any other
    /// instruction or place.
    #[must_use]
    pub fn value_read(&self, i: usize) -> Option<u32> {
        let word = self.data[i][0];
        if self.tags[i] != Tag::Read || word & place::BARE_LOCAL != 0 {
            return None;
        }
        match *self.extra.get(word as usize..word as usize + 3)? {
            [2, place::ROOT_VALUE, v] => Some(v),
            _ => None,
        }
    }

    /// The top-level binding a plain global read `i` reads: a `Read` of
    /// a global root with no projection, `[@g]`. `None` for any other
    /// instruction.
    #[must_use]
    pub fn global_read(&self, i: usize) -> Option<DefId> {
        let word = self.data[i][0];
        if self.tags[i] != Tag::Read || word & place::BARE_LOCAL != 0 {
            return None;
        }
        match self.extra.get(word as usize..word as usize + 4)? {
            [3, place::ROOT_GLOBAL, d, _] => Some(DefId::from_raw(*d)),
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

    /// `Read [*%base .k]`: field `k` of the data object `base` names,
    /// mode `use`.
    pub fn data_field(&mut self, base: Ref, k: u32, ty: Ty, syn: NodeIdx) -> Ref {
        let word = self.record(&[
            place::ROOT_VALUE,
            base.0,
            place::PROJ_DEREF,
            place::PROJ_FIELD | k,
        ]);
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
/// the checker's facts ([`VerifyCx`], [`verify_in`]), 3's `Hole` and
/// `Poison` half the module's errors ([`poison_sites`]), and 23 holds
/// after the ownership passes ([`verify_owned`]) where 22 holds before.
pub const CHECKED_INVARIANTS: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17, 20, 21, 22, 23,
];

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
            _ => {}
        }
    }
    // A global root's type; instructions may share a place record, which
    // is mapped once.
    for at in global_place_types(b) {
        b.extra[at] = ty(Ty(b.extra[at])).0;
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
    out.sort_unstable();
    out.dedup();
    out
}

/// The blocks instruction `i` owns through its operands: a block
/// operand, a block list, a switch's two arms, or a `Defer`'s suite.
pub(crate) fn owned_blocks(b: &Body, i: usize) -> Vec<u32> {
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

/// Verifies the checker's TIR of a body (§4.13.11 "Invariants"): the
/// structural invariants, and 22, that it has no ownership yet. A body
/// after the ownership passes is verified with [`verify_owned`].
#[must_use]
pub fn verify(b: &Body) -> Vec<VerifyError> {
    let mut errs = verify_structure(b);
    no_ownership(b, &mut errs);
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
            Tag::Drop | Tag::IsUnique | Tag::Pin | Tag::FlagSet | Tag::FlagClear => {
                format!("an ownership statement `{}` before the passes", tag.name())
            }
            Tag::Ref if w & ref_word::TAKES != 0 => {
                "a `Ref` that takes its local's count before the passes".to_owned()
            }
            Tag::Ref if w & ref_word::LENT != 0 => "a lent `Ref` before the passes".to_owned(),
            _ => continue,
        };
        errs.push(VerifyError {
            invariant: 22,
            inst: u32::try_from(i).expect("i"),
            what,
        });
    }
    if !b.exit_drops.is_empty()
        || !b.unwind_drops.is_empty()
        || !b.full_cells.is_empty()
        || !b.stable_lists.is_empty()
        || !b.regions.is_empty()
        || !b.anchors.is_empty()
    {
        errs.push(VerifyError {
            invariant: 22,
            inst: NONE,
            what: format!(
                "{} exit drop, {} unwind drop, {} full cell, {} stable list, {} region and {} anchor rows before the ownership passes",
                b.exit_drops.len(),
                b.unwind_drops.len(),
                b.full_cells.len(),
                b.stable_lists.len(),
                b.regions.len(),
                b.anchors.len()
            ),
        });
    }
}

/// The invariants of every body, before or after the ownership passes:
/// all of [`CHECKED_INVARIANTS`] but 22 and 23.
pub(crate) fn verify_structure(b: &Body) -> Vec<VerifyError> {
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
                Op::Place => errs.extend(place_errors(b, i, word)),
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

/// Invariant 20: place word `word` of instruction `user` is well formed.
/// It decodes; a local root names a local of the body; a value root and
/// each element index precede their user (of invariant 1, def-before-use
/// only); a frame root occurs only in tier 2, so never yet.
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
                Op::Place => vals.extend(b.place_values(word)),
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
    for (sub, local) in &b.exit_drops {
        let _ = writeln!(o, "exitdrop {} {}", sub.raw(), local.raw());
    }
    for (inst, value) in &b.unwind_drops {
        let _ = writeln!(o, "unwinddrop {inst} {value}");
    }
    for (inst, local) in &b.full_cells {
        let _ = writeln!(o, "fullcell {inst} {local}");
    }
    for (inst, word) in &b.stable_lists {
        let _ = writeln!(o, "stablelist {inst} {word}");
    }
    for (inst, region) in &b.regions {
        let _ = writeln!(o, "region {inst} {region}");
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
            "exitdrop" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("exitdrop needs 2 fields".into()));
                }
                b.exit_drops
                    .push((SubId::from_raw(v[0]), LocalId::from_raw(v[1])));
            }
            "unwinddrop" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("unwinddrop needs 2 fields".into()));
                }
                b.unwind_drops.push((v[0], v[1]));
            }
            "fullcell" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("fullcell needs 2 fields".into()));
                }
                b.full_cells.push((v[0], v[1]));
            }
            "stablelist" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("stablelist needs 2 fields".into()));
                }
                b.stable_lists.push((v[0], v[1]));
            }
            "region" => {
                let v = nums(1)?;
                if v.len() != 2 {
                    return Err(e("region needs 2 fields".into()));
                }
                b.regions.push((v[0], v[1]));
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
        for bad in ["$0", "[]", "[$x]", "[$0 .]", "[$0 as]", "[$0 [5]]", "[#1]"] {
            assert!(bad.parse::<Place>().is_err(), "{bad}");
        }
    }

    #[test]
    fn malformed_place_records_do_not_decode() {
        use super::place::{PROJ_DEREF, PROJ_ELEM};
        let mut body = super::Body::new(DefId::from_raw(1), BodyKind::Fn);
        for (k, rec) in [
            vec![2, 4, 0],                 // root kind 4
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

    /// A struct field, a data object's field and a payload field are reads
    /// of one projection step from a value, which `read_step` names.
    #[test]
    fn projection_reads_name_their_step() {
        use super::Step;
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let v = b.const_value(Ty::I32, 7);
        let f = b.field(v, 2, Ty::I32, n);
        let d = b.data_field(f, 1, Ty::I32, n);
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

    impl crate::own::OwnCx for Counts {
        fn counted(&self, ty: Ty) -> crate::own::Tri {
            if ty == Ty::STRING {
                crate::own::Tri::Yes
            } else {
                crate::own::Tri::No
            }
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn may_write(&self, _: &Callee) -> bool {
            true
        }
        fn parts(&self, _: Ty, _: Option<u32>, out: &mut Vec<Ty>) -> bool {
            out.clear();
            false
        }
        fn received(&self, _: hd_base::DefId, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn header_copy(&self, _: Ty) -> Option<crate::own::HeaderCopy> {
            None
        }
        fn in_place_view(&self, _: Ty) -> bool {
            false
        }
        fn receiver(&self, _: &Callee, _: usize, _: usize) -> bool {
            false
        }
    }

    /// Invariant 22: the checker's TIR has no ownership: no decided read
    /// mode, no ownership statement, no exit set, unwind set or region row.
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
        rows.unwind_drops.push((0, 0));
        assert_eq!(findings(&verify(&rows)), [(22, NONE)]);
    }

    /// The new statements and columns print and parse back.
    #[test]
    fn ownership_statements_and_columns_round_trip() {
        use super::ReadMode;
        let mut body = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Borrow, n);
            statement(b, Tag::IsUnique, "[$1]", n);
            statement(b, Tag::Pin, "[$1]", n);
            statement(b, Tag::Drop, "[$0]", n);
        });
        body.exit_drops
            .push((SubId::from_raw(0), LocalId::from_raw(1)));
        body.unwind_drops.push((2, 0));
        body.regions.push((0, 3));
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

    /// Invariant 23: nothing tier 2 reserves, a decided mode for every
    /// read, and an exit set in order.
    #[test]
    fn verifier_rejects_tier_two_and_undecided_ownership() {
        use super::{ReadMode, verify_owned};
        let mut body = owned_body(|b, n| {
            read(b, "[$0]", ReadMode::Use, n);
            statement(b, Tag::FlagSet, "[$0]", n);
        });
        body.regions.push((0, 0));
        body.exit_drops = vec![
            (SubId::from_raw(0), LocalId::from_raw(1)),
            (SubId::from_raw(0), LocalId::from_raw(0)),
        ];
        assert_eq!(
            findings(&verify_owned(&body, &Counts)),
            [(23, 0), (23, 1), (23, NONE), (23, NONE)]
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
        assert_eq!(Tag::ALL.len(), 57);
        assert!(
            Tag::ALL
                .iter()
                .all(|t| Tag::from_name(t.name()) == Some(*t))
        );
    }
}
