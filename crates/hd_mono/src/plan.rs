//! What collection reads of a body's optimization plan (opt-pipeline.md
//! §2.2). The passes that decide a plan live in `hd_opt`, which implements
//! [`Planner`]; collection applies a plan by generic rules that name no
//! pass, so it never decides *whether* to devirtualize.

use std::sync::Arc;

use hd_base::DefId;
use hd_tir::ir::Body;

use crate::ProgramEnv;

/// Where a value comes from in a body: the instruction that makes it, or
/// the item parameter it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Made(u32),
    Param(u32),
}

/// The decisions body-unit passes made for one generic body. Collection
/// resolves each entry against the instance it scans.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyPlan {
    /// Per `CallValue` instruction: the source of its callee. Where that
    /// source is a known closure in the instance being scanned, the call
    /// is direct (`Target::KnownClosure`).
    pub direct: Vec<(u32, Source)>,
}

/// The pipeline's body-unit passes, as collection sees them. A pipeline
/// with no body pass plans nothing, so collection does no plan work.
pub trait Planner: Sync {
    /// The plan for `body`, the body of `def` (or a body the compiler
    /// generated for it); `None` when it has none.
    fn plan(&self, def: DefId, body: &Body, env: &dyn ProgramEnv) -> Option<Arc<BodyPlan>>;
}
