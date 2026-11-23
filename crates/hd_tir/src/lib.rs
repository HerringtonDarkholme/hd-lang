#![forbid(unsafe_code)]
//! `hd_tir`: TIR (`crates/hd_tir/DESIGN.md` §4.13.11; `docs/compiler/data-structures.md` §3.18):
//! one typed IR per body in columns (`ir`), and its wire form with
//! entry-local tables and the TIR hash (`wire`).

// Test-only work accounting: absent from the compiler used by benchmarks.
macro_rules! work {
    ($amount:expr) => {
        #[cfg(test)]
        crate::measurements::add($amount);
    };
}
#[cfg(test)]
mod measurements {
    use std::cell::Cell;
    thread_local! { static WORK: Cell<usize> = const { Cell::new(0) }; }
    pub(crate) fn add(n: usize) {
        WORK.with(|work| work.set(work.get() + n));
    }
    pub(crate) fn reset() {
        WORK.with(|work| work.set(0));
    }
    pub(crate) fn total() -> usize {
        WORK.with(Cell::get)
    }
}

pub mod inline;
pub mod ir;
pub mod own;
pub mod wire;
pub mod work;

pub use ir::{Body, BodyKind, Callee, ChoiceKind, PrimOp, Ref, Tag, TirBuilder, TirSink};
