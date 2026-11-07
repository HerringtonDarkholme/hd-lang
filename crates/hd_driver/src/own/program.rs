//! The whole-program ownership pass: one run after collection, over every
//! instance's TIR, for the decisions that need a fact of every body, which
//! the per-body passes cannot see. Today: what each instance receives lent
//! ([`hd_mono::ProgramEnv::lends`]), read by the emitter at both ends of a call.

use hd_mono::ProgramEnv as _;
use hd_tir::BodyKind;
use hd_tir::ir::{Body, Callee, ChoiceKind, Tag};
use hd_tir::own::{OwnCx as _, Pass};

use super::super::{Env, lock};

impl Env<'_> {
    /// Release: what each instance its summary lends receives lent
    /// ([`Env::lent_mask`]). A derive's generated body serves every
    /// instance of a trait method's own parameters, so its passes cannot
    /// name the instance it calls: no instance of a method lends a
    /// parameter such a call passes with a count, and a caller that read it
    /// as lent gives it a count of its own.
    pub(crate) fn own_program(&self, instances: &hd_opt::whole::Instances<'_>) {
        let Some(all) = &self.run.summaries else {
            return;
        };
        let mut unnamed = hd_base::HashSet::default();
        for &id in instances.order {
            if let Some(b) = (instances.body)(id).filter(|b| b.kind == BodyKind::DeriveInstance) {
                self.unnamed_calls(b, &mut unnamed);
            }
        }
        for (&(item, _), lent) in lock(&all.lent).iter_mut() {
            let owner = hd_base::DefId::from_raw(
                (self.run.names().paths)
                    .parent(hd_base::PathId::from_raw(item.raw()))
                    .raw(),
            );
            for &(m, k) in &unnamed {
                if m == item || self.impl_method(owner, m) == Some(item) {
                    *lent &= !(1 << k);
                }
            }
        }
    }

    /// Each trait method and parameter that generated body `body` passes
    /// with a count at a receiver that stays open.
    fn unnamed_calls(&self, body: &Body, out: &mut hd_base::HashSet<(hd_base::DefId, u32)>) {
        for i in (0..body.len()).filter(|&i| body.tags[i] == Tag::Call) {
            let Some(callee) = Callee::from_words(body.record(body.data[i][0])) else {
                continue;
            };
            let Callee::TraitMethod {
                method,
                choice: (ChoiceKind::Bound, _),
                ..
            } = self.chosen(&callee)
            else {
                continue;
            };
            let n = body.record(body.data[i][1]).len().saturating_sub(3);
            for k in (0..n).filter(|&k| k < 64 && self.pass(&callee, k, n) == Pass::Owned) {
                out.insert((method, u32::try_from(k).expect("arguments")));
            }
        }
    }
}
