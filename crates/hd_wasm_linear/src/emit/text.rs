//! Strings (`crates/hd_wasm_linear/DESIGN.md` §23.7, job 7). A string is a view,
//! its bytes' address and length: a literal's bytes are static data, a
//! built string's are a counted byte buffer ([`Helper::Str`]), and every
//! empty string is the one static empty value ([`EMPTY`]). A slice is a
//! new view of the same bytes; nothing is copied.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_tir::ir::{NONE, PlaceRoot, Proj, ReadMode, Ref, Tag};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::{CountPart, Em};
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
        if let [left, right] = parts {
            if let Some(args) = self.appended_place(i, *left, *right)? {
                for local in args {
                    self.f.get(local);
                }
                self.f.helper(Helper::StrAppend);
                return self.store_owned(i);
            }
            let moved = if self.take(*left) {
                Some(Vec::new())
            } else {
                self.appended_local(i, *left)?
            };
            if let Some(locals) = moved {
                self.comp(*left, 0)?;
                self.comp(*left, 1)?;
                self.comp(*right, 0)?;
                self.comp(*right, 1)?;
                self.f.helper(Helper::StrAppend);
                // The helper took the old count. The following store sees
                // zeros, so it only installs the helper's owned result.
                for local in locals {
                    self.f.zero(local);
                }
                return self.store_owned(i);
            }
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
        // Literal text among the parts already gives the result bytes, so
        // its length is never 0 and no empty test is needed.
        if self.fixed_len(parts) > 0 {
            self.f.get(len);
            self.f.helper(Helper::Str);
            self.f.set(at);
        } else {
            self.new_text(len, at);
        }
        self.f.get(at);
        self.f.set(to);
        for p in parts {
            self.f.get(to);
            self.comp(*p, 0)?;
            self.comp(*p, 1)?;
            self.f.push(crate::bulk::copy(HEAP, HEAP));
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

    /// Transfer the destination's count for a read/append/store of the
    /// same reference or cell place. Do this only across straight-line
    /// reads and arithmetic: a call or write can replace the destination
    /// after the left operand has already read its old value.
    fn appended_place(&mut self, i: u32, left: u32, right: u32) -> StageResult<Option<Vec<u32>>> {
        let Some(read) = Ref(left).as_inst().map(|r| r.0 as usize) else {
            return Ok(None);
        };
        let store = self.consumer.get(i as usize).copied().unwrap_or(NONE);
        if store == NONE
            || self.b.tags[read] != Tag::Read
            || self.b.data[read][1] == ReadMode::Move as u32
            || self.value_uses.get(read) != Some(&1)
            || self.b.tags[store as usize] != Tag::Store
        {
            return Ok(None);
        }
        let p = self.place(self.b.data[read][0])?;
        if p != self.place(self.b.data[store as usize][0])? {
            return Ok(None);
        }
        let blk = self.topo.blk[read];
        if blk == NONE || self.topo.blk[i as usize] != blk || self.topo.blk[store as usize] != blk {
            return Ok(None);
        }
        let (a, b, c) = (
            self.topo.pos[read] as usize,
            self.topo.pos[i as usize] as usize,
            self.topo.pos[store as usize] as usize,
        );
        let statements = self.b.record(self.b.data[blk as usize][0]);
        if a >= b
            || b >= c
            || statements[a + 1..b]
                .iter()
                .filter(|&&s| !hd_tir::ir::count::boundary(self.b, s as usize))
                .any(|&s| match self.b.tags[s as usize] {
                    Tag::Read if self.b.data[s as usize][1] == ReadMode::Move as u32 => {
                        // A move changes its source place too. A distinct
                        // whole local cannot overlap this destination.
                        self.b
                            .local_read(s as usize)
                            .is_none_or(|l| self.is_cell(l.idx()) || p.root == PlaceRoot::Local(l))
                    }
                    Tag::Read | Tag::Coerce | Tag::Prim => false,
                    _ => true,
                })
            || statements[b + 1..c].iter().any(|&s| {
                if hd_tir::ir::count::boundary(self.b, s as usize) {
                    return false;
                }
                !matches!(
                    self.b.tags[s as usize],
                    Tag::Drop | Tag::LoanEnd | Tag::Release
                ) || self
                    .b
                    .decode_place(self.b.data[s as usize][0])
                    .is_none_or(|p| {
                        // Retiring a string operand cannot change the
                        // destination. Dynamic suffixes have their Drop
                        // here too, before the replacement store.
                        !p.proj.is_empty()
                            || !matches!(p.root, PlaceRoot::Value(r)
                                if r.0 != i && self.is_string(self.ty_of(r.0)))
                    })
            })
        {
            return Ok(None);
        }
        let reference = match (p.root, p.proj.as_slice()) {
            (PlaceRoot::Local(l), []) if self.is_cell(l.idx()) => self.cell_of(l.idx())?,
            (PlaceRoot::Local(l), [Proj::Deref]) => self.local_value(l.raw())?[0],
            (PlaceRoot::Value(r), [Proj::Deref]) => {
                let at = self.f.local(Val::I32);
                self.comp(r.0, 0)?;
                self.f.set(at);
                at
            }
            _ => return Ok(None),
        };
        // Preserve a suffix that aliases the left operand before zeroing
        // its SSA locals or moving the destination's count.
        let mut suffix = Vec::new();
        for part in 0..2 {
            let local = self.f.local(Val::I32);
            self.comp(right, part)?;
            self.f.set(local);
            suffix.push(local);
        }
        // Retire the stated copy before moving the place's count. A
        // borrowed read owns nothing; keep its existing slot zeroing.
        if let Some(reason) = self.operation_count(i, Tag::Release, CountPart::Operand(0)) {
            self.because(reason, |em| em.release_value(left))?;
        } else if self.b.is_borrow(left) {
            self.release_value(left)?;
        } else {
            return unsupported("a place append without its stated copied-left cleanup");
        }
        let value = self.ref_layout(Ty::STRING)?;
        let mut args = crate::rt::ref_take(&mut self.f, reference, &value);
        args.extend(suffix);
        Ok(Some(args))
    }

    /// The Wasm locals whose count `left` lends to `i` in `x = x + right`.
    /// The append helper consumes that count before the whole-local store.
    fn appended_local(&mut self, i: u32, left: u32) -> StageResult<Option<Vec<u32>>> {
        let Some(local) = super::shape::read_local(self.b, left) else {
            return Ok(None);
        };
        let Some(&store) = self.consumer.get(i as usize) else {
            return Ok(None);
        };
        if store == NONE {
            return Ok(None);
        }
        let Some((target, value)) = super::shape::local_store(self.b, store as usize) else {
            return Ok(None);
        };
        if target != local || value != i || self.is_cell(local) {
            return Ok(None);
        }
        self.local(u32::try_from(local).expect("local index"))
            .map(Some)
    }

    /// The total length the constant parts of `parts` guarantee: every
    /// part's length is non-negative, so a literal part's bytes are a lower
    /// bound for the whole.
    fn fixed_len(&self, parts: &[u32]) -> usize {
        parts
            .iter()
            .fold(0usize, |n, p| n.saturating_add(self.const_len(*p)))
    }

    /// The length of constant string `r`, or 0 for any other part.
    fn const_len(&self, r: u32) -> usize {
        if Ref(r).as_inst().is_some() {
            return 0;
        }
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        let stripped = hd_mono::layout::strip_newtype(self.pool(), self.env(), self.sub(ty));
        if stripped != Ty::STRING {
            return 0;
        }
        self.b.strings[usize::try_from(bits).expect("string constant")].len()
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
        self.str_slice_offsets(i, text, lo, hi, false, true)
    }

    /// A `string.slice` whose only consumer is string equality: the public
    /// method's range and scalar-boundary checks, then the byte range itself.
    pub(super) fn compared_str_slice(
        &mut self,
        i: u32,
        text: u32,
        start: u32,
        end: u32,
    ) -> StageResult<()> {
        let borrowed = self.loans.contains(&i);
        let moved = !borrowed && self.take(text);
        if !borrowed && !moved {
            self.comp(text, 0)?;
            self.f.helper(Helper::RetainKnown);
        }
        let (lo, hi) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(start, 0)?;
        self.f.set(lo);
        self.comp(end, 0)?;
        self.f.set(hi);
        self.str_slice_offsets(i, text, lo, hi, true, !borrowed)
    }

    fn str_slice_offsets(
        &mut self,
        i: u32,
        text: u32,
        lo: u32,
        hi: u32,
        scalar_boundaries: bool,
        owned: bool,
    ) -> StageResult<()> {
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
        if scalar_boundaries {
            for offset in [lo, hi] {
                // The bounds check makes `offset <= len`; only a UTF-8
                // continuation byte at a smaller offset is not a boundary.
                self.f.get(offset);
                self.comp(text, 1)?;
                self.f.op(I::I32Ne);
                self.f.op(I::If(BlockType::Empty));
                self.comp(text, 0)?;
                self.f.get(offset);
                self.f.op(I::I32Add);
                self.f.op(I::I32Load8U(mem(HEAP, 0, 0)));
                let byte = self.f.local(Val::I32);
                self.f.set(byte);
                self.f.get(byte);
                self.f.i32(128);
                self.f.op(I::I32GeU);
                self.f.get(byte);
                self.f.i32(192);
                self.f.op(I::I32LtU);
                self.f.op(I::I32And);
                self.f.op(I::If(BlockType::Empty));
                self.panic("index-out-of-bounds: string slice offset is inside a character");
                self.f.op(I::End);
                self.f.op(I::End);
            }
        }
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
        if owned {
            self.store_owned(i)
        } else {
            self.store(i)
        }
    }

    /// Whether `slice` is a direct call of `string.slice`.
    pub(super) fn is_string_slice_call(&self, slice: u32) -> bool {
        matches!(
            self.calls.get(&slice),
            Some(hd_mono::Target::Call(call))
                if call.kind == hd_mono::TargetKind::Instance
                    && (self.input.path)(call.item) == "std/text/impl string.slice"
        )
    }

    /// Equality of two byte ranges, with no whole-string helper call.
    pub(super) fn str_range_eq(&mut self, i: u32, left: u32, right: u32) -> StageResult<()> {
        self.comp(left, 1)?;
        self.comp(right, 1)?;
        self.f.op(I::I32Eq);
        self.f
            .op(I::If(BlockType::Result(wasm_encoder::ValType::I32)));
        self.comp(left, 0)?;
        self.comp(left, 1)?;
        self.comp(right, 0)?;
        self.comp(right, 1)?;
        self.f.helper(Helper::BytesMismatch);
        self.comp(left, 1)?;
        self.f.op(I::I32Eq);
        self.f.op(I::Else);
        self.f.i32(0);
        self.f.op(I::End);
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
