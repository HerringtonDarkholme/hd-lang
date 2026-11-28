//! Checked evidence abstraction/application, following GHC's WpEvLam/WpEvApp
//! and superclass selectors (GHC/Tc/Types/Evidence.hs). Unlike GHC's direct
//! dictionary arguments, hd packs dictionaries in a counted flat context.

use super::{CTX_DATA, CTX_ENTRY, CTX_VTABLE, Helper, LIST_ELEMS, LIST_LEN, ctx_elem};
use crate::module::{Func, HEAP, Module, Sym, Val, mem};
use crate::op::Op as I;
use std::collections::BTreeMap;
use wasm_encoder::BlockType;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowEntry {
    pub required: i64,
    pub supplier: i64,
    pub projection: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowAdapter {
    /// Environment first, context last, exactly the function-value ABI.
    pub params: Vec<Val>,
    pub results: Vec<Val>,
    pub entries: Vec<RowEntry>,
    pub open: bool,
    /// Constant-code specialization keeps the original environment directly.
    pub code: Option<hd_base::Hash128>,
}

/// One allocation, O(n log(u+1) + q + sum(projection lengths)) invocation
/// work, where n is incoming length, u unique suppliers, q output entries.
/// Open rows additionally copy and retain n tail entries. Scratch flags live
/// beyond LEN, inside capacity: ordinary context drop/trace never visits them.
/// Code size O(q + sum(paths)); constant scratch locals, not q locals.
pub fn build(m: &Module, row: &RowAdapter) -> Func {
    enum Task {
        Range(usize, usize),
        Else,
        End,
    }
    let mut f = m.func("rt:row_adapter", row.params.clone(), row.results.clone());
    let incoming = hd_tir::ir::u32_of(row.params.len() - 1);
    let q = hd_tir::ir::u32_of(row.entries.len());
    let mut groups: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (at, entry) in row.entries.iter().enumerate() {
        groups.entry(entry.supplier).or_default().push(at);
    }
    let groups: Vec<_> = groups.into_iter().collect();
    let u = hd_tir::ir::u32_of(groups.len());
    let tail = f.local(Val::I32);
    let out = f.local(Val::I32);
    let flags = f.local(Val::I32);
    let at = f.local(Val::I32);
    let end = f.local(Val::I32);
    let id = f.local(Val::I64);
    let remaining = f.local(Val::I32);
    if row.open {
        f.get(incoming);
        f.op(I::If(BlockType::Empty));
        f.get(incoming);
        f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        f.set(tail);
        f.op(I::End);
    }
    f.u32(q + u.div_ceil(CTX_ENTRY));
    f.get(tail);
    f.op(I::I32Add);
    f.helper(Helper::ListBuf { elem: ctx_elem() });
    f.set(out);
    f.get(out);
    f.u32(q);
    f.get(tail);
    f.op(I::I32Add);
    f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(out);
    f.u32(q);
    f.get(tail);
    f.op(I::I32Add);
    f.u32(CTX_ENTRY);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.u32(LIST_ELEMS);
    f.op(I::I32Add);
    f.set(flags);
    f.get(flags);
    f.i32(0);
    f.u32(u);
    f.push(crate::bulk::fill(HEAP));
    if row.open {
        f.get(tail);
        f.op(I::If(BlockType::Empty));
        f.get(out);
        f.u32(LIST_ELEMS + q * CTX_ENTRY);
        f.op(I::I32Add);
        f.set(at);
        f.get(at);
        f.get(incoming);
        f.u32(LIST_ELEMS);
        f.op(I::I32Add);
        f.get(tail);
        f.u32(CTX_ENTRY);
        f.op(I::I32Mul);
        f.push(crate::bulk::copy(HEAP, HEAP));
        f.get(at);
        f.get(tail);
        f.helper(Helper::CopyElems { elem: ctx_elem() });
        f.op(I::End);
    }
    f.u32(u);
    f.set(remaining);
    f.get(incoming);
    f.op(I::If(BlockType::Empty));
    f.get(incoming);
    f.set(at);
    f.get(incoming);
    f.get(incoming);
    f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.u32(CTX_ENTRY);
    f.op(I::I32Mul);
    f.op(I::I32Add);
    f.set(end);
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    f.get(at);
    f.get(end);
    f.op(I::I32GeU);
    f.get(remaining);
    f.op(I::I32Eqz);
    f.op(I::I32Or);
    f.op(I::BrIf(1));
    f.get(at);
    f.op(I::I64Load(mem(HEAP, u64::from(LIST_ELEMS), 3)));
    f.set(id);
    // Iterative generation of a balanced decision tree: no native recursion
    // whose depth is controlled by input, no runtime global hash-map calls.
    let mut tasks = vec![Task::Range(0, groups.len())];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Else => f.op(I::Else),
            Task::End => f.op(I::End),
            Task::Range(lo, hi) if lo < hi => {
                let mid = lo + (hi - lo) / 2;
                f.get(id);
                f.op(I::I64Const(groups[mid].0));
                f.op(I::I64Eq);
                f.op(I::If(BlockType::Empty));
                f.get(flags);
                f.op(I::I32Load8U(mem(HEAP, mid as u64, 0)));
                f.op(I::I32Eqz);
                f.op(I::If(BlockType::Empty));
                f.get(flags);
                f.i32(1);
                f.op(I::I32Store8(mem(HEAP, mid as u64, 0)));
                f.get(remaining);
                f.i32(1);
                f.op(I::I32Sub);
                f.set(remaining);
                for &j in &groups[mid].1 {
                    let entry = &row.entries[j];
                    let off = j as u64 * u64::from(CTX_ENTRY);
                    f.get(out);
                    f.op(I::I64Const(entry.required));
                    f.op(I::I64Store(mem(HEAP, u64::from(LIST_ELEMS) + off, 3)));
                    f.get(out);
                    f.get(at);
                    f.op(I::I32Load(mem(HEAP, CTX_DATA, 2)));
                    f.op(I::I32Store(mem(HEAP, CTX_DATA + off, 2)));
                    f.get(out);
                    f.get(at);
                    f.op(I::I32Load(mem(HEAP, CTX_VTABLE, 2)));
                    for &step in &entry.projection {
                        f.op(I::I32Load(mem(HEAP, u64::from(step), 2)));
                    }
                    f.op(I::I32Store(mem(HEAP, CTX_VTABLE + off, 2)));
                    f.get(at);
                    f.op(I::I32Load(mem(HEAP, CTX_DATA, 2)));
                    f.call(Sym::Inst(m.runtime.retain));
                }
                f.op(I::End);
                f.op(I::Else);
                f.get(id);
                f.op(I::I64Const(groups[mid].0));
                f.op(I::I64LtS);
                f.op(I::If(BlockType::Empty));
                tasks.extend([
                    Task::End,
                    Task::End,
                    Task::Range(mid + 1, hi),
                    Task::Else,
                    Task::Range(lo, mid),
                ]);
            }
            Task::Range(..) => {}
        }
    }
    f.get(at);
    f.u32(CTX_ENTRY);
    f.op(I::I32Add);
    f.set(at);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
    f.op(I::End);
    // ruled out: the checked wrapper's expected row supplies every H.
    f.get(remaining);
    f.op(I::If(BlockType::Empty));
    f.op(I::Unreachable);
    f.op(I::End);
    // The original closure object, at offset 4 of the adapter's.
    f.get(0);
    f.op(I::I32Load(mem(HEAP, 4, 2)));
    for p in 1..incoming {
        f.get(p);
    }
    f.get(out);
    if let Some(code) = &row.code {
        f.call(Sym::Inst(*code));
    } else {
        f.get(0);
        f.op(I::I32Load(mem(HEAP, 4, 2)));
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.call_indirect(row.params.clone(), row.results.clone());
    }
    let results: Vec<_> = row.results.iter().map(|&v| f.local(v)).collect();
    for &r in results.iter().rev() {
        f.set(r);
    }
    f.get(out);
    f.helper(Helper::ReleaseRef);
    for r in results {
        f.get(r);
    }
    f
}
