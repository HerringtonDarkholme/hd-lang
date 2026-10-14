//! A pipeline's body-unit passes as the `hd_mono::plan::Planner` the
//! driver hands collection (opt-pipeline.md §1.2, §2.2).

use std::sync::Arc;

use hd_base::{DefId, Fuel};
use hd_mono::ProgramEnv;
use hd_mono::plan::{BodyPlan, Planner};
use hd_tir::ir::Body;

use crate::passes::{BodyCx, BodyFacts, PassOutcome, Pipeline, Run, body_gate, spec};

type BodyRun = fn(&BodyCx<'_>, &mut BodyFacts, &mut Fuel) -> hd_base::StageResult<PassOutcome>;

/// The body-unit passes of one pipeline, in its order, with their gates.
pub struct PipelinePlanner {
    passes: Vec<(u8, BodyRun)>,
}

impl PipelinePlanner {
    /// The planner of `p`: its body-unit passes. A pipeline with none, such
    /// as `DEV`, plans nothing.
    #[must_use]
    pub fn new(p: &Pipeline) -> Self {
        let passes = p
            .tir
            .iter()
            .filter_map(|name| {
                let s = spec(name)?;
                match s.run {
                    Run::Body(run) => Some((s.gate, run)),
                    Run::Instance(_) => None,
                }
            })
            .collect();
        Self { passes }
    }
}

impl Planner for PipelinePlanner {
    /// Runs each body pass whose gate the body passes, then gives
    /// collection the decisions: `None` when there are none. A pass that
    /// runs out of fuel leaves the body unplanned, which is plain code.
    fn plan(&self, def: DefId, body: &Body, env: &dyn ProgramEnv) -> Option<Arc<BodyPlan>> {
        if self.passes.is_empty() {
            return None;
        }
        let summary = body_gate(body);
        let cx = BodyCx { def, body, env };
        let mut facts = BodyFacts::default();
        let mut fuel = Fuel::new(Fuel::BODY_DEFAULT);
        for &(gate, run) in &self.passes {
            if gate == 0 || summary & gate != 0 {
                run(&cx, &mut facts, &mut fuel).ok()?;
            }
        }
        let plan = BodyPlan {
            direct: facts.known_callees,
        };
        (plan != BodyPlan::default()).then(|| Arc::new(plan))
    }
}

#[cfg(test)]
mod tests {
    use super::PipelinePlanner;
    use crate::passes::{DEV, OPTIMIZED};

    #[test]
    fn only_the_optimized_pipeline_has_body_passes() {
        assert!(PipelinePlanner::new(&DEV).passes.is_empty());
        assert_eq!(PipelinePlanner::new(&OPTIMIZED).passes.len(), 2);
    }
}
