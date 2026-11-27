use crate::lookup::{UnitImpls, UniverseImpls};
use crate::solver::ImplUniverseId;
use hd_base::{HashMap, SliceSort as _, UnitId};
use std::sync::{Arc, Mutex, PoisonError};

/// Interns impl universes once per run; equal lists share one id and one
/// merged view of their units' unowned rows and directory.
#[derive(Default)]
pub(super) struct ImplUniverses {
    by_list: Mutex<UniverseMap>,
}

type UniverseMap = HashMap<Vec<UnitId>, (ImplUniverseId, Arc<UniverseImpls>)>;

impl ImplUniverses {
    /// The universe of a context: `units` are the units of its closure
    /// that hold `arg_impls` or unowned rows, in any order.
    pub fn intern(&self, units: &[(UnitId, &UnitImpls)]) -> (ImplUniverseId, Arc<UniverseImpls>) {
        let mut list = units.to_vec();
        list.sort_stable_by_key(|(f, _)| f.raw());
        list.dedup_by_key(|(f, _)| *f);
        let key: Vec<UnitId> = list.iter().map(|(f, _)| *f).collect();
        let mut m = self.by_list.lock().unwrap_or_else(PoisonError::into_inner);
        let next = ImplUniverseId(crate::word(m.len()));
        m.entry(key)
            .or_insert_with(|| {
                (
                    next,
                    Arc::new(
                        list.iter()
                            .fold(UniverseImpls::default(), |out, (unit, fi)| {
                                out.union(&UniverseImpls::unit(*unit, fi, &|_, _| false))
                            }),
                    ),
                )
            })
            .clone()
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.by_list.lock().expect("universes").len()
    }
}
