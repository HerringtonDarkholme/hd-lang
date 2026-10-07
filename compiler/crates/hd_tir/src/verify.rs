//! The TIR verifier, a stub: structural checks only (operand ranges, list
//! records, locals, labels and roots). The design's verifier also checks
//! types per tag (checking-and-tir.md §4.13.11); that waits for the catalog.

use crate::{CALLEE_TRAIT_METHOD, CONST_BIT, NONE, TirBody, TirTag};

/// Returns every structural problem found; empty means the body passed.
#[must_use]
pub fn verify_body(b: &TirBody) -> Vec<String> {
    let mut errors = Vec::new();
    let n = u32::try_from(b.tags.len()).expect("insts");
    let nlocals = u32::try_from(b.local_ty.len()).expect("locals");
    let list_ok = |at: u32| -> bool {
        let at = at as usize;
        at < b.extra.len() && at + 1 + b.extra[at] as usize <= b.extra.len()
    };
    for i in 0..n {
        let [a, bb] = b.data[i as usize];
        let mut refs: Vec<u32> = Vec::new();
        let mut lists: Vec<u32> = Vec::new();
        match b.tag(i) {
            TirTag::LocalGet => {
                if a >= nlocals {
                    errors.push(format!("%{i}: local {a} out of range"));
                }
            }
            TirTag::LocalSet => {
                if a >= nlocals {
                    errors.push(format!("%{i}: local {a} out of range"));
                }
                refs.push(bb);
            }
            TirTag::Prim | TirTag::Intrinsic | TirTag::NewData => lists.push(bb),
            TirTag::Call => {
                lists.extend([a, bb]);
                if list_ok(a)
                    && b.get_list(a)
                        .first()
                        .is_none_or(|&k| k > CALLEE_TRAIT_METHOD)
                {
                    errors.push(format!("%{i}: bad callee record"));
                }
            }
            TirTag::Field | TirTag::Return => refs.push(a),
            TirTag::Block => {
                lists.push(a);
                refs.push(bb);
            }
            TirTag::If => {
                refs.push(a);
                lists.push(bb);
            }
            TirTag::Loop => {
                if a >= n || b.tag(a) != TirTag::Block {
                    errors.push(format!("%{i}: loop body is not a block"));
                }
            }
            TirTag::Break => {
                if a as usize >= b.label_inst.len() {
                    errors.push(format!("%{i}: label {a} out of range"));
                }
            }
        }
        for at in lists {
            if !list_ok(at) {
                errors.push(format!("%{i}: list record at {at} out of range"));
            } else if matches!(b.tag(i), TirTag::Prim | TirTag::Intrinsic | TirTag::NewData)
                || (b.tag(i) == TirTag::Call && at == bb)
            {
                refs.extend_from_slice(b.get_list(at));
            }
        }
        for r in refs {
            if r != NONE && r & CONST_BIT == 0 && r >= n {
                errors.push(format!("%{i}: operand %{r} out of range"));
            }
        }
    }
    if b.sub_root.is_empty() {
        errors.push("no sub-body root".to_owned());
    }
    for (l, inst) in b.label_inst.iter().enumerate() {
        if inst.0 >= n || b.tag(inst.0) != TirTag::Loop {
            errors.push(format!("label {l} does not name a loop"));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::verify_body;
    use crate::print::print_body;
    use crate::world::{TyKind, World};
    use crate::{CONST_BIT, Inst, LOCAL_PARAM, NONE, PrimOp, TirBody, TirTag};

    /// `fn inc(x: i32) -> i32: x + 1`, built by hand.
    fn inc(w: &mut World) -> TirBody {
        let i32_ = w.ty(TyKind::I32);
        let void = w.ty(TyKind::Void);
        let mut b = TirBody::default();
        let x = w.sym("x");
        b.local(x, i32_, LOCAL_PARAM);
        let get = b.push(TirTag::LocalGet, 0, NONE, i32_, (0, 0));
        let one = w.konst(i32_, 1) | CONST_BIT;
        let args = b.list(&[get.0, one]);
        let add = b.push(TirTag::Prim, PrimOp::Add as u32, args, i32_, (0, 0));
        let items = b.list(&[get.0, add.0]);
        let root = b.push(TirTag::Block, items, add.0, void, (0, 0));
        b.sub_root.push(root);
        b
    }

    #[test]
    fn verifier_accepts_a_well_formed_body_and_printer_prints_it() {
        let mut w = World::default();
        let b = inc(&mut w);
        assert_eq!(verify_body(&b), Vec::<String>::new());
        let text = print_body(&w, &b);
        assert!(text.contains("%1 = Prim Add(%0, 1:i32) : i32"), "{text}");
    }

    #[test]
    fn verifier_reports_a_forward_operand_and_a_bad_local() {
        let mut w = World::default();
        let mut b = inc(&mut w);
        b.data[0][0] = 7;
        let list = b.data[1][1] as usize;
        b.extra[list + 1] = 99;
        let errors = verify_body(&b);
        assert!(errors.iter().any(|e| e.contains("local 7")), "{errors:?}");
        assert!(
            errors.iter().any(|e| e.contains("operand %99")),
            "{errors:?}"
        );
        b.sub_root.clear();
        b.label_inst.push(Inst(0));
        let errors = verify_body(&b);
        assert!(
            errors.iter().any(|e| e.contains("no sub-body root")),
            "{errors:?}"
        );
        assert!(errors.iter().any(|e| e.contains("label 0")), "{errors:?}");
    }
}
