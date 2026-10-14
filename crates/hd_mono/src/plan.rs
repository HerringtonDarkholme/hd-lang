//! What collection reads of a body's optimization plan (opt-pipeline.md
//! §2.2). The passes that decide a plan live in `hd_opt`, which implements
//! [`Planner`]; collection applies a plan by generic rules that name no
//! pass, so it never decides *whether* to devirtualize or specialize.

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
/// resolves each entry against the instance it scans. The per-instruction
/// lists are in instruction order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyPlan {
    /// Per `CallValue` instruction: the source of its callee. Where that
    /// source is a known closure in the instance being scanned, the call
    /// is direct (`Target::KnownClosure`).
    pub direct: Vec<(u32, Source)>,
    /// Per `Call` instruction: the source of each argument, where it has
    /// one. Where the callee's `spec_params` mark an argument whose source
    /// is a known closure, the call goes to the callee's instance
    /// specialized on it.
    pub spec_args: Vec<(u32, Vec<Option<Source>>)>,
    /// Per `Closure` instruction: the item parameters its sub-body calls.
    /// An instance specialized on some of them carries those into the
    /// closure's instance.
    pub carry: Vec<(u32, Vec<u32>)>,
    /// This body as a callee: per parameter, whether a known value for it
    /// is worth an instance specialized on it. `None` when no parameter is.
    pub spec_params: Option<Vec<bool>>,
}

impl BodyPlan {
    /// The argument sources of the `Call` at instruction `ix`.
    #[must_use]
    pub fn spec_args_at(&self, ix: u32) -> Option<&[Option<Source>]> {
        let at = self.spec_args.binary_search_by_key(&ix, |e| e.0).ok()?;
        Some(&self.spec_args[at].1)
    }

    /// The parameters the `Closure` at instruction `ix` carries.
    #[must_use]
    pub fn carry_at(&self, ix: u32) -> &[u32] {
        self.carry
            .binary_search_by_key(&ix, |e| e.0)
            .map_or(&[], |at| &self.carry[at].1)
    }
}

/// The pipeline's body-unit passes, as collection sees them. A pipeline
/// with no body pass plans nothing, so collection does no plan work.
pub trait Planner: Sync {
    /// The plan for `body`, the body of `def` (or a body the compiler
    /// generated for it); `None` when it has none.
    fn plan(&self, def: DefId, body: &Body, env: &dyn ProgramEnv) -> Option<Arc<BodyPlan>>;
    /// How many specialized instances may nest: a call inside an instance
    /// this many specializations deep calls an existing specialized
    /// instance or the plain one, never a new one.
    fn spec_depth(&self) -> u32;
}
