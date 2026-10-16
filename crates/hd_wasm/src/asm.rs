//! The function assembler: plain instructions through `wasm-encoder`,
//! relocated immediates (functions, types) as raw opcode bytes plus a
//! 5-byte padded slot (`crates/hd_mono/DESIGN.md` §13.8).

use wasm_encoder::{BlockType, HeapType, InstructionSink, MemArg};

use crate::{Code, GSym, Reloc, Sym, VT, WTy, padded};

#[derive(Default)]
pub struct Asm {
    pub out: Vec<u8>,
    pub relocs: Vec<(u32, Reloc)>,
    pub params: Vec<VT>,
    /// Declared locals: defaultable, so a local set in one branch
    /// validates when read after it; reads narrow (§15.2). A local from
    /// [`Asm::local_init`] is declared non-null at finish instead.
    pub locals: Vec<VT>,
    /// What each local holds: a non-null reference is narrowed on read.
    logical: Vec<VT>,
    /// Per local, its non-null state (#274).
    nn: Vec<Nn>,
    /// Each read of a non-null local: the offset just after its
    /// `local.get`, where a demoted local's read gets `ref.as_non_null`.
    nn_reads: Vec<(u32, u32)>,
    /// Open blocks: the depth of the innermost, and per depth the
    /// non-null locals first initialized there.
    depth: usize,
    inits: Vec<Vec<u32>>,
    /// Every local is declared defaultable: a suspending body saves and
    /// reloads its locals from defaultable frame fields (§14.2).
    dflt_only: bool,
    /// What each parameter holds, when it is declared defaultable (a
    /// suspending body's resume arguments, `crates/hd_wasm/DESIGN.md` §14.1).
    param_logical: Vec<VT>,
    /// The TIR instruction being emitted, the anchor of each call written
    /// meanwhile; `None` outside an instruction with a source position.
    pub at: Option<u32>,
    /// Per call written at an anchor: its offset and the anchor, the
    /// code entry's `lines` (`crates/hd_wasm/DESIGN.md` §15.9).
    lines: Vec<(u32, u32)>,
}

/// A local's non-null state: [`Asm::local_init`] declares a non-null
/// reference local in its own form, so its reads need no narrowing.
/// Wasm accepts a read of one only where a `local.set` earlier in the
/// same or an enclosing block initialized it; a read anywhere else
/// demotes the local to its defaultable form, and finish narrows each
/// read again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nn {
    /// Defaultable, as declared by [`Asm::local`].
    No,
    /// Non-null; initialized from this depth inward, if at all.
    Live(Option<usize>),
    /// Read before an initialization: defaultable after all.
    Demoted,
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
            dflt_only: true,
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
        let live = self.live_read(l);
        self.s().local_get(l);
        if live {
            let at = u32::try_from(self.out.len()).expect("offset");
            self.nn_reads.push((at, l));
        }
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
    /// Records the call about to be written at the current anchor. A
    /// frame's address maps to the greatest recorded offset at or below it
    /// (`crates/hd_wasm/DESIGN.md` §15.9, Q1), so a call at the anchor of the row
    /// before it needs no row of its own.
    fn line(&mut self) {
        let Some(at) = self.at else {
            return;
        };
        if self.lines.last().is_none_or(|&(_, last)| last != at) {
            self.lines
                .push((u32::try_from(self.out.len()).expect("offset"), at));
        }
    }
    fn gc(&mut self, op: u8) {
        self.out.push(0xfb);
        self.out.push(op);
    }
    /// A fresh local of type `vt`.
    pub fn local(&mut self, vt: VT) -> u32 {
        self.locals.push(vt.dflt());
        self.logical.push(vt);
        self.nn.push(Nn::No);
        u32::try_from(self.params.len() + self.locals.len() - 1).expect("locals")
    }
    /// A fresh local of type `vt`, set from the value on top of the
    /// stack, which must have type `vt`. A non-null reference local is
    /// declared non-null, and its reads in the block of this write need
    /// no `ref.as_non_null` (#274).
    pub fn local_init(&mut self, vt: VT) -> u32 {
        let nn = matches!(vt, VT::Ref(_, false)) && !self.dflt_only;
        let l = self.local(vt);
        if nn {
            *self.nn.last_mut().expect("local") = Nn::Live(None);
        }
        self.set(l);
        l
    }
    /// Locals of `vts`, set from the values on top of the stack, the
    /// last on top: [`Self::local_init`] of each.
    pub fn locals_init(&mut self, vts: &[VT]) -> Vec<u32> {
        let mut ls: Vec<u32> = vts
            .iter()
            .rev()
            .map(|v| self.local_init(v.clone()))
            .collect();
        ls.reverse();
        ls
    }
    /// Makes local `l` defaultable again before a write of a value that
    /// may be null.
    pub fn demote(&mut self, l: u32) {
        if let Some(n) = self.nn_mut(l)
            && matches!(n, Nn::Live(_))
        {
            *n = Nn::Demoted;
        }
    }
    fn nn_mut(&mut self, l: u32) -> Option<&mut Nn> {
        let np = self.params.len();
        (l as usize)
            .checked_sub(np)
            .and_then(|k| self.nn.get_mut(k))
    }
    /// Whether a read of `l` here reads a live non-null local, demoting
    /// it first when no write initializes it here.
    fn live_read(&mut self, l: u32) -> bool {
        let Some(n) = self.nn_mut(l) else {
            return false;
        };
        match *n {
            Nn::Live(Some(_)) => true,
            Nn::Live(None) => {
                *n = Nn::Demoted;
                false
            }
            Nn::No | Nn::Demoted => false,
        }
    }
    pub fn get(&mut self, l: u32) {
        let live = self.live_read(l);
        self.s().local_get(l);
        if live {
            let at = u32::try_from(self.out.len()).expect("offset");
            self.nn_reads.push((at, l));
            return;
        }
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
        let depth = self.depth;
        if let Some(Nn::Live(init)) = self.nn_mut(l)
            && init.is_none_or(|d| d > depth)
        {
            *init = Some(depth);
            if self.inits.len() <= depth {
                self.inits.resize(depth + 1, Vec::new());
            }
            self.inits[depth].push(l);
        }
    }
    /// How many blocks are open around the next instruction.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }
    /// Opens a block level for the init check.
    fn open(&mut self) {
        self.depth += 1;
    }
    /// Closes the innermost block level: the locals first initialized in
    /// it are uninitialized again.
    fn close(&mut self) {
        let depth = self.depth;
        if let Some(ls) = self.inits.get_mut(depth) {
            for l in std::mem::take(ls) {
                if let Some(Nn::Live(init)) = self.nn_mut(l)
                    && *init == Some(depth)
                {
                    *init = None;
                }
            }
        }
    }
    pub fn i32(&mut self, v: i32) {
        self.s().i32_const(v);
    }
    pub fn i64(&mut self, v: i64) {
        self.s().i64_const(v);
    }
    pub fn block(&mut self) {
        self.s().block(BlockType::Empty);
        self.open();
    }
    pub fn loop_(&mut self) {
        self.s().loop_(BlockType::Empty);
        self.open();
    }
    pub fn if_(&mut self) {
        self.if_ty(BlockType::Empty);
    }
    /// An `if` of block type `bt`.
    pub fn if_ty(&mut self, bt: BlockType) {
        self.s().if_(bt);
        self.open();
    }
    /// `else`: Wasm resets the locals initialized in the `if` arm.
    pub fn else_(&mut self) {
        self.s().else_();
        self.close();
    }
    pub fn end(&mut self) {
        self.s().end();
        self.close();
        self.depth = self.depth.saturating_sub(1);
    }
    pub fn br(&mut self, d: u32) {
        self.s().br(d);
    }
    pub fn br_if(&mut self, d: u32) {
        self.s().br_if(d);
    }
    pub fn call(&mut self, s: Sym) {
        self.line();
        self.out.push(0x10);
        self.slot(Reloc::Func(s));
    }
    /// A string literal: its number in the module's literal pool, which
    /// link assigns, then a call of the pool's getter.
    pub fn lit(&mut self, bytes: &[u8]) {
        self.out.push(0x41);
        self.slot(Reloc::Lit(bytes.to_vec()));
        self.call(Sym::Helper(crate::Helper::Lit));
    }
    pub fn ref_func(&mut self, s: Sym) {
        self.out.push(0xd2);
        self.slot(Reloc::Func(s));
    }
    pub fn call_ref(&mut self, t: &WTy) {
        self.line();
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
            VT::Func => {
                self.s().ref_null(HeapType::FUNC);
            }
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
    /// Declares each live non-null local non-null, and narrows each read
    /// of a demoted one with `ref.as_non_null`, shifting the offsets
    /// recorded after it.
    fn narrow_demoted(&mut self) {
        let np = self.params.len();
        for (k, n) in self.nn.iter().enumerate() {
            if matches!(n, Nn::Live(_)) {
                self.locals[k] = self.logical[k].clone();
            }
        }
        let at: Vec<u32> = self
            .nn_reads
            .iter()
            .filter(|(_, l)| self.nn[*l as usize - np] == Nn::Demoted)
            .map(|(at, _)| *at)
            .collect();
        if at.is_empty() {
            return;
        }
        let mut out = Vec::with_capacity(self.out.len() + at.len());
        let mut from = 0;
        for &p in &at {
            out.extend_from_slice(&self.out[from..p as usize]);
            out.push(0xd4);
            from = p as usize;
        }
        out.extend_from_slice(&self.out[from..]);
        self.out = out;
        let shift = |o: u32| o + u32::try_from(at.partition_point(|&p| p <= o)).expect("reads");
        for (o, _) in &mut self.relocs {
            *o = shift(*o);
        }
        for (o, _) in &mut self.lines {
            *o = shift(*o);
        }
    }
    pub fn mem8_load(&mut self, off: u64) {
        self.s().i32_load8_u(mem(off));
    }
    pub fn mem8_store(&mut self, off: u64) {
        self.s().i32_store8(mem(off));
    }

    /// The instructions written, with their relocations, as a constant
    /// expression's body: no locals and no closing `end`.
    #[must_use]
    pub fn into_const(self) -> crate::ConstInit {
        crate::ConstInit {
            body: self.out,
            relocs: self.relocs,
        }
    }
    /// The finished code entry: the locals header, then the body.
    #[must_use]
    pub fn finish(mut self, results: Vec<VT>) -> Code {
        self.s().end();
        self.narrow_demoted();
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
                VT::Func => head.push(0x70),
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
            lines: self
                .lines
                .into_iter()
                .map(|(off, at)| (off + shift, at))
                .collect(),
            parts: Vec::new(),
        }
    }
}
