//! hd_walk: the walking skeleton of the new compiler. A tiny hd subset goes
//! through every designed stage: parse, folder interface, resolution, body
//! checking to TIR, monomorphizing collection, Wasm GC emission and link,
//! under the serial scheduler and the in-memory cache.
//! See future-work/compiler/skeleton-findings.md.
#![forbid(unsafe_code)]

pub mod check;
pub mod driver;
pub mod iface;
pub mod mono;
pub mod sched;
pub mod tir;
pub mod wasm;
pub mod world;
