//! `hd_wasm`: Wasm GC emission (codegen.md §12, wasm-layout.md §15) and link
//! (§13.10). `Emit` writes a code entry per instance with symbolic
//! relocations (callees by instance key, struct types by stable path); `Link`
//! assigns indices. It reads TIR and collection output only, never syntax.

use std::collections::{BTreeMap, HashMap};

use hd_base::Hash128;
use wasm_encoder::{
    BlockType, CodeSection, CompositeInnerType, CompositeType, EntityType, ExportKind,
    ExportSection, FieldType, FunctionSection, HeapType, ImportSection, Module, RefType,
    StorageType, StructType, SubType, TypeSection, ValType,
};

use hd_iface::{Reader, put_hash, put_str, put_u32};
use hd_mono::{Instance, classify, instance_key, resolve_method};
use hd_tir::world::{DefId, DefKind, Ty, TyKind, World};
use hd_tir::{CALLEE_ITEM, CONST_BIT, LOCAL_PARAM, NONE, PrimOp, TirBody, TirTag};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VT {
    I32,
    Ref(String),
    /// `eqref`: the erased layout of the `REF` class (wasm-layout.md §15.1).
    Eq,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum W {
    I32(i32),
    LGet(u32),
    LSet(u32),
    Call(Hash128),
    CallImport(u32),
    StructNew(String),
    StructGet(String, u32, bool),
    Prim(u32),
    If,
    Else,
    End,
    Block,
    Loop,
    Br(u32),
    Return,
    Unreachable,
    /// `ref.cast (ref $T)`: the reader of an erased value casts at first use.
    Cast(String),
}

/// A relocation target (codegen.md §13.8 `Reloc`, the kinds the subset uses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reloc {
    Func(Hash128),
    Import(u32),
    Type(String),
}

/// A code entry (codegen.md §13.8): locals and instructions as bytes, index
/// immediates as 5-byte padded LEBs, with relocations sorted by offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub params: Vec<VT>,
    pub results: Vec<VT>,
    pub body: Vec<u8>,
    pub relocs: Vec<(u32, Reloc)>,
}

impl VT {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            VT::I32 => out.push(0),
            VT::Eq => out.push(1),
            VT::Ref(p) => {
                out.push(2);
                put_str(out, p);
            }
        }
    }
    fn decode(r: &mut Reader<'_>) -> VT {
        match r.u8() {
            0 => VT::I32,
            1 => VT::Eq,
            _ => VT::Ref(r.str()),
        }
    }
}

impl Code {
    /// The code entry's bytes (codegen.md §13.8): ID-free, so a store keeps
    /// them as plain bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for list in [&self.params, &self.results] {
            put_u32(&mut out, u32::try_from(list.len()).expect("n"));
            for v in list {
                v.encode(&mut out);
            }
        }
        put_u32(&mut out, u32::try_from(self.body.len()).expect("n"));
        out.extend_from_slice(&self.body);
        put_u32(&mut out, u32::try_from(self.relocs.len()).expect("n"));
        for (at, r) in &self.relocs {
            put_u32(&mut out, *at);
            match r {
                Reloc::Func(k) => {
                    out.push(0);
                    put_hash(&mut out, *k);
                }
                Reloc::Import(i) => {
                    out.push(1);
                    put_u32(&mut out, *i);
                }
                Reloc::Type(p) => {
                    out.push(2);
                    put_str(&mut out, p);
                }
            }
        }
        out
    }
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Code {
        let mut r = Reader::new(bytes);
        let n = r.u32();
        let params = (0..n).map(|_| VT::decode(&mut r)).collect();
        let n = r.u32();
        let results = (0..n).map(|_| VT::decode(&mut r)).collect();
        let n = r.u32() as usize;
        let body = r.bytes[r.pos..r.pos + n].to_vec();
        r.pos += n;
        let n = r.u32();
        let relocs = (0..n)
            .map(|_| {
                let at = r.u32();
                let reloc = match r.u8() {
                    0 => Reloc::Func(r.hash()),
                    1 => Reloc::Import(r.u32()),
                    _ => Reloc::Type(r.str()),
                };
                (at, reloc)
            })
            .collect();
        Code { params, results, body, relocs }
    }
}

fn padded(out: &mut Vec<u8>, v: u32) {
    for k in 0..4 {
        out.push(u8::try_from((v >> (7 * k)) & 0x7f).expect("7 bits") | 0x80);
    }
    out.push(u8::try_from((v >> 28) & 0x7f).expect("7 bits"));
}
fn slot(out: &mut Vec<u8>, relocs: &mut Vec<(u32, Reloc)>, r: Reloc) {
    relocs.push((u32::try_from(out.len()).expect("offset"), r));
    padded(out, 0);
}
fn val_bytes(out: &mut Vec<u8>, relocs: &mut Vec<(u32, Reloc)>, vt: &VT) {
    match vt {
        VT::I32 => out.push(0x7f),
        VT::Eq => out.push(0x6d),
        VT::Ref(p) => {
            out.push(0x64);
            slot(out, relocs, Reloc::Type(p.clone()));
        }
    }
}

/// Encodes a body: plain instructions through `wasm-encoder`, relocated
/// immediates as raw opcode bytes plus a padded slot.
fn encode(locals: &[VT], ws: &[W]) -> (Vec<u8>, Vec<(u32, Reloc)>) {
    let mut out = Vec::new();
    let mut relocs = Vec::new();
    let n = u32::try_from(locals.len()).expect("locals");
    wasm_encoder::Encode::encode(&n, &mut out);
    for l in locals {
        out.push(1);
        val_bytes(&mut out, &mut relocs, l);
    }
    for w in ws {
        match w {
            W::Call(k) => {
                out.push(0x10);
                slot(&mut out, &mut relocs, Reloc::Func(*k));
            }
            W::CallImport(i) => {
                out.push(0x10);
                slot(&mut out, &mut relocs, Reloc::Import(*i));
            }
            W::StructNew(p) => {
                out.extend_from_slice(&[0xfb, 0x00]);
                slot(&mut out, &mut relocs, Reloc::Type(p.clone()));
            }
            W::StructGet(p, f, packed) => {
                out.extend_from_slice(&[0xfb, if *packed { 0x04 } else { 0x02 }]);
                slot(&mut out, &mut relocs, Reloc::Type(p.clone()));
                wasm_encoder::Encode::encode(f, &mut out);
            }
            W::Cast(p) => {
                out.extend_from_slice(&[0xfb, 0x16]);
                slot(&mut out, &mut relocs, Reloc::Type(p.clone()));
            }
            other => {
                let mut s = wasm_encoder::InstructionSink::new(&mut out);
                match other {
                    W::I32(v) => s.i32_const(*v),
                    W::LGet(l) => s.local_get(*l),
                    W::LSet(l) => s.local_set(*l),
                    W::Prim(op) => match PrimOp::from_u32(*op) {
                        PrimOp::Add => s.i32_add(),
                        PrimOp::Sub | PrimOp::Neg => s.i32_sub(),
                        PrimOp::Mul => s.i32_mul(),
                        PrimOp::Div => s.i32_div_s(),
                        PrimOp::Rem => s.i32_rem_s(),
                        PrimOp::Eq => s.i32_eq(),
                        PrimOp::Ne => s.i32_ne(),
                        PrimOp::Lt => s.i32_lt_s(),
                        PrimOp::Le => s.i32_le_s(),
                        PrimOp::Gt => s.i32_gt_s(),
                        PrimOp::Ge => s.i32_ge_s(),
                        PrimOp::And => s.i32_and(),
                        PrimOp::Or => s.i32_or(),
                    },
                    W::If => s.if_(BlockType::Empty),
                    W::Else => s.else_(),
                    W::End => s.end(),
                    W::Block => s.block(BlockType::Empty),
                    W::Loop => s.loop_(BlockType::Empty),
                    W::Br(d) => s.br(*d),
                    W::Return => s.return_(),
                    W::Unreachable => s.unreachable(),
                    _ => unreachable!(),
                };
            }
        }
    }
    (out, relocs)
}

/// The layout of a type (wasm-layout.md §15.1): `None` for `void`/`never`.
fn layout(w: &World, t: Ty) -> Option<VT> {
    match w.kind(t) {
        TyKind::Void | TyKind::Never => None,
        TyKind::Bool | TyKind::I32 => Some(VT::I32),
        TyKind::Adt(d) => Some(VT::Ref(w.path(*d).to_owned())),
        TyKind::ClassRef => Some(VT::Eq),
        TyKind::Param(_) | TyKind::SelfTy => panic!("layout of an unsubstituted type"),
    }
}

struct Em<'a> {
    w: &'a mut World,
    bodies: &'a HashMap<DefId, TirBody>,
    b: &'a TirBody,
    args: &'a [Ty],
    out: Vec<W>,
    locals: Vec<VT>,
    nparams: u32,
    temps: HashMap<u32, u32>,
    local_map: Vec<u32>,
    /// Control stack: `Some(label)` for a loop's outer break block.
    ctrl: Vec<Option<u32>>,
}

impl Em<'_> {
    fn sub(&mut self, t: Ty) -> Ty {
        self.w.subst(t, self.args, None)
    }
    fn new_local(&mut self, vt: VT) -> u32 {
        self.locals.push(vt);
        self.nparams + u32::try_from(self.locals.len() - 1).expect("locals")
    }
    fn load(&mut self, r: u32) {
        if r == NONE {
            return;
        }
        if r & CONST_BIT != 0 {
            let (_, bits) = self.w.const_value(r & !CONST_BIT);
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            self.out.push(W::I32(bits as u32 as i32));
        } else if let Some(&l) = self.temps.get(&r) {
            self.out.push(W::LGet(l));
        }
    }
    fn store(&mut self, i: u32) {
        let t = self.sub(self.b.ty[i as usize]);
        if let Some(vt) = layout(self.w, t) {
            let l = self.new_local(vt);
            self.temps.insert(i, l);
            self.out.push(W::LSet(l));
        }
    }
    fn block(&mut self, blk: u32) {
        let items = self.b.get_list(self.b.data[blk as usize][0]).to_vec();
        for i in items {
            self.inst(i);
        }
    }
    fn tail(&self, blk: u32) -> u32 {
        self.b.data[blk as usize][1]
    }
    fn inst(&mut self, i: u32) {
        let [a, bb] = self.b.data[i as usize];
        match self.b.tags[i as usize] {
            TirTag::LocalGet => {
                self.out.push(W::LGet(self.local_map[a as usize]));
                self.store(i);
            }
            TirTag::LocalSet => {
                self.load(bb);
                self.out.push(W::LSet(self.local_map[a as usize]));
            }
            TirTag::Prim => {
                for r in self.b.get_list(bb).to_vec() {
                    if a == PrimOp::Neg as u32 {
                        self.out.push(W::I32(0));
                    }
                    self.load(r);
                }
                self.out.push(W::Prim(a));
                self.store(i);
            }
            TirTag::Intrinsic => {
                for r in self.b.get_list(bb).to_vec() {
                    self.load(r);
                }
                self.out.push(W::CallImport(a));
            }
            TirTag::Call => {
                for r in self.b.get_list(bb).to_vec() {
                    self.load(r);
                }
                let rec = self.b.get_list(a).to_vec();
                let callee = if rec[0] == CALLEE_ITEM {
                    let targs: Vec<Ty> = rec[2..].iter().map(|&t| self.sub(Ty(t))).collect();
                    Instance { item: DefId(rec[1]), ty_args: targs }
                } else {
                    resolve_method(self.w, &rec, self.args).expect("collection resolved every method")
                };
                let callee = classify(self.w, self.bodies, callee);
                self.out.push(W::Call(instance_key(self.w, &callee)));
                // An erased result is cast back where the exact type is needed.
                let cret = match self.w.defs.get(&callee.item) {
                    Some(DefKind::Fn(s) | DefKind::ImplMethod { sig: s, .. }) => s.ret,
                    _ => panic!("callee"),
                };
                let cret = self.w.subst(cret, &callee.ty_args, None);
                let want = self.sub(self.b.ty[i as usize]);
                if let (Some(VT::Eq), Some(VT::Ref(p))) = (layout(self.w, cret), layout(self.w, want)) {
                    self.out.push(W::Cast(p));
                }
                self.store(i);
            }
            TirTag::NewData => {
                for r in self.b.get_list(bb).to_vec() {
                    self.load(r);
                }
                let t = self.sub(self.b.ty[i as usize]);
                let Some(VT::Ref(p)) = layout(self.w, t) else { panic!("data layout") };
                self.out.push(W::StructNew(p));
                self.store(i);
            }
            TirTag::Field => {
                self.load(a);
                let base_ty = self.b.ty[a as usize];
                let base_ty = self.sub(base_ty);
                let TyKind::Adt(d) = *self.w.kind(base_ty) else { panic!("field base") };
                let packed = match self.w.defs.get(&d) {
                    Some(DefKind::Data(fs)) => matches!(self.w.kind(fs[bb as usize].1), TyKind::Bool),
                    _ => false,
                };
                self.out.push(W::StructGet(self.w.path(d).to_owned(), bb, packed));
                self.store(i);
            }
            TirTag::If => {
                self.load(a);
                let rec = self.b.get_list(bb).to_vec();
                self.out.push(W::If);
                self.ctrl.push(None);
                self.block(rec[0]);
                if rec[1] != NONE {
                    self.out.push(W::Else);
                    self.block(rec[1]);
                }
                self.ctrl.pop();
                self.out.push(W::End);
            }
            TirTag::Loop => {
                let label = (0..self.b.label_inst.len())
                    .find(|&l| self.b.label_inst[l].0 == i)
                    .map(|l| u32::try_from(l).expect("label"));
                self.out.push(W::Block);
                self.ctrl.push(label);
                self.out.push(W::Loop);
                self.ctrl.push(None);
                self.block(a);
                self.out.push(W::Br(0));
                self.ctrl.pop();
                self.out.push(W::End);
                self.ctrl.pop();
                self.out.push(W::End);
            }
            TirTag::Break => {
                let pos = self.ctrl.iter().rposition(|c| *c == Some(a)).expect("break target");
                let depth = u32::try_from(self.ctrl.len() - 1 - pos).expect("depth");
                self.out.push(W::Br(depth));
            }
            TirTag::Return => {
                self.load(a);
                self.out.push(W::Return);
            }
            TirTag::Block => panic!("a Block is emitted by its owner"),
        }
    }
}

/// `Emit(inst)`: walks the generic TIR under the substitution.
#[expect(clippy::implicit_hasher, reason = "the run's tables use the std hasher only")]
pub fn emit(w: &mut World, bodies: &HashMap<DefId, TirBody>, b: &TirBody, inst: &Instance) -> Code {
    let mut params = Vec::new();
    let mut local_map = Vec::new();
    let mut user_locals = Vec::new();
    for l in 0..b.local_ty.len() {
        let t = w.subst(b.local_ty[l], &inst.ty_args, None);
        let vt = layout(w, t).unwrap_or(VT::I32);
        if b.local_flags[l] & LOCAL_PARAM != 0 {
            local_map.push(u32::try_from(params.len()).expect("p"));
            params.push(vt);
        } else {
            local_map.push(u32::MAX);
            user_locals.push((l, vt));
        }
    }
    let nparams = u32::try_from(params.len()).expect("p");
    let mut locals = Vec::new();
    for (l, vt) in user_locals {
        local_map[l] = nparams + u32::try_from(locals.len()).expect("l");
        locals.push(vt);
    }
    let ret = match w.defs.get(&inst.item) {
        Some(DefKind::Fn(s) | DefKind::ImplMethod { sig: s, .. }) => s.ret,
        _ => panic!("emit of a non-function"),
    };
    let ret = w.subst(ret, &inst.ty_args, None);
    let results: Vec<VT> = layout(w, ret).into_iter().collect();
    let root = b.sub_root[0].0;
    let mut em = Em {
        w,
        bodies,
        b,
        args: &inst.ty_args,
        out: Vec::new(),
        locals,
        nparams,
        temps: HashMap::new(),
        local_map,
        ctrl: Vec::new(),
    };
    em.block(root);
    let tail = em.tail(root);
    if tail == NONE {
        if !results.is_empty() {
            em.out.push(W::Unreachable);
        }
    } else {
        em.load(tail);
    }
    em.out.push(W::End);
    let (body, relocs) = encode(&em.locals, &em.out);
    Code { params, results, body, relocs }
}

// ------------------------------------------------------------------- link

pub const IMPORTS: [(&str, &str); 1] = [("hd", "println_i32")];

/// `Link(P)`: index assignment, type section, relocation patching. Inputs are
/// sorted by instance key, so the bytes are deterministic.
#[must_use]
pub fn link(
    w: &World,
    codes: &[(Hash128, Code)],
    root: Hash128,
    data_types: &[String],
    imports: &[u32],
) -> Vec<u8> {
    let mut types = TypeSection::new();
    // Struct types first, one rec group so fields may name any data type.
    let struct_idx: BTreeMap<String, u32> = data_types
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), u32::try_from(i).expect("t")))
        .collect();
    let val = |vt: &VT| -> ValType {
        match vt {
            VT::I32 => ValType::I32,
            VT::Eq => ValType::Ref(RefType::EQREF),
            VT::Ref(p) => ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(struct_idx[p]),
            }),
        }
    };
    let subtypes: Vec<SubType> = data_types
        .iter()
        .map(|p| {
            let d = w.lookup(p).expect("data def");
            let Some(DefKind::Data(fields)) = w.defs.get(&d) else { panic!("data") };
            let fields: Vec<FieldType> = fields
                .iter()
                .map(|(_, t)| {
                    let element_type = match w.kind(*t) {
                        TyKind::Bool => StorageType::I8,
                        _ => StorageType::Val(val(&layout(w, *t).expect("field layout"))),
                    };
                    FieldType { element_type, mutable: true }
                })
                .collect();
            SubType {
                is_final: true,
                supertype_idxs: vec![],
                composite_type: CompositeType {
                    inner: CompositeInnerType::Struct(StructType { fields: fields.into() }),
                    shared: false,
                    descriptor: None,
                    describes: None,
                },
            }
        })
        .collect();
    let mut ntypes = 0u32;
    if !subtypes.is_empty() {
        ntypes = u32::try_from(subtypes.len()).expect("t");
        types.ty().rec(subtypes);
    }
    let mut func_types: BTreeMap<(Vec<VT>, Vec<VT>), u32> = BTreeMap::new();
    let mut sig_of = |types: &mut TypeSection, params: &[VT], results: &[VT]| -> u32 {
        let key = (params.to_vec(), results.to_vec());
        if let Some(&i) = func_types.get(&key) {
            return i;
        }
        let i = ntypes;
        ntypes += 1;
        types.ty().function(params.iter().map(val), results.iter().map(val));
        func_types.insert(key, i);
        i
    };
    let mut imps = ImportSection::new();
    let mut import_idx = HashMap::new();
    for (n, &imp) in imports.iter().enumerate() {
        let t = sig_of(&mut types, &[VT::I32], &[]);
        let (m, f) = IMPORTS[imp as usize];
        imps.import(m, f, EntityType::Function(t));
        import_idx.insert(imp, u32::try_from(n).expect("i"));
    }
    let nimports = u32::try_from(imports.len()).expect("i");
    let func_idx: HashMap<Hash128, u32> = codes
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (*k, nimports + u32::try_from(i).expect("f")))
        .collect();
    let mut funcs = FunctionSection::new();
    let mut code_sec = CodeSection::new();
    for (_, c) in codes {
        let t = sig_of(&mut types, &c.params, &c.results);
        funcs.function(t);
        // Patch each relocation's padded slot; no re-encoding.
        let mut body = c.body.clone();
        for (at, r) in &c.relocs {
            let v = match r {
                Reloc::Func(k) => func_idx[k],
                Reloc::Import(i) => import_idx[i],
                Reloc::Type(p) => struct_idx[p],
            };
            let mut b = Vec::new();
            padded(&mut b, v);
            body[*at as usize..*at as usize + 5].copy_from_slice(&b);
        }
        code_sec.raw(&body);
    }
    let mut exports = ExportSection::new();
    exports.export("main", ExportKind::Func, func_idx[&root]);
    let mut module = Module::new();
    module.section(&types);
    module.section(&imps);
    module.section(&funcs);
    module.section(&exports);
    module.section(&code_sec);
    module.finish()
}
