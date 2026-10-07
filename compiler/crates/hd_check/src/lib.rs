#![forbid(unsafe_code)]
//! `hd_check`: body checking that emits TIR (checking-and-tir.md §4.13)
//! over `hd_types` (`InternPool`, `InferTable`, the solver) and
//! `hd_tir::ir`, plus the checking stages beyond bodies (test overlay,
//! coherence, init order). It reads syntax and interfaces and writes TIR;
//! nothing downstream reads syntax.

pub mod body;
mod call;
mod expr;
mod pat;
mod render;
pub mod stages;
mod ty;

pub use body::{BodyCx, check_default, check_fn, default_body_def};
pub use call::MethodIndex;
pub use render::render;
