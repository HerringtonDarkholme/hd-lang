//! One instance's TIR body to one Wasm function
//! (`crates/hd_wasm/DESIGN.md` §23).

use hd_base::{InstId, StageResult};
use hd_mono::{Sub, subst};
use hd_tir::ir::{Body, NONE, Tag, local_flags};
use hd_types::Ty;

use crate::module::{Func, Module};
use crate::{Input, unsupported};

/// The function of instance `id`, at `slot` in content order.
pub fn instance(input: &Input<'_>, _m: &mut Module, _slot: usize, id: InstId) -> StageResult<Func> {
    let t = &input.collected.table;
    let item = t.item[id.idx()];
    let args = t.args[id.idx()];
    let Sub::Body(sub) = t.sub[id.idx()] else {
        return unsupported("adapters, erased bodies and thunks");
    };
    if sub != 0 {
        return unsupported("closures (job 11)");
    }
    let Some(b) = (input.body)(id) else {
        return unsupported("an instance without TIR");
    };
    if input.env.suspends(item) {
        return unsupported("suspending functions (job 13)");
    }
    if !input.env.row_keys(item, args).is_empty() {
        return unsupported("providers (job 3, #540)");
    }
    let s = |ty: Ty| subst(input.pool, input.env, item, args, ty);
    let mut params = Vec::new();
    for l in param_locals(b) {
        params.extend(crate::layout::vals(input.pool, s(b.local_ty[l]))?);
    }
    let ret = input.env.ret(item).unwrap_or(Ty::VOID);
    let results = crate::layout::vals(input.pool, s(ret))?;
    let name = format!("{}#{}", (input.path)(item), t.sub[id.idx()]);
    let mut f = Func::new(name, params, results.clone());
    let root = b.sub_root[0] as usize;
    block(b, root)?;
    let tail = b.data[root][1];
    if tail == NONE && !results.is_empty() {
        f.op(wasm_encoder::Instruction::Unreachable);
    } else if tail != NONE && !results.is_empty() {
        return unsupported("function bodies with a result (job 3, #540)");
    }
    Ok(f)
}

/// The parameter locals of the body's own function, in order: those
/// flagged as parameters that belong to no closure.
fn param_locals(b: &Body) -> Vec<usize> {
    let mut in_sub = vec![false; b.local_ty.len()];
    for r in &b.sub_params[1..] {
        for &l in &b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize] {
            if let Some(x) = in_sub.get_mut(l as usize) {
                *x = true;
            }
        }
    }
    (0..b.local_ty.len())
        .filter(|&l| b.local_flags[l] & local_flags::PARAM != 0 && !in_sub[l])
        .collect()
}

/// A block whose members do nothing: only nested blocks, which their
/// owners emit.
fn block(b: &Body, blk: usize) -> StageResult<()> {
    if b.tags[blk] != Tag::Block {
        return unsupported("a block operand that is not a block");
    }
    for &i in b.record(b.data[blk][0]) {
        match b.tags[i as usize] {
            Tag::Block => {}
            tag => return unsupported(format!("`{}` (job 3, #540)", tag.name())),
        }
    }
    Ok(())
}
