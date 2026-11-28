//! Substitute storage type shapes without expanding a shared type graph or
//! recursing on its depth. Function dependency maps and row parameters do not
//! describe a place's reference mutability; keep those annotations unchanged.

use hd_base::HashMap;
use hd_types::{InternPool, ParamRef, RowId, Ty, TyData, TyMap};

pub fn substitute(
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
