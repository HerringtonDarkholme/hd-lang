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

fn widths(mut n: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for width in [8, 4, 2, 1] {
        while n >= width {
            out.push((offset, width));
            offset += width;
            n -= width;
        }
    }
    out
}

fn load(f: &mut Func, memory: u32, offset: u32, width: u32) {
    let arg = mem(memory, u64::from(offset), 0);
    f.op(match width {
        8 => I::I64Load(arg),
        4 => I::I32Load(arg),
        2 => I::I32Load16U(arg),
        1 => I::I32Load8U(arg),
        _ => unreachable!("scalar width"),
    });
}

fn store(f: &mut Func, memory: u32, offset: u32, width: u32) {
    let arg = mem(memory, u64::from(offset), 0);
    f.op(match width {
        8 => I::I64Store(arg),
        4 => I::I32Store(arg),
        2 => I::I32Store16(arg),
        1 => I::I32Store8(arg),
        _ => unreachable!("scalar width"),
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
    if n <= 8 && n.is_power_of_two() {
        if copying {
            load(f, source, 0, n);
        } else if let Some(byte) = byte {
            literal_pattern(f, byte, n);
        } else {
            byte_pattern(f, n);
        }
        store(f, dest, 0, n);
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
            let value = scratch.local(f, width, if width == 8 { 0 } else { word });
            if width != 8 {
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
                literal_pattern(f, byte, width);
            } else {
                f.get(value.expect("fill pattern"));
                if wide && width != 8 {
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

fn pair(f: &mut Func, source: u32, dest: u32, width: u32, filled: Option<u32>) {
    let mut parts = Vec::new();
    if filled.is_none() {
        for end in [false, true] {
            let value = f.local(if width == 8 { Val::I64 } else { Val::I32 });
            if end {
                tail(f, 1, width);
            } else {
                f.get(1);
            }
            load(f, source, 0, width);
            f.set(value);
            parts.push(value);
        }
    }
    for end in [true, false] {
        if end {
            tail(f, 0, width);
        } else {
            f.get(0);
        }
        if let Some(value) = filled {
            f.get(value);
        } else {
            f.get(parts[usize::from(end)]);
        }
        store(f, dest, 0, width);
    }
}

pub fn helper(m: &Module, source: u32, dest: u32, copying: bool) -> Func {
    let name = if copying { "copy" } else { "fill" };
    let mut f = m.func(
        format!("rt:memory_{name}/{source}/{dest}"),
        vec![Val::I32; 3],
        vec![],
    );
    // Copies retain their scalar-width fast paths. Fills dispatch by ranges
    // below: a repeated-byte pattern needs only the selected store width.
    if copying {
        for width in [8, 4, 2, 1] {
            f.get(2);
            f.i32(width);
            f.op(I::I32Eq);
            f.op(I::If(BlockType::Empty));
            f.get(0);
            f.get(1);
            inline(
                &mut f,
                &mut Scratch::default(),
                width.cast_unsigned(),
                source,
                dest,
                true,
            );
            f.op(I::Return);
            f.op(I::End);
        }
    }
    f.get(2);
    f.i32(SMALL);
    f.op(I::I32GtU);
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
    if copying {
        f.get(2);
        f.op(I::I32Eqz);
        f.op(I::If(BlockType::Empty));
        f.get(0);
        f.get(1);
        inline(&mut f, &mut Scratch::default(), 0, source, dest, true);
        f.op(I::Return);
        f.op(I::End);
        no_wrap(&mut f, 0);
        no_wrap(&mut f, 1);
        for (limit, width) in [(4, 2), (8, 4)] {
            f.get(2);
            f.i32(limit);
            f.op(I::I32LtU);
            f.op(I::If(BlockType::Empty));
            pair(&mut f, source, dest, width, None);
            f.op(I::Return);
            f.op(I::End);
        }
        pair(&mut f, source, dest, 8, None);
    } else {
        for width in [8, 4, 2] {
            f.get(2);
            f.u32(width);
            f.op(I::I32GeU);
            f.op(I::If(BlockType::Empty));
            f.get(2);
            f.u32(width);
            f.op(I::I32Eq);
            f.op(I::If(BlockType::Empty));
            f.get(0);
            f.get(1);
            inline(&mut f, &mut Scratch::default(), width, source, dest, false);
            f.op(I::Return);
            f.op(I::End);
            no_wrap(&mut f, 0);
            let value = f.local(if width == 8 { Val::I64 } else { Val::I32 });
            f.get(1);
            byte_pattern(&mut f, width);
            f.set(value);
            pair(&mut f, source, dest, width, Some(value));
            f.op(I::Return);
            f.op(I::End);
        }
        f.get(2);
        f.op(I::If(BlockType::Empty));
        f.get(0);
        f.get(1);
        inline(&mut f, &mut Scratch::default(), 1, source, dest, false);
        f.op(I::Else);
        f.get(0);
        f.get(1);
        inline(&mut f, &mut Scratch::default(), 0, source, dest, false);
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
