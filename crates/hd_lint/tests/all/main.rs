//! `hd_lint`'s integration tests as one binary. Each separate test binary is a
//! full debug link of the compiler, so one per crate is the cheap layout.

mod binding_patterns;
mod bindings;
mod comparisons;
mod derivation_facts;
mod flow;
mod modules;
mod registry;
mod warm;
