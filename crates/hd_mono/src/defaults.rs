//! Specialization of pure, omitted defaults before reachability collection.
//! The transformed TIR keeps instruction indices stable for source locations
//! and call targets; code generation receives this same body.

use hd_base::{Hash128, StableHasher};
use hd_tir::ir::{Body, Coercion, NONE, Op, PrimOp, Ref, Tag, local_flags};
use hd_types::Ty;

/// Values that can be copied without allocating or repeating an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Scalar(u64),
    Variant(u32),
}

impl Value {
    pub fn hash_into(self, h: &mut StableHasher) {
        match self {
            Self::Scalar(bits) => {
                h.u8(0);
                h.u64(bits);
            }
            Self::Variant(tag) => {
                h.u8(1);
                h.u32(tag);
            }
        }
    }
}

/// The mark of an exclusive copy's instance among its defaults (#1118): a
/// slot no parameter has. Like a default, it gives the copy a key and
/// emitted code of its own, and its closure bodies carry it.
pub const EXCLUSIVE: (u32, Value) = (u32::MAX, Value::Variant(u32::MAX));

/// Preserve all pre-existing keys when no default was specialized.
#[must_use]
pub fn digest(known: &[(u32, Hash128)], defaults: &[(u32, Value)]) -> Hash128 {
    if defaults.is_empty() {
        return crate::layout::known_digest(known);
    }
    let mut h = StableHasher::new("instance-defaults");
    h.hash(crate::layout::known_digest(known));
    h.u32(1); // Transformation version: also separates emitted-code cache entries.
    h.u32(u32::try_from(defaults.len()).expect("defaults"));
    for &(p, value) in defaults {
        h.u32(p);
        value.hash_into(&mut h);
    }
    h.finish()
}

#[must_use]
pub fn key(plain: Hash128, known: &[(u32, Hash128)], defaults: &[(u32, Value)]) -> Hash128 {
    if defaults.is_empty() {
        return crate::layout::known_key(plain, known);
    }
    let mut h = StableHasher::new("instance-defaults-key");
    h.hash(plain);
    h.hash(digest(known, defaults));
    h.finish()
}

/// A default must consist entirely of constants and transparent coercions.
/// Returning a constant after a call or a write is deliberately not enough.
#[must_use]
pub fn constant(body: &Body) -> Option<Value> {
    if body.tags.iter().enumerate().any(|(i, tag)| match tag {
        Tag::Block => false,
        Tag::NewVariant => !body.record(body.data[i][1]).is_empty(),
        Tag::Coerce => !transparent(body, i),
        _ => true,
    }) {
        return None;
    }
    let root = *body.sub_root.first()?;
    value(body, root, &vec![None; body.local_ty.len()], 0)
}

fn transparent(body: &Body, i: usize) -> bool {
    matches!(body.record(body.data[i][1]).first().copied(),
        Some(k) if k == Coercion::Identity as u32 || k == Coercion::Weaken as u32)
}

fn value(body: &Body, word: u32, locals: &[Option<Value>], depth: usize) -> Option<Value> {
    if word == NONE || depth > 64 {
        return None;
    }
    if word & Ref::CONST_BIT != 0 {
        let &(ty, bits) = body.consts.get((word & !Ref::CONST_BIT) as usize)?;
        return (ty != Ty::STRING).then_some(Value::Scalar(bits));
    }
    let i = word as usize;
    let [a, b] = *body.data.get(i)?;
    match body.tags[i] {
        Tag::Block => value(body, b, locals, depth + 1),
        Tag::Read => locals.get(body.local_read(i)?.idx()).copied().flatten(),
        Tag::NewVariant if body.record(b).is_empty() => Some(Value::Variant(a)),
        Tag::Coerce if transparent(body, i) => value(body, a, locals, depth + 1),
        Tag::Prim => {
            let args = body.record(b);
            let Value::Scalar(left) = value(body, *args.first()?, locals, depth + 1)? else {
                return None;
            };
            let result = match PrimOp::from_u32(a)? {
                PrimOp::Not if body.ty[i] == Ty::BOOL => left == 0,
                _ => return None,
            };
            Some(Value::Scalar(u64::from(result)))
        }
        Tag::And | Tag::Or if body.ty[i] == Ty::BOOL => {
            let Value::Scalar(left) = value(body, a, locals, depth + 1)? else {
                return None;
            };
            match (body.tags[i], left != 0) {
                (Tag::And, false) => Some(Value::Scalar(0)),
                (Tag::Or, true) => Some(Value::Scalar(1)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The item's parameter locals, excluding parameters of nested functions.
fn params(body: &Body) -> Vec<usize> {
    let mut nested = vec![false; body.local_ty.len()];
    for r in body.sub_params.iter().skip(1) {
        for &l in body.record(r.start) {
            nested[l as usize] = true;
        }
    }
    body.local_flags
        .iter()
        .enumerate()
        .filter_map(|(l, &f)| (f & local_flags::PARAM != 0 && !nested[l]).then_some(l))
        .collect()
}

/// Follow a pure match decision to its arm. Binding patterns and guards
/// with unknown results retain the original match and all its effects.
fn arm(body: &Body, block: u32, locals: &[Option<Value>], depth: usize) -> Option<u32> {
    if depth > 64 {
        return None;
    }
    for &i in body.record(body.data[block as usize][0]) {
        let i = i as usize;
        let [a, b] = body.data[i];
        match body.tags[i] {
            Tag::Block => {}
            Tag::Read if body.local_read(i).is_some() => {}
            Tag::ToArm => return Some(a),
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                let val = value(body, a, locals, 0)?;
                let rec = body.record(b);
                let equal = match (body.tags[i], val) {
                    (Tag::SwitchTag, Value::Variant(tag)) => tag == rec[0],
                    (Tag::SwitchInt | Tag::SwitchChar, Value::Scalar(bits)) => {
                        u32::try_from(bits & u64::from(u32::MAX)).expect("low bits") == rec[0]
                    }
                    _ => return None,
                };
                return arm(body, rec[if equal { 1 } else { 2 }], locals, depth + 1);
            }
            _ => return None,
        }
    }
    None
}

/// Fold only when at least one branch disappears. Ordinary instances and
/// programs without useful constant defaults keep their original identity.
#[must_use]
pub fn specialize(body: &Body, defaults: &[(u32, Value)]) -> Option<Body> {
    let mut locals = vec![None; body.local_ty.len()];
    let mut writes = vec![0; locals.len()];
    let mut assigned = vec![NONE; locals.len()];
    // A store into a field of a local writes it too, with no value of
    // the whole local.
    for i in 0..body.len() {
        if let Some((l, whole)) = body.stored_local(i) {
            writes[l.idx()] += 1;
            assigned[l.idx()] = if whole { body.data[i][1] } else { NONE };
        }
    }
    let params = params(body);
    for &(p, value) in defaults {
        let &l = params.get(p as usize)?;
        if writes[l] == 0 && body.local_flags[l] & local_flags::CAPTURED_ASSIGNED == 0 {
            locals[l] = Some(value);
        }
    }
    // Single-assignment aliases can carry a constant to a later decision.
    // With no seeded defaults this also propagates constants introduced by
    // another post-monomorphization fold.
    for _ in 0..locals.len().min(64) {
        let mut changed = false;
        for l in 0..locals.len() {
            if locals[l].is_none()
                && writes[l] == 1
                && body.local_flags[l] & (local_flags::PARAM | local_flags::CAPTURED_ASSIGNED) == 0
                && let Some(v) = value(body, assigned[l], &locals, 0)
            {
                locals[l] = Some(v);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut out = body.clone();
    let empty = u32::try_from(out.extra.len()).expect("extra");
    out.extra.push(0);
    let mut folded = false;
    // A decisive short-circuit operand is itself a branch. Replace the
    // operator, not just an enclosing decision, so reachability does not
    // retain the unevaluated right-hand block. Keep the left operand as an
    // identity coercion because proving its value does not prove it has no
    // effects.
    for i in 0..body.len() {
        let instruction = u32::try_from(i).expect("instructions");
        if matches!(body.tags[i], Tag::And | Tag::Or)
            && value(body, instruction, &locals, 0).is_some()
        {
            let coercion = u32::try_from(out.extra.len()).expect("records");
            out.extra.push(2);
            out.extra.push(Coercion::Identity as u32);
            out.extra.push(NONE);
            out.tags[i] = Tag::Coerce;
            out.data[i] = [body.data[i][0], coercion];
            folded = true;
        }
    }
    for i in 0..body.len() {
        let [a, b] = body.data[i];
        let chosen = match body.tags[i] {
            Tag::Match => {
                let blocks = body.record(b);
                arm(body, blocks[0], &locals, 0).and_then(|k| blocks.get(k as usize + 1).copied())
            }
            Tag::If
                if body.ty.get(a as usize) == Some(&Ty::BOOL)
                    || (a & Ref::CONST_BIT != 0
                        && body
                            .consts
                            .get((a & !Ref::CONST_BIT) as usize)
                            .is_some_and(|c| c.0 == Ty::BOOL)) =>
            {
                match value(body, a, &locals, 0) {
                    Some(Value::Scalar(bits)) => {
                        body.record(b).get(usize::from(bits == 0)).copied()
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(block) = chosen {
            out.tags[i] = Tag::Scope;
            out.data[i] = [block, empty];
            folded = true;
        }
    }
    if !folded {
        return None;
    }
    // Remove unreachable instructions, not just their calls: otherwise the
    // emitter could still reserve suspension states and frame fields.
    let mut live = vec![false; out.len()];
    mark(&out, out.sub_root[0], &mut live);
    for (i, reached) in live.iter().enumerate() {
        if !reached {
            out.tags[i] = Tag::Block;
            out.data[i] = [empty, NONE];
            out.ty[i] = Ty::VOID;
        }
    }
    out.susp.retain(|s| live[s.inst as usize]);
    out.fallback.retain(|f| live[f[0] as usize]);
    out.unwind_drops
        .retain(|r| live[r.0 as usize] && live[r.1 as usize]);
    out.stable_lists.retain(|r| live[r.0 as usize]);
    out.full_cells.retain(|r| live[r.0 as usize]);
    Some(out)
}

fn mark(body: &Body, i: u32, live: &mut [bool]) {
    if i == NONE || i & Ref::CONST_BIT != 0 {
        return;
    }
    let i = i as usize;
    if i >= live.len() || live[i] {
        return;
    }
    live[i] = true;
    let [a, b] = body.data[i];
    if body.tags[i] == Tag::Block {
        for &child in body.record(a) {
            if body.tags[child as usize] != Tag::Block {
                mark(body, child, live);
            }
        }
        mark(body, b, live);
        return;
    }
    if body.tags[i] == Tag::Closure
        && let Some(&root) = body.sub_root.get(a as usize)
    {
        mark(body, root, live);
    }
    if let Some(values) = body.value_operands(i) {
        for v in values {
            mark(body, v, live);
        }
    }
    if body.tags[i] == Tag::Match || body.tags[i] == Tag::If {
        for &block in body.record(b) {
            mark(body, block, live);
        }
        mark(body, a, live);
        return;
    }
    if matches!(
        body.tags[i],
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr
    ) {
        for &block in body.record(b).iter().skip(1).take(2) {
            mark(body, block, live);
        }
        mark(body, a, live);
        return;
    }
    for (op, word) in [body.tags[i].operands().0, body.tags[i].operands().1]
        .into_iter()
        .zip([a, b])
    {
        match op {
            Op::Value => mark(body, word, live),
            Op::Block if body.tags.get(word as usize) == Some(&Tag::Block) => {
                mark(body, word, live);
            }
            Op::Blocks => {
                for &block in body.record(word) {
                    mark(body, block, live);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Callee, Providers, TirBuilder, TirSink};
    use hd_types::TyList;

    #[test]
    fn constant_result_after_a_call_is_not_a_constant_default() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Default);
        let block = b.open_block();
        b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let result = b.const_value(Ty::BOOL, 0);
        let root = b.close_block(block, Some(result), Ty::BOOL, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid default body");
        assert_eq!(constant(&body), None);
    }

    #[test]
    fn folding_drops_the_unselected_calls_and_retains_source_indices() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let param = b.local(
            Ty::BOOL,
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let cond = b.get(param, Ty::BOOL, NodeIdx::NONE);
        let yes = b.open_block();
        let dead = b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let yes = b.close_block(yes, None, Ty::VOID, NodeIdx::NONE);
        let no = b.open_block();
        let kept = b.call(
            &Callee::Item {
                def: DefId::from_raw(3),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let no = b.close_block(no, None, Ty::VOID, NodeIdx::NONE);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid function body");
        let out = specialize(&body, &[(0, Value::Scalar(0))]).expect("constant false branch");
        assert_eq!(out.tags.len(), body.tags.len());
        assert_eq!(out.syn, body.syn);
        assert_eq!(out.tags[dead.0 as usize], Tag::Block);
        assert_eq!(out.tags[kept.0 as usize], Tag::Call);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    #[test]
    fn decisive_short_circuit_drops_its_rhs_and_unselected_branch() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let left = b.open_block();
        let left_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(5),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let left_value = b.const_value(Ty::BOOL, 0);
        let left = b.close_block(left, Some(left_value), Ty::BOOL, NodeIdx::NONE);
        let rhs = b.open_block();
        let rhs_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(2),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let rhs_value = b.const_value(Ty::BOOL, 1);
        let rhs = b.close_block(rhs, Some(rhs_value), Ty::BOOL, NodeIdx::NONE);
        let cond = b.emit(Tag::And, left.0, rhs.0, Ty::BOOL, NodeIdx::NONE);
        let yes = b.open_block();
        let yes_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(3),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let yes = b.close_block(yes, None, Ty::VOID, NodeIdx::NONE);
        let no = b.open_block();
        let no_call = b.call(
            &Callee::Item {
                def: DefId::from_raw(4),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let no = b.close_block(no, None, Ty::VOID, NodeIdx::NONE);
        let blocks = b.refs_record(&[yes, no]);
        b.emit(Tag::If, cond.0, blocks, Ty::VOID, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let body = b.finish(root, &[]).expect("valid function body");

        let out = specialize(&body, &[]).expect("constant false short circuit");
        assert_eq!(out.tags[left_call.0 as usize], Tag::Call);
        assert_eq!(out.tags[rhs_call.0 as usize], Tag::Block);
        assert_eq!(out.tags[yes_call.0 as usize], Tag::Block);
        assert_eq!(out.tags[no_call.0 as usize], Tag::Call);
        let errors = hd_tir::ir::verify(&out);
        assert!(errors.is_empty(), "specialized TIR: {errors:?}");
    }

    #[test]
    fn specialization_keys_separate_values_and_keep_plain_keys() {
        let plain = Hash128(42);
        assert_eq!(key(plain, &[], &[]), plain);
        assert_ne!(
            key(plain, &[], &[(0, Value::Scalar(0))]),
            key(plain, &[], &[(0, Value::Variant(0))])
        );
        assert_ne!(
            key(plain, &[], &[(0, Value::Scalar(0))]),
            key(plain, &[], &[(1, Value::Scalar(0))])
        );
    }
}
