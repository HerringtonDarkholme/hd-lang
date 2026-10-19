//! The module under construction: functions with symbolic call targets,
//! the imports they name, globals, the function table, static vtables
//! and static data (`crates/hd_wasm/DESIGN.md` §23.2, §23.8, §23.9).
//! [`Module::finish`] builds every helper a body names, numbers imports
//! first, then instances in content order, then helpers in their order,
//! and encodes the module.

use std::collections::{BTreeMap, BTreeSet};

use hd_base::{Hash128, NotImplemented, Stage, StageResult};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ElementSection, Elements, EntityType, ExportKind,
    ExportSection, Function, FunctionSection, GlobalSection, GlobalType, ImportSection,
    Instruction, MemorySection, MemoryType, NameMap, NameSection, RefType, TableSection, TableType,
    TypeSection, ValType,
};

use crate::rt::Helper;

/// The exchange buffer `hd.x`, the host contract's memory
/// (`crates/hd_run_wasmtime/DESIGN.md` §17.3).
pub const BUF: u32 = 0;

/// The heap memory (`crates/hd_wasm/DESIGN.md` §23.2).
pub const HEAP: u32 = 1;

/// The first address of static data: addresses below it are never used,
/// so 0 stays null.
pub const STATIC_BASE: u32 = 16;

/// The address of the one empty string (§23.7): every string of no bytes
/// is this view, whose bytes are never read.
pub const EMPTY: u32 = STATIC_BASE;

/// The size of a counted object's header, before its payload (§23.4).
pub const HEADER: u32 = 8;

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

impl Import {
    #[must_use]
    pub fn new(module: &str, name: &str, params: &[Val], results: &[Val]) -> Self {
        Import {
            module: module.to_owned(),
            name: name.to_owned(),
            params: params.to_vec(),
            results: results.to_vec(),
        }
    }
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

/// A runtime global (`crates/hd_wasm/DESIGN.md` §23.9), or one Wasm
/// value of a module storage binding.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Global {
    /// The forbidden-context counter (`req.drive.block-on.forbidden-contexts`).
    Forbid,
    /// The first free heap address of the bump allocation that serves
    /// until the std allocator (job 5) takes over.
    HeapTop,
    /// The end of the pages that bump allocation owns: at first the heap's
    /// initial size, then the end of the pages it last grew.
    HeapEnd,
    /// One value of a top-level binding: its item's path hash and the
    /// value's index in its layout.
    Storage(Hash128, u32, Val),
}

impl Global {
    const fn val(&self) -> Val {
        match self {
            Global::Forbid | Global::HeapTop | Global::HeapEnd => Val::I32,
            Global::Storage(_, _, v) => *v,
        }
    }
}

/// One instruction of a body: a plain one, or one whose immediate
/// [`Module::finish`] resolves.
#[derive(Clone, Debug)]
pub enum Ins {
    Op(Instruction<'static>),
    Call(Sym),
    GlobalGet(Global),
    GlobalSet(Global),
    /// `i32.const` of the function's slot in the function table.
    TableSlot(Sym),
    /// `call_indirect` through the function table, at this signature.
    CallIndirect(Vec<Val>, Vec<Val>),
    /// `i32.const` of a static vtable's address ([`Module::vtable`]).
    VTable(usize),
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

    /// A fresh local of type `v`: its index.
    pub fn local(&mut self, v: Val) -> u32 {
        self.locals.push(v);
        u32::try_from(self.params.len() + self.locals.len() - 1).expect("locals")
    }

    pub fn op(&mut self, i: Instruction<'static>) {
        self.body.push(Ins::Op(i));
    }

    pub fn call(&mut self, s: Sym) {
        self.body.push(Ins::Call(s));
    }

    pub fn helper(&mut self, h: Helper) {
        self.call(Sym::Helper(h));
    }

    pub fn i32(&mut self, v: i32) {
        self.op(Instruction::I32Const(v));
    }

    /// An address or a length: the bits of `v` as an `i32` immediate.
    pub fn u32(&mut self, v: u32) {
        self.op(Instruction::I32Const(v.cast_signed()));
    }

    pub fn get(&mut self, l: u32) {
        self.op(Instruction::LocalGet(l));
    }

    pub fn set(&mut self, l: u32) {
        self.op(Instruction::LocalSet(l));
    }

    pub fn global_get(&mut self, g: Global) {
        self.body.push(Ins::GlobalGet(g));
    }

    pub fn global_set(&mut self, g: Global) {
        self.body.push(Ins::GlobalSet(g));
    }
}

/// A memory argument of `memory` with no offset, aligned to `align`
/// bytes' log.
#[must_use]
pub const fn mem(memory: u32, offset: u64, align: u32) -> wasm_encoder::MemArg {
    wasm_encoder::MemArg {
        offset,
        align,
        memory_index: memory,
    }
}

/// The module under construction.
#[derive(Default)]
pub struct Module {
    /// Defined functions by symbol. A helper is defined when
    /// [`Module::finish`] finds a body naming it.
    funcs: BTreeMap<Sym, Func>,
    /// Instances in the order they were defined: content order.
    inst_order: Vec<Sym>,
    /// Static data from [`STATIC_BASE`], deduplicated by content.
    data: Vec<u8>,
    data_at: BTreeMap<Vec<u8>, u32>,
    /// Static vtables: each one's slots, laid out after the data.
    vtables: Vec<Vec<Sym>>,
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

    /// Exports function `s` as `name`.
    pub fn export(&mut self, name: impl Into<String>, s: Sym) {
        self.exports.push((name.into(), s));
    }

    /// The heap address of `bytes` in static data, placed on first use
    /// and aligned to `align`.
    pub fn static_bytes(&mut self, bytes: &[u8], align: u32) -> u32 {
        if let Some(at) = self.data_at.get(bytes) {
            return *at;
        }
        let align = align.max(1) as usize;
        while !self.data.len().is_multiple_of(align) {
            self.data.push(0);
        }
        let at = STATIC_BASE + u32::try_from(self.data.len()).expect("static data");
        self.data.extend_from_slice(bytes);
        self.data_at.insert(bytes.to_vec(), at);
        at
    }

    /// The number of a static vtable whose slots are `slots`' table
    /// indices, one `i32` each; [`Ins::VTable`] names its address.
    pub fn vtable(&mut self, slots: Vec<Sym>) -> usize {
        if let Some(k) = self.vtables.iter().position(|v| *v == slots) {
            return k;
        }
        self.vtables.push(slots);
        self.vtables.len() - 1
    }

    /// The helpers that bodies and vtables name and nobody defined yet.
    fn missing(&self) -> BTreeSet<Helper> {
        let mut out = BTreeSet::new();
        let mut note = |s: &Sym| {
            if let Sym::Helper(h) = s
                && !self.funcs.contains_key(s)
            {
                out.insert(h.clone());
            }
        };
        for f in self.funcs.values() {
            for i in &f.body {
                if let Ins::Call(s) | Ins::TableSlot(s) = i {
                    note(s);
                }
            }
        }
        for s in self.vtables.iter().flatten() {
            note(s);
        }
        for (_, s) in &self.exports {
            note(s);
        }
        out
    }

    /// Builds the named helpers to a fixed point, numbers every function
    /// and encodes the module.
    pub fn finish(mut self) -> StageResult<Vec<u8>> {
        loop {
            let missing = self.missing();
            if missing.is_empty() {
                break;
            }
            for h in missing {
                let f = crate::rt::helper(&h, &mut self);
                self.funcs.insert(Sym::Helper(h), f);
            }
        }
        // Every import and global a body names; imports come first.
        let mut imports: BTreeSet<Import> = BTreeSet::new();
        let mut globals: BTreeSet<Global> = BTreeSet::new();
        for f in self.funcs.values() {
            for i in &f.body {
                match i {
                    Ins::Call(Sym::Import(im)) | Ins::TableSlot(Sym::Import(im)) => {
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
        let undefined = |s: &Sym| {
            NotImplemented::bug(
                Stage::Link,
                format!("the linear backend names an undefined function {s:?}"),
            )
        };
        // The function table: every function a slot or a vtable names,
        // in the order first named.
        let mut slots: Vec<u32> = Vec::new();
        let mut slot_of: BTreeMap<Sym, u32> = BTreeMap::new();
        let mut slot = |s: &Sym, slots: &mut Vec<u32>| -> StageResult<u32> {
            if let Some(k) = slot_of.get(s) {
                return Ok(*k);
            }
            let f = *index.get(s).ok_or_else(|| undefined(s))?;
            let k = u32::try_from(slots.len()).expect("table");
            slots.push(f);
            slot_of.insert(s.clone(), k);
            Ok(k)
        };
        // Vtables follow the data, one `i32` slot index each.
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut vtable_at = Vec::new();
        for v in &self.vtables {
            vtable_at.push(STATIC_BASE + u32::try_from(self.data.len()).expect("static data"));
            for s in v {
                let k = slot(s, &mut slots)?;
                self.data.extend_from_slice(&k.to_le_bytes());
            }
        }
        let static_end = STATIC_BASE + u32::try_from(self.data.len()).expect("static data");
        let heap_base = static_end.next_multiple_of(8);
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
                    Ins::Call(c) => body.instruction(&Instruction::Call(
                        *index.get(c).ok_or_else(|| undefined(c))?,
                    )),
                    Ins::GlobalGet(g) => body.instruction(&Instruction::GlobalGet(gindex[g])),
                    Ins::GlobalSet(g) => body.instruction(&Instruction::GlobalSet(gindex[g])),
                    Ins::TableSlot(t) => {
                        let k = slot(t, &mut slots)?;
                        body.instruction(&Instruction::I32Const(k.cast_signed()))
                    }
                    Ins::CallIndirect(p, r) => body.instruction(&Instruction::CallIndirect {
                        type_index: ty(p, r, &mut types),
                        table_index: 0,
                    }),
                    Ins::VTable(k) => {
                        body.instruction(&Instruction::I32Const(vtable_at[*k].cast_signed()))
                    }
                };
            }
            body.instruction(&Instruction::End);
            code.function(&body);
            names.append(index[s], &f.name);
        }
        let mut tables = TableSection::new();
        let mut elements = ElementSection::new();
        if !slots.is_empty() {
            let n = u64::try_from(slots.len()).expect("table");
            tables.table(TableType {
                element_type: RefType::FUNCREF,
                table64: false,
                minimum: n,
                maximum: Some(n),
                shared: false,
            });
            elements.active(
                None,
                &ConstExpr::i32_const(0),
                Elements::Functions(slots.as_slice().into()),
            );
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
        // Memory 1: the heap, whose first pages hold static data.
        let heap_pages = heap_base.div_ceil(1 << 16).max(1);
        memories.memory(MemoryType {
            minimum: u64::from(heap_pages),
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        let mut global_sec = GlobalSection::new();
        for g in &globals {
            let v = g.val();
            let init = match (g, v) {
                (Global::HeapTop, _) => ConstExpr::i32_const(heap_base.cast_signed()),
                (Global::HeapEnd, _) => ConstExpr::i32_const((heap_pages << 16).cast_signed()),
                (_, Val::I32) => ConstExpr::i32_const(0),
                (_, Val::I64) => ConstExpr::i64_const(0),
                (_, Val::F32) => ConstExpr::f32_const(wasm_encoder::Ieee32::from(0.0)),
                (_, Val::F64) => ConstExpr::f64_const(wasm_encoder::Ieee64::from(0.0)),
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
            exports.export(
                name,
                ExportKind::Func,
                *index.get(s).ok_or_else(|| undefined(s))?,
            );
        }
        exports.export("hd.x", ExportKind::Memory, BUF);
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
        if !slots.is_empty() {
            module.section(&tables);
        }
        module.section(&memories);
        if !globals.is_empty() {
            module.section(&global_sec);
        }
        module.section(&exports);
        if !slots.is_empty() {
            module.section(&elements);
        }
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
