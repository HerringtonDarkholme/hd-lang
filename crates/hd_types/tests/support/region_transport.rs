//! Internal proof transport, replay, work-count and destruction tests.
use super::*;
use crate::{InternPool, LocalPool};

type RegionShape = (Option<ImplRef>, TyList, Vec<(u16, usize)>);

fn shape(proof: &RegionObligations) -> Vec<RegionShape> {
    let mut done = HashMap::<usize, usize>::default();
    let mut interned = HashMap::<RegionShape, usize>::default();
    let mut shapes = Vec::new();
    let mut work = vec![(proof, false)];
    while let Some((p, exit)) = work.pop() {
        if p.is_empty() || done.contains_key(&p.key()) {
            continue;
        }
        let Some(node) = &p.0 else { continue };
        if exit {
            let shape = (
                node.row,
                node.args,
                node.steps
                    .iter()
                    .map(|(s, p)| (*s, if p.is_empty() { 0 } else { done[&p.key()] }))
                    .collect(),
            );
            let next = shapes.len() + 1;
            let id = *interned.entry(shape.clone()).or_insert_with(|| {
                shapes.push(shape);
                next
            });
            done.insert(p.key(), id);
        } else {
            work.push((p, true));
            work.extend(node.steps.iter().map(|(_, p)| (p, false)));
        }
    }
    shapes
}

#[test]
fn canonical_replay_remaps_retained_arguments() -> Result<(), &'static str> {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let pool = Types::with_local(&global, &local);
    let from = InferVar::from_raw(10);
    let to = InferVar::from_raw(20);
    let vars = |v| CanonVars {
        map: vec![v],
        kinds: vec![VarKind::General],
    };
    let answer = Answer::Holds {
        evidence: Evidence::Builtin(BuiltinImpl::Static),
        learned: Vec::new(),
        obligations: RegionObligations::new(
            pool,
            Some(ImplRef {
                module: ModuleId::from_raw(0),
                row: 0,
            }),
            pool.list(&[pool.intern_ty(&TyData::Infer(from))]),
            vec![(
                0,
                RegionObligations::new(pool, None, TyList::EMPTY, Vec::new()),
            )],
        ),
    };
    let canonical = canon_answer(pool, &answer, &vars(from)).ok_or("proof did not canonicalize")?;
    let Answer::Holds { obligations, .. } = decanon_answer(pool, &canonical, &vars(to)) else {
        return Err("proof did not replay");
    };
    let (_, args, steps) = obligations.parts().ok_or("replayed proof lost its impl")?;
    assert_eq!(pool.list_items(args), &[pool.intern_ty(&TyData::Infer(to))]);
    assert!(!steps[0].1.is_empty());
    Ok(())
}

#[test]
fn shared_mapping_and_closed_replay_count_actual_mapper_calls() -> Result<(), &'static str> {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let pool = Types::with_local(&global, &local);
    let infer = pool.list(&[pool.intern_ty(&TyData::Infer(InferVar::from_raw(0)))]);
    for width in [100, 1000] {
        let mut proof = RegionObligations::new(pool, None, TyList::EMPTY, Vec::new());
        for _ in 0..20 {
            proof = RegionObligations::new(
                pool,
                Some(ImplRef {
                    module: ModuleId::from_raw(0),
                    row: 0,
                }),
                infer,
                (0..width).map(|step| (step, proof.clone())).collect(),
            );
        }
        let mut calls = 0;
        let mapped = proof.try_map(pool, 1, |_| {
            calls += 1;
            Ok::<_, &'static str>(pool.list(&[Ty::I32]))
        })?;
        assert_eq!(calls, 20);
        assert_eq!(shape(&mapped).len(), 21);
        let replay = mapped.try_map(pool, 1, |args| {
            calls += 1;
            Ok::<_, &'static str>(args)
        })?;
        assert_eq!(calls, 20);
        assert_eq!(mapped.key(), replay.key());
        assert_eq!(mapped, replay);
    }
    Ok(())
}

#[test]
fn deep_shared_proofs_drop_iteratively_with_concurrent_last_owners() -> Result<(), &'static str> {
    let global = InternPool::new();
    let pool = global.types();
    let mut proof = RegionObligations::new(pool, None, TyList::EMPTY, Vec::new());
    for _ in 0..10_000 {
        proof = RegionObligations::new(
            pool,
            Some(ImplRef {
                module: ModuleId::from_raw(0),
                row: 0,
            }),
            TyList::EMPTY,
            vec![(0, proof)],
        );
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let spawn = |proof, barrier: std::sync::Arc<std::sync::Barrier>| {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                barrier.wait();
                drop(proof);
            })
    };
    let a = spawn(proof.clone(), barrier.clone()).map_err(|_| "start first worker")?;
    let b = spawn(proof, barrier).map_err(|_| "start second worker")?;
    a.join().map_err(|_| "first worker failed")?;
    b.join().map_err(|_| "second worker failed")?;
    Ok(())
}
