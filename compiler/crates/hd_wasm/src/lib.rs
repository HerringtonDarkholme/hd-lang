#![forbid(unsafe_code)]
//! `hd_wasm`: Wasm GC emission (codegen.md §12, wasm-layout.md §15) and link
//! (§13.10). `Emit` walks one instance's `hd_tir::ir::Body` under its
//! substitution, with value layouts from `hd_mono::layout::layout_of`,
//! call targets from collection (no selection here), and host imports from
//! `hd_host_abi`. A code entry holds symbolic relocations (callees by
//! instance key, struct types by stable path); `Link` assigns indices.

pub mod meta;

use std::collections::{BTreeMap, HashMap};

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, Hash128, NotImplemented, Stage, StageResult};
use wasm_encoder::{
    BlockType, CodeSection, CompositeInnerType, CompositeType, EntityType, ExportKind,
    ExportSection, FieldType, FunctionSection, HeapType, ImportSection, Module, RefType,
    StorageType, StructType, SubType, TypeSection, ValType,
};

use hd_mono::layout::{LayoutClass, ValType as LV, layout_of};
use hd_mono::{CallTarget, ProgramEnv, is_class_ref, subst};
use hd_tir::ir::{Body, NONE, PrimOp, Ref, Tag, local_flags};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

/// A Wasm value type with symbolic struct references.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VT {
    I32,
    I64,
    /// `(ref $T)` by stable path.
    Ref(String),
    /// `eqref`: the erased layout of the `REF` class (wasm-layout.md §15.1).
    Eq,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum W {
    I32(i32),
    LGet(u32),
    LSet(u32),
    Call(Hash128),
    CallImport(u32),
    StructNew(String),
    StructGet(String, u32, bool),
    Prim(PrimOp),
    If,
    Else,
    End,
    Block,
    Loop,
    Br(u32),
    Return,
    Unreachable,
    /// `ref.cast (ref $T)`: an erased value is cast where its exact type is needed.
    Cast(String),
}

/// A relocation target (codegen.md §13.8 `Reloc`).
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
    fn encode(&self, w: &mut Writer) {
        match self {
            VT::I32 => w.u8(0),
            VT::Eq => w.u8(1),
            VT::Ref(p) => {
                w.u8(2);
                w.str(p);
            }
            VT::I64 => w.u8(3),
        }
    }
    fn decode(r: &mut Reader<'_>) -> VT {
        match r.u8() {
            0 => VT::I32,
            1 => VT::Eq,
            3 => VT::I64,
            _ => VT::Ref(r.str().to_owned()),
        }
    }
}

impl Code {
    /// The code entry's bytes (codegen.md §13.8): ID-free.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        for list in [&self.params, &self.results] {
            w.len_of(list);
            for v in list {
                v.encode(&mut w);
            }
        }
        w.blob(&self.body);
        w.len_of(&self.relocs);
        for (at, r) in &self.relocs {
            w.u32(*at);
            match r {
                Reloc::Func(k) => {
                    w.u8(0);
                    w.hash(*k);
                }
                Reloc::Import(i) => {
                    w.u8(1);
                    w.u32(*i);
                }
                Reloc::Type(p) => {
                    w.u8(2);
                    w.str(p);
                }
            }
        }
        w.bytes
    }

    /// Decodes a code entry; `None` when malformed (a cache miss).
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Code> {
        let mut r = Reader::new(bytes);
        let n = r.count();
        let params = (0..n).map(|_| VT::decode(&mut r)).collect();
        let n = r.count();
        let results = (0..n).map(|_| VT::decode(&mut r)).collect();
        let body = r.blob().to_vec();
        let n = r.count();
        let relocs = (0..n)
            .map(|_| {
                let at = r.u32();
                let reloc = match r.u8() {
                    0 => Reloc::Func(r.hash()),
                    1 => Reloc::Import(r.u32()),
                    _ => Reloc::Type(r.str().to_owned()),
                };
                (at, reloc)
            })
            .collect();
        let relocs_ok =
            |rs: &Vec<(u32, Reloc)>| rs.iter().all(|(at, _)| (*at as usize) + 5 <= body.len());
        (r.ok() && relocs_ok(&relocs)).then_some(Code {
            params,
            results,
            body,
            relocs,
        })
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
        VT::I64 => out.push(0x7e),
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
            W::I32(v) => {
                wasm_encoder::InstructionSink::new(&mut out).i32_const(*v);
            }
            W::LGet(l) => {
                wasm_encoder::InstructionSink::new(&mut out).local_get(*l);
            }
            W::LSet(l) => {
                wasm_encoder::InstructionSink::new(&mut out).local_set(*l);
            }
            W::Prim(op) => {
                let mut s = wasm_encoder::InstructionSink::new(&mut out);
                match op {
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
                };
            }
            W::If => {
                wasm_encoder::InstructionSink::new(&mut out).if_(BlockType::Empty);
            }
            W::Else => {
                wasm_encoder::InstructionSink::new(&mut out).else_();
            }
            W::End => {
                wasm_encoder::InstructionSink::new(&mut out).end();
            }
            W::Block => {
                wasm_encoder::InstructionSink::new(&mut out).block(BlockType::Empty);
            }
            W::Loop => {
                wasm_encoder::InstructionSink::new(&mut out).loop_(BlockType::Empty);
            }
            W::Br(d) => {
                wasm_encoder::InstructionSink::new(&mut out).br(*d);
            }
            W::Return => {
                wasm_encoder::InstructionSink::new(&mut out).return_();
            }
            W::Unreachable => {
                wasm_encoder::InstructionSink::new(&mut out).unreachable();
            }
        }
    }
    (out, relocs)
}

fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Emit, what))
}

/// The Wasm values of a concrete type (wasm-layout.md §15.1, §15.2):
/// `layout_of`'s classes, with a data type's reference named by its path.
pub fn vt_of(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    t: Ty,
) -> StageResult<Option<VT>> {
    if is_class_ref(pool, t) {
        return Ok(Some(VT::Eq));
    }
    let l = layout_of(pool, env, t)?;
    Ok(match (l.class, l.values.as_slice()) {
        (LayoutClass::Void, _) => None,
        (LayoutClass::I32, [LV::I32]) => Some(VT::I32),
        (LayoutClass::I64, [LV::I64]) => Some(VT::I64),
        (LayoutClass::Ref, [LV::Ref { nullable: false }]) => match pool.get(t) {
            TyData::Adt { def, args } if args == TyList::EMPTY => Some(VT::Ref(path(def))),
            _ => return unsupported(format!("the layout of {}", pool.display(t))),
        },
        _ => return unsupported(format!("the layout of {}", pool.display(t))),
    })
}

struct Em<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    path: &'a dyn Fn(DefId) -> String,
    b: &'a Body,
    item: DefId,
    args: TyList,
    calls: &'a HashMap<u32, CallTarget>,
    out: Vec<W>,
    locals: Vec<VT>,
    nparams: u32,
    temps: HashMap<u32, u32>,
    local_map: Vec<u32>,
    /// Control stack: `Some(label)` for a loop's outer break block.
    ctrl: Vec<Option<u32>>,
}

impl Em<'_> {
    fn sub(&self, t: Ty) -> Ty {
        subst(self.pool, self.item, self.args, t)
    }
    fn vt(&self, t: Ty) -> StageResult<Option<VT>> {
        vt_of(self.pool, self.env, self.path, self.sub(t))
    }
    fn new_local(&mut self, vt: VT) -> u32 {
        self.locals.push(vt);
        self.nparams + u32::try_from(self.locals.len() - 1).expect("locals")
    }
    fn load(&mut self, r: u32) -> StageResult<()> {
        if r == NONE {
            return Ok(());
        }
        let r = Ref(r);
        if r.as_inst().is_none() {
            let Some(&(t, bits)) = self.b.consts.get((r.0 & !Ref::CONST_BIT) as usize) else {
                return unsupported("a constant outside the body's column");
            };
            match self.pool.get(self.sub(t)) {
                TyData::Prim(p) if p.is_integer() || p == Prim::Bool => {
                    let low = u32::try_from(bits & 0xffff_ffff).expect("32 bits");
                    self.out.push(W::I32(low.cast_signed()));
                }
                _ => return unsupported("a constant of this type"),
            }
        } else if let Some(&l) = self.temps.get(&r.0) {
            self.out.push(W::LGet(l));
        }
        Ok(())
    }
    fn store(&mut self, i: u32) -> StageResult<()> {
        if let Some(vt) = self.vt(self.b.ty[i as usize])? {
            let l = self.new_local(vt);
            self.temps.insert(i, l);
            self.out.push(W::LSet(l));
        }
        Ok(())
    }
    fn block(&mut self, blk: u32) -> StageResult<()> {
        if blk == NONE {
            return Ok(());
        }
        let list = self.b.record(self.b.data[blk as usize][0]).to_vec();
        for i in list {
            // A `Block` in a list belongs to the `If` or `Loop` after it.
            if self.b.tags[i as usize] != Tag::Block {
                self.inst(i)?;
            }
        }
        Ok(())
    }
    fn values(&mut self, rec: u32) -> StageResult<()> {
        for r in self.b.record(rec).to_vec() {
            self.load(r)?;
        }
        Ok(())
    }
    fn inst(&mut self, i: u32) -> StageResult<()> {
        let [a, bb] = self.b.data[i as usize];
        match self.b.tags[i as usize] {
            Tag::LocalGet => {
                self.out.push(W::LGet(self.local_map[a as usize]));
                self.store(i)?;
            }
            Tag::LocalSet => {
                self.load(bb)?;
                self.out.push(W::LSet(self.local_map[a as usize]));
            }
            Tag::Prim => {
                let Some(op) = PrimOp::from_u32(a) else {
                    return unsupported("an unknown operator");
                };
                if op == PrimOp::Neg {
                    self.out.push(W::I32(0));
                }
                self.values(bb)?;
                self.out.push(W::Prim(op));
                self.store(i)?;
            }
            Tag::CallHost => {
                self.values(bb)?;
                self.out.push(W::CallImport(a));
            }
            Tag::Call => {
                let args_at = bb;
                let n = self.b.record(args_at).len().saturating_sub(3);
                let body = self.b;
                for &r in &body.record(args_at)[..n] {
                    self.load(r)?;
                }
                let Some(target) = self.calls.get(&i).copied() else {
                    return unsupported("a call that collection did not resolve");
                };
                self.out.push(W::Call(target.key));
                let want = self.vt(self.b.ty[i as usize])?;
                let got = vt_of(self.pool, self.env, self.path, target.ret)?;
                if let (Some(VT::Eq), Some(VT::Ref(p))) = (got, want) {
                    self.out.push(W::Cast(p));
                }
                self.store(i)?;
            }
            Tag::NewData => {
                self.values(bb)?;
                let Some(VT::Ref(p)) = self.vt(self.b.ty[i as usize])? else {
                    return unsupported("a data value without a struct layout");
                };
                self.out.push(W::StructNew(p));
                self.store(i)?;
            }
            Tag::Field => {
                self.load(a)?;
                let base = self.sub(self.b.ty[a as usize]);
                let TyData::Adt { def, .. } = self.pool.get(base) else {
                    return unsupported("a field of a non-data value");
                };
                let fields = self.env.data_fields(def).unwrap_or_default();
                let packed = fields.get(bb as usize).is_some_and(|t| *t == Ty::BOOL);
                self.out.push(W::StructGet((self.path)(def), bb, packed));
                self.store(i)?;
            }
            Tag::If => {
                self.load(a)?;
                let rec = self.b.record(bb).to_vec();
                self.out.push(W::If);
                self.ctrl.push(None);
                self.block(rec[0])?;
                if rec.get(1).is_some_and(|e| *e != NONE) {
                    self.out.push(W::Else);
                    self.block(rec[1])?;
                }
                self.ctrl.pop();
                self.out.push(W::End);
            }
            Tag::Loop => {
                let label = self
                    .b
                    .label_inst
                    .iter()
                    .position(|&x| x == i)
                    .map(|l| u32::try_from(l).expect("label"));
                self.out.push(W::Block);
                self.ctrl.push(label);
                self.out.push(W::Loop);
                self.ctrl.push(None);
                self.block(a)?;
                self.out.push(W::Br(0));
                self.ctrl.pop();
                self.out.push(W::End);
                self.ctrl.pop();
                self.out.push(W::End);
            }
            Tag::Break => {
                let Some(pos) = self.ctrl.iter().rposition(|c| *c == Some(a)) else {
                    return unsupported("a break outside its loop");
                };
                let depth = u32::try_from(self.ctrl.len() - 1 - pos).expect("depth");
                self.out.push(W::Br(depth));
            }
            Tag::Return => {
                self.load(a)?;
                self.out.push(W::Return);
            }
            Tag::Block => {}
            other => return unsupported(format!("emission of TIR tag {}", other.name())),
        }
        Ok(())
    }
}

/// `Emit(inst)`: walks the generic TIR under the instance's arguments.
pub fn emit(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    b: &Body,
    args: TyList,
    ret: Ty,
    calls: &HashMap<u32, CallTarget>,
) -> StageResult<Code> {
    let item = b.item;
    let mut params = Vec::new();
    let mut local_map = Vec::new();
    let mut user_locals = Vec::new();
    for l in 0..b.local_ty.len() {
        let t = subst(pool, item, args, b.local_ty[l]);
        let vt = vt_of(pool, env, path, t)?.unwrap_or(VT::I32);
        if b.local_flags[l] & local_flags::PARAM != 0 {
            local_map.push(u32::try_from(params.len()).expect("params"));
            params.push(vt);
        } else {
            local_map.push(u32::MAX);
            user_locals.push((l, vt));
        }
    }
    let nparams = u32::try_from(params.len()).expect("params");
    let mut locals = Vec::new();
    for (l, vt) in user_locals {
        local_map[l] = nparams + u32::try_from(locals.len()).expect("locals");
        locals.push(vt);
    }
    let results: Vec<VT> = vt_of(pool, env, path, subst(pool, item, args, ret))?
        .into_iter()
        .collect();
    let root = b.sub_root[0];
    let mut em = Em {
        pool,
        env,
        path,
        b,
        item,
        args,
        calls,
        out: Vec::new(),
        locals,
        nparams,
        temps: HashMap::new(),
        local_map,
        ctrl: Vec::new(),
    };
    em.block(root)?;
    let tail = b.data[root as usize][1];
    if tail == NONE {
        if !results.is_empty() {
            em.out.push(W::Unreachable);
        }
    } else {
        em.load(tail)?;
    }
    em.out.push(W::End);
    let (body, relocs) = encode(&em.locals, &em.out);
    Ok(Code {
        params,
        results,
        body,
        relocs,
    })
}

// ------------------------------------------------------------------- link

/// One struct type: its stable path and fields (value type, packed `i8`).
pub type StructDef = (String, Vec<(VT, bool)>);

/// `Link(P)`: index assignment, type section, relocation patching. Inputs
/// are in content order, so the bytes are deterministic. Imports come from
/// `hd_host_abi::PRELUDE_IMPORTS` by index.
pub fn link(
    codes: &[(Hash128, Code)],
    root: Hash128,
    structs: &[StructDef],
    imports: &[u32],
) -> StageResult<Vec<u8>> {
    let mut types = TypeSection::new();
    let struct_idx: BTreeMap<String, u32> = structs
        .iter()
        .enumerate()
        .map(|(i, s)| (s.0.clone(), u32::try_from(i).expect("types")))
        .collect();
    let val = |vt: &VT| -> StageResult<ValType> {
        Ok(match vt {
            VT::I32 => ValType::I32,
            VT::I64 => ValType::I64,
            VT::Eq => ValType::Ref(RefType::EQREF),
            VT::Ref(p) => {
                let Some(&i) = struct_idx.get(p) else {
                    return unsupported(format!("no struct type for {p}"));
                };
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(i),
                })
            }
        })
    };
    let mut subtypes = Vec::new();
    for (_, fields) in structs {
        let mut fs = Vec::new();
        for (vt, packed) in fields {
            let element_type = if *packed {
                StorageType::I8
            } else {
                StorageType::Val(val(vt)?)
            };
            fs.push(FieldType {
                element_type,
                mutable: true,
            });
        }
        subtypes.push(SubType {
            is_final: true,
            supertype_idxs: vec![],
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType { fields: fs.into() }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
    }
    let mut ntypes = 0u32;
    if !subtypes.is_empty() {
        ntypes = u32::try_from(subtypes.len()).expect("types");
        types.ty().rec(subtypes);
    }
    let mut func_types: BTreeMap<(Vec<VT>, Vec<VT>), u32> = BTreeMap::new();
    let mut sig_of = |types: &mut TypeSection, params: &[VT], results: &[VT]| -> StageResult<u32> {
        let key = (params.to_vec(), results.to_vec());
        if let Some(&i) = func_types.get(&key) {
            return Ok(i);
        }
        let i = ntypes;
        ntypes += 1;
        let ps: Vec<ValType> = params.iter().map(&val).collect::<StageResult<_>>()?;
        let rs: Vec<ValType> = results.iter().map(&val).collect::<StageResult<_>>()?;
        types.ty().function(ps, rs);
        func_types.insert(key, i);
        Ok(i)
    };
    let mut imps = ImportSection::new();
    let mut import_idx = HashMap::new();
    for (n, &imp) in imports.iter().enumerate() {
        let Some(p) = hd_host_abi::PRELUDE_IMPORTS.get(imp as usize) else {
            return unsupported("an unknown import");
        };
        let params: Vec<VT> = p
            .params
            .iter()
            .map(|s| match s {
                hd_host_abi::Scalar::I32 => VT::I32,
                _ => VT::I64,
            })
            .collect();
        let t = sig_of(&mut types, &params, &[])?;
        imps.import(p.module, p.field, EntityType::Function(t));
        import_idx.insert(imp, u32::try_from(n).expect("imports"));
    }
    let nimports = u32::try_from(imports.len()).expect("imports");
    let func_idx: HashMap<Hash128, u32> = codes
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (*k, nimports + u32::try_from(i).expect("funcs")))
        .collect();
    let mut funcs = FunctionSection::new();
    let mut code_sec = CodeSection::new();
    for (_, c) in codes {
        let t = sig_of(&mut types, &c.params, &c.results)?;
        funcs.function(t);
        let mut body = c.body.clone();
        for (at, r) in &c.relocs {
            let v = match r {
                Reloc::Func(k) => func_idx.get(k).copied(),
                Reloc::Import(i) => import_idx.get(i).copied(),
                Reloc::Type(p) => struct_idx.get(p).copied(),
            };
            let Some(v) = v else {
                return unsupported("a relocation with no target");
            };
            let mut b = Vec::new();
            padded(&mut b, v);
            body[*at as usize..*at as usize + 5].copy_from_slice(&b);
        }
        code_sec.raw(&body);
    }
    let Some(&main) = func_idx.get(&root) else {
        return unsupported("the root instance has no code");
    };
    let mut exports = ExportSection::new();
    exports.export("main", ExportKind::Func, main);
    let mut module = Module::new();
    module.section(&types);
    module.section(&imps);
    module.section(&funcs);
    module.section(&exports);
    module.section(&code_sec);
    Ok(module.finish())
}
