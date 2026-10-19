//! The module under construction: functions with symbolic call targets,
//! the imports they name, globals, the function table and static data
//! (`crates/hd_wasm/DESIGN.md` §23.2, §23.9). [`Module::finish`] numbers
//! imports first, then instances in content order, then helpers in their
//! order, and encodes the module.

use std::collections::{BTreeMap, BTreeSet};

use hd_base::{Hash128, StageResult};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, Instruction, MemorySection,
    MemoryType, NameMap, NameSection, TypeSection, ValType,
};

use crate::rt::Helper;

/// The heap memory (`crates/hd_wasm/DESIGN.md` §23.2); memory 0 is the
/// exchange buffer.
pub const HEAP: u32 = 1;

/// The first address of static data: addresses below it are never used,
/// so 0 stays null.
pub const STATIC_BASE: u32 = 16;

/// A function: an instance by key, a runtime helper, or a host import.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sym {
    Inst(Hash128),
    Helper(Helper),
    Import(Import),
}

/// A host import (`crates/hd_run_wasmtime/DESIGN.md` §17.2): its module,
/// name and signature.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub params: Vec<Val>,
    pub results: Vec<Val>,
}

/// A Wasm value type of this backend: scalars only, since every reference
/// is an `i32` address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Val {
    I32,
    I64,
    F32,
    F64,
}

impl Val {
    #[must_use]
    pub const fn wasm(self) -> ValType {
        match self {
            Val::I32 => ValType::I32,
            Val::I64 => ValType::I64,
            Val::F32 => ValType::F32,
            Val::F64 => ValType::F64,
        }
    }
}

/// A runtime global (`crates/hd_wasm/DESIGN.md` §23.9).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Global {
    /// The forbidden-context counter (`req.drive.block-on.forbidden-contexts`).
    Forbid,
}

impl Global {
    const fn val(&self) -> Val {
        match self {
            Global::Forbid => Val::I32,
        }
    }
}

/// One instruction of a body: a plain one, or one whose immediate is a
/// symbol that [`Module::finish`] resolves.
#[derive(Clone, Debug)]
pub enum Ins {
    Op(Instruction<'static>),
    Call(Sym),
    GlobalGet(Global),
    GlobalSet(Global),
}

/// One function body before numbering.
#[derive(Clone, Debug, Default)]
pub struct Func {
    pub name: String,
    pub params: Vec<Val>,
    pub results: Vec<Val>,
    /// The locals after the parameters.
    pub locals: Vec<Val>,
    pub body: Vec<Ins>,
}

impl Func {
    #[must_use]
    pub fn new(name: impl Into<String>, params: Vec<Val>, results: Vec<Val>) -> Self {
        Func {
            name: name.into(),
            params,
            results,
            ..Func::default()
        }
    }

    pub fn op(&mut self, i: Instruction<'static>) {
        self.body.push(Ins::Op(i));
    }

    pub fn call(&mut self, s: Sym) {
        self.body.push(Ins::Call(s));
    }

    pub fn i32(&mut self, v: i32) {
        self.op(Instruction::I32Const(v));
    }
}

/// The module under construction.
#[derive(Default)]
pub struct Module {
    /// Defined functions by symbol; a helper is defined when first
    /// requested, so `pending` holds the requested ones not yet built.
    funcs: BTreeMap<Sym, Func>,
    /// Instances in the order they were defined: content order.
    inst_order: Vec<Sym>,
    pending: BTreeSet<Helper>,
    /// Static data from [`STATIC_BASE`].
    data: Vec<u8>,
    exports: Vec<(String, Sym)>,
}

impl Module {
    /// Defines `s` as `f`.
    pub fn define(&mut self, s: Sym, f: Func) {
        if matches!(s, Sym::Inst(_)) {
            self.inst_order.push(s.clone());
        }
        self.funcs.insert(s, f);
    }

    /// Asks for helper `h`, built by [`Module::finish`]'s fixed point.
    pub fn want(&mut self, h: &Helper) {
        if !self.funcs.contains_key(&Sym::Helper(h.clone())) {
            self.pending.insert(h.clone());
        }
    }

    /// Exports function `s` as `name`.
    pub fn export(&mut self, name: impl Into<String>, s: Sym) {
        if let Sym::Helper(h) = &s {
            self.want(h);
        }
        self.exports.push((name.into(), s));
    }

    /// Builds the requested helpers to a fixed point, numbers every
    /// function and encodes the module.
    pub fn finish(mut self) -> StageResult<Vec<u8>> {
        while let Some(h) = self.pending.pop_first() {
            let f = crate::rt::helper(&h, &mut self);
            self.funcs.insert(Sym::Helper(h), f);
        }
        // Every symbol a body calls: the imports among them come first.
        let mut imports: BTreeSet<Import> = BTreeSet::new();
        let mut globals: BTreeSet<Global> = BTreeSet::new();
        for f in self.funcs.values() {
            for i in &f.body {
                match i {
                    Ins::Call(Sym::Import(im)) => {
                        imports.insert(im.clone());
                    }
                    Ins::GlobalGet(g) | Ins::GlobalSet(g) => {
                        globals.insert(g.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut index: BTreeMap<Sym, u32> = BTreeMap::new();
        let mut next = 0u32;
        for im in &imports {
            index.insert(Sym::Import(im.clone()), next);
            next += 1;
        }
        let mut defined: Vec<Sym> = self.inst_order.clone();
        defined.extend(
            self.funcs
                .keys()
                .filter(|s| matches!(s, Sym::Helper(_)))
                .cloned(),
        );
        for s in &defined {
            index.insert(s.clone(), next);
            next += 1;
        }
        let gindex: BTreeMap<Global, u32> = globals
            .iter()
            .enumerate()
            .map(|(k, g)| (g.clone(), u32::try_from(k).expect("globals")))
            .collect();

        let mut types = TypeSection::new();
        let mut type_of: BTreeMap<(Vec<Val>, Vec<Val>), u32> = BTreeMap::new();
        let mut ty = |params: &[Val], results: &[Val], types: &mut TypeSection| {
            let key = (params.to_vec(), results.to_vec());
            if let Some(t) = type_of.get(&key) {
                return *t;
            }
            let t = u32::try_from(type_of.len()).expect("types");
            types.ty().function(
                params.iter().map(|v| v.wasm()),
                results.iter().map(|v| v.wasm()),
            );
            type_of.insert(key, t);
            t
        };
        let mut import_sec = ImportSection::new();
        for im in &imports {
            let t = ty(&im.params, &im.results, &mut types);
            import_sec.import(&im.module, &im.name, EntityType::Function(t));
        }
        let mut funcs = FunctionSection::new();
        let mut code = CodeSection::new();
        let mut names = NameMap::new();
        for im in &imports {
            names.append(
                index[&Sym::Import(im.clone())],
                &format!("{}/{}", im.module, im.name),
            );
        }
        for s in &defined {
            let f = &self.funcs[s];
            funcs.function(ty(&f.params, &f.results, &mut types));
            let mut body = Function::new_with_locals_types(f.locals.iter().map(|v| v.wasm()));
            for i in &f.body {
                match i {
                    Ins::Op(op) => body.instruction(op),
                    Ins::Call(c) => {
                        let Some(at) = index.get(c) else {
                            return Err(hd_base::NotImplemented::bug(
                                hd_base::Stage::Link,
                                format!("the linear backend calls an undefined function {c:?}"),
                            ));
                        };
                        body.instruction(&Instruction::Call(*at))
                    }
                    Ins::GlobalGet(g) => body.instruction(&Instruction::GlobalGet(gindex[g])),
                    Ins::GlobalSet(g) => body.instruction(&Instruction::GlobalSet(gindex[g])),
                };
            }
            body.instruction(&Instruction::End);
            code.function(&body);
            names.append(index[s], &f.name);
        }
        let mut memories = MemorySection::new();
        // Memory 0: the exchange buffer, grown by the host's results.
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        // Memory 1: the heap, whose first page holds static data.
        let heap_pages = (u64::from(STATIC_BASE) + self.data.len() as u64).div_ceil(1 << 16);
        memories.memory(MemoryType {
            minimum: heap_pages.max(1),
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        let mut global_sec = GlobalSection::new();
        for g in &globals {
            let v = g.val();
            let init = match v {
                Val::I32 => ConstExpr::i32_const(0),
                Val::I64 => ConstExpr::i64_const(0),
                Val::F32 => ConstExpr::f32_const(wasm_encoder::Ieee32::from(0.0)),
                Val::F64 => ConstExpr::f64_const(wasm_encoder::Ieee64::from(0.0)),
            };
            global_sec.global(
                GlobalType {
                    val_type: v.wasm(),
                    mutable: true,
                    shared: false,
                },
                &init,
            );
        }
        let mut exports = ExportSection::new();
        for (name, s) in &self.exports {
            exports.export(name, ExportKind::Func, index[s]);
        }
        exports.export("hd.x", ExportKind::Memory, 0);
        let mut data = DataSection::new();
        if !self.data.is_empty() {
            data.active(
                HEAP,
                &ConstExpr::i32_const(STATIC_BASE.cast_signed()),
                self.data.iter().copied(),
            );
        }
        let mut module = wasm_encoder::Module::new();
        module.section(&types);
        if !imports.is_empty() {
            module.section(&import_sec);
        }
        module.section(&funcs);
        module.section(&memories);
        if !globals.is_empty() {
            module.section(&global_sec);
        }
        module.section(&exports);
        module.section(&code);
        if !self.data.is_empty() {
            module.section(&data);
        }
        let mut name_sec = NameSection::new();
        name_sec.functions(&names);
        module.section(&name_sec);
        Ok(module.finish())
    }
}
