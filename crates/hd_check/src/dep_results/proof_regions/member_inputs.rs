//! Compact positional inputs for the member goals of a Structure template.
//! Plan prefix ends select a member in logarithmic time; tuple ranges are
//! indexed once, rather than rescanning the tuple for each member proof.
use super::{Layouts, Projection, Slots, StageResult, Ty, TyData, bug};
use hd_types::solver::{ImplOrigin, MemberOn, MemberStep};

pub(super) struct Members {
    steps: std::sync::Arc<[MemberStep]>,
    ends: Vec<usize>,
    elements: Vec<(Ty, Slots)>,
    rest_item: Option<(Ty, Slots)>,
    rest_list: Option<(Ty, Slots)>,
}

impl Members {
    pub(super) fn new(
        projection: &mut Projection,
        ls: &Layouts<'_>,
        origin: ImplOrigin,
        target: Ty,
        source: Slots,
        steps: std::sync::Arc<[MemberStep]>,
    ) -> StageResult<Self> {
        let pool = ls.pool;
        let mut elements = Vec::new();
        let (mut rest_item, mut rest_list) = (None, None);
        match (origin, pool.get(target)) {
            (ImplOrigin::TupleTemplate, TyData::Tuple { elems, rest }) => {
                let mut offset = 0;
                for &ty in pool.list_items(elems) {
                    Projection::charge(1);
                    let width = projection.width(ls, ty);
                    elements.push((ty, projection.maps.slice(source, offset, width)));
                    offset += width;
                }
                if let Some(ty) = rest {
                    let width = projection.width(ls, ty);
                    let slots = projection.maps.slice(source, offset, width);
                    rest_list = Some((ty, slots));
                    if let TyData::Adt { args, .. } = pool.get(ty)
                        && let [item] = pool.list_items(args)
                    {
                        rest_item = Some((*item, slots));
                    }
                }
            }
            (ImplOrigin::UnionTemplate, TyData::Union(elems)) => {
                let mut offset = 0;
                for &ty in pool.list_items(elems) {
                    Projection::charge(1);
                    let width = projection.width(ls, ty);
                    elements.push((ty, projection.maps.slice(source, offset, width)));
                    offset += width;
                }
            }
            _ => return Err(bug("a template proof lost its member-bearing target")),
        }
        let mut end = 0;
        let ends = steps
            .iter()
            .map(|step| {
                Projection::charge(1);
                end += match (origin, step.on) {
                    (_, MemberOn::Elements) => elements.len(),
                    (ImplOrigin::TupleTemplate, MemberOn::RestItem) => {
                        usize::from(rest_item.is_some())
                    }
                    (ImplOrigin::TupleTemplate, MemberOn::RestList) => {
                        usize::from(rest_list.is_some())
                    }
                    _ => 0,
                };
                end
            })
            .collect();
        Ok(Self {
            steps,
            ends,
            elements,
            rest_item,
            rest_list,
        })
    }

    pub(super) fn get(&self, goal: usize) -> Option<(&MemberStep, Ty, Slots)> {
        let at = self.ends.partition_point(|&end| {
            crate::work::scan_work(1);
            end <= goal
        });
        let step = self.steps.get(at)?;
        let start = if at == 0 { 0 } else { self.ends[at - 1] };
        let &(ty, slots) = match step.on {
            MemberOn::Elements => self.elements.get(goal - start)?,
            MemberOn::RestItem => self.rest_item.as_ref()?,
            MemberOn::RestList => self.rest_list.as_ref()?,
        };
        Some((step, ty, slots))
    }
}

#[cfg(test)]
mod tests {
    use super::super::Origin;
    use super::*;
    use hd_base::DefId;
    use hd_types::{InternPool, TyList};

    #[test]
    fn member_ranges_are_indexed_once_and_selected_without_rescanning_members() -> StageResult<()> {
        let gp = InternPool::new();
        let pool = gp.types();
        let lifetimed = |_: DefId| false;
        let cache = super::super::super::LayoutCache::default();
        let ls = Layouts::new(pool, DefId::NONE, &lifetimed, &cache);
        let reference = pool.intern_ty(&TyData::Ref {
            mutable: false,
            counted: false,
            inner: Ty::I32,
        });
        let mut builds = Vec::new();
        for width in [100, 1000] {
            let target = pool.intern_ty(&TyData::Tuple {
                elems: pool.list(&vec![reference; width]),
                rest: None,
            });
            let steps: std::sync::Arc<[MemberStep]> = (0..20)
                .map(|_| MemberStep {
                    on: MemberOn::Elements,
                    trait_: DefId::NONE,
                    args: TyList::EMPTY,
                })
                .collect();
            let mut projection = Projection::default();
            let source = projection.maps.root(0, width);
            let (members, work) = crate::work::count_scan_work(|| {
                Members::new(
                    &mut projection,
                    &ls,
                    ImplOrigin::TupleTemplate,
                    target,
                    source,
                    steps,
                )
            });
            let members = members?;
            builds.push(work);
            let ((), probes) = crate::work::count_scan_work(|| {
                for goal in 0..20 * width {
                    let found = members.get(goal);
                    assert!(found.is_some(), "generated member plan lost goal {goal}");
                    let Some((_, ty, slots)) = found else {
                        continue;
                    };
                    assert_eq!(ty, reference);
                    let origins = projection.maps.expand(slots, 0, 1);
                    assert_eq!(
                        origins,
                        [vec![Origin::Input {
                            port: 0,
                            slot: goal % width
                        }]]
                    );
                }
            });
            // At most six binary-search comparisons plus a slice/root
            // visit and one emitted origin per goal, independent of width.
            eprintln!("template members: width={width}, steps=20, build={work}, probes={probes}");
            assert!(probes <= 9 * 20 * width as u64, "{probes}");
        }
        assert!(builds[0] > 0 && builds[1] <= 12 * builds[0], "{builds:?}");
        Ok(())
    }
}
