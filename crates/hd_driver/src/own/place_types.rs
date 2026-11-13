//! Substitute storage type shapes without expanding a shared type graph or
//! recursing on its depth. Function dependency maps and row parameters do not
//! describe a place's reference mutability; keep those annotations unchanged.

use hd_base::HashMap;
use hd_types::{InternPool, ParamRef, RowId, Ty, TyData, TyMap};

pub(super) fn substitute(
    pool: &InternPool,
    root: Ty,
    memo: &mut HashMap<Ty, Ty>,
    replace: &dyn Fn(ParamRef) -> Option<Ty>,
    mut visit: impl FnMut(),
) -> Ty {
    struct Resolved<'a> {
        pool: &'a InternPool,
        types: &'a HashMap<Ty, Ty>,
    }
    impl TyMap for Resolved<'_> {
        fn ty(&mut self, t: Ty) -> Ty {
            self.types.get(&t).copied().unwrap_or(t)
        }
        fn row(&mut self, r: RowId) -> RowId {
            let mut data = self.pool.row_data(r);
            for ty in &mut data.keys {
                *ty = self.ty(*ty);
            }
            self.pool.row(&data)
        }
    }
    let types = pool.types();
    let mut work = vec![(root, false)];
    let mut children = Vec::new();
    while let Some((ty, ready)) = work.pop() {
        visit();
        if memo.contains_key(&ty) {
            continue;
        }
        let result = if !pool.has_param(ty) {
            ty
        } else if let TyData::Param(p) = pool.get(ty) {
            replace(p).unwrap_or(ty)
        } else if ready {
            types.map_children(ty, &mut Resolved { pool, types: memo })
        } else {
            work.push((ty, true));
            children.clear();
            types.children(ty, &mut children);
            work.extend(children.iter().rev().map(|&child| (child, false)));
            continue;
        };
        memo.insert(ty, result);
    }
    memo.get(&root).copied().unwrap_or(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::DefId;

    #[test]
    fn substitution_visits_shared_edges_once_and_never_uses_the_native_stack() {
        let pool = InternPool::default();
        let parameter = ParamRef {
            owner: DefId::from_raw(7),
            index: 0,
        };
        let leaf = pool.intern_ty(&TyData::Param(parameter));
        for (width, depth) in [(1000, 20), (1, 10000)] {
            let mut root = leaf;
            let mut roots = Vec::new();
            for _ in 0..depth {
                root = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&vec![root; width]),
                    rest: None,
                });
                roots.push(root);
            }
            let mut memo = HashMap::default();
            let mut work = 0;
            let mut result = substitute(
                &pool,
                root,
                &mut memo,
                &|p| (p == parameter).then_some(Ty::I32),
                || work += 1,
            );
            assert!(
                work <= (width + 2) * depth + 1,
                "{work} visits for width {width}, depth {depth}"
            );
            let before = work;
            for root in roots {
                substitute(
                    &pool,
                    root,
                    &mut memo,
                    &|p| (p == parameter).then_some(Ty::I32),
                    || work += 1,
                );
            }
            assert_eq!(
                work - before,
                depth,
                "overlapping field queries reuse all substituted children"
            );
            for _ in 0..depth {
                let TyData::Tuple { elems, .. } = pool.get(result) else {
                    panic!("tuple shape changed");
                };
                let elements = pool.list_items(elems);
                assert_eq!(elements.len(), width);
                result = elements[0];
                assert!(elements.iter().all(|&ty| ty == result));
            }
            assert_eq!(result, Ty::I32);
        }
    }
}
