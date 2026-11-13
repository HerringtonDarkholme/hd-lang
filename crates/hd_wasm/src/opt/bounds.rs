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
        if matches!(body[at], Ins::LoadAll | Ins::Save(_)) {
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

pub(super) fn run(f: &mut Func) -> bool {
    rewrite(f, &mut || {})
}

// O(N+B) time/storage. Only innermost loops are planned, so their bodies
// are disjoint. Each is emitted at most twice, once in each version; no
// retry or depth multiplication. B counts branch-table entries inspected.
fn rewrite(f: &mut Func, visit: &mut impl FnMut()) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{Module, Sym, Val};
    use hd_base::Hash128;
    use std::io::Write;

    fn scan(last_offset: i32, escape: bool, table: bool) -> Func {
        let mut f = Module::default().func("scan", vec![Val::I32; 4], vec![Val::I32; 2]);
        f.locals = vec![Val::I32; 2];
        f.op(I::I32Const(-1));
        f.op(I::LocalSet(4));
        if escape {
            f.op(I::Block(Empty));
        }
        f.op(I::Block(Empty));
        f.op(I::Loop(Empty));
        for op in [
            I::LocalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::LocalGet(1),
            I::I32LeU,
            I::I32Eqz,
            I::BrIf(1),
        ] {
            f.op(op);
        }
        for offset in [0, 1, 2, last_offset] {
            f.op(I::LocalGet(0));
            let index = if offset == 0 {
                0
            } else {
                f.op(I::I32Const(offset));
                f.op(I::I32Add);
                f.op(I::LocalTee(5));
                5
            };
            for op in [
                I::LocalGet(2),
                I::I32GeU,
                I::If(Empty),
                I::Unreachable,
                I::End,
                I::LocalGet(index),
                I::LocalGet(3),
                I::I32Eq,
                I::If(Empty),
                I::LocalGet(index),
                I::LocalSet(4),
            ] {
                f.op(op);
            }
            let target = if escape { 3 } else { 2 };
            if table {
                f.op(I::I32Const(0));
                f.br_table(vec![target], target);
            } else {
                f.op(I::Br(target));
            }
            f.op(I::End);
        }
        for op in [
            I::LocalGet(0),
            I::I32Const(4),
            I::I32Add,
            I::LocalSet(0),
            I::Br(0),
            I::End,
            I::End,
        ] {
            f.op(op);
        }
        if escape {
            for op in [I::I32Const(99), I::LocalSet(4), I::End] {
                f.op(op);
            }
        }
        f.op(I::LocalGet(4));
        f.op(I::LocalGet(0));
        f.origins = Some(vec![Some(7); f.body.len()]);
        f
    }

    fn wasm(f: Func) -> Vec<u8> {
        let mut module = Module::default();
        // These generated bodies have no symbolic calls or globals.
        let mut local = module.func(f.name, f.params, f.results);
        local.locals = f.locals;
        local.body = f.body;
        local.labels = f.labels;
        local.origins = f.origins;
        module.define(Sym::Inst(Hash128(1760)), local);
        module.export("scan", Sym::Inst(Hash128(1760)));
        let bytes = module.finish().expect("range test module");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("valid versioned control flow");
        bytes
    }

    fn compare(base: Func) {
        let mut head = base.clone();
        assert!(run(&mut head));
        assert_eq!(head.origins.as_ref().map(Vec::len), Some(head.body.len()));
        let base = wasm(base);
        let head = wasm(head);
        let script = format!(
            r"
const a = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({base:?}))).exports.scan;
const b = new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array({head:?}))).exports.scan;
const call = (f, args) => {{try {{ return f(...args); }} catch(e) {{ if(e instanceof WebAssembly.RuntimeError) return 'trap'; throw e; }}}};
if(JSON.stringify(call(a,[0,8,1,0]))!=='[0,0]') throw Error('early exit must precede the invalid later index');
if(call(a,[0,8,1,-1])!=='trap' || call(a,[-3,5,5,0])!=='trap') throw Error('reached invalid indices must trap');
const cases = [
 [0,8,8,3], [0,8,8,-1], [4,12,12,7],
 [0,8,1,0], [0,8,1,-1], [2,3,0,-1],
 [-3,5,5,0], [-3,-1,-1,-3], [0,4,4,-1], [4,4,0,-1]
];
for (const args of cases) {{const x=call(a,args), y=call(b,args); if(JSON.stringify(x)!==JSON.stringify(y)) throw Error(JSON.stringify({{args,x,y}}));}}
console.log('ok');
"
        );
        // cost: one Node process per comparison of two fixed-size test modules.
        let mut child = std::process::Command::new("node")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // cost: the process makes ten bounded scan comparisons, independent of repository size.
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("script");
        let result = child.wait_with_output().expect("range comparisons");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, b"ok\n");
    }

    #[test]
    fn versions_preserve_early_exits_zero_trips_and_wraparound() {
        compare(scan(3, false, false));
        compare(scan(4, false, false));
    }

    #[test]
    fn versions_adjust_branches_and_branch_tables_past_the_region() {
        compare(scan(3, true, false));
        compare(scan(3, true, true));
    }

    #[test]
    fn changing_lengths_bounds_and_unproved_counter_updates_keep_checks() {
        for local in [1, 2, 0] {
            let mut f = scan(3, false, false);
            let end = f
                .body
                .iter()
                .rposition(|x| matches!(x, Ins::Op(I::Br(0))))
                .expect("backedge");
            f.body.splice(
                end..end,
                [Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(local))],
            );
            f.origins = None;
            assert!(!run(&mut f));
        }
    }

    #[test]
    fn version_work_and_output_are_linear_at_width_1000_depth_20() {
        for (width, depth) in [(1usize, 1usize), (1000, 1), (1, 20), (1000, 20)] {
            let one = scan(3, false, false);
            let region = &one.body[2..one.body.len() - 2];
            let mut f = one.clone();
            f.results.clear();
            f.body = [I::Block(Empty), I::Loop(Empty)]
                .into_iter()
                .map(Ins::Op)
                .cycle()
                .take(2 * depth)
                .collect();
            for _ in 0..width {
                f.body.extend_from_slice(region);
            }
            f.body.extend(vec![Ins::Op(I::End); 2 * depth]);
            f.origins = Some(vec![Some(7); f.body.len()]);
            let n = f.body.len();
            let mut work = 0;
            assert!(rewrite(&mut f, &mut || work += 1));
            assert!(work <= 8 * n, "{work} > 8*{n}");
            assert!(f.body.len() <= 3 * n);
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Op(I::Loop(_))))
                    .count(),
                2 * width + depth
            );
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
        }
    }
}
