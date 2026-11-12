//! Local constant propagation and executable-arm selection after inlining.
//! Like LLVM `InstCombine`, fold operations where their operands become literals;
//! unknown joins deliberately discard facts instead of solving a global lattice.

use super::{I, Ins, branch_frame};
use crate::module::Func;
use crate::origins::Remap;

#[derive(Default)]
struct Frame {
    end: usize,
    other: Option<usize>,
    targeted: bool,
    selected: bool,
    join: bool,
}

/// O(instructions + locals + branch-table entries), with heap control stacks.
/// Each input instruction is indexed once and rewritten at most once. Epochs
/// invalidate local facts in O(1); there is no nesting-dependent retry.
pub(super) fn run(f: &mut Func) -> bool {
    run_with(f, &mut || {})
}

fn run_with(f: &mut Func, visit: &mut impl FnMut()) -> bool {
    let mut frames = vec![Frame::default()];
    let mut ids = vec![0; f.body.len()];
    let mut stack = vec![0];
    for (at, ins) in f.body.iter().enumerate() {
        visit();
        if matches!(ins, Ins::Op(I::If(_) | I::Block(_) | I::Loop(_))) {
            ids[at] = frames.len();
            stack.push(frames.len());
            frames.push(Frame {
                join: matches!(ins, Ins::Op(I::If(_) | I::Loop(_))),
                ..Frame::default()
            });
        } else if matches!(ins, Ins::Op(I::Else)) {
            let Some(&id) = stack.last() else {
                return false;
            };
            ids[at] = id;
            frames[id].other = Some(at);
        } else if matches!(ins, Ins::Op(I::End)) && stack.len() > 1 {
            let Some(id) = stack.pop() else { return false };
            ids[at] = id;
            frames[id].end = at;
        } else if let Ins::Op(I::Br(depth) | I::BrIf(depth)) = ins {
            let Some(id) = branch_frame(&stack, *depth) else {
                return false;
            };
            frames[id].targeted = true;
        } else if let Ins::Op(I::BrTable(table, default)) = ins {
            for &depth in f.label_list(*table).iter().chain(std::iter::once(default)) {
                visit();
                let Some(id) = branch_frame(&stack, depth) else {
                    return false;
                };
                frames[id].targeted = true;
            }
        }
    }

    if stack.len() != 1 {
        return false;
    }
    let mut facts = vec![(0, None); f.params.len() + f.locals.len()];
    let mut epoch = 1usize;
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(f.body.len());
    let mut changed = false;
    let mut at = 0;
    while at < f.body.len() {
        visit();
        let ins = f.body[at];
        let frame = &mut frames[ids[at]];
        match ins {
            Ins::Op(I::LocalGet(l)) if facts[l as usize].0 == epoch => {
                if let Some(value) = facts[l as usize].1 {
                    out.push(value);
                    changed = true;
                } else {
                    out.push(ins);
                }
            }
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => {
                let value = out.last().copied().filter(literal);
                facts[l as usize] = (epoch, value);
                if let (Ins::Op(I::LocalTee(_)), Some(value)) = (ins, value) {
                    // Keep the write, but expose the tee's literal result.
                    out.pop();
                    let source = origins.pop().or(origins.first(at..at + 1));
                    out.extend([value, Ins::Op(I::LocalSet(l)), value]);
                    origins.repeat(source, 3);
                    at += 1;
                    changed = true;
                    continue;
                }
                out.push(ins);
            }
            Ins::Op(I::If(bt)) => {
                if let Some(Ins::Op(I::I32Const(c))) = out.last() {
                    let take_then = *c != 0;
                    out.pop();
                    origins.pop();
                    out.push(Ins::Op(I::Block(bt)));
                    frame.selected = true;
                    changed = true;
                    origins.copy(at..at + 1);
                    at = if take_then {
                        at + 1
                    } else {
                        frame.other.map_or(frame.end, |e| e + 1)
                    };
                    continue;
                }
                epoch += 1;
                out.push(ins);
            }
            Ins::Op(I::Else) if frame.selected => {
                at = frame.end;
                continue;
            }
            Ins::Op(
                I::Else
                | I::Loop(_)
                | I::Br(_)
                | I::BrIf(_)
                | I::BrTable(..)
                | I::Return
                | I::Unreachable,
            ) => {
                epoch += 1;
                out.push(ins);
            }
            Ins::Op(I::End) => {
                // Unselected ifs and loops join unknown paths. Plain blocks
                // only join paths when a branch actually names their label.
                if frame.targeted || (!frame.selected && frame.join) {
                    epoch += 1;
                }
                out.push(ins);
            }
            Ins::Op(op) => {
                if let Some((n, value)) = evaluate(op, &out) {
                    let mut source = origins.first(at..at + 1);
                    for _ in 0..n {
                        out.pop();
                        source = origins.pop().or(source);
                    }
                    out.push(Ins::Op(value));
                    origins.repeat(source, 1);
                    at += 1;
                    changed = true;
                    continue;
                }
                // Wasm masks shift counts to the operand width.
                let zero_shift = matches!((op, out.last()),
                    (I::I32Shl | I::I32ShrS | I::I32ShrU, Some(Ins::Op(I::I32Const(c)))) if c.trailing_zeros() >= 5)
                    || matches!((op, out.last()),
                    (I::I64Shl | I::I64ShrS | I::I64ShrU, Some(Ins::Op(I::I64Const(c)))) if c.trailing_zeros() >= 6);
                if zero_shift {
                    out.pop();
                    origins.pop();
                    at += 1;
                    changed = true;
                    continue;
                }
                out.push(ins);
            }
            _ => out.push(ins),
        }
        origins.copy(at..at + 1);
        at += 1;
    }
    f.body = out;
    origins.finish(f);
    changed
}

fn literal(ins: &Ins) -> bool {
    matches!(
        ins,
        Ins::Op(I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_))
    )
}

fn evaluate(op: I, out: &[Ins]) -> Option<(usize, I)> {
    let &Ins::Op(last) = out.last()? else {
        return None;
    };
    let unary = match (op, last) {
        (I::I32Eqz, I::I32Const(a)) => Some(I::I32Const(i32::from(a == 0))),
        (I::I64Eqz, I::I64Const(a)) => Some(I::I32Const(i32::from(a == 0))),
        (I::I64ExtendI32U, I::I32Const(a)) => Some(I::I64Const(i64::from(a.cast_unsigned()))),
        (I::I64ExtendI32S, I::I32Const(a)) => Some(I::I64Const(i64::from(a))),
        (I::I32WrapI64, I::I64Const(a)) => {
            let [a, b, c, d, ..] = a.to_le_bytes();
            Some(I::I32Const(i32::from_le_bytes([a, b, c, d])))
        }
        _ => None,
    };
    if let Some(value) = unary {
        return Some((1, value));
    }
    let &Ins::Op(first) = out.get(out.len().checked_sub(2)?)? else {
        return None;
    };
    macro_rules! integers {
        ($a:ident, $b:ident, $const:ident, $bits:literal) => {
            match op {
                I::I32Eq | I::I64Eq => I::I32Const(i32::from($a == $b)),
                I::I32Ne | I::I64Ne => I::I32Const(i32::from($a != $b)),
                I::I32LtS | I::I64LtS => I::I32Const(i32::from($a < $b)),
                I::I32LeS | I::I64LeS => I::I32Const(i32::from($a <= $b)),
                I::I32GtS | I::I64GtS => I::I32Const(i32::from($a > $b)),
                I::I32GeS | I::I64GeS => I::I32Const(i32::from($a >= $b)),
                I::I32LtU | I::I64LtU => {
                    I::I32Const(i32::from($a.cast_unsigned() < $b.cast_unsigned()))
                }
                I::I32LeU | I::I64LeU => {
                    I::I32Const(i32::from($a.cast_unsigned() <= $b.cast_unsigned()))
                }
                I::I32GtU | I::I64GtU => {
                    I::I32Const(i32::from($a.cast_unsigned() > $b.cast_unsigned()))
                }
                I::I32GeU | I::I64GeU => {
                    I::I32Const(i32::from($a.cast_unsigned() >= $b.cast_unsigned()))
                }
                I::I32Add | I::I64Add => I::$const($a.wrapping_add($b)),
                I::I32Sub | I::I64Sub => I::$const($a.wrapping_sub($b)),
                I::I32Mul | I::I64Mul => I::$const($a.wrapping_mul($b)),
                I::I32DivS | I::I64DivS => I::$const($a.checked_div($b)?),
                I::I32DivU | I::I64DivU => I::$const(
                    $a.cast_unsigned()
                        .checked_div($b.cast_unsigned())?
                        .cast_signed(),
                ),
                I::I32RemS | I::I64RemS => {
                    I::$const(if $b == -1 { 0 } else { $a.checked_rem($b)? })
                }
                I::I32RemU | I::I64RemU => I::$const(
                    $a.cast_unsigned()
                        .checked_rem($b.cast_unsigned())?
                        .cast_signed(),
                ),
                I::I32And | I::I64And => I::$const($a & $b),
                I::I32Or | I::I64Or => I::$const($a | $b),
                I::I32Xor | I::I64Xor => I::$const($a ^ $b),
                I::I32Shl | I::I64Shl => {
                    I::$const($a.wrapping_shl(u32::from($b.to_le_bytes()[0]) & ($bits - 1)))
                }
                I::I32ShrS | I::I64ShrS => {
                    I::$const($a.wrapping_shr(u32::from($b.to_le_bytes()[0]) & ($bits - 1)))
                }
                I::I32ShrU | I::I64ShrU => I::$const(
                    $a.cast_unsigned()
                        .wrapping_shr(u32::from($b.to_le_bytes()[0]) & ($bits - 1))
                        .cast_signed(),
                ),
                _ => return None,
            }
        };
    }
    let value = match (first, last) {
        (I::I32Const(a), I::I32Const(b)) => integers!(a, b, I32Const, 32),
        (I::I64Const(a), I::I64Const(b)) => integers!(a, b, I64Const, 64),
        _ => return None,
    };
    Some((2, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Val;
    use wasm_encoder::BlockType;

    fn function(body: Vec<I>) -> Func {
        let mut f = Func::default();
        f.params = vec![Val::I32];
        f.locals = vec![Val::I32, Val::I64];
        f.body = body.into_iter().map(Ins::Op).collect();
        f.origins = Some((0..f.body.len()).map(|i| u32::try_from(i).ok()).collect());
        f
    }

    #[test]
    fn constants_cross_selected_arms_but_effects_and_labels_stay() {
        let mut f = function(vec![
            I::I32Const(32),
            I::LocalTee(0),
            I::LocalSet(1),
            I::LocalGet(0),
            I::I32Const(128),
            I::I32LtU,
            I::If(BlockType::Empty),
            I::LocalGet(1),
            I::I32Const(64),
            I::I32GtU,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::I64Const(7),
            I::LocalSet(2),
            I::Else,
            I::Unreachable,
            I::End,
            I::LocalGet(2),
            I::I32Const(0),
            I::I64ExtendI32U,
            I::I64Shl,
        ]);
        assert!(run(&mut f));
        assert!(!f.body.iter().any(|i| matches!(
            i,
            Ins::Op(I::If(_) | I::Else | I::Unreachable | I::I64Shl | I::I32LtU | I::I32GtU)
        )));
        assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::LocalSet(2)))));
        assert!(matches!(f.body.last(), Some(Ins::Op(I::I64Const(7)))));
        assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
    }

    #[test]
    fn unknown_joins_loop_backedges_and_label_exits_discard_facts() {
        for control in [
            I::If(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::Block(BlockType::Empty),
        ] {
            let mut f = function(vec![
                I::I32Const(7),
                I::LocalSet(1),
                I::LocalGet(0),
                control,
                I::I32Const(8),
                I::LocalSet(1),
                I::Br(0),
                I::I32Const(9),
                I::LocalSet(1),
                I::End,
                I::LocalGet(1),
            ]);
            run(&mut f);
            assert!(matches!(f.body.last(), Some(Ins::Op(I::LocalGet(1)))));
        }
        // A no-else conditional joins its skipped path too.
        let mut f = function(vec![
            I::LocalGet(0),
            I::If(BlockType::Empty),
            I::I32Const(3),
            I::LocalSet(1),
            I::End,
            I::LocalGet(1),
        ]);
        run(&mut f);
        assert!(matches!(f.body.last(), Some(Ins::Op(I::LocalGet(1)))));
    }

    #[test]
    fn signed_unsigned_and_masked_shifts_match_wasm_integer_rules() {
        for (op, expected) in [
            (I::I32LtU, 0),
            (I::I32LtS, 1),
            (I::I32GtU, 1),
            (I::I32GeS, 0),
        ] {
            assert!(
                matches!(evaluate(op, &[Ins::Op(I::I32Const(-1)), Ins::Op(I::I32Const(2))]), Some((2, I::I32Const(value))) if value == expected)
            );
        }
        assert!(matches!(
            evaluate(
                I::I64ShrU,
                &[Ins::Op(I::I64Const(-1)), Ins::Op(I::I64Const(65))]
            ),
            Some((2, I::I64Const(i64::MAX)))
        ));
        let mut f = function(vec![I::LocalGet(0), I::I32Const(32), I::I32Shl]);
        assert!(run(&mut f));
        assert!(matches!(f.body.as_slice(), [Ins::Op(I::LocalGet(0))]));
        // Keep trapping division for runtime; Wasm MIN % -1 is zero.
        assert!(
            evaluate(
                I::I32DivS,
                &[Ins::Op(I::I32Const(1)), Ins::Op(I::I32Const(0))]
            )
            .is_none()
        );
    }

    #[test]
    fn safe_division_and_remainders_fold_without_hiding_traps() {
        for (op, expected) in [(I::I32DivS, -3), (I::I32RemS, -2)] {
            assert!(
                matches!(evaluate(op, &[Ins::Op(I::I32Const(-17)), Ins::Op(I::I32Const(5))]), Some((2, I::I32Const(value))) if value == expected)
            );
        }
        assert!(matches!(
            evaluate(
                I::I64DivU,
                &[Ins::Op(I::I64Const(-1)), Ins::Op(I::I64Const(7))]
            ),
            Some((2, I::I64Const(2_635_249_153_387_078_802)))
        ));
        assert!(matches!(
            evaluate(
                I::I64RemS,
                &[Ins::Op(I::I64Const(i64::MIN)), Ins::Op(I::I64Const(-1))]
            ),
            Some((2, I::I64Const(0)))
        ));
        assert!(
            evaluate(
                I::I64DivS,
                &[Ins::Op(I::I64Const(i64::MIN)), Ins::Op(I::I64Const(-1))]
            )
            .is_none()
        );
        for op in [I::I64DivS, I::I64DivU, I::I64RemS, I::I64RemU] {
            assert!(evaluate(op, &[Ins::Op(I::I64Const(7)), Ins::Op(I::I64Const(0))]).is_none());
        }
    }

    #[test]
    fn generated_width_and_depth_count_linear_work() {
        for (width, depth) in [(1_000, 20), (10_000, 20), (1_000, 1_000)] {
            let mut body = vec![I::Block(BlockType::Empty); depth];
            for _ in 0..width {
                body.extend([I::I32Const(3), I::LocalSet(1), I::LocalGet(1), I::Drop]);
            }
            body.extend(std::iter::repeat_n(I::End, depth));
            let mut f = function(body);
            let n = f.body.len();
            let mut visits = 0;
            run_with(&mut f, &mut || visits += 1);
            assert_eq!(visits, 2 * n);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
        }
    }
}
