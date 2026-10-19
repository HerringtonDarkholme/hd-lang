//! Strings (`crates/hd_wasm/DESIGN.md` §23.7, job 7). A string is a view,
//! its bytes' address and length: a literal's bytes are static data, a
//! built string's are a counted byte buffer ([`Helper::Str`]), and every
//! empty string is the one static empty value ([`EMPTY`]). A slice is a
//! new view of the same bytes; nothing is copied.

use hd_base::StageResult;
use hd_types::{Ty, TyData};
use wasm_encoder::{BlockType, Instruction as I};

use super::Em;
use crate::module::{EMPTY, HEAP, Val, mem};
use crate::rt::{Helper, PrimArg};
use crate::unsupported;

impl Em<'_, '_, '_> {
    /// Whether `t` is `string`, through newtypes and `mut`.
    fn is_string(&self, t: Ty) -> bool {
        crate::layout::strip(self.pool(), self.env(), t) == Ty::STRING
    }

    /// An interpolation or a concatenation of string parts: one part is
    /// itself; more are copied into one new buffer, or are the empty value
    /// when they have no bytes.
    pub(super) fn concat(&mut self, i: u32, parts: &[u32]) -> StageResult<()> {
        for p in parts {
            if !self.is_string(self.ty_of(*p)) {
                return unsupported("an interpolation part that is not a string");
            }
        }
        match parts {
            [] => {
                self.f.u32(EMPTY);
                self.f.i32(0);
                return self.store(i);
            }
            [one] => {
                self.push(*one)?;
                return self.store(i);
            }
            _ => {}
        }
        let (len, at, to) = (
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
        );
        self.f.i32(0);
        for p in parts {
            self.comp(*p, 1)?;
            self.f.op(I::I32Add);
        }
        self.f.set(len);
        self.new_text(len, at);
        self.f.get(at);
        self.f.set(to);
        for p in parts {
            self.f.get(to);
            self.comp(*p, 0)?;
            self.comp(*p, 1)?;
            self.f.op(I::MemoryCopy {
                src_mem: HEAP,
                dst_mem: HEAP,
            });
            self.f.get(to);
            self.comp(*p, 1)?;
            self.f.op(I::I32Add);
            self.f.set(to);
        }
        self.f.get(at);
        self.f.get(len);
        // A new buffer arrives with its count.
        self.store_owned(i)
    }

    /// Sets local `at` to the address of a new buffer of the `len` bytes in
    /// local `len`, or to the empty value's when `len` is 0.
    fn new_text(&mut self, len: u32, at: u32) {
        self.f.get(len);
        self.f.op(I::I32Eqz);
        self.f
            .op(I::If(BlockType::Result(wasm_encoder::ValType::I32)));
        self.f.u32(EMPTY);
        self.f.op(I::Else);
        self.f.get(len);
        self.f.helper(Helper::Str);
        self.f.op(I::End);
        self.f.set(at);
    }

    /// `s[i]` (`StrIndex`, `bytes_at`): the byte at `index`, which must be
    /// below the length.
    pub(super) fn str_at(&mut self, i: u32, text: u32, index: u32) -> StageResult<()> {
        let ix = self.f.local(Val::I32);
        self.comp(index, 0)?;
        self.f.set(ix);
        self.f.get(ix);
        self.comp(text, 1)?;
        self.f.op(I::I32GeU);
        self.f.op(I::If(BlockType::Empty));
        self.panic("index-out-of-bounds: string index out of bounds");
        self.f.op(I::End);
        self.comp(text, 0)?;
        self.f.get(ix);
        self.f.op(I::I32Add);
        self.f.op(I::I32Load8U(mem(HEAP, 0, 0)));
        self.store(i)
    }

    /// `bytes_slice(text, start, end)`: `start <= end <= len`, then a view
    /// of the same bytes, or the empty value when it has none.
    pub(super) fn str_slice(&mut self, i: u32, text: u32, start: u32, end: u32) -> StageResult<()> {
        let (lo, hi) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(start, 0)?;
        self.f.set(lo);
        self.comp(end, 0)?;
        self.f.set(hi);
        self.f.get(lo);
        self.f.get(hi);
        self.f.op(I::I32GtU);
        self.f.get(hi);
        self.comp(text, 1)?;
        self.f.op(I::I32GtU);
        self.f.op(I::I32Or);
        self.f.op(I::If(BlockType::Empty));
        self.panic("index-out-of-bounds: string slice out of bounds");
        self.f.op(I::End);
        // The address: the empty value's when `lo == hi`.
        self.comp(text, 0)?;
        self.f.get(lo);
        self.f.op(I::I32Add);
        self.f.u32(EMPTY);
        self.f.get(lo);
        self.f.get(hi);
        self.f.op(I::I32Ne);
        self.f.op(I::Select);
        self.f.get(hi);
        self.f.get(lo);
        self.f.op(I::I32Sub);
        self.store(i)
    }

    /// `char_from_scalar(point)`: `.Some(point)` when `point` is a scalar
    /// value (at most `0x10FFFF`, not a surrogate), else `.None`, whose
    /// slot holds zero.
    pub(super) fn char_from_scalar(&mut self, i: u32, point: u32) -> StageResult<()> {
        if self.vals(self.ty(i))? != [Val::I32, Val::I32] {
            return unsupported("a `char?` of this layout");
        }
        let (p, ok) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(point, 0)?;
        self.f.set(p);
        self.f.get(p);
        self.f.i32(0x10_FFFF);
        self.f.op(I::I32LeU);
        self.f.get(p);
        self.f.i32(0xD800);
        self.f.op(I::I32Sub);
        self.f.i32(0x7FF);
        self.f.op(I::I32GtU);
        self.f.op(I::I32And);
        self.f.set(ok);
        self.f.get(ok);
        self.f.get(p);
        self.f.get(ok);
        self.f.op(I::I32Mul);
        self.store(i)
    }

    /// `cmp` or `partial_cmp` of two strings: their bytes in order, a
    /// prefix first (`expr.ord.string`), as an `Ordering`, which
    /// `partial_cmp` wraps in `.Some`.
    pub(super) fn str_ordering(&mut self, i: u32, args: &[u32], partial: bool) -> StageResult<()> {
        let want = if partial { 2 } else { 1 };
        if self.vals(self.ty(i))?.len() != want {
            return unsupported("an `Ordering` of this layout");
        }
        if partial {
            self.f.i32(1);
        }
        self.push(args[0])?;
        self.push(args[1])?;
        self.f.helper(Helper::StrCmp);
        self.f.i32(1);
        self.f.op(I::I32Add);
        self.store(i)
    }

    /// A host primitive (spec/std/README.md, "Standard library
    /// primitives"): the `hd:prim` import `name`. A scalar argument goes in
    /// as itself and a string in the exchange buffer, its length the
    /// parameter; a string result comes back in the exchange buffer and is
    /// copied into a new buffer, and a scalar result is the import's own.
    pub(super) fn host_prim(&mut self, i: u32, name: &str, args: &[u32]) -> StageResult<()> {
        let mut kinds = Vec::new();
        for a in args {
            let t = self.ty_of(*a);
            kinds.push(if self.is_string(t) {
                PrimArg::Str
            } else {
                match self.vals(t)?.as_slice() {
                    [v] => PrimArg::Scalar(*v),
                    _ => {
                        return unsupported(
                            "a host primitive argument that is neither scalar nor string",
                        );
                    }
                }
            });
        }
        let t = self.ty(i);
        let result = if self.is_string(t) {
            None
        } else {
            match self.vals(t)?.as_slice() {
                [v] => Some(*v),
                _ => {
                    return unsupported(
                        "a host primitive result that is neither scalar nor string",
                    );
                }
            }
        };
        if matches!(self.pool().get(t), TyData::Never) {
            return unsupported("a host primitive that does not return");
        }
        // One exchange buffer holds one string argument.
        if kinds.iter().filter(|k| **k == PrimArg::Str).count() > 1 {
            return unsupported("a host primitive of two string arguments");
        }
        for a in args {
            self.push(*a)?;
        }
        self.f.helper(Helper::HostPrim {
            name: name.to_owned(),
            args: kinds,
            result,
        });
        self.store_owned(i)
    }
}
