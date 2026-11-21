//! Deferred type-outlives requirements of a successful proof. Nested impl
//! steps retain their substitution, so equal types in different input
//! positions do not lose their distinct lifetimes. Sharing keeps a proof
//! diamond a DAG rather than expanding its paths into obligation lists.

use std::sync::Arc;

use hd_base::{DefId, HashMap};

use super::ImplRef;
use crate::{Ty, TyList, Types};

/// A lifetime named by an outlives requirement. A declared lifetime is
/// interpreted in its owner's region environment, not as a body-local ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegionBound {
    Static,
    Lifetime { owner: DefId, index: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_proof_remapping_counts_nodes_and_edges_at_width_1000_and_depth_20() {
        let gp = crate::InternPool::new();
        let pool = gp.types();
        for width in [100, 1000] {
            let mut proof = RegionObligations::new(
                pool,
                vec![RegionObligation::TypeOutlives {
                    ty: Ty::I32,
                    region: RegionBound::Static,
                }],
            );
            for _ in 0..20 {
                proof = RegionObligations::new(
                    pool,
                    (0..width)
                        .map(|_| RegionObligation::ImplBound {
                            row: ImplRef {
                                module: hd_base::ModuleId::from_raw(0),
                                row: 0,
                            },
                            args: pool.list(&[Ty::I32]),
                            step: 0,
                            obligations: proof.clone(),
                        })
                        .collect(),
                );
            }
            let mut mapped = 0;
            let result = proof.try_map_types(pool, |t| {
                mapped += 1;
                Ok::<_, std::convert::Infallible>(t)
            });
            let Ok(result) = result;
            assert_eq!(mapped, width * 20 + 1);
            assert_eq!(proof, result);
            // Normal memo replay and inference resolution must not walk
            // closed shared proofs again for every root that uses them.
            let mut replayed = 0;
            let inferred = proof.map_infer(pool, |t| {
                replayed += 1;
                t
            });
            let instantiated = proof.map_params(pool, |t| {
                replayed += 1;
                t
            });
            let canonical = proof.try_map_canon_with(
                pool,
                |t| {
                    replayed += 1;
                    Ok::<_, std::convert::Infallible>(t)
                },
                &mut HashMap::default(),
            );
            let Ok(canonical) = canonical;
            assert_eq!(replayed, 0);
            assert_eq!(inferred.id(), proof.id());
            assert_eq!(instantiated.id(), proof.id());
            assert_eq!(canonical.id(), proof.id());
        }
    }

    #[test]
    fn structural_equality_is_independent_of_proof_sharing() {
        let gp = crate::InternPool::new();
        let pool = gp.types();
        let leaf = || {
            RegionObligations::new(
                pool,
                vec![RegionObligation::TypeOutlives {
                    ty: Ty::I32,
                    region: RegionBound::Static,
                }],
            )
        };
        let edge = |obligations| RegionObligation::ImplBound {
            row: ImplRef {
                module: hd_base::ModuleId::from_raw(0),
                row: 0,
            },
            args: TyList::EMPTY,
            step: 0,
            obligations,
        };
        let shared = leaf();
        let a = RegionObligations::new(pool, vec![edge(shared.clone()), edge(shared)]);
        let b = RegionObligations::new(pool, vec![edge(leaf()), edge(leaf())]);
        assert_eq!(a, b);
        assert_ne!(a, leaf());
    }

    #[test]
    fn deep_shared_proofs_drop_on_small_stacks_in_parallel() -> Result<(), &'static str> {
        let gp = crate::InternPool::new();
        let pool = gp.types();
        let mut proof = RegionObligations::new(
            pool,
            vec![RegionObligation::TypeOutlives {
                ty: Ty::I32,
                region: RegionBound::Static,
            }],
        );
        for _ in 0..10_000 {
            proof = RegionObligations::new(
                pool,
                vec![RegionObligation::ImplBound {
                    row: ImplRef {
                        module: hd_base::ModuleId::from_raw(0),
                        row: 0,
                    },
                    args: TyList::EMPTY,
                    step: 0,
                    obligations: proof,
                }],
            );
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let spawn = |proof: RegionObligations, barrier: Arc<std::sync::Barrier>| {
            std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(move || {
                    barrier.wait();
                    drop(proof);
                })
        };
        let a = spawn(proof.clone(), barrier.clone()).map_err(|_| "start first drop worker")?;
        let b = spawn(proof, barrier).map_err(|_| "start second drop worker")?;
        a.join().map_err(|_| "first drop worker failed")?;
        b.join().map_err(|_| "second drop worker failed")?;
        Ok(())
    }
}

/// One deferred requirement, relative to the inputs of the proof carrying
/// it. An impl-bound step changes those inputs to the step's trait reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionObligation {
    TypeOutlives {
        ty: Ty,
        region: RegionBound,
    },
    ImplBound {
        row: ImplRef,
        args: TyList,
        step: u16,
        obligations: RegionObligations,
    },
}

/// Empty proofs allocate nothing; cloning a nonempty proof shares its
/// immutable requirements. Only committed successes retain child proofs.
#[derive(Clone, Default)]
pub struct RegionObligations(Option<Arc<ProofList>>);

struct ProofList {
    requirements: Box<[RegionObligation]>,
    flags: u8,
}

impl Drop for RegionObligations {
    fn drop(&mut self) {
        let Some(root) = self.0.take() else { return };
        // Shared handles are the common case: dropping one does no
        // allocation and never enters its children.
        let Some(mut requirements) = Arc::into_inner(root) else {
            return;
        };
        let mut pending = Vec::new();
        loop {
            // Follow std's Arc::into_inner linked-list destructor. Every
            // strong reference is owned by this private wrapper, so even
            // concurrent last owners cannot fall back to recursive Drop.
            for requirement in &mut requirements.requirements {
                if let RegionObligation::ImplBound { obligations, .. } = requirement
                    && let Some(child) = obligations.0.take()
                    && let Some(child) = Arc::into_inner(child)
                {
                    pending.push(child);
                }
            }
            match pending.pop() {
                Some(next) => requirements = next,
                None => break,
            }
        }
    }
}

/// Identity of a node while its borrowed proof graph remains alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionProofId(usize);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum RequirementKey {
    Outlives(Ty, RegionBound),
    Impl(ImplRef, TyList, u16, usize),
}

impl PartialEq for RegionObligations {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) => true,
            _ => self.signature() == other.signature(),
        }
    }
}

impl Eq for RegionObligations {}

impl std::fmt::Debug for RegionObligations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Printing a shared diamond must not expand all of its paths.
        f.debug_list().entries(self.signature()).finish()
    }
}

impl<'a> IntoIterator for &'a RegionObligations {
    type Item = &'a RegionObligation;
    type IntoIter = std::slice::Iter<'a, RegionObligation>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl RegionObligations {
    #[must_use]
    pub fn id(&self) -> Option<RegionProofId> {
        self.key().map(RegionProofId)
    }

    fn key(&self) -> Option<usize> {
        self.0
            .as_ref()
            .map(|a| Arc::as_ptr(a).cast::<()>() as usize)
    }

    /// Deterministic postorder numbering, interning structurally equal
    /// lists. Equality and debugging therefore visit nodes and edges,
    /// rather than recursively expanding shared proof paths.
    fn signature(&self) -> Vec<Vec<RequirementKey>> {
        let mut done = HashMap::<usize, usize>::default();
        let mut interned = HashMap::<Vec<RequirementKey>, usize>::default();
        let mut order = Vec::new();
        let mut stack = vec![(self, false)];
        while let Some((at, exit)) = stack.pop() {
            let Some(id) = at.key() else { continue };
            if done.contains_key(&id) {
                continue;
            }
            if !exit {
                stack.push((at, true));
                for r in at {
                    if let RegionObligation::ImplBound { obligations, .. } = r {
                        stack.push((obligations, false));
                    }
                }
                continue;
            }
            let list: Vec<_> = at
                .iter()
                .map(|r| match r {
                    RegionObligation::TypeOutlives { ty, region } => {
                        RequirementKey::Outlives(*ty, *region)
                    }
                    RegionObligation::ImplBound {
                        row,
                        args,
                        step,
                        obligations,
                    } => {
                        // ruled out: each nonempty child's exit frame has run.
                        let child = obligations.key().map_or(0, |k| done[&k]);
                        RequirementKey::Impl(*row, *args, *step, child)
                    }
                })
                .collect();
            let n = if let Some(&n) = interned.get(&list) {
                n
            } else {
                let n = order.len() + 1;
                interned.insert(list.clone(), n);
                order.push(list);
                n
            };
            done.insert(id, n);
        }
        order
    }

    #[must_use]
    pub fn new(pool: Types<'_>, requirements: Vec<RegionObligation>) -> Self {
        let flags = requirements.iter().fold(0, |flags, r| {
            flags
                | match r {
                    RegionObligation::TypeOutlives { ty, region } => {
                        u8::from(pool.has_infer(*ty))
                            | (u8::from(pool.has_canon(*ty)) << 1)
                            | (u8::from(pool.has_param(*ty)) << 2)
                            | (u8::from(matches!(region, RegionBound::Lifetime { .. })) << 3)
                    }
                    RegionObligation::ImplBound {
                        args, obligations, ..
                    } => {
                        u8::from(pool.list_has_infer(*args))
                            | (u8::from(pool.list_has_canon(*args)) << 1)
                            | (u8::from(pool.list_has_param(*args)) << 2)
                            | obligations.flags()
                    }
                }
        });
        Self((!requirements.is_empty()).then(|| {
            Arc::new(ProofList {
                requirements: requirements.into_boxed_slice(),
                flags,
            })
        }))
    }

    fn flags(&self) -> u8 {
        self.0.as_ref().map_or(0, |p| p.flags)
    }

    #[must_use]
    pub fn has_infer(&self) -> bool {
        self.flags() & 1 != 0
    }

    #[must_use]
    pub fn has_canon(&self) -> bool {
        self.flags() & 2 != 0
    }

    #[must_use]
    pub fn has_param(&self) -> bool {
        self.flags() & 4 != 0
    }

    /// Whether any deferred predicate needs a scoped lifetime variable.
    #[must_use]
    pub fn has_named_regions(&self) -> bool {
        self.flags() & 8 != 0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, RegionObligation> {
        self.0
            .as_deref()
            .map_or(&[][..], |p| p.requirements.as_ref())
            .iter()
    }

    #[must_use]
    pub fn map_types(&self, pool: Types<'_>, mut map: impl FnMut(Ty) -> Ty) -> Self {
        let Ok(mapped) = self.try_map_types(pool, |t| Ok::<_, std::convert::Infallible>(map(t)));
        mapped
    }

    #[must_use]
    pub fn map_infer(&self, pool: Types<'_>, map: impl FnMut(Ty) -> Ty) -> Self {
        if self.has_infer() {
            self.map_types(pool, map)
        } else {
            self.clone()
        }
    }

    #[must_use]
    pub fn map_params(&self, pool: Types<'_>, map: impl FnMut(Ty) -> Ty) -> Self {
        if self.has_param() {
            self.map_types(pool, map)
        } else {
            self.clone()
        }
    }

    pub(super) fn try_map_infer_with<E>(
        &self,
        pool: Types<'_>,
        map: impl FnMut(Ty) -> Result<Ty, E>,
        done: &mut HashMap<usize, Self>,
    ) -> Result<Self, E> {
        if self.has_infer() {
            self.try_map_types_with(pool, map, done)
        } else {
            Ok(self.clone())
        }
    }

    pub(super) fn try_map_canon_with<E>(
        &self,
        pool: Types<'_>,
        map: impl FnMut(Ty) -> Result<Ty, E>,
        done: &mut HashMap<usize, Self>,
    ) -> Result<Self, E> {
        if self.has_canon() {
            self.try_map_types_with(pool, map, done)
        } else {
            Ok(self.clone())
        }
    }

    /// Remaps each distinct shared requirement list once, in postorder.
    /// Canonical memo entries cannot retain body-local types after a trial
    /// rolls back. Work is O(nodes + edges + mapped type arguments), with
    /// an explicit stack whose depth is independent of the native stack.
    pub(super) fn try_map_types<E>(
        &self,
        pool: Types<'_>,
        map: impl FnMut(Ty) -> Result<Ty, E>,
    ) -> Result<Self, E> {
        self.try_map_types_with(pool, map, &mut HashMap::default())
    }

    /// A single answer may contain candidates sharing subproofs. Its
    /// canonicalization shares this cache across those candidate roots;
    /// the borrowed answer keeps every source allocation alive throughout.
    pub(super) fn try_map_types_with<E>(
        &self,
        pool: Types<'_>,
        mut map: impl FnMut(Ty) -> Result<Ty, E>,
        done: &mut HashMap<usize, Self>,
    ) -> Result<Self, E> {
        let key = |r: &Self| r.0.as_ref().map(|a| Arc::as_ptr(a).cast::<()>() as usize);
        let Some(root) = key(self) else {
            return Ok(Self::default());
        };
        let mut stack = vec![(self, false)];
        while let Some((at, exit)) = stack.pop() {
            let Some(id) = key(at) else { continue };
            if done.contains_key(&id) {
                continue;
            }
            if !exit {
                stack.push((at, true));
                for r in at {
                    if let RegionObligation::ImplBound { obligations, .. } = r {
                        stack.push((obligations, false));
                    }
                }
                continue;
            }
            let mut requirements = Vec::with_capacity(at.iter().len());
            for r in at {
                requirements.push(match r {
                    RegionObligation::TypeOutlives { ty, region } => {
                        RegionObligation::TypeOutlives {
                            ty: map(*ty)?,
                            region: *region,
                        }
                    }
                    RegionObligation::ImplBound {
                        row,
                        args,
                        step,
                        obligations,
                    } => {
                        let args = pool
                            .list_items(*args)
                            .iter()
                            .copied()
                            .map(&mut map)
                            .collect::<Result<Vec<_>, _>>()?;
                        // ruled out: a nonempty child's postorder frame runs
                        // before this parent's exit frame.
                        let obligations = match key(obligations) {
                            Some(k) => done[&k].clone(),
                            None => Self::default(),
                        };
                        RegionObligation::ImplBound {
                            row: *row,
                            args: pool.list(&args),
                            step: *step,
                            obligations,
                        }
                    }
                });
            }
            done.insert(id, Self::new(pool, requirements));
        }
        // ruled out: the root exit frame runs before the stack empties.
        Ok(done[&root].clone())
    }
}
