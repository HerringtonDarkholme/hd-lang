#![forbid(unsafe_code)]
//! `hd_tir`: TIR (checking-and-tir.md §4.13.11; data-structures.md §3.18):
//! one typed IR per body in columns (`ir`), and its wire form with
//! entry-local tables and the TIR hash (`wire`).

pub mod ir;
pub mod wire;

pub use ir::{Body, BodyKind, Callee, ChoiceKind, PrimOp, Ref, Tag, TirBuilder, TirSink};
