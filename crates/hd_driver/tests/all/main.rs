//! Every driver integration test, linked once: one binary per crate keeps the
//! compiler from relinking and from starting a process for each file.
//! `conformance.rs` stays a separate target (CI runs it on its own).

mod all_wakes;
mod ambiguous_row_fit;
mod annotation_member_names;
mod assoc_call_qualified;
mod assoc_types;
mod assoc_validation;
mod bare_trait_body;
mod block_decorator;
mod by_structure_import;
mod caret_is_xor;
mod checker;
mod closure_parameter_annotation;
mod coherence_scale;
mod collection_value_fix;
mod combinator_call_forms;
mod competing_drivers;
mod constant_defaults;
mod context_values;
mod cycle_linking;
mod dbg_release;
mod declaration_checks;
mod decorator_misplaced;
mod default_args_run;
mod derivation_line_drift;
mod derive_members;
mod derive_run;
mod derived_bound_unmet;
mod derived_default_declared;
mod diagnostic_messages;
mod downcast_check;
mod duplicate_facts;
mod dyn_ref_value_fix;
mod enclosing_bounds;
mod entry_point;
mod error_markers;
mod exit;
mod explicit_row_slot;
mod extra_trait_members;
mod fact_eval_run;
mod facts_of_check;
mod failed_use;
mod fixture_roots;
mod fixture_views;
mod float_text;
mod forbidden_context_run;
mod function_reference_builtin_methods;
mod function_reference_lowered;
mod function_references;
mod generic_inference_join;
mod generic_local_fn_run;
mod generic_member_call;
mod generics_determinism;
mod header_goals;
mod identity_operands;
mod impl_targets;
mod impl_universe;
mod inferred_result_variance;
mod inferred_results_run;
mod inferred_rows_run;
mod init_cycle_run;
mod inspectable_types;
mod instantiation_depth;
mod instruction_origins;
mod intrinsic_methods;
mod item_names;
mod item_too_complex;
mod join_once;
mod lazy_ifaces;
mod least_common_type_run;
mod let_mut_module_level;
mod list_invalidation_run;
mod literals;
mod local_items_run;
mod map_key_bounds;
mod map_run;
mod match_cascade;
mod match_decision_tree_run;
mod method_bound_unmet;
mod method_choice;
mod misplaced_derivation;
mod mixed_signedness;
mod module_case_collision;
mod named_argument_errors;
mod nonnumeric_cast;
mod numeric_widen_fix;
mod operator_fn_left_exact;
mod parallel_bodies;
mod pipe_bare_steps;
mod pipe_placeholder;
mod poisoned_expectation;
mod poll_context_construction;
mod prelude_binding_shadow;
mod privacy;
mod private_main;
mod private_type_leak;
mod profile_handles;
mod promotion;
mod provider_keys_run;
mod qualified_variants;
mod range_bound_numeric;
mod range_patterns;
mod receiver_fix;
mod recursive_layouts;
mod recursive_value_fix;
mod reference_equality_fix;
mod rejected_pattern_binds;
mod related_traits;
mod release_profile_run;
mod resume_dispatch;
mod row_aliases;
mod row_invariant_nested;
mod row_one_body;
mod row_param_nested;
mod rows_determinism;
mod runtime;
mod same_name_packages;
mod scenario_cancel;
mod scenario_cleanup;
mod sealed_rows;
mod sealed_user_impls;
mod session_store;
mod std_interfaces;
mod std_stages;
mod stored_fn_call;
mod structure_run;
mod suspending_closures_run;
mod template_shape;
mod test_placement;
mod test_plan;
mod test_registration_spellings;
mod test_runner_registration_rows;
mod tests_block_impls;
mod tests_block_shape;
mod trailing_blocks;
mod trait_resolution_depth;
mod trials;
mod tuple_structure;
mod tuple_template_bounds;
mod type_arity_written_run;
mod type_default_written_run;
mod type_name_forms_run;
mod type_too_long_run;
mod typed_facts;
mod typeid_of;
mod unlowered_intrinsic;
mod unreachable_code;
mod unread_facts;
mod unread_must_use_binding;
mod unsigned_comparison;
mod unsolved_call_parameter;
mod unsolved_scrutinee;
mod vararg_invalid;
mod vararg_value_run;
mod variance_representation;
mod variant_binding_names;
mod variant_value_run;
mod void_final;
mod void_unit_run;
mod waiting_impl_run;

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

mod owned_adapter_calls;
