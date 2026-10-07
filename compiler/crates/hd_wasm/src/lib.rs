#![forbid(unsafe_code)]
//! `hd_wasm`: Wasm GC emission (codegen.md §12, wasm-layout.md §15) and link
//! (§13.10). `Emit` walks one instance's `hd_tir::ir::Body` under its
//! substitution (`emit`), with value layouts from `layout` and call targets
//! from collection (no selection here). Runtime pieces the compiler builds
//! (literal getters, number formatting, panic stubs, host provider stubs,
//! vtable adapters, the entry wrapper) are `rt` helpers. A code entry holds
//! symbolic relocations: callees by instance key or helper, Wasm types by
//! their structural descriptor; `Link` assigns indices.

pub mod asm;
pub mod emit;
pub mod layout;
pub mod meta;
pub mod rt;

use std::collections::{BTreeMap, BTreeSet};

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, NotImplemented, Stage, StageResult};
use wasm_encoder::{
    CodeSection, CompositeInnerType, CompositeType, ConstExpr, DataCountSection, DataSection,
    ElementSection, Elements, EntityType, ExportKind, ExportSection, FieldType, FuncType,
    FunctionSection, GlobalSection, GlobalType, HeapType, ImportSection, MemorySection, MemoryType,
    Module, RefType, StorageType, StructType, SubType, TypeSection, ValType,
};

pub use emit::{emit, entry};
pub use rt::Helper;

/// A Wasm value type, with references to structural type descriptors.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VT {
    I32,
    I64,
    F32,
    F64,
    /// `eqref`, nullable: erased storage (the A1 class `REF`, wasm-layout.md §15.1).
    Eq,
    /// `(ref $T)` or `(ref null $T)`.
    Ref(Box<WTy>, bool),
}

/// A Wasm heap type, described by structure (hd needs no nominal Wasm
/// types, codegen.md §13.7): equal descriptors are one type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WTy {
    /// `(array (mut i8))`: string bytes.
    Bytes,
    /// `(array (mut T))`.
    Array(VT),
    /// A struct of mutable fields; `open` is a non-final type that closures,
    /// suspension frames and their subtypes extend.
    Struct {
        fields: Vec<VT>,
        sup: Option<Box<WTy>>,
        open: bool,
    },
    Func(Vec<VT>, Vec<VT>),
}

impl VT {
    #[must_use]
    pub fn r(t: WTy) -> VT {
        VT::Ref(Box::new(t), false)
    }
    #[must_use]
    pub fn rn(t: WTy) -> VT {
        VT::Ref(Box::new(t), true)
    }
    /// The defaultable form (wasm-layout.md §15.2): references nullable.
    #[must_use]
    pub fn dflt(&self) -> VT {
        match self {
            VT::Ref(t, _) => VT::Ref(t.clone(), true),
            v => v.clone(),
        }
    }
    fn encode(&self, w: &mut Writer) {
        match self {
            VT::I32 => w.u8(0),
            VT::I64 => w.u8(1),
            VT::F32 => w.u8(2),
            VT::F64 => w.u8(3),
            VT::Eq => w.u8(4),
            VT::Ref(t, n) => {
                w.u8(if *n { 6 } else { 5 });
                t.encode(w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>, depth: u8) -> Option<VT> {
        Some(match r.u8() {
            0 => VT::I32,
            1 => VT::I64,
            2 => VT::F32,
            3 => VT::F64,
            4 => VT::Eq,
            5 => VT::Ref(Box::new(WTy::decode(r, depth)?), false),
            6 => VT::Ref(Box::new(WTy::decode(r, depth)?), true),
            _ => return None,
        })
    }
}

pub(crate) fn encode_vts(v: &[VT], w: &mut Writer) {
    w.len_of(v);
    for x in v {
        x.encode(w);
    }
}

pub(crate) fn decode_vts(r: &mut Reader<'_>, depth: u8) -> Option<Vec<VT>> {
    let n = r.count();
    (0..n).map(|_| VT::decode(r, depth)).collect()
}

impl WTy {
    fn encode(&self, w: &mut Writer) {
        match self {
            WTy::Bytes => w.u8(0),
            WTy::Array(v) => {
                w.u8(1);
                v.encode(w);
            }
            WTy::Struct { fields, sup, open } => {
                w.u8(2);
                encode_vts(fields, w);
                w.u8(u8::from(*open));
                match sup {
                    Some(s) => {
                        w.u8(1);
                        s.encode(w);
                    }
                    None => w.u8(0),
                }
            }
            WTy::Func(p, r) => {
                w.u8(3);
                encode_vts(p, w);
                encode_vts(r, w);
            }
        }
    }
    fn decode(r: &mut Reader<'_>, depth: u8) -> Option<WTy> {
        let depth = depth.checked_add(1).filter(|d| *d < 64)?;
        Some(match r.u8() {
            0 => WTy::Bytes,
            1 => WTy::Array(VT::decode(r, depth)?),
            2 => {
                let fields = decode_vts(r, depth)?;
                let open = r.u8() == 1;
                let sup = if r.u8() == 1 {
                    Some(Box::new(WTy::decode(r, depth)?))
                } else {
                    None
                };
                WTy::Struct { fields, sup, open }
            }
            3 => WTy::Func(decode_vts(r, depth)?, decode_vts(r, depth)?),
            _ => return None,
        })
    }
    #[must_use]
    pub fn fields(&self) -> &[VT] {
        match self {
            WTy::Struct { fields, .. } => fields,
            _ => &[],
        }
    }
}

/// A function symbol (codegen.md §13.8 `FuncTarget`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sym {
    /// A collected instance, by instance key.
    Inst(Hash128),
    /// A host import: module, name and Wasm signature.
    Import {
        module: String,
        name: String,
        params: Vec<VT>,
        results: Vec<VT>,
    },
    /// A compiler-generated runtime function.
    Helper(Helper),
}

/// A relocation target (codegen.md §13.8 `Reloc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reloc {
    Func(Sym),
    Type(WTy),
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

impl Sym {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Sym::Inst(k) => {
                w.u8(0);
                w.hash(*k);
            }
            Sym::Import {
                module,
                name,
                params,
                results,
            } => {
                w.u8(1);
                w.str(module);
                w.str(name);
                encode_vts(params, w);
                encode_vts(results, w);
            }
            Sym::Helper(h) => {
                w.u8(2);
                h.encode(w);
            }
        }
    }
    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Sym> {
        Some(match r.u8() {
            0 => Sym::Inst(r.hash()),
            1 => Sym::Import {
                module: r.str().to_owned(),
                name: r.str().to_owned(),
                params: decode_vts(r, 0)?,
                results: decode_vts(r, 0)?,
            },
            2 => Sym::Helper(Helper::decode(r)?),
            _ => return None,
        })
    }
}

impl Code {
    /// The code entry's bytes (codegen.md §13.8): ID-free.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        encode_vts(&self.params, &mut w);
        encode_vts(&self.results, &mut w);
        w.blob(&self.body);
        w.len_of(&self.relocs);
        for (at, r) in &self.relocs {
            w.u32(*at);
            match r {
                Reloc::Func(s) => {
                    w.u8(0);
                    s.encode(&mut w);
                }
                Reloc::Type(t) => {
                    w.u8(1);
                    t.encode(&mut w);
                }
            }
        }
        w.bytes
    }

    /// Decodes a code entry; `None` when malformed (a cache miss).
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Code> {
        let mut r = Reader::new(bytes);
        let params = decode_vts(&mut r, 0)?;
        let results = decode_vts(&mut r, 0)?;
        let body = r.blob().to_vec();
        let n = r.count();
        let mut relocs = Vec::new();
        for _ in 0..n {
            let at = r.u32();
            let reloc = match r.u8() {
                0 => Reloc::Func(Sym::decode(&mut r)?),
                1 => Reloc::Type(WTy::decode(&mut r, 0)?),
                _ => return None,
            };
            if (at as usize) + 5 > body.len() {
                return None;
            }
            relocs.push((at, reloc));
        }
        r.ok().then_some(Code {
            params,
            results,
            body,
            relocs,
        })
    }
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Emit, what))
}

pub(crate) fn padded(out: &mut Vec<u8>, v: u32) {
    for k in 0..4 {
        out.push(u8::try_from((v >> (7 * k)) & 0x7f).expect("7 bits") | 0x80);
    }
    out.push(u8::try_from((v >> 28) & 0x7f).expect("7 bits"));
}

// ------------------------------------------------------------------- link

/// The type section under construction: each descriptor once, its
/// dependencies first, each in its own recursion group.
#[derive(Default)]
struct Types {
    idx: BTreeMap<WTy, u32>,
    sec: TypeSection,
    n: u32,
}

impl Types {
    fn val(&mut self, v: &VT) -> ValType {
        match v {
            VT::I32 => ValType::I32,
            VT::I64 => ValType::I64,
            VT::F32 => ValType::F32,
            VT::F64 => ValType::F64,
            VT::Eq => ValType::Ref(RefType::EQREF),
            VT::Ref(t, n) => {
                let i = self.of(t);
                ValType::Ref(RefType {
                    nullable: *n,
                    heap_type: HeapType::Concrete(i),
                })
            }
        }
    }
    fn of(&mut self, t: &WTy) -> u32 {
        if let Some(&i) = self.idx.get(t) {
            return i;
        }
        let (inner, sup, open) = match t {
            WTy::Bytes => (
                CompositeInnerType::Array(wasm_encoder::ArrayType(FieldType {
                    element_type: StorageType::I8,
                    mutable: true,
                })),
                None,
                false,
            ),
            WTy::Array(v) => {
                let e = self.val(v);
                (
                    CompositeInnerType::Array(wasm_encoder::ArrayType(FieldType {
                        element_type: StorageType::Val(e),
                        mutable: true,
                    })),
                    None,
                    false,
                )
            }
            WTy::Struct { fields, sup, open } => {
                let sup = sup.as_ref().map(|s| self.of(s));
                let fs: Vec<FieldType> = fields
                    .iter()
                    .map(|f| FieldType {
                        element_type: StorageType::Val(self.val(f)),
                        mutable: true,
                    })
                    .collect();
                (
                    CompositeInnerType::Struct(StructType { fields: fs.into() }),
                    sup,
                    *open,
                )
            }
            WTy::Func(p, r) => {
                let ps: Vec<ValType> = p.iter().map(|v| self.val(v)).collect();
                let rs: Vec<ValType> = r.iter().map(|v| self.val(v)).collect();
                (CompositeInnerType::Func(FuncType::new(ps, rs)), None, false)
            }
        };
        self.sec.ty().subtype(&SubType {
            is_final: !open,
            supertype_idxs: sup.into_iter().collect(),
            composite_type: CompositeType {
                inner,
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
        let i = self.n;
        self.n += 1;
        self.idx.insert(t.clone(), i);
        i
    }
}

/// `Link(P)`: helpers, index assignment, the type section, the literal
/// pool, relocation patching (codegen.md §13.10). Inputs are in content
/// order, so the bytes are deterministic. `entry` is the exported `main`;
/// `names` are the instances' item paths for the dev `name` section.
pub fn link(codes: &[(Hash128, Code)], names: &[String], entry: &Helper) -> StageResult<Vec<u8>> {
    // Helpers and imports reachable from the code, to a fixed point.
    let mut helpers: BTreeMap<Helper, Code> = BTreeMap::new();
    let mut imports: BTreeSet<(String, String, Vec<VT>, Vec<VT>)> = BTreeSet::new();
    let mut todo = vec![entry.clone()];
    let mut scan = |c: &Code, todo: &mut Vec<Helper>| {
        for (_, r) in &c.relocs {
            match r {
                Reloc::Func(Sym::Helper(h)) => todo.push(h.clone()),
                Reloc::Func(Sym::Import {
                    module,
                    name,
                    params,
                    results,
                }) => {
                    imports.insert((
                        module.clone(),
                        name.clone(),
                        params.clone(),
                        results.clone(),
                    ));
                }
                _ => {}
            }
        }
    };
    for (_, c) in codes {
        scan(c, &mut todo);
    }
    while let Some(h) = todo.pop() {
        if helpers.contains_key(&h) {
            continue;
        }
        let c = rt::helper_code(&h)?;
        scan(&c, &mut todo);
        helpers.insert(h, c);
    }
    // Literals: one passive segment, deduplicated by content.
    let mut lits: BTreeMap<Vec<u8>, (u32, u32)> = BTreeMap::new();
    let mut data = Vec::new();
    for h in helpers.keys() {
        if let Helper::Lit(bytes) = h {
            let off = u32::try_from(data.len()).expect("data");
            data.extend_from_slice(bytes);
            lits.insert(
                bytes.clone(),
                (off, u32::try_from(bytes.len()).expect("lit")),
            );
        }
    }
    let mut types = Types::default();
    let mut imps = ImportSection::new();
    let mut func_idx: BTreeMap<Sym, u32> = BTreeMap::new();
    let mut nfuncs = 0u32;
    for (module, name, params, results) in &imports {
        let t = types.of(&WTy::Func(params.clone(), results.clone()));
        imps.import(module, name, EntityType::Function(t));
        func_idx.insert(
            Sym::Import {
                module: module.clone(),
                name: name.clone(),
                params: params.clone(),
                results: results.clone(),
            },
            nfuncs,
        );
        nfuncs += 1;
    }
    let mut bodies: Vec<&Code> = Vec::new();
    for (h, c) in &helpers {
        func_idx.insert(Sym::Helper(h.clone()), nfuncs);
        nfuncs += 1;
        bodies.push(c);
    }
    for (k, c) in codes {
        func_idx.insert(Sym::Inst(*k), nfuncs);
        nfuncs += 1;
        bodies.push(c);
    }
    // Globals: one lazily filled cell per literal (wasm-layout.md §15.4).
    let mut globals = GlobalSection::new();
    let mut lit_global: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
    for (i, bytes) in lits.keys().enumerate() {
        let t = types.of(&WTy::Bytes);
        globals.global(
            GlobalType {
                val_type: ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(t),
                }),
                mutable: true,
                shared: false,
            },
            &ConstExpr::ref_null(HeapType::Concrete(t)),
        );
        lit_global.insert(bytes.clone(), u32::try_from(i).expect("globals"));
    }
    let mut funcs = FunctionSection::new();
    let mut code_sec = CodeSection::new();
    let mut declared = BTreeSet::new();
    let helper_list: Vec<&Helper> = helpers.keys().collect();
    for (n, c) in bodies.iter().enumerate() {
        let t = types.of(&WTy::Func(c.params.clone(), c.results.clone()));
        funcs.function(t);
        let lit;
        let c = if let Some(Helper::Lit(bytes)) = helper_list.get(n) {
            let (off, len) = lits[bytes];
            lit = rt::lit_code(off, len, lit_global[bytes]);
            &lit
        } else {
            *c
        };
        let mut body = c.body.clone();
        for (at, r) in &c.relocs {
            let v = match r {
                Reloc::Func(s) => {
                    let Some(&f) = func_idx.get(s) else {
                        return unsupported(format!("a relocation with no target: {s:?}"));
                    };
                    if body.get(*at as usize - 1) == Some(&0xd2) {
                        declared.insert(f);
                    }
                    f
                }
                Reloc::Type(t) => types.of(t),
            };
            let mut b = Vec::new();
            padded(&mut b, v);
            body[*at as usize..*at as usize + 5].copy_from_slice(&b);
        }
        code_sec.raw(&body);
    }
    let Some(&main) = func_idx.get(&Sym::Helper(entry.clone())) else {
        return unsupported("the entry wrapper has no code");
    };
    let mut exports = ExportSection::new();
    exports.export("main", ExportKind::Func, main);
    let mut mems = MemorySection::new();
    if !imports.is_empty() {
        mems.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        exports.export("hd.x", ExportKind::Memory, 0);
    }
    let mut elems = ElementSection::new();
    if !declared.is_empty() {
        let fs: Vec<u32> = declared.into_iter().collect();
        elems.declared(Elements::Functions(fs.into()));
    }
    let mut module = Module::new();
    module.section(&types.sec);
    module.section(&imps);
    module.section(&funcs);
    if !imports.is_empty() {
        module.section(&mems);
    }
    module.section(&globals);
    module.section(&exports);
    module.section(&elems);
    module.section(&DataCountSection { count: 1 });
    module.section(&code_sec);
    let mut ds = DataSection::new();
    ds.passive(data);
    module.section(&ds);
    // The dev pipeline's standard `name` section (codegen.md §13.10 step 9).
    let mut fnames = wasm_encoder::NameMap::new();
    let nh = u32::try_from(helpers.len()).expect("helpers");
    let ni = u32::try_from(imports.len()).expect("imports");
    for (k, (module_name, name, _, _)) in imports.iter().enumerate() {
        fnames.append(
            u32::try_from(k).expect("k"),
            &format!("{module_name}/{name}"),
        );
    }
    for (k, h) in helper_list.iter().enumerate() {
        let text = format!("{h:?}");
        let short: String = text.chars().take(60).collect();
        fnames.append(ni + u32::try_from(k).expect("k"), &format!("rt:{short}"));
    }
    for (k, n) in names.iter().enumerate() {
        fnames.append(ni + nh + u32::try_from(k).expect("k"), n);
    }
    let mut ns = wasm_encoder::NameSection::new();
    ns.functions(&fnames);
    module.section(&ns);
    Ok(module.finish())
}
