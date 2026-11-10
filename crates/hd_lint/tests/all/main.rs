//! `hd_lint`'s integration tests as one binary. Each separate test binary is a
//! full debug link of the compiler, so one per crate is the cheap layout.

mod binding_patterns;
mod bindings;
mod block_decorators;
mod comparisons;
mod derivation_drift;
mod derivation_facts;
mod disposable;
mod flow;
mod lifetime_order;
mod lost_writes;
mod modules;
mod registry;
