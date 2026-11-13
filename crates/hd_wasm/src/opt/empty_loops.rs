//! Delete finite, unobserved induction loops after instance folding.
//!
//! A strict comparison followed by a unit increment reaches its bound
//! without wrapping, for both signed and unsigned integers. Larger steps,
//! inclusive comparisons and arbitrary backedges do not carry that proof.
//! This is the local form of LLVM `LoopDeletion`'s finite, side-effect-free
//! loop rule; it needs no general scalar-evolution solver.
//! <https://llvm.org/doxygen/LoopDeletion_8cpp_source.html>

use super::{Func, I, Ins, Remap};

pub(super) fn run(f: &mut Func) -> bool {
    rewrite(f, &mut || {})
}

// O(N + L) time/storage: count reads, recognize disjoint straight-line
// candidates, rebuild once. A failed candidate stops at its first control
// instruction, so nested loops never rescan an enclosing body. No retries
// or native recursion. `visit` counts examined input instructions.
fn rewrite(f: &mut Func, visit: &mut impl FnMut()) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Val;
    use wasm_encoder::BlockType::Empty;

    fn finite(step: i32) -> Vec<Ins> {
        vec![
            I::Block(Empty),
            I::Loop(Empty),
            I::LocalGet(1),
            I::LocalGet(0),
            I::I32LtU,
            I::I32Eqz,
            I::BrIf(1),
            I::LocalGet(1),
            I::I32Const(step),
            I::I32Add,
            I::LocalSet(1),
            I::Br(0),
            I::End,
            I::End,
        ]
        .into_iter()
        .map(Ins::Op)
        .collect()
    }

    fn function(body: Vec<Ins>, locals: usize) -> Func {
        let mut f = crate::module::Module::default().func("empty", vec![Val::I32], vec![]);
        f.locals = vec![Val::I32; locals];
        f.body = body;
        f.origins = Some(vec![Some(7); f.body.len()]);
        f
    }

    #[test]
    fn deletes_only_finite_unobserved_effect_free_loops() {
        let mut f = function(finite(1), 1);
        assert!(run(&mut f));
        assert_eq!(f.body.len(), 2);
        assert_eq!(f.origins, Some(vec![Some(7); 2]));
        for body in [
            finite(2),
            {
                let mut body = finite(1);
                body.push(Ins::Op(I::LocalGet(1)));
                body.push(Ins::Op(I::Drop));
                body
            },
            {
                let mut body = finite(1);
                body.insert(8, Ins::Op(I::Unreachable));
                body
            },
            {
                let mut body = finite(1);
                body[4] = Ins::Op(I::I32LeU);
                body
            },
            {
                let mut body = finite(1);
                body[3] = Ins::Op(I::LocalGet(1));
                body
            },
        ] {
            let mut f = function(body, 1);
            assert!(!run(&mut f));
        }
    }

    #[test]
    fn signed_and_wide_strict_bounds_are_finite_but_calls_stay() {
        for comparison in [I::I32LtS, I::I64LtU, I::I64LtS] {
            let wide = !matches!(comparison, I::I32LtS);
            let mut body = finite(1);
            body[4] = Ins::Op(comparison);
            if wide {
                body[8] = Ins::Op(I::I64Const(1));
                body[9] = Ins::Op(I::I64Add);
            }
            let mut f = function(body, 1);
            if wide {
                f.params[0] = Val::I64;
                f.locals[0] = Val::I64;
            }
            assert!(run(&mut f));
        }
        let mut body = finite(1);
        body.insert(7, Ins::Call(crate::module::SymId(0)));
        let mut f = function(body, 1);
        assert!(!run(&mut f));
    }

    #[test]
    fn work_is_linear_at_width_1000_and_depth_20() {
        for width in [1, 1000] {
            let mut body = vec![Ins::Op(I::Block(Empty)); 20];
            for k in 0..width {
                let mut loop_body = finite(1);
                for ins in &mut loop_body {
                    if let Ins::Op(I::LocalGet(local) | I::LocalSet(local)) = ins
                        && *local == 1
                    {
                        *local += k;
                    }
                }
                body.extend(loop_body);
            }
            body.extend(vec![Ins::Op(I::End); 20]);
            let mut f = function(body, width as usize);
            let n = f.body.len();
            let mut visits = 0;
            assert!(rewrite(&mut f, &mut || visits += 1));
            assert!(visits <= 4 * n, "{visits} visits for {n} instructions");
        }
    }
}
