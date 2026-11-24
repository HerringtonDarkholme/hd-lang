//! `hd_driver`'s unit tests, over the `testing` facade (`src/testing.rs`):
//! one module per source file they test, and `support` for the shared
//! warm std store.

mod count_audit_tests;
mod count_ops_tests;
mod diagnostics;
mod driver;
mod own;
#[cfg(not(target_arch = "wasm32"))]
mod own_metadata;
mod own_place_types;
mod package_result;
mod pipeline;
mod positions;
#[cfg(not(target_arch = "wasm32"))]
mod program_ir_tests;
mod skims;
mod strip_comments;
#[path = "../../src/strip_comments.rs"]
mod strip_comments_src;
mod support;
