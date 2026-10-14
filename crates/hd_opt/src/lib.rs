//! `hd_opt`: the optimization pass manager. It holds the pass declarations,
//! the pipelines as data and the startup validator that hashes a pipeline.
//! It depends on `hd_mono`, never the reverse, and never on `hd_wasm`.

pub mod passes;
