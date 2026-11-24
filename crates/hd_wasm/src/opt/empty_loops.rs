//! Delete finite, unobserved induction loops after instance folding.
//!
//! A strict comparison followed by a unit increment reaches its bound
//! without wrapping, for both signed and unsigned integers. Larger steps,
//! inclusive comparisons and arbitrary backedges do not carry that proof.
//! This is the local form of LLVM `LoopDeletion`'s finite, side-effect-free
//! loop rule; it needs no general scalar-evolution solver.
//! <https://llvm.org/doxygen/LoopDeletion_8cpp_source.html>

use super::{Func, I, Ins, Remap};

pub fn run(f: &mut Func) -> bool {
    rewrite(f, &mut || {})
}

// O(N + L) time/storage: count reads, recognize disjoint straight-line
// candidates, rebuild once. A failed candidate stops at its first control
// instruction, so nested loops never rescan an enclosing body. No retries
// or native recursion. `visit` counts examined input instructions.
pub fn rewrite(f: &mut Func, visit: &mut impl FnMut()) -> bool {
    let mut reads = vec![0usize; f.params.len() + f.locals.len()];
    for ins in &f.body {
        visit();
        if let Ins::Op(I::LocalGet(local)) = ins {
            reads[*local as usize] += 1;
        }
    }
    let mut gone = vec![false; f.body.len()];
    for at in 0..f.body.len() {
        visit();
        if !matches!(f.body[at], Ins::Op(I::Loop(wasm_encoder::BlockType::Empty))) {
            continue;
        }
        let Some((counter, end)) = candidate(&f.body, at, visit) else {
            continue;
        };
        // Both reads belong to this loop. Its final counter is therefore
        // unobservable, including through an enclosing loop's backedge.
        if reads[counter as usize] == 2 {
            gone[at..end].fill(true);
        }
    }
    if !gone.iter().any(|g| *g) {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let mut body = Vec::with_capacity(f.body.len());
    for (at, ins) in f.body.iter().enumerate() {
        visit();
        if !gone[at] {
            body.push(*ins);
            origins.copy(at..at + 1);
        }
    }
    f.body = body;
    origins.finish(f);
    true
}

fn candidate(body: &[Ins], at: usize, visit: &mut impl FnMut()) -> Option<(u32, usize)> {
    visit();
    let Ins::Op(I::LocalGet(counter)) = *body.get(at + 1)? else {
        return None;
    };
    let mut end = at + 2;
    visit();
    // The invariant is a local/constant, optionally loaded through held
    // storage. Like opt::drops, we rely on emission's checked-address
    // invariant: these loads cannot trap. No call or memory write is allowed.
    match *body.get(end)? {
        Ins::Op(I::LocalGet(local)) if local != counter => {}
        Ins::Op(I::I32Const(_) | I::I64Const(_)) => {}
        _ => return None,
    }
    end += 1;
    loop {
        visit();
        if !matches!(body.get(end), Some(Ins::Op(I::I32Load(_) | I::I64Load(_)))) {
            break;
        }
        end += 1;
    }
    visit();
    let wide = match *body.get(end)? {
        Ins::Op(I::I32LtU | I::I32LtS) => false,
        Ins::Op(I::I64LtU | I::I64LtS) => true,
        _ => return None,
    };
    let tail = body.get(end + 1..end + 9)?;
    for _ in tail {
        visit();
    }
    let [
        Ins::Op(I::I32Eqz),
        Ins::Op(I::BrIf(1)),
        Ins::Op(I::LocalGet(read)),
        Ins::Op(one),
        Ins::Op(add),
        Ins::Op(I::LocalSet(write)),
        Ins::Op(I::Br(0)),
        Ins::Op(I::End),
    ] = tail
    else {
        return None;
    };
    if *read != counter || *write != counter {
        return None;
    }
    if !matches!(
        (wide, one, add),
        (false, I::I32Const(1), I::I32Add) | (true, I::I64Const(1), I::I64Add)
    ) {
        return None;
    }
    Some((counter, end + 9))
}
