//! The design's checking stages beyond bodies (checking-and-tir.md
//! §4.13.1, §4.13.9, §4.13.10; trait-solver.md §5; scheduler.md §6.1):
//! test overlay and init order; coherence is `hd_resolve::Universe`. Each
//! returns its result or a structured "not implemented".

use hd_base::{NotImplemented, Stage, StageResult};

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
    use super::{ModuleFacts, init_order, test_overlay};

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
}
