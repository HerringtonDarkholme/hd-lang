//! The design's checking stages beyond bodies (checking-and-tir.md
//! §4.13.1, §4.13.9, §4.13.10; trait-solver.md §5; scheduler.md §6.1):
//! test overlay, coherence and init order. Each returns its result or a
//! structured "not implemented".

use hd_base::{DefId, NotImplemented, Stage, StageResult};
use hd_types::{InternPool, Ty};

/// Facts a module exposes to these stages, read from its source skim.
#[derive(Clone, Debug, Default)]
pub struct ModuleFacts {
    pub path: String,
    pub has_tests_block: bool,
    pub top_level_statements: usize,
    pub impls: usize,
}

/// `TestOverlay(m)` (§4.13.9): checks `tests:` blocks and doc tests.
pub fn test_overlay(m: &ModuleFacts) -> StageResult<Option<String>> {
    if m.has_tests_block {
        Err(NotImplemented::new(
            Stage::TestOverlay,
            format!("tests: block of {}", m.path),
        ))
    } else {
        Ok(None)
    }
}

/// One impl head as coherence sees it: trait, self type, impl.
pub type ImplHead = (DefId, Ty, DefId);

/// `Coherence` (trait-solver.md §5.2): the overlap check per trait. Heads
/// with no parameter overlap exactly when they are equal; generic heads
/// need unification and are not implemented yet. Returns overlapping pairs.
pub fn coherence(pool: &InternPool, heads: &[ImplHead]) -> StageResult<Vec<(DefId, DefId)>> {
    if heads.iter().any(|h| pool.has_param(h.1)) {
        return Err(NotImplemented::new(
            Stage::Coherence,
            "overlap check of generic impl heads",
        ));
    }
    let mut out = Vec::new();
    for (i, a) in heads.iter().enumerate() {
        for b in &heads[i + 1..] {
            if a.0 == b.0 && a.1 == b.1 {
                out.push((a.2, b.2));
            }
        }
    }
    Ok(out)
}

/// `InitOrder(F)` (§4.13.10): only when a folder's init group spans
/// modules. A folder whose statements sit in at most one module has
/// nothing to order.
pub fn init_order(folder: &str, modules: &[&ModuleFacts]) -> StageResult<Vec<String>> {
    let with_stmts = modules
        .iter()
        .filter(|m| m.top_level_statements > 0)
        .count();
    if with_stmts <= 1 {
        Ok(Vec::new())
    } else {
        Err(NotImplemented::new(
            Stage::InitOrder,
            format!("cross-module init group in {folder}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ModuleFacts, coherence, init_order, test_overlay};
    use hd_base::DefId;
    use hd_types::{InternPool, ParamRef, Ty, TyData};

    #[test]
    fn trivial_inputs_pass_and_real_work_is_reported() {
        let a = ModuleFacts {
            path: "p.a".into(),
            top_level_statements: 1,
            ..ModuleFacts::default()
        };
        let b = ModuleFacts {
            path: "p.b".into(),
            top_level_statements: 2,
            impls: 1,
            has_tests_block: true,
        };
        assert!(init_order("p", &[&a]).is_ok());
        assert!(init_order("p", &[&a, &b]).is_err());
        assert!(test_overlay(&a).expect("no tests").is_none());
        assert!(test_overlay(&b).is_err());
    }

    #[test]
    fn equal_exact_heads_overlap() {
        let p = InternPool::new();
        let d = DefId::from_raw;
        assert_eq!(
            coherence(&p, &[(d(1), Ty::I32, d(2)), (d(1), Ty::BOOL, d(3))]).expect("ok"),
            []
        );
        assert_eq!(
            coherence(&p, &[(d(1), Ty::I32, d(2)), (d(1), Ty::I32, d(3))]).expect("ok"),
            [(d(2), d(3))]
        );
        let t = p.intern_ty(&TyData::Param(ParamRef {
            owner: d(9),
            index: 0,
        }));
        assert!(coherence(&p, &[(d(1), t, d(2))]).is_err());
    }
}
