use hd_base::DefId;
use hd_driver::testing::{RefFacts, TypeFacts, TypeTable, read_only_factories, ref_facts};
use hd_tir::ir::Callee;
use hd_types::{Ty, TyData, TyList};

mod owned_cache_tests {
    use hd_base::{BuildProfile, Hash128};
    use hd_cache::{CacheStore, EntryKind, MemoryStore};
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::InternPool;

    use crate::support::build_at;
    use hd_driver::testing::{stored_bodies_verified, verify_stored_bodies};

    /// Programs over much of std: JSON and serde, maps, sorting, strings,
    /// closures, tasks and trait values.
    const PROGRAMS: [&str; 5] = [
        include_str!("../../../../bench/study/json.hd"),
        include_str!("../../../../bench/runtime/progs/map-count.hd"),
        include_str!("../../../../bench/runtime/progs/task-pipeline.hd"),
        include_str!("../../../../bench/runtime/progs/string-builder.hd"),
        include_str!("../../../../bench/runtime/progs/plugins.hd"),
    ];

    fn build(store: &MemoryStore, text: &str, profile: BuildProfile) -> hd_driver::Output {
        build_at(store, text, profile, hd_base::OptLevel::Dev)
    }

    /// Every std body these programs own is stored once per profile, its
    /// wire form decodes into another run's tables and encodes back to the
    /// same bytes, and a later build takes each from the store, whose
    /// owned verifier accepts it.
    #[test]
    fn every_std_body_owned_form_round_trips() {
        let store = MemoryStore::default();
        for profile in [BuildProfile::Checked, BuildProfile::Release] {
            for text in PROGRAMS {
                build(&store, text, profile);
            }
        }
        let owned: Vec<Hash128> = store
            .drain_new()
            .into_iter()
            .filter_map(|(kind, key)| (kind == EntryKind::Owned).then_some(key))
            .collect();
        assert!(owned.len() > 380, "{} owned std bodies", owned.len());
        // A store with only the owned bodies: the builds below check, collect
        // and link again, and take every std body from it.
        let warm = MemoryStore::default();
        for key in &owned {
            let entry = store.get(EntryKind::Owned, key).expect("stored");
            let (_, sections) = hd_cache::decode_entry(&entry).expect("an entry");
            let wire = sections[1].1;
            let (pool, paths, syms) = (
                InternPool::new(),
                PathTable::new(),
                ShardedInterner::default(),
            );
            // Other IDs than the writer's run.
            paths.item("zzz", "a", "b");
            let body = hd_tir::wire::read_body(wire, &pool, &paths, &syms).expect("decodes");
            let again = hd_tir::wire::write_body(&body, &pool, &paths, &syms).expect("encodes");
            assert_eq!(wire, &again[..], "{key:?}");
            warm.put(EntryKind::Owned, key, &entry);
        }
        verify_stored_bodies();
        let mut hits = 0;
        for profile in [BuildProfile::Checked, BuildProfile::Release] {
            for text in PROGRAMS {
                let out = build(&warm, text, profile);
                assert_eq!(out.counters.misses.get("owned"), None);
                hits += out.counters.hits.get("owned").copied().unwrap_or(0);
            }
        }
        assert_eq!(stored_bodies_verified(), Some(hits));
        assert!(hits >= owned.len());
    }
}

#[test]
fn supplied_factory_effects_preserve_external_initializers_and_target_identity()
-> Result<(), String> {
    use hd_tir::ir::{BodyKind, Providers, TirBuilder, TirSink};
    let pool = hd_types::InternPool::new();
    let target = Ty::I32;
    let helper = |def: u32, callee: Option<(u32, Ty)>| {
        let mut b = TirBuilder::new(DefId::from_raw(def), BodyKind::Handle);
        let root = b.open_block();
        if let Some((called, target)) = callee {
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(called),
                    targs: pool.list(&[target]),
                },
                &[],
                Providers::None,
                Ty::VOID,
                hd_base::NodeIdx::NONE,
            );
        }
        let root = b.close_block(root, None, Ty::VOID, hd_base::NodeIdx::NONE);
        b.finish(root, &[]).map_err(|errors| format!("{errors:?}"))
    };
    let index = [(DefId::from_raw(1), 0), (DefId::from_raw(2), 1)]
        .into_iter()
        .collect();
    let static_family = [helper(1, Some((2, target)))?, helper(2, None)?];
    assert!(read_only_factories(
        &pool,
        target,
        &static_family,
        &index,
        &|_| true
    ));
    let effectful = [helper(1, Some((2, target)))?, helper(2, Some((3, target)))?];
    assert!(!read_only_factories(
        &pool,
        target,
        &effectful,
        &index,
        &|_| true
    ));
    assert!(read_only_factories(
        &pool,
        target,
        &effectful,
        &index,
        &|_| false
    ));
    let other_target = [helper(1, Some((2, Ty::BOOL)))?, helper(2, None)?];
    assert!(!read_only_factories(
        &pool,
        target,
        &other_target,
        &index,
        &|_| true
    ));
    Ok(())
}

#[test]
fn a_function_beside_a_counted_reference_may_hold_a_local_reference() {
    let pool = hd_types::InternPool::new();
    let ptr = DefId::from_raw(1);
    let boxed = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        counted: true,
        mutable: false,
    });
    let callback = pool.intern_ty(&TyData::Fn {
        params: TyList::EMPTY,
        result: Ty::I32,
        row: hd_types::RowId::EMPTY,
        suspends: false,
        inputs: hd_types::Inputs::Fixed,
        deps: hd_types::FnDeps::NONE,
    });
    let pair = |right| pool.product(pool.list(&[boxed, right]), None);
    let mut memo = TypeTable::default();
    let mut only = |t| {
        let f = ref_facts(&pool, ptr, t, &mut memo);
        f.only_counted && f.counted
    };
    assert!(only(pair(Ty::STRING)));
    assert!(!only(pair(callback)));
}

#[test]
fn the_type_table_indexes_pool_types_and_maps_body_local_ones() {
    let facts = |holds| TypeFacts {
        refs: RefFacts {
            holds,
            ..RefFacts::NONE
        },
        layout: None,
        mode: None,
        function_input: None,
        collection: None,
    };
    let mut table = TypeTable::default();
    let (far, near, local) = (Ty(1_000), Ty(3), Ty(Ty::I32.0 | 0x8000_0000));
    assert!(local.is_local() && !far.is_local());
    table.insert(far, facts(true));
    table.insert(near, facts(false));
    table.insert(local, facts(true));
    assert!(table.contains(far) && table.contains(near) && table.contains(local));
    assert!(!table.contains(Ty(999)) && !table.contains(Ty(5_000)));
    assert_eq!(table.global.len(), 1_001);
    assert_eq!(table.local.len(), 1);
    // ruled out: a missing entry; both were inserted above.
    table.get_mut(near).expect("an entry").layout = Some((true, None));
    // ruled out: a missing entry; both were inserted above.
    table.get_mut(local).expect("an entry").layout = Some((false, Some(true)));
    assert_eq!(table.get(near).and_then(|f| f.layout), Some((true, None)));
    assert_eq!(
        table.get(local).and_then(|f| f.layout),
        Some((false, Some(true)))
    );
    assert!(table.get_mut(Ty(7)).is_none());
}
