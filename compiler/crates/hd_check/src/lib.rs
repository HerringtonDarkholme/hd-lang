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
pub mod stages;
mod ty;

pub use body::{BodyCx, check_fn};
pub use call::MethodIndex;
