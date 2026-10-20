//! A function's local cleanup before encoding (`crates/hd_wasm_linear/DESIGN.md`
//! §23.22): emission gives every TIR value a fresh local, so most values
//! pass through a `local.set` and a `local.get` that follow each other.
//! Each rule below keeps what every path through the function computes:
//!
//! 1. `local.set t; local.get t`, where that set is `t`'s only write and
//!    that get its only read, is the value left on the stack;
//! 2. another `local.set t; local.get t` is `local.tee t`;
//! 3. a `local.set t` of a local no instruction reads is a `drop`, and a
//!    constant or `local.get` it drops goes too;
//! 4. a copy `P; local.set t`, where `P` pushes a local, a global or a
//!    constant and that set is `t`'s only write, moves to `t`'s only read
//!    when the read follows in the same straight-line run of
//!    instructions and nothing between writes what `P` reads;
//! 5. `if A else br N end`, with `N` past the `if` and no branch in `A`
//!    to the `if` itself, is `i32.eqz; br_if N-1; A`, and `if br N else A
//!    end` is `br_if N-1; A`: the shape of a `while` loop's test;
//! 6. `if A; local.set x else B; local.set x end; local.get x`, where
//!    those are `x`'s only writes and that get its only read and neither
//!    arm branches to the `if` itself, is `if (result T) A else B end`;
//! 7. constants fold: a nonzero constant's zero check goes, `+ a ± b` is
//!    one addition, `x == 0` is `i32.eqz`, a test of `x != 0` is a test
//!    of `x`, a multiplication, quotient or remainder by a power of two is
//!    a shift or a mask, and a constant added to a load's or store's
//!    address is its offset;
//! 8. `local.set a; R; local.set b; local.get a; local.get b`, where
//!    those are `a`'s and `b`'s only writes and reads and `R` is a
//!    straight-line run that pushes one value over what it found, is
//!    `R` over `a`'s value: two arguments evaluated in order;
//! 9. `i32.const c; i32.add; R; store`, with `R` such a run, is `R` and
//!    the store at `c` more offset;
//! 10. `X; local.set a; P; local.get a`, where those are `a`'s only write
//!     and read, `X` is a straight-line run that pushes one value and `P`
//!     pushes a local, a global or a constant that `X` cannot change, is
//!     `P; X`;
//! 11. a local no instruction names is not declared, and the others are
//!     numbered again in order.
//!
//! Across functions, [`constants`] reads a top-level binding that only
//! its init writes, with a constant, as that constant.

use std::collections::BTreeMap;

use hd_base::Hash128;
use wasm_encoder::Instruction as I;

use crate::module::{Func, Global, Ins, Sym};
use crate::rt::Helper;

/// Reads each top-level binding that one instruction writes, a constant
/// stored by an init, as that constant, and drops the store. An init runs
/// before any code that reads its bindings (`module.init.order`), so every
/// read sees the constant.
pub fn constants(funcs: &mut BTreeMap<Sym, Func>) {
    let mut inits: Vec<Hash128> = Vec::new();
    for s in funcs.keys() {
        match s {
            Sym::Helper(Helper::EntryInit { inits: ks }) => inits.extend(ks.iter().map(|k| k.0)),
            Sym::Helper(Helper::EntryPoll { then, .. }) => inits.extend(then.iter().map(|k| k.0)),
            _ => {}
        }
    }
    let mut sets: BTreeMap<Global, (u32, Option<Write>)> = BTreeMap::new();
    for (s, f) in funcs.iter() {
        for (k, i) in f.body.iter().enumerate() {
            if let Ins::GlobalSet(g @ Global::Storage(..)) = i {
                let e = sets.entry(g.clone()).or_insert((0, None));
                e.0 += 1;
                let init = matches!(s, Sym::Inst(key) if inits.contains(key));
                e.1 = match (init, k.checked_sub(1).map(|p| &f.body[p])) {
                    (true, Some(c @ Ins::Op(I::I32Const(_) | I::I64Const(_)))) => {
                        Some((s.clone(), k, c.clone()))
                    }
                    _ => None,
                };
            }
        }
    }
    let known: BTreeMap<Global, Write> = sets
        .into_iter()
        .filter_map(|(g, (n, at))| (n == 1).then_some(at).flatten().map(|at| (g, at)))
        .collect();
    if known.is_empty() {
        return;
    }
    for f in funcs.values_mut() {
        for i in &mut f.body {
            if let Ins::GlobalGet(g) = i
                && let Some((_, _, c)) = known.get(g)
            {
                *i = c.clone();
            }
        }
    }
    let mut drops: BTreeMap<Sym, Vec<usize>> = BTreeMap::new();
    for (s, k, _) in known.into_values() {
        drops.entry(s).or_default().push(k);
    }
    for (s, mut ks) in drops {
        ks.sort_unstable();
        if let Some(f) = funcs.get_mut(&s) {
            for k in ks.into_iter().rev() {
                f.body.drain(k - 1..=k);
            }
        }
    }
}

/// A binding's one write when an init stores a constant: the function,
/// the store's index and the constant.
type Write = (Sym, usize, Ins);

/// The instructions a rewrite removes, and an instruction it changes.
type Edit = (Vec<usize>, Option<(usize, Ins)>);

/// The parameter and result counts of each function a call can name.
pub type Arity = BTreeMap<Sym, (usize, usize)>;

/// Cleans up `f`'s locals by the rules of the module documentation.
pub fn locals(f: &mut Func, arity: &Arity) {
    let np = u32::try_from(f.params.len()).expect("params");
    loop {
        let (reads, writes) = counts(f);
        let mut out: Vec<Ins> = Vec::with_capacity(f.body.len());
        let mut changed = false;
        let mut k = 0;
        while k < f.body.len() {
            let next = f.body.get(k + 1);
            match (&f.body[k], next) {
                (Ins::Op(I::LocalSet(t)), Some(Ins::Op(I::LocalGet(u)))) if t == u => {
                    let t = *t as usize;
                    if *u >= np && reads[t] == 1 && writes[t] == 1 {
                        changed = true;
                    } else {
                        out.push(Ins::Op(I::LocalTee(*u)));
                        changed = true;
                    }
                    k += 2;
                }
                (Ins::Op(I::LocalSet(t)), _) if reads[*t as usize] == 0 => {
                    changed = true;
                    match out.last() {
                        Some(Ins::Op(
                            I::LocalGet(_)
                            | I::I32Const(_)
                            | I::I64Const(_)
                            | I::F32Const(_)
                            | I::F64Const(_),
                        )) => {
                            out.pop();
                        }
                        _ => out.push(Ins::Op(I::Drop)),
                    }
                    k += 1;
                }
                (Ins::Op(I::LocalTee(t)), _) if reads[*t as usize] == 0 => {
                    changed = true;
                    k += 1;
                }
                (other, _) => {
                    out.push(other.clone());
                    k += 1;
                }
            }
        }
        f.body = out;
        changed |= forward(f, np);
        changed |= shapes(f, np);
        changed |= fold(f);
        changed |= schedule(f, np, arity);
        if !changed {
            break;
        }
    }
    renumber(f);
}

/// Rule 4: moves each copy into a local read once to its read.
fn forward(f: &mut Func, np: u32) -> bool {
    let (reads, writes) = counts(f);
    let mut changed = false;
    let mut k = 0;
    while k + 1 < f.body.len() {
        let Ins::Op(I::LocalSet(t)) = f.body[k + 1] else {
            k += 1;
            continue;
        };
        let p = f.body[k].clone();
        let pure = matches!(
            p,
            Ins::Op(
                I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_)
            ) | Ins::GlobalGet(_)
                | Ins::TableSlot(_)
                | Ins::VTable(_)
        );
        if !pure || t < np || reads[t as usize] != 1 || writes[t as usize] != 1 {
            k += 1;
            continue;
        }
        let mut found = None;
        for j in k + 2..f.body.len() {
            let i = &f.body[j];
            if matches!(i, Ins::Op(I::LocalGet(u)) if *u == t) {
                found = Some(j);
                break;
            }
            if clobbers(&p, i) || ends_run(i) {
                break;
            }
        }
        if let Some(j) = found {
            f.body[j] = p;
            f.body.drain(k..k + 2);
            changed = true;
        } else {
            k += 1;
        }
    }
    changed
}

/// Rule 7: folds the constants of each window of instructions.
fn fold(f: &mut Func) -> bool {
    let b = std::mem::take(&mut f.body);
    let mut out: Vec<Ins> = Vec::with_capacity(b.len());
    let mut changed = false;
    let mut k = 0;
    let c32 = |i: Option<&Ins>| match i {
        Some(Ins::Op(I::I32Const(c))) => Some(*c),
        _ => None,
    };
    let push = |i: Option<&Ins>| {
        matches!(
            i,
            Some(
                Ins::Op(
                    I::LocalGet(_)
                        | I::I32Const(_)
                        | I::I64Const(_)
                        | I::F32Const(_)
                        | I::F64Const(_)
                ) | Ins::GlobalGet(_)
            )
        )
    };
    while k < b.len() {
        let at = |j: usize| b.get(k + j);
        // `c; local.tee t; i32.eqz; if ... end` of a nonzero `c`, with no
        // `else`: the test never holds.
        if let (
            Some(c),
            Some(Ins::Op(I::LocalTee(t))),
            Some(Ins::Op(I::I32Eqz)),
            Some(Ins::Op(I::If(_))),
        ) = (c32(at(0)), at(1), at(2), at(3))
            && c != 0
            && let Some(end) = block_end(&b, k + 3)
        {
            out.push(Ins::Op(I::I32Const(c)));
            out.push(Ins::Op(I::LocalSet(*t)));
            k = end + 1;
            changed = true;
            continue;
        }
        match (at(0), at(1), at(2), at(3)) {
            // `+ a ± b` is one addition, of `a ± b` (`i32` wraps either way).
            (
                Some(Ins::Op(I::I32Const(a))),
                Some(Ins::Op(I::I32Add)),
                Some(Ins::Op(I::I32Const(b))),
                Some(Ins::Op(op @ (I::I32Add | I::I32Sub))),
            ) => {
                let c = if matches!(op, I::I32Add) {
                    a.wrapping_add(*b)
                } else {
                    a.wrapping_sub(*b)
                };
                out.extend([Ins::Op(I::I32Const(c)), Ins::Op(I::I32Add)]);
                k += 4;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(0))), Some(Ins::Op(I::I32Eq)), ..) => {
                out.push(Ins::Op(I::I32Eqz));
                k += 2;
                changed = true;
            }
            (
                Some(Ins::Op(I::I32Const(0))),
                Some(Ins::Op(I::I32Ne)),
                Some(Ins::Op(I::If(_) | I::BrIf(_))),
                _,
            ) => {
                k += 2;
                changed = true;
            }
            (
                Some(Ins::Op(I::I32Const(c))),
                Some(Ins::Op(op @ (I::I32DivU | I::I32RemU | I::I32Mul))),
                ..,
            ) if *c > 0
                && c.cast_unsigned().is_power_of_two()
                && (*c >= 64 || !matches!(op, I::I32Mul)) =>
            {
                let n = c.cast_unsigned().trailing_zeros().cast_signed();
                match op {
                    I::I32DivU => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32ShrU)]),
                    I::I32RemU => out.extend([Ins::Op(I::I32Const(c - 1)), Ins::Op(I::I32And)]),
                    _ => out.extend([Ins::Op(I::I32Const(n)), Ins::Op(I::I32Shl)]),
                }
                k += 2;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add)), Some(Ins::Op(load)), _)
                if *c >= 0 && with_offset(load, 0).is_some() && is_load(load) =>
            {
                out.push(Ins::Op(
                    with_offset(load, c.cast_unsigned()).expect("a load"),
                ));
                k += 3;
                changed = true;
            }
            (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add)), v, Some(Ins::Op(store)))
                if *c >= 0 && push(v) && with_offset(store, 0).is_some() && !is_load(store) =>
            {
                out.push(v.expect("a push").clone());
                out.push(Ins::Op(
                    with_offset(store, c.cast_unsigned()).expect("a store"),
                ));
                k += 4;
                changed = true;
            }
            _ => {
                out.push(b[k].clone());
                k += 1;
            }
        }
    }
    f.body = out;
    changed
}

/// Rules 8 and 9: lets values stay on the stack across a straight-line
/// run that pushes one more.
fn schedule(f: &mut Func, np: u32, arity: &Arity) -> bool {
    let mut changed = false;
    'outer: loop {
        let (reads, writes) = counts(f);
        let once = |l: u32| l >= np && reads[l as usize] == 1 && writes[l as usize] == 1;
        let b = &f.body;
        let mut edit: Option<Edit> = None;
        for q in 0..b.len() {
            // Rule 9.
            if let (Some(Ins::Op(I::I32Const(c))), Some(Ins::Op(I::I32Add))) =
                (b.get(q), b.get(q + 1))
                && *c >= 0
                && let Some(t) = store_after(b, q + 2, arity)
                && let Ins::Op(st) = &b[t]
                && !is_load(st)
                && let Some(moved) = with_offset(st, c.cast_unsigned())
            {
                edit = Some((vec![q, q + 1], Some((t, Ins::Op(moved)))));
                break;
            }
            // Rule 10.
            if let (Some(Ins::Op(I::LocalSet(a))), Some(p), Some(Ins::Op(I::LocalGet(g)))) =
                (b.get(q), b.get(q + 1), b.get(q + 2))
                && a == g
                && once(*a)
                && matches!(
                    p,
                    Ins::Op(I::LocalGet(_) | I::I32Const(_) | I::I64Const(_)) | Ins::GlobalGet(_)
                )
                && let Some(s) = (0..q).rev().find(|&s| net(b, s, q, arity) == Some(1))
                && !b[s..q].iter().any(|x| clobbers(p, x))
            {
                let mut body = b[..s].to_vec();
                body.push(p.clone());
                body.extend_from_slice(&b[s..q]);
                body.extend_from_slice(&b[q + 3..]);
                f.body = body;
                changed = true;
                continue 'outer;
            }
            // Rule 8.
            let (
                Some(Ins::Op(I::LocalSet(bl))),
                Some(Ins::Op(I::LocalGet(al))),
                Some(Ins::Op(I::LocalGet(bg))),
            ) = (b.get(q), b.get(q + 1), b.get(q + 2))
            else {
                continue;
            };
            if bl != bg || al == bl || !once(*al) || !once(*bl) {
                continue;
            }
            let Some(p) = (0..q)
                .rev()
                .find(|&p| matches!(b[p], Ins::Op(I::LocalSet(x)) if x == *al))
            else {
                continue;
            };
            if net(b, p + 1, q, arity) == Some(1) {
                edit = Some((vec![p, q, q + 1, q + 2], None));
                break;
            }
        }
        let Some((gone, replace)) = edit else {
            return changed;
        };
        if let Some((t, i)) = replace {
            f.body[t] = i;
        }
        for k in gone.into_iter().rev() {
            f.body.remove(k);
        }
        changed = true;
    }
}

/// The depth over its start that straight-line run `b[from..to]` ends at,
/// when no instruction of it reaches below that start.
fn net(b: &[Ins], from: usize, to: usize, arity: &Arity) -> Option<usize> {
    let mut depth = 0usize;
    for i in &b[from..to] {
        let (pops, pushes) = effect(i, arity)?;
        depth = depth.checked_sub(pops)? + pushes;
    }
    Some(depth)
}

/// The store that takes the address below the straight-line run from
/// `at` and the one value the run pushes over it: the first instruction
/// of the run that reaches below its start, when that is a store at depth
/// one.
fn store_after(b: &[Ins], at: usize, arity: &Arity) -> Option<usize> {
    let mut depth = 0usize;
    for (k, i) in b.iter().enumerate().skip(at) {
        let (pops, pushes) = effect(i, arity)?;
        if pops > depth {
            let store = matches!(i, Ins::Op(op) if !is_load(op) && with_offset(op, 0).is_some());
            return (store && depth == 1).then_some(k);
        }
        depth = depth - pops + pushes;
    }
    None
}

/// What straight-line instruction `i` pops and pushes; `None` for control
/// and for anything this table does not know.
fn effect(i: &Ins, arity: &Arity) -> Option<(usize, usize)> {
    Some(match i {
        Ins::GlobalGet(_) | Ins::TableSlot(_) | Ins::VTable(_) => (0, 1),
        Ins::Anchor(_) => (0, 0),
        Ins::GlobalSet(_) => (1, 0),
        Ins::Call(s) => match s {
            Sym::Import(im) => (im.params.len(), im.results.len()),
            other => *arity.get(other)?,
        },
        Ins::CallIndirect(p, r) => (p.len() + 1, r.len()),
        Ins::Op(op) => match op {
            I::LocalGet(_) | I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_) => {
                (0, 1)
            }
            I::LocalTee(_) => (1, 1),
            I::LocalSet(_) | I::Drop => (1, 0),
            I::Select => (3, 1),
            I::I32Eqz
            | I::I64Eqz
            | I::I32WrapI64
            | I::I64ExtendI32S
            | I::I64ExtendI32U
            | I::I32Clz
            | I::I64Clz
            | I::I32Ctz
            | I::I64Ctz => (1, 1),
            I::I32Add
            | I::I32Sub
            | I::I32Mul
            | I::I32And
            | I::I32Or
            | I::I32Xor
            | I::I32Shl
            | I::I32ShrU
            | I::I32ShrS
            | I::I32Eq
            | I::I32Ne
            | I::I32LtU
            | I::I32LtS
            | I::I32GtU
            | I::I32GtS
            | I::I32LeU
            | I::I32LeS
            | I::I32GeU
            | I::I32GeS
            | I::I64Add
            | I::I64Sub
            | I::I64Mul
            | I::I64And
            | I::I64Or
            | I::I64Xor
            | I::I64Shl
            | I::I64ShrU
            | I::I64ShrS
            | I::I64Eq
            | I::I64Ne
            | I::I64LtU
            | I::I64LtS
            | I::I64GtU
            | I::I64GtS
            | I::I64LeU
            | I::I64LeS
            | I::I64GeU
            | I::I64GeS => (2, 1),
            other if is_load(other) => (1, 1),
            other if with_offset(other, 0).is_some() => (2, 0),
            _ => return None,
        },
        _ => return None,
    })
}

/// The `end` of the block that instruction `at` of `b` opens, when it has
/// no `else`.
fn block_end(b: &[Ins], at: usize) -> Option<usize> {
    let mut d = 0u32;
    for (k, i) in b.iter().enumerate().skip(at) {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::Else) if d == 1 => return None,
            Ins::Op(I::End) => {
                d -= 1;
                if d == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether memory instruction `i` loads.
fn is_load(i: &I<'static>) -> bool {
    matches!(
        i,
        I::I32Load(_)
            | I::I64Load(_)
            | I::F32Load(_)
            | I::F64Load(_)
            | I::I32Load8S(_)
            | I::I32Load8U(_)
            | I::I32Load16S(_)
            | I::I32Load16U(_)
            | I::I64Load8U(_)
            | I::I64Load16U(_)
            | I::I64Load32U(_)
    )
}

/// Load or store `i` with `by` more on its offset; `None` for any other
/// instruction, or an offset past 32 bits.
fn with_offset(i: &I<'static>, by: u32) -> Option<I<'static>> {
    let bump = |m: &wasm_encoder::MemArg| {
        let offset = m
            .offset
            .checked_add(u64::from(by))
            .filter(|o| u32::try_from(*o).is_ok())?;
        Some(wasm_encoder::MemArg { offset, ..*m })
    };
    Some(match i {
        I::I32Load(m) => I::I32Load(bump(m)?),
        I::I64Load(m) => I::I64Load(bump(m)?),
        I::F32Load(m) => I::F32Load(bump(m)?),
        I::F64Load(m) => I::F64Load(bump(m)?),
        I::I32Load8S(m) => I::I32Load8S(bump(m)?),
        I::I32Load8U(m) => I::I32Load8U(bump(m)?),
        I::I32Load16S(m) => I::I32Load16S(bump(m)?),
        I::I32Load16U(m) => I::I32Load16U(bump(m)?),
        I::I64Load8U(m) => I::I64Load8U(bump(m)?),
        I::I64Load16U(m) => I::I64Load16U(bump(m)?),
        I::I64Load32U(m) => I::I64Load32U(bump(m)?),
        I::I32Store(m) => I::I32Store(bump(m)?),
        I::I64Store(m) => I::I64Store(bump(m)?),
        I::F32Store(m) => I::F32Store(bump(m)?),
        I::F64Store(m) => I::F64Store(bump(m)?),
        I::I32Store8(m) => I::I32Store8(bump(m)?),
        I::I32Store16(m) => I::I32Store16(bump(m)?),
        I::I64Store8(m) => I::I64Store8(bump(m)?),
        I::I64Store16(m) => I::I64Store16(bump(m)?),
        I::I64Store32(m) => I::I64Store32(bump(m)?),
        _ => return None,
    })
}

/// Rules 5 and 6: rewrites each `if` of those shapes.
fn shapes(f: &mut Func, np: u32) -> bool {
    let mut changed = false;
    while let Some(next) = shape_once(f, np) {
        f.body = next;
        changed = true;
    }
    changed
}

/// The body after the first rewrite by rule 5 or 6, if there is one.
fn shape_once(f: &Func, np: u32) -> Option<Vec<Ins>> {
    let (reads, writes) = counts(f);
    let b = &f.body;
    for (s, e, n) in ifs(b) {
        let Ins::Op(I::If(wasm_encoder::BlockType::Empty)) = b[s] else {
            continue;
        };
        let Some(e) = e else { continue };
        let then = s + 1..e;
        let other = e + 1..n;
        // Rule 5.
        let lone_br = |r: std::ops::Range<usize>| match &b[r] {
            [Ins::Op(I::Br(d))] if *d >= 1 => Some(*d),
            _ => None,
        };
        for (exit, keep, negate) in [
            (other.clone(), then.clone(), true),
            (then.clone(), other.clone(), false),
        ] {
            if let Some(d) = lone_br(exit)
                && !targets_self(&b[keep.clone()])
            {
                let mut out = b[..s].to_vec();
                if negate {
                    out.push(Ins::Op(I::I32Eqz));
                }
                out.push(Ins::Op(I::BrIf(d - 1)));
                out.extend(lowered(&b[keep]));
                out.extend_from_slice(&b[n + 1..]);
                return Some(out);
            }
        }
        // Rule 6.
        if let (
            Some(Ins::Op(I::LocalSet(x))),
            Some(Ins::Op(I::LocalSet(y))),
            Some(Ins::Op(I::LocalGet(z))),
        ) = (b.get(e - 1), b.get(n - 1), b.get(n + 1))
            && x == y
            && y == z
            && *x >= np
            && reads[*x as usize] == 1
            && writes[*x as usize] == 2
            && !targets_self(&b[then.clone()])
            && !targets_self(&b[other.clone()])
        {
            let t = f.locals[(*x - np) as usize].wasm();
            let mut out = b[..s].to_vec();
            out.push(Ins::Op(I::If(wasm_encoder::BlockType::Result(t))));
            out.extend_from_slice(&b[s + 1..e - 1]);
            out.push(Ins::Op(I::Else));
            out.extend_from_slice(&b[e + 1..n - 1]);
            out.push(Ins::Op(I::End));
            out.extend_from_slice(&b[n + 2..]);
            return Some(out);
        }
    }
    None
}

/// Each `if` of `b`: its own index, its `else`'s and its `end`'s.
fn ifs(b: &[Ins]) -> Vec<(usize, Option<usize>, usize)> {
    let mut open: Vec<(usize, bool, Option<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (k, i) in b.iter().enumerate() {
        match i {
            Ins::Op(I::If(_)) => open.push((k, true, None)),
            Ins::Op(I::Block(_) | I::Loop(_)) => open.push((k, false, None)),
            Ins::Op(I::Else) => {
                if let Some(top) = open.last_mut() {
                    top.2 = Some(k);
                }
            }
            Ins::Op(I::End) => {
                if let Some((at, is_if, els)) = open.pop()
                    && is_if
                {
                    out.push((at, els, k));
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether instructions `r`, an arm of an `if`, branch to that `if`.
fn targets_self(r: &[Ins]) -> bool {
    let mut d = 0u32;
    for i in r {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::End) => d = d.saturating_sub(1),
            Ins::Op(I::Br(l) | I::BrIf(l)) if *l == d => return true,
            Ins::Op(I::BrTable(ls, def)) if *def == d || ls.contains(&d) => return true,
            _ => {}
        }
    }
    false
}

/// An arm of an `if` moved out of it: each branch past the `if` names one
/// label fewer.
fn lowered(r: &[Ins]) -> Vec<Ins> {
    let mut d = 0u32;
    let mut out = Vec::with_capacity(r.len());
    for i in r {
        let down = |l: u32| if l > d { l - 1 } else { l };
        out.push(match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
                d += 1;
                i.clone()
            }
            Ins::Op(I::End) => {
                d = d.saturating_sub(1);
                i.clone()
            }
            Ins::Op(I::Br(l)) => Ins::Op(I::Br(down(*l))),
            Ins::Op(I::BrIf(l)) => Ins::Op(I::BrIf(down(*l))),
            Ins::Op(I::BrTable(ls, def)) => Ins::Op(I::BrTable(
                ls.iter().map(|l| down(*l)).collect::<Vec<_>>().into(),
                down(*def),
            )),
            other => other.clone(),
        });
    }
    out
}

/// Whether `i` may change what pure push `p` pushes.
fn clobbers(p: &Ins, i: &Ins) -> bool {
    match (p, i) {
        (Ins::Op(I::LocalGet(a)), Ins::Op(I::LocalSet(b) | I::LocalTee(b))) => a == b,
        (Ins::GlobalGet(g), Ins::GlobalSet(h)) => g == h,
        (Ins::GlobalGet(_), Ins::Call(_) | Ins::CallIndirect(..)) => true,
        _ => false,
    }
}

/// Whether `i` ends a straight-line run: a branch, or a block's edge.
fn ends_run(i: &Ins) -> bool {
    matches!(
        i,
        Ins::Op(
            I::Block(_)
                | I::Loop(_)
                | I::If(_)
                | I::Else
                | I::End
                | I::Br(_)
                | I::BrIf(_)
                | I::BrTable(..)
                | I::Return
                | I::Unreachable
        ) | Ins::SaveAll
            | Ins::LoadAll
    )
}

/// Each local's reads and writes; a `local.tee` is a write.
fn counts(f: &Func) -> (Vec<u32>, Vec<u32>) {
    let n = f.params.len() + f.locals.len();
    let (mut reads, mut writes) = (vec![0u32; n], vec![0u32; n]);
    for i in &f.body {
        match i {
            Ins::Op(I::LocalGet(l)) => reads[*l as usize] += 1,
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => writes[*l as usize] += 1,
            _ => {}
        }
    }
    (reads, writes)
}

/// Drops the locals past the parameters that no instruction names, and
/// numbers the others again in order.
pub(crate) fn renumber(f: &mut Func) {
    let np = f.params.len();
    let mut used = vec![false; f.locals.len()];
    for i in &f.body {
        if let Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) = i
            && let Some(at) = (*l as usize).checked_sub(np)
        {
            used[at] = true;
        }
    }
    let mut map = Vec::with_capacity(f.locals.len());
    let mut kept = Vec::new();
    for (j, v) in f.locals.iter().enumerate() {
        map.push(u32::try_from(np + kept.len()).expect("locals"));
        if used[j] {
            kept.push(*v);
        }
    }
    let to = |l: u32| -> u32 {
        match (l as usize).checked_sub(np) {
            Some(at) => map[at],
            None => l,
        }
    };
    for i in &mut f.body {
        match i {
            Ins::Op(I::LocalGet(l)) => *l = to(*l),
            Ins::Op(I::LocalSet(l)) => *l = to(*l),
            Ins::Op(I::LocalTee(l)) => *l = to(*l),
            _ => {}
        }
    }
    f.locals = kept;
}

/// Cancels balanced count changes on unchanged locals in a short straight
/// line. Calls other than retains, stores to an operand local, and control
/// flow end the proof: any of them may invalidate the original owner.
pub fn count_pairs(f: &mut Func, retain: Hash128) -> bool {
    #[derive(PartialEq, Eq)]
    enum Count {
        Ref,
        KnownRef,
        Value(Vec<crate::module::Val>, crate::layout::Walk),
    }
    fn count(ins: &Ins, retain: Hash128) -> Option<(bool, Count, usize)> {
        match ins {
            Ins::Call(Sym::Inst(k)) if *k == retain => Some((true, Count::Ref, 1)),
            Ins::Call(Sym::Helper(Helper::ReleaseRef)) => Some((false, Count::Ref, 1)),
            Ins::Call(Sym::Helper(Helper::RetainKnown)) => Some((true, Count::KnownRef, 1)),
            Ins::Call(Sym::Helper(Helper::ReleaseKnown)) => Some((false, Count::KnownRef, 1)),
            Ins::Call(Sym::Helper(Helper::Retain { vals, walk })) => {
                Some((true, Count::Value(vals.clone(), walk.clone()), vals.len()))
            }
            Ins::Call(Sym::Helper(Helper::Release { vals, walk })) => {
                Some((false, Count::Value(vals.clone(), walk.clone()), vals.len()))
            }
            _ => None,
        }
    }
    fn arguments(body: &[Ins], at: usize, n: usize) -> Option<Vec<(usize, u32)>> {
        let mut args = Vec::new();
        for (i, ins) in body[..at].iter().enumerate().rev() {
            match ins {
                Ins::Anchor(_) => continue,
                Ins::Op(I::LocalGet(l)) => args.push((i, *l)),
                _ => return None,
            }
            if args.len() == n {
                args.reverse();
                return Some(args);
            }
        }
        None
    }
    let mut gone = std::collections::BTreeSet::new();
    for (i, ins) in f.body.iter().enumerate() {
        if gone.contains(&i) {
            continue;
        }
        let Some((true, key, n)) = count(ins, retain) else {
            continue;
        };
        let Some(args) = arguments(&f.body, i, n) else {
            continue;
        };
        for (j, next) in f.body.iter().enumerate().skip(i + 1).take(128) {
            if gone.contains(&j) {
                continue;
            }
            if let Some((adding, other, width)) = count(next, retain) {
                if adding {
                    continue;
                }
                if other == key
                    && width == n
                    && let Some(end) = arguments(&f.body, j, n)
                    && args.iter().map(|(_, l)| l).eq(end.iter().map(|(_, l)| l))
                {
                    gone.extend([i, j]);
                    gone.extend(args.iter().chain(&end).map(|(at, _)| *at));
                }
                break;
            }
            match next {
                Ins::Anchor(_)
                | Ins::Op(
                    I::LocalGet(_)
                    | I::I32Const(_)
                    | I::I64Const(_)
                    | I::F32Const(_)
                    | I::F64Const(_)
                    | I::Drop
                    | I::I32Add
                    | I::I32Sub
                    | I::I32Mul
                    | I::I32Eq
                    | I::I32Ne
                    | I::I32Eqz
                    | I::I32And
                    | I::I32Or
                    | I::I32Xor,
                ) => {}
                Ins::Op(I::LocalSet(l) | I::LocalTee(l)) if !args.iter().any(|(_, a)| a == l) => {}
                _ => break,
            }
        }
    }
    if gone.is_empty() {
        return false;
    }
    f.body = std::mem::take(&mut f.body)
        .into_iter()
        .enumerate()
        .filter_map(|(i, ins)| (!gone.contains(&i)).then_some(ins))
        .collect();
    true
}

/// Folds code that a constant decides (#655): an `if` whose condition is
/// an `i32` constant becomes a `block` of the arm that runs (or goes,
/// with no `else` and no result), and an `i32` local whose every write
/// stores 0 is read as 0, so the flag of a branch folded away goes with
/// it. A `block` keeps the label depth of the `if` it replaces, so no
/// branch inside the arm changes. Gives whether anything changed.
pub fn fold_constant_branches(f: &mut Func) -> bool {
    use crate::module::Val;
    use wasm_encoder::BlockType;
    let mut changed = false;
    loop {
        let mut again = false;
        // Constant conditions.
        let mut i = 0;
        while i + 1 < f.body.len() {
            let (Ins::Op(I::I32Const(c)), Ins::Op(I::If(bt))) = (&f.body[i], &f.body[i + 1]) else {
                i += 1;
                continue;
            };
            let (c, bt) = (*c, *bt);
            // The `if`'s `else` (if any) and `end`.
            let (mut depth, mut els, mut end) = (0usize, None, None);
            for (k, ins) in f.body.iter().enumerate().skip(i + 2) {
                match ins {
                    Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => depth += 1,
                    Ins::Op(I::Else) if depth == 0 => els = Some(k),
                    Ins::Op(I::End) if depth == 0 => {
                        end = Some(k);
                        break;
                    }
                    Ins::Op(I::End) => depth -= 1,
                    _ => {}
                }
            }
            let Some(end) = end else {
                i += 1;
                continue;
            };
            let then = i + 2..els.unwrap_or(end);
            let other = els.map_or(end..end, |e| e + 1..end);
            let arm: Vec<Ins> = f.body[if c != 0 { then } else { other }].to_vec();
            let mut out: Vec<Ins> = Vec::new();
            if !(arm.is_empty() && matches!(bt, BlockType::Empty)) {
                out.push(Ins::Op(I::Block(bt)));
                out.extend(arm);
                out.push(Ins::Op(I::End));
            }
            f.body.splice(i..=end, out);
            again = true;
        }
        // Locals that only ever hold 0.
        let np = f.params.len();
        let mut zero: Vec<bool> = f.locals.iter().map(|v| *v == Val::I32).collect();
        for (k, ins) in f.body.iter().enumerate() {
            if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
                && let Some(at) = (*l as usize).checked_sub(np)
                && !(k > 0 && matches!(f.body[k - 1], Ins::Op(I::I32Const(0))))
            {
                zero[at] = false;
            }
        }
        let np32 = u32::try_from(np).expect("params");
        if zero.iter().any(|z| *z)
            && f.body.iter().any(|ins| {
                matches!(ins, Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l))
                    if *l >= np32 && zero[(*l - np32) as usize])
            })
        {
            let body = std::mem::take(&mut f.body);
            for ins in body {
                match ins {
                    Ins::Op(I::LocalGet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.op(I::I32Const(0));
                    }
                    // Every write stores a constant 0 just pushed: a set
                    // takes it back, and a tee leaves it.
                    Ins::Op(I::LocalSet(l)) if l >= np32 && zero[(l - np32) as usize] => {
                        f.body.pop();
                    }
                    Ins::Op(I::LocalTee(l)) if l >= np32 && zero[(l - np32) as usize] => {}
                    other => f.body.push(other),
                }
            }
            again = true;
        }
        if !again {
            return changed;
        }
        changed = true;
    }
}

/// Inlines the fast paths of the counts on known block starts
/// ([`Helper::RetainKnown`], [`Helper::ReleaseKnown`]; §23.30): a retain
/// adds one to the header's count unless the address is null or a static
/// object of count 0, and a release that leaves a count above zero takes
/// one off. A last count, an object that takes part in cycle collection
/// (so its decrement reaches `suspect`), or a release while the collector
/// traces (`Tracing`) still calls the helper, which does all the rest.
/// `acyclic` drops the two collector tests, as no object takes part.
/// Gives whether anything changed.
pub fn inline_counts(f: &mut Func, acyclic: bool) -> bool {
    use crate::module::{HEAP, Val, mem};
    use wasm_encoder::BlockType;
    if !f.body.iter().any(|i| {
        matches!(
            i,
            Ins::Call(Sym::Helper(Helper::RetainKnown | Helper::ReleaseKnown))
        )
    }) {
        return false;
    }
    let (t, b, c) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let body = std::mem::take(&mut f.body);
    // Leaves the header's address in `b` and its count in `c`, branching
    // out of the enclosing block for null.
    let header = |f: &mut Func| {
        f.set(t);
        f.op(I::Block(BlockType::Empty));
        f.get(t);
        f.op(I::I32Eqz);
        f.op(I::BrIf(0));
        f.get(t);
        f.i32(8);
        f.op(I::I32Sub);
        f.op(I::LocalTee(b));
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.op(I::LocalTee(c));
    };
    for ins in body {
        match ins {
            Ins::Call(Sym::Helper(Helper::RetainKnown)) => {
                header(f);
                f.op(I::I32Eqz);
                f.op(I::BrIf(0));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Add);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::End);
            }
            Ins::Call(Sym::Helper(Helper::ReleaseKnown)) => {
                header(f);
                f.i32(1);
                f.op(I::I32GtU);
                if !acyclic {
                    f.body.push(Ins::GlobalGet(Global::Tracing));
                    f.op(I::I32Eqz);
                    f.op(I::I32And);
                    f.get(b);
                    f.op(I::I32Load(mem(HEAP, 4, 2)));
                    f.i32(0);
                    f.op(I::I32GeS);
                    f.op(I::I32And);
                }
                f.op(I::If(BlockType::Empty));
                f.get(b);
                f.get(c);
                f.i32(1);
                f.op(I::I32Sub);
                f.op(I::I32Store(mem(HEAP, 0, 2)));
                f.op(I::Else);
                // A count of 0 is a static object's, except while the
                // collector traces: a gray object's count may have reached
                // 0, and `visit` must still see the reference.
                f.get(c);
                if !acyclic {
                    f.body.push(Ins::GlobalGet(Global::Tracing));
                    f.op(I::I32Or);
                }
                f.op(I::If(BlockType::Empty));
                f.get(t);
                f.body.push(Ins::Call(Sym::Helper(Helper::ReleaseKnown)));
                f.op(I::End);
                f.op(I::End);
                f.op(I::End);
            }
            other => f.body.push(other),
        }
    }
    true
}
