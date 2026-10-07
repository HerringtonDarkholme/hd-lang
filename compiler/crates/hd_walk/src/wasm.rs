//! Wasm GC emission (codegen.md §12, wasm-layout.md §15) and link (§13.10).
//! `Emit` writes a code entry per instance with symbolic relocations (callees
//! by instance key, struct types by stable path); `Link` assigns indices.

use std::collections::{BTreeMap, HashMap};

use hd_base::Hash128;
use wasm_encoder::{
    BlockType, CodeSection, CompositeInnerType, CompositeType, EntityType, ExportKind,
    ExportSection, FieldType, Function, FunctionSection, HeapType, ImportSection, Module, RefType,
    StorageType, StructType, SubType, TypeSection, ValType,
};

use crate::mono::{Instance, instance_key, resolve_method};
use crate::tir::{CALLEE_ITEM, CONST_BIT, NONE, PrimOp, TirBody, TirTag};
use crate::world::{DefKind, Ty, TyKind, World};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VT {
    I32,
    Ref(String),
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub params: Vec<VT>,
    pub results: Vec<VT>,
    pub locals: Vec<VT>,
    pub body: Vec<W>,
}

/// The layout of a type (wasm-layout.md §15.1): `None` for `void`/`never`.
fn layout(w: &World, t: Ty) -> Option<VT> {
    match w.kind(t) {
        TyKind::Void | TyKind::Never => None,
        TyKind::Bool | TyKind::I32 => Some(VT::I32),
        TyKind::Adt(d) => Some(VT::Ref(w.path(*d).to_owned())),
        TyKind::Param(_) | TyKind::SelfTy => panic!("layout of an unsubstituted type"),
    }
}

struct Em<'a> {
    w: &'a mut World,
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
                    Instance { item: crate::world::DefId(rec[1]), ty_args: targs }
                } else {
                    resolve_method(self.w, &rec, self.args)
                };
                self.out.push(W::Call(instance_key(self.w, &callee)));
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
pub fn emit(w: &mut World, b: &TirBody, inst: &Instance) -> Code {
    let mut params = Vec::new();
    let mut local_map = Vec::new();
    let mut user_locals = Vec::new();
    for l in 0..b.local_ty.len() {
        let t = w.subst(b.local_ty[l], &inst.ty_args, None);
        let vt = layout(w, t).unwrap_or(VT::I32);
        if b.local_flags[l] & crate::tir::LOCAL_PARAM != 0 {
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
    Code { params, results, locals: em.locals, body: em.out }
}

// ------------------------------------------------------------------- link

pub const IMPORTS: [(&str, &str); 1] = [("hd", "println_i32")];

/// `Link(P)`: index assignment, type section, relocation patching. Inputs are
/// sorted by instance key, so the bytes are deterministic.
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
        let mut f = Function::new_with_locals_types(c.locals.iter().map(val));
        let mut s = f.instructions();
        for op in &c.body {
            match op {
                W::I32(v) => s.i32_const(*v),
                W::LGet(l) => s.local_get(*l),
                W::LSet(l) => s.local_set(*l),
                W::Call(k) => s.call(func_idx[k]),
                W::CallImport(i) => s.call(import_idx[i]),
                W::StructNew(p) => s.struct_new(struct_idx[p]),
                W::StructGet(p, f, packed) => {
                    if *packed {
                        s.struct_get_u(struct_idx[p], *f)
                    } else {
                        s.struct_get(struct_idx[p], *f)
                    }
                }
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
            };
        }
        code_sec.function(&f);
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
