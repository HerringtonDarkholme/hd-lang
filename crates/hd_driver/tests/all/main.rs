//! Every driver integration test, linked once: one binary per crate keeps the
//! compiler from relinking and from starting a process for each file.
//! `conformance.rs` stays a separate target (CI runs it on its own).

mod ambiguous_row_fit;
mod annotation_member_names;
mod arity_generic_run;
mod assoc_call_qualified;
mod assoc_types;
mod assoc_validation;
mod bare_trait_body;
mod binding_chain_run;
mod block_decorator;
mod by_structure_import;
mod call_place_run;
mod cancel_suspension_run;
mod char_escapes_run;
mod checker;
mod closure_code_targets;
mod closure_parameter_annotation;
mod collect_default_run;
mod combinator_call_forms;
mod competing_drivers;
mod constant_closures;
mod constant_vtables;
mod context_values;
mod copy_assign_run;
mod copy_update_run;
mod crlf_run;
mod dbg_release;
mod declaration_checks;
mod decorator_misplaced;
mod default_args_run;
mod delegation_run;
mod derivation_line_drift;
mod derive_members;
mod derive_run;
mod derived_bound_unmet;
mod derived_default_declared;
mod diagnostic_messages;
mod downcast_check;
mod downcast_run;
mod duplicate_facts;
mod dyn_bindings_run;
mod dyn_bound_run;
mod dyn_generic_run;
mod dyn_rows_run;
mod dyn_self_member_run;
mod enclosing_bounds;
mod entry_point;
mod error_derive_run;
mod error_markers;
mod exit;
mod explicit_row_slot;
mod explicit_type_arguments_run;
mod extra_trait_members;
mod fact_eval_run;
mod facts_of_check;
mod facts_of_run;
mod failed_use;
mod float_patterns_run;
mod float_run;
mod float_text;
mod forbidden_context_run;
mod function_reference_builtin_methods;
mod function_reference_lowered;
mod function_references;
mod generic_inference_join;
mod generic_local_fn_run;
mod generic_member_call;
mod generic_shared_data_run;
mod generics_determinism;
mod header_goals;
mod identity_operands;
mod identity_run;
mod impl_targets;
mod impl_universe;
mod incremental;
mod inferred_result_variance;
mod inferred_results_run;
mod inferred_rows_run;
mod init_cycle_run;
mod initialization_implicit_run;
mod inspectable_types;
mod instantiation_depth;
mod int_widths_run;
mod intrinsic_methods;
mod item_names;
mod item_too_complex;
mod key_assoc_function_run;
mod known_closure_calls;
mod least_common_type_run;
mod let_mut_module_level;
mod let_pattern_mut_run;
mod list_invalidation_run;
mod list_spread_run;
mod literals;
mod local_fn_default_run;
mod local_items_run;
mod map_duplicates_run;
mod map_key_bounds;
mod map_run;
mod match_cascade;
mod match_decision_tree_run;
mod member_line_lists_run;
mod method_bound_unmet;
mod method_choice;
mod misplaced_derivation;
mod mixed_signedness;
mod mixed_variant_patterns_run;
mod module_case_collision;
mod module_member_run;
mod named_argument_errors;
mod nested_interpolation_run;
mod nested_optional_run;
mod newtype_derive_run;
mod operator_fn_left_exact;
mod operator_left_primitive_run;
mod optional_join_run;
mod payloadless_variants;
mod pipe_bare_steps;
mod pipe_placeholder;
mod poisoned_expectation;
mod poll_context_construction;
mod power_run;
mod prelude_binding_shadow;
mod privacy;
mod private_main;
mod private_type_leak;
mod promotion;
mod provider_keys_run;
mod qualified_variants;
mod radix_literals_run;
mod range_bound_integer;
mod range_patterns;
mod raw_identifier_run;
mod recursive_closure_run;
mod recursive_layouts;
mod rejected_pattern_binds;
mod related_traits;
mod release_profile_run;
mod repl_input;
mod row_aliases;
mod row_invariant_nested;
mod row_param_nested;
mod row_union_run;
mod rows_determinism;
mod runtime;
mod same_name_packages;
mod scenario_cancel;
mod scenario_cleanup;
mod sealed_rows;
mod sealed_user_impls;
mod session_collect;
mod session_store;
mod slicing_run;
mod spread_args_run;
mod std_interfaces;
mod std_stages;
mod stored_fn_call;
mod structure_run;
mod supertrait_calls_run;
mod suspending_closures_run;
mod suspending_fn_weaken_run;
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
mod try_hint_run;
mod tuple_rest_run;
mod tuple_structure;
mod tuple_template_bounds;
mod type_arity_signature_run;
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
