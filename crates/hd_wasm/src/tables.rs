//! Dense choices of scalar constants become immutable lookup tables.
//! The final row preserves the original else for every out-of-range input.
use crate::module::{Func, HEAP, Ins, Val, mem};
use crate::op::Op as I;
use wasm_encoder::{BlockType, ValType};

#[derive(Clone)]
struct Row {
    values: Vec<(Val, Option<u32>, u64)>,
}

fn scalar(ins: Ins) -> Option<(Val, u64)> {
    match ins {
        Ins::Op(I::I32Const(v)) => Some((Val::I32, u64::from(v.cast_unsigned()))),
        Ins::Op(I::I64Const(v)) => Some((Val::I64, v.cast_unsigned())),
        Ins::Op(I::F32Const(v)) => Some((Val::F32, u64::from(v.bits()))),
        Ins::Op(I::F64Const(v)) => Some((Val::F64, v.bits())),
        _ => None,
    }
}

fn row(body: &[Ins], suffix: &[Ins], result: Option<Val>) -> Option<Row> {
    let mut stack = Vec::new();
    let mut values = Vec::new();
    for &ins in body.iter().chain(suffix) {
        if let Some(value) = scalar(ins) {
            stack.push(value);
        } else {
            match ins {
                Ins::Op(I::LocalSet(local)) => {
                    let (ty, bits) = stack.pop()?;
                    values.push((ty, Some(local), bits));
                }
                Ins::Op(I::Drop) => {
                    stack.pop()?;
                }
                _ => return None,
            }
        }
    }
    match (result, stack.as_slice()) {
        (Some(want), &[(ty, bits)]) if want == ty => values.push((ty, None, bits)),
        (None, []) => {}
        _ => return None,
    }
    (!values.is_empty()).then_some(Row { values })
}

fn condition(body: &[Ins], start: usize, case: u32) -> Option<(u32, usize)> {
    let Ins::Op(I::LocalGet(local)) = *body.get(start)? else {
        return None;
    };
    if case == 0 && matches!(body.get(start + 1), Some(Ins::Op(I::I32Eqz))) {
        Some((local, start + 2))
    } else if matches!(body.get(start + 1), Some(Ins::Op(I::I32Const(value))) if value.cast_unsigned() == case)
        && matches!(body.get(start + 2), Some(Ins::Op(I::I32Eq)))
    {
        Some((local, start + 3))
    } else {
        None
    }
}

fn links(body: &[Ins]) -> Vec<Option<(usize, usize)>> {
    let mut result = vec![None; body.len()];
    let mut stack = Vec::new();
    for (at, ins) in body.iter().enumerate() {
        match ins {
            Ins::Op(I::If(_)) => stack.push((at, None)),
            Ins::Op(I::Block(_) | I::Loop(_)) => stack.push((usize::MAX, None)),
            Ins::Op(I::Else) => {
                if let Some((_, otherwise)) = stack.last_mut() {
                    *otherwise = Some(at);
                }
            }
            Ins::Op(I::End) => {
                if let Some((open, Some(otherwise))) = stack.pop()
                    && open != usize::MAX
                {
                    result[open] = Some((otherwise, at));
                }
            }
            _ => {}
        }
    }
    result
}

struct Choice {
    end: usize,
    local: u32,
    rows: Vec<Row>,
}

fn choice(body: &[Ins], spans: &[Option<(usize, usize)>], start: usize) -> Option<Choice> {
    let (local, first) = condition(body, start, 0)?;
    let result = match *body.get(first)? {
        Ins::Op(I::If(BlockType::Empty)) => None,
        Ins::Op(I::If(BlockType::Result(ty))) => Some(match ty {
            ValType::I32 => Val::I32,
            ValType::I64 => Val::I64,
            ValType::F32 => Val::F32,
            ValType::F64 => Val::F64,
            _ => return None,
        }),
        _ => return None,
    };
    let end = spans.get(first)?.as_ref()?.1;
    let mut rows = Vec::new();
    let mut current = first;
    let mut suffix = Vec::new();
    loop {
        let (otherwise, close) = spans.get(current)?.as_ref().copied()?;
        rows.push(row(&body[current + 1..otherwise], &suffix, result)?);
        let next = otherwise + 1;
        let case = u32::try_from(rows.len()).ok()?;
        if let Some((same, inner)) = condition(body, next, case)
            && same == local
            && let Some((_, inner_end)) = spans.get(inner).copied().flatten()
            && inner_end < close
        {
            let mut tail = body[inner_end + 1..close].to_vec();
            tail.extend(suffix);
            suffix = tail;
            current = inner;
        } else {
            rows.push(row(&body[next..close], &suffix, result)?);
            break;
        }
    }
    // One binary choice is already a cheap native select. Tables represent
    // multiway selections, with at least two explicit cases and the else.
    if rows.len() < 3 {
        return None;
    }
    // A result the next instruction drops needs no column.
    let mut end = end;
    if result.is_some()
        && matches!(body.get(end + 1), Some(Ins::Op(I::Drop)))
        && rows[0].values.len() > 1
    {
        for r in &mut rows {
            r.values.retain(|v| v.1.is_some());
        }
        end += 1;
    }
    let columns = &rows[0].values;
    if rows.iter().any(|r| {
        r.values.len() != columns.len()
            || r.values
                .iter()
                .zip(columns)
                .any(|(a, b)| (a.0, a.1) != (b.0, b.1))
    }) {
        return None;
    }
    Some(Choice { end, local, rows })
}

/// Rewrites pure constant decisions only. Calls, stores, arbitrary branches,
/// and reads of other locals cannot become table entries.
pub(crate) fn lower(f: &mut Func, mut allocate: impl FnMut(&[u8], u32) -> u32) -> bool {
    if f.raw.is_some() || !f.body.iter().any(|i| matches!(i, Ins::Op(I::If(_)))) {
        return false;
    }
    let semantic: Vec<_> = f
        .body
        .iter()
        .enumerate()
        .filter(|(_, i)| !matches!(i, Ins::Anchor(_)))
        .map(|(at, i)| (at, *i))
        .collect();
    let body: Vec<_> = semantic.iter().map(|(_, i)| *i).collect();
    let spans = links(&body);
    let mut plans = Vec::new();
    let mut at = 0;
    while at < body.len() {
        if let Some(found) = choice(&body, &spans, at) {
            let end = found.end;
            plans.push((semantic[at].0, semantic[end].0 + 1, found));
            at = end + 1;
        } else {
            at += 1;
        }
    }
    if plans.is_empty() {
        return false;
    }
    let original = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    let mut consumed = 0;
    for (start, end, plan) in plans {
        for at in consumed..start {
            f.origin_at = origins.as_ref().and_then(|o| o[at]);
            f.push(original[at]);
        }
        f.origin_at = origins.as_ref().and_then(|o| o[start]);
        let mut stride = 0usize;
        let mut align = 4usize;
        let columns: Vec<_> = plan.rows[0]
            .values
            .iter()
            .map(|&(ty, local, _)| {
                let width = if matches!(ty, Val::I64 | Val::F64) {
                    8
                } else {
                    4
                };
                align = align.max(width);
                stride = stride.next_multiple_of(width);
                let offset = stride;
                stride += width;
                (ty, local, offset)
            })
            .collect();
        stride = stride.next_multiple_of(align);
        let mut bytes = vec![0; stride * plan.rows.len()];
        for (row, values) in plan.rows.iter().enumerate() {
            for ((_, _, bits), (ty, _, offset)) in values.values.iter().zip(&columns) {
                let width = if matches!(ty, Val::I64 | Val::F64) {
                    8
                } else {
                    4
                };
                bytes[row * stride + offset..row * stride + offset + width]
                    .copy_from_slice(&bits.to_le_bytes()[..width]);
            }
        }
        let address = allocate(&bytes, u32::try_from(align).expect("alignment"));
        let last = u32::try_from(plan.rows.len() - 1).expect("table rows");
        let slot = f.local(Val::I32);
        f.get(plan.local);
        f.u32(last);
        f.get(plan.local);
        f.u32(last);
        f.op(I::I32LtU);
        f.op(I::Select);
        f.u32(u32::try_from(stride).expect("table stride"));
        f.op(I::I32Mul);
        f.static_addr(address);
        f.op(I::I32Add);
        f.set(slot);
        for (ty, local, offset) in columns {
            f.get(slot);
            let load = mem(
                HEAP,
                u64::try_from(offset).expect("column"),
                if matches!(ty, Val::I64 | Val::F64) {
                    3
                } else {
                    2
                },
            );
            f.op(match ty {
                Val::I32 => I::I32Load(load),
                Val::I64 => I::I64Load(load),
                Val::F32 => I::F32Load(load),
                Val::F64 => I::F64Load(load),
            });
            if let Some(local) = local {
                f.set(local);
            }
        }
        consumed = end;
    }
    for at in consumed..original.len() {
        f.origin_at = origins.as_ref().and_then(|o| o[at]);
        f.push(original[at]);
    }
    f.origin_at = outer;
    f.rebuild_callees();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{Module, Sym};
    use hd_base::Hash128;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    #[test]
    fn dense_tables_preserve_scalar_bits_and_the_default_for_any_index() {
        let mut module = Module::default();
        let kinds = [Val::I32, Val::I64, Val::F32, Val::F64];
        for (id, ty) in kinds.into_iter().enumerate() {
            let name = ["i32", "i64", "f32", "f64"][id];
            let mut f = module.func(name, vec![Val::I32], vec![ty]);
            let constant = |case| match ty {
                Val::I32 => I::I32Const([-7, 44, 9][case]),
                Val::I64 => I::I64Const([i64::MIN, i64::MAX, -3][case]),
                Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(
                    [(-0.0f32).to_bits(), 1.5f32.to_bits(), 0x7fc0_1234][case],
                )),
                Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(
                    [(-0.0f64).to_bits(), 1.5f64.to_bits(), 0x7ff8_0000_0000_1234][case],
                )),
            };
            f.get(0);
            f.op(I::I32Eqz);
            f.op(I::If(BlockType::Result(ty.wasm())));
            f.op(constant(0));
            f.op(I::Else);
            f.get(0);
            f.i32(1);
            f.op(I::I32Eq);
            f.op(I::If(BlockType::Result(ty.wasm())));
            f.op(constant(1));
            f.op(I::Else);
            f.op(constant(2));
            f.op(I::End);
            f.op(I::End);
            let key = Sym::Inst(Hash128(u128::try_from(id + 1).expect("id")));
            module.define(key.clone(), f);
            module.export(name, key);
        }
        let wasm = module.finish().expect("table module");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&wasm)
            .expect("valid table loads");
        let mut loads = 0;
        for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.expect("payload") {
                for op in body.get_operators_reader().expect("operators") {
                    match op.expect("operator") {
                        wasmparser::Operator::If { .. } => panic!("constant choice kept a branch"),
                        wasmparser::Operator::I32Load { .. }
                        | wasmparser::Operator::I64Load { .. }
                        | wasmparser::Operator::F32Load { .. }
                        | wasmparser::Operator::F64Load { .. } => loads += 1,
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(loads, 4);
        let script = format!(
            r"
import assert from 'node:assert/strict';
const e = (await WebAssembly.instantiate(new Uint8Array({wasm:?}), {{}})).instance.exports;
for (const [input, index] of [[0,0],[1,1],[2,2],[3,2],[0xffffffff,2],[-1,2]]) {{
  assert.equal(e.i32(input), [-7,44,9][index]);
  assert.equal(e.i64(input), [-(1n<<63n),(1n<<63n)-1n,-3n][index]);
  for (const name of ['f32','f64']) {{
    if (index === 0) assert.ok(Object.is(e[name](input), -0));
    else if (index === 1) assert.equal(e[name](input), 1.5);
    else assert.ok(Number.isNaN(e[name](input)));
  }}
}}
"
        );
        let mut child = Command::new("node")
            .arg("--input-type=module")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("script");
        let result = child.wait_with_output().expect("table program");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
