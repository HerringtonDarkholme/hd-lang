//! One lowering path for bulk copies and fills, including generated runtime
//! code. Constants up to 31 bytes use scalar operations, exact widths up to
//! 16 and the helper's `[16, 32)` class above; dynamic sizes share a helper
//! per memory pair (Wasm memory indices are instruction immediates).

use wasm_encoder::BlockType;

use crate::module::{Func, Ins, Module, Val, mem};
use crate::op::Op as I;
use crate::rt::Helper;

/// The longest constant length lowered to scalar operations: the dynamic
/// helper's longest class, so a known length is never slower than the same
/// length through the helper (#1696).
const SMALL: i32 = (1 << CLASSES) - 1;

#[must_use]
pub fn copy(src_mem: u32, dst_mem: u32) -> Ins {
    Ins::Op(I::MemoryCopy { src_mem, dst_mem })
}

#[must_use]
pub fn fill(memory: u32) -> Ins {
    Ins::Op(I::MemoryFill(memory))
}

/// Lower after constant propagation, and again for a wide frame's generated
/// fill, before discovering the requested helpers. Their fallbacks are final.
pub fn lower<'a>(f: &mut Func, static_bytes: impl Fn(u32, u32) -> Option<&'a [u8]>) -> bool {
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
            let bytes = if copying && source == crate::module::HEAP && n > 0 {
                match f.body.last() {
                    Some(Ins::Op(I::I32Const(at))) => {
                        static_bytes(at.cast_unsigned(), n.cast_unsigned())
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(bytes) = bytes {
                f.remove(f.body.len() - 1);
                constant_copy(f, &mut scratch, bytes, n.cast_unsigned(), dest);
            } else {
                inline(f, &mut scratch, n.cast_unsigned(), source, dest, copying);
            }
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

/// `n` bytes from 17 to 31 as the helper's class `[16, 32)`: the first 16
/// and the last 16, each two `i64`s, overlapping by `32 - n` bytes. Four
/// pieces in place of up to eight exact widths; every load still precedes
/// every store, and the piece ending at `n` is stored first.
fn halves(n: u32) -> Vec<(u32, Width)> {
    [0, 8, n - 16, n - 8]
        .map(|offset| (offset, Width::W8))
        .to_vec()
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

// Immutable source bytes need no runtime loads. Write the highest chunk first,
// preserving the short-copy lowering's destination trap atomicity.
fn constant_copy(f: &mut Func, scratch: &mut Scratch, bytes: &[u8], n: u32, dest: u32) {
    let chunks = if n > 16 { halves(n) } else { widths(n) };
    let to = (chunks.len() > 1).then(|| {
        let to = scratch.local(f, 4, 1);
        f.op(I::LocalTee(to));
        to
    });
    for (k, (offset, width)) in chunks.into_iter().rev().enumerate() {
        if k != 0
            && let Some(to) = to
        {
            f.get(to);
        }
        let mut word = [0; 8];
        let start = offset as usize;
        word[..width.bytes() as usize]
            .copy_from_slice(&bytes[start..start + width.bytes() as usize]);
        if width == Width::W8 {
            f.op(I::I64Const(i64::from_le_bytes(word)));
        } else {
            f.i32(i32::from_le_bytes([word[0], word[1], word[2], word[3]]));
        }
        store(f, dest, offset, width);
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
    let chunks = if n > 1 << (CLASSES - 1) {
        halves(n)
    } else {
        widths(n)
    };
    let to = scratch.local(f, 4, 1);
    if copying {
        let from = scratch.local(f, 4, 0);
        f.set(from);
        f.op(I::LocalTee(to));
        let mut parts = Vec::new();
        let (mut word, mut long) = (2, 0);
        for &(offset, width) in &chunks[..chunks.len() - 1] {
            let slot = if width == Width::W8 {
                &mut long
            } else {
                &mut word
            };
            let value = scratch.local(f, width.bytes(), *slot);
            *slot += 1;
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

/// The dynamic helper serves lengths 1 to `2^CLASSES - 1`: 1 to 3 as three
/// bytes, and `[w, 2w)` for `w` of 4, 8 and 16 as the first `w` bytes plus
/// the last `w`. Lengths 0 and 32 or more use the bulk instruction, and a
/// constant length of 17 to 31 takes the last class's pieces inline. A class
/// of 32 to 63 bytes, 4 more `i64` pairs, grew every module by 266 bytes
/// and won nothing on the bench programs (#1359).
const CLASSES: u32 = 5;

// The helper's locals after its three parameters (destination, source or
// byte, length): the destination's and the source's tail base.
const TO: u32 = 3;
const FROM: u32 = 4;

/// One length class `[w, 2w)`: the first and the last `w` bytes, each as
/// `w / width` pieces, the last ones at the tail bases `TO` and `FROM`.
/// Every load precedes every store, so the halves may overlap each other
/// and the source may overlap the destination. The loaded pieces wait on
/// the operand stack, each above its destination, and the piece ending at
/// `dest + n`, stored first, traps whenever any store would.
fn class(f: &mut Func, source: u32, dest: u32, w: u32, copying: bool) {
    let width = if w >= 8 { Width::W8 } else { Width::W4 };
    let p = width.bytes();
    let pieces = w / p;
    let value = (!copying).then(|| {
        let value = f.local(if width == Width::W8 {
            Val::I64
        } else {
            Val::I32
        });
        f.get(1);
        byte_pattern(f, if width == Width::W8 { 8 } else { 4 });
        f.set(value);
        value
    });
    // Stored in reverse: the head's pieces, then the tail's, last on top.
    for (out, base) in [(0, 1), (TO, FROM)] {
        for k in 0..pieces {
            f.get(out);
            if let Some(value) = value {
                f.get(value);
            } else {
                f.get(base);
                load(f, source, k * p, width);
            }
        }
    }
    for _ in [0, TO] {
        for k in (0..pieces).rev() {
            store(f, dest, k * p, width);
        }
    }
}

/// Lengths 1 to 3, with no branch on the length: bytes 0, `n / 2` and
/// `n - 1`, which may coincide. Loading all three first makes every overlap
/// safe, and the byte at `n - 1` is stored first. A one-byte head proves no
/// later address in bounds of a 4-GiB memory, so here the source's last
/// address is checked for wrapping too.
fn tiny(f: &mut Func, source: u32, dest: u32, copying: bool) {
    for (base, at) in [(0, TO), (1, FROM)] {
        if base == 1 && !copying {
            break;
        }
        f.get(base);
        f.get(2);
        f.op(I::I32Add);
        f.i32(1);
        f.op(I::I32Sub);
        f.op(I::LocalTee(at));
        f.get(base);
        f.op(I::I32LtU);
        trap_if(f);
    }
    // The destination of each byte, under its value; the last on top.
    let half = |f: &mut Func, base: u32| {
        f.get(base);
        f.get(2);
        f.i32(1);
        f.op(I::I32ShrU);
        f.op(I::I32Add);
    };
    for k in 0..3 {
        if k == 1 {
            half(f, 0);
        } else {
            f.get(if k == 0 { 0 } else { TO });
        }
        if copying {
            if k == 1 {
                half(f, 1);
            } else {
                f.get(if k == 0 { 1 } else { FROM });
            }
            load(f, source, 0, Width::W1);
        } else {
            f.get(1);
        }
    }
    for _ in 0..3 {
        store(f, dest, 0, Width::W1);
    }
}

/// Classes `lo..=hi` (`[2^k, 2^(k+1))` bytes for class `k`), chosen by a
/// balanced tree of length tests.
fn classes(f: &mut Func, source: u32, dest: u32, (lo, hi): (u32, u32), copying: bool) {
    if lo == hi {
        class(f, source, dest, 1 << lo, copying);
        return;
    }
    let mid = u32::midpoint(lo, hi + 1);
    f.get(2);
    f.u32(1 << mid);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    classes(f, source, dest, (mid, hi), copying);
    f.op(I::Else);
    classes(f, source, dest, (lo, mid - 1), copying);
    f.op(I::End);
}

/// The runtime copy or fill for a length known only at run time.
///
/// One unsigned interval test sends zero and lengths of `2^CLASSES` or more
/// to the bulk instruction, which keeps native bounds checks for a zero
/// length. Lengths 1 to 3 take [`tiny`], with no branch between them: a
/// branch between 1 and 2 or 3 mispredicts on text built from short
/// pieces. For a length `n` from 4, with `c` leading zeros, its class is
/// `w = 2^(31 - c)`, and `d = n & (0x7fff_ffff >> c)` is `n - w`. One shared
/// prologue computes the tail bases `dest + d` and `source + d` with
/// wrapping adds, and a tree of length tests picks the class. No length has
/// a path of its own.
///
/// Trap atomicity of the classes from 4, in a memory of any size up to
/// 4 GiB:
/// - A source tail base cannot wrap once the head at `source` is in bounds,
///   because `d < w`. So a wrapped source tail has a head load past the end
///   of memory, which traps before any store. The source needs no check.
/// - Stores run in order, so a wrapped destination tail base traps first.
pub fn helper(m: &Module, source: u32, dest: u32, copying: bool) -> Func {
    let name = if copying { "copy" } else { "fill" };
    let mut f = m.func(
        format!("rt:memory_{name}/{source}/{dest}"),
        vec![Val::I32; 3],
        vec![],
    );
    for local in [TO, FROM] {
        assert_eq!(f.local(Val::I32), local);
    }
    f.get(2);
    f.i32(1);
    f.op(I::I32Sub);
    f.u32((1 << CLASSES) - 1);
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
    f.get(2);
    f.i32(4);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    f.get(0);
    f.get(2);
    f.op(I::I32Const(0x7fff_ffff));
    f.get(2);
    f.op(I::I32Clz);
    f.op(I::I32ShrU);
    f.op(I::I32And);
    if copying {
        f.op(I::LocalTee(FROM));
    }
    f.op(I::I32Add);
    f.op(I::LocalTee(TO));
    f.get(0);
    f.op(I::I32LtU);
    trap_if(&mut f);
    if copying {
        f.get(1);
        f.get(FROM);
        f.op(I::I32Add);
        f.set(FROM);
    }
    classes(&mut f, source, dest, (2, CLASSES - 1), copying);
    f.op(I::Else);
    tiny(&mut f, source, dest, copying);
    f.op(I::End);
    crate::opt::locals(&mut f, &vec![]);
    crate::opt::coalesce(&mut f);
    f
}
