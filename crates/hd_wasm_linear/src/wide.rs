//! Wide signatures and value storage use the shadow stack
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

use crate::op::Op as I;
use hd_base::{NotImplemented, Stage, StageResult};
use wasm_encoder::BlockType;

use crate::module::{Func, Global, Ins, STACK, Sym, SymId, Symbols, Val, mem};

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
pub fn lower(
    funcs: &mut [Option<Func>],
    exports: &[(String, SymId)],
    symbols: &Symbols,
) -> StageResult<bool> {
    let sig: Vec<Option<(Vec<Val>, Vec<Val>)>> = funcs
        .iter()
        .map(|f| {
            f.as_ref()
                .filter(|f| wide(&f.params, &f.results))
                .map(|f| (f.params.clone(), f.results.clone()))
        })
        .collect();
    for (_, id) in exports {
        if sig.get(id.idx()).is_some_and(Option::is_some) {
            return Err(bug(&format!(
                "an export of a wide signature {:?}",
                *symbols.get(*id)
            )));
        }
    }
    let mut any = sig.iter().any(Option::is_some);
    for f in funcs.iter_mut().flatten() {
        if wide(&f.params, &f.results) {
            callee(f)?;
        }
        any |= calls(f, &sig)?;
        any |= storage(f)?;
        f.rebuild_callees();
    }
    Ok(any)
}

/// A function that materializes a wide aggregate keeps its temporaries in
/// one zeroed shadow frame. Rewriting after value emission leaves ownership,
/// suspension saves and early-exit cleanup unchanged. Narrow bodies use no
/// frame. Spill all locals in the wide body so helper scratch and copies
/// cannot recreate an unbounded Wasm-local signature.
fn storage(f: &mut Func) -> StageResult<bool> {
    if !f.stack_values || f.locals.is_empty() {
        return Ok(false);
    }
    if f.raw.is_some() {
        return Err(bug("a raw body needing wide local storage"));
    }
    let np = u32::try_from(f.params.len()).expect("params");
    let locals = f.locals.clone();
    let end = np + u32::try_from(locals.len()).expect("locals");
    let size = i32::try_from(locals.len()).expect("wide locals") * SLOT.cast_signed();
    let base = f.local(Val::I32);
    let mut temps = BTreeMap::new();
    for &v in &locals {
        temps.entry(v).or_insert_with(|| f.local(v));
    }
    let selector = f.local(Val::I32);
    let stack = f.intern_global(Global::Stack);
    let mut out = vec![
        Ins::GlobalGet(stack),
        Ins::Op(I::I32Const(size)),
        Ins::Op(I::I32Sub),
        Ins::Op(I::LocalTee(base)),
        Ins::GlobalSet(stack),
        Ins::Op(I::LocalGet(base)),
        Ins::Op(I::I32Const(0)),
        Ins::Op(I::I32Const(size)),
        Ins::Op(I::MemoryFill(STACK)),
    ];
    let restore = |out: &mut Vec<Ins>| {
        out.extend([
            Ins::Op(I::LocalGet(base)),
            Ins::Op(I::I32Const(size)),
            Ins::Op(I::I32Add),
            Ins::GlobalSet(stack),
        ]);
    };
    let mut origins = crate::origins::Remap::new(f.origins.as_deref());
    origins.repeat(f.origin_root, out.len());
    let body = std::mem::take(&mut f.body);
    let mut depth = 0;
    for (position, ins) in body.into_iter().enumerate() {
        let output_start = out.len();
        match ins {
            Ins::Op(I::LocalGet(l)) if (np..end).contains(&l) => {
                out.push(Ins::Op(I::LocalGet(base)));
                out.push(Ins::Op(load(
                    locals[(l - np) as usize],
                    slot((l - np) as usize),
                )));
            }
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) if (np..end).contains(&l) => {
                let v = locals[(l - np) as usize];
                let t = temps[&v];
                let tee = matches!(ins, Ins::Op(I::LocalTee(_)));
                out.push(Ins::Op(if tee { I::LocalTee(t) } else { I::LocalSet(t) }));
                out.push(Ins::Op(I::LocalGet(base)));
                out.push(Ins::Op(I::LocalGet(t)));
                out.push(Ins::Op(store(v, slot((l - np) as usize))));
            }
            ins => exit(f, ins, &mut depth, selector, &restore, &mut out),
        }
        origins.merged(position..position + 1, out.len() - output_start);
    }
    let output_start = out.len();
    restore(&mut out);
    origins.repeat(f.origin_root, out.len() - output_start);
    f.body = out;
    origins.finish(f);
    crate::opt::renumber(f);
    Ok(true)
}

/// Writes `ins` to `out`, with the code `restore` writes before it where
/// it leaves the function: a `return`, or a branch to the function's own
/// label at `depth`, conditional and table branches included. `depth`
/// follows the blocks `ins` opens and closes; `selector` is an `i32`
/// local that holds a branch's condition or index around the restore.
fn exit(
    f: &Func,
    ins: Ins,
    depth: &mut u32,
    selector: u32,
    restore: &impl Fn(&mut Vec<Ins>),
    out: &mut Vec<Ins>,
) {
    match ins {
        Ins::Op(I::Return) => {
            restore(out);
            out.push(ins);
        }
        Ins::Op(I::Br(d)) if d == *depth => {
            restore(out);
            out.push(ins);
        }
        Ins::Op(I::BrIf(d)) if d == *depth => {
            // Preserve the branch payload below the selector; no
            // branch crosses the temporary if's operand-stack floor.
            out.push(Ins::Op(I::LocalTee(selector)));
            out.push(Ins::Op(I::If(BlockType::Empty)));
            restore(out);
            out.push(Ins::Op(I::End));
            out.push(Ins::Op(I::LocalGet(selector)));
            out.push(ins);
        }
        Ins::Op(I::BrTable(id, default))
            if default == *depth || f.label_list(id).contains(depth) =>
        {
            let labels = f.label_list(id);
            out.push(Ins::Op(I::LocalSet(selector)));
            out.push(Ins::Op(I::I32Const(0)));
            for (k, &label) in labels.iter().enumerate() {
                if label == *depth {
                    out.push(Ins::Op(I::LocalGet(selector)));
                    out.push(Ins::Op(I::I32Const(i32::try_from(k).expect("labels"))));
                    out.push(Ins::Op(I::I32Eq));
                    out.push(Ins::Op(I::I32Or));
                }
            }
            if default == *depth {
                out.push(Ins::Op(I::LocalGet(selector)));
                out.push(Ins::Op(I::I32Const(
                    i32::try_from(labels.len()).expect("labels"),
                )));
                out.push(Ins::Op(I::I32GeU));
                out.push(Ins::Op(I::I32Or));
            }
            out.push(Ins::Op(I::If(BlockType::Empty)));
            restore(out);
            out.push(Ins::Op(I::End));
            out.push(Ins::Op(I::LocalGet(selector)));
            out.push(ins);
        }
        Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => {
            *depth += 1;
            out.push(ins);
        }
        Ins::Op(I::End) => {
            *depth -= 1;
            out.push(ins);
        }
        _ => out.push(ins),
    }
}

/// Reserves a frame of `size` bytes on the local stack (§23.38) at the
/// entry of `f`, whose address local `base` holds, and gives it back at
/// every exit. A frame that does not fit in the stack's segment starts a
/// new one: pages memory 1 grows, which the page table marks static, as
/// it does every page the allocator did not grow. Each frame gives back
/// its own top, so a frame in a new segment needs no link to the last.
pub fn locals_frame(f: &mut Func, base: u32, size: u32) {
    let pages = size.div_ceil(1 << 16).max(1);
    let size = i32::try_from(size).expect("frame size");
    let selector = f.local(Val::I32);
    let top = f.intern_global(Global::Locals);
    let bottom = f.intern_global(Global::LocalsBase);
    let mut out = vec![
        Ins::GlobalGet(top),
        Ins::GlobalGet(bottom),
        Ins::Op(I::I32Sub),
        Ins::Op(I::I32Const(size)),
        Ins::Op(I::I32LtU),
        Ins::Op(I::If(BlockType::Empty)),
        Ins::Op(I::I32Const(pages.cast_signed())),
        Ins::Op(I::MemoryGrow(crate::module::HEAP)),
        Ins::Op(I::LocalTee(base)),
        Ins::Op(I::I32Const(-1)),
        Ins::Op(I::I32Eq),
        Ins::Op(I::If(BlockType::Empty)),
        Ins::Op(I::Unreachable),
        Ins::Op(I::End),
        Ins::Op(I::LocalGet(base)),
        Ins::Op(I::I32Const(16)),
        Ins::Op(I::I32Shl),
        Ins::Op(I::LocalTee(base)),
        Ins::GlobalSet(bottom),
        Ins::Op(I::LocalGet(base)),
        Ins::Op(I::I32Const((pages << 16).cast_signed())),
        Ins::Op(I::I32Add),
        Ins::GlobalSet(top),
        Ins::Op(I::End),
        Ins::GlobalGet(top),
        Ins::Op(I::I32Const(size)),
        Ins::Op(I::I32Sub),
        Ins::Op(I::LocalTee(base)),
        Ins::GlobalSet(top),
    ];
    let restore = |out: &mut Vec<Ins>| {
        out.extend([
            Ins::Op(I::LocalGet(base)),
            Ins::Op(I::I32Const(size)),
            Ins::Op(I::I32Add),
            Ins::GlobalSet(top),
        ]);
    };
    let mut origins = crate::origins::Remap::new(f.origins.as_deref());
    origins.repeat(f.origin_root, out.len());
    let body = std::mem::take(&mut f.body);
    let mut depth = 0;
    for (position, ins) in body.into_iter().enumerate() {
        let output_start = out.len();
        exit(f, ins, &mut depth, selector, &restore, &mut out);
        origins.merged(position..position + 1, out.len() - output_start);
    }
    let output_start = out.len();
    restore(&mut out);
    origins.repeat(f.origin_root, out.len() - output_start);
    f.body = out;
    origins.finish(f);
}

fn bug(what: &str) -> NotImplemented {
    NotImplemented::bug(Stage::Link, format!("the linear backend has {what}"))
}

const fn load(v: Val, at: u64) -> I {
    match v {
        Val::I32 => I::I32Load(mem(STACK, at, 2)),
        Val::I64 => I::I64Load(mem(STACK, at, 3)),
        Val::F32 => I::F32Load(mem(STACK, at, 2)),
        Val::F64 => I::F64Load(mem(STACK, at, 3)),
    }
}

const fn store(v: Val, at: u64) -> I {
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
    if wr && branches_out(f) {
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
    if let Some(names) = &mut f.local_names {
        *names = std::mem::take(names)
            .into_iter()
            .map(|(l, n)| (to(l), n))
            .collect();
        names.insert(base, "args".to_owned());
    }
    let mut locals = if wp { params.clone() } else { Vec::new() };
    locals.append(&mut f.locals);
    f.params = np;
    f.results = nr;
    f.locals = locals;
    let mut origins = crate::origins::Remap::new(f.origins.as_deref());
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len() + 3 * params.len());
    if wp {
        for (k, v) in params.iter().enumerate() {
            out.push(Ins::Op(I::LocalGet(base)));
            out.push(Ins::Op(load(*v, slot(k))));
            out.push(Ins::Op(I::LocalSet(1 + u32::try_from(k).expect("params"))));
        }
    }
    origins.repeat(f.origin_root, out.len());
    let mut scratch = Scratch::default();
    for (position, i) in body.into_iter().enumerate() {
        let output_start = out.len();
        if wr && matches!(i, Ins::Op(I::Return)) {
            scratch.reset();
            spill(f, &mut scratch, &results, base, &mut out);
        }
        out.push(i);
        origins.merged(position..position + 1, out.len() - output_start);
    }
    let output_start = out.len();
    if wr {
        scratch.reset();
        spill(f, &mut scratch, &results, base, &mut out);
    }
    origins.repeat(f.origin_root, out.len() - output_start);
    f.body = out;
    origins.finish(f);
    Ok(())
}

/// Whether a body branches to its function's own label, which would
/// return its results without a [`I::Return`].
fn branches_out(f: &Func) -> bool {
    let mut d = 0u32;
    for i in &f.body {
        match i {
            Ins::Op(I::Block(_) | I::Loop(_) | I::If(_)) => d += 1,
            Ins::Op(I::End) => d = d.saturating_sub(1),
            Ins::Op(I::Br(l) | I::BrIf(l)) if *l == d => return true,
            Ins::Op(I::BrTable(ls, def)) if *def == d || f.label_list(*ls).contains(&d) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Lowers each call of a wide signature in `f`: the caller reserves a
/// frame below the stack pointer, stores the arguments there and passes
/// its address, loads the results after the call and frees the frame.
/// Whether `f` made such a call.
fn calls(f: &mut Func, sig: &[Option<(Vec<Val>, Vec<Val>)>]) -> StageResult<bool> {
    for i in &f.body {
        if let Ins::Call(id) = i
            && let Sym::Import(im) = &*f.symbol(*id)
            && wide(&im.params, &im.results)
        {
            return Err(bug(&format!("a wide import {}/{}", im.module, im.name)));
        }
    }
    let needs = f.body.iter().any(|i| match i {
        Ins::Call(id) => sig.get(id.idx()).is_some_and(Option::is_some),
        Ins::CallIndirect(id) => {
            let signature = f.signature(*id);
            wide(&signature.0, &signature.1)
        }
        _ => false,
    });
    if !needs {
        return Ok(false);
    }
    let mut origins = crate::origins::Remap::new(f.origins.as_deref());
    let body = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(body.len());
    let mut scratch = Scratch::default();
    let stack = f.intern_global(Global::Stack);
    for (position, i) in body.into_iter().enumerate() {
        let output_start = out.len();
        let signature = match i {
            Ins::Call(id) => sig.get(id.idx()).and_then(Option::as_ref).cloned(),
            Ins::CallIndirect(id) => {
                let signature = f.signature(id);
                wide(&signature.0, &signature.1).then(|| signature.clone())
            }
            _ => None,
        };
        let Some((params, results)) = signature else {
            out.push(i);
            origins.copy(position..position + 1);
            continue;
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
        out.push(Ins::GlobalGet(stack));
        out.push(Ins::Op(I::I32Const(size)));
        out.push(Ins::Op(I::I32Sub));
        out.push(Ins::Op(I::LocalTee(base)));
        out.push(Ins::GlobalSet(stack));
        if params.len() > WIDTH {
            spill(f, &mut scratch, &params, base, &mut out);
        }
        out.push(Ins::Op(I::LocalGet(base)));
        match index {
            Some(t) => {
                out.push(Ins::Op(I::LocalGet(t)));
                let (p, r) = lowered(&params, &results);
                out.push(Ins::CallIndirect(f.symbols.intern_signature(p, r)));
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
        out.push(Ins::GlobalSet(stack));
        origins.merged(position..position + 1, out.len() - output_start);
    }
    f.body = out;
    origins.finish(f);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;

    #[test]
    fn wide_expansions_preserve_origins_without_changing_instructions() {
        let m = Module::default();
        let mut f = m.func("storage", vec![], vec![Val::I32]);
        f.locals = vec![Val::I32; WIDTH + 1];
        f.stack_values = true;
        f.origin_root = Some(99);
        f.body = vec![
            Ins::Op(I::I32Const(7)),
            Ins::Op(I::LocalSet(0)),
            Ins::Op(I::LocalGet(0)),
        ];
        f.origins = Some(vec![Some(10), Some(11), Some(12)]);
        let mut plain = f.clone();
        plain.origins = None;
        storage(&mut plain).unwrap();
        storage(&mut f).unwrap();
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        let expected = [
            vec![Some(99); 9],
            vec![Some(10)],
            vec![Some(11); 4],
            vec![Some(12); 2],
            vec![Some(99); 4],
        ]
        .concat();
        assert_eq!(f.origins.as_ref().unwrap(), &expected);
        assert_eq!(f.body.len(), expected.len());

        let mut f = m.func("callee", vec![Val::I32; WIDTH + 1], vec![]);
        f.origin_root = Some(99);
        f.body = vec![
            Ins::Op(I::LocalGet(u32::try_from(WIDTH).unwrap())),
            Ins::Op(I::Drop),
        ];
        f.origins = Some(vec![Some(10), Some(11)]);
        callee(&mut f).unwrap();
        let spans = f.origins.as_ref().unwrap();
        assert_eq!(spans.len(), f.body.len());
        assert!(spans[..3 * (WIDTH + 1)].iter().all(|s| *s == Some(99)));
        assert_eq!(&spans[3 * (WIDTH + 1)..], [Some(10), Some(11)]);

        let target = Sym::Inst(hd_base::Hash128(7));
        let mut f = m.func("caller", vec![], vec![]);
        let target = f.intern(target);
        f.body = vec![Ins::Call(target)];
        f.origins = Some(vec![Some(17)]);
        let mut sig = vec![None; target.idx() + 1];
        sig[target.idx()] = Some((vec![Val::I32; WIDTH + 1], vec![]));
        calls(&mut f, &sig).unwrap();
        assert_eq!(f.origins.as_ref().unwrap().len(), f.body.len());
        assert!(f.origins.unwrap().iter().all(|s| *s == Some(17)));
    }

    use hd_base::Hash128;
    use std::io::Write;

    #[test]
    fn shadow_storage_zeroes_each_entry_and_restores_every_exit() {
        let mut m = Module::default();
        let mut branched = m.func("branched", vec![Val::I32], vec![Val::I32]);
        branched.stack_values = true;
        let value = branched.local(Val::I32);
        // A local is zero at each entry, including after a previous entry
        // left nonzero bytes in this frame.
        branched.get(value);
        branched.get(0);
        branched.op(I::I32Eqz);
        branched.op(I::BrIf(0));
        branched.i32(7);
        branched.op(I::LocalTee(value));
        branched.op(I::Block(BlockType::Result(wasm_encoder::ValType::I32)));
        branched.get(value);
        branched.get(0);
        branched.i32(1);
        branched.op(I::I32Sub);
        branched.br_table(vec![0], 1);
        branched.op(I::End);
        branched.op(I::I32Add);
        branched.op(I::Return);
        m.define(Sym::Inst(Hash128(6401)), branched);
        m.export("branched", Sym::Inst(Hash128(6401)));
        let mut top = m.func("top", vec![], vec![Val::I32]);
        top.global_get(Global::Stack);
        m.define(Sym::Inst(Hash128(6402)), top);
        m.export("top", Sym::Inst(Hash128(6402)));
        let mut numeric = m.func(
            "numeric",
            vec![],
            vec![Val::I64, Val::F64, Val::F32, Val::I32],
        );
        numeric.stack_values = true;
        let locals = [Val::I64, Val::F64, Val::F32, Val::I32].map(|v| numeric.local(v));
        for (l, op) in locals.into_iter().zip([
            I::I64Const(123),
            I::F64Const(1.5.into()),
            I::F32Const(2.25.into()),
            I::I32Const(7),
        ]) {
            numeric.op(op);
            numeric.set(l);
        }
        for l in locals {
            numeric.get(l);
        }
        m.define(Sym::Inst(Hash128(6403)), numeric);
        m.export("numeric", Sym::Inst(Hash128(6403)));
        let bytes = m.finish().expect("link shadow-storage fixture");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("validate shadow storage");
        let mut child = std::process::Command::new("node")
            .args([
                "--input-type=module",
                "-e",
                r"
                import assert from 'node:assert/strict';
                const chunks=[]; for await (const c of process.stdin) chunks.push(c);
                const {instance:{exports:e}}=await WebAssembly.instantiate(Buffer.concat(chunks));
                const top=e.top();
                assert.deepEqual(e.numeric(),[123n,1.5,2.25,7]);
                assert.equal(e.top(),top);
                for(const [input,want] of [[1,14],[2,7],[0,0],[1,14],[0,0]]){
                    assert.equal(e.branched(input),want);
                    assert.equal(e.top(),top);
                }
            ",
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("start node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&bytes)
            .expect("write wasm");
        assert!(child.wait().expect("run node").success());
    }
}
