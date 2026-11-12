//! The existing monomorphization depth check, over the interned type DAG.
use hd_base::HashMap;
use hd_types::{InternPool, Ty, TyData};

fn parts(pool: &InternPool, t: Ty) -> Vec<Ty> {
    let mut parts = Vec::new();
    let row_keys = |row, parts: &mut Vec<Ty>| parts.extend(pool.row_data(row).keys);
    match pool.get(t) {
        TyData::Adt { args, .. } => parts.extend(pool.list_items(args)),
        TyData::Opaque { args, bound, .. } => {
            parts.extend(pool.list_items(args));
            parts.push(bound);
        }
        TyData::Tuple { elems, rest } => {
            parts.extend(pool.list_items(elems));
            parts.extend(rest);
        }
        TyData::Union(members) => parts.extend(pool.list_items(members)),
        TyData::Ref { inner: i, .. } => parts.push(i),
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            parts.extend(pool.list_items(params));
            parts.push(result);
            row_keys(row, &mut parts);
        }
        TyData::TraitValue { args, bindings, .. } => {
            parts.extend(pool.list_items(args));
            parts.extend(bindings.into_iter().map(|(_, b)| b));
        }
        TyData::Assoc { self_ty, args, .. } => {
            parts.push(self_ty);
            parts.extend(pool.list_items(args));
        }
        TyData::Row(r) | TyData::Context(r) => row_keys(r, &mut parts),
        TyData::Prim(_)
        | TyData::Never
        | TyData::Poison
        | TyData::Param(_)
        | TyData::Const(_)
        | TyData::Infer(_)
        | TyData::Canon(_) => {}
    }
    parts
}

/// Cached argument depth used by emission and summary-only clients. Each new
/// type/edge is visited a constant number of times, with an explicit stack.
pub fn type_argument_depth(pool: &InternPool, done: &mut HashMap<Ty, u32>, root: Ty) -> u32 {
    depth_with(pool, done, root, &mut || {})
}

fn depth_with(
    pool: &InternPool,
    done: &mut HashMap<Ty, u32>,
    root: Ty,
    visit: &mut impl FnMut(),
) -> u32 {
    let mut active = hd_base::HashSet::default();
    let mut work = vec![(root, false)];
    while let Some((ty, ready)) = work.pop() {
        visit();
        if done.contains_key(&ty) {
            continue;
        }
        let children = parts(pool, ty);
        if !ready {
            if !active.insert(ty) {
                // A malformed cyclic type graph cannot pass monomorphization.
                done.insert(ty, u32::from(crate::MAX_DEPTH) + 1);
                continue;
            }
            if children.iter().any(|child| !done.contains_key(child)) {
                work.push((ty, true));
                for child in children {
                    visit();
                    work.push((child, false));
                }
                continue;
            }
        }
        let mut depth = 0;
        for child in children {
            visit();
            depth = depth.max(done.get(&child).copied().unwrap_or(u32::MAX));
        }
        done.insert(ty, depth.saturating_add(1));
        active.remove(&ty);
    }
    done.get(&root).copied().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn depth_query_counts_shared_width_and_deep_dag_work() {
        let run = |width: usize, depth: usize| {
            let pool = InternPool::default();
            let mut ty = Ty::STRING;
            for _ in 0..depth {
                ty = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&vec![ty; width]),
                    rest: None,
                });
            }
            let mut memo = HashMap::default();
            let mut work = 0;
            assert_eq!(
                Some(depth_with(&pool, &mut memo, ty, &mut || work += 1)),
                u32::try_from(depth + 1).ok()
            );
            assert!(work <= 5 * width * depth + 5, "{width}x{depth}: {work}");
            let mut warm = 0;
            depth_with(&pool, &mut memo, ty, &mut || warm += 1);
            assert_eq!(warm, 1);
            work
        };
        let small = run(100, 20);
        let large = run(1000, 20);
        assert!(large <= 12 * small);
        run(2, 1000);
    }
}
