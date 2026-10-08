//! The function assembler: plain instructions through `wasm-encoder`,
//! relocated immediates (functions, types) as raw opcode bytes plus a
//! 5-byte padded slot (codegen.md §13.8).

use wasm_encoder::{BlockType, HeapType, InstructionSink, MemArg};

use crate::{Code, GSym, Reloc, Sym, VT, WTy, padded};

#[derive(Default)]
pub struct Asm {
    pub out: Vec<u8>,
    pub relocs: Vec<(u32, Reloc)>,
    pub params: Vec<VT>,
    /// Declared locals: every one defaultable, so a local set in one
    /// branch validates when read after it; reads narrow (§15.2).
    pub locals: Vec<VT>,
    /// What each local holds: a non-null reference is narrowed on read.
    logical: Vec<VT>,
    /// What each parameter holds, when it is declared defaultable (a
    /// suspending body's resume arguments, suspension.md §14.1).
    param_logical: Vec<VT>,
}

#[must_use]
pub fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }
}

impl Asm {
    #[must_use]
    pub fn new(params: Vec<VT>) -> Asm {
        Asm {
            params,
            ..Asm::default()
        }
    }
    /// Parameters declared in their defaultable form, read as `logical`.
    #[must_use]
    pub fn new_dflt(logical: Vec<VT>) -> Asm {
        Asm {
            params: logical.iter().map(VT::dflt).collect(),
            param_logical: logical,
            ..Asm::default()
        }
    }
    /// The number of parameters.
    #[must_use]
    pub fn nparams(&self) -> u32 {
        u32::try_from(self.params.len()).expect("params")
    }
    /// The declared (defaultable) type of a local or parameter.
    #[must_use]
    pub fn decl(&self, l: u32) -> VT {
        let np = self.params.len();
        if (l as usize) < np {
            self.params[l as usize].clone()
        } else {
            self.locals[l as usize - np].clone()
        }
    }
    /// Reads a local as declared, never narrowed.
    pub fn raw_get(&mut self, l: u32) {
        self.s().local_get(l);
    }
    pub fn global_get(&mut self, g: GSym) {
        self.out.push(0x23);
        self.slot(Reloc::Global(g));
    }
    pub fn global_set(&mut self, g: GSym) {
        self.out.push(0x24);
        self.slot(Reloc::Global(g));
    }
    pub fn s(&mut self) -> InstructionSink<'_> {
        InstructionSink::new(&mut self.out)
    }
    fn slot(&mut self, r: Reloc) {
        self.relocs
            .push((u32::try_from(self.out.len()).expect("offset"), r));
        padded(&mut self.out, 0);
    }
    fn gc(&mut self, op: u8) {
        self.out.push(0xfb);
        self.out.push(op);
    }
    /// A fresh local of type `vt`.
    pub fn local(&mut self, vt: VT) -> u32 {
        self.locals.push(vt.dflt());
        self.logical.push(vt);
        u32::try_from(self.params.len() + self.locals.len() - 1).expect("locals")
    }
    pub fn get(&mut self, l: u32) {
        self.s().local_get(l);
        let np = self.params.len();
        let narrow = if (l as usize) >= np {
            matches!(self.logical.get(l as usize - np), Some(VT::Ref(_, false)))
        } else {
            matches!(self.param_logical.get(l as usize), Some(VT::Ref(_, false)))
        };
        if narrow {
            self.s().ref_as_non_null();
        }
    }
    pub fn set(&mut self, l: u32) {
        self.s().local_set(l);
    }
    pub fn i32(&mut self, v: i32) {
        self.s().i32_const(v);
    }
    pub fn i64(&mut self, v: i64) {
        self.s().i64_const(v);
    }
    pub fn block(&mut self) {
        self.s().block(BlockType::Empty);
    }
    pub fn loop_(&mut self) {
        self.s().loop_(BlockType::Empty);
    }
    pub fn if_(&mut self) {
        self.s().if_(BlockType::Empty);
    }
    pub fn else_(&mut self) {
        self.s().else_();
    }
    pub fn end(&mut self) {
        self.s().end();
    }
    pub fn br(&mut self, d: u32) {
        self.s().br(d);
    }
    pub fn br_if(&mut self, d: u32) {
        self.s().br_if(d);
    }
    pub fn call(&mut self, s: Sym) {
        self.out.push(0x10);
        self.slot(Reloc::Func(s));
    }
    pub fn ref_func(&mut self, s: Sym) {
        self.out.push(0xd2);
        self.slot(Reloc::Func(s));
    }
    pub fn call_ref(&mut self, t: &WTy) {
        self.out.push(0x14);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn struct_new(&mut self, t: &WTy) {
        self.gc(0x00);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn struct_new_default(&mut self, t: &WTy) {
        self.gc(0x01);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn struct_get(&mut self, t: &WTy, f: u32) {
        self.gc(0x02);
        self.slot(Reloc::Type(t.clone()));
        wasm_encoder::Encode::encode(&f, &mut self.out);
    }
    pub fn struct_set(&mut self, t: &WTy, f: u32) {
        self.gc(0x05);
        self.slot(Reloc::Type(t.clone()));
        wasm_encoder::Encode::encode(&f, &mut self.out);
    }
    pub fn array_new_default(&mut self, t: &WTy) {
        self.gc(0x07);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn array_new_fixed(&mut self, t: &WTy, n: u32) {
        self.gc(0x08);
        self.slot(Reloc::Type(t.clone()));
        wasm_encoder::Encode::encode(&n, &mut self.out);
    }
    pub fn array_new_data(&mut self, t: &WTy, seg: u32) {
        self.gc(0x09);
        self.slot(Reloc::Type(t.clone()));
        wasm_encoder::Encode::encode(&seg, &mut self.out);
    }
    /// `array.get` (`array.get_u` for bytes).
    pub fn array_get(&mut self, t: &WTy) {
        self.gc(if *t == WTy::Bytes { 0x0d } else { 0x0b });
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn array_set(&mut self, t: &WTy) {
        self.gc(0x0e);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn array_len(&mut self) {
        self.gc(0x0f);
    }
    pub fn array_copy(&mut self, dst: &WTy, src: &WTy) {
        self.gc(0x11);
        self.slot(Reloc::Type(dst.clone()));
        self.slot(Reloc::Type(src.clone()));
    }
    pub fn ref_cast(&mut self, t: &WTy, nullable: bool) {
        self.gc(if nullable { 0x17 } else { 0x16 });
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn ref_null(&mut self, t: &WTy) {
        self.out.push(0xd0);
        self.slot(Reloc::Type(t.clone()));
    }
    pub fn ref_null_eq(&mut self) {
        self.s().ref_null(HeapType::Abstract {
            shared: false,
            ty: wasm_encoder::AbstractHeapType::Eq,
        });
    }
    /// The default value of a defaultable Wasm value (§15.2).
    pub fn zero(&mut self, v: &VT) {
        match v {
            VT::I32 => self.i32(0),
            VT::I64 => self.i64(0),
            VT::F32 => {
                self.s().f32_const(0.0_f32.into());
            }
            VT::F64 => {
                self.s().f64_const(0.0_f64.into());
            }
            VT::Eq => self.ref_null_eq(),
            VT::Ref(t, _) => self.ref_null(t),
        }
    }
    /// Converts the value on top of the stack from `from` to `to`: an
    /// erased or nullable reference is cast or narrowed; widening is free.
    pub fn conv(&mut self, from: &VT, to: &VT) {
        match (from, to) {
            (a, b) if a == b => {}
            (VT::Eq | VT::Ref(..), VT::Ref(t, n)) if !matches!(from, VT::Ref(f, _) if f == t) => {
                self.ref_cast(t, *n);
            }
            (VT::Ref(_, true), VT::Ref(_, false)) => {
                self.s().ref_as_non_null();
            }
            _ => {}
        }
    }
    pub fn mem8_load(&mut self, off: u64) {
        self.s().i32_load8_u(mem(off));
    }
    pub fn mem8_store(&mut self, off: u64) {
        self.s().i32_store8(mem(off));
    }

    /// The finished code entry: the locals header, then the body.
    #[must_use]
    pub fn finish(mut self, results: Vec<VT>) -> Code {
        self.s().end();
        let mut head = Vec::new();
        let mut relocs = Vec::new();
        let n = u32::try_from(self.locals.len()).expect("locals");
        wasm_encoder::Encode::encode(&n, &mut head);
        for l in &self.locals {
            head.push(1);
            match l {
                VT::I32 => head.push(0x7f),
                VT::I64 => head.push(0x7e),
                VT::F32 => head.push(0x7d),
                VT::F64 => head.push(0x7c),
                VT::Eq => head.push(0x6d),
                VT::Ref(t, nullable) => {
                    head.push(if *nullable { 0x63 } else { 0x64 });
                    relocs.push((
                        u32::try_from(head.len()).expect("offset"),
                        Reloc::Type((**t).clone()),
                    ));
                    padded(&mut head, 0);
                }
            }
        }
        let shift = u32::try_from(head.len()).expect("offset");
        relocs.extend(self.relocs.into_iter().map(|(at, r)| (at + shift, r)));
        head.extend(self.out);
        Code {
            params: self.params,
            results,
            body: head,
            relocs,
            parts: Vec::new(),
        }
    }
}
