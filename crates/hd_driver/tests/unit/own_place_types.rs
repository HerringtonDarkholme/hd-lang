use hd_base::HashMap;
use hd_driver::testing::substitute;
use hd_types::{InternPool, ParamRef, Ty, TyData};

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
                root = pool.product(pool.list(&vec![root; width]), None);
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
                let (elements, _) = pool.product_parts(result).expect("product shape changed");
                assert_eq!(elements.len(), width);
                result = elements[0];
                assert!(elements.iter().all(|&ty| ty == result));
            }
            assert_eq!(result, Ty::I32);
        }
    }
}
