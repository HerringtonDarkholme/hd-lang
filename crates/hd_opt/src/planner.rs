//! A pipeline's body-unit passes as the `hd_mono::plan::Planner` the
//! driver hands collection (opt-pipeline.md §1.2, §2.2).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hd_base::{DefId, Fuel};
use hd_mono::ProgramEnv;
use hd_mono::plan::{BodyPlan, Planner};
use hd_tir::ir::Body;

use crate::passes::{
    BodyCx, BodyFacts, PassOutcome, Pipeline, Run, SPEC_MAX_DEPTH, body_gate, spec,
};

type BodyRun = fn(&BodyCx<'_>, &mut BodyFacts, &mut Fuel) -> hd_base::StageResult<PassOutcome>;

/// The body-unit passes of one pipeline, in its order, with their gates.
pub struct PipelinePlanner {
    passes: Vec<(u8, BodyRun)>,
    /// `Planner::spec_depth`: `closure_spec`'s nesting bound when the
    /// pipeline holds it, else 0, so no specialized instance is made.
    spec_depth: u32,
    /// How many body-pass runs `plan` made, for the build's counters.
    runs: AtomicUsize,
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
        let spec_depth = if p.tir.contains(&"closure_spec") {
            SPEC_MAX_DEPTH
        } else {
            0
        };
        Self {
            passes,
            spec_depth,
            runs: AtomicUsize::new(0),
        }
    }

    /// How many body-pass runs this planner has made: 0 for a pipeline
    /// with no body pass, however many bodies collection scanned.
    #[must_use]
    pub fn pass_runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
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
                self.runs.fetch_add(1, Ordering::Relaxed);
                run(&cx, &mut facts, &mut fuel).ok()?;
            }
        }
        let spec = facts.closure_spec;
        let mut direct = facts.known_callees;
        direct.extend(spec.param_calls);
        direct.sort_unstable_by_key(|d| d.0);
        let plan = BodyPlan {
            direct,
            spec_args: spec.spec_args,
            carry: spec.carry,
            spec_params: spec.spec_params,
        };
        (plan != BodyPlan::default()).then(|| Arc::new(plan))
    }

    fn spec_depth(&self) -> u32 {
        self.spec_depth
    }
}

#[cfg(test)]
mod tests {
    use super::PipelinePlanner;
    use crate::passes::{DEV, OPTIMIZED, SPEC_MAX_DEPTH};
    use hd_mono::plan::Planner;

    #[test]
    fn only_the_optimized_pipeline_has_body_passes() {
        let dev = PipelinePlanner::new(&DEV);
        assert!(dev.passes.is_empty());
        assert_eq!(dev.spec_depth(), 0);
        let optimized = PipelinePlanner::new(&OPTIMIZED);
        assert_eq!(optimized.passes.len(), 3);
        assert_eq!(optimized.spec_depth(), SPEC_MAX_DEPTH);
    }
}
