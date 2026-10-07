//! The full TIR (checking-and-tir.md §4.13.11; data-structures.md §3.18):
//! the instruction catalog with its operand schema, the body columns, the
//! `TirSink` builder API, a text printer, a text parser that round-trips,
//! and the verifier.
//!
//! Constants: `Ref::konst(i)` names row `i` of the body's own constant
//! column (`consts`), so a body and its TIR hash need no run-wide table.

use std::fmt::Write as _;

use hd_base::{CaptureId, DefId, LabelId, LocalId, NodeIdx, Range32, SubId, Symbol};
use hd_types::{Ty, TyList};

pub const NONE: u32 = u32::MAX;

/// An instruction index in its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Inst(pub u32);

/// A value: bit 31 clear, an instruction's value; set, a global pool
/// constant (bits 0..30).
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
    NewTuple: None, Values;
    NewList: None, Record;
    NewMap: None, Values;
    Field: Value, Meta;
    FieldSet: Value, Record;
    TupleGet: Value, Meta;
    Closure: Meta, Record;
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
}

impl PrimOp {
    pub const ALL: [PrimOp; 21] = [
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
}

impl IntrinsicOp {
    pub const ALL: [IntrinsicOp; 15] = [
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
    ];
    #[must_use]
    pub fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.get(v as usize).copied()
    }
}

/// Coercion kinds (§4.13.11 "Coercion kinds"; type-checking.md §4.2).
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CaptureMode {
    Copy,
    Move,
    Shared,
}

/// Local flags (u8): READ | ASSIGNED | MUTATED | CAPTURED | `CAPTURED_ASSIGNED` | PARAM.
pub mod local_flags {
    pub const READ: u8 = 1;
    pub const ASSIGNED: u8 = 2;
    pub const MUTATED: u8 = 4;
    pub const CAPTURED: u8 = 8;
    pub const CAPTURED_ASSIGNED: u8 = 16;
    pub const PARAM: u8 = 32;
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
        }
    }

    /// A record in `extra`: its words after the length word.
    #[must_use]
    pub fn record(&self, at: u32) -> &[u32] {
        let n = self.extra[at as usize] as usize;
        &self.extra[at as usize + 1..at as usize + 1 + n]
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
}

#[derive(Clone, Copy, Debug)]
pub struct BlockMark(u32);
#[derive(Clone, Copy, Debug)]
pub struct ScopeMark(u32);
#[derive(Clone, Copy, Debug)]
pub struct LoopMark(pub LabelId);
#[derive(Clone, Copy, Debug)]
pub struct Slot(u32);
#[derive(Clone, Copy, Debug)]
pub struct SubMark(SubId, u32);

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
        mut self,
        root: Ref,
        capture_modes: &[CaptureMode],
    ) -> Result<Body, Vec<VerifyError>> {
        self.body.sub_root[0] = root.0;
        self.body.cap_mode = capture_modes.to_vec();
        self.body
            .cap_mode
            .resize(self.body.cap_local.len(), CaptureMode::Shared);
        let errs = verify(&self.body);
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
    /// The scope's suites are the `Defer`s emitted since `open_scope`.
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
        let mine: Vec<LocalId> = self
            .open_caps
            .drain(m.1 as usize..)
            .filter(|c| c.0 == m.0)
            .map(|c| c.1)
            .collect();
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

/// Invariants this verifier checks; the others (4, 6 to 9, 11 to 13, 15,
/// 17) need types and the item table and are not implemented yet.
pub const CHECKED_INVARIANTS: &[u8] = &[1, 2, 5, 10, 14];

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
                        if v & Ref::CONST_BIT == 0 && v >= i && tag != Tag::Call {
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
    errs
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
            "str" => {
                let hex = f.get(1).copied().unwrap_or(".").trim_end_matches('.');
                let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
                    .collect();
                let text = bytes
                    .and_then(|v| String::from_utf8(v).ok())
                    .ok_or_else(|| e("bad string".into()))?;
                b.strings.push(text.into());
            }
            "extra" => b.extra = nums(1)?,
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
        BodyKind, Callee, CaptureMode, Coercion, Providers, Ref, Tag, TirBuilder, TirSink, parse,
        print, verify,
    };
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    fn sample() -> super::Body {
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let n = NodeIdx::from_raw(1);
        let blk = b.open_block();
        let x = b.local(Ty::I32, Symbol::from_raw(0), super::local_flags::PARAM, n);
        let one = b.konst(3);
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
        b.rollback(cp);
        let root = b.close_block(blk, Some(Ref::konst(1)), Ty::I32, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("ok");
        assert_eq!(body.len(), 1);
        assert_eq!(Tag::ALL.len(), 59);
        assert!(
            Tag::ALL
                .iter()
                .all(|t| Tag::from_name(t.name()) == Some(*t))
        );
    }
}
