#![forbid(unsafe_code)]
//! `hd_check`: body checking that emits TIR (checking-and-tir.md §4.13)
//! over `hd_types` (`InternPool`, `InferTable`, the solver) and
//! `hd_tir::ir`, plus the checking stages beyond bodies (the header checks
//! that need goals, test overlay, init order). It reads syntax and
//! interfaces and writes TIR; nothing downstream reads syntax.

mod access;
pub mod body;
mod call;
pub mod conflicts;
mod conform;
pub mod derive;
pub mod error;
mod expr;
mod fnref;
pub mod header;
pub mod init;
mod literals;
mod local;
mod pat;
mod privacy;
mod promote;
mod render;
pub mod results;
mod rows;
pub mod stages;
pub mod structure;
pub mod tests;
mod trial;
mod ty;
mod update;

pub use body::{BodyCx, TestsView, check_default, check_fn, default_body_def};
pub use call::MethodIndex;
pub use conform::omitted_trait_methods;
pub use render::render;
