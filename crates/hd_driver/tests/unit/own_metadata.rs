use hd_base::BuildProfile;
use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::testing::{
    arm_metadata_checks, metadata_checks, stored_bodies_verified, verify_stored_bodies,
};

use crate::support::build_at;

/// Ownership guards and equivalent IR representations from real checked
/// programs (the checks live in `src/own/test_hooks.rs`).
#[test]
fn corrupt_ownership_metadata_from_a_real_program_is_rejected() {
    let source = include_str!("../cases/counts/local-reference-alias-adaptations.hd");
    let store = MemoryStore::default();
    build_at(
        &store,
        source,
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    let warm = MemoryStore::default();
    for (kind, key) in store.drain_new() {
        if kind == EntryKind::Owned {
            warm.put(
                kind,
                &key,
                &store.get(kind, &key).expect("stored owned body"),
            );
        }
    }
    verify_stored_bodies();
    arm_metadata_checks();
    build_at(
        &warm,
        source,
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../cases/counts/returned-call-owner-handoffs.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../cases/own/conditional-match-uniqueness.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    build_at(
        &warm,
        include_str!("../cases/own/deferred-uniqueness-sequences.hd"),
        BuildProfile::Release,
        hd_base::OptLevel::Optimized,
    );
    let (variants, corrupted) = metadata_checks();
    assert_eq!(variants, Some([1; 3]));
    stored_bodies_verified().expect("active verification");
    let corrupted = corrupted.expect("active corruption checks");
    assert_eq!(corrupted, [1; 16]);
}
