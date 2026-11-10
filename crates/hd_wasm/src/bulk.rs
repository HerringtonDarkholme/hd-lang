//! One lowering path for bulk copies and fills, including generated runtime
//! code. Constants use exact-width operations; dynamic sizes share a helper
//! per memory pair (Wasm memory indices are instruction immediates).

use wasm_encoder::BlockType;

use crate::module::{Func, Ins, Module, Val, mem};
use crate::op::Op as I;
use crate::rt::Helper;

const SMALL: i32 = 16;

pub fn copy(src_mem: u32, dst_mem: u32) -> Ins {
    Ins::Op(I::MemoryCopy { src_mem, dst_mem })
}

pub fn fill(memory: u32) -> Ins {
    Ins::Op(I::MemoryFill(memory))
}

/// Lower after constant propagation, and again for a wide frame's generated
/// fill, before discovering the requested helpers. Their fallbacks are final.
pub fn lower(f: &mut Func) -> bool {
    if !f.body.iter().enumerate().any(|(k, i)| {
        matches!(i, Ins::Op(I::MemoryCopy { .. } | I::MemoryFill(_)))
            && !matches!(k.checked_sub(1).and_then(|p| f.body.get(p)),
                    Some(Ins::Op(I::I32Const(n))) if !(0..=SMALL).contains(n))
    }) {
        return false;
    }
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let (outer, at) = (f.origin_at, f.at.take());
    let mut scratch = Scratch::default();
    for (k, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[k]);
        let (source, dest, copying) = match ins {
            Ins::Op(I::MemoryCopy { src_mem, dst_mem }) => (src_mem, dst_mem, true),
            Ins::Op(I::MemoryFill(memory)) => (memory, memory, false),
            _ => {
                f.push(ins);
                continue;
            }
        };
        // A known large range already selected the bulk operation. It
        // needs neither a helper call nor a run-time length dispatch.
        if matches!(f.body.last(), Some(Ins::Op(I::I32Const(n))) if !(0..=SMALL).contains(n)) {
            f.push(ins);
            continue;
        }
        if let Some(Ins::Op(I::I32Const(n))) = f.body.last().copied()
            && (0..=SMALL).contains(&n)
        {
            f.remove(f.body.len() - 1);
            inline(f, &mut scratch, n.cast_unsigned(), source, dest, copying);
        } else {
            f.helper(if copying {
                Helper::MemoryCopy { source, dest }
            } else {
                Helper::MemoryFill(dest)
            });
        }
    }
    f.origin_at = outer;
    f.at = at;
    f.rebuild_callees();
    true
}

// These scratch slots are dead at the end of each bulk operation. Reuse
// them across the function instead of running whole-function local cleanup
// again, or adding one set of locals for every short copy.
#[derive(Default)]
struct Scratch {
    words: Vec<u32>,
    longs: Vec<u32>,
}

impl Scratch {
    fn local(&mut self, f: &mut Func, width: u32, slot: usize) -> u32 {
        let (locals, ty) = if width == 8 {
            (&mut self.longs, Val::I64)
        } else {
            (&mut self.words, Val::I32)
        };
        while locals.len() <= slot {
            locals.push(f.local(ty));
        }
        locals[slot]
    }
}

/// The width of one scalar load or store: 1, 2, 4 or 8 bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Width {
    W1 = 1,
    W2 = 2,
    W4 = 4,
    W8 = 8,
}

impl Width {
    /// The width of `n` bytes, when one scalar moves them.
    const fn of(n: u32) -> Option<Self> {
        match n {
            1 => Some(Self::W1),
            2 => Some(Self::W2),
            4 => Some(Self::W4),
            8 => Some(Self::W8),
            _ => None,
        }
    }

    const fn bytes(self) -> u32 {
        self as u32
    }
}

/// `n` bytes as scalar chunks, widest first, with their offsets.
fn widths(mut n: u32) -> Vec<(u32, Width)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for width in [Width::W8, Width::W4, Width::W2, Width::W1] {
        while n >= width.bytes() {
            out.push((offset, width));
            offset += width.bytes();
            n -= width.bytes();
        }
    }
    out
}

fn load(f: &mut Func, memory: u32, offset: u32, width: Width) {
    let arg = mem(memory, u64::from(offset), 0);
    f.op(match width {
        Width::W8 => I::I64Load(arg),
        Width::W4 => I::I32Load(arg),
        Width::W2 => I::I32Load16U(arg),
        Width::W1 => I::I32Load8U(arg),
    });
}

fn store(f: &mut Func, memory: u32, offset: u32, width: Width) {
    let arg = mem(memory, u64::from(offset), 0);
    f.op(match width {
        Width::W8 => I::I64Store(arg),
        Width::W4 => I::I32Store(arg),
        Width::W2 => I::I32Store16(arg),
        Width::W1 => I::I32Store8(arg),
    });
}

fn trap_if(f: &mut Func) {
    f.op(I::If(BlockType::Empty));
    f.op(I::Unreachable);
    f.op(I::End);
}

// Zero-length bulk instructions still validate addresses, including a full
// 4-GiB memory. Do not turn them into an unchecked no-op.
fn zero_on_stack(f: &mut Func, memory: u32) {
    f.op(I::I64ExtendI32U);
    f.op(I::MemorySize(memory));
    f.op(I::I64ExtendI32U);
    f.op(I::I64Const(16));
    f.op(I::I64Shl);
    f.op(I::I64GtU);
    trap_if(f);
}

fn byte_pattern(f: &mut Func, width: u32) {
    if width == 1 {
        // store8 already discards all but the low byte.
        return;
    }
    f.i32(255);
    f.op(I::I32And);
    if width == 8 {
        f.op(I::I64ExtendI32U);
        f.op(I::I64Const(0x0101_0101_0101_0101));
        f.op(I::I64Mul);
    } else {
        f.i32(0x0101_0101);
        f.op(I::I32Mul);
    }
}

fn literal_pattern(f: &mut Func, byte: u8, width: u32) {
    if width == 8 {
        f.op(I::I64Const(u64::from_le_bytes([byte; 8]).cast_signed()));
    } else {
        f.i32(u32::from_le_bytes([byte; 4]).cast_signed());
    }
}

// The stack holds destination, then source/byte. Consume those operands
// directly for a scalar width. Multiword copies keep the first destination
// under all source loads, then write the highest chunk before the rest.
fn inline(f: &mut Func, scratch: &mut Scratch, n: u32, source: u32, dest: u32, copying: bool) {
    let byte = if !copying && let Some(Ins::Op(I::I32Const(byte))) = f.body.last().copied() {
        f.remove(f.body.len() - 1);
        Some(byte.to_le_bytes()[0])
    } else {
        None
    };
    if n == 0 {
        if copying {
            zero_on_stack(f, source);
        } else if byte.is_none() {
            f.op(I::Drop);
        }
        zero_on_stack(f, dest);
        return;
    }
    if let Some(width) = Width::of(n) {
        if copying {
            load(f, source, 0, width);
        } else if let Some(byte) = byte {
            literal_pattern(f, byte, n);
        } else {
            byte_pattern(f, n);
        }
        store(f, dest, 0, width);
        return;
    }
    let chunks = widths(n);
    let to = scratch.local(f, 4, 1);
    if copying {
        let from = scratch.local(f, 4, 0);
        f.set(from);
        f.op(I::LocalTee(to));
        let mut parts = Vec::new();
        let mut word = 2;
        for &(offset, width) in &chunks[..chunks.len() - 1] {
            let value = scratch.local(f, width.bytes(), if width == Width::W8 { 0 } else { word });
            if width != Width::W8 {
                word += 1;
            }
            f.get(from);
            load(f, source, offset, width);
            f.set(value);
            parts.push(value);
        }
        let &(offset, width) = chunks.last().expect("nonempty copy");
        f.get(from);
        load(f, source, offset, width);
        store(f, dest, offset, width);
        for (&(offset, width), value) in chunks[..chunks.len() - 1].iter().zip(parts).rev() {
            f.get(to);
            f.get(value);
            store(f, dest, offset, width);
        }
    } else {
        let wide = n >= 8;
        let value = byte.is_none().then(|| {
            let width = if wide { 8 } else { 4 };
            byte_pattern(f, width);
            let value = scratch.local(f, width, 0);
            f.set(value);
            value
        });
        f.op(I::LocalTee(to));
        for (k, &(offset, width)) in chunks.iter().rev().enumerate() {
            if k != 0 {
                f.get(to);
            }
            if let Some(byte) = byte {
                literal_pattern(f, byte, width.bytes());
            } else {
                f.get(value.expect("fill pattern"));
                if wide && width != Width::W8 {
                    f.op(I::I32WrapI64);
                }
            }
            store(f, dest, offset, width);
        }
    }
}

// For positive lengths, reject wrapping end addresses before any store.
// Bounds of the non-wrapping ranges are then checked by the loads/stores.
fn no_wrap(f: &mut Func, address: u32) {
    f.get(address);
    f.get(2);
    f.i32(1);
    f.op(I::I32Sub);
    f.op(I::I32Add);
    f.get(address);
    f.op(I::I32LtU);
    trap_if(f);
}

fn tail(f: &mut Func, address: u32, width: u32) {
    f.get(address);
    f.get(2);
    f.u32(width);
    f.op(I::I32Sub);
    f.op(I::I32Add);
}

fn filled(f: &mut Func, value: u32, width: u32) {
    f.get(value);
    if width != 8 {
        f.op(I::I32WrapI64);
    }
}

fn pair(f: &mut Func, source: u32, dest: u32, width: Width, pattern: Option<u32>) {
    // Keep the first destination beneath the source loads, so the tail
    // value can flow directly from its load into the first store.
    tail(f, 0, width.bytes());
    let first = if let Some(value) = pattern {
        filled(f, value, width.bytes());
        None
    } else {
        let value = f.local(if width == Width::W8 {
            Val::I64
        } else {
            Val::I32
        });
        f.get(1);
        load(f, source, 0, width);
        f.set(value);
        tail(f, 1, width.bytes());
        load(f, source, 0, width);
        Some(value)
    };
    store(f, dest, 0, width);
    f.get(0);
    if let Some(value) = first {
        f.get(value);
    } else {
        filled(f, pattern.expect("fill pattern"), width.bytes());
    }
    store(f, dest, 0, width);
}

// The tiny class uses byte 0, byte n/2 and byte n-1. These may coincide;
// reading all three first makes every overlap direction safe.
fn tiny_at(f: &mut Func, address: u32, position: u32) {
    f.get(address);
    if position != 0 {
        f.get(2);
        f.i32(1);
        f.op(if position == 1 { I::I32ShrU } else { I::I32Sub });
        f.op(I::I32Add);
    }
}

fn tiny(f: &mut Func, source: u32, dest: u32, copying: bool) {
    tiny_at(f, 0, 2);
    let mut parts = Vec::new();
    if copying {
        for position in 0..2 {
            let value = f.local(Val::I32);
            tiny_at(f, 1, position);
            load(f, source, 0, Width::W1);
            f.set(value);
            parts.push(value);
        }
        tiny_at(f, 1, 2);
        load(f, source, 0, Width::W1);
    } else {
        f.get(1);
    }
    store(f, dest, 0, Width::W1);
    for position in (0..2).rev() {
        tiny_at(f, 0, position);
        f.get(if copying { parts[position as usize] } else { 1 });
        store(f, dest, 0, Width::W1);
    }
}

pub fn helper(m: &Module, source: u32, dest: u32, copying: bool) -> Func {
    let name = if copying { "copy" } else { "fill" };
    let mut f = m.func(
        format!("rt:memory_{name}/{source}/{dest}"),
        vec![Val::I32; 3],
        vec![],
    );
    // The maximum scalar width needs one load/store and no address math.
    // Keep that natural boundary direct before the overlapping range cases.
    if copying {
        f.get(2);
        f.i32(8);
        f.op(I::I32Eq);
        f.op(I::If(BlockType::Empty));
        f.get(0);
        f.get(1);
        inline(&mut f, &mut Scratch::default(), 8, source, dest, true);
        f.op(I::Return);
        f.op(I::End);
    }
    // One unsigned interval test routes both zero and lengths above 16 to
    // the bulk instruction. In particular, zero retains native bounds checks.
    f.get(2);
    f.i32(1);
    f.op(I::I32Sub);
    f.i32(SMALL);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    for k in 0..3 {
        f.get(k);
    }
    f.push(if copying {
        copy(source, dest)
    } else {
        fill(dest)
    });
    f.op(I::Return);
    f.op(I::End);
    no_wrap(&mut f, 0);
    if copying {
        no_wrap(&mut f, 1);
        for width in [Width::W8, Width::W4] {
            f.get(2);
            f.u32(width.bytes());
            f.op(I::I32GeU);
            f.op(I::If(BlockType::Empty));
            pair(&mut f, source, dest, width, None);
            f.op(I::Return);
            f.op(I::End);
        }
        tiny(&mut f, source, dest, true);
    } else {
        f.get(2);
        f.i32(4);
        f.op(I::I32GeU);
        f.op(I::If(BlockType::Empty));
        let value = f.local(Val::I64);
        f.get(1);
        byte_pattern(&mut f, 8);
        f.set(value);
        f.get(2);
        f.i32(8);
        f.op(I::I32GeU);
        f.op(I::If(BlockType::Empty));
        pair(&mut f, source, dest, Width::W8, Some(value));
        f.op(I::Else);
        pair(&mut f, source, dest, Width::W4, Some(value));
        f.op(I::End);
        f.op(I::Else);
        tiny(&mut f, source, dest, false);
        f.op(I::End);
    }
    crate::opt::locals(&mut f, &vec![]);
    crate::opt::coalesce(&mut f);
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{BUF, Sym};
    use hd_base::Hash128;
    use std::io::Write;

    #[test]
    fn lowered_copies_and_fills_preserve_bytes_overlap_and_trap_atomicity() {
        let mut m = Module::default();
        let mut next = 1;
        for copying in [true, false] {
            for count in std::iter::once(None).chain((0..=64).map(Some)) {
                let name = format!(
                    "{}{}",
                    if copying { "copy" } else { "fill" },
                    count.map_or_else(|| "dynamic".to_owned(), |n| n.to_string())
                );
                let mut f = m.func(
                    &name,
                    vec![Val::I32; if count.is_some() { 2 } else { 3 }],
                    vec![],
                );
                f.get(0);
                f.get(1);
                if let Some(n) = count {
                    f.i32(n);
                } else {
                    f.get(2);
                }
                f.push(if copying { copy(BUF, BUF) } else { fill(BUF) });
                let symbol = Sym::Inst(Hash128(next));
                next += 1;
                m.define(symbol.clone(), f);
                m.export(name, symbol);
            }
        }
        let bytes = m.finish().expect("link bulk fixture");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("validate lowered bulk operations");
        let mut child = std::process::Command::new("node")
            .args(["--input-type=module", "-e", r"
                import assert from 'node:assert/strict';
                const chunks=[]; for await (const c of process.stdin) chunks.push(c);
                const {instance:{exports:e}}=await WebAssembly.instantiate(Buffer.concat(chunks));
                const bytes=new Uint8Array(e['hd.x'].buffer), end=bytes.length;
                const seed=Uint8Array.from({length:1024},(_,i)=>i%251);
                for(let n=0;n<=64;n++) {
                    const copies=[e.copydynamic];
                    copies.push(e['copy'+n]);
                    for(const copy of copies) for(let from=32;from<40;from++) for(let to=24;to<48;to++) {
                        bytes.set(seed);
                        const want=seed.slice();want.copyWithin(to,from,from+n);
                        copy(to,from,n);
                        assert.deepEqual(bytes.subarray(0,1024),want);
                    }
                    const fills=[e.filldynamic];
                    fills.push(e['fill'+n]);
                    for(const fill of fills) for(let to=24;to<32;to++) for(const value of [0,1,255,0x12345678,-1]) {
                        bytes.set(seed);
                        const want=seed.slice();want.fill(value,to,to+n);
                        fill(to,value,n);
                        assert.deepEqual(bytes.subarray(0,1024),want);
                    }
                    for(const copy of copies) {
                        if(n===0) {
                            copy(end,end,0);
                            assert.throws(()=>copy(end+1,0,0),WebAssembly.RuntimeError);
                            assert.throws(()=>copy(0,end+1,0),WebAssembly.RuntimeError);
                        } else {
                            for(const [to,from] of [[end-n+1,128],[128,end-n+1],[0xffffffff,128],[128,0xffffffff]]) {
                                bytes.fill(37); bytes.fill(93,128,192); bytes.fill(7,end-64);
                                const before=bytes.slice();
                                assert.throws(()=>copy(to,from,n),WebAssembly.RuntimeError);
                                assert.deepEqual(bytes,before,'a failed copy writes no bytes');
                            }
                        }
                    }
                    for(const fill of fills) {
                        if(n===0) {
                            fill(end,255,0);
                            assert.throws(()=>fill(end+1,255,0),WebAssembly.RuntimeError);
                        } else {
                            for(const to of [end-n+1,0xffffffff]) {
                                bytes.fill(37); const before=bytes.slice();
                                assert.throws(()=>fill(to,255,n),WebAssembly.RuntimeError);
                                assert.deepEqual(bytes,before,'a failed fill writes no bytes');
                            }
                        }
                    }
                }
            "])
            .stdin(std::process::Stdio::piped()).spawn().expect("start node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&bytes)
            .expect("write wasm");
        assert!(child.wait().expect("run bulk fixture").success());
    }
}
