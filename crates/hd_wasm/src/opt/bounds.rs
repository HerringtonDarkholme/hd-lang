//! Guarded range-check elimination for counted, possibly unrolled loops.
//!
//! As LLVM's `LoopVersioning` does, select a proven fast loop with a pure
//! preheader guard and retain the original loop otherwise. In particular,
//! do not move a trapping check ahead of an early exit.
//! <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/LoopVersioning.cpp>

use hd_base::HashSet;
use wasm_encoder::BlockType::Empty;

use super::{Func, I, Ins, Remap};

struct Structure {
    end: Vec<usize>,
    depth: Vec<usize>,
    has_else: Vec<bool>,
    loops: Vec<usize>,
}

impl Structure {
    fn new(body: &[Ins], visit: &mut impl FnMut()) -> Option<Self> {
        let mut out = Self {
            end: vec![usize::MAX; body.len()],
            depth: vec![0; body.len()],
            has_else: vec![false; body.len()],
            loops: vec![0; body.len() + 1],
        };
        let mut stack = Vec::new();
        for (at, ins) in body.iter().enumerate() {
            visit();
            out.depth[at] = stack.len();
            out.loops[at + 1] = out.loops[at] + usize::from(matches!(ins, Ins::Op(I::Loop(_))));
            if matches!(ins, Ins::Op(I::Block(_) | I::Loop(_) | I::If(_))) {
                stack.push(at);
            } else if matches!(ins, Ins::Op(I::End)) {
                out.end[stack.pop()?] = at;
            } else if matches!(ins, Ins::Op(I::Else)) {
                let &start = stack.last()?;
                if !matches!(body[start], Ins::Op(I::If(_))) {
                    return None;
                }
                out.has_else[start] = true;
            }
        }
        stack.is_empty().then_some(out)
    }
}

struct Check {
    start: usize,
    end: usize,
    tee: Option<usize>,
    length: u32,
}

struct Plan {
    start: usize,
    end: usize,
    current: u32,
    limit: u32,
    width: u32,
    lengths: Vec<u32>,
    checks: Vec<Check>,
}

fn op(body: &[Ins], at: usize) -> Option<I> {
    match body.get(at)? {
        Ins::Op(op) => Some(*op),
        _ => None,
    }
}

fn header(body: &[Ins], at: usize) -> Option<(u32, u32, u32, usize)> {
    match body.get(at + 1..at + 9)? {
        [
            Ins::Op(I::LocalGet(cur)),
            Ins::Op(I::I32Const(width)),
            Ins::Op(I::I32Add),
            Ins::Op(I::LocalGet(limit)),
            Ins::Op(I::I32LeU),
            Ins::Op(I::I32Eqz),
            Ins::Op(I::BrIf(1)),
            ..,
        ] if *width > 0 => Some((*cur, *limit, width.cast_unsigned(), at + 8)),
        _ => match body.get(at + 1..at + 6)? {
            [
                Ins::Op(I::LocalGet(cur)),
                Ins::Op(I::LocalGet(limit)),
                Ins::Op(I::I32LtU),
                Ins::Op(I::I32Eqz),
                Ins::Op(I::BrIf(1)),
            ] => Some((*cur, *limit, 1, at + 6)),
            _ => None,
        },
    }
}

fn check(body: &[Ins], s: &Structure, at: usize, current: u32, width: u32) -> Option<Check> {
    if !matches!(op(body, at), Some(I::I32GeU))
        || !matches!(op(body, at + 1), Some(I::If(Empty)))
        || s.has_else[at + 1]
    {
        return None;
    }
    let I::LocalGet(length) = op(body, at.checked_sub(1)?)? else {
        return None;
    };
    let operand = at.checked_sub(2)?;
    let (start, tee) = match op(body, operand)? {
        I::LocalGet(local) if local == current => (operand, None),
        I::LocalTee(_) if operand >= 3 => match body.get(operand - 3..operand)? {
            [
                Ins::Op(I::LocalGet(local)),
                Ins::Op(I::I32Const(offset)),
                Ins::Op(I::I32Add),
            ] if *local == current && offset.cast_unsigned() < width => {
                (operand + 1, Some(operand))
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(Check {
        start,
        end: s.end[at + 1].checked_add(1)?,
        tee,
        length,
    })
}

fn labels_fit(f: &Func, ins: &Ins, inner: usize, visit: &mut impl FnMut()) -> bool {
    let fits = |target: u32| (target as usize) < inner || target.checked_add(1).is_some();
    match ins {
        Ins::Op(I::Br(target) | I::BrIf(target)) => fits(*target),
        Ins::Op(I::BrTable(id, default)) => {
            fits(*default)
                && f.label_list(*id).iter().all(|&t| {
                    visit();
                    fits(t)
                })
        }
        _ => true,
    }
}

fn plan(f: &Func, s: &Structure, loop_at: usize, visit: &mut impl FnMut()) -> Option<Plan> {
    let body = &f.body;
    let start = loop_at.checked_sub(1)?;
    let loop_end = *s.end.get(loop_at)?;
    if !matches!(op(body, start), Some(I::Block(Empty)))
        || !matches!(op(body, loop_at), Some(I::Loop(Empty)))
        || loop_end.checked_add(1)? != s.end[start]
        || s.loops[loop_end] != s.loops[loop_at + 1]
    {
        return None;
    }
    let (current, limit, width, first) = header(body, loop_at)?;
    let mut writes = HashSet::default();
    for at in first..loop_end {
        visit();
        let inner = s.depth[at] - s.depth[start];
        if !labels_fit(f, &body[at], inner, visit) {
            return None;
        }
        if matches!(body[at], Ins::LoadAll) {
            return None;
        }
        if let Ins::Op(I::LocalSet(local) | I::LocalTee(local)) = body[at] {
            writes.insert(local);
            if local == current {
                if matches!(body[at], Ins::Op(I::LocalTee(_))) {
                    return None;
                }
                let I::Br(target) = op(body, at + 1)? else {
                    return None;
                };
                let to_loop = s.depth[at + 1] - s.depth[loop_at] - 1;
                if (target as usize) < to_loop {
                    return None;
                }
                // A write followed by a loop exit needs no induction fact.
                // A backedge must preserve current <= limit for the next test.
                if target as usize == to_loop {
                    let valid = matches!(body.get(at.checked_sub(3)?..at)?,
                        [Ins::Op(I::LocalGet(local)), Ins::Op(I::I32Const(step)), Ins::Op(I::I32Add)]
                            if *local == current && step.cast_unsigned() <= width);
                    if !valid {
                        return None;
                    }
                }
            }
        }
    }
    if writes.contains(&limit) {
        return None;
    }
    let mut checks: Vec<Check> = Vec::new();
    let mut lengths = Vec::new();
    let mut seen = HashSet::default();
    for at in first..loop_end {
        visit();
        let Some(check) = check(body, s, at, current, width) else {
            continue;
        };
        if check.end > loop_end
            || writes.contains(&check.length)
            || checks.last().is_some_and(|old| check.start < old.end)
        {
            continue;
        }
        if seen.insert(check.length) {
            lengths.push(check.length);
        }
        checks.push(check);
    }
    // A version guard amortizes at least two dynamic range checks.
    (checks.len() >= 2).then_some(Plan {
        start,
        end: loop_end + 2,
        current,
        limit,
        width,
        lengths,
        checks,
    })
}

pub fn run(f: &mut Func) -> bool {
    rewrite(f, &mut || {})
}

// O(N+B) time/storage. Only innermost loops are planned, so their bodies
// are disjoint. Each is emitted at most twice, once in each version; no
// retry or depth multiplication. B counts branch-table entries inspected.
pub fn rewrite(f: &mut Func, visit: &mut impl FnMut()) -> bool {
    if !f.body.iter().any(|ins| {
        visit();
        matches!(ins, Ins::Op(I::Loop(_)))
    }) {
        return false;
    }
    let Some(s) = Structure::new(&f.body, visit) else {
        return false;
    };
    let mut plans = Vec::new();
    for at in 0..f.body.len() {
        visit();
        if matches!(op(&f.body, at), Some(I::Loop(Empty)))
            && let Some(p) = plan(f, &s, at, visit)
        {
            plans.push(p);
        }
    }
    if plans.is_empty() {
        return false;
    }
    let mut origins = Remap::new(f.origins.as_deref());
    let source = std::mem::take(&mut f.body);
    let mut out = Vec::with_capacity(source.len());
    let mut at = 0;
    for p in plans {
        out.extend_from_slice(&source[at..p.start]);
        origins.copy(at..p.start);
        visit();
        let before = out.len();
        out.extend(
            [
                I::LocalGet(p.current),
                I::LocalGet(p.limit),
                I::I32LeU,
                I::LocalGet(p.limit),
                I::I32Const((u32::MAX - p.width).cast_signed()),
                I::I32LeU,
                I::I32And,
            ]
            .map(Ins::Op),
        );
        for &len in &p.lengths {
            out.extend([I::LocalGet(p.limit), I::LocalGet(len), I::I32LeU, I::I32And].map(Ins::Op));
        }
        out.push(Ins::Op(I::If(Empty)));
        origins.merged(p.start..p.start + 2, out.len() - before);
        emit(f, &source, &s, &p, true, (&mut out, &mut origins), visit);
        out.push(Ins::Op(I::Else));
        origins.merged(p.start..p.start + 2, 1);
        emit(f, &source, &s, &p, false, (&mut out, &mut origins), visit);
        out.push(Ins::Op(I::End));
        origins.merged(p.start..p.start + 2, 1);
        at = p.end;
    }
    out.extend_from_slice(&source[at..]);
    origins.copy(at..source.len());
    f.body = out;
    origins.finish(f);
    true
}

fn emit(
    f: &mut Func,
    source: &[Ins],
    s: &Structure,
    p: &Plan,
    fast: bool,
    output: (&mut Vec<Ins>, &mut Remap),
    visit: &mut impl FnMut(),
) {
    let (out, origins) = output;
    let mut check = 0;
    let mut at = p.start;
    while at < p.end {
        visit();
        if fast && let Some(c) = p.checks.get(check) {
            if c.tee == Some(at)
                && let Ins::Op(I::LocalTee(local)) = source[at]
            {
                out.push(Ins::Op(I::LocalSet(local)));
                origins.copy(at..at + 1);
                at += 1;
                continue;
            }
            if at == c.start {
                at = c.end;
                check += 1;
                continue;
            }
        }
        let inner = s.depth[at] - s.depth[p.start];
        // ruled out: planning checked every escaping label has room for
        // the one enclosing version-selection If added here.
        let adjust = |target: u32| {
            if (target as usize) < inner {
                target
            } else {
                target + 1
            }
        };
        let ins = match source[at] {
            Ins::Op(I::Br(t)) => Ins::Op(I::Br(adjust(t))),
            Ins::Op(I::BrIf(t)) => Ins::Op(I::BrIf(adjust(t))),
            Ins::Op(I::BrTable(id, default)) => {
                let labels = f.label_list(id).iter().copied().map(adjust).collect();
                let id = f.intern_labels(labels);
                Ins::Op(I::BrTable(id, adjust(default)))
            }
            other => other,
        };
        out.push(ins);
        origins.copy(at..at + 1);
        at += 1;
    }
}
