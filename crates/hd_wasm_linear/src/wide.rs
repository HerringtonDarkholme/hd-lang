//! Wide signatures pass through the shadow stack
//! (`crates/hd_wasm_linear/DESIGN.md` §23.29). A value lays out as its flattened
//! Wasm values, so a function that takes or returns a struct of a
//! thousand fields would have a thousand parameters or results, more than
//! V8 accepts. Before encoding, every signature with more than [`WIDTH`]
//! parameters or more than [`WIDTH`] results changes, as clang's and
//! Rust's wasm32 `byval` and `sret` do:
//!
//! - wide parameters become one `i32`, the address of a frame on the
//!   shadow stack that holds each argument in an 8-byte slot, in order;
//!   the callee loads them all into locals before anything else runs, so
//!   a suspending callee keeps them in its own frame from then on;
//! - wide results become no results, and the function takes the frame's
//!   address as its last parameter; it stores each result in the frame's
//!   slots just before it returns.
//!
//! The caller reserves the frame below the stack pointer, writes the
//! arguments, calls, reads the results and gives the frame back. Every
//! function, `call` and `call_indirect` of a wide signature changes the
//! same way, so instances, closures, adapters, vtable slots, thunks and
//! runtime helpers agree without knowing of it; a function under the bound
//! never touches the stack pointer. The moves copy bits only: an
//! argument's or result's counts move through the frame as through Wasm
//! values, and the frame itself is never allocated or counted.

use std::collections::BTreeMap;

use hd_base::{NotImplemented, Stage, StageResult};
use wasm_encoder::Instruction as I;

use crate::module::{Func, Global, Ins, STACK, Sym, Val, mem};

/// The most parameters, and the most results, that a signature passes as
/// Wasm values. V8 and wasmparser (wasmtime) both refuse a function type
/// of more than 1000 parameters or 1000 results, and any bound under that
/// is correct; 64 keeps every ordinary signature as Wasm values, a struct
/// of some thirty strings included, and moves only rare wide ones.
pub const WIDTH: usize = 64;

/// The bytes of one argument or result slot: room for any [`Val`], and
/// every frame stays 8-byte aligned.
const SLOT: u32 = 8;

/// Whether a signature passes through the shadow stack.
#[must_use]
pub fn wide(params: &[Val], results: &[Val]) -> bool {
    params.len() > WIDTH || results.len() > WIDTH
}

/// The signature that a wide one lowers to: one frame address for wide
/// parameters, and the frame address after the parameters for wide
/// results.
fn lowered(params: &[Val], results: &[Val]) -> (Vec<Val>, Vec<Val>) {
    let wp = params.len() > WIDTH;
    let wr = results.len() > WIDTH;
    let mut p = if wp { Vec::new() } else { params.to_vec() };
    if wp || wr {
        p.push(Val::I32);
    }
    let r = if wr { Vec::new() } else { results.to_vec() };
    (p, r)
}

/// A wide call's frame size: a slot for each wide argument or result,
/// arguments and results sharing the slots, since the callee reads every
/// argument before it writes a result.
fn frame(params: &[Val], results: &[Val]) -> i32 {
    let mut n = 0;
    if params.len() > WIDTH {
        n = params.len();
    }
    if results.len() > WIDTH {
        n = n.max(results.len());
    }
    i32::try_from(n).expect("width") * SLOT.cast_signed()
}

/// Lowers every wide signature and every call of one. Whether any was
/// wide: the module then needs the stack memory. Fails on a wide import
/// or export, which the host calls by its Wasm signature.
pub fn lower(funcs: &mut BTreeMap<Sym, Func>, exports: &[(String, Sym)]) -> StageResult<bool> {
    let sig: BTreeMap<Sym, (Vec<Val>, Vec<Val>)> = funcs
        .iter()
        .filter(|(_, f)| wide(&f.params, &f.results))
        .map(|(s, f)| (s.clone(), (f.params.clone(), f.results.clone())))
        .collect();
    for (_, s) in exports {
        if sig.contains_key(s) {
            return Err(bug(&format!("an export of a wide signature {s:?}")));
        }
    }
    let mut any = !sig.is_empty();
    for f in funcs.values_mut() {
        if wide(&f.params, &f.results) {
            callee(f)?;
        }
        any |= calls(f, &sig)?;
    }
    Ok(any)
}

fn bug(what: &str) -> NotImplemented {
    NotImplemented::bug(Stage::Link, format!("the linear backend has {what}"))
}

const fn load(v: Val, at: u64) -> I<'static> {
    match v {
        Val::I32 => I::I32Load(mem(STACK, at, 2)),
        Val::I64 => I::I64Load(mem(STACK, at, 3)),
        Val::F32 => I::F32Load(mem(STACK, at, 2)),
        Val::F64 => I::F64Load(mem(STACK, at, 3)),
    }
}

const fn store(v: Val, at: u64) -> I<'static> {
    match v {
        Val::I32 => I::I32Store(mem(STACK, at, 2)),
        Val::I64 => I::I64Store(mem(STACK, at, 3)),
        Val::F32 => I::F32Store(mem(STACK, at, 2)),
        Val::F64 => I::F64Store(mem(STACK, at, 3)),
    }
}

fn slot(k: usize) -> u64 {
    u64::try_from(k).expect("slot") * u64::from(SLOT)
}

/// Scratch locals of one function, by type, that each lowered sequence
/// takes afresh: the `k`th of each type it needs.
#[derive(Default)]
struct Scratch {
    by: BTreeMap<Val, Vec<u32>>,
    used: BTreeMap<Val, usize>,
}

impl Scratch {
    /// Starts a new sequence: every scratch local is free again.
    fn reset(&mut self) {
        self.used.clear();
    }

    fn take(&mut self, f: &mut Func, v: Val) -> u32 {
        let k = self.used.entry(v).or_default();
        let pool = self.by.entry(v).or_default();
        if *k == pool.len() {
            pool.push(f.local(v));
        }
        *k += 1;
        pool[*k - 1]
    }
}

/// Pops `vals`, the top of the stack, into scratch locals and stores each
/// in its slot of the frame at local `base`.
fn spill(f: &mut Func, scratch: &mut Scratch, vals: &[Val], base: u32, out: &mut Vec<Ins>) {
    let tmps: Vec<u32> = vals.iter().map(|v| scratch.take(f, *v)).collect();
    for t in tmps.iter().rev() {
        out.push(Ins::Op(I::LocalSet(*t)));
    }
    for (k, (v, t)) in vals.iter().zip(&tmps).enumerate() {
        out.push(Ins::Op(I::LocalGet(base)));
        out.push(Ins::Op(I::LocalGet(*t)));
        out.push(Ins::Op(store(*v, slot(k))));
    }
}

/// Lowers a wide function's own signature: its parameters load from the
/// frame first, and every return stores its results there.
fn callee(f: &mut Func) -> StageResult<()> {
    if f.raw.is_some() {
        return Err(bug(&format!("a raw body of a wide signature {}", f.name)));
    }
    let (params, results) = (f.params.clone(), f.results.clone());
    let wp = params.len() > WIDTH;
    let wr = results.len() > WIDTH;
    if wr && branches_out(&f.body) {
        return Err(bug(&format!(
            "a branch out of {} with wide results",
            f.name
        )));
    }
    let (np, nr) = lowered(&params, &results);
    let old_np = u32::try_from(params.len()).expect("params");
    // Each old local's new index: wide parameters become the first
    // locals, after the frame address; narrow ones keep their places
    // before it.
    let to = |l: u32| -> u32 { if wp || l >= old_np { l + 1 } else { l } };
    for i in &mut f.body {
        if let Ins::Op(I::LocalGet(l) | I::LocalSet(l) | I::LocalTee(l)) = i {
            *l = to(*l);
        }
    }
    let base = u32::try_from(np.len() - 1).expect("params");
    let mut locals = if wp { params.clone() } else { Vec::new() };
    locals.append(&mut f.locals);
    f.params = np;
    f.results = nr;
    f.locals = locals;
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len() + 3 * params.len());
    if wp {
        for (k, v) in params.iter().enumerate() {
            out.push(Ins::Op(I::LocalGet(base)));
            out.push(Ins::Op(load(*v, slot(k))));
            out.push(Ins::Op(I::LocalSet(1 + u32::try_from(k).expect("params"))));
        }
    }
    let mut scratch = Scratch::default();
    for i in body {
        if wr && matches!(i, Ins::Op(I::Return)) {
            scratch.reset();
            spill(f, &mut scratch, &results, base, &mut out);
        }
        out.push(i);
    }
    if wr {
        scratch.reset();
        spill(f, &mut scratch, &results, base, &mut out);
    }
    f.body = out;
    Ok(())
}

/// Whether a body branches to its function's own label, which would
/// return its results without a [`I::Return`].
fn branches_out(body: &[Ins]) -> bool {
    let mut d = 0u32;
    for i in body {
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

/// Lowers each call of a wide signature in `f`: the caller reserves a
/// frame below the stack pointer, stores the arguments there and passes
/// its address, loads the results after the call and frees the frame.
/// Whether `f` made such a call.
fn calls(f: &mut Func, sig: &BTreeMap<Sym, (Vec<Val>, Vec<Val>)>) -> StageResult<bool> {
    for i in &f.body {
        if let Ins::Call(Sym::Import(im)) = i
            && wide(&im.params, &im.results)
        {
            return Err(bug(&format!("a wide import {}/{}", im.module, im.name)));
        }
    }
    let needs = f.body.iter().any(|i| match i {
        Ins::Call(s) => sig.contains_key(s),
        Ins::CallIndirect(p, r) => wide(p, r),
        _ => false,
    });
    if !needs {
        return Ok(false);
    }
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len());
    let mut scratch = Scratch::default();
    for i in body {
        let (params, results) = match &i {
            Ins::Call(s) if sig.contains_key(s) => sig[s].clone(),
            Ins::CallIndirect(p, r) if wide(p, r) => (p.clone(), r.clone()),
            _ => {
                out.push(i);
                continue;
            }
        };
        scratch.reset();
        // A `call_indirect`'s table slot is over its arguments.
        let index = matches!(i, Ins::CallIndirect(..)).then(|| {
            let t = scratch.take(f, Val::I32);
            out.push(Ins::Op(I::LocalSet(t)));
            t
        });
        let size = frame(&params, &results);
        let base = scratch.take(f, Val::I32);
        out.push(Ins::GlobalGet(Global::Stack));
        out.push(Ins::Op(I::I32Const(size)));
        out.push(Ins::Op(I::I32Sub));
        out.push(Ins::Op(I::LocalTee(base)));
        out.push(Ins::GlobalSet(Global::Stack));
        if params.len() > WIDTH {
            spill(f, &mut scratch, &params, base, &mut out);
        }
        out.push(Ins::Op(I::LocalGet(base)));
        match index {
            Some(t) => {
                out.push(Ins::Op(I::LocalGet(t)));
                let (p, r) = lowered(&params, &results);
                out.push(Ins::CallIndirect(p, r));
            }
            None => out.push(i),
        }
        if results.len() > WIDTH {
            for (k, v) in results.iter().enumerate() {
                out.push(Ins::Op(I::LocalGet(base)));
                out.push(Ins::Op(load(*v, slot(k))));
            }
        }
        out.push(Ins::Op(I::LocalGet(base)));
        out.push(Ins::Op(I::I32Const(size)));
        out.push(Ins::Op(I::I32Add));
        out.push(Ins::GlobalSet(Global::Stack));
    }
    f.body = out;
    Ok(true)
}
