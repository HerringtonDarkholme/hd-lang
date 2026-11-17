//! Every driver integration test, linked once: one binary per crate keeps the
//! compiler from relinking and from starting a process for each file.
//! `conformance.rs` stays a separate target (CI runs it on its own).
//!
//! A test here checks what a fixture cannot show: counters and cost at two
//! sizes, the cache and warm rebuilds, executors, host linking and APIs. A
//! check that puts hd source in and expects diagnostics, output or the
//! linked functions is a stage case under `tests/cases/` (#1975).

mod all_wakes;
mod allocator_init;
mod block_decorator;
mod checker;
mod claim_order;
mod closure_exit_rows;
mod coherence_scale;
mod competing_drivers;
mod context_values;
mod dbg_release;
mod derive_members;
mod derive_run;
mod dyn_diamond_work;
mod entry_point;
mod exit;
mod fixture_roots;
mod fixture_views;
mod float_text;
mod function_reference_lowered;
mod function_references;
mod function_value_loan_work;
mod function_value_native_drop;
mod generics_determinism;
mod header_goals;
mod host_drive_slots;
mod iface_put_order;
mod impl_universe;
mod import_fix_line_endings;
mod inferred_rows_run;
mod init_cycle_run;
mod inlined_callee_edit;
mod instantiation_depth;
mod instruction_origins;
mod join_once;
mod json_keyed_work;
mod json_object_remove;
mod lazy_borrow_work;
mod lazy_ifaces;
mod least_common_type_run;
mod map_run;
mod match_decision_tree_run;
mod match_work;
mod method_chain_depth;
mod module_row_work;
mod owned_package_edit;
mod parallel_bodies;
mod pathological;
mod profile_handles;
mod provider_evidence_work;
mod recursive_layouts;
mod release_profile_run;
mod resume_dispatch;
mod reuse_ifaces;
mod row_aliases;
mod rows_determinism;
mod runtime;
mod same_name_packages;
mod scenario_cancel;
mod scenario_cleanup;
mod session_store;
mod std_drop_invariant;
mod std_interfaces;
mod std_stages;
mod struct_chain_size;
mod structure_run;
mod structure_specialize;
mod suspension_frame_work;
mod test_plan;
mod tests_block_impls;
mod tests_block_shape;
mod track_caller_dependency;
mod trials;
mod type_depth;
mod unlowered_intrinsic;
mod validate_pattern;
mod warm_source_reads;

/// One content-addressed store shared by every test that only checks what
/// a build produces: std compiles once for the process.
pub struct Shared(&'static hd_cache::MemoryStore);

pub fn warm_store() -> Shared {
    static STORE: std::sync::OnceLock<hd_cache::MemoryStore> = std::sync::OnceLock::new();
    Shared(STORE.get_or_init(hd_cache::MemoryStore::default))
}

impl std::ops::Deref for Shared {
    type Target = hd_cache::MemoryStore;
    fn deref(&self) -> &hd_cache::MemoryStore {
        self.0
    }
}

impl hd_cache::CacheStore for Shared {
    fn get(
        &self,
        kind: hd_cache::EntryKind,
        key: &hd_base::Hash128,
    ) -> Option<std::sync::Arc<[u8]>> {
        self.0.get(kind, key)
    }
    fn put(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128, bytes: &[u8]) {
        self.0.put(kind, key, bytes);
    }
    fn touch(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128) {
        self.0.touch(kind, key);
    }
}

mod map_seed_runtime;
mod trait_reference_coverage;

mod supplied_tuple_specialize;
