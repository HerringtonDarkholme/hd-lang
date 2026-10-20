//! Scalar-only assembler for the portable floating-point helpers.
use crate::module::Val;
use wasm_encoder::{BlockType, Function, InstructionSink};

pub struct Asm {
    params: Vec<Val>,
    locals: Vec<Val>,
    out: Vec<u8>,
}
impl Asm {
    pub fn new(params: Vec<Val>) -> Self {
        Self {
            params,
            locals: Vec::new(),
            out: Vec::new(),
        }
    }
    pub fn local(&mut self, ty: Val) -> u32 {
        self.locals.push(ty);
        u32::try_from(self.params.len() + self.locals.len() - 1).expect("locals")
    }
    pub fn s(&mut self) -> InstructionSink<'_> {
        InstructionSink::new(&mut self.out)
    }
    pub fn get(&mut self, n: u32) {
        self.s().local_get(n);
    }
    pub fn raw_get(&mut self, n: u32) {
        self.get(n);
    }
    pub fn set(&mut self, n: u32) {
        self.s().local_set(n);
    }
    pub fn i32(&mut self, n: i32) {
        self.s().i32_const(n);
    }
    pub fn i64(&mut self, n: i64) {
        self.s().i64_const(n);
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
    pub fn br(&mut self, n: u32) {
        self.s().br(n);
    }
    pub fn br_if(&mut self, n: u32) {
        self.s().br_if(n);
    }
    pub fn finish(mut self) -> Vec<u8> {
        self.end();
        let mut f = Function::new_with_locals_types(self.locals.into_iter().map(Val::wasm));
        f.raw(self.out);
        f.into_raw_body()
    }
}
