//! Every driver integration test, linked once: one binary per crate keeps the
//! compiler from relinking and from starting a process for each file.
//! `conformance.rs` stays a separate target (CI runs it on its own).

mod arity_generic_run;
mod assoc_types;
mod assoc_validation;
mod binding_chain_run;
mod block_decorator;
mod call_place_run;
mod cancel_suspension_run;
mod char_escapes_run;
mod checker;
mod collect_default_run;
mod context_values;
mod copy_assign_run;
mod copy_update_run;
mod declaration_checks;
mod default_args_run;
mod delegation_run;
mod derive_members;
mod derive_run;
mod diagnostic_messages;
mod downcast_check;
mod downcast_run;
mod dyn_bindings_run;
mod dyn_bound_run;
mod dyn_generic_run;
mod dyn_rows_run;
mod error_derive_run;
mod error_markers;
mod exit;
mod explicit_type_arguments_run;
mod facts_of_check;
mod facts_of_run;
mod failed_use;
mod float_patterns_run;
mod float_run;
mod float_text;
mod function_reference_builtin_methods;
mod function_reference_lowered;
mod function_references;
mod generics_determinism;
mod header_goals;
mod identity_operands;
mod identity_run;
mod impl_universe;
mod incremental;
mod inferred_results_run;
mod inferred_rows_run;
mod instantiation_depth;
mod int_widths_run;
mod intrinsic_methods;
mod item_names;
mod least_common_type_run;
mod let_pattern_mut_run;
mod list_invalidation_run;
mod list_spread_run;
mod literals;
mod local_items_run;
mod map_duplicates_run;
mod map_key_bounds;
mod map_run;
mod match_cascade;
mod method_choice;
mod misplaced_derivation;
mod mixed_variant_patterns_run;
mod module_member_run;
mod nested_interpolation_run;
mod nested_optional_run;
mod newtype_derive_run;
mod optional_join_run;
mod pipe_bare_steps;
mod pipe_placeholder;
mod power_run;
mod privacy;
mod private_main;
mod promotion;
mod provider_keys_run;
mod qualified_variants;
mod range_patterns;
mod recursive_closure_run;
mod recursive_layouts;
mod related_traits;
mod row_aliases;
mod row_invariant_nested;
mod row_param_nested;
mod row_union_run;
mod rows_determinism;
mod runtime;
mod sealed_rows;
mod slicing_run;
mod spread_args_run;
mod std_interfaces;
mod std_stages;
mod stored_fn_call;
mod structure_run;
mod supertrait_calls_run;
mod suspending_closures_run;
mod test_plan;
mod trailing_blocks;
mod trials;
mod try_hint_run;
mod tuple_rest_run;
mod tuple_structure;
mod tuple_template_bounds;
mod type_name_forms_run;
mod typeid_of;
mod vararg_value_run;
mod variant_value_run;
mod void_unit_run;

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
