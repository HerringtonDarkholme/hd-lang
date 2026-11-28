//! Local constant propagation and executable-arm selection after inlining.
//! Like LLVM `InstCombine`, fold operations where their operands become literals.
//!
//! A literal written to a local holds at a later read when the write
//! dominates it and no other write may reach it, as LLVM's `EarlyCSE`
//! keeps a value per dominator-tree scope: the write's frame is still open
//! (structured control enters a frame only at its start), it is the
//! local's last write in walk order, and no loop entered after it writes
//! the local, whose back edge would reach the read.

use super::{I, Ins, branch_frame};
use crate::module::Func;
use crate::origins::Remap;

#[derive(Default)]
struct Frame {
    start: usize,
    end: usize,
    is_loop: bool,
    /// A `block`, whose end joins nothing unless a branch names it.
    plain: bool,
    targeted: bool,
    other: Option<usize>,
    selected: bool,
}

/// O(instructions + locals + branch-table entries), with heap control
/// stacks, plus per folded read the open frames and a binary search of its
/// local's writes. Two input walks and one compaction walk of at most three
/// times the input; there is no nesting-dependent retry.
pub fn run(f: &mut Func, words: &StaticWords<'_>) -> bool {
    run_with(f, words, &mut || {})
}

/// The `i32` words of immutable static data: word `offset / 4` of static
/// vtable `k` ([`crate::module::Module::fn_object`] among them), as the
/// instruction that pushes it, a table slot or another vtable's address;
/// `None` where nothing is known.
pub type StaticWords<'a> = dyn Fn(usize, u64) -> Option<Ins> + Sync + 'a;

pub fn run_with(f: &mut Func, words: &StaticWords<'_>, visit: &mut impl FnMut()) -> bool {
    let mut frames = vec![Frame {
        end: f.body.len(),
        ..Frame::default()
    }];
    let mut ids = vec![0; f.body.len()];
    let mut stack = vec![0];
    // Each local's writes, in walk order.
    let mut writes: Vec<Vec<usize>> = vec![Vec::new(); f.params.len() + f.locals.len()];
    for (at, ins) in f.body.iter().enumerate() {
        visit();
        if let Ins::Op(I::LocalSet(l) | I::LocalTee(l)) = ins
            && let Some(w) = writes.get_mut(*l as usize)
        {
            w.push(at);
        }
        if matches!(ins, Ins::Op(I::If(_) | I::Block(_) | I::Loop(_))) {
            ids[at] = frames.len();
            stack.push(frames.len());
            frames.push(Frame {
                start: at,
                is_loop: matches!(ins, Ins::Op(I::Loop(_))),
                plain: matches!(ins, Ins::Op(I::Block(_))),
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
    // Per local: its last write's literal, position, frame and the frame's
    // arm then: an `else` arm is not dominated by its `then` arm.
    let mut facts: Vec<Option<(Ins, usize, usize, u32)>> =
        vec![None; f.params.len() + f.locals.len()];
    let mut open = vec![0usize];
    let mut arm = vec![0u32; frames.len()];
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(f.body.len());
    let mut changed = false;
    let mut expanded = false;
    // Surviving copy sources can keep their existing locals. Arithmetic users
    // still receive literals, allowing dead constant locals to disappear.
    let mut exposed = Vec::new();
    let mut at = 0;
    while at < f.body.len() {
        visit();
        let ins = f.body[at];
        if matches!(ins, Ins::Op(I::If(_) | I::Block(_) | I::Loop(_))) {
            open.push(ids[at]);
        } else if matches!(ins, Ins::Op(I::End)) && open.len() > 1 && open.last() == Some(&ids[at])
        {
            open.pop();
        } else if matches!(ins, Ins::Op(I::Else)) {
            arm[ids[at]] += 1;
        }
        match ins {
            Ins::Op(I::LocalGet(l))
                if let Some((value, set, id, a)) = facts[l as usize]
                    && arm[id] == a
                    && holds(&frames, &open, &writes[l as usize], (set, id), at) =>
            {
                exposed.push((out.len(), l));
                out.push(value);
                changed = true;
            }
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => {
                let value = out.last().copied().filter(literal);
                // A plain block no branch names ends only by falling
                // through, so what it writes holds past its end: the fact
                // belongs to the nearest frame around that joins.
                let id = open
                    .iter()
                    .rev()
                    .copied()
                    .find(|&k| {
                        let fr = &frames[k];
                        k == 0 || fr.targeted || !(fr.plain || fr.selected)
                    })
                    .unwrap_or(0);
                facts[l as usize] = value.map(|v| (v, at, id, arm[id]));
                if let (Ins::Op(I::LocalTee(_)), Some(value)) = (ins, value) {
                    // Keep the write, but expose the tee's literal result.
                    let producer = pop_exposed(&mut out, &mut exposed);
                    let source = origins.pop().or(origins.first(at..at + 1));
                    if let Some(local) = producer {
                        exposed.push((out.len(), local));
                    }
                    out.extend([value, Ins::Op(I::LocalSet(l)), value]);
                    origins.repeat(source, 3);
                    at += 1;
                    expanded = true;
                    continue;
                }
                out.push(ins);
            }
            Ins::Op(I::If(bt)) => {
                let frame = &mut frames[ids[at]];
                if let Some(Ins::Op(I::I32Const(c))) = out.last() {
                    let take_then = *c != 0;
                    pop_exposed(&mut out, &mut exposed);
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
                out.push(ins);
            }
            Ins::Op(I::Else) if frames[ids[at]].selected => {
                let frame = &frames[ids[at]];
                at = frame.end;
                continue;
            }
            // A load from immutable static data at a constant address is
            // the word stored there: a closure object's code, a vtable's
            // slot or parent.
            Ins::Op(I::I32Load(arg))
                if arg.memory_index == crate::module::HEAP
                    && let Some(&Ins::VTable(k)) = out.last()
                    && let Some(word) = words(k, arg.offset) =>
            {
                let source = origins.pop().or(origins.first(at..at + 1));
                pop_exposed(&mut out, &mut exposed);
                out.push(word);
                origins.repeat(source, 1);
                at += 1;
                changed = true;
                continue;
            }
            Ins::Op(op) => {
                if let Some((n, value)) = evaluate(op, &out) {
                    let mut source = origins.first(at..at + 1);
                    for _ in 0..n {
                        pop_exposed(&mut out, &mut exposed);
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
                    pop_exposed(&mut out, &mut exposed);
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
    if changed || expanded {
        compact_tees(f, &exposed, visit);
    }
    changed
}

/// Whether the literal a local's last write `(set, frame)` gave holds at
/// a read at `at`: the write's frame is open, and no loop opened after the
/// write writes the local after the read (`writes`, its writes in walk
/// order). O(open frames + log writes).
fn holds(
    frames: &[Frame],
    open: &[usize],
    writes: &[usize],
    fact: (usize, usize),
    at: usize,
) -> bool {
    let (set, id) = fact;
    if !open.contains(&id) {
        return false;
    }
    let mut end = at;
    for &k in open.iter().rev() {
        let fr = &frames[k];
        if fr.start < set {
            break;
        }
        if fr.is_loop {
            end = end.max(fr.end);
        }
    }
    let after = writes.partition_point(|&w| w <= at);
    writes.get(after).is_none_or(|&w| w > end)
}

fn pop_exposed(out: &mut Vec<Ins>, exposed: &mut Vec<(usize, u32)>) -> Option<u32> {
    out.pop();
    if exposed.last().is_some_and(|&(at, _)| at == out.len()) {
        exposed.pop().map(|(_, local)| local)
    } else {
        None
    }
}

/// Restore exposed tee results and surviving local-to-local copies. The top
/// hint spans only adjacent literals and tees; no control-flow fact is assumed.
pub fn compact_tees(f: &mut Func, exposed: &[(usize, u32)], visit: &mut impl FnMut()) {
    let mut out = Vec::with_capacity(f.body.len());
    let mut origins = Remap::new(f.origins.as_deref());
    let mut top = None;
    let mut stored = None;
    let mut exposed = exposed.iter().peekable();
    for (at, &ins) in f.body.iter().enumerate() {
        visit();
        let original = if exposed.peek().is_some_and(|&&(index, _)| index == at) {
            exposed
                .next()
                .filter(|_| {
                    matches!(
                        f.body.get(at + 1),
                        Some(Ins::Op(I::LocalSet(_) | I::LocalTee(_)))
                    )
                })
                .map(|&(_, local)| Ins::Op(I::LocalGet(local)))
        } else {
            None
        };
        if literal(&ins) {
            if stored.is_some_and(|value| same_literal(value, ins))
                && let Some(Ins::Op(I::LocalSet(local))) = out.last().copied()
            {
                out.pop();
                out.push(Ins::Op(I::LocalTee(local)));
                top = Some(ins);
                stored = None;
                continue;
            }
            top = Some(ins);
            stored = None;
        } else if matches!(ins, Ins::Op(I::LocalSet(_))) {
            stored = top.take();
        } else if matches!(ins, Ins::Op(I::LocalTee(_))) {
            stored = None;
        } else {
            top = None;
            stored = None;
        }
        out.push(original.unwrap_or(ins));
        origins.copy(at..at + 1);
    }
    f.body = out;
    origins.finish(f);
}

fn same_literal(a: Ins, b: Ins) -> bool {
    match (a, b) {
        (Ins::VTable(a), Ins::VTable(b)) => a == b,
        (Ins::TableSlot(a), Ins::TableSlot(b)) => a == b,
        (Ins::Op(I::I32Const(a)), Ins::Op(I::I32Const(b))) => a == b,
        (Ins::Op(I::I64Const(a)), Ins::Op(I::I64Const(b))) => a == b,
        (Ins::Op(I::F32Const(a)), Ins::Op(I::F32Const(b))) => a.bits() == b.bits(),
        (Ins::Op(I::F64Const(a)), Ins::Op(I::F64Const(b))) => a.bits() == b.bits(),
        _ => false,
    }
}

/// A constant: a number, or an address or a table slot the link fixes.
fn literal(ins: &Ins) -> bool {
    matches!(
        ins,
        Ins::Op(I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_))
            | Ins::VTable(_)
            | Ins::TableSlot(_)
    )
}

#[must_use]
pub fn evaluate(op: I, out: &[Ins]) -> Option<(usize, I)> {
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
